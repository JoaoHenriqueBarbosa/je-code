//! Porte de `src/vs/platform/networkFilter/common/networkFilterService.ts`: o serviço que filtra as requisições
//! de rede feitas pelas ferramentas do agente (ferramenta de fetch, navegador integrado) pelas listas de domínios
//! permitidos e negados.
//!
//! Divergências:
//! - `_serviceBrand` não existe: o identificador do serviço é [`I_AGENT_NETWORK_FILTER_SERVICE`].
//! - `ISandboxNetworkRestrictions` é [`SandboxNetworkRestrictions`] e o parâmetro opcional é `Option<&_>`.
//! - `getValue<string[]>(...)` devolve um valor dinâmico: os itens que não são texto são descartados (o tipo do
//!   original só promete textos, e um item de outro tipo derrubaria o casamento de padrões).
//! - `createSandboxNetworkFilter` devolve um serviço que ignora o parâmetro `restrictions` das chamadas, como o
//!   objeto literal do original (as restrições são as dadas na criação).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::map::LRUCache;
use je_base::network::{UriOrString, matches_scheme, schemas};
use je_base::nls::localize;
use je_base::uri::URI;
use je_instantiation::service_id;

use super::domain_matcher::{extract_domain_from_uri, is_domain_allowed};
use super::settings::AgentNetworkDomainSettingId;
use crate::configuration::service::IConfigurationService;
use crate::sandbox::sandbox_settings_resolution_helper::SandboxNetworkRestrictions;

service_id!(pub I_AGENT_NETWORK_FILTER_SERVICE: dyn IAgentNetworkFilterService = "agentNetworkFilterService");

fn is_filtered_network_scheme(uri: &URI) -> bool {
    [schemas::HTTP, schemas::HTTPS, "ws", "wss"].into_iter().any(|scheme| matches_scheme(UriOrString::Uri(uri), scheme))
}

/// Serviço que filtra as requisições de rede feitas pelas ferramentas do agente (ferramenta de fetch,
/// navegador integrado) pelas listas de domínios permitidos e negados.
///
/// A filtragem vale para todos os chamadores quando a configuração `chat.agent.networkFilter` está ligada. As
/// chamadas do navegador do Copilot podem trazer restrições de sandbox já resolvidas. Com as duas listas vazias,
/// todo domínio é negado fora do sandbox. Um domínio da lista de negados é sempre bloqueado, mesmo que também
/// case com uma entrada da lista de permitidos.
pub trait IAgentNetworkFilterService {
    /// Extrai o domínio de uma URI e o confere com o filtro de permitidos e negados. URIs de arquivo e de
    /// esquemas não filtrados sem autoridade sempre passam. Devolve `true` se o domínio é permitido e `false`
    /// se está bloqueado.
    fn is_uri_allowed(&self, uri: &URI, restrictions: Option<&SandboxNetworkRestrictions>) -> bool;

    /// Se a filtragem de rede está ligada.
    fn is_enabled(&self, restrictions: Option<&SandboxNetworkRestrictions>) -> bool;

    /// Monta a mensagem de erro de uma URI bloqueada, segundo a configuração do filtro: explica que o acesso
    /// à URI está bloqueado por política.
    fn format_error(&self, uri: &URI) -> String;

    /// Dispara quando a configuração do filtro muda.
    fn on_did_change(&self) -> Event<()>;
}

/// A lista de textos de uma configuração (`getValue<string[]>(...) ?? []`).
fn configured_patterns(configuration_service: &dyn IConfigurationService, id: AgentNetworkDomainSettingId) -> Vec<String> {
    match configuration_service.get_value(Some(id.as_str()), None) {
        Dynamic::Array(items) => items
            .to_vec()
            .into_iter()
            .filter_map(|item| match item {
                Dynamic::String(text) => Some(text.to_string_lossy()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `AgentNetworkFilterService`.
pub struct AgentNetworkFilterService {
    configuration_service: Rc<dyn IConfigurationService>,
    network_filter_enabled: Cell<bool>,
    allowed_patterns: RefCell<Vec<String>>,
    denied_patterns: RefCell<Vec<String>>,
    domain_cache: LRUCache<String, bool>,
    disposables: Rc<DisposableStore>,
    on_did_change_emitter: Rc<Emitter<()>>,
}

impl AgentNetworkFilterService {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let service = Rc::new(Self {
            configuration_service: configuration_service.clone(),
            network_filter_enabled: Cell::new(false),
            allowed_patterns: RefCell::new(Vec::new()),
            denied_patterns: RefCell::new(Vec::new()),
            domain_cache: LRUCache::new(100, 1.0),
            on_did_change_emitter: disposables.add_rc(Emitter::new()),
            disposables,
        });
        service.read_configuration();

        let weak = Rc::downgrade(&service);
        service.disposables.add(configuration_service.on_did_change_configuration().listen(move |e| {
            if e.affects_configuration(AgentNetworkDomainSettingId::NetworkFilter.as_str(), None)
                || e.affects_configuration(AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str(), None)
                || e.affects_configuration(AgentNetworkDomainSettingId::DeniedNetworkDomains.as_str(), None)
            {
                if let Some(this) = weak.upgrade() {
                    this.read_configuration();
                    this.on_did_change_emitter.fire(());
                }
            }
        }));
        service
    }

    fn read_configuration(&self) {
        let enabled = self.configuration_service.get_value(Some(AgentNetworkDomainSettingId::NetworkFilter.as_str()), None);
        self.network_filter_enabled.set(enabled.is_truthy());
        *self.allowed_patterns.borrow_mut() = configured_patterns(&*self.configuration_service, AgentNetworkDomainSettingId::AllowedNetworkDomains);
        *self.denied_patterns.borrow_mut() = configured_patterns(&*self.configuration_service, AgentNetworkDomainSettingId::DeniedNetworkDomains);
        self.domain_cache.clear();
    }
}

impl IAgentNetworkFilterService for AgentNetworkFilterService {
    fn is_uri_allowed(&self, uri: &URI, restrictions: Option<&SandboxNetworkRestrictions>) -> bool {
        // When domain filtering is inactive, allow all requests.
        if !self.is_enabled(restrictions) {
            return true;
        }

        // File URIs and unfiltered schemes without authority always pass
        if matches_scheme(UriOrString::Uri(uri), schemas::FILE) || (uri.authority().is_empty() && !is_filtered_network_scheme(uri)) {
            return true;
        }

        if restrictions.is_some_and(|restrictions| restrictions.sandbox_enabled && !restrictions.allow_network) {
            return false;
        }

        let Some(domain) = extract_domain_from_uri(uri) else {
            return !is_filtered_network_scheme(uri);
        };

        if let Some(restrictions) = restrictions {
            let allowed = if restrictions.sandbox_enabled && restrictions.allowed_domains.is_empty() { vec!["*".to_owned()] } else { restrictions.allowed_domains.clone() };
            return is_domain_allowed(&domain, &allowed, &restrictions.denied_domains);
        }

        if let Some(cached) = self.domain_cache.get(&domain) {
            return cached;
        }
        let result = is_domain_allowed(&domain, self.allowed_patterns.borrow().as_slice(), self.denied_patterns.borrow().as_slice());
        self.domain_cache.set(domain, result);
        result
    }

    fn is_enabled(&self, restrictions: Option<&SandboxNetworkRestrictions>) -> bool {
        self.network_filter_enabled.get() || restrictions.is_some_and(|restrictions| restrictions.sandbox_enabled)
    }

    fn format_error(&self, uri: &URI) -> String {
        let domain = extract_domain_from_uri(uri).unwrap_or_else(|| uri.authority().to_string_lossy());
        localize(
            "networkFilter.blockedByPolicy",
            "Access to {0} is blocked by network domain policy (see `{1}` and `{2}` settings).",
            &[domain.as_str(), AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str(), AgentNetworkDomainSettingId::DeniedNetworkDomains.as_str()],
        )
    }

    fn on_did_change(&self) -> Event<()> {
        self.on_did_change_emitter.event()
    }
}

impl IDisposable for AgentNetworkFilterService {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// O filtro devolvido por [`create_sandbox_network_filter`]: repassa ao serviço as restrições dadas na criação.
struct SandboxNetworkFilter {
    service: Rc<dyn IAgentNetworkFilterService>,
    restrictions: SandboxNetworkRestrictions,
}

impl IAgentNetworkFilterService for SandboxNetworkFilter {
    fn is_uri_allowed(&self, uri: &URI, _restrictions: Option<&SandboxNetworkRestrictions>) -> bool {
        self.service.is_uri_allowed(uri, Some(&self.restrictions))
    }

    fn is_enabled(&self, _restrictions: Option<&SandboxNetworkRestrictions>) -> bool {
        self.service.is_enabled(Some(&self.restrictions))
    }

    fn format_error(&self, uri: &URI) -> String {
        self.service.format_error(uri)
    }

    fn on_did_change(&self) -> Event<()> {
        self.service.on_did_change()
    }
}

/// `createSandboxNetworkFilter`.
pub fn create_sandbox_network_filter(service: Rc<dyn IAgentNetworkFilterService>, restrictions: SandboxNetworkRestrictions) -> Rc<dyn IAgentNetworkFilterService> {
    Rc::new(SandboxNetworkFilter { service, restrictions })
}
