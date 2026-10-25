//! Porte de `src/vs/workbench/contrib/editSessions/browser/editSessionsStorageService.ts`: o `EditSessionsWorkbenchService`, que
//! autentica o usuário no servidor do Cloud Changes (`editSessions.store` do produto), guarda a preferência de conta, e lê, grava,
//! apaga e lista os recursos `editSessions` e `workspaceState` pelo [`EditSessionsStoreClient`].
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são os campos de [`EditSessionsWorkbenchServices`], e
//!   [`EditSessionsWorkbenchService::new`] os recebe explícitos. O registro do singleton
//!   (`registerSingleton(IEditSessionsStorageService, EditSessionsWorkbenchService, ...)`) é da `editSessions.contribution.ts`
//!   (fatia 2b). O serviço de autenticação entra como `Rc<dyn IAuthenticationServiceProviders>`: o `getProvider` do original
//!   (rótulo e `supportsMultipleAccounts` do provedor) só existe nesse trait do porte, que estende o `IAuthenticationService`.
//! - **Classe `Disposable`** é o campo `base`, e o serviço fica num `Rc` com uma referência fraca a si (`me`), usada pelos ouvintes,
//!   pelas promessas e pelas duas ações. As ações guardam só a referência fraca (o `that = this` do original é uma forte, mas o
//!   registro de ações do porte vive até o descarte do serviço, e uma forte fecharia um ciclo).
//! - **`async`** é `async_fn`: o corpo roda já, até o primeiro `await`, e um `throw` vira a promessa rejeitada. `this.storeClient!`
//!   e `this.machineClient!` ausentes são o `TypeError` que o JavaScript lançaria ao ler a propriedade de `undefined`.
//! - **Itens do quick pick.** `IQuickPickItem & { session }` e `IQuickPickItem & { provider }` não cabem num `IQuickPickItem`
//!   (que não é genérico); cada item leva em `id` o índice da sua escolha (`QuickPickChoice`) na lista que acompanha os itens, e a
//!   seleção é resolvida por esse índice (o `'provider' in selection` do original).
//!   O item `canceledAuthentication` do tipo da união nunca é criado e não existe aqui.
//! - **`Map` de contas** (`getAllSessions`): `accounts.set(id, item)` numa chave que existe troca o valor e mantém a posição, como
//!   o `Map`; a lista final é ordenada por `localeCompare` do rótulo (ordenação estável, como a do V8).
//! - **`createQuickpickItems` / `getAuthenticationProviders`**: `getAuthenticationProviders` não espera nada, então é uma função
//!   síncrona que devolve `Result` (o `throw` do original, dentro de um `async`, é o `Err` que o `?` propaga como rejeição).
//! - **`isWeb`** é o [`Platform::is_web`] da plataforma detectada.
//! - **`logService.error(ex)`** é o `error_args` com o [`Error`] como mensagem (o log escreve a pilha).
//! - **Ações.** `registerAction2(class ... extends Action2)` são as structs [`SignInAction`] e [`ResetAuthenticationAction`]; o
//!   `run` assíncrono do original dispara a promessa e descarta o resultado (o `Action2::run` do porte não devolve valor).
//! - **`write`**: o `content.machine` é preenchido na cópia recebida (ver `common.rs`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::collator::locale_compare;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::product::EditSessionsStore;
use je_base::promise::Promise;
use je_base::uuid::generate_uuid;
use je_instantiation::ServicesAccessor;
use je_platform::action::{Category, CommandAction, Title};
use je_platform::actions::actions::{Action2, Action2MenuItem, Action2Options, AnyMenuItem, MenuItem, menu_registry, register_action2};
use je_platform::actions::menu_ids;
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression, IContextKey, IContextKeyService};
use je_platform::dialogs::{BaseDialogOptions, Checkbox, Confirmation, IDialogService};
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::LogMessage;
use je_platform::product::IProductService;
use je_platform::quickinput::quick_input::{
    IQuickItem, IQuickPickItem, IQuickPickSeparator, IQuickInputService, QuickPickCreateOptions, QuickPickInput, QuickPickOk,
};
use je_platform::secrets::ISecretStorageService;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::user_data_sync::user_data_sync::{
    IAuthenticationProvider as SyncAuthenticationProvider, IResourceRefHandle, IUserDataSyncStoreService, ServerResource, create_sync_headers,
};
use je_platform::user_data_sync::user_data_sync_machines::{IUserDataSyncMachinesService, UserDataSyncMachinesService};
use je_storage::StorageValue;

use super::common::{
    EDIT_SESSIONS_PENDING_KEY, EDIT_SESSIONS_SIGNED_IN_KEY, EditSessionContent, EditSessionResource, EditSessionResourceMap, IEditSessionsLogService,
    IEditSessionsStorageService, InitializeReason, SyncResource, edit_session_sync_category, edit_sessions_signed_in,
};
use super::edit_sessions_storage_client::EditSessionsStoreClient;
use crate::services::authentication::authentication::{
    AuthenticationScopeListOrRequest, AuthenticationSession, AuthenticationSessionsChangeEvent, IAuthenticationServiceProviders,
};
use crate::services::authentication::authentication_service::get_current_authentication_session_info;
use crate::services::extensions::extensions::IExtensionService;

/// `AuthenticationSession & { providerId: string }`.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthenticationSessionWithProvider {
    pub session: AuthenticationSession,
    pub provider_id: String,
}

/// `ExistingSession`: `IQuickPickItem & { session: AuthenticationSession & { providerId: string } }`.
#[derive(Clone)]
struct ExistingSession {
    label: String,
    description: String,
    session: AuthenticationSessionWithProvider,
}

/// O que cada item selecionável do quick pick representa (o `'provider' in selection` / `'session' in selection` do original).
/// `AuthenticationProviderOption` (`IQuickPickItem & { provider: IAuthenticationProvider }`) é o próprio provedor, porque o rótulo do
/// item é calculado ao criá-lo.
#[derive(Clone)]
enum QuickPickChoice {
    Session(ExistingSession),
    Provider(SyncAuthenticationProvider),
}

/// Os itens do quick pick (separadores incluídos) e a escolha de cada item selecionável, pelo índice guardado no `id` do item.
#[derive(Clone)]
struct QuickPickItems {
    items: Vec<QuickPickInput>,
    choices: Vec<QuickPickChoice>,
}

/// O `{ sessionId: string; token: string; providerId: string }` de `authenticationInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AuthenticationInfo {
    session_id: String,
    token: String,
    provider_id: String,
}

/// `SIZE_LIMIT = Math.floor(1024 * 1024 * 1.9); // 2 MB`.
const SIZE_LIMIT: usize = (1024.0 * 1024.0 * 1.9) as usize;

/// `EditSessionsWorkbenchService.CACHED_SESSION_STORAGE_KEY`.
const CACHED_SESSION_STORAGE_KEY: &str = "editSessionAccountPreference";

const SIGN_IN_ACTION_ID: &str = "workbench.editSessions.actions.signIn";
const RESET_AUTHENTICATION_ACTION_ID: &str = "workbench.editSessions.actions.resetAuth";
const ACCOUNTS_CONTEXT_GROUP: &str = "2_editSessions";

/// Os serviços que o `EditSessionsWorkbenchService` recebe por injeção.
#[derive(Clone)]
pub struct EditSessionsWorkbenchServices {
    pub file_service: Rc<dyn IFileService>,
    pub storage_service: Rc<dyn StorageService>,
    pub quick_input_service: Rc<dyn IQuickInputService>,
    pub authentication_service: Rc<dyn IAuthenticationServiceProviders>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub environment_service: Rc<dyn EnvironmentService>,
    pub log_service: Rc<dyn IEditSessionsLogService>,
    pub product_service: Rc<dyn IProductService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub dialog_service: Rc<dyn IDialogService>,
    pub secret_storage_service: Rc<dyn ISecretStorageService>,
}

/// O `TypeError` de ler `property` de `undefined`.
fn undefined_property_error(property: &str) -> Error {
    Error::named("TypeError", format!("Cannot read properties of undefined (reading '{property}')"))
}

/// O texto de um `string | undefined` interpolado num template literal.
fn js_text(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("undefined")
}

/// `EditSessionsWorkbenchService`.
pub struct EditSessionsWorkbenchService {
    me: Weak<Self>,
    base: Rc<Disposable>,
    file_service: Rc<dyn IFileService>,
    storage_service: Rc<dyn StorageService>,
    quick_input_service: Rc<dyn IQuickInputService>,
    authentication_service: Rc<dyn IAuthenticationServiceProviders>,
    extension_service: Rc<dyn IExtensionService>,
    environment_service: Rc<dyn EnvironmentService>,
    log_service: Rc<dyn IEditSessionsLogService>,
    product_service: Rc<dyn IProductService>,
    dialog_service: Rc<dyn IDialogService>,
    secret_storage_service: Rc<dyn ISecretStorageService>,
    server_configuration: Option<EditSessionsStore>,
    machine_client: RefCell<Option<Rc<dyn IUserDataSyncMachinesService>>>,
    authentication_info: RefCell<Option<AuthenticationInfo>>,
    initialized: Cell<bool>,
    signed_in_context: Rc<dyn IContextKey>,
    did_sign_in: Rc<Emitter<()>>,
    did_sign_out: Rc<Emitter<()>>,
    last_written_resources: EditSessionResourceMap,
    last_read_resources: EditSessionResourceMap,
    store_client: RefCell<Option<Rc<EditSessionsStoreClient>>>,
    cached_machines: RefCell<Option<HashMap<String, String>>>,
}

impl EditSessionsWorkbenchService {
    pub fn new(services: EditSessionsWorkbenchServices) -> Rc<Self> {
        let EditSessionsWorkbenchServices {
            file_service,
            storage_service,
            quick_input_service,
            authentication_service,
            extension_service,
            environment_service,
            log_service,
            product_service,
            context_key_service,
            dialog_service,
            secret_storage_service,
        } = services;

        let base = Disposable::new();
        let did_sign_in = Emitter::new();
        base.register(did_sign_in.clone());
        let did_sign_out = Emitter::new();
        base.register(did_sign_out.clone());

        let server_configuration = product_service.edit_sessions_store().cloned();
        let signed_in_context = edit_sessions_signed_in().bind_to(context_key_service.as_ref());

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            file_service,
            storage_service,
            quick_input_service,
            authentication_service,
            extension_service,
            environment_service,
            log_service,
            product_service,
            dialog_service,
            secret_storage_service,
            server_configuration,
            machine_client: RefCell::new(None),
            authentication_info: RefCell::new(None),
            initialized: Cell::new(false),
            signed_in_context,
            did_sign_in,
            did_sign_out,
            last_written_resources: Rc::default(),
            last_read_resources: Rc::default(),
            store_client: RefCell::new(None),
            cached_machines: RefCell::new(None),
        });

        // If the user signs out of the current session, reset our cached auth state in memory and on disk
        let weak = this.me.clone();
        this.base.register(this.authentication_service.on_did_change_sessions().listen(move |change| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_sessions(&change.event);
            }
        }));

        // If another window changes the preferred session storage, reset our cached auth state in memory
        let weak = this.me.clone();
        this.base.register(this.storage_service.on_did_change_value(StorageScope::Application, Some(CACHED_SESSION_STORAGE_KEY), this.base.store()).listen(
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_storage();
                }
            },
        ));

        this.register_sign_in_action();
        this.register_reset_authentication_action();

        this.signed_in_context.set(Dynamic::Bool(this.existing_session_id().is_some()));
        this
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    /// `serverConfiguration?.url` com valor (a condição `!this.serverConfiguration?.url` negada).
    fn has_server_url(&self) -> bool {
        self.server_configuration.as_ref().is_some_and(|configuration| !configuration.url.is_empty())
    }

    /// `this.storeClient!.<method>`.
    fn required_store_client(&self, method: &str) -> Result<Rc<EditSessionsStoreClient>, Error> {
        self.store_client.borrow().clone().ok_or_else(|| undefined_property_error(method))
    }

    /// `this.machineClient!.<method>`.
    fn required_machine_client(&self, method: &str) -> Result<Rc<dyn IUserDataSyncMachinesService>, Error> {
        self.machine_client.borrow().clone().ok_or_else(|| undefined_property_error(method))
    }

    fn log_error(&self, error: &Error) {
        self.log_service.error_args(&LogMessage::from(error), &[]);
    }

    /// O `get existingSessionId()`.
    fn existing_session_id(&self) -> Option<String> {
        self.storage_service.get(CACHED_SESSION_STORAGE_KEY, StorageScope::Application, None)
    }

    /// O `set existingSessionId(sessionId)`.
    fn set_existing_session_id(&self, session_id: Option<&str>) {
        self.log_service.trace(&format!("Saving authentication session preference for ID {}.", session_id.unwrap_or("undefined")));
        match session_id {
            None => self.storage_service.remove(CACHED_SESSION_STORAGE_KEY, StorageScope::Application),
            Some(session_id) => {
                self.storage_service.store(CACHED_SESSION_STORAGE_KEY, StorageValue::from(session_id), StorageScope::Application, StorageTarget::Machine);
            }
        }
    }

    /**
     * Ensures that the store client is initialized,
     * meaning that authentication is configured and it
     * can be used to communicate with the remote storage service
     */
    fn do_initialize(&self, reason: InitializeReason, silent: bool) -> Promise<bool> {
        let this = self.rc();
        async_fn(async move {
            // Wait for authentication extensions to be registered
            this.extension_service.when_installed_extensions_registered().await?;

            if !this.has_server_url() {
                return Err(Error::new("Unable to initialize sessions sync as session sync preference is not configured in product.json."));
            }

            let Some(store_client) = this.store_client.borrow().clone() else {
                return Ok(false);
            };

            let weak = this.me.clone();
            this.base.register(store_client.on_token_failed().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.log_service.info("Clearing edit sessions authentication preference because of successive token failures.");
                    this.clear_authentication_preference();
                }
            }));

            if this.machine_client.borrow().is_none() {
                let machine_client: Rc<dyn IUserDataSyncMachinesService> = UserDataSyncMachinesService::new(
                    this.environment_service.clone(),
                    this.file_service.clone(),
                    this.storage_service.clone(),
                    store_client.clone(),
                    this.log_service.clone(),
                    this.product_service.clone(),
                );
                *this.machine_client.borrow_mut() = Some(machine_client);
            }

            // If we already have an existing auth session in memory, use that
            if this.authentication_info.borrow().is_some() {
                return Ok(true);
            }

            let authentication_session = this.get_authentication_session(reason, silent).await?;
            if let Some(authentication_session) = authentication_session {
                store_client.set_auth_token(&authentication_session.token, &authentication_session.provider_id);
                *this.authentication_info.borrow_mut() = Some(authentication_session);
                return Ok(true);
            }

            Ok(false)
        })
    }

    fn get_or_create_current_machine_id(&self) -> Promise<String> {
        let this = self.rc();
        async_fn(async move {
            let machine_client = this.required_machine_client("getMachines")?;
            let machines = machine_client.get_machines(None).await?;
            let current_machine_id = machines.iter().find(|machine| machine.is_current).map(|machine| machine.id.clone());

            match current_machine_id {
                Some(current_machine_id) => Ok(current_machine_id),
                None => {
                    machine_client.add_current_machine(None).await?;
                    let machines = machine_client.get_machines(None).await?;
                    machines.into_iter().find(|machine| machine.is_current).map(|machine| machine.id).ok_or_else(|| undefined_property_error("id"))
                }
            }
        })
    }

    fn get_authentication_session(&self, reason: InitializeReason, silent: bool) -> Promise<Option<AuthenticationInfo>> {
        let this = self.rc();
        async_fn(async move {
            // If the user signed in previously and the session is still available, reuse that without prompting the user again
            if let Some(existing_session_id) = this.existing_session_id().filter(|session_id| !session_id.is_empty()) {
                this.log_service.info(&format!("Searching for existing authentication session with ID {existing_session_id}"));
                let existing_session = this.get_existing_session().await?;
                if let Some(existing_session) = existing_session {
                    let session = &existing_session.session.session;
                    this.log_service.info(&format!("Found existing authentication session with ID {}", session.id));
                    return Ok(Some(AuthenticationInfo {
                        session_id: session.id.clone(),
                        token: session.id_token.clone().unwrap_or_else(|| session.access_token.clone()),
                        provider_id: existing_session.session.provider_id.clone(),
                    }));
                } else {
                    this.did_sign_out.fire(());
                }
            }

            // If settings sync is already enabled, avoid asking again to authenticate
            if this.should_attempt_edit_session_init() {
                this.log_service.info("Reusing user data sync enablement");
                let authentication_session_info = get_current_authentication_session_info(this.secret_storage_service.clone(), this.product_service.clone()).await?;
                if let Some(authentication_session_info) = authentication_session_info {
                    this.log_service.info(&format!("Using current authentication session with ID {}", authentication_session_info.id));
                    this.set_existing_session_id(Some(&authentication_session_info.id));
                    return Ok(Some(AuthenticationInfo {
                        session_id: authentication_session_info.id,
                        token: authentication_session_info.access_token,
                        provider_id: authentication_session_info.provider_id,
                    }));
                }
            }

            // If we aren't supposed to prompt the user because
            // we're in a silent flow, just return here
            if silent {
                return Ok(None);
            }

            // Ask the user to pick a preferred account
            let authentication_session = this.get_account_preference(reason).await?;
            if let Some(authentication_session) = authentication_session {
                this.set_existing_session_id(Some(&authentication_session.session.id));
                return Ok(Some(AuthenticationInfo {
                    session_id: authentication_session.session.id.clone(),
                    token: authentication_session.session.id_token.clone().unwrap_or_else(|| authentication_session.session.access_token.clone()),
                    provider_id: authentication_session.provider_id,
                }));
            }

            Ok(None)
        })
    }

    fn should_attempt_edit_session_init(&self) -> bool {
        Platform::detect().is_web && self.storage_service.is_new(StorageScope::Application) && self.storage_service.is_new(StorageScope::Workspace)
    }

    /**
     * Prompts the user to pick an authentication option for storing and getting edit sessions.
     */
    fn get_account_preference(&self, reason: InitializeReason) -> Promise<Option<AuthenticationSessionWithProvider>> {
        let this = self.rc();
        async_fn(async move {
            let disposables = DisposableStore::new();
            let quickpick = this.quick_input_service.create_quick_pick(Some(QuickPickCreateOptions { use_separators: true }));
            disposables.add(quickpick.clone());
            quickpick.set_ok(QuickPickOk::Bool(false));
            quickpick.set_placeholder(Some(match reason {
                InitializeReason::Read => localize("choose account read placeholder", "Select an account to restore your working changes from the cloud", &[]),
                InitializeReason::Write => localize("choose account placeholder", "Select an account to store your working changes in the cloud", &[]),
            }));
            quickpick.set_ignore_focus_out(true);
            let QuickPickItems { items, choices } = this.create_quickpick_items().await?;
            quickpick.set_items(items);
            let choices = Rc::new(choices);

            let selected = Promise::with_executor(|resolver| {
                let weak_disposables = Rc::downgrade(&disposables);
                let on_did_hide = {
                    let resolver = resolver.clone();
                    quickpick.on_did_hide().listen(move |_| {
                        resolver.reject(Error::cancellation());
                        if let Some(disposables) = weak_disposables.upgrade() {
                            disposables.dispose();
                        }
                    })
                };
                disposables.add(on_did_hide);

                let on_did_accept = {
                    let resolver = resolver.clone();
                    let quickpick = quickpick.clone();
                    let this = this.clone();
                    let choices = choices.clone();
                    quickpick.on_did_accept().listen(move |_| {
                        let resolver = resolver.clone();
                        let quickpick = quickpick.clone();
                        let this = this.clone();
                        let choices = choices.clone();
                        let _ = async_fn(async move {
                            let selection = quickpick
                                .selected_items()
                                .into_iter()
                                .next()
                                .ok_or_else(|| Error::named("TypeError", "Cannot use 'in' operator to search for 'provider' in undefined"))?;
                            let choice = selection.base.id.as_deref().and_then(|id| id.parse::<usize>().ok()).and_then(|index| choices.get(index));
                            let session = match choice {
                                Some(QuickPickChoice::Provider(provider)) => {
                                    let created =
                                        this.authentication_service.create_session(&provider.id, AuthenticationScopeListOrRequest::Scopes(provider.scopes.clone()), None).await?;
                                    Some(AuthenticationSessionWithProvider { session: created, provider_id: provider.id.clone() })
                                }
                                Some(QuickPickChoice::Session(existing_session)) => Some(existing_session.session.clone()),
                                None => None,
                            };
                            resolver.resolve(session);
                            quickpick.hide();
                            Ok(())
                        });
                    })
                };
                disposables.add(on_did_accept);

                quickpick.show();
            });
            selected.await
        })
    }

    fn create_quickpick_items(&self) -> Promise<QuickPickItems> {
        let this = self.rc();
        async_fn(async move {
            let mut items: Vec<QuickPickInput> = Vec::new();
            let mut choices: Vec<QuickPickChoice> = Vec::new();
            let separator = |label: String| QuickPickInput::Separator(IQuickPickSeparator { label: Some(label), ..IQuickPickSeparator::default() });
            let item = |index: usize, label: &str, description: Option<&str>| {
                QuickPickInput::Item(IQuickPickItem {
                    base: IQuickItem { id: Some(index.to_string()), label: JsString::from(label), description: description.map(JsString::from), ..IQuickItem::default() },
                    ..IQuickPickItem::default()
                })
            };

            items.push(separator(localize("signed in", "Signed In", &[])));

            let sessions = this.get_all_sessions().await?;
            for session in &sessions {
                items.push(item(choices.len(), &session.label, Some(&session.description)));
                choices.push(QuickPickChoice::Session(session.clone()));
            }

            items.push(separator(localize("others", "Others", &[])));

            for authentication_provider in this.get_authentication_providers()? {
                let signed_in_for_provider = sessions.iter().any(|account| account.session.provider_id == authentication_provider.id);
                if !signed_in_for_provider || this.authentication_service.get_provider(&authentication_provider.id).supports_multiple_accounts() {
                    let provider_name = this.authentication_service.get_provider(&authentication_provider.id).label();
                    let label = localize("sign in using account", "Sign in with {0}", &[provider_name.as_str()]);
                    items.push(item(choices.len(), &label, None));
                    choices.push(QuickPickChoice::Provider(authentication_provider));
                }
            }

            Ok(QuickPickItems { items, choices })
        })
    }

    /**
     * Returns all authentication sessions available from {@link getAuthenticationProviders}.
     */
    fn get_all_sessions(&self) -> Promise<Vec<ExistingSession>> {
        let this = self.rc();
        async_fn(async move {
            let authentication_providers = this.get_authentication_providers()?;
            // `new Map<string, ExistingSession>()`: trocar o valor de uma chave existente mantém a posição dela.
            let mut accounts: Vec<(String, ExistingSession)> = Vec::new();
            let mut set_account = |account_id: String, item: ExistingSession| match accounts.iter_mut().find(|(key, _)| *key == account_id) {
                Some(entry) => entry.1 = item,
                None => accounts.push((account_id, item)),
            };
            let mut current_session: Option<ExistingSession> = None;

            for provider in &authentication_providers {
                let sessions = this
                    .authentication_service
                    .get_sessions(&provider.id, Some(AuthenticationScopeListOrRequest::Scopes(provider.scopes.clone())), None, None)
                    .await?;

                for session in sessions {
                    let item = ExistingSession {
                        label: session.account.label.clone(),
                        description: this.authentication_service.get_provider(&provider.id).label(),
                        session: AuthenticationSessionWithProvider { session: session.clone(), provider_id: provider.id.clone() },
                    };
                    set_account(session.account.id.clone(), item.clone());
                    if this.existing_session_id().as_deref() == Some(session.id.as_str()) {
                        current_session = Some(item);
                    }
                }
            }

            if let Some(current_session) = current_session {
                set_account(current_session.session.session.account.id.clone(), current_session);
            }

            let mut sessions: Vec<ExistingSession> = accounts.into_iter().map(|(_, item)| item).collect();
            sessions.sort_by(|a, b| locale_compare(&a.label, &b.label));
            Ok(sessions)
        })
    }

    /**
     * Returns all authentication providers which can be used to authenticate
     * to the remote storage service, based on product.json configuration
     * and registered authentication providers.
     */
    fn get_authentication_providers(&self) -> Result<Vec<SyncAuthenticationProvider>, Error> {
        let Some(server_configuration) = &self.server_configuration else {
            return Err(Error::new("Unable to get configured authentication providers as session sync preference is not configured in product.json."));
        };

        // Get the list of authentication providers configured in product.json
        let configured_authentication_providers = server_configuration
            .authentication_providers
            .iter()
            .map(|(id, provider)| SyncAuthenticationProvider { id: id.clone(), scopes: provider.scopes.clone() });

        // Filter out anything that isn't currently available through the authenticationService
        let available_authentication_providers = self.authentication_service.declared_providers();

        Ok(configured_authentication_providers.filter(|provider| available_authentication_providers.iter().any(|available| available.id == provider.id)).collect())
    }

    fn get_existing_session(&self) -> Promise<Option<ExistingSession>> {
        let this = self.rc();
        async_fn(async move {
            let accounts = this.get_all_sessions().await?;
            let existing_session_id = this.existing_session_id();
            Ok(accounts.into_iter().find(|account| existing_session_id.as_deref() == Some(account.session.session.id.as_str())))
        })
    }

    fn on_did_change_storage(&self) {
        let new_session_id = self.existing_session_id();
        let previous_session_id = self.authentication_info.borrow().as_ref().map(|info| info.session_id.clone());

        if previous_session_id != new_session_id {
            self.log_service.trace(&format!(
                "Resetting authentication state because authentication session ID preference changed from {} to {}.",
                js_text(&previous_session_id),
                js_text(&new_session_id)
            ));
            *self.authentication_info.borrow_mut() = None;
            self.initialized.set(false);
        }
    }

    fn clear_authentication_preference(&self) {
        *self.authentication_info.borrow_mut() = None;
        self.initialized.set(false);
        self.set_existing_session_id(None);
        self.signed_in_context.set(Dynamic::Bool(false));
    }

    fn on_did_change_sessions(&self, e: &AuthenticationSessionsChangeEvent) {
        let session_id = self.authentication_info.borrow().as_ref().map(|info| info.session_id.clone()).filter(|session_id| !session_id.is_empty());
        if let Some(session_id) = session_id
            && e.removed.as_ref().is_some_and(|removed| removed.iter().any(|session| session.id == session_id))
        {
            self.clear_authentication_preference();
        }
    }

    fn register_sign_in_action(&self) {
        if !self.has_server_url() {
            return;
        }
        let when = ContextKeyExpr::and([ContextKeyExpr::equals(EDIT_SESSIONS_PENDING_KEY, false), ContextKeyExpr::equals(EDIT_SESSIONS_SIGNED_IN_KEY, false)]);
        self.base.register(register_action2(Rc::new(SignInAction { service: self.me.clone(), when })));

        self.base.register(menu_registry().append_menu_item(
            &menu_ids::accounts_context(),
            AnyMenuItem::Menu(MenuItem {
                group: Some(ACCOUNTS_CONTEXT_GROUP.to_string()),
                when: ContextKeyExpr::and([ContextKeyExpr::equals(EDIT_SESSIONS_PENDING_KEY, true), ContextKeyExpr::equals(EDIT_SESSIONS_SIGNED_IN_KEY, false)]),
                ..MenuItem::new(CommandAction::new(SIGN_IN_ACTION_ID, Title::Plain(localize("sign in badge", "Turn on Cloud Changes... (1)", &[]))))
            }),
        ));
    }

    fn register_reset_authentication_action(&self) {
        self.base.register(register_action2(Rc::new(ResetAuthenticationAction { service: self.me.clone() })));
    }
}

/// O item de `MenuId.CommandPalette` e o de `MenuId.AccountsContext` (no grupo `2_editSessions`) de uma ação do serviço.
fn command_palette_and_accounts_menu(accounts_when: Option<ContextKeyExpression>) -> Vec<Action2MenuItem> {
    vec![
        Action2MenuItem { id: menu_ids::command_palette(), null_precondition: false, alt: None, when: None, group: None, order: None, is_hidden_by_default: None },
        Action2MenuItem {
            id: menu_ids::accounts_context(),
            null_precondition: false,
            alt: None,
            when: accounts_when,
            group: Some(ACCOUNTS_CONTEXT_GROUP.to_string()),
            order: None,
            is_hidden_by_default: None,
        },
    ]
}

/// A ação `workbench.editSessions.actions.signIn` (a classe `ResetEditSessionAuthenticationAction` de `registerSignInAction`).
struct SignInAction {
    service: Weak<EditSessionsWorkbenchService>,
    when: Option<ContextKeyExpression>,
}

impl Action2 for SignInAction {
    fn desc(&self) -> Action2Options {
        Action2Options {
            command: CommandAction {
                category: Some(Category::Localized(edit_session_sync_category())),
                precondition: self.when.clone(),
                ..CommandAction::new(SIGN_IN_ACTION_ID, Title::Plain(localize("sign in", "Turn on Cloud Changes...", &[])))
            },
            menu: command_palette_and_accounts_menu(self.when.clone()),
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run(&self, _accessor: &ServicesAccessor, _args: &[Dynamic]) {
        if let Some(service) = self.service.upgrade() {
            let _ = service.initialize(InitializeReason::Write, false);
        }
    }
}

/// A ação `workbench.editSessions.actions.resetAuth`.
struct ResetAuthenticationAction {
    service: Weak<EditSessionsWorkbenchService>,
}

impl Action2 for ResetAuthenticationAction {
    fn desc(&self) -> Action2Options {
        let signed_in = ContextKeyExpr::equals(EDIT_SESSIONS_SIGNED_IN_KEY, true);
        Action2Options {
            command: CommandAction {
                category: Some(Category::Localized(edit_session_sync_category())),
                precondition: Some(signed_in.clone()),
                ..CommandAction::new(RESET_AUTHENTICATION_ACTION_ID, Title::Plain(localize("reset auth.v3", "Turn off Cloud Changes...", &[])))
            },
            menu: command_palette_and_accounts_menu(Some(signed_in)),
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run(&self, _accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let Some(that) = self.service.upgrade() else {
            return;
        };
        let _ = async_fn(async move {
            let result = that
                .dialog_service
                .confirm(Confirmation {
                    base: BaseDialogOptions {
                        checkbox: Some(Checkbox { label: localize("delete all cloud changes", "Delete all stored data from the cloud.", &[]), checked: None }),
                        ..BaseDialogOptions::new(localize("sign out of cloud changes clear data prompt", "Do you want to disable storing working changes in the cloud?", &[]))
                    },
                    primary_button: None,
                    cancel_button: None,
                })
                .await?;
            if result.confirmed {
                if result.checkbox_checked == Some(true)
                    && let Some(store_client) = that.store_client.borrow().clone()
                {
                    let _ = store_client.delete_resource(ServerResource::EditSessions, None, None);
                }
                that.clear_authentication_preference();
            }
            Ok(())
        });
    }
}

impl IEditSessionsStorageService for EditSessionsWorkbenchService {
    fn size_limit(&self) -> usize {
        SIZE_LIMIT
    }

    fn is_signed_in(&self) -> bool {
        self.existing_session_id().is_some()
    }

    fn on_did_sign_in(&self) -> Event<()> {
        self.did_sign_in.event()
    }

    fn on_did_sign_out(&self) -> Event<()> {
        self.did_sign_out.event()
    }

    fn store_client(&self) -> Option<Rc<EditSessionsStoreClient>> {
        self.store_client.borrow().clone()
    }

    fn set_store_client(&self, store_client: Option<Rc<EditSessionsStoreClient>>) {
        *self.store_client.borrow_mut() = store_client;
    }

    fn last_read_resources(&self) -> EditSessionResourceMap {
        self.last_read_resources.clone()
    }

    fn last_written_resources(&self) -> EditSessionResourceMap {
        self.last_written_resources.clone()
    }

    fn initialize(&self, reason: InitializeReason, silent: bool) -> Promise<bool> {
        let this = self.rc();
        async_fn(async move {
            if this.initialized.get() {
                return Ok(true);
            }
            let initialized = this.do_initialize(reason, silent).await?;
            this.initialized.set(initialized);
            this.signed_in_context.set(Dynamic::Bool(initialized));
            if initialized {
                this.did_sign_in.fire(());
            }
            Ok(this.initialized.get())
        })
    }

    /**
     * @param resource: The resource to retrieve content for.
     * @param ref: A specific content ref to retrieve content for, if it exists.
     * If undefined, this method will return the latest saved edit session, if any.
     *
     * @returns An object representing the requested or latest state, if any.
     */
    fn read(&self, resource: SyncResource, r#ref: Option<String>) -> Promise<Option<EditSessionResource>> {
        let this = self.rc();
        async_fn(async move {
            this.initialize(InitializeReason::Read, false).await?;
            if !this.initialized.get() {
                return Err(Error::new("Please sign in to apply your latest edit session."));
            }

            let mut content: Option<String> = None;
            let mut r#ref = r#ref;
            let headers = create_sync_headers(&generate_uuid());
            let outcome: Result<(), Error> = async {
                let store_client = this.store_client.borrow().clone();
                if let Some(requested_ref) = r#ref.clone() {
                    if let Some(store_client) = store_client {
                        content = store_client.resolve_resource_content(resource.into(), &requested_ref, None, Some(headers)).await?;
                    }
                } else if let Some(store_client) = store_client {
                    let result = store_client.read_resource(resource.into(), None, None, Some(headers)).await?;
                    content = result.content;
                    r#ref = Some(result.r#ref);
                }
                Ok::<(), Error>(())
            }
            .await;
            if let Err(ex) = outcome {
                this.log_error(&ex);
            }

            // TODO@joyceerhl Validate session data, check schema version
            if let (Some(content), Some(r#ref)) = (content, r#ref) {
                let resource_content = EditSessionResource { r#ref, content };
                this.last_read_resources.borrow_mut().insert(resource, resource_content.clone());
                return Ok(Some(resource_content));
            }
            Ok(None)
        })
    }

    /**
     * @param resource: The resource to retrieve content for.
     * @param content An object representing resource state to be restored.
     * @returns The ref of the stored state.
     */
    fn write(&self, resource: SyncResource, content: EditSessionContent) -> Promise<String> {
        let this = self.rc();
        async_fn(async move {
            this.initialize(InitializeReason::Write, false).await?;
            if !this.initialized.get() {
                return Err(Error::new("Please sign in to store your edit session."));
            }

            let content = match content {
                EditSessionContent::Text(content) => content,
                EditSessionContent::Session(mut session) => {
                    if session.machine.is_none() {
                        session.machine = Some(this.get_or_create_current_machine_id().await?);
                    }
                    json_stringify(&session.to_dynamic()).unwrap_or_default()
                }
            };
            let store_client = this.required_store_client("writeResource")?;
            let r#ref = store_client.write_resource(resource.into(), &content, None, None, Some(create_sync_headers(&generate_uuid()))).await?;

            this.last_written_resources.borrow_mut().insert(resource, EditSessionResource { r#ref: r#ref.clone(), content });

            Ok(r#ref)
        })
    }

    fn delete(&self, resource: SyncResource, r#ref: Option<String>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.initialize(InitializeReason::Write, false).await?;
            if !this.initialized.get() {
                return Err(Error::new(format!("Unable to delete edit session with ref {}.", r#ref.as_deref().unwrap_or("null"))));
            }

            let store_client = this.store_client.borrow().clone();
            if let Some(store_client) = store_client
                && let Err(ex) = store_client.delete_resource(resource.into(), r#ref.as_deref(), None).await
            {
                this.log_error(&ex);
            }
            Ok(())
        })
    }

    fn list(&self, resource: SyncResource) -> Promise<Vec<IResourceRefHandle>> {
        let this = self.rc();
        async_fn(async move {
            this.initialize(InitializeReason::Read, false).await?;
            if !this.initialized.get() {
                return Err(Error::new("Unable to list edit sessions."));
            }

            let store_client = this.store_client.borrow().clone();
            let listed = match store_client {
                Some(store_client) => errors::catch(|| store_client.get_all_resource_refs(resource.into(), None)),
                None => Ok(Promise::resolved(Vec::new())),
            };
            match listed {
                Ok(refs) => Ok(refs.await?),
                Err(ex) => {
                    this.log_error(&ex);
                    Ok(Vec::new())
                }
            }
        })
    }

    fn get_machine_by_id(&self, machine_id: &str) -> Promise<Option<String>> {
        let this = self.rc();
        let machine_id = machine_id.to_string();
        async_fn(async move {
            this.initialize(InitializeReason::Read, false).await?;

            if this.cached_machines.borrow().is_none() {
                let machines = this.required_machine_client("getMachines")?.get_machines(None).await?;
                *this.cached_machines.borrow_mut() = Some(machines.into_iter().map(|machine| (machine.id, machine.name)).collect());
            }

            Ok(this.cached_machines.borrow().as_ref().and_then(|machines| machines.get(&machine_id).cloned()))
        })
    }
}

impl IDisposable for EditSessionsWorkbenchService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
