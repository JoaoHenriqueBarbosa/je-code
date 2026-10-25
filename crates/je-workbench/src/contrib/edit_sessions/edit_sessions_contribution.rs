//! Porte de `src/vs/workbench/contrib/editSessions/browser/editSessions.contribution.ts` (linhas 78 a 884): a
//! `EditSessionsContribution`, que liga o Cloud Changes ao workbench. Ela retoma a edit session guardada (ao abrir a janela, por
//! `editSessionId` ou pelo identificador do workspace), guarda as mudanças de trabalho do SCM (a pedido, ao fechar a janela ou ao
//! continuar em outro ambiente), mostra o selo no menu de contas e registra os comandos, a vista e o provedor de arquivos do
//! esquema `vscode-edit-sessions`. Também estão aqui as ações do módulo (`installAdditionalContinueOnOptionsCommand`).
//!
//! Fica para a fatia 2c (linhas 886 em diante): `registerContributedEditSessionOptions`, `generateStandaloneOptionCommand`,
//! `registerContinueInLocalFolderAction`, `pickContinueEditSessionDestination`, `resolveDestination`, `createPickItems`, a classe
//! `ContinueEditSessionItem`, o ponto de extensão `continueEditSession`, o registro da contribuição e das configurações. Elas
//! são o que a contribuição chama por [`ContinueEditSessionDestinations`] (ver abaixo).
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são os campos de [`EditSessionsContributionServices`] (menos
//!   `IInstantiationService`, `ICommandService`, `IFileDialogService` e `IExtensionService`, que só as peças da 2c leem, e
//!   `IOutputService` e `IViewsService`, que as ações leem do `accessor`, como no original). O `createInstance(EditSessionsDataViews)`
//!   é o construtor dela com os serviços da árvore em `tree_view_services`.
//! - **A parte do mesmo `class` que está na 2c** é o trait [`ContinueEditSessionDestinations`]: a 2c o implementa sobre o estado que
//!   o original guarda na própria contribuição (`continueEditSessionOptions`, `registeredCommands`) e ele chega em
//!   [`EditSessionsContributionServices::continue_on`]. O construtor chama os mesmos métodos nos mesmos pontos
//!   (`registerContinueInLocalFolderAction` dentro de `registerActions`, `registerContributedEditSessionOptions` depois das vistas),
//!   e o descarte do colaborador fica com a contribuição (o `_register` do original).
//! - **Classe `Disposable`** é o campo `base`, e a contribuição fica num `Rc` com uma referência fraca a si (`me`), que os
//!   ouvintes, as promessas e as ações usam (as ações guardam só a fraca: o registro de ações vive até o descarte). As classes
//!   anônimas de `registerAction2` são [`FnAction`]. O `Action2::run` do porte não devolve valor, então a promessa do `run`
//!   assíncrono do original é disparada e descartada.
//! - **`async`** é `async_fn`: o corpo roda já, até o primeiro `await`, e um `throw` vira a promessa rejeitada. O
//!   `try { ... } catch { ... }` de `resumeEditSession` é um bloco `async` cujo `Result` o `match` seguinte trata.
//! - **`withProgress` devolve `Promise<void>`** no porte, e o original devolve o valor da tarefa (a `ref` de `continueEditSession`):
//!   a tarefa deixa o valor numa célula compartilhada, que a ação lê depois. `async (progress) => ...` recebe o `progress` do serviço,
//!   e `onDidCancel` recebe o índice do botão.
//! - **`environmentService.editSessionId` e `continueOn`** são lidos uma vez (no construtor) para os campos `edit_session_id` e
//!   `continue_on` da contribuição, que são quem o original escreve (`= undefined`): o trait do ambiente é só de leitura, e só esta
//!   contribuição lê e escreve esses dois valores no processo da janela.
//! - **`IEditSessionsStorageService.lastReadResources.set(...)`** é o `borrow_mut().insert(...)` do mapa compartilhado.
//! - **`Set<URI>` de `getChangedResources`** compara por identidade (o `Set` do JavaScript), como o original: [`URI::ptr_eq`]. O
//!   `Set<string>` de `generateChanges` e o `ResourceSet` de `storeEditSession` comparam por valor.
//! - **Argumentos de comando** (`workspaceUri`, `destination`, `editSessionId`, `forceApplyUnrelatedChange`) chegam como [`Dynamic`]:
//!   o que não tem o tipo esperado é `undefined`, e o URI é o `Uri` de `ApiValue`.
//! - **`editSessionsStorageClient`** (campo do original, só escrito no construtor) é uma variável local.
//! - **`productService['editSessions.store']?.url`** é o [`IProductService::edit_sessions_store`] com `url` não vazia.
//! - **Telemetria.** `publicLog2<E, C>(name, data)` é `public_log2(name, Some(objeto))`; a classificação é só tipo.
//! - **Texto.** `${ref}` e as demais interpolações de valor ausente usam o texto `undefined`, como o JavaScript.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::buffer::encode_base64;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::hash::{HashAsyncInput, hash_async};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::nls::localize;
use je_base::performance::mark;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange, encode_uri_component};
use je_platform::action::{Category, CommandAction, Title, localize2};
use je_platform::actions::actions::Action2Options;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::{IContextKey, IContextKeyService};
use je_platform::dialogs::{BaseDialogOptions, Confirmation, DialogDetail, DialogTypeOrSeverity, FileNameOrResource, IDialogService, get_file_names_message};
use je_platform::edit_sessions::{EditSessionIdentityMatch, IEditSessionIdentityService};
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{LogArg, LogMessage, LogServiceRef};
use je_platform::notification::notification_message::{NotificationPart, NotificationText};
use je_platform::notification::{BasePromptChoice, INotificationService, NotificationMessage, PromptChoice, PromptChoiceKind, Severity};
use je_platform::opener::{IOpenerService, OpenOptions, OpenResource};
use je_platform::product::IProductService;
use je_platform::progress::progress::{IProgress, IProgressService, ProgressCancellable, ProgressLocation, ProgressOptions, ProgressStep, ProgressTask, ProgressType};
use je_platform::quickinput::quick_input::{IInputOptions, IQuickInputService, IQuickItem, IQuickPickItem, QuickPickInput, QuickPickOk};
use je_platform::request::IRequestService;
use je_platform::resources::{ext_uri, join_path};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::ITelemetryService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::user_data_profile::IUserDataProfilesService;
use je_platform::user_data_sync::user_data_sync::{UserDataSyncErrorCode, UserDataSyncStoreError};
use je_platform::user_data_sync::user_data_sync_store_service::UserDataSyncStoreClientServices;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState, WorkspaceFolder};
use je_storage::StorageValue;

use super::common::{
    Change, EDIT_SESSIONS_CONTAINER_ID, EDIT_SESSIONS_DATA_VIEW_ID, EDIT_SESSIONS_LOG_ID, EDIT_SESSION_SCHEMA_VERSION, EditSession, EditSessionContent, EditSessionResource, FileType, FnAction,
    Folder, IEditSessionsLogService, IEditSessionsStorageService, InitializeReason, SyncResource, action2_options, command_title, decode_edit_session_file_content,
    edit_session_sync_category, edit_sessions_pending, edit_sessions_show_view, edit_sessions_view_icon, hashed_edit_session_id,
};
use super::edit_sessions_file_system_provider::EditSessionsFileSystemProvider;
use super::edit_sessions_storage_client::EditSessionsStoreClient;
use super::edit_sessions_views::{EditSessionsDataViews, EditSessionsViewsServices, edit_sessions_nls_title};
use super::workspace_state_sync::{WorkspaceStateSynchroniser, WorkspaceStateSynchroniserServices};
use crate::api::common::ext_host_values::ApiValue;
use crate::browser::parts::views::tree_view::CustomTreeViewServices;
use crate::common::contextkeys::workspace_folder_count_context;
use crate::common::contributions::IWorkbenchContribution;
use crate::common::views::{IViewContainerDescriptor, RegisterViewContainerOptions, ViewContainerLocation, ViewIcon, view_containers_registry};
use crate::contrib::extensions::extension_recommendation_notification_service::IExtensionsOpener;
use crate::contrib::scm::common::scm::{ISCMRepository, ISCMService};
use crate::services::activity::activity::{IActivity, IActivityService, NumberBadge};
use crate::services::editor::editor_service::IEditorService;
use crate::services::lifecycle::lifecycle::{ILifecycleService, ShutdownReason, WillShutdownEventJoiner};
use crate::services::output::output::I_OUTPUT_SERVICE;
use crate::services::remote::remote_agent_service::IRemoteAgentService;
use crate::services::views::views_service::I_VIEWS_SERVICE;
use crate::services::workspaces::common::workspace_identity_service::IWorkspaceIdentityService;

/// `continueWorkingOnCommand.id`.
pub const CONTINUE_WORKING_ON_COMMAND_ID: &str = "_workbench.editSessions.actions.continueEditSession";
/// `showOutputChannelCommand.id`.
pub const SHOW_OUTPUT_CHANNEL_COMMAND_ID: &str = "workbench.editSessions.actions.showOutputChannel";
/// `installAdditionalContinueOnOptionsCommand.id`.
pub const INSTALL_ADDITIONAL_CONTINUE_ON_OPTIONS_COMMAND_ID: &str = "workbench.action.continueOn.extensions";
/// `queryParamName`.
const QUERY_PARAM_NAME: &str = "editSessionId";
/// `useEditSessionsWithContinueOn`.
const USE_EDIT_SESSIONS_WITH_CONTINUE_ON: &str = "workbench.editSessions.continueOn";
/// `EditSessionsContribution.APPLICATION_LAUNCHED_VIA_CONTINUE_ON_STORAGE_KEY`.
const APPLICATION_LAUNCHED_VIA_CONTINUE_ON_STORAGE_KEY: &str = "applicationLaunchedViaContinueOn";

/// `installAdditionalContinueOnOptionsCommand.title`.
pub fn install_additional_continue_on_options_title() -> String {
    localize("continueOn.installAdditional", "Install additional development environment options", &[])
}

/// `continueWorkingOnCommand`.
fn continue_working_on_options() -> Action2Options {
    action2_options(
        CommandAction {
            precondition: Some(workspace_folder_count_context().not_equals_to("0")),
            ..CommandAction::new(CONTINUE_WORKING_ON_COMMAND_ID, command_title(localize2("continue working on", "Continue Working On...")))
        },
        true,
    )
}

/// `showOutputChannelCommand`.
fn show_output_channel_options() -> Action2Options {
    action2_options(
        CommandAction {
            category: Some(Category::Localized(edit_session_sync_category())),
            ..CommandAction::new(SHOW_OUTPUT_CHANNEL_COMMAND_ID, command_title(localize2("show log", "Show Log")))
        },
        false,
    )
}

/// O `registerAction2` do módulo: o comando `workbench.action.continueOn.extensions`, que abre a busca de extensões com
/// `@tag:continueOn`. O `accessor.get(IExtensionsWorkbenchService).openSearch(...)` é o [`IExtensionsOpener::open_search`] (o
/// serviço do porte não abre a vista, ver `extension_recommendation_notification_service`).
pub fn register_install_additional_continue_on_options_action(extensions_opener: Rc<dyn IExtensionsOpener>) -> DisposableRef {
    FnAction::register(
        action2_options(CommandAction::new(INSTALL_ADDITIONAL_CONTINUE_ON_OPTIONS_COMMAND_ID, Title::Plain(install_additional_continue_on_options_title())), false),
        move |_accessor, _args| extensions_opener.open_search("@tag:continueOn"),
    )
}

/// `resumeProgressOptionsTitle`.
fn resume_progress_options_title() -> NotificationText {
    NotificationText::link(&localize("resuming working changes window", "Resuming working changes...", &[]), &format!("command:{SHOW_OUTPUT_CHANNEL_COMMAND_ID}"), None)
}

/// `resumeProgressOptions`.
fn resume_progress_options() -> ProgressOptions {
    ProgressOptions { kind: Some(ProgressType::Syncing), ..ProgressOptions::new(ProgressLocation::Window) }
}

/// `{ ...resumeProgressOptions, title: resumeProgressOptionsTitle }`.
fn resume_progress_options_with_title() -> ProgressOptions {
    ProgressOptions { title: Some(NotificationPart::Rich(resume_progress_options_title())), ..resume_progress_options() }
}

/// O destino que `resolveDestination` devolve: um URI, ou `'noDestinationUri'` (o comando escolhido cuida da abertura).
#[derive(Clone, Debug)]
pub enum ContinueDestination {
    Uri(URI),
    NoDestinationUri,
}

/// A parte da `EditSessionsContribution` que o original declara depois do `//#region Continue Edit Session extension contribution
/// point` (fatia 2c): as opções contribuídas por extensões e a escolha do destino do "Continue Working On...". É descartada junto
/// com a contribuição.
pub trait ContinueEditSessionDestinations: IDisposable {
    /// `registerContributedEditSessionOptions()`.
    fn register_contributed_edit_session_options(&self);

    /// `registerContinueInLocalFolderAction()`.
    fn register_continue_in_local_folder_action(&self);

    /// `pickContinueEditSessionDestination()`: o comando escolhido, ou `None` se o usuário não escolheu.
    fn pick_continue_edit_session_destination(&self) -> Promise<Option<String>>;

    /// `resolveDestination(command)`: o destino do comando, ou `None` se ele é inválido ou foi cancelado.
    fn resolve_destination(&self, command: &str) -> Promise<Option<ContinueDestination>>;
}

/// Os serviços que a `EditSessionsContribution` recebe por injeção.
#[derive(Clone)]
pub struct EditSessionsContributionServices {
    pub edit_sessions_storage_service: Rc<dyn IEditSessionsStorageService>,
    pub file_service: Rc<dyn IFileService>,
    pub progress_service: Rc<dyn IProgressService>,
    pub opener_service: Rc<dyn IOpenerService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub scm_service: Rc<dyn ISCMService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub dialog_service: Rc<dyn IDialogService>,
    pub log_service: Rc<dyn IEditSessionsLogService>,
    pub environment_service: Rc<dyn EnvironmentService>,
    pub product_service: Rc<dyn IProductService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub edit_session_identity_service: Rc<dyn IEditSessionIdentityService>,
    pub quick_input_service: Rc<dyn IQuickInputService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub storage_service: Rc<dyn StorageService>,
    pub activity_service: Rc<dyn IActivityService>,
    pub editor_service: Rc<dyn IEditorService>,
    pub remote_agent_service: Rc<dyn IRemoteAgentService>,
    pub request_service: Rc<dyn IRequestService>,
    pub user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub workspace_identity_service: Rc<dyn IWorkspaceIdentityService>,
    /// Os serviços do `TreeView` da vista Cloud Changes.
    pub tree_view_services: CustomTreeViewServices,
    /// A parte da fatia 2c (ver [`ContinueEditSessionDestinations`]).
    pub continue_on: Rc<dyn ContinueEditSessionDestinations>,
}

/// Os argumentos de `resumeEditSession(ref?, silent?, forceApplyUnrelatedChange?, applyPartialMatch?, progress?, serializedData?)`,
/// menos o `progress`. Os booleanos ausentes são `false`, que é o que o original lê (`silent &&` e os valores padrão).
#[derive(Clone, Debug, Default)]
pub struct ResumeEditSessionArgs {
    pub r#ref: Option<String>,
    pub silent: bool,
    pub force_apply_unrelated_change: bool,
    pub apply_partial_match: bool,
    pub serialized_data: Option<String>,
}

/// O `{ uri, type, contents }` de `generateChanges`: onde a mudança se aplica e a mudança.
#[derive(Clone)]
struct IncomingChange {
    uri: URI,
    change: Change,
}

/// O `{ changes, conflictingChanges }` de `generateChanges`.
#[derive(Clone, Default)]
struct GeneratedChanges {
    changes: Vec<IncomingChange>,
    conflicting_changes: Vec<IncomingChange>,
}

/// `getChangedResources(repository)`: as URIs de todos os grupos de recursos do repositório (uma URI pode estar em mais de um
/// grupo, e o `Set` compara por identidade).
fn get_changed_resources(repository: &dyn ISCMRepository) -> Vec<URI> {
    let mut resources: Vec<URI> = Vec::new();
    for resource_group in repository.provider().groups() {
        for resource in resource_group.resources() {
            let source_uri = resource.source_uri();
            if !resources.iter().any(|existing| URI::ptr_eq(existing, &source_uri)) {
                resources.push(source_uri);
            }
        }
    }
    resources
}

/// O `onDidCancel` de `withProgress` que cancela e descarta a fonte de cancelamento e roda `then`.
fn cancel_and_dispose(cancellation_token_source: CancellationTokenSource, then: impl Fn() + 'static) -> Rc<dyn Fn(Option<i32>)> {
    Rc::new(move |_| {
        cancellation_token_source.cancel();
        cancellation_token_source.dispose();
        then();
    })
}

/// O argumento de texto de um comando (`undefined` se não é texto).
fn string_arg(args: &[Dynamic], index: usize) -> Option<String> {
    match args.get(index) {
        Some(Dynamic::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// O argumento booleano de um comando (`undefined`, `false`, é `false`).
fn bool_arg(args: &[Dynamic], index: usize) -> bool {
    matches!(args.get(index), Some(Dynamic::Bool(true)))
}

/// `EditSessionsContribution`.
pub struct EditSessionsContribution {
    me: Weak<Self>,
    base: Rc<Disposable>,
    services: EditSessionsContributionServices,

    should_show_views_context: Rc<dyn IContextKey>,
    pending_edit_sessions_context: Rc<dyn IContextKey>,
    accounts_menu_badge: Rc<MutableDisposable<dyn IDisposable>>,

    workspace_state_synchronizer: Option<Rc<WorkspaceStateSynchroniser>>,

    /// `environmentService.editSessionId` e `environmentService.continueOn`, que a contribuição consome (ver as divergências).
    edit_session_id: RefCell<Option<String>>,
    continue_on: RefCell<Option<String>>,
}

impl EditSessionsContribution {
    pub fn new(services: EditSessionsContributionServices) -> Rc<Self> {
        let base = Disposable::new();
        let accounts_menu_badge: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        base.register(accounts_menu_badge.clone());
        let continue_on_destinations: DisposableRef = services.continue_on.clone();
        base.register(continue_on_destinations);

        let should_show_views_context = edit_sessions_show_view().bind_to(services.context_key_service.as_ref());
        let pending_edit_sessions_context = edit_sessions_pending().bind_to(services.context_key_service.as_ref());
        pending_edit_sessions_context.set(Dynamic::Bool(false));

        let store_url = services.product_service.edit_sessions_store().map(|store| store.url.clone()).filter(|url| !url.is_empty());
        let workspace_state_synchronizer = store_url.as_deref().map(|url| Self::create_workspace_state_synchroniser(&services, url));
        let edit_session_id = services.environment_service.edit_session_id();
        let continue_on = services.environment_service.continue_on();

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            services,
            should_show_views_context,
            pending_edit_sessions_context,
            accounts_menu_badge,
            workspace_state_synchronizer,
            edit_session_id: RefCell::new(edit_session_id),
            continue_on: RefCell::new(continue_on),
        });

        if store_url.is_none() {
            return this;
        }

        let _ = this.auto_resume_edit_session();

        this.register_actions();
        this.register_views();
        this.services.continue_on.register_contributed_edit_session_options();

        match this.services.file_service.register_provider(EditSessionsFileSystemProvider::SCHEMA, EditSessionsFileSystemProvider::new(this.services.edit_sessions_storage_service.clone())) {
            Ok(registration) => {
                this.base.register(registration);
            }
            Err(error) => errors::throw(error),
        }

        let weak = this.me.clone();
        this.base.register(this.services.lifecycle_service.on_will_shutdown().listen(move |event| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            if event.reason != ShutdownReason::Reload
                && this.services.edit_sessions_storage_service.is_signed_in()
                && this.config_text("workbench.experimental.cloudChanges.autoStore").as_deref() == Some("onShutdown")
                && !Platform::detect().is_web
            {
                event.join(
                    this.auto_store_edit_session(),
                    WillShutdownEventJoiner {
                        id: "autoStoreWorkingChanges".to_string(),
                        label: localize("autoStoreWorkingChanges", "Storing current working changes...", &[]),
                        order: None,
                    },
                );
            }
        }));

        let weak = this.me.clone();
        this.base.register(this.services.edit_sessions_storage_service.on_did_sign_in().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_accounts_menu_badge();
            }
        }));
        let weak = this.me.clone();
        this.base.register(this.services.edit_sessions_storage_service.on_did_sign_out().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_accounts_menu_badge();
            }
        }));

        this
    }

    /// O `new EditSessionsStoreClient(...)` e o `new WorkspaceStateSynchroniser(...)` do construtor.
    fn create_workspace_state_synchroniser(services: &EditSessionsContributionServices, url: &str) -> Rc<WorkspaceStateSynchroniser> {
        let log_service: LogServiceRef = services.log_service.clone();
        let edit_sessions_storage_client = EditSessionsStoreClient::new(
            Some(&URI::parse(url, false)),
            UserDataSyncStoreClientServices {
                product_service: services.product_service.clone(),
                request_service: services.request_service.clone(),
                log_service: log_service.clone(),
                environment_service: services.environment_service.clone(),
                file_service: services.file_service.clone(),
                storage_service: services.storage_service.clone(),
            },
        );
        services.edit_sessions_storage_service.set_store_client(Some(edit_sessions_storage_client.clone()));
        WorkspaceStateSynchroniser::new(
            services.user_data_profiles_service.default_profile(),
            None,
            edit_sessions_storage_client,
            log_service,
            WorkspaceStateSynchroniserServices {
                file_service: services.file_service.clone(),
                environment_service: services.environment_service.clone(),
                telemetry_service: services.telemetry_service.clone(),
                configuration_service: services.configuration_service.clone(),
                storage_service: services.storage_service.clone(),
                uri_identity_service: services.uri_identity_service.clone(),
                workspace_identity_service: services.workspace_identity_service.clone(),
                edit_sessions_storage_service: services.edit_sessions_storage_service.clone(),
            },
        )
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("a contribuição está viva enquanto é usada")
    }

    /// `this.configurationService.getValue(section)` quando o valor é um texto.
    fn config_text(&self, section: &str) -> Option<String> {
        match self.services.configuration_service.get_value(Some(section), None) {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        }
    }

    /// `this.configurationService.getValue(section) === true`.
    fn config_is_true(&self, section: &str) -> bool {
        matches!(self.services.configuration_service.get_value(Some(section), None), Dynamic::Bool(true))
    }

    /// `this.telemetryService.publicLog2(eventName, data)`.
    fn log_telemetry(&self, event_name: &str, data: &[(&str, &str)]) {
        let data = (!data.is_empty()).then(|| Dynamic::object(data.iter().map(|(key, value)| (*key, Dynamic::string(value)))));
        self.services.telemetry_service.public_log2(event_name, data);
    }

    /// `this.notificationService.error(message)`.
    fn notify_error(&self, message: String) {
        self.services.notification_service.error(vec![NotificationMessage::from(message)]);
    }

    /// `withProgress(options, async (progress) => await this.resumeEditSession(...))`: `forward_progress` é se o `progress` vai
    /// para a retomada (as chamadas do original que usam `async () =>` não o passam).
    fn with_resume_progress(&self, options: ProgressOptions, args: ResumeEditSessionArgs, forward_progress: bool) -> Promise<()> {
        let this = self.rc();
        let task: ProgressTask = Box::new(move |progress| this.resume_edit_session(args, forward_progress.then_some(progress)));
        self.services.progress_service.with_progress(options, task, None)
    }

    fn auto_resume_edit_session(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let should_auto_resume_on_reload = this.config_text("workbench.cloudChanges.autoResume").as_deref() == Some("onReload");
            let edit_session_id = this.edit_session_id.borrow().clone();

            if let Some(edit_session_id) = edit_session_id {
                this.services.log_service.info(&format!("Resuming cloud changes, reason: found editSessionId {edit_session_id} in environment service..."));
                let task_this = this.clone();
                let task: ProgressTask = Box::new(move |progress| {
                    let current_edit_session_id = task_this.edit_session_id.borrow().clone();
                    let cleared = task_this.clone();
                    task_this
                        .resume_edit_session(ResumeEditSessionArgs { r#ref: current_edit_session_id, ..ResumeEditSessionArgs::default() }, Some(progress))
                        .finally(move || *cleared.edit_session_id.borrow_mut() = None)
                });
                this.services.progress_service.with_progress(resume_progress_options(), task, None).await?;
            } else if should_auto_resume_on_reload && this.services.edit_sessions_storage_service.is_signed_in() {
                this.services.log_service.info("Resuming cloud changes, reason: cloud changes enabled...");
                // Attempt to resume edit session based on edit workspace identifier
                // Note: at this point if the user is not signed into edit sessions,
                // we don't want them to be prompted to sign in and should just return early
                this.with_resume_progress(resume_progress_options(), ResumeEditSessionArgs { silent: true, ..ResumeEditSessionArgs::default() }, true).await?;
            } else if should_auto_resume_on_reload {
                // The application has previously launched via a protocol URL Continue On flow
                let has_application_launched_from_continue_on_flow =
                    this.services.storage_service.get_boolean(APPLICATION_LAUNCHED_VIA_CONTINUE_ON_STORAGE_KEY, StorageScope::Application, Some(false)).unwrap_or(false);
                this.services.log_service.info(&format!(
                    "Prompting to enable cloud changes, has application previously launched from Continue On flow: {has_application_launched_from_continue_on_flow}"
                ));

                if this.continue_on.borrow().is_some()
                    && !this.services.edit_sessions_storage_service.is_signed_in()
                    // and user has not yet been prompted to sign in on this machine
                    && !has_application_launched_from_continue_on_flow
                {
                    // store the fact that we prompted the user
                    this.services.storage_service.store(APPLICATION_LAUNCHED_VIA_CONTINUE_ON_STORAGE_KEY, StorageValue::from(true), StorageScope::Application, StorageTarget::Machine);
                    this.services.log_service.info("Prompting to enable cloud changes...");
                    this.services.edit_sessions_storage_service.initialize(InitializeReason::Read, false).await?;
                    if this.services.edit_sessions_storage_service.is_signed_in() {
                        this.services.log_service.info("Prompting to enable cloud changes succeeded, resuming cloud changes...");
                        this.with_resume_progress(resume_progress_options(), ResumeEditSessionArgs { silent: true, ..ResumeEditSessionArgs::default() }, true).await?;
                    } else {
                        this.handle_pending_edit_sessions();
                    }
                } else if !this.services.edit_sessions_storage_service.is_signed_in()
                    // and user has been prompted to sign in on this machine
                    && has_application_launched_from_continue_on_flow
                {
                    this.handle_pending_edit_sessions();
                }
            } else {
                this.services.log_service.debug("Auto resuming cloud changes disabled.");
            }
            Ok(())
        })
    }

    /// `handlePendingEditSessions`, o fecho de `autoResumeEditSession`.
    fn handle_pending_edit_sessions(&self) {
        // display a badge in the accounts menu but do not prompt the user to sign in again
        self.services.log_service.info("Showing badge to enable cloud changes in accounts menu...");
        self.update_accounts_menu_badge();
        self.pending_edit_sessions_context.set(Dynamic::Bool(true));
        // attempt a resume if we are in a pending state and the user just signed in
        let weak = self.me.clone();
        self.services.edit_sessions_storage_service.on_did_sign_in().once().listen(move |_| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            let _ = async_fn(async move {
                this.services.log_service.info("Showing badge to enable cloud changes in accounts menu succeeded, resuming cloud changes...");
                this.with_resume_progress(resume_progress_options(), ResumeEditSessionArgs { silent: true, ..ResumeEditSessionArgs::default() }, true).await?;
                this.services.storage_service.remove(APPLICATION_LAUNCHED_VIA_CONTINUE_ON_STORAGE_KEY, StorageScope::Application);
                *this.continue_on.borrow_mut() = None;
                Ok(())
            });
        });
    }

    fn update_accounts_menu_badge(&self) {
        if self.services.edit_sessions_storage_service.is_signed_in() {
            return self.accounts_menu_badge.clear();
        }

        let badge = NumberBadge::new(1.0, |_| localize("check for pending cloud changes", "Check for pending cloud changes", &[]));
        self.accounts_menu_badge.set(Some(self.services.activity_service.show_accounts_activity(IActivity { badge: Rc::new(badge) })));
    }

    fn auto_store_edit_session(&self) -> Promise<()> {
        let this = self.rc();
        let cancellation_token_source = CancellationTokenSource::new(None);
        let token = cancellation_token_source.token();
        let task: ProgressTask = Box::new(move |_progress| this.store_edit_session(false, token).then(|_| ()));
        let options = ProgressOptions {
            kind: Some(ProgressType::Syncing),
            title: Some(NotificationPart::from(localize("store working changes", "Storing working changes...", &[]))),
            ..ProgressOptions::new(ProgressLocation::Window)
        };
        self.services.progress_service.with_progress(options, task, Some(cancel_and_dispose(cancellation_token_source, || {})))
    }

    fn register_views(&self) {
        let container = view_containers_registry().register_view_container(
            // `ctorDescriptor: new SyncDescriptor(ViewPaneContainer, [...])`: o `ViewPaneContainer` ainda não foi portado.
            IViewContainerDescriptor {
                icon: Some(ViewIcon::Theme(edit_sessions_view_icon())),
                hide_if_empty: true,
                ..IViewContainerDescriptor::new(EDIT_SESSIONS_CONTAINER_ID, edit_sessions_nls_title())
            },
            ViewContainerLocation::Sidebar,
            Some(RegisterViewContainerOptions { is_default: false, do_not_register_open_command: true }),
        );
        self.base.register(EditSessionsDataViews::new(
            &container,
            EditSessionsViewsServices {
                edit_sessions_storage_service: self.services.edit_sessions_storage_service.clone(),
                context_key_service: self.services.context_key_service.clone(),
                workspace_context_service: self.services.context_service.clone(),
                file_service: self.services.file_service.clone(),
                tree_view_services: self.services.tree_view_services.clone(),
            },
        ));
    }

    fn register_actions(&self) {
        self.register_continue_edit_session_action();

        self.register_resume_latest_edit_session_action();
        self.register_store_latest_edit_session_action();

        self.services.continue_on.register_continue_in_local_folder_action();

        self.register_show_edit_session_view_action();
        self.register_show_edit_session_output_channel_action();
    }

    fn register_show_edit_session_output_channel_action(&self) {
        self.base.register(FnAction::register(show_output_channel_options(), |accessor, _args| {
            let output_channel = accessor.get(I_OUTPUT_SERVICE);
            let _ = output_channel.show_channel(EDIT_SESSIONS_LOG_ID, false);
        }));
    }

    fn register_show_edit_session_view_action(&self) {
        let weak = self.me.clone();
        self.base.register(FnAction::register(
            action2_options(
                CommandAction {
                    category: Some(Category::Localized(edit_session_sync_category())),
                    ..CommandAction::new("workbench.editSessions.actions.showEditSessions", command_title(localize2("show cloud changes", "Show Cloud Changes")))
                },
                true,
            ),
            move |accessor, _args| {
                if let Some(that) = weak.upgrade() {
                    that.should_show_views_context.set(Dynamic::Bool(true));
                }
                let views_service = accessor.get(I_VIEWS_SERVICE);
                let _ = views_service.open_view(EDIT_SESSIONS_DATA_VIEW_ID, false);
            },
        ));
    }

    fn register_continue_edit_session_action(&self) {
        let weak = self.me.clone();
        self.base.register(FnAction::register(continue_working_on_options(), move |_accessor, args| {
            let workspace_uri = args.first().filter(|value| !value.is_nullish()).cloned();
            let destination = string_arg(args, 1);
            if let Some(that) = weak.upgrade() {
                let _ = that.continue_edit_session(workspace_uri, destination);
            }
        }));
    }

    /// O `run(accessor, workspaceUri, destination)` da ação `_workbench.editSessions.actions.continueEditSession`.
    fn continue_edit_session(&self, workspace_uri: Option<Dynamic>, destination: Option<String>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let mut uri: Option<ContinueDestination> = match workspace_uri {
                Some(workspace_uri) => Some(ContinueDestination::Uri(<URI as ApiValue>::from_dynamic(&workspace_uri)?)),
                None => None,
            };
            let mut destination = destination;

            // First ask the user to pick a destination, if necessary
            if destination.as_deref().is_none_or(str::is_empty) && uri.is_none() {
                destination = this.services.continue_on.pick_continue_edit_session_destination().await?;
                if destination.as_deref().is_none_or(str::is_empty) {
                    this.log_telemetry("continueOn.editSessions.pick.outcome", &[("outcome", "noSelection")]);
                    return Ok(());
                }
            }

            // Determine if we need to store an edit session, asking for edit session auth if necessary
            let should_store_edit_session = this.should_continue_on_with_edit_session().await?;

            // Run the store action to get back a ref
            let mut r#ref: Option<String> = None;
            if should_store_edit_session {
                this.log_telemetry("continueOn.editSessions.store", &[]);

                let cancellation_token_source = CancellationTokenSource::new(None);
                let stored_ref: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
                let task: ProgressTask = {
                    let (this, stored_ref, token) = (this.clone(), stored_ref.clone(), cancellation_token_source.token());
                    Box::new(move |_progress| {
                        let (outcome_this, stored_ref) = (this.clone(), stored_ref.clone());
                        this.store_edit_session(false, token).then(move |stored| {
                            match &stored {
                                Some(stored) => outcome_this.log_telemetry(
                                    "continueOn.editSessions.store.outcome",
                                    &[("outcome", "storeSucceeded"), ("hashedId", hashed_edit_session_id(stored).as_str())],
                                ),
                                None => outcome_this.log_telemetry("continueOn.editSessions.store.outcome", &[("outcome", "storeSkipped")]),
                            }
                            *stored_ref.borrow_mut() = stored;
                        })
                    })
                };
                let on_did_cancel = {
                    let this = this.clone();
                    cancel_and_dispose(cancellation_token_source, move || this.log_telemetry("continueOn.editSessions.store.outcome", &[("outcome", "storeCancelledByUser")]))
                };
                let options = ProgressOptions {
                    cancellable: Some(ProgressCancellable::Flag(true)),
                    kind: Some(ProgressType::Syncing),
                    title: Some(NotificationPart::from(localize("store your working changes", "Storing your working changes...", &[]))),
                    ..ProgressOptions::new(ProgressLocation::Notification)
                };
                if let Err(error) = this.services.progress_service.with_progress(options, task, Some(on_did_cancel)).await {
                    this.log_telemetry("continueOn.editSessions.store.outcome", &[("outcome", "storeFailed")]);
                    return Err(error);
                }
                r#ref = stored_ref.borrow_mut().take();
            }

            // Append the ref to the URI
            if let Some(destination) = destination.as_deref().filter(|destination| !destination.is_empty()) {
                uri = this.services.continue_on.resolve_destination(destination).await?;
            }
            let Some(uri) = uri else {
                return Ok(());
            };
            let destination_uri = match &uri {
                ContinueDestination::Uri(destination_uri) => Some(destination_uri.clone()),
                ContinueDestination::NoDestinationUri => None,
            };

            if let (Some(r#ref), Some(destination_uri)) = (&r#ref, &destination_uri) {
                let encoded_ref = String::from_utf16_lossy(&encode_uri_component(&r#ref.encode_utf16().collect::<Vec<u16>>()));
                let query = destination_uri.query().to_string_lossy();
                let query = if !query.is_empty() { format!("{query}&{QUERY_PARAM_NAME}={encoded_ref}&continueOn=1") } else { format!("{QUERY_PARAM_NAME}={encoded_ref}&continueOn=1") };
                let destination_uri = destination_uri.with(&UriChange { query: Field::from(query.as_str()), ..UriChange::default() });

                // Open the URI
                this.services.log_service.info(&format!("Opening {}", destination_uri.to_string(false)));
                this.services.opener_service.open(&OpenResource::Uri(destination_uri), Some(&OpenOptions { open_external: Some(true), ..OpenOptions::default() })).await?;
            } else if (!should_store_edit_session || r#ref.is_none()) && destination_uri.is_some() {
                // Open the URI without an edit session ref
                if let Some(destination_uri) = destination_uri {
                    this.services.log_service.info(&format!("Opening {}", destination_uri.to_string(false)));
                    this.services.opener_service.open(&OpenResource::Uri(destination_uri), Some(&OpenOptions { open_external: Some(true), ..OpenOptions::default() })).await?;
                }
            } else if r#ref.is_none() && should_store_edit_session {
                this.services.log_service.warn(&format!("Failed to store working changes when invoking {CONTINUE_WORKING_ON_COMMAND_ID}."));
            }
            Ok(())
        })
    }

    fn register_resume_latest_edit_session_action(&self) {
        let weak = self.me.clone();
        self.base.register(FnAction::register(
            action2_options(
                CommandAction {
                    category: Some(Category::Localized(edit_session_sync_category())),
                    ..CommandAction::new("workbench.editSessions.actions.resumeLatest", command_title(localize2("resume latest cloud changes", "Resume Latest Changes from Cloud")))
                },
                true,
            ),
            move |_accessor, args| {
                let args = ResumeEditSessionArgs { r#ref: string_arg(args, 0), force_apply_unrelated_change: bool_arg(args, 1), ..ResumeEditSessionArgs::default() };
                if let Some(that) = weak.upgrade() {
                    let _ = that.with_resume_progress(resume_progress_options_with_title(), args, false);
                }
            },
        ));

        let weak = self.me.clone();
        self.base.register(FnAction::register(
            action2_options(
                CommandAction {
                    category: Some(Category::Text("Developer".to_string())),
                    ..CommandAction::new("workbench.editSessions.actions.resumeFromSerializedPayload", command_title(localize2("resume cloud changes", "Resume Changes from Serialized Data")))
                },
                true,
            ),
            move |_accessor, args| {
                let edit_session_id = string_arg(args, 0);
                if let Some(that) = weak.upgrade() {
                    let _ = that.resume_from_serialized_payload(edit_session_id);
                }
            },
        ));
    }

    /// O `run(accessor, editSessionId)` da ação `workbench.editSessions.actions.resumeFromSerializedPayload`.
    fn resume_from_serialized_payload(&self, edit_session_id: Option<String>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let data = this.services.quick_input_service.input(Some(&IInputOptions { prompt: Some("Enter serialized data".to_string()), ..IInputOptions::default() }), None).await?;
            if let Some(data) = data.as_ref().filter(|data| !data.is_empty()) {
                this.services.edit_sessions_storage_service.last_read_resources().borrow_mut().insert(SyncResource::EditSessions, EditSessionResource { content: data.clone(), r#ref: String::new() });
            }
            this.with_resume_progress(
                resume_progress_options_with_title(),
                ResumeEditSessionArgs { r#ref: edit_session_id, serialized_data: data, ..ResumeEditSessionArgs::default() },
                false,
            )
            .await
        })
    }

    fn register_store_latest_edit_session_action(&self) {
        let weak = self.me.clone();
        self.base.register(FnAction::register(
            action2_options(
                CommandAction {
                    category: Some(Category::Localized(edit_session_sync_category())),
                    ..CommandAction::new("workbench.editSessions.actions.storeCurrent", command_title(localize2("store working changes in cloud", "Store Working Changes in Cloud")))
                },
                true,
            ),
            move |_accessor, _args| {
                if let Some(that) = weak.upgrade() {
                    let _ = that.store_current();
                }
            },
        ));
    }

    /// O `run` da ação `workbench.editSessions.actions.storeCurrent`.
    fn store_current(&self) -> Promise<()> {
        let this = self.rc();
        let cancellation_token_source = CancellationTokenSource::new(None);
        let token = cancellation_token_source.token();
        let task: ProgressTask = Box::new(move |_progress| {
            this.log_telemetry("editSessions.store", &[]);
            this.store_edit_session(true, token).then(|_| ())
        });
        let options = ProgressOptions {
            title: Some(NotificationPart::from(localize("storing working changes", "Storing working changes...", &[]))),
            ..ProgressOptions::new(ProgressLocation::Notification)
        };
        self.services.progress_service.with_progress(options, task, Some(cancel_and_dispose(cancellation_token_source, || {})))
    }

    /// `resumeEditSession(ref?, silent?, forceApplyUnrelatedChange?, applyPartialMatch?, progress?, serializedData?)`.
    pub fn resume_edit_session(&self, args: ResumeEditSessionArgs, progress: Option<Rc<dyn IProgress<ProgressStep>>>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let ResumeEditSessionArgs { r#ref, silent, force_apply_unrelated_change, apply_partial_match, serialized_data } = args;

            // Wait for the remote environment to become available, if any
            this.services.remote_agent_service.get_environment().await?;

            // Edit sessions are not currently supported in empty workspaces
            // https://github.com/microsoft/vscode/issues/159220
            if this.services.context_service.get_workbench_state() == WorkbenchState::Empty {
                return Ok(());
            }

            this.services.log_service.info(&match &r#ref {
                Some(session_ref) => format!("Resuming changes from cloud with ref {session_ref}..."),
                None => "Checking for pending cloud changes...".to_string(),
            });

            if silent && !this.services.edit_sessions_storage_service.initialize(InitializeReason::Read, true).await? {
                return Ok(());
            }

            this.log_telemetry("editSessions.resume", &[]);

            mark("code/willResumeEditSessionFromIdentifier", None);

            if let Some(progress) = &progress {
                progress.report(ProgressStep { message: Some(NotificationPart::from(localize("checkingForWorkingChanges", "Checking for pending cloud changes...", &[]))), ..ProgressStep::default() });
            }
            let data = match serialized_data.filter(|serialized_data| !serialized_data.is_empty()) {
                Some(content) => Some(EditSessionResource { content, r#ref: String::new() }),
                None => this.services.edit_sessions_storage_service.read(SyncResource::EditSessions, r#ref.clone()).await?,
            };
            let Some(data) = data else {
                if r#ref.is_none() && !silent {
                    this.services.notification_service.info(vec![NotificationMessage::from(localize("no cloud changes", "There are no changes to resume from the cloud.", &[]))]);
                } else if let Some(r#ref) = &r#ref {
                    this.services
                        .notification_service
                        .warn(vec![NotificationMessage::from(localize("no cloud changes for ref", "Could not resume changes from the cloud for ID {0}.", &[r#ref.as_str()]))]);
                }
                this.services.log_service.info(&match &r#ref {
                    Some(session_ref) => format!("Aborting resuming changes from cloud as no edit session content is available to be applied from ref {session_ref}."),
                    None => "Aborting resuming edit session as no edit session content is available to be applied".to_string(),
                });
                return Ok(());
            };

            if let Some(progress) = &progress {
                progress.report(ProgressStep { message: Some(NotificationPart::Rich(resume_progress_options_title())), ..ProgressStep::default() });
            }
            let edit_session = EditSession::parse(&data.content)?;
            let r#ref = data.r#ref;

            if edit_session.version > EDIT_SESSION_SCHEMA_VERSION {
                this.notify_error(localize(
                    "client too old",
                    "Please upgrade to a newer version of {0} to resume your working changes from the cloud.",
                    &[this.services.product_service.name_long()],
                ));
                this.log_telemetry("editSessions.resume.outcome", &[("hashedId", hashed_edit_session_id(&r#ref).as_str()), ("outcome", "clientUpdateNeeded")]);
                return Ok(());
            }

            let outcome: Result<bool, Error> = async {
                let GeneratedChanges { changes, conflicting_changes } =
                    this.generate_changes(edit_session.clone(), r#ref.clone(), force_apply_unrelated_change, apply_partial_match).await?;
                if changes.is_empty() {
                    return Ok(false);
                }

                // Pendente do original: oferecer o diff dos arquivos que a edit session sobrescreveria
                if !conflicting_changes.is_empty() {
                    // Allow to show edit sessions

                    let many = conflicting_changes.len() > 1;
                    let message = if many {
                        localize(
                            "resume edit session warning many",
                            "Resuming your working changes from the cloud will overwrite the following {0} files. Do you want to proceed?",
                            &[&conflicting_changes.len().to_string()],
                        )
                    } else {
                        localize(
                            "resume edit session warning 1",
                            "Resuming your working changes from the cloud will overwrite {0}. Do you want to proceed?",
                            &[&ext_uri().basename(&conflicting_changes[0].uri, None)],
                        )
                    };
                    let detail = many.then(|| {
                        DialogDetail::Text(get_file_names_message(&conflicting_changes.iter().map(|conflicting| FileNameOrResource::Resource(conflicting.uri.clone())).collect::<Vec<_>>()))
                    });
                    let result = this
                        .services
                        .dialog_service
                        .confirm(Confirmation {
                            base: BaseDialogOptions { dialog_type: Some(DialogTypeOrSeverity::Severity(Severity::Warning)), detail, ..BaseDialogOptions::new(message) },
                            primary_button: None,
                            cancel_button: None,
                        })
                        .await?;

                    if !result.confirmed {
                        return Ok(false);
                    }
                }

                for IncomingChange { uri, change } in &changes {
                    match change {
                        Change::Addition { contents, .. } => {
                            let content = decode_edit_session_file_content(edit_session.version, contents)?;
                            this.services.file_service.write_file(uri, FileContentSource::from(content), None).await?;
                        }
                        Change::Deletion { .. } => {
                            if this.services.file_service.exists(uri).await? {
                                this.services.file_service.del(uri, None).await?;
                            }
                        }
                    }
                }

                if let Some(workspace_state_synchronizer) = &this.workspace_state_synchronizer {
                    workspace_state_synchronizer.apply().await?;
                }

                this.services.log_service.info(&format!("Deleting edit session with ref {} after successfully applying it to current workspace...", r#ref));
                this.services.edit_sessions_storage_service.delete(SyncResource::EditSessions, Some(r#ref.clone())).await?;
                this.services.log_service.info(&format!("Deleted edit session with ref {}.", r#ref));

                this.log_telemetry("editSessions.resume.outcome", &[("hashedId", hashed_edit_session_id(&r#ref).as_str()), ("outcome", "resumeSucceeded")]);
                Ok(true)
            }
            .await;
            match outcome {
                Ok(false) => return Ok(()),
                Ok(true) => {}
                Err(error) => {
                    this.services.log_service.error_args(&LogMessage::from("Failed to resume edit session, reason: "), &[LogArg::from(error.to_string().as_str())]);
                    this.notify_error(localize("resume failed", "Failed to resume your working changes from the cloud.", &[]));
                }
            }

            mark("code/didResumeEditSessionFromIdentifier", None);
            Ok(())
        })
    }

    fn generate_changes(&self, edit_session: EditSession, r#ref: String, force_apply_unrelated_change: bool, apply_partial_match: bool) -> Promise<GeneratedChanges> {
        let this = self.rc();
        async_fn(async move {
            let mut changes: Vec<IncomingChange> = Vec::new();
            let mut conflicting_changes: Vec<IncomingChange> = Vec::new();
            let workspace_folders = this.services.context_service.get_workspace().folders();
            let cancellation_token_source = CancellationTokenSource::new(None);
            let ext_uri = this.services.uri_identity_service.ext_uri();

            for folder in &edit_session.folders {
                let mut folder_root: Option<WorkspaceFolder> = None;

                match folder.canonical_identity.as_deref().filter(|canonical_identity| !canonical_identity.is_empty()) {
                    Some(canonical_identity) => {
                        // Look for an edit session identifier that we can use
                        for f in &workspace_folders {
                            let identity = this.services.edit_session_identity_service.get_edit_session_identifier(f, cancellation_token_source.token()).await?;
                            this.services
                                .log_service
                                .info(&format!("Matching identity {} against edit session folder identity {canonical_identity}...", identity.as_deref().unwrap_or("undefined")));

                            if identity.as_deref() == Some(canonical_identity) || force_apply_unrelated_change {
                                folder_root = Some(f.clone());
                                break;
                            }

                            if let Some(identity) = &identity {
                                let identity_match = this
                                    .services
                                    .edit_session_identity_service
                                    .provide_edit_session_identity_match(f, identity, canonical_identity, cancellation_token_source.token())
                                    .await?;
                                if identity_match == Some(EditSessionIdentityMatch::Complete) {
                                    folder_root = Some(f.clone());
                                    break;
                                } else if identity_match == Some(EditSessionIdentityMatch::Partial) && this.config_is_true("workbench.experimental.cloudChanges.partialMatches.enabled") {
                                    if !apply_partial_match {
                                        // Surface partially matching edit session
                                        let (weak, partial_ref) = (this.me.clone(), r#ref.clone());
                                        let _ = this.services.notification_service.prompt(
                                            Severity::Info,
                                            NotificationMessage::from(localize(
                                                "editSessionPartialMatch",
                                                "You have pending working changes in the cloud for this workspace. Would you like to resume them?",
                                                &[],
                                            )),
                                            vec![PromptChoiceKind::Plain(PromptChoice {
                                                base: BasePromptChoice {
                                                    telemetry_id: None,
                                                    label: localize("resume", "Resume", &[]),
                                                    keep_open: false,
                                                    run: Rc::new(move || {
                                                        if let Some(this) = weak.upgrade() {
                                                            let _ = this.resume_edit_session(
                                                                ResumeEditSessionArgs { r#ref: Some(partial_ref.clone()), apply_partial_match: true, ..ResumeEditSessionArgs::default() },
                                                                None,
                                                            );
                                                        }
                                                    }),
                                                },
                                                is_secondary: false,
                                            })],
                                            None,
                                        );
                                    } else {
                                        folder_root = Some(f.clone());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        folder_root = workspace_folders.iter().find(|f| f.name == folder.name).cloned();
                    }
                }

                let Some(folder_root) = folder_root else {
                    this.services.log_service.info(&format!(
                        "Skipping applying {} changes from edit session with ref {} as no matching workspace folder was found.",
                        folder.working_changes.len(),
                        r#ref
                    ));
                    return Ok(GeneratedChanges::default());
                };

                let mut local_changes: HashSet<String> = HashSet::new();
                for repository in this.services.scm_service.repositories() {
                    let Some(root_uri) = repository.provider().root_uri() else {
                        continue;
                    };
                    if this.services.context_service.get_workspace_folder(&root_uri).is_some_and(|workspace_folder| workspace_folder.name == folder.name) {
                        for change in get_changed_resources(&*repository) {
                            local_changes.insert(change.to_string(false).to_string_lossy());
                        }
                    }
                }

                for change in &folder.working_changes {
                    let uri = join_path(&folder_root.uri, &[change.relative_file_path()]);
                    if !ext_uri.is_equal_or_parent(&uri, &folder_root.uri, false, false) || ext_uri.is_equal(Some(&uri), Some(&folder_root.uri), false) {
                        this.services.log_service.warn(&format!("Skipping change outside workspace folder: {}", change.relative_file_path()));
                        continue;
                    }

                    let incoming_change = IncomingChange { uri: uri.clone(), change: change.clone() };
                    changes.push(incoming_change.clone());
                    if this.will_change_local_contents(&local_changes, &uri, change).await? {
                        conflicting_changes.push(incoming_change);
                    }
                }
            }

            Ok(GeneratedChanges { changes, conflicting_changes })
        })
    }

    fn will_change_local_contents(&self, local_changes: &HashSet<String>, uri_with_incoming_changes: &URI, incoming_change: &Change) -> Promise<bool> {
        if !local_changes.contains(&uri_with_incoming_changes.to_string(false).to_string_lossy()) {
            return Promise::resolved(false);
        }

        let this = self.rc();
        let (uri, incoming_change) = (uri_with_incoming_changes.clone(), incoming_change.clone());
        async_fn(async move {
            match incoming_change {
                Change::Addition { contents, .. } => {
                    let contents_units: Vec<u16> = contents.encode_utf16().collect();
                    let original_contents = hash_async(HashAsyncInput::Text(&contents_units)).await?;
                    let file = this.services.file_service.read_file(&uri, None, None).await?;
                    let encoded_units: Vec<u16> = encode_base64(&file.value, true, false).encode_utf16().collect();
                    let incoming_contents = hash_async(HashAsyncInput::Text(&encoded_units)).await?;
                    Ok(original_contents != incoming_contents)
                }
                Change::Deletion { .. } => this.services.file_service.exists(&uri).await,
            }
        })
    }

    pub fn store_edit_session(&self, from_store_command: bool, cancellation_token: CancellationToken) -> Promise<Option<String>> {
        let this = self.rc();
        async_fn(async move {
            let mut folders: Vec<Folder> = Vec::new();
            let mut edit_session_size: usize = 0;
            let mut has_edits = false;

            // Save all saveable editors before building edit session contents
            this.services.editor_service.save_all(None).await?;

            // Do a first pass over all repositories to ensure that the edit session identity is created for each.
            // This may change the working changes that need to be stored later
            let mut created_edit_session_identities = je_base::map::ResourceSet::new(None);
            for repository in this.services.scm_service.repositories() {
                let changed_resources = get_changed_resources(&*repository);
                if changed_resources.is_empty() {
                    continue;
                }
                for uri in changed_resources {
                    let workspace_folder = this.services.context_service.get_workspace_folder(&uri);
                    let Some(workspace_folder) = workspace_folder.filter(|_| !created_edit_session_identities.has(&uri)) else {
                        continue;
                    };
                    created_edit_session_identities.add(uri);
                    this.services.edit_session_identity_service.on_will_create_edit_session_identity(&workspace_folder, cancellation_token.clone()).await?;
                }
            }

            for repository in this.services.scm_service.repositories() {
                // Look through all resource groups and compute which files were added/modified/deleted
                let tracked_uris = get_changed_resources(&*repository); // A URI might appear in more than one resource group

                let mut working_changes: Vec<Change> = Vec::new();

                let root_uri = repository.provider().root_uri();
                let workspace_folder = root_uri.and_then(|root_uri| this.services.context_service.get_workspace_folder(&root_uri));
                let mut name = workspace_folder.as_ref().map(|workspace_folder| workspace_folder.name.clone());

                for uri in tracked_uris {
                    let Some(tracked_workspace_folder) = this.services.context_service.get_workspace_folder(&uri) else {
                        this.services.log_service.info(&format!("Skipping working change {} as no associated workspace folder was found.", uri.to_string(false)));

                        continue;
                    };

                    name = name.or_else(|| Some(tracked_workspace_folder.name.clone()));
                    let relative_file_path = ext_uri().relative_path(&tracked_workspace_folder.uri, &uri).unwrap_or_else(|| uri.path().to_string_lossy());

                    // Only deal with file contents for now
                    if let Ok(stat) = this.services.file_service.stat(&uri).await
                        && !stat.is_file
                    {
                        continue;
                    }

                    has_edits = true;

                    if this.services.file_service.exists(&uri).await? {
                        let contents = encode_base64(&this.services.file_service.read_file(&uri, None, None).await?.value, true, false);
                        edit_session_size += contents.encode_utf16().count();
                        if edit_session_size > this.services.edit_sessions_storage_service.size_limit() {
                            this.notify_error(localize("payload too large", "Your working changes exceed the size limit and cannot be stored.", &[]));
                            return Ok(None);
                        }

                        working_changes.push(Change::Addition { relative_file_path, file_type: FileType::File, contents });
                    } else {
                        // Assume it's a deletion
                        working_changes.push(Change::Deletion { relative_file_path, file_type: FileType::File });
                    }
                }

                let mut canonical_identity = None;
                if let Some(workspace_folder) = &workspace_folder {
                    canonical_identity = this.services.edit_session_identity_service.get_edit_session_identifier(workspace_folder, cancellation_token.clone()).await?;
                }

                // As mudanças ficam como filhas da pasta (o original anota a dívida de não guardá-las assim)
                folders.push(Folder {
                    working_changes,
                    name: name.unwrap_or_default(),
                    canonical_identity,
                    absolute_uri: workspace_folder.as_ref().map(|workspace_folder| workspace_folder.uri.to_string(false).to_string_lossy()),
                });
            }

            // Store contributed workspace state
            if let Some(workspace_state_synchronizer) = &this.workspace_state_synchronizer {
                workspace_state_synchronizer.sync().await?;
            }

            if !has_edits {
                this.services.log_service.info("Skipped storing working changes in the cloud as there are no edits to store.");
                if from_store_command {
                    this.services.notification_service.info(vec![NotificationMessage::from(localize(
                        "no working changes to store",
                        "Skipped storing working changes in the cloud as there are no edits to store.",
                        &[],
                    ))]);
                }
                return Ok(None);
            }

            let workspace_state_id = this.services.edit_sessions_storage_service.last_written_resources().borrow().get(&SyncResource::WorkspaceState).map(|resource| resource.r#ref.clone());
            let data = EditSession { folders, version: 2, workspace_state_id, machine: None };

            match this.write_edit_session(data).await {
                Ok(r#ref) => Ok(Some(r#ref)),
                Err(error) => {
                    this.services.log_service.error_args(&LogMessage::from("Failed to store edit session, reason: "), &[LogArg::from(error.to_string().as_str())]);

                    if let Some(store_error) = UserDataSyncStoreError::from_error(&error) {
                        match store_error.error.code {
                            UserDataSyncErrorCode::TooLarge => {
                                // Uploading a payload can fail due to server size limits
                                this.log_telemetry("editSessions.upload.failed", &[("reason", "TooLarge")]);
                                this.notify_error(localize("payload too large", "Your working changes exceed the size limit and cannot be stored.", &[]));
                            }
                            _ => {
                                this.log_telemetry("editSessions.upload.failed", &[("reason", "unknown")]);
                                this.notify_error(localize("payload failed", "Your working changes cannot be stored.", &[]));
                            }
                        }
                    }

                    Ok(None)
                }
            }
        })
    }

    /// O `try { ... write ... }` de `storeEditSession`.
    fn write_edit_session(&self, data: EditSession) -> Promise<String> {
        let this = self.rc();
        async_fn(async move {
            this.services.log_service.info("Storing edit session...");
            let r#ref = this.services.edit_sessions_storage_service.write(SyncResource::EditSessions, EditSessionContent::Session(data)).await?;
            this.services.log_service.info(&format!("Stored edit session with ref {}.", r#ref));
            Ok(r#ref)
        })
    }

    fn has_edit_session(&self) -> bool {
        self.services.scm_service.repositories().iter().any(|repository| !get_changed_resources(&**repository).is_empty())
    }

    fn should_continue_on_with_edit_session(&self) -> Promise<bool> {
        let this = self.rc();
        async_fn(async move {
            const CAN_STORE_OUTCOME: &str = "continueOn.editSessions.canStore.outcome";
            const WITH_CLOUD_CHANGES: &str = "withCloudChanges";
            const WITHOUT_CLOUD_CHANGES: &str = "withoutCloudChanges";

            // If the user is already signed in, we should store edit session
            if this.services.edit_sessions_storage_service.is_signed_in() {
                return Ok(this.has_edit_session());
            }

            // If the user has been asked before and said no, don't use edit sessions
            if this.config_text(USE_EDIT_SESSIONS_WITH_CONTINUE_ON).as_deref() == Some("off") {
                this.log_telemetry(CAN_STORE_OUTCOME, &[("outcome", "disabledEditSessionsViaSetting")]);
                return Ok(false);
            }

            // Prompt the user to use edit sessions if they currently could benefit from using it
            if this.has_edit_session() {
                let disposables = DisposableStore::new();
                let quickpick = this.services.quick_input_service.create_quick_pick(None);
                disposables.add(quickpick.clone());
                quickpick.set_placeholder(Some(localize("continue with cloud changes", "Select whether to bring your working changes with you", &[])));
                quickpick.set_ok(QuickPickOk::Bool(false));
                quickpick.set_ignore_focus_out(true);
                let item = |id: &str, label: String| QuickPickInput::Item(IQuickPickItem { base: IQuickItem { id: Some(id.to_string()), label: label.as_str().into(), ..IQuickItem::default() }, ..IQuickPickItem::default() });
                quickpick.set_items(vec![
                    item(WITH_CLOUD_CHANGES, localize("with cloud changes", "Yes, continue with my working changes", &[])),
                    item(WITHOUT_CLOUD_CHANGES, localize("without cloud changes", "No, continue without my working changes", &[])),
                ]);

                let continue_with_cloud_changes = Promise::<bool>::with_executor(|resolver| {
                    let weak_disposables = Rc::downgrade(&disposables);
                    let on_did_accept = {
                        let (resolver, quickpick, weak_disposables) = (resolver.clone(), quickpick.clone(), weak_disposables.clone());
                        quickpick.on_did_accept().listen(move |_| {
                            resolver.resolve(quickpick.selected_items().first().is_some_and(|selected| selected.base.id.as_deref() == Some(WITH_CLOUD_CHANGES)));
                            if let Some(disposables) = weak_disposables.upgrade() {
                                disposables.dispose();
                            }
                        })
                    };
                    disposables.add(on_did_accept);
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
                    quickpick.show();
                })
                .await?;

                if !continue_with_cloud_changes {
                    this.log_telemetry(CAN_STORE_OUTCOME, &[("outcome", "didNotEnableEditSessionsWhenPrompted")]);
                    return Ok(continue_with_cloud_changes);
                }

                let initialized = this.services.edit_sessions_storage_service.initialize(InitializeReason::Write, false).await?;
                if !initialized {
                    this.log_telemetry(CAN_STORE_OUTCOME, &[("outcome", "didNotEnableEditSessionsWhenPrompted")]);
                }
                return Ok(initialized);
            }

            Ok(false)
        })
    }
}

impl IDisposable for EditSessionsContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkbenchContribution for EditSessionsContribution {
    fn disposable(self: Rc<Self>) -> Option<DisposableRef> {
        Some(self)
    }
}
