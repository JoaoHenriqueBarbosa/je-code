//! Porte de `src/vs/workbench/services/accounts/common/githubEnterprise.ts`: a configuração de enrolamento das
//! instâncias do GitHub Enterprise, a validação do endereço de uma instância, a leitura do texto digitado para
//! o Copilot e a base de uma sessão derivada do emissor OAuth.
//!
//! Decisões de tradução:
//! - **`IConfigurationService`**: o `inspect` devolve o `IConfigurationValue` do original, que é o trait
//!   [`ConfigurationValue`] (um objeto com as camadas); as funções recebem o serviço como `&dyn IConfigurationService`
//!   (ou `Rc<dyn IConfigurationService>`), sem recorte próprio.
//! - **Valores dinâmicos**: a lista de instâncias é lida como [`Dynamic`] e validada como o original
//!   (`Array.isArray` e `typeof uri === 'string'`); o erro de validação é o `Err` de quem lança.
//! - **Funções `async`**: o `throw` dentro de `addGitHubEnterpriseUri` é uma promessa rejeitada.
//! - **Parâmetros com valor padrão** (`legacySetting = 'github-enterprise.uri'`) e opcionais (`replacedUri?`) são
//!   `Option`, onde `None` é o `undefined` que aciona o padrão.
//! - **`new URL(...)`** é a crate `url` pelo módulo WHATWG de `je_platform` ([`try_new_url`]); onde o `URL`
//!   lançaria, a função de endereço responde "não é uma instância", que é o que os dois chamadores fazem no `catch`.
//!   `URI.parse(value, true)` lança no porte por [`errors::throw`], capturado por [`errors::catch`].
//! - **`GheParseResult`** (união discriminada por `kind`, com `GheParseResultKind` como `const enum` de textos) é
//!   um `enum` com as variantes, mais [`GheParseResultKind`] e [`GheParseResult::kind`].
//! - **Expressões regulares** de `parseGheInstanceInput`: são conferências sobre texto ASCII ([`is_ghe_host`]),
//!   porque as classes são `[a-zA-Z0-9-]` e a bandeira `i` só dobra a caixa ASCII sem a bandeira `u`.

use std::rc::Rc;

use je_base::JsString;
use je_base::arrays::equals;
use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::js_string::trim;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget, ConfigurationValue, is_configured};
use je_platform::url::whatwg::{hostname, try_new_url};

pub const GITHUB_ENTERPRISE_URIS_SETTING: &str = "github-enterprise.uris";

/// O valor padrão de `legacySetting`.
pub const LEGACY_GITHUB_ENTERPRISE_URI_SETTING: &str = "github-enterprise.uri";

/// `{ ...inspected, workspaceValue: undefined, workspaceFolderValue: undefined }`: as camadas do usuário de um
/// valor lido, sem as do workspace (que um workspace não confiável não pode impor).
struct WithoutWorkspaceValues<'a>(&'a dyn ConfigurationValue);

impl ConfigurationValue for WithoutWorkspaceValues<'_> {
    fn default_value(&self) -> Dynamic {
        self.0.default_value()
    }

    fn application_value(&self) -> Dynamic {
        self.0.application_value()
    }

    fn user_value(&self) -> Dynamic {
        self.0.user_value()
    }

    fn user_local_value(&self) -> Dynamic {
        self.0.user_local_value()
    }

    fn user_remote_value(&self) -> Dynamic {
        self.0.user_remote_value()
    }

    fn workspace_value(&self) -> Dynamic {
        Dynamic::Undefined
    }

    fn workspace_folder_value(&self) -> Dynamic {
        Dynamic::Undefined
    }

    fn memory_value(&self) -> Dynamic {
        self.0.memory_value()
    }

    fn policy_value(&self) -> Dynamic {
        self.0.policy_value()
    }

    fn value(&self) -> Dynamic {
        self.0.value()
    }

    fn override_identifiers(&self) -> Option<Vec<JsString>> {
        self.0.override_identifiers()
    }
}

/// Reads enrollment configuration, never the destination for an authenticated request.
pub fn get_configured_github_enterprise_uris(
    configuration_service: &dyn IConfigurationService,
    is_workspace_trusted: bool,
    legacy_setting: Option<&str>,
) -> Result<Vec<JsString>, Error> {
    let legacy_setting = legacy_setting.unwrap_or(LEGACY_GITHUB_ENTERPRISE_URI_SETTING);
    let inspected = configuration_service.inspect(GITHUB_ENTERPRISE_URIS_SETTING, &ConfigurationOverrides::default());
    let configured = if is_workspace_trusted { is_configured(&*inspected) } else { is_configured(&WithoutWorkspaceValues(&*inspected)) };
    let legacy = configuration_service.get_value(Some(legacy_setting), None);
    let uris = if configured {
        configuration_service.get_value(Some(GITHUB_ENTERPRISE_URIS_SETTING), None)
    } else if legacy.is_truthy() {
        Dynamic::array([legacy])
    } else {
        Dynamic::array(Vec::new())
    };
    let strings = match uris {
        Dynamic::Array(items) => items
            .to_vec()
            .into_iter()
            .map(|uri| match uri {
                Dynamic::String(text) => Some(text),
                _ => None,
            })
            .collect::<Option<Vec<JsString>>>(),
        _ => None,
    };
    strings.ok_or_else(|| Error::new(localize("invalidGitHubEnterpriseUris", "GitHub Enterprise URIs must be an array of instance URLs.", &[])))
}

/// `addGitHubEnterpriseUri`: acrescenta `uri` à lista de instâncias (trocando `replaced_uri`, se houver) sem
/// sobrescrever a lista efetiva nem a configuração legada. Rejeita um endereço que não é de instância.
pub fn add_github_enterprise_uri(
    configuration_service: Rc<dyn IConfigurationService>,
    uri: &str,
    is_workspace_trusted: bool,
    legacy_setting: Option<&str>,
    replaced_uri: Option<&str>,
) -> Promise<()> {
    let uri = JsString::from(uri);
    let legacy_setting = legacy_setting.map(str::to_owned);
    let replaced_uri = replaced_uri.map(JsString::from);
    async_fn(async move {
        if !is_valid_github_enterprise_uri(&uri.to_string_lossy()) {
            return Err(Error::new(localize("invalidGitHubEnterpriseUri", "Invalid GitHub Enterprise instance URI: {0}", &[&uri.to_string_lossy()])));
        }
        let uris = get_configured_github_enterprise_uris(&*configuration_service, is_workspace_trusted, legacy_setting.as_deref())?;
        let mut updated: Vec<JsString> = uris.iter().filter(|value| replaced_uri.as_ref() != Some(*value)).cloned().collect();
        if !updated.contains(&uri) {
            updated.push(uri);
        }
        if !equals(Some(&uris), Some(&updated), |one, other| one == other) {
            let target = if is_workspace_trusted
                && !configuration_service.inspect(GITHUB_ENTERPRISE_URIS_SETTING, &ConfigurationOverrides::default()).workspace_value().is_undefined()
            {
                ConfigurationTarget::Workspace
            } else {
                ConfigurationTarget::User
            };
            configuration_service
                .update_value(GITHUB_ENTERPRISE_URIS_SETTING, Dynamic::array(updated.into_iter().map(Dynamic::String)), None, Some(target))
                .await?;
        }
        Ok(())
    })
}

/// `isGitHubEnterpriseEndpoint`: `http` ou `https`, fora do GitHub público, sem credenciais, consulta nem
/// fragmento, e com um caminho sem `//` nem segmentos `.` e `..`.
fn is_github_enterprise_endpoint(uri: &URI) -> bool {
    let Some(url) = try_new_url(&uri.to_string(false)) else {
        return false;
    };
    let host = hostname(&url).to_string_lossy();
    let host = host.strip_suffix('.').unwrap_or(&host);
    let path = uri.path().to_string_lossy();
    matches!(url.scheme(), "http" | "https")
        && !matches!(host, "github.com" | "www.github.com" | "api.github.com")
        && url.username().is_empty()
        && url.password().is_none_or(str::is_empty)
        && url.query().is_none_or(str::is_empty)
        && url.fragment().is_none_or(str::is_empty)
        && !path.contains("//")
        && !path.split('/').any(|part| part == "." || part == "..")
}

/// `isValidGitHubEnterpriseUri`: o endereço (com `URI.parse` estrito) tem autoridade e, sem as barras finais do
/// caminho, é um endpoint de instância.
pub fn is_valid_github_enterprise_uri(value: &str) -> bool {
    errors::catch(|| {
        let uri = URI::parse(value, true);
        let units = uri.path().units();
        // `uri.path.replace(/\/+$/, '')`
        let end = units.iter().rposition(|&unit| unit != u16::from(b'/')).map_or(0, |last| last + 1);
        let without_trailing_slashes = JsString::from_units(units[..end].to_vec());
        !uri.authority().is_empty() && is_github_enterprise_endpoint(&uri.with(&UriChange { path: Field::Set(without_trailing_slashes), ..Default::default() }))
    })
    .unwrap_or(false)
}

/// `GheParseResultKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GheParseResultKind {
    Empty,
    SingleWord,
    FullUri,
    Invalid,
}

impl GheParseResultKind {
    /// O texto do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::SingleWord => "singleWord",
            Self::FullUri => "fullUri",
            Self::Invalid => "invalid",
        }
    }
}

/// `GheParseResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GheParseResult {
    Empty,
    SingleWord { resolved_uri: String },
    FullUri { resolved_uri: String },
    Invalid,
}

impl GheParseResult {
    /// O `kind` da união do original.
    pub fn kind(&self) -> GheParseResultKind {
        match self {
            Self::Empty => GheParseResultKind::Empty,
            Self::SingleWord { .. } => GheParseResultKind::SingleWord,
            Self::FullUri { .. } => GheParseResultKind::FullUri,
            Self::Invalid => GheParseResultKind::Invalid,
        }
    }
}

/// `/^[a-zA-Z0-9-]+$/`.
fn is_instance_label(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// `/^(?:[a-zA-Z0-9-]+\.)+ghe\.com\/?$/i`.
fn is_ghe_host(text: &str) -> bool {
    const SUFFIX: &str = "ghe.com";
    let host = text.strip_suffix('/').unwrap_or(text);
    let Some(labels_end) = host.len().checked_sub(SUFFIX.len()) else {
        return false;
    };
    let (Some(labels), Some(suffix)) = (host.get(..labels_end), host.get(labels_end..)) else {
        return false;
    };
    suffix.eq_ignore_ascii_case(SUFFIX) && labels.strip_suffix('.').is_some_and(|labels| labels.split('.').all(is_instance_label))
}

/// O texto depois de `https://` (a caixa não conta), se ele começa assim.
fn strip_https_scheme(text: &str) -> Option<&str> {
    const SCHEME: &str = "https://";
    let (scheme, rest) = (text.get(..SCHEME.len())?, text.get(SCHEME.len()..)?);
    scheme.eq_ignore_ascii_case(SCHEME).then_some(rest)
}

/// Accepts a GHE.com instance name or HTTPS URL for Copilot enrollment.
pub fn parse_ghe_instance_input(value: &str) -> GheParseResult {
    let units: Vec<u16> = value.encode_utf16().collect();
    let trimmed = String::from_utf16_lossy(trim(&units));
    if trimmed.is_empty() {
        return GheParseResult::Empty;
    }
    if is_instance_label(&trimmed) {
        return GheParseResult::SingleWord { resolved_uri: format!("https://{trimmed}.ghe.com") };
    }
    let resolved_uri = if is_ghe_host(&trimmed) { format!("https://{trimmed}") } else { trimmed };
    if strip_https_scheme(&resolved_uri).is_some_and(is_ghe_host) { GheParseResult::FullUri { resolved_uri } } else { GheParseResult::Invalid }
}

/// Returns the enterprise base identified by a session's OAuth issuer, never by configuration.
pub fn get_github_enterprise_uri(authorization_server: Option<&URI>) -> Option<URI> {
    let authorization_server = authorization_server?;
    errors::catch(|| {
        let path = authorization_server.path();
        let suffix: Vec<u16> = "/login/oauth".encode_utf16().collect();
        if !is_github_enterprise_endpoint(authorization_server) || !path.units().ends_with(&suffix) {
            return None;
        }
        Some(authorization_server.with(&UriChange {
            scheme: Field::Set(authorization_server.scheme().to_lower_case()),
            authority: Field::Set(authorization_server.authority().to_lower_case()),
            path: Field::Set(path.slice(0, path.len() - suffix.len())),
            ..Default::default()
        }))
    })
    .ok()
    .flatten()
}
