//! O host de extensões: o programa Node (`NodeRunner`), os serviços `ExtHost*` e o objeto `vscode` que eles alimentam.

use std::rc::Rc;

use je_base::event::Event;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_extensions::scanner::ExtensionDescription;
use je_platform::files::files::Stat;
use je_platform::log::{LogServiceRef, NullLogService};
use je_platform::request::request::{AuthInfo, Credentials};
use je_platform::resources::ext_uri;
use je_workbench::api::common::ext_host_notebook::{ExtHostNotebookController, IExtHostConsumerFileSystem};
use je_workbench::api::common::ext_host_notebook_documents::ExtHostNotebookDocuments;
use je_workbench::api::common::ext_host_notebook_kernels::ExtHostNotebookKernels;
use je_workbench::api::browser::main_thread_commands::MainThreadCommands;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_authentication::ExtHostAuthentication;
use je_workbench::api::common::ext_host_comments::create_ext_host_comments;
use je_workbench::api::common::ext_host_configuration::ExtHostConfiguration;
use je_workbench::api::common::ext_host_debug_protocol::ExtHostDebugServiceShape;
use je_workbench::api::common::ext_host_debug_service::{ExtHostDebugServiceBase, ExtHostDebugServiceDeps, WorkerExtHostDebugService};
use je_workbench::api::common::ext_host_diagnostics::ExtHostDiagnostics;
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_bulk_edits::ExtHostBulkEdits;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_editors::ExtHostEditors;
use je_workbench::api::common::ext_host_file_system::ExtHostFileSystem;
use je_workbench::api::common::ext_host_file_system_consumer::ExtHostConsumerFileSystem;
use je_workbench::api::common::ext_host_file_system_event_service::ExtHostFileSystemEventService;
use je_workbench::api::common::ext_host_file_system_info::ExtHostFileSystemInfo;
use je_workbench::api::common::ext_host_language_features::ExtHostLanguageFeatures;
use je_workbench::api::common::ext_host_language_features_api::IExtHostApiDeprecationService;
use je_workbench::api::common::ext_host_languages::ExtHostLanguages;
use je_workbench::api::common::ext_host_output::ExtHostOutputService;
use je_workbench::api::common::ext_host_progress::ExtHostProgress;
use je_workbench::api::common::ext_host_protocol::{
    CreateDocumentOptions, ExtHostAuthenticationShape, ExtHostCommandsShape, ExtHostCommentsShape, ExtHostConfigurationShape, ExtHostContext, ExtHostDiagnosticsShape, ExtHostDocumentsAndEditorsShape, ExtHostDocumentsShape,
    ExtHostEditorsShape, ExtHostFileSystemEventServiceShape, ExtHostFileSystemShape, ExtHostLanguageFeaturesShape, ExtHostLanguagesShape, ExtHostNotebookDocumentsShape, ExtHostNotebookKernelsShape,
    ExtHostNotebookShape, ExtHostOutputServiceShape, ExtHostProgressShape, ExtHostQuickOpenShape, ExtHostSCMShape,
    ExtHostStatusBarShape, ExtHostTerminalServiceShape, ExtHostTreeViewsShape, ExtHostWorkspaceShape,
    ICommandMetadataDto, IExtHostTelemetry, IMainContext, MainContext, MainThreadDocumentsShape, MainThreadTelemetryShape, OpenDocumentOptions,
};
use je_workbench::api::common::ext_host_quick_open::ExtHostQuickOpen;
use je_workbench::api::common::ext_host_scm::ExtHostSCM;
use je_workbench::api::common::ext_host_status_bar::ExtHostStatusBar;
use je_workbench::api::common::ext_host_terminal_service::WorkerExtHostTerminalService;
use je_workbench::api::common::ext_host_tree_views::ExtHostTreeViews;
use je_workbench::api::common::ext_host_window::ExtHostWindow;
use je_workbench::api::common::ext_host_workspace::{ExtHostWorkspace, IExtHostInitDataService, IExtHostWorkspaceProvider, IURITransformerService, WorkspaceFolder};
use je_workbench::services::extensions::extensions::IExtensionService;
use zjsc::api::embed_node::{JsData, NodeRunner};

use crate::api::env::EnvServices;
use crate::api::events::EventTables;
use crate::api::extensions::ExtensionServiceSlot;
use crate::api::languages::{LanguagesServices, LanguagesState};
use crate::api::notebooks::NotebookApi;
use crate::api::text_editors::{TextEditorApi, TextEditorServices};
use crate::api::window::{WindowServices, WindowState};
use crate::api::workspace::WorkspaceServices;
use crate::api::file_system::FileSystemApi;
use crate::api::file_system_events::{FileSystemEventsApi, WatcherServices};
use crate::api::{authentication, commands, comments, debug, disposable, enums, env, events, extensions, file_system, l10n, languages, notebooks, scm, types, uri, window, workspace, workspace_edit};
use crate::api::js_object::JsObjects;
use crate::ext_host_extension_service::ExtHostExtensionService;
use crate::in_process::{BridgedContext, IMainThreadBinder, LateForwarders};
use crate::local_context::LocalContext;

/// O `ExtHostCommandsShape` que ainda não existe quando o lado do workbench é criado: encaminha para o real depois de
/// ligado (os dois lados do RPC nascem um dentro do outro).
#[derive(Default)]
struct LateExtHostCommands(std::cell::RefCell<Option<Rc<dyn ExtHostCommandsShape>>>);

impl ExtHostCommandsShape for LateExtHostCommands {
    fn rpc_execute_contributed_command(&self, id: &str, args: Vec<je_base::dynamic::Dynamic>) -> Promise<je_base::dynamic::Dynamic> {
        self.0.borrow().as_ref().expect("ExtHostCommands ainda não ligado").rpc_execute_contributed_command(id, args)
    }

    fn rpc_get_contributed_command_metadata(&self) -> Promise<Vec<(String, ICommandMetadataDto)>> {
        self.0.borrow().as_ref().expect("ExtHostCommands ainda não ligado").rpc_get_contributed_command_metadata()
    }
}

/// A telemetria que descarta tudo (o lado do workbench real entra quando a telemetria for montada).
struct NullTelemetry;

impl IDisposable for NullTelemetry {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for NullTelemetry {
    fn rpc_public_log(&self, _event_name: &str, _data: Option<je_base::dynamic::Dynamic>) {}
    fn rpc_public_log2(&self, _event_name: &str, _data: Option<je_base::dynamic::Dynamic>) {}
}

impl IExtHostTelemetry for NullTelemetry {
    fn on_extension_error(&self, _extension: &je_extensions::manifest::ExtensionIdentifier, _error: &je_base::errors::Error) -> bool {
        false
    }
}

/// O `IExtHostApiDeprecationService` do host: registra o aviso no log (o `extHostApiDeprecationService.ts` ainda não tem porte).
struct HostApiDeprecation(LogServiceRef);

impl IExtHostApiDeprecationService for HostApiDeprecation {
    fn report(&self, api_id: &str, extension: &ExtensionDescription, migration_suggestion: &str) {
        self.0.warn(&format!("[{}] Extension '{}' uses a deprecated API '{api_id}': {migration_suggestion}", extension.identifier.value, extension.identifier.value));
    }
}

/// Os dados de início do host, sem workspace: ele chega pelo `$initializeWorkspace` que o `MainThreadWorkspace` envia.
struct NoInitialWorkspace;

impl IExtHostInitDataService for NoInitialWorkspace {
    fn workspace(&self) -> Option<je_workbench::api::common::ext_host_protocol::IWorkspaceData> {
        None
    }
}

/// O `IURITransformerService` de um host em processo: não há transformação de URIs (os dois lados enxergam o mesmo sistema de
/// arquivos e os mesmos esquemas).
struct IdentityUriTransformer;

impl IURITransformerService for IdentityUriTransformer {
    fn transform_outgoing_scheme(&self, scheme: &str) -> String {
        scheme.to_owned()
    }
}

/// O `IExtHostWorkspaceProvider` de um host sem os clientes de workspace: não há pastas abertas nem proxy ou certificados para
/// consultar. O `ExtHostQuickOpen` só o usa em `showWorkspaceFolderPick`, que o `vscode` do host não expõe.
struct NoWorkspaceProvider;

impl IExtHostWorkspaceProvider for NoWorkspaceProvider {
    fn get_workspace_folder2(&self, _uri: &URI, _resolve_parent: Option<bool>) -> Promise<Option<Rc<WorkspaceFolder>>> {
        Promise::resolved(None)
    }

    fn resolve_workspace_folder(&self, _uri: &URI) -> Promise<Option<Rc<WorkspaceFolder>>> {
        Promise::resolved(None)
    }

    fn get_workspace_folders2(&self) -> Promise<Option<Vec<Rc<WorkspaceFolder>>>> {
        Promise::resolved(None)
    }

    fn resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None)
    }

    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        Promise::resolved(None)
    }

    fn lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None)
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        Promise::resolved(Vec::new())
    }
}

/// O `MainThreadDocuments` de um host que tem o controle de código-fonte mas não os documentos de texto: o `ExtHostSCM` guarda o
/// `ExtHostDocuments` (só o usa em `SourceControlInputBox#document`, que é uma proposta de API), e este rejeita o que pedir à
/// thread principal, por não haver cliente de documentos para atendê-lo.
struct NoDocuments;

const NO_DOCUMENTS: &str = "o host não tem o cliente MainThreadDocuments: os documentos de texto não estão disponíveis";

impl IDisposable for NoDocuments {
    fn dispose(&self) {}
}

impl IExtHostDocumentSaveDelegate for NoDocuments {
    fn rpc_try_save_document(&self, _uri: &URI) -> Promise<bool> {
        Promise::rejected(Error::new(NO_DOCUMENTS))
    }
}

impl MainThreadDocumentsShape for NoDocuments {
    fn rpc_try_create_document(&self, _options: Option<CreateDocumentOptions>) -> Promise<URI> {
        Promise::rejected(Error::new(NO_DOCUMENTS))
    }

    fn rpc_try_open_document(&self, _uri: &URI, _options: Option<OpenDocumentOptions>) -> Promise<URI> {
        Promise::rejected(Error::new(NO_DOCUMENTS))
    }
}

/// O `IExtHostConsumerFileSystem` de um host sem o `ExtHostConsumerFileSystem` (`extHostFileSystemConsumer.ts`, ainda sem porte): nenhum
/// esquema é gravável, então o `$saveNotebook` responde com o erro de arquivo somente leitura que o original dá a um esquema sem provedor
/// gravável, e a gravação e a consulta rejeitam com a explicação.
struct NoConsumerFileSystem;

const NO_CONSUMER_FILE_SYSTEM: &str = "o host não tem o ExtHostConsumerFileSystem: o sistema de arquivos não está disponível para os notebooks";

impl IExtHostConsumerFileSystem for NoConsumerFileSystem {
    fn is_writable_file_system(&self, _scheme: &str) -> Option<bool> {
        None
    }

    fn write_file(&self, _uri: &URI, _content: Vec<u8>) -> Promise<()> {
        Promise::rejected(Error::new(NO_CONSUMER_FILE_SYSTEM))
    }

    fn stat(&self, _uri: &URI) -> Promise<Stat> {
        Promise::rejected(Error::new(NO_CONSUMER_FILE_SYSTEM))
    }

    fn get_file_system_provider_ext_uri(&self, _scheme: &str) -> ExtUri {
        ext_uri()
    }
}

/// O `ExtHostDocuments` de um host sem os clientes de documentos e de editores (ver [`NoDocuments`]).
fn detached_documents(log_service: &LogServiceRef) -> Rc<ExtHostDocuments> {
    let context: Rc<dyn IMainContext> = LocalContext::new();
    let main_thread: Rc<dyn MainThreadDocumentsShape> = Rc::new(NoDocuments);
    context.set(&MainContext::MAIN_THREAD_DOCUMENTS, main_thread);
    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(context.clone(), log_service.clone()));
    ExtHostDocuments::new(&*context, documents_and_editors)
}

/// Os serviços do workbench de que o host depende e que o chamador monta.
pub struct WorkbenchServices {
    pub command_service: Rc<dyn je_platform::commands::commands::ICommandService>,
    pub extension_service: Rc<dyn IExtensionService>,
}

/// Os serviços `ExtHost*` dos namespaces que dependem de um cliente `MainThread*` em processo, e os clientes que o host é dono.
#[derive(Default)]
struct BridgedServices {
    workspace: Option<Rc<ExtHostWorkspace>>,
    configuration: Option<Rc<ExtHostConfiguration>>,
    status_bar: Option<Rc<ExtHostStatusBar>>,
    quick_open: Option<Rc<ExtHostQuickOpen>>,
    output: Option<Rc<ExtHostOutputService>>,
    progress: Option<Rc<ExtHostProgress>>,
    text_editors: Option<TextEditorServices>,
    language_features: Option<Rc<ExtHostLanguageFeatures>>,
    terminal: Option<Rc<WorkerExtHostTerminalService>>,
    tree_views: Option<Rc<ExtHostTreeViews>>,
    debug: Option<Rc<ExtHostDebugServiceBase>>,
    main_thread: Vec<Rc<dyn IDisposable>>,
}

/// O host de extensões.
pub struct ExtensionHost {
    runner: NodeRunner,
    commands: Rc<ExtHostCommands>,
    vscode: JsData,
    table: Rc<disposable::DisposableTable>,
    bridged: BridgedServices,
    extension_slot: Rc<ExtensionServiceSlot>,
    _main_thread_commands: Option<Rc<MainThreadCommands>>,
}

impl ExtensionHost {
    /// Monta os serviços e registra `require('vscode')` em `runner`, com o `MainThreadCommands` do workbench no mesmo
    /// [`LocalContext`] (sem serialização: os dois lados são Rust no mesmo processo).
    pub fn new(runner: &NodeRunner, services: WorkbenchServices) -> ExtensionHost {
        let context = LocalContext::new();
        let context_ref: &dyn IMainContext = &*context;
        let late = Rc::new(LateExtHostCommands::default());
        let late_shape: Rc<dyn ExtHostCommandsShape> = late.clone();
        context_ref.set_erased(ExtHostContext::EXT_HOST_COMMANDS.sid, Rc::new(late_shape));
        let telemetry: Rc<dyn MainThreadTelemetryShape> = Rc::new(NullTelemetry);
        context_ref.set_erased(MainContext::MAIN_THREAD_TELEMETRY.sid, Rc::new(telemetry));
        let main_thread_commands = MainThreadCommands::new(context_ref, services.command_service, services.extension_service);
        let main_shape: Rc<dyn je_workbench::api::common::ext_host_protocol::MainThreadCommandsShape> = main_thread_commands.clone();
        context_ref.set_erased(MainContext::MAIN_THREAD_COMMANDS.sid, Rc::new(main_shape));
        let mut host = Self::with_context(runner, context_ref);
        let shape: Rc<dyn ExtHostCommandsShape> = host.commands.clone();
        *late.0.borrow_mut() = Some(shape);
        host._main_thread_commands = Some(main_thread_commands);
        host
    }

    /// Monta o host sobre `context`, que já entrega o proxy de `MainThreadCommands` e `MainThreadTelemetry` (o `RPCProtocol` do
    /// lado do host, quando o workbench está do outro lado de um protocolo de mensagens), registra o `ExtHostCommands` nele
    /// (para o workbench executar os comandos que as extensões registram) e registra `require('vscode')` em `runner`. O objeto
    /// `vscode` só tem `commands`, `l10n` e as classes de valor: os namespaces que pedem clientes do workbench entram por
    /// [`ExtensionHost::with_bridge`].
    pub fn with_context(runner: &NodeRunner, context: &dyn IMainContext) -> ExtensionHost {
        Self::with_bridge(runner, context, None, NullLogService::shared())
    }

    /// Como [`ExtensionHost::with_context`], com o `binder` que monta, em processo, os clientes `MainThread*` do workbench
    /// (ver `crate::in_process`). Cada namespace entra em `vscode` se o `binder` registrou os clientes de que ele depende:
    /// `workspace` pede `MainThreadWorkspace`, `MainThreadConfiguration` e `MainThreadMessageService`; `window` existe se houver ao
    /// menos um destes: `MainThreadMessageService` (as mensagens), `MainThreadStatusBar` (a barra de status), `MainThreadQuickOpen`
    /// (`showQuickPick` e `showInputBox`), `MainThreadOutputService` com [`IMainThreadBinder::output_environment`]
    /// (`createOutputChannel`) e `MainThreadProgress` (`withProgress`). Os editores e documentos de texto (`activeTextEditor`,
    /// `showTextDocument`, `workspace.textDocuments`, `openTextDocument` e os eventos) pedem `MainThreadDocuments` e
    /// `MainThreadTextEditors` (o `MainThreadDocumentsAndEditors` registra os dois). `vscode.env` existe se o binder entrega
    /// [`IMainThreadBinder::host_environment`], [`IMainThreadBinder::clipboard_service`] ou o cliente `MainThreadWindow`
    /// (`openExternal`); `vscode.extensions` sempre existe e lê o `ExtHostExtensionService` que
    /// [`ExtensionHost::bind_extension_service`] liga. `vscode.languages` existe se há os documentos de texto e ao menos um destes:
    /// `MainThreadLanguages` (`getLanguages`, `setTextDocumentLanguage`), `MainThreadDiagnostics` (coleções, `getDiagnostics`) e
    /// `MainThreadLanguageFeatures` junto de `MainThreadDiagnostics` (os `register*Provider`).
    pub fn with_bridge(runner: &NodeRunner, context: &dyn IMainContext, binder: Option<&dyn IMainThreadBinder>, log_service: LogServiceRef) -> ExtensionHost {
        let commands = ExtHostCommands::new(context, NullLogService::shared(), Rc::new(NullTelemetry));
        let shape: Rc<dyn ExtHostCommandsShape> = commands.clone();
        context.set(&ExtHostContext::EXT_HOST_COMMANDS, shape);

        let table = disposable::DisposableTable::new();
        let event_tables = EventTables::new();
        let window_state = WindowState::default();
        let languages_state = LanguagesState::default();
        let disposable_class = disposable::define(runner, &table);
        let mut members = types::classes(runner);
        members.push(("Disposable".to_owned(), disposable_class));
        members.extend(events::classes(runner, &table, &event_tables));
        members.push(uri::class(runner));
        members.extend(enums::enums());
        members.extend(workspace::classes(runner));
        members.extend(file_system::classes(runner));
        let workspace_edit_objects = Rc::new(workspace_edit::WorkspaceEditObjects::default());
        members.extend(workspace_edit::classes(runner, &workspace_edit_objects));
        members.extend(window::classes(runner, &window_state, &table));
        members.extend(languages::classes(runner, &languages_state));
        let scm_objects = Rc::new(scm::ScmObjects::default());
        scm::define_classes(runner, &scm_objects, &event_tables);
        let comment_objects = Rc::new(comments::CommentObjects::default());
        comments::define_classes(runner, &comment_objects, &event_tables, &window_state.text_objects);
        let debug_objects = Rc::new(debug::DebugObjects::default());
        members.extend(debug::classes(runner, &debug_objects));
        let notebook_objects = Rc::new(notebooks::NotebookObjects::default());
        notebooks::define_classes(runner, &notebook_objects, &window_state.text_objects, &table, &event_tables);
        members.extend(notebooks::classes(runner));
        let extension_slot = ExtensionServiceSlot::new();
        extensions::define_class(runner, &extension_slot);

        let mut bridged = BridgedServices::default();
        let mut env_services = EnvServices::default();
        if let Some(binder) = binder {
            let local = LocalContext::new();
            let late = LateForwarders::register(&local);
            let local_context: Rc<dyn IMainContext> = local.clone();
            bridged.main_thread = binder.bind(local_context.clone());
            let bridged_context = BridgedContext::new(context, local.clone());
            let bridged_ref: &dyn IMainContext = &bridged_context;
            let provided = |sid: &'static str| local.contains(sid);
            // As capacidades dos provedores de arquivo: o workspace e o `createFileSystemWatcher` leem o mesmo registro.
            let file_system_info = Rc::new(ExtHostFileSystemInfo::new());

            if provided(MainContext::MAIN_THREAD_DOCUMENTS.sid) && provided(MainContext::MAIN_THREAD_TEXT_EDITORS.sid) {
                let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(local_context.clone(), log_service.clone()));
                let documents = ExtHostDocuments::new(bridged_ref, documents_and_editors.clone());
                let editors = ExtHostEditors::new(bridged_ref, documents_and_editors.clone());
                // Os clientes já entregaram o delta inicial aos encaminhadores: ligar o serviço real o esvazia na ordem.
                let documents_and_editors_shape: Rc<dyn ExtHostDocumentsAndEditorsShape> = documents_and_editors.clone();
                late.documents_and_editors.0.bind(documents_and_editors_shape);
                let documents_shape: Rc<dyn ExtHostDocumentsShape> = documents.clone();
                late.documents.0.bind(documents_shape);
                let editors_shape: Rc<dyn ExtHostEditorsShape> = editors.clone();
                late.editors.0.bind(editors_shape);
                bridged.text_editors = Some(TextEditorServices { documents_and_editors, documents, editors });
            }
            let text_editor_api = bridged.text_editors.clone().map(|services| TextEditorApi { services, objects: window_state.text_objects.clone() });

            env_services.environment = binder.host_environment();
            env_services.clipboard = binder.clipboard_service();
            if provided(MainContext::MAIN_THREAD_WINDOW.sid) {
                env_services.window = Some(ExtHostWindow::new(bridged_ref, None));
            }

            if provided(MainContext::MAIN_THREAD_WORKSPACE.sid) && provided(MainContext::MAIN_THREAD_CONFIGURATION.sid) && provided(MainContext::MAIN_THREAD_MESSAGE_SERVICE.sid) {
                let workspace_service = ExtHostWorkspace::new(
                    bridged_ref,
                    &NoInitialWorkspace,
                    file_system_info.clone(),
                    log_service.clone(),
                    Rc::new(IdentityUriTransformer),
                );
                let configuration = ExtHostConfiguration::new(bridged_ref, workspace_service.clone(), log_service.clone());
                let workspace_shape: Rc<dyn ExtHostWorkspaceShape> = workspace_service.clone();
                late.workspace.0.bind(workspace_shape);
                let configuration_shape: Rc<dyn ExtHostConfigurationShape> = configuration.clone();
                late.configuration.0.bind(configuration_shape);
                bridged.workspace = Some(workspace_service);
                bridged.configuration = Some(configuration);
            }

            // `vscode.workspace` existe se houver os clientes de workspace ou os de documentos e editores (o original sempre o tem).
            let workspace_pair = bridged.workspace.clone().zip(bridged.configuration.clone());
            // `applyEdit` pede o cliente `MainThreadBulkEdits` e o `ExtHostDocumentsAndEditors` (a versão de cada documento aberto).
            // Sem os clientes de documento não há documentos abertos: o serviço enxerga um conjunto vazio, e toda edição vai sem versão.
            let documents_and_editors = match &text_editor_api {
                Some(api) => api.services.documents_and_editors.clone(),
                None => Rc::new(ExtHostDocumentsAndEditors::new(LocalContext::new(), log_service.clone())),
            };
            let bulk_edits = provided(MainContext::MAIN_THREAD_BULK_EDITS.sid)
                .then(|| (Rc::new(ExtHostBulkEdits::new(bridged_ref, documents_and_editors.clone())), workspace_edit_objects.clone()));
            // Os eventos de arquivo pedem o cliente `MainThreadFileSystemEventService`; o `createFileSystemWatcher` pede também o workspace.
            let file_system_events = provided(MainContext::MAIN_THREAD_FILE_SYSTEM_EVENT_SERVICE.sid).then(|| {
                let service = ExtHostFileSystemEventService::new(local_context.clone(), log_service.clone(), documents_and_editors.clone());
                let service_shape: Rc<dyn ExtHostFileSystemEventServiceShape> = service.clone();
                late.file_system_events.0.bind(service_shape);
                FileSystemEventsApi {
                    service,
                    watcher: workspace_pair
                        .clone()
                        .map(|(workspace, configuration)| Rc::new(WatcherServices { workspace, configuration, file_system_info: file_system_info.clone() })),
                    edits: workspace_edit_objects.clone(),
                    events: event_tables.clone(),
                }
            });
            // `fs` e `registerFileSystemProvider` pedem o cliente `MainThreadFileSystem`; o `ExtHostFileSystem` (o registro) nasce no bloco de
            // linguagens, que lhe dá o `ExtHostLanguageFeatures`.
            let file_system_api = provided(MainContext::MAIN_THREAD_FILE_SYSTEM.sid).then(|| FileSystemApi {
                consumer: ExtHostConsumerFileSystem::new(bridged_ref, file_system_info.clone()),
                provider: Rc::default(),
                js: JsObjects::new(runner),
                table: table.clone(),
            });
            // `vscode.notebooks` e os membros de notebook de `vscode.workspace` pedem os clientes `MainThreadNotebook`, `MainThreadNotebookDocuments` e
            // `MainThreadNotebookEditors` e os documentos de texto (cada célula é um documento de texto); o `createNotebookController` pede também
            // `MainThreadNotebookKernels`. O host não tem o `ExtHostConsumerFileSystem` (ver `NoConsumerFileSystem`).
            let notebook_api = match &text_editor_api {
                Some(api)
                    if provided(MainContext::MAIN_THREAD_NOTEBOOK.sid)
                        && provided(MainContext::MAIN_THREAD_NOTEBOOK_DOCUMENTS.sid)
                        && provided(MainContext::MAIN_THREAD_NOTEBOOK_EDITORS.sid) =>
                {
                    let controller = ExtHostNotebookController::new(
                        bridged_ref,
                        &commands,
                        api.services.documents_and_editors.clone(),
                        api.services.documents.clone(),
                        Rc::new(NoConsumerFileSystem),
                        log_service.clone(),
                    );
                    let controller_shape: Rc<dyn ExtHostNotebookShape> = controller.clone();
                    late.notebook.0.bind(controller_shape);
                    let documents_shape: Rc<dyn ExtHostNotebookDocumentsShape> = ExtHostNotebookDocuments::new(controller.clone());
                    late.notebook_documents.0.bind(documents_shape);
                    let kernels = provided(MainContext::MAIN_THREAD_NOTEBOOK_KERNELS.sid)
                        .then(|| ExtHostNotebookKernels::new(bridged_ref, controller.clone(), &commands, log_service.clone()))
                        .and_then(|kernels| match kernels {
                            Ok(kernels) => {
                                let kernels_shape: Rc<dyn ExtHostNotebookKernelsShape> = kernels.clone();
                                late.notebook_kernels.0.bind(kernels_shape);
                                Some(kernels)
                            }
                            Err(error) => {
                                log_service.error(&format!("vscode.notebooks: o ExtHostNotebookKernels não subiu: {}", error.message));
                                None
                            }
                        });
                    Some(NotebookApi { notebooks: controller, kernels, objects: notebook_objects.clone(), events: event_tables.clone() })
                }
                _ => None,
            };

            if workspace_pair.is_some() || text_editor_api.is_some() || bulk_edits.is_some() || file_system_events.is_some() || notebook_api.is_some() || file_system_api.is_some() {
                members.push((
                    "workspace".to_owned(),
                    workspace::namespace(
                        runner,
                        &WorkspaceServices {
                            workspace: workspace_pair,
                            text_editors: text_editor_api.clone(),
                            bulk_edits,
                            file_system_events,
                            notebooks: notebook_api.clone(),
                            file_system: file_system_api.clone(),
                        },
                        &table,
                        &event_tables,
                    ),
                ));
            }
            if let Some(api) = &notebook_api {
                members.push(("notebooks".to_owned(), notebooks::namespace(runner, &JsObjects::new(runner), api, &table)));
            }

            if provided(MainContext::MAIN_THREAD_STATUS_BAR.sid) {
                let status_bar = ExtHostStatusBar::new(bridged_ref, commands.converter());
                let status_bar_shape: Rc<dyn ExtHostStatusBarShape> = status_bar.clone();
                late.status_bar.0.bind(status_bar_shape);
                bridged.status_bar = Some(status_bar);
            }

            if provided(MainContext::MAIN_THREAD_QUICK_OPEN.sid) {
                let workspace_provider: Rc<dyn IExtHostWorkspaceProvider> = match &bridged.workspace {
                    Some(workspace_service) => workspace_service.clone(),
                    None => Rc::new(NoWorkspaceProvider),
                };
                let quick_open = ExtHostQuickOpen::new(bridged_ref, workspace_provider, commands.clone());
                let quick_open_shape: Rc<dyn ExtHostQuickOpenShape> = quick_open.clone();
                late.quick_open.0.bind(quick_open_shape);
                bridged.quick_open = Some(quick_open);
            }

            if provided(MainContext::MAIN_THREAD_OUTPUT_SERVICE.sid)
                && let Some(environment) = binder.output_environment()
            {
                let output = ExtHostOutputService::new(
                    bridged_ref,
                    environment.logs_location,
                    environment.extension_log_levels,
                    environment.directory_creator,
                    environment.logger_service,
                    log_service.clone(),
                );
                let output_shape: Rc<dyn ExtHostOutputServiceShape> = output.clone();
                late.output.0.bind(output_shape);
                bridged.output = Some(output);
            }

            if provided(MainContext::MAIN_THREAD_PROGRESS.sid) {
                let progress = ExtHostProgress::new(bridged_ref);
                let progress_shape: Rc<dyn ExtHostProgressShape> = progress.clone();
                late.progress.0.bind(progress_shape);
                bridged.progress = Some(progress);
            }

            if provided(MainContext::MAIN_THREAD_TERMINAL_SERVICE.sid) {
                let terminal = WorkerExtHostTerminalService::new_local(commands.clone(), bridged_ref);
                let terminal_shape: Rc<dyn ExtHostTerminalServiceShape> = terminal.clone();
                late.terminal_service.0.bind(terminal_shape);
                bridged.terminal = Some(terminal);
            }

            if provided(MainContext::MAIN_THREAD_TREE_VIEWS.sid) {
                let proxy = bridged_ref.get_proxy(&MainContext::MAIN_THREAD_TREE_VIEWS);
                let tree_views = ExtHostTreeViews::new(proxy, &commands, log_service.clone());
                let tree_views_shape: Rc<dyn ExtHostTreeViewsShape> = tree_views.clone();
                late.tree_views.0.bind(tree_views_shape);
                bridged.tree_views = Some(tree_views);
            }

            // `vscode.scm` pede o cliente `MainThreadSCM`; os documentos de texto são opcionais (só a proposta `scmTextDocument` os lê).
            if provided(MainContext::MAIN_THREAD_SCM.sid) {
                let documents = match &text_editor_api {
                    Some(api) => api.services.documents.clone(),
                    None => detached_documents(&log_service),
                };
                let scm_service = ExtHostSCM::new(bridged_ref, commands.clone(), documents, log_service.clone());
                let scm_shape: Rc<dyn ExtHostSCMShape> = scm_service.clone();
                late.scm.0.bind(scm_shape);
                members.push(("scm".to_owned(), scm::namespace(runner, &scm_service, &scm_objects)));
            }

            // `vscode.comments` pede o cliente `MainThreadComments`; os documentos de texto são opcionais (só o `commentingRangeProvider` os lê).
            if provided(MainContext::MAIN_THREAD_COMMENTS.sid) {
                let documents = match &text_editor_api {
                    Some(api) => api.services.documents.clone(),
                    None => detached_documents(&log_service),
                };
                let comments_service = create_ext_host_comments(bridged_ref, &commands, documents);
                let comments_shape: Rc<dyn ExtHostCommentsShape> = comments_service.clone();
                late.comments.0.bind(comments_shape);
                members.push(("comments".to_owned(), comments::namespace(runner, &comments_service, &comment_objects)));
            }

            // `vscode.authentication` pede o cliente `MainThreadAuthentication`.
            if provided(MainContext::MAIN_THREAD_AUTHENTICATION.sid) {
                let authentication_service = ExtHostAuthentication::new(bridged_ref, log_service.clone());
                let authentication_shape: Rc<dyn ExtHostAuthenticationShape> = authentication_service.clone();
                late.authentication.0.bind(authentication_shape);
                members.push(("authentication".to_owned(), authentication::namespace(runner, &JsObjects::new(runner), &authentication_service, &table)));
            }

            // `vscode.debug` pede o cliente `MainThreadDebugService`. O host não tem o `IConfigurationResolverService` nem o namespace
            // `tests` (ver `NoVariableResolver` e `NoTesting`); o registro de extensões vem do serviço que o host liga depois.
            if provided(MainContext::MAIN_THREAD_DEBUG_SERVICE.sid) {
                let workspace_provider: Rc<dyn IExtHostWorkspaceProvider> = match &bridged.workspace {
                    Some(workspace_service) => workspace_service.clone(),
                    None => Rc::new(NoWorkspaceProvider),
                };
                let debug_context: Rc<dyn IMainContext> = Rc::new(debug::DebugContext {
                    debug: bridged_ref.get_proxy(&MainContext::MAIN_THREAD_DEBUG_SERVICE),
                    telemetry: bridged_ref.get_proxy(&MainContext::MAIN_THREAD_TELEMETRY),
                });
                let service = WorkerExtHostDebugService::new(ExtHostDebugServiceDeps {
                    rpc_service: debug_context,
                    workspace_service: workspace_provider,
                    extension_service: extension_slot.clone(),
                    variable_resolver: Rc::new(debug::NoVariableResolver),
                    commands: commands.clone(),
                    testing: Rc::new(debug::NoTesting),
                });
                let service_shape: Rc<dyn ExtHostDebugServiceShape> = service.clone();
                late.debug_service.0.bind(service_shape);
                let services = debug::DebugServices { service: service.clone(), extensions: extension_slot.clone() };
                members.push((
                    "debug".to_owned(),
                    debug::namespace(runner, &JsObjects::new(runner), &services, &debug_objects, &table, &event_tables),
                ));
                bridged.debug = Some(service);
            }

            // `vscode.languages` pede os documentos de texto e, por grupo, o cliente `MainThread*` dele; os provedores pedem também os
            // diagnósticos (o `ExtHostLanguageFeatures` os usa nas ações de código).
            if let Some(text_editor_api) = &text_editor_api {
                let documents = text_editor_api.services.documents.clone();
                let languages = provided(MainContext::MAIN_THREAD_LANGUAGES.sid).then(|| {
                    let service = ExtHostLanguages::new(bridged_ref, documents.clone(), commands.converter());
                    let shape: Rc<dyn ExtHostLanguagesShape> = service.clone();
                    late.languages.0.bind(shape);
                    service
                });
                let diagnostics = provided(MainContext::MAIN_THREAD_DIAGNOSTICS.sid).then(|| {
                    let service = Rc::new(ExtHostDiagnostics::new(
                        bridged_ref,
                        log_service.clone(),
                        Rc::new(ExtHostFileSystemInfo::new()),
                        text_editor_api.services.documents_and_editors.clone(),
                    ));
                    let shape: Rc<dyn ExtHostDiagnosticsShape> = service.clone();
                    late.diagnostics.0.bind(shape);
                    service
                });
                let features = diagnostics.clone().filter(|_| provided(MainContext::MAIN_THREAD_LANGUAGE_FEATURES.sid)).map(|diagnostics| {
                    let service = ExtHostLanguageFeatures::new(
                        bridged_ref,
                        None,
                        documents.clone(),
                        commands.clone(),
                        diagnostics,
                        log_service.clone(),
                        Rc::new(HostApiDeprecation(log_service.clone())),
                        Rc::new(NullTelemetry),
                    );
                    let shape: Rc<dyn ExtHostLanguageFeaturesShape> = service.clone();
                    late.language_features.0.bind(shape);
                    service
                });
                if languages.is_some() || diagnostics.is_some() || features.is_some() {
                    bridged.language_features = features.clone();
                    if let (Some(api), Some(features)) = (&file_system_api, &features) {
                        let service = ExtHostFileSystem::new(bridged_ref, features.clone());
                        let service_shape: Rc<dyn ExtHostFileSystemShape> = service.clone();
                        late.file_system.0.bind(service_shape);
                        *api.provider.borrow_mut() = Some(service);
                    }
                    let services = LanguagesServices { languages, diagnostics, features, text_editors: text_editor_api.clone(), workspace_edits: workspace_edit_objects.clone() };
                    members.push(("languages".to_owned(), languages::namespace(runner, &services, &table, &event_tables, &languages_state)));
                }
            }

            let message_service = provided(MainContext::MAIN_THREAD_MESSAGE_SERVICE.sid).then(|| bridged_ref.get_proxy(&MainContext::MAIN_THREAD_MESSAGE_SERVICE));
            if message_service.is_some()
                || bridged.status_bar.is_some()
                || bridged.quick_open.is_some()
                || bridged.output.is_some()
                || bridged.progress.is_some()
                || bridged.terminal.is_some()
                || bridged.tree_views.is_some()
                || text_editor_api.is_some()
            {
                let window_services = Rc::new(WindowServices {
                    message_service,
                    log_service: log_service.clone(),
                    status_bar: bridged.status_bar.clone(),
                    quick_open: bridged.quick_open.clone(),
                    output: bridged.output.clone(),
                    progress: bridged.progress.clone(),
                    terminal: bridged.terminal.clone(),
                    tree_views: bridged.tree_views.clone(),
                    text_editors: text_editor_api,
                });
                members.push(("window".to_owned(), window::namespace(runner, window_services, &table, &event_tables, &window_state)));
            }
        }

        members.extend([
            ("commands".to_owned(), commands::namespace(runner, &commands, &table)),
            ("l10n".to_owned(), l10n::namespace(runner)),
            ("extensions".to_owned(), extensions::namespace(runner, &extension_slot)),
        ]);
        if env_services.is_any() {
            members.push(("env".to_owned(), env::namespace(runner, &env_services)));
        }
        let vscode = JsData::Object(members);
        runner.register_module("vscode", &vscode);
        ExtensionHost { runner: runner.clone(), commands, vscode, table, bridged, extension_slot, _main_thread_commands: None }
    }

    /// Liga o `ExtHostExtensionService` ao `vscode.extensions` (o serviço nasce depois do objeto `vscode`).
    pub fn bind_extension_service(&self, service: Rc<ExtHostExtensionService>) {
        self.extension_slot.bind(service);
    }

    pub fn commands(&self) -> &Rc<ExtHostCommands> {
        &self.commands
    }

    pub fn vscode(&self) -> &JsData {
        &self.vscode
    }

    /// O `ExtHostWorkspace` do host, se o host foi montado com os clientes de workspace.
    pub fn workspace(&self) -> Option<&Rc<ExtHostWorkspace>> {
        self.bridged.workspace.as_ref()
    }

    /// O `ExtHostConfiguration` do host, se o host foi montado com os clientes de configuração.
    pub fn configuration(&self) -> Option<&Rc<ExtHostConfiguration>> {
        self.bridged.configuration.as_ref()
    }

    /// Os serviços de documentos e editores de texto do host, se o host foi montado com os clientes de documentos e de editores.
    pub fn text_editors(&self) -> Option<&TextEditorServices> {
        self.bridged.text_editors.as_ref()
    }

    /// A tabela dos descartáveis do Rust que o JavaScript segura (`registerCommand` devolve um deles).
    pub fn disposables(&self) -> &Rc<disposable::DisposableTable> {
        &self.table
    }

    /// Descarta os clientes `MainThread*` que o `binder` criou e a mensagem de status do host (a parada do host: o workbench deixa de
    /// enxergar o que as extensões registraram).
    pub fn dispose_main_thread(&self) {
        if let Some(status_bar) = &self.bridged.status_bar {
            status_bar.dispose_status_message();
        }
        if let Some(text_editors) = &self.bridged.text_editors {
            text_editors.editors.dispose();
            text_editors.documents.dispose();
        }
        if let Some(language_features) = &self.bridged.language_features {
            language_features.dispose();
        }
        if let Some(terminal) = &self.bridged.terminal {
            terminal.dispose();
        }
        if let Some(tree_views) = &self.bridged.tree_views {
            tree_views.dispose();
        }
        if let Some(debug) = &self.bridged.debug {
            debug.dispose();
        }
        for client in &self.bridged.main_thread {
            client.dispose();
        }
    }

    /// Esvazia as microtarefas dos dois mundos (as do motor e as das `Promise` do Rust) e os timers vencidos até estabilizar.
    pub fn pump(&self) {
        for _ in 0..64 {
            self.runner.drain_microtasks();
            je_base::runtime::turn();
        }
    }

    /// `Event::none()` serve de marcador para o laço do host quando não há evento externo.
    pub fn idle_event() -> Event<()> {
        Event::none()
    }
}
