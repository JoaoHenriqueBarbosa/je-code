//! Porte de `src/vs/workbench/services/extensionManagement/electron-browser/extensionGalleryAccountService.ts`: a conta
//! do marketplace privado. Os dois provedores (`GitHubGalleryAccountProvider`, que decide o direito de acesso pela conta
//! padrão, e `MicrosoftGalleryAccountProvider`, que o decide localmente pela declaração de tenant do token), o serviço
//! que encaminha ao provedor configurado e a contribuição que cria o provedor.
//!
//! Divergências:
//! - `AbstractGalleryAccountProvider` (classe abstrata) é o trait [`AbstractGalleryAccountProvider`], que cada provedor
//!   implementa com `doGetAccount` e `signIn`, mais o estado compartilhado [`GalleryAccountProviderState`] (o estado da
//!   conta, os eventos, a telemetria). O `getAccount` que captura o erro e marca o estado `Unknown` é o único
//!   `impl` de [`IExtensionGalleryAccountProvider`], para qualquer tipo que implemente o trait abstrato.
//! - Os provedores são `Clone` (todos os campos são `Rc`): o corpo `async` de cada método leva uma cópia.
//! - As dependências por injeção entram nos construtores. `IAuthenticationService` e `IDefaultAccountService` são os
//!   contratos portados em `services::authentication::authentication` e `services::accounts::default_account`.
//! - A contribuição (`ExtensionGalleryAccountProviderContribution`) recebe, no lugar do `IInstantiationService`, as duas
//!   funções que criam os provedores (o `createInstance` de cada um). `registerSingleton` e
//!   `registerWorkbenchContribution2` não têm equivalente: quem monta o workbench cria o serviço e a contribuição.
//! - O item de escolha de conta do quick pick (`IAccountPickItem extends IQuickPickItem`, com `account?`) guarda a
//!   posição da conta na lista em `extra.accountIndex`, porque o `extra` do [`IQuickPickItem`] do porte é um valor
//!   dinâmico, que não carrega a conta. O item "Sign in with a Different Account" não tem `extra`.
//! - O JSON da conta preferida é lido e escrito com `serde_json` (`{"authProvider":..., "id":...}`), e uma conta que
//!   não é um objeto, sem `authProvider` igual ao do provedor ou sem `id` textual vale como "sem preferência".
//! - A mensagem de erro do token ilegível e a da falta de `accessScopes` trocam o travessão do original por hífen.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::default_account::IDefaultAccount;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::json::Value;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, MutableDisposable};
use je_base::nls::localize;
use je_base::oauth::get_claims_from_jwt;
use je_base::promise::Promise;
use je_platform::configuration::service::IConfigurationService;
use je_platform::extension_management::extension_gallery_manifest::EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};
use je_platform::product::IProductService;
use je_platform::quickinput::quick_input::{IPickOptions, IQuickInputService, IQuickItem, IQuickPickItem};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_storage::StorageValue;

use super::extension_gallery_account::{
    ExtensionGalleryAccountStatus, IExtensionGalleryAccount, IExtensionGalleryAccountProvider, IExtensionGalleryAccountService,
};
use crate::services::accounts::default_account::IDefaultAccountService;
use crate::services::authentication::authentication::{
    AuthenticationScopeListOrRequest, AuthenticationSession, AuthenticationSessionAccount, IAuthenticationCreateSessionOptions, IAuthenticationProviderSessionOptions,
    IAuthenticationService,
};

const PREFERRED_ACCOUNT_KEY: &str = "marketplace.account";

// Well-known MSA (personal account) tenant ids. Duplicated from the microsoft-authentication
// extension, which lives in the extension host and is not importable here.
const MSA_TENANT_ID: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";
const MSA_PASSTHROUGH_TENANT_ID: &str = "f8cdef31-a31e-4b4a-93e4-5f571e91255a";

/// `configurationService.getValue<string>(ExtensionGalleryAuthProviderConfigKey) === 'microsoft'`: o provedor de
/// autenticação que a implantação configurou (qualquer outro valor, inclusive `Microsoft` com outra caixa, é o GitHub).
pub fn uses_microsoft_auth_provider(configuration_service: &dyn IConfigurationService) -> bool {
    matches!(
        configuration_service.get_value(Some(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY), None),
        Dynamic::String(value) if value.to_string_lossy() == "microsoft"
    )
}

/// O estado que os provedores compartilham (o que a classe abstrata guarda): o estado da conta, os eventos e a
/// telemetria de elegibilidade.
pub struct GalleryAccountProviderState {
    base: Rc<Disposable>,
    on_did_change_account: Rc<Emitter<()>>,
    account_status: Cell<ExtensionGalleryAccountStatus>,
    on_did_change_account_status: Rc<Emitter<ExtensionGalleryAccountStatus>>,
    auth_provider_id: &'static str,
    telemetry_service: Rc<dyn ITelemetryService>,
    log_service: LogServiceRef,
}

impl GalleryAccountProviderState {
    fn new(auth_provider_id: &'static str, telemetry_service: Rc<dyn ITelemetryService>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_account = Emitter::new();
        base.register(on_did_change_account.clone());
        let on_did_change_account_status = Emitter::new();
        base.register(on_did_change_account_status.clone());
        Rc::new(Self { base, on_did_change_account, account_status: Cell::new(ExtensionGalleryAccountStatus::Unknown), on_did_change_account_status, auth_provider_id, telemetry_service, log_service })
    }

    fn set_account_status(&self, status: ExtensionGalleryAccountStatus) {
        if self.account_status.get() != status {
            self.account_status.set(status);
            self.on_did_change_account_status.fire(status);
        }
    }

    fn report_eligibility(&self, eligible: bool) {
        self.telemetry_service.public_log2(
            "marketplace:auth:checked",
            Some(Dynamic::object([("authProvider", Dynamic::string(self.auth_provider_id)), ("eligible", Dynamic::Bool(eligible))])),
        );
    }
}

/// `AbstractGalleryAccountProvider`: status bookkeeping and eligibility reporting shared by the provider
/// implementations.
pub trait AbstractGalleryAccountProvider {
    fn state(&self) -> &Rc<GalleryAccountProviderState>;

    fn do_get_account(&self) -> Promise<Option<IExtensionGalleryAccount>>;

    fn do_sign_in(&self) -> Promise<()>;
}

impl<T: AbstractGalleryAccountProvider> IExtensionGalleryAccountProvider for T {
    fn account_status(&self) -> ExtensionGalleryAccountStatus {
        self.state().account_status.get()
    }

    fn on_did_change_account_status(&self) -> Event<ExtensionGalleryAccountStatus> {
        self.state().on_did_change_account_status.event()
    }

    fn on_did_change_account(&self) -> Event<()> {
        self.state().on_did_change_account.event()
    }

    fn get_account(&self) -> Promise<Option<IExtensionGalleryAccount>> {
        let state = self.state().clone();
        self.do_get_account().then_result(move |result| match result {
            Ok(account) => Ok(account),
            Err(error) => {
                // Distinct from "no account" so the caller does not demand sign-in for a transient failure.
                state.log_service.error_args(&LogMessage::from("[Marketplace] Unable to resolve the marketplace account"), &[LogArg::from(&error)]);
                state.set_account_status(ExtensionGalleryAccountStatus::Unknown);
                Ok(None)
            }
        })
    }

    fn sign_in(&self) -> Promise<()> {
        self.do_sign_in()
    }
}

/// `GitHubGalleryAccountProvider`: entitlement from the default account's SKU or enterprise flag. No bearer is carried.
#[derive(Clone)]
pub struct GitHubGalleryAccountProvider {
    state: Rc<GalleryAccountProviderState>,
    default_account_service: Rc<dyn IDefaultAccountService>,
    product_service: Rc<dyn IProductService>,
}

impl GitHubGalleryAccountProvider {
    pub fn new(
        default_account_service: Rc<dyn IDefaultAccountService>,
        product_service: Rc<dyn IProductService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        log_service: LogServiceRef,
    ) -> Self {
        let state = GalleryAccountProviderState::new("github", telemetry_service, log_service);
        let on_did_change_account = state.on_did_change_account.clone();
        state.base.register(default_account_service.on_did_change_default_account().listen(move |_| on_did_change_account.fire(())));
        Self { state, default_account_service, product_service }
    }

    fn check_access(&self, account: &IDefaultAccount) -> bool {
        let sku = account.entitlements_data.as_ref().map(|data| data.access_type_sku.as_str());
        self.state.log_service.debug_args("[Marketplace] Checking Account SKU access for configured gallery", &[LogArg::Value(sku.map_or(Dynamic::Undefined, Dynamic::string))]);
        if let Some(sku) = sku.filter(|sku| !sku.is_empty()) {
            let allowed = self.product_service.extensions_gallery().and_then(|gallery| gallery.access_skus.as_ref()).is_some_and(|skus| skus.iter().any(|candidate| candidate == sku));
            if allowed {
                self.state.log_service.debug("[Marketplace] Account has access to configured gallery");
                return true;
            }
        }
        self.state.log_service.debug_args("[Marketplace] Checking enterprise account access for configured gallery", &[LogArg::Value(Dynamic::Bool(account.enterprise))]);
        account.enterprise
    }
}

impl AbstractGalleryAccountProvider for GitHubGalleryAccountProvider {
    fn state(&self) -> &Rc<GalleryAccountProviderState> {
        &self.state
    }

    fn do_get_account(&self) -> Promise<Option<IExtensionGalleryAccount>> {
        let this = self.clone();
        async_fn(async move {
            let Some(account) = this.default_account_service.get_default_account().await? else {
                this.state.set_account_status(ExtensionGalleryAccountStatus::SignedOut);
                return Ok(None);
            };
            let eligible = this.check_access(&account);
            this.state.report_eligibility(eligible);
            this.state.set_account_status(if eligible { ExtensionGalleryAccountStatus::Eligible } else { ExtensionGalleryAccountStatus::Ineligible });
            // A result is returned even when ineligible, so the caller can tell "signed in but denied"
            // apart from "no account": the two map to different statuses.
            Ok(Some(IExtensionGalleryAccount::default()))
        })
    }

    fn do_sign_in(&self) -> Promise<()> {
        self.default_account_service.sign_in(None).then(|_| ())
    }
}

impl IDisposable for GitHubGalleryAccountProvider {
    fn dispose(&self) {
        self.state.base.dispose();
    }
}

/// `MicrosoftGalleryAccountProvider`: entitlement decided locally from the token's tenant claim. The bearer travels
/// with it.
#[derive(Clone)]
pub struct MicrosoftGalleryAccountProvider {
    state: Rc<GalleryAccountProviderState>,
    authentication_service: Rc<dyn IAuthenticationService>,
    quick_input_service: Rc<dyn IQuickInputService>,
    product_service: Rc<dyn IProductService>,
    storage_service: Rc<dyn StorageService>,
}

impl MicrosoftGalleryAccountProvider {
    pub fn new(
        authentication_service: Rc<dyn IAuthenticationService>,
        quick_input_service: Rc<dyn IQuickInputService>,
        product_service: Rc<dyn IProductService>,
        storage_service: Rc<dyn StorageService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        log_service: LogServiceRef,
    ) -> Self {
        let state = GalleryAccountProviderState::new("microsoft", telemetry_service, log_service);
        let on_did_change_account = state.on_did_change_account.clone();
        state.base.register(authentication_service.on_did_change_sessions().listen(move |change| {
            if change.provider_id == "microsoft" {
                on_did_change_account.fire(());
            }
        }));
        Self { state, authentication_service, quick_input_service, product_service, storage_service }
    }

    /// Session lookup and interactive sign-in must request the same scopes, or the session created
    /// by one is invisible to the other. Absent when the deployment did not configure them.
    fn scopes(&self) -> Option<Vec<String>> {
        self.product_service.extensions_gallery().and_then(|gallery| gallery.access_scopes.clone())
    }

    /// Work/school tenant is eligible, personal is not. Fails closed on an unreadable token.
    fn is_eligible(&self, session: &AuthenticationSession) -> bool {
        let raw_token = session.id_token.as_deref().unwrap_or(&session.access_token);
        let tid = match get_claims_from_jwt(raw_token) {
            Ok(Value::Object(claims)) => claims.get("tid").cloned(),
            // Um array não tem `tid`.
            Ok(Value::Array(_)) => None,
            // `null.tid` lança `TypeError`.
            Ok(_) => {
                self.log_unreadable_token(&je_base::errors::Error::named("TypeError", "Cannot read properties of null (reading 'tid')"));
                return false;
            }
            Err(error) => {
                self.log_unreadable_token(&error);
                return false;
            }
        };
        match tid {
            None | Some(Value::Null) | Some(Value::Bool(false)) => false,
            Some(Value::String(tid)) => {
                let tid = tid.to_string_lossy();
                !tid.is_empty() && tid != MSA_TENANT_ID && tid != MSA_PASSTHROUGH_TENANT_ID
            }
            Some(Value::Number(number)) if number == 0.0 || number.is_nan() => false,
            Some(_) => true,
        }
    }

    fn log_unreadable_token(&self, error: &je_base::errors::Error) {
        self.state.log_service.error_args(
            &LogMessage::from("[Marketplace] Unable to decode the Microsoft token to determine account eligibility - treating as ineligible."),
            &[LogArg::from(error)],
        );
    }

    /// Anchored to the remembered account rather than an arbitrary `sessions[0]`. Several accounts
    /// with no preference returns `None` rather than guessing. Never prompts.
    fn get_session(&self) -> Promise<Option<AuthenticationSession>> {
        let this = self.clone();
        async_fn(async move {
            let Some(scopes) = this.scopes() else {
                this.state.log_service.error("[Marketplace] extensionsGallery.accessScopes is not configured - the Microsoft marketplace path cannot request a session.");
                return Ok(None);
            };
            let sessions = this.authentication_service.get_sessions("microsoft", Some(AuthenticationScopeListOrRequest::Scopes(scopes)), None, None).await?;
            if sessions.is_empty() {
                return Ok(None);
            }
            if let Some(preferred_id) = this.read_preferred_account_id() {
                // Fall through rather than silently switching accounts.
                if let Some(remembered) = sessions.iter().find(|session| session.account.id == preferred_id) {
                    return Ok(Some(remembered.clone()));
                }
            }
            if sessions.len() == 1 {
                this.store_preferred_account_id(&sessions[0].account.id);
                return Ok(Some(sessions[0].clone()));
            }
            Ok(None)
        })
    }

    /// Passing a known account binds to it without a fresh interactive login; omitting it falls
    /// back to interactive sign-in, which is also how the user adds a different account.
    fn choose_account(&self, scopes: &[String], account: Option<&AuthenticationSessionAccount>) -> Promise<()> {
        // Persist before creating the session so re-validation already sees the grounded account.
        if let Some(account) = account {
            self.store_preferred_account_id(&account.id);
        }
        let this = self.clone();
        let options = account.map(|account| IAuthenticationCreateSessionOptions {
            session_options: IAuthenticationProviderSessionOptions { account: Some(account.clone()), ..Default::default() },
            activate_immediate: None,
        });
        self.authentication_service
            .create_session("microsoft", AuthenticationScopeListOrRequest::Scopes(scopes.to_vec()), options)
            .then(move |session| this.store_preferred_account_id(&session.account.id))
    }

    fn read_preferred_account_id(&self) -> Option<String> {
        let raw = self.storage_service.get(PREFERRED_ACCOUNT_KEY, StorageScope::Application, None).filter(|raw| !raw.is_empty())?;
        let parsed: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(parsed) => parsed,
            Err(_) => {
                self.storage_service.remove(PREFERRED_ACCOUNT_KEY, StorageScope::Application);
                return None;
            }
        };
        let candidate = parsed.as_object()?;
        if candidate.get("authProvider").and_then(|auth_provider| auth_provider.as_str()) != Some(self.state.auth_provider_id) {
            return None;
        }
        candidate.get("id").and_then(|id| id.as_str()).map(str::to_owned)
    }

    fn store_preferred_account_id(&self, account_id: &str) {
        let preferred = serde_json::json!({ "authProvider": self.state.auth_provider_id, "id": account_id }).to_string();
        self.storage_service.store(PREFERRED_ACCOUNT_KEY, StorageValue::from(preferred.as_str()), StorageScope::Application, StorageTarget::Machine);
    }
}

/// A conta que o item escolhido do quick pick guarda em `extra.accountIndex` (a posição na lista), se guarda.
fn picked_account<'a>(pick: &IQuickPickItem, accounts: &'a [AuthenticationSessionAccount]) -> Option<&'a AuthenticationSessionAccount> {
    match &pick.extra {
        Dynamic::Object(_) => match pick.extra.get_str("accountIndex") {
            Dynamic::Number(index) => accounts.get(index as usize),
            _ => None,
        },
        _ => None,
    }
}

impl AbstractGalleryAccountProvider for MicrosoftGalleryAccountProvider {
    fn state(&self) -> &Rc<GalleryAccountProviderState> {
        &self.state
    }

    fn do_get_account(&self) -> Promise<Option<IExtensionGalleryAccount>> {
        let this = self.clone();
        async_fn(async move {
            let Some(session) = this.get_session().await? else {
                this.state.set_account_status(ExtensionGalleryAccountStatus::SignedOut);
                return Ok(None);
            };
            let eligible = this.is_eligible(&session);
            this.state.report_eligibility(eligible);
            this.state.set_account_status(if eligible { ExtensionGalleryAccountStatus::Eligible } else { ExtensionGalleryAccountStatus::Ineligible });
            // The bearer is withheld when ineligible: that identity must never reach the marketplace.
            Ok(Some(IExtensionGalleryAccount { access_token: if eligible { Some(session.access_token.clone()) } else { None } }))
        })
    }

    fn do_sign_in(&self) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let Some(scopes) = this.scopes() else {
                this.state.log_service.error("[Marketplace] extensionsGallery.accessScopes is not configured - cannot sign in to the Microsoft marketplace path.");
                return Ok(());
            };

            let accounts = this.authentication_service.get_accounts("microsoft").await?;
            if accounts.len() <= 1 {
                this.choose_account(&scopes, accounts.first()).await?;
                return Ok(());
            }

            let mut picks: Vec<IQuickPickItem> = accounts
                .iter()
                .enumerate()
                .map(|(index, account)| IQuickPickItem {
                    base: IQuickItem { label: JsString::from(account.label.as_str()), ..IQuickItem::default() },
                    extra: Dynamic::object([("accountIndex", Dynamic::Number(index as f64))]),
                    ..IQuickPickItem::default()
                })
                .collect();
            picks.push(IQuickPickItem {
                base: IQuickItem { label: JsString::from(localize("marketplace.signInDifferentAccount", "Sign in with a Different Account…", &[]).as_str()), ..IQuickItem::default() },
                extra: Dynamic::Undefined,
                ..IQuickPickItem::default()
            });

            let options = IPickOptions {
                place_holder: Some(localize("marketplace.pickAccount", "Select the account to use for the Extensions Marketplace", &[])),
                ..IPickOptions::default()
            };
            let Some(pick) = this.quick_input_service.pick_items(picks, Some(&options)).await? else {
                return Ok(()); // cancelled
            };
            this.choose_account(&scopes, picked_account(&pick, &accounts)).await?;
            Ok(())
        })
    }
}

impl IDisposable for MicrosoftGalleryAccountProvider {
    fn dispose(&self) {
        self.state.base.dispose();
    }
}

/// `ExtensionGalleryAccountService`: forwards to whichever provider the deployment configured. Holds no
/// authentication dependency of its own, so it can sit in the service graph that authentication itself depends on.
pub struct ExtensionGalleryAccountService {
    base: Rc<Disposable>,
    provider: RefCell<Option<Rc<dyn IExtensionGalleryAccountProvider>>>,
    provider_listeners: Rc<MutableDisposable<DisposableStore>>,
    on_did_change_account: Rc<Emitter<()>>,
    on_did_change_account_status: Rc<Emitter<ExtensionGalleryAccountStatus>>,
}

impl ExtensionGalleryAccountService {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let provider_listeners = MutableDisposable::<DisposableStore>::new();
        base.register(provider_listeners.clone());
        let on_did_change_account = Emitter::new();
        base.register(on_did_change_account.clone());
        let on_did_change_account_status = Emitter::new();
        base.register(on_did_change_account_status.clone());
        Rc::new(Self { base, provider: RefCell::new(None), provider_listeners, on_did_change_account, on_did_change_account_status })
    }

    fn provider(&self) -> Option<Rc<dyn IExtensionGalleryAccountProvider>> {
        self.provider.borrow().clone()
    }
}

impl IDisposable for ExtensionGalleryAccountService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IExtensionGalleryAccountService for ExtensionGalleryAccountService {
    fn account_status(&self) -> ExtensionGalleryAccountStatus {
        self.provider().map_or(ExtensionGalleryAccountStatus::Unknown, |provider| provider.account_status())
    }

    fn on_did_change_account_status(&self) -> Event<ExtensionGalleryAccountStatus> {
        self.on_did_change_account_status.event()
    }

    fn on_did_change_account(&self) -> Event<()> {
        self.on_did_change_account.event()
    }

    fn get_account(&self) -> Promise<Option<IExtensionGalleryAccount>> {
        match self.provider() {
            Some(provider) => provider.get_account(),
            None => Promise::resolved(None),
        }
    }

    fn sign_in(&self) -> Promise<()> {
        match self.provider() {
            Some(provider) => provider.sign_in(),
            None => Promise::resolved(()),
        }
    }

    fn set_account_provider(&self, provider: Rc<dyn IExtensionGalleryAccountProvider>) {
        *self.provider.borrow_mut() = Some(provider.clone());
        let listeners = DisposableStore::new();
        let on_did_change_account = self.on_did_change_account.clone();
        listeners.add(provider.on_did_change_account().listen(move |_| on_did_change_account.fire(())));
        let on_did_change_account_status = self.on_did_change_account_status.clone();
        listeners.add(provider.on_did_change_account_status().listen(move |status| on_did_change_account_status.fire(*status)));
        self.provider_listeners.set(Some(listeners));
        // Anything that resolved before the provider arrived saw no account; let it try again.
        self.on_did_change_account.fire(());
    }
}

/// `ExtensionGalleryAccountProviderContribution`: creates the configured provider and hands it to the service. Lives
/// outside the core service graph, so it can depend on authentication without forming the cycle the service must avoid.
pub struct ExtensionGalleryAccountProviderContribution {
    base: Rc<Disposable>,
}

impl ExtensionGalleryAccountProviderContribution {
    pub const ID: &'static str = "workbench.contrib.extensionGalleryAccountProvider";

    /// `create_github_provider` e `create_microsoft_provider` são os `instantiationService.createInstance` de cada provedor.
    pub fn new(
        configuration_service: &dyn IConfigurationService,
        account_service: &dyn IExtensionGalleryAccountService,
        create_github_provider: impl FnOnce() -> GitHubGalleryAccountProvider,
        create_microsoft_provider: impl FnOnce() -> MicrosoftGalleryAccountProvider,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let provider: Rc<dyn IExtensionGalleryAccountProvider> = if uses_microsoft_auth_provider(configuration_service) {
            let provider = Rc::new(create_microsoft_provider());
            base.register(provider.clone());
            provider
        } else {
            let provider = Rc::new(create_github_provider());
            base.register(provider.clone());
            provider
        };
        account_service.set_account_provider(provider);
        Rc::new(Self { base })
    }
}

impl IDisposable for ExtensionGalleryAccountProviderContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}
