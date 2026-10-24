//! Porte de `src/vs/platform/defaultAccount/common/defaultAccount.ts` (os contratos `IDefaultAccountProvider` e
//! `IDefaultAccountService`) e da classe `DefaultAccountService` de
//! `src/vs/workbench/services/accounts/browser/defaultAccount.ts`.
//!
//! Decisões de tradução:
//! - **`ManagedSettingsFetchStatus`** (`number | 'ok' | 'no-url' | 'no-response' | 'parse-error' | null`) é o
//!   `enum` [`ManagedSettingsFetchStatus`], e o `null` ("nunca buscou") é o `None` do `Option` que os acessores
//!   devolvem.
//! - **`managedSettingsRawResponse: unknown`** é um [`Dynamic`] (`null` quando nunca buscou).
//! - **`signIn(options?: { additionalScopes?; [key: string]: unknown })`** é [`ISignInOptions`]: os escopos extras e o
//!   restante das opções como um objeto dinâmico.
//! - **`setDefaultAccountProvider`**, que lança quando já há um provedor, devolve `Result<(), Error>`.
//! - **`new DefaultAccountService(productService)`** lê só `productService.defaultChatAgent.provider.default`
//!   (o provedor padrão usado enquanto não há um provedor de conta); o porte recebe esse [`IChatProvider`] direto.
//! - **`@IDefaultAccountService` do contrato reduzido** de `extension_management::service_contracts` (só
//!   `getDefaultAccountAuthenticationProvider`) continua como está: o [`DefaultAccountService`] também o implementa.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::{Barrier, async_fn};
use je_base::default_account::{ICopilotTokenInfo, IDefaultAccount, IDefaultAccountAuthenticationProvider, IPolicyData};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::product::IChatProvider;
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::policy::managed_settings_freshness::{IManagedSettingsFreshness, ManagedSettingsFreshnessState};

use super::managed_settings::IManagedSettingsCompatibilityError;
use crate::services::extension_management::service_contracts::{self as reduced, IDefaultAccountService as IReducedDefaultAccountService};

/// `GitHubPaths.copilotSettings`.
pub const GITHUB_PATH_COPILOT_SETTINGS: &str = "settings/copilot/features";
/// `GitHubPaths.billingBudgets`.
pub const GITHUB_PATH_BILLING_BUDGETS: &str = "settings/copilot/features?utm_source=vscode";
/// `GitHubPaths.copilotUpgrade`.
pub const GITHUB_PATH_COPILOT_UPGRADE: &str = "github-copilot/upgrade?utm_source=vscode";

/// Outcome of the last `/copilot_internal/managed_settings` fetch.
/// - [`Http`](Self::Http): the server responded with that status code.
/// - [`Ok`](Self::Ok): response parsed and adapted successfully (including an empty `{}` body).
/// - [`NoUrl`](Self::NoUrl): no `managedSettingsUrl` configured in product.json.
/// - [`NoResponse`](Self::NoResponse): network error, all sessions rejected, or active rate-limit backoff.
/// - [`ParseError`](Self::ParseError): response received but JSON parsing failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedSettingsFetchStatus {
    Http(u32),
    Ok,
    NoUrl,
    NoResponse,
    ParseError,
}

/// `IDefaultAccountRefreshOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IDefaultAccountRefreshOptions {
    pub force_refresh: Option<bool>,
    /// Refreshes entitlement data even when its cache is fresh.
    pub refresh_entitlements: Option<bool>,
    /// Allows an explicit user action to retry managed settings after a failed attempt.
    pub retry_managed_settings: Option<bool>,
}

/// `{ additionalScopes?: readonly string[]; [key: string]: unknown }`, as opções de `signIn`.
#[derive(Clone, Debug, Default)]
pub struct ISignInOptions {
    pub additional_scopes: Option<Vec<String>>,
    /// As demais chaves (`[key: string]: unknown`), repassadas à criação da sessão.
    pub session_options: Option<Dynamic>,
}

/// `IDefaultAccountProvider`.
pub trait IDefaultAccountProvider {
    fn default_account(&self) -> Option<IDefaultAccount>;
    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>>;
    fn policy_data(&self) -> Option<IPolicyData>;
    fn on_did_change_policy_data(&self) -> Event<Option<IPolicyData>>;
    fn copilot_token_info(&self) -> Option<ICopilotTokenInfo>;
    fn on_did_change_copilot_token_info(&self) -> Event<Option<ICopilotTokenInfo>>;
    fn managed_settings_fetch_status(&self) -> Option<ManagedSettingsFetchStatus>;
    /// Timestamp (ms) of the last managed-settings fetch, or `None` if never fetched.
    fn managed_settings_fetched_at(&self) -> Option<f64>;
    /// The raw JSON response from the managed-settings endpoint, for diagnostics.
    fn managed_settings_raw_response(&self) -> Dynamic;
    fn managed_settings_compatibility_error(&self) -> Option<IManagedSettingsCompatibilityError>;
    fn on_did_change_managed_settings_compatibility_error(&self) -> Event<Option<IManagedSettingsCompatibilityError>>;
    fn managed_settings_freshness(&self) -> IManagedSettingsFreshness;
    fn on_did_change_managed_settings_freshness(&self) -> Event<IManagedSettingsFreshness>;
    fn get_default_account_authentication_provider(&self) -> IDefaultAccountAuthenticationProvider;

    /// Resolves a GitHub path against the selected session's enterprise server, or `https://github.com` for a
    /// non-enterprise provider. Returns `None` when enterprise authentication is selected but the session's
    /// server is unavailable.
    fn resolve_git_hub_url(&self, path: &str) -> Option<String>;

    fn refresh(&self, options: Option<IDefaultAccountRefreshOptions>) -> Promise<Option<IDefaultAccount>>;
    fn sign_in(&self, options: Option<ISignInOptions>) -> Promise<Option<IDefaultAccount>>;
    fn sign_out(&self) -> Promise<()>;
}

/// `IDefaultAccountService`.
pub trait IDefaultAccountService {
    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>>;
    fn on_did_change_policy_data(&self) -> Event<Option<IPolicyData>>;
    fn policy_data(&self) -> Option<IPolicyData>;
    fn current_default_account(&self) -> Option<IDefaultAccount>;
    fn copilot_token_info(&self) -> Option<ICopilotTokenInfo>;
    fn on_did_change_copilot_token_info(&self) -> Event<Option<ICopilotTokenInfo>>;
    fn managed_settings_fetch_status(&self) -> Option<ManagedSettingsFetchStatus>;
    /// Timestamp (ms) of the last managed-settings fetch, or `None` if never fetched.
    fn managed_settings_fetched_at(&self) -> Option<f64>;
    /// The raw JSON response from the managed-settings endpoint, for diagnostics.
    fn managed_settings_raw_response(&self) -> Dynamic;
    fn managed_settings_compatibility_error(&self) -> Option<IManagedSettingsCompatibilityError>;
    fn on_did_change_managed_settings_compatibility_error(&self) -> Event<Option<IManagedSettingsCompatibilityError>>;
    fn managed_settings_freshness(&self) -> IManagedSettingsFreshness;
    fn on_did_change_managed_settings_freshness(&self) -> Event<IManagedSettingsFreshness>;
    fn get_default_account(&self) -> Promise<Option<IDefaultAccount>>;
    fn get_default_account_authentication_provider(&self) -> IDefaultAccountAuthenticationProvider;
    fn set_default_account_provider(&self, provider: Rc<dyn IDefaultAccountProvider>) -> Result<(), Error>;
    fn refresh(&self, options: Option<IDefaultAccountRefreshOptions>) -> Promise<Option<IDefaultAccount>>;
    fn sign_in(&self, options: Option<ISignInOptions>) -> Promise<Option<IDefaultAccount>>;
    fn sign_out(&self) -> Promise<()>;

    /// Resolves a GitHub path against the selected session's enterprise server, or `https://github.com` for a
    /// non-enterprise provider. Returns `None` when enterprise authentication is selected but the session's
    /// server is unavailable.
    fn resolve_git_hub_url(&self, path: &str) -> Option<String>;
}

service_id!(pub I_DEFAULT_ACCOUNT_SERVICE: dyn IDefaultAccountService = "defaultAccountService");

/// O estado mutável do [`DefaultAccountService`], compartilhado com os ouvintes do provedor.
struct Inner {
    me: Weak<Inner>,
    default_account: RefCell<Option<IDefaultAccount>>,
    default_account_provider: RefCell<Option<Rc<dyn IDefaultAccountProvider>>>,
    init_barrier: Barrier,
    default_provider: IChatProvider,
    on_did_change_default_account: Rc<Emitter<Option<IDefaultAccount>>>,
    on_did_change_policy_data: Rc<Emitter<Option<IPolicyData>>>,
    on_did_change_copilot_token_info: Rc<Emitter<Option<ICopilotTokenInfo>>>,
    on_did_change_managed_settings_compatibility_error: Rc<Emitter<Option<IManagedSettingsCompatibilityError>>>,
    on_did_change_managed_settings_freshness: Rc<Emitter<IManagedSettingsFreshness>>,
    base: Rc<Disposable>,
}

impl Inner {
    fn provider(&self) -> Option<Rc<dyn IDefaultAccountProvider>> {
        self.default_account_provider.borrow().clone()
    }

    /// `setDefaultAccount(account)`.
    fn set_default_account(&self, account: Option<IDefaultAccount>) {
        if *self.default_account.borrow() == account {
            return;
        }
        *self.default_account.borrow_mut() = account.clone();
        self.on_did_change_default_account.fire(account);
    }
}

/// `DefaultAccountService`.
pub struct DefaultAccountService {
    inner: Rc<Inner>,
}

impl DefaultAccountService {
    /// `new DefaultAccountService(productService)`; `default_provider` é
    /// `productService.defaultChatAgent.provider.default`.
    pub fn new(default_provider: IChatProvider) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_default_account = Emitter::new();
        let on_did_change_policy_data = Emitter::new();
        let on_did_change_copilot_token_info = Emitter::new();
        let on_did_change_managed_settings_compatibility_error = Emitter::new();
        let on_did_change_managed_settings_freshness = Emitter::new();
        base.register(on_did_change_default_account.clone());
        base.register(on_did_change_policy_data.clone());
        base.register(on_did_change_copilot_token_info.clone());
        base.register(on_did_change_managed_settings_compatibility_error.clone());
        base.register(on_did_change_managed_settings_freshness.clone());
        let inner = Rc::new_cyclic(|me| Inner {
            me: me.clone(),
            default_account: RefCell::new(None),
            default_account_provider: RefCell::new(None),
            init_barrier: Barrier::new(),
            default_provider,
            on_did_change_default_account,
            on_did_change_policy_data,
            on_did_change_copilot_token_info,
            on_did_change_managed_settings_compatibility_error,
            on_did_change_managed_settings_freshness,
            base,
        });
        Rc::new(Self { inner })
    }
}

impl IDefaultAccountService for DefaultAccountService {
    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>> {
        self.inner.on_did_change_default_account.event()
    }

    fn on_did_change_policy_data(&self) -> Event<Option<IPolicyData>> {
        self.inner.on_did_change_policy_data.event()
    }

    fn policy_data(&self) -> Option<IPolicyData> {
        self.inner.provider().and_then(|provider| provider.policy_data())
    }

    fn current_default_account(&self) -> Option<IDefaultAccount> {
        self.inner.default_account.borrow().clone()
    }

    fn copilot_token_info(&self) -> Option<ICopilotTokenInfo> {
        self.inner.provider().and_then(|provider| provider.copilot_token_info())
    }

    fn on_did_change_copilot_token_info(&self) -> Event<Option<ICopilotTokenInfo>> {
        self.inner.on_did_change_copilot_token_info.event()
    }

    fn managed_settings_fetch_status(&self) -> Option<ManagedSettingsFetchStatus> {
        self.inner.provider().and_then(|provider| provider.managed_settings_fetch_status())
    }

    fn managed_settings_fetched_at(&self) -> Option<f64> {
        self.inner.provider().and_then(|provider| provider.managed_settings_fetched_at())
    }

    fn managed_settings_raw_response(&self) -> Dynamic {
        // `provider?.managedSettingsRawResponse ?? null`
        match self.inner.provider().map(|provider| provider.managed_settings_raw_response()) {
            Some(response) if !response.is_nullish() => response,
            _ => Dynamic::Null,
        }
    }

    fn managed_settings_compatibility_error(&self) -> Option<IManagedSettingsCompatibilityError> {
        self.inner.provider().and_then(|provider| provider.managed_settings_compatibility_error())
    }

    fn on_did_change_managed_settings_compatibility_error(&self) -> Event<Option<IManagedSettingsCompatibilityError>> {
        self.inner.on_did_change_managed_settings_compatibility_error.event()
    }

    fn managed_settings_freshness(&self) -> IManagedSettingsFreshness {
        self.inner.provider().map_or(IManagedSettingsFreshness::NotRequired, |provider| provider.managed_settings_freshness())
    }

    fn on_did_change_managed_settings_freshness(&self) -> Event<IManagedSettingsFreshness> {
        self.inner.on_did_change_managed_settings_freshness.event()
    }

    fn get_default_account(&self) -> Promise<Option<IDefaultAccount>> {
        let inner = self.inner.clone();
        async_fn(async move {
            inner.init_barrier.wait().await?;
            Ok(inner.default_account.borrow().clone())
        })
    }

    fn get_default_account_authentication_provider(&self) -> IDefaultAccountAuthenticationProvider {
        if let Some(provider) = self.inner.provider() {
            return provider.get_default_account_authentication_provider();
        }
        IDefaultAccountAuthenticationProvider { id: self.inner.default_provider.id.clone(), name: self.inner.default_provider.name.clone(), enterprise: false, extra: Default::default() }
    }

    fn set_default_account_provider(&self, provider: Rc<dyn IDefaultAccountProvider>) -> Result<(), Error> {
        let inner = &self.inner;
        if inner.default_account_provider.borrow().is_some() {
            return Err(Error::new("Default account provider is already set"));
        }

        *inner.default_account_provider.borrow_mut() = Some(provider.clone());
        let weak = inner.me.clone();
        inner.base.register(provider.on_did_change_managed_settings_compatibility_error().listen({
            let weak = weak.clone();
            move |error| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_did_change_managed_settings_compatibility_error.fire(error.clone());
                }
            }
        }));
        inner.base.register(provider.on_did_change_managed_settings_freshness().listen({
            let weak = weak.clone();
            move |freshness| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_did_change_managed_settings_freshness.fire(freshness.clone());
                }
            }
        }));
        if let Some(policy_data) = provider.policy_data() {
            inner.on_did_change_policy_data.fire(Some(policy_data));
        }
        if let Some(error) = provider.managed_settings_compatibility_error() {
            inner.on_did_change_managed_settings_compatibility_error.fire(Some(error));
        }
        let freshness = provider.managed_settings_freshness();
        if freshness.state() != ManagedSettingsFreshnessState::NotRequired {
            inner.on_did_change_managed_settings_freshness.fire(freshness);
        }

        let on_refreshed = inner.me.clone();
        let on_settled = inner.me.clone();
        let settled_provider = provider.clone();
        let _ = provider
            .refresh(None)
            .then(move |account| {
                if let Some(inner) = on_refreshed.upgrade() {
                    *inner.default_account.borrow_mut() = account;
                }
            })
            .finally(move || {
                let Some(inner) = on_settled.upgrade() else { return };
                inner.init_barrier.open();
                let weak = inner.me.clone();
                inner.base.register(settled_provider.on_did_change_default_account().listen({
                    let weak = weak.clone();
                    move |account| {
                        if let Some(inner) = weak.upgrade() {
                            inner.set_default_account(account.clone());
                        }
                    }
                }));
                inner.base.register(settled_provider.on_did_change_policy_data().listen({
                    let weak = weak.clone();
                    move |policy_data| {
                        if let Some(inner) = weak.upgrade() {
                            inner.on_did_change_policy_data.fire(policy_data.clone());
                        }
                    }
                }));
                inner.base.register(settled_provider.on_did_change_copilot_token_info().listen(move |token_info| {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_did_change_copilot_token_info.fire(token_info.clone());
                    }
                }));
            });
        Ok(())
    }

    fn refresh(&self, options: Option<IDefaultAccountRefreshOptions>) -> Promise<Option<IDefaultAccount>> {
        let inner = self.inner.clone();
        async_fn(async move {
            inner.init_barrier.wait().await?;

            let account = match inner.provider() {
                Some(provider) => provider.refresh(options).await?,
                None => None,
            };
            inner.set_default_account(account);
            Ok(inner.default_account.borrow().clone())
        })
    }

    fn sign_in(&self, options: Option<ISignInOptions>) -> Promise<Option<IDefaultAccount>> {
        let inner = self.inner.clone();
        async_fn(async move {
            inner.init_barrier.wait().await?;
            match inner.provider() {
                Some(provider) => provider.sign_in(options).await,
                None => Ok(None),
            }
        })
    }

    fn sign_out(&self) -> Promise<()> {
        let inner = self.inner.clone();
        async_fn(async move {
            inner.init_barrier.wait().await?;
            if let Some(provider) = inner.provider() {
                provider.sign_out().await?;
            }
            Ok(())
        })
    }

    fn resolve_git_hub_url(&self, path: &str) -> Option<String> {
        match self.inner.provider() {
            Some(provider) => provider.resolve_git_hub_url(path),
            None => Some(format!("https://github.com/{path}")),
        }
    }
}

impl IReducedDefaultAccountService for DefaultAccountService {
    fn get_default_account_authentication_provider(&self) -> reduced::IDefaultAccountAuthenticationProvider {
        let provider = IDefaultAccountService::get_default_account_authentication_provider(self);
        reduced::IDefaultAccountAuthenticationProvider { id: provider.id, name: provider.name, enterprise: provider.enterprise }
    }
}

impl IDisposable for DefaultAccountService {
    fn dispose(&self) {
        self.inner.base.dispose();
    }
}
