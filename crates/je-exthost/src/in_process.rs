//! A ponte em processo entre o host de extensões e os clientes `MainThread*` do workbench, para os namespaces da API `vscode` além
//! de `commands`.
//!
//! Os dois lados do RPC são Rust no mesmo processo (decisão de arquitetura de `wip/notes/exthost-plan.md`): para os serviços que esta
//! ponte atende, o "proxy" que um `ExtHost*` obtém de `MainThread*` é o próprio cliente do workbench, sem serialização, e o proxy que
//! o cliente obtém de `ExtHost*` é o serviço do host. Os demais serviços (comandos, extensões, telemetria) seguem pelo `RPCProtocol`.
//!
//! O ciclo que isso cria (o `ExtHostWorkspace` precisa do proxy de `MainThreadWorkspace` ao nascer, e o `MainThreadWorkspace` chama
//! `$initializeWorkspace` ao nascer) é quebrado pelos encaminhadores `Late*`: o cliente do workbench é criado primeiro, com um
//! `ExtHost*` que ainda não existe; as chamadas sem retorno feitas nesse intervalo (`$initializeWorkspace`,
//! `$initializeConfiguration`, `$acceptStaticEntries`) ficam numa fila e são entregues ao serviço real assim que ele nasce
//! ([`Late::bind`]). Uma chamada com retorno antes do `bind` é erro de programação (o host só as faz depois de montado).
//!
//! Divergências:
//! - O original cria cada cliente `MainThread*` pelo registro de `extHostNamedCustomer`, uma vez por host; aqui quem monta os clientes
//!   é o [`IMainThreadBinder`] que o dono do host entrega (o aplicativo ou o teste), porque os clientes dependem de serviços do
//!   workbench (busca, editores, confiança) que o `je-exthost` não conhece.
//! - Sem serialização, um valor que o RPC revive por `URI.revive` ou `cloneAndChange` chega como o tipo Rust, igual ao que o
//!   `RPCProtocol` entregaria depois de revivido.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::buffer::VSBuffer;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::languages::{CodeActionContext, TextEdit as EditorTextEdit};
use je_editor::folding::syntax_range_provider::{FoldingContext, FoldingRange as EditorFoldingRange};
use je_editor::language_providers::{DocumentHighlight as EditorDocumentHighlight, DocumentSymbol as EditorDocumentSymbol, SelectionRange as EditorSelectionRange};
use je_editor::languages::{CompletionContext as EditorCompletionContext, StringOrMarkdown};
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_editor::services::clipboard_service::IClipboardService;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::configuration::ConfigurationChange;
use je_platform::edit_sessions::EditSessionIdentityMatch;
use je_platform::log::{LogLevel, LoggerService};
use je_platform::markers::markers::MarkerData;
use je_platform::quickinput::quick_input::InputValidationResult;
use je_workbench::api::common::ext_host_output::IDirectoryCreator;
use je_workbench::api::common::ext_host_protocol::{
    ChainedCacheId, DataTransferDto, EvaluatableExpressionDto, ExtHostDiagnosticsShape, ExtHostLanguageFeaturesShape, ExtHostLanguagesShape, FormattingOptions, HoverContextDto,
    HoverWithId, IColorPresentationDto, ICallHierarchyItemDto, ICodeActionListDto, ICodeLensDto, ICodeLensListDto, IDocumentDropEditDto, IDocumentPasteContextDto, IIncomingCallDto,
    IInlayHintDto, IInlayHintsDto, IInlineValueContextDto, ILanguageWordDefinitionDto, ILinkDto, ILinkedEditingRangesDto, ILinksListDto, ILocationDto, ILocationLinkDto,
    IOutgoingCallDto, IPasteEditDto, IRawColorInfo, ISignatureHelpContextDto, ISignatureHelpDto, ISuggestDataDto, ISuggestResultDto, ITypeHierarchyItemDto, IWorkspaceSymbolDto,
    IWorkspaceSymbolsDto, InlineValueDto, LanguagesNewSymbolNameTriggerKind, MultiDocumentHighlightDto, NewSymbolNameDto, RangeOrSelection, ReferenceContext, RenameEditsDto,
    RenameLocationDto, ResolvedCodeActionDto, ResolvedDropEditDto, ResolvedPasteEditDto,
};
use je_workbench::services::inline_completions::inline_completions_unification::IInlineCompletionsUnificationState;
use je_base::arrays::OneOrMany;
use je_platform::terminal::environment_variable::ISerializableEnvironmentVariableCollection;
use je_platform::terminal::terminal::{ITerminalLaunchError, ITerminalProfile, TerminalExitReason as PlatformExitReason, TerminalShellType};
use je_workbench::api::common::ext_host_protocol::{
    ActiveCommentInfo, CheckboxUpdate, CommentChanges, CommentReaction, CommentThreadChanges, CommentingRangesDto, ExtHostCommentsShape, ExtHostSCMShape, ExtHostTerminalServiceShape,
    ExtHostTreeViewsShape, ICreateContributedTerminalProfileOptions, ISCMHistoryOptions, ITerminalCommandDto,
    ITerminalCompletionContextDto, ITerminalDimensionsDto, ITerminalLinkDto, IShellLaunchConfigDto, SCMArtifactDto, SCMArtifactGroupDto, SCMHistoryItemChangeDto, SCMHistoryItemDto,
    SCMHistoryItemRefDto, TerminalCommandMatchResultDto, TerminalCompletionListDto, TerminalQuickFix, TreeChildrenBatch,
};
use je_workbench::api::common::ext_host_debug_protocol::{ExtHostDebugServiceShape, IBreakpointsDeltaDto, IDebugSessionDto, StackFocusDto};
use je_workbench::api::common::ext_host_protocol::ExtHostAuthenticationShape;
use je_workbench::api::common::ext_host_protocol::{ExtHostSecretStateShape, ExtHostStorageShape};use je_workbench::api::common::ext_host_protocol::{
    ExtHostNotebookDocumentSaveParticipantShape, ExtHostNotebookDocumentsAndEditorsShape, ExtHostNotebookDocumentsShape, ExtHostNotebookKernelsShape, ExtHostNotebookShape, INotebookCellStatusBarListDto,
    INotebookDocumentsAndEditorsDelta, NotebookCellsChangedEventDto, NotebookDataDto, NotebookSaveResult, SerializableObjectWithBuffers, VariablesRequestKindDto,
};
use je_workbench::contrib::notebook::common::notebook_common::INotebookKernelSourceAction;
use je_platform::files::files::WriteFileOptions;
use je_workbench::services::authentication::authentication::{
    AuthenticationSession, IAuthenticationConstraint, IAuthenticationCreateSessionOptions, IAuthenticationGetSessionsOptions,
};
use je_workbench::common::views::ITreeItem;
use je_workbench::contrib::debug::debug::{
    IAdapterDescriptor, IConfig, IDebugVisualization, IDebugVisualizationContext, IDebugVisualizationTreeItem, MainThreadDebugVisualization,
};
use je_workbench::contrib::debug::debug_protocol::ProtocolMessage;
use je_workbench::api::common::ext_host_protocol::{
    DocumentsAndEditorsDelta, EditorPropertiesChangeData, ExtHostConfigurationShape, ExtHostContext, ExtHostDocumentsAndEditorsShape, ExtHostDocumentsShape,
    ExtHostEditorsShape, ExtHostFileSystemEventServiceShape, ExtHostFileSystemShape, ExtHostOutputServiceShape, ExtHostProgressShape, ExtHostQuickOpenShape, ExtHostStatusBarShape, ExtHostWorkspaceShape, IConfigurationInitData,
    FileSystemEvents, IMainContext, IRawFileMatch2, ITextEditorDiffInformation, IWillRunFileOperationParticipation, IWorkspaceData, SerializedModelContentChangedEvent,
    SourceTargetPair, StatusBarItemDto, TextEditorPositionData,
};
use je_platform::files::files::{FileDeleteOptions, FileOpenOptions, FileOperation, FileOverwriteOptions, FileType, FileWriteOptions, Stat, WatchOptions};

use je_workbench::api::common::ext_host_telemetry::ExtHostTelemetry;

use crate::api::env::HostEnvironment;
use crate::extension_storage_paths::ExtensionStorageEnvironment;
use crate::local_context::LocalContext;

/// O que o host precisa do lado do processo dele para criar canais de saída: no original o `ExtHostOutputService` usa o `ILoggerService`
/// e o `IExtHostConsumerFileSystem` do próprio processo do host, e o `initData` (`logsLocation`, `extensionLogLevel`). Aqui o host não
/// tem esses serviços, então quem monta o `MainThreadOutputService` os entrega junto.
pub struct OutputEnvironment {
    /// `initData.logsLocation`.
    pub logs_location: URI,
    /// `initData.environment.extensionLogLevel`.
    pub extension_log_levels: Vec<(ExtensionIdentifier, LogLevel)>,
    /// `IExtHostConsumerFileSystem.value.createDirectory`.
    pub directory_creator: Rc<dyn IDirectoryCreator>,
    /// `ILoggerService`.
    pub logger_service: Rc<dyn LoggerService>,
}

/// Quem monta os clientes `MainThread*` em processo.
pub trait IMainThreadBinder {
    /// Cria os clientes que o dono do host quer ligar e os registra em `context` (`context.set(&MainContext::MAIN_THREAD_X, cliente)`).
    /// `context.get_proxy(&ExtHostContext::EXT_HOST_X)` já devolve o encaminhador do serviço do host. O contexto vem como `Rc`
    /// porque alguns clientes (o `MainThreadWorkspace`, `Injectable`) o recebem por valor. Devolve o que precisa ser descartado junto
    /// com o host.
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>>;

    /// O ambiente do `ExtHostOutputService`, se o binder registrou o `MainThreadOutputService`. Sem ele o host não cria canais de
    /// saída, mesmo com o cliente registrado.
    fn output_environment(&self) -> Option<OutputEnvironment> {
        None
    }

    /// Os valores de `vscode.env` que o original lê do `initData` (`appName`, `appRoot`, `language`, `machineId`, `sessionId`,
    /// `uriScheme`, `shell`, `uiKind`). Sem eles esses membros não existem em `vscode.env`.
    fn host_environment(&self) -> Option<HostEnvironment> {
        None
    }

    /// O `ExtHostTelemetry` do host (`isTelemetryEnabled`, `onDidChangeTelemetryEnabled` e `createTelemetryLogger`). Quem o cria também
    /// registra o encaminhador `ExtHostTelemetryShape` no contexto, para o nível de telemetria chegar a ele. Sem ele esses membros não
    /// existem em `vscode.env`.
    fn telemetry(&self) -> Option<Rc<ExtHostTelemetry>> {
        None
    }

    /// O `IClipboardService` do workbench, que `vscode.env.clipboard` usa. Sem ele o membro não existe.
    fn clipboard_service(&self) -> Option<Rc<dyn IClipboardService>> {
        None
    }

    /// Os diretórios de log e de armazenamento que o `ExtensionContext` expõe (`logUri`, `storageUri`, `globalStorageUri` e os caminhos
    /// equivalentes). Sem eles esses membros ficam de fora do contexto.
    fn extension_storage_environment(&self) -> Option<ExtensionStorageEnvironment> {
        None
    }
}

/// Um encaminhador preguiçoso para o `T` que ainda não existe (ver o cabeçalho do módulo).
pub struct Late<T: ?Sized> {
    target: RefCell<Option<Rc<T>>>,
    pending: RefCell<Vec<Box<dyn FnOnce(&T)>>>,
}

impl<T: ?Sized> Default for Late<T> {
    fn default() -> Self {
        Self { target: RefCell::new(None), pending: RefCell::new(Vec::new()) }
    }
}

impl<T: ?Sized> Late<T> {
    /// Liga o serviço real e entrega a ele, em ordem, as chamadas sem retorno que esperaram.
    pub fn bind(&self, target: Rc<T>) {
        *self.target.borrow_mut() = Some(target.clone());
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for call in pending {
            call(&*target);
        }
    }

    /// Uma chamada sem retorno: entrega já se o serviço existe, senão guarda para o `bind`.
    fn call_void(&self, call: impl FnOnce(&T) + 'static) {
        let target = self.target.borrow().clone();
        match target {
            Some(target) => call(&*target),
            None => self.pending.borrow_mut().push(Box::new(call)),
        }
    }

    /// O serviço real, para as chamadas com retorno.
    fn bound(&self) -> Rc<T> {
        self.target.borrow().clone().expect("o serviço do host ainda não foi ligado ao encaminhador")
    }
}

/// O `ExtHostWorkspaceShape` que ainda não existe quando `MainThreadWorkspace` nasce.
#[derive(Default)]
pub struct LateExtHostWorkspace(pub Late<dyn ExtHostWorkspaceShape>);

impl ExtHostWorkspaceShape for LateExtHostWorkspace {
    fn rpc_initialize_workspace(&self, workspace: Option<IWorkspaceData>, trusted: bool) {
        self.0.call_void(move |target| target.rpc_initialize_workspace(workspace, trusted));
    }

    fn rpc_accept_workspace_data(&self, workspace: Option<IWorkspaceData>) {
        self.0.call_void(move |target| target.rpc_accept_workspace_data(workspace));
    }

    fn rpc_handle_text_search_result(&self, result: IRawFileMatch2, request_id: u64) {
        self.0.call_void(move |target| target.rpc_handle_text_search_result(result, request_id));
    }

    fn rpc_on_did_grant_workspace_trust(&self) {
        self.0.call_void(|target| target.rpc_on_did_grant_workspace_trust());
    }

    fn rpc_on_did_change_workspace_trusted_folders(&self) {
        self.0.call_void(|target| target.rpc_on_did_change_workspace_trusted_folders());
    }

    fn rpc_get_edit_session_identifier(&self, folder: URI, token: CancellationToken) -> Promise<Option<String>> {
        self.0.bound().rpc_get_edit_session_identifier(folder, token)
    }

    fn rpc_provide_edit_session_identity_match(&self, folder: URI, identity1: &str, identity2: &str, token: CancellationToken) -> Promise<Option<EditSessionIdentityMatch>> {
        self.0.bound().rpc_provide_edit_session_identity_match(folder, identity1, identity2, token)
    }

    fn rpc_on_will_create_edit_session_identity(&self, folder: URI, token: CancellationToken, timeout: f64) -> Promise<()> {
        self.0.bound().rpc_on_will_create_edit_session_identity(folder, token, timeout)
    }

    fn rpc_provide_canonical_uri(&self, uri: URI, target_scheme: &str, token: CancellationToken) -> Promise<Option<URI>> {
        self.0.bound().rpc_provide_canonical_uri(uri, target_scheme, token)
    }
}

/// O `ExtHostConfigurationShape` que ainda não existe quando `MainThreadConfiguration` nasce.
#[derive(Default)]
pub struct LateExtHostConfiguration(pub Late<dyn ExtHostConfigurationShape>);

impl ExtHostConfigurationShape for LateExtHostConfiguration {
    fn rpc_initialize_configuration(&self, data: IConfigurationInitData) {
        self.0.call_void(move |target| target.rpc_initialize_configuration(data));
    }

    fn rpc_accept_configuration_changed(&self, data: IConfigurationInitData, change: ConfigurationChange) {
        self.0.call_void(move |target| target.rpc_accept_configuration_changed(data, change));
    }
}

/// O `ExtHostStatusBarShape` que ainda não existe quando `MainThreadStatusBar` nasce.
#[derive(Default)]
pub struct LateExtHostStatusBar(pub Late<dyn ExtHostStatusBarShape>);

impl ExtHostStatusBarShape for LateExtHostStatusBar {
    fn rpc_accept_static_entries(&self, added: Vec<StatusBarItemDto>) {
        self.0.call_void(move |target| target.rpc_accept_static_entries(added));
    }

    fn rpc_provide_tooltip(&self, entry_id: &str, cancellation: CancellationToken) -> Promise<Option<StringOrMarkdown>> {
        self.0.bound().rpc_provide_tooltip(entry_id, cancellation)
    }
}

/// O `ExtHostQuickOpenShape` que ainda não existe quando `MainThreadQuickOpen` nasce.
#[derive(Default)]
pub struct LateExtHostQuickOpen(pub Late<dyn ExtHostQuickOpenShape>);

impl ExtHostQuickOpenShape for LateExtHostQuickOpen {
    fn rpc_on_item_selected(&self, handle: u32) {
        self.0.call_void(move |target| target.rpc_on_item_selected(handle));
    }

    fn rpc_validate_input(&self, input: &str) -> Promise<InputValidationResult> {
        self.0.bound().rpc_validate_input(input)
    }

    fn rpc_on_did_change_active(&self, session_id: u32, handles: Vec<u32>) {
        self.0.call_void(move |target| target.rpc_on_did_change_active(session_id, handles));
    }

    fn rpc_on_did_change_selection(&self, session_id: u32, handles: Vec<u32>) {
        self.0.call_void(move |target| target.rpc_on_did_change_selection(session_id, handles));
    }

    fn rpc_on_did_accept(&self, session_id: u32) {
        self.0.call_void(move |target| target.rpc_on_did_accept(session_id));
    }

    fn rpc_on_did_change_value(&self, session_id: u32, value: &str) {
        let value = value.to_owned();
        self.0.call_void(move |target| target.rpc_on_did_change_value(session_id, &value));
    }

    fn rpc_on_did_trigger_button(&self, session_id: u32, handle: Option<i64>, checked: Option<bool>) {
        self.0.call_void(move |target| target.rpc_on_did_trigger_button(session_id, handle, checked));
    }

    fn rpc_on_did_trigger_item_button(&self, session_id: u32, item_handle: Option<u32>, button_handle: Option<i64>, checked: Option<bool>) {
        self.0.call_void(move |target| target.rpc_on_did_trigger_item_button(session_id, item_handle, button_handle, checked));
    }

    fn rpc_on_did_hide(&self, session_id: u32) {
        self.0.call_void(move |target| target.rpc_on_did_hide(session_id));
    }
}

/// O `ExtHostOutputServiceShape` que ainda não existe quando `MainThreadOutputService` nasce (ele avisa o canal visível no próprio
/// construtor).
#[derive(Default)]
pub struct LateExtHostOutputService(pub Late<dyn ExtHostOutputServiceShape>);

impl ExtHostOutputServiceShape for LateExtHostOutputService {
    fn rpc_set_visible_channel(&self, visible_channel_id: Option<String>) {
        self.0.call_void(move |target| target.rpc_set_visible_channel(visible_channel_id));
    }
}

/// O `ExtHostProgressShape` que ainda não existe quando `MainThreadProgress` nasce.
#[derive(Default)]
pub struct LateExtHostProgress(pub Late<dyn ExtHostProgressShape>);

impl ExtHostProgressShape for LateExtHostProgress {
    fn rpc_accept_progress_canceled(&self, handle: u32) {
        self.0.call_void(move |target| target.rpc_accept_progress_canceled(handle));
    }
}

/// O `ExtHostFileSystemShape` que ainda não existe quando `MainThreadFileSystem` nasce (o `ExtHostFileSystem` só nasce no primeiro
/// `registerFileSystemProvider`, e o cliente só o chama depois do registro).
#[derive(Default)]
pub struct LateExtHostFileSystem(pub Late<dyn ExtHostFileSystemShape>);

impl ExtHostFileSystemShape for LateExtHostFileSystem {
    fn rpc_stat(&self, handle: u32, resource: &URI) -> Promise<Stat> {
        self.0.bound().rpc_stat(handle, resource)
    }

    fn rpc_readdir(&self, handle: u32, resource: &URI) -> Promise<Vec<(String, FileType)>> {
        self.0.bound().rpc_readdir(handle, resource)
    }

    fn rpc_read_file(&self, handle: u32, resource: &URI) -> Promise<VSBuffer> {
        self.0.bound().rpc_read_file(handle, resource)
    }

    fn rpc_write_file(&self, handle: u32, resource: &URI, content: VSBuffer, opts: FileWriteOptions) -> Promise<()> {
        self.0.bound().rpc_write_file(handle, resource, content, opts)
    }

    fn rpc_rename(&self, handle: u32, resource: &URI, target: &URI, opts: FileOverwriteOptions) -> Promise<()> {
        self.0.bound().rpc_rename(handle, resource, target, opts)
    }

    fn rpc_copy(&self, handle: u32, resource: &URI, target: &URI, opts: FileOverwriteOptions) -> Promise<()> {
        self.0.bound().rpc_copy(handle, resource, target, opts)
    }

    fn rpc_mkdir(&self, handle: u32, resource: &URI) -> Promise<()> {
        self.0.bound().rpc_mkdir(handle, resource)
    }

    fn rpc_delete(&self, handle: u32, resource: &URI, opts: FileDeleteOptions) -> Promise<()> {
        self.0.bound().rpc_delete(handle, resource, opts)
    }

    fn rpc_watch(&self, handle: u32, session: u32, resource: &URI, opts: WatchOptions) {
        let resource = resource.clone();
        self.0.call_void(move |target| target.rpc_watch(handle, session, &resource, opts));
    }

    fn rpc_unwatch(&self, handle: u32, session: u32) {
        self.0.call_void(move |target| target.rpc_unwatch(handle, session));
    }

    fn rpc_open(&self, handle: u32, resource: &URI, opts: FileOpenOptions) -> Promise<u32> {
        self.0.bound().rpc_open(handle, resource, opts)
    }

    fn rpc_close(&self, handle: u32, fd: u32) -> Promise<()> {
        self.0.bound().rpc_close(handle, fd)
    }

    fn rpc_read(&self, handle: u32, fd: u32, pos: u64, length: usize) -> Promise<VSBuffer> {
        self.0.bound().rpc_read(handle, fd, pos, length)
    }

    fn rpc_write(&self, handle: u32, fd: u32, pos: u64, data: VSBuffer) -> Promise<usize> {
        self.0.bound().rpc_write(handle, fd, pos, data)
    }
}

/// O `ExtHostFileSystemEventServiceShape` que ainda não existe quando `MainThreadFileSystemEventService` nasce.
#[derive(Default)]
pub struct LateExtHostFileSystemEventService(pub Late<dyn ExtHostFileSystemEventServiceShape>);

impl ExtHostFileSystemEventServiceShape for LateExtHostFileSystemEventService {
    fn rpc_on_file_event(&self, events: FileSystemEvents) {
        self.0.call_void(move |target| target.rpc_on_file_event(events));
    }

    fn rpc_on_will_run_file_operation(
        &self,
        operation: FileOperation,
        files: Vec<SourceTargetPair>,
        timeout: f64,
        token: CancellationToken,
    ) -> Promise<Option<IWillRunFileOperationParticipation>> {
        self.0.bound().rpc_on_will_run_file_operation(operation, files, timeout, token)
    }

    fn rpc_on_did_run_file_operation(&self, operation: FileOperation, files: Vec<SourceTargetPair>) {
        self.0.call_void(move |target| target.rpc_on_did_run_file_operation(operation, files));
    }
}

/// O `ExtHostDocumentsAndEditorsShape` que ainda não existe quando `MainThreadDocumentsAndEditors` nasce (o construtor dele já
/// entrega o delta inicial).
#[derive(Default)]
pub struct LateExtHostDocumentsAndEditors(pub Late<dyn ExtHostDocumentsAndEditorsShape>);

impl ExtHostDocumentsAndEditorsShape for LateExtHostDocumentsAndEditors {
    fn rpc_accept_documents_and_editors_delta(&self, delta: DocumentsAndEditorsDelta) {
        self.0.call_void(move |target| target.rpc_accept_documents_and_editors_delta(delta));
    }
}

/// O `ExtHostDocumentsShape` que ainda não existe quando `MainThreadDocuments` nasce.
#[derive(Default)]
pub struct LateExtHostDocuments(pub Late<dyn ExtHostDocumentsShape>);

impl ExtHostDocumentsShape for LateExtHostDocuments {
    fn rpc_accept_model_language_changed(&self, uri: &URI, new_language_id: &str) {
        let (uri, new_language_id) = (uri.clone(), new_language_id.to_owned());
        self.0.call_void(move |target| target.rpc_accept_model_language_changed(&uri, &new_language_id));
    }

    fn rpc_accept_model_saved(&self, uri: &URI) {
        let uri = uri.clone();
        self.0.call_void(move |target| target.rpc_accept_model_saved(&uri));
    }

    fn rpc_accept_dirty_state_changed(&self, uri: &URI, is_dirty: bool) {
        let uri = uri.clone();
        self.0.call_void(move |target| target.rpc_accept_dirty_state_changed(&uri, is_dirty));
    }

    fn rpc_accept_encoding_changed(&self, uri: &URI, encoding: &str) {
        let (uri, encoding) = (uri.clone(), encoding.to_owned());
        self.0.call_void(move |target| target.rpc_accept_encoding_changed(&uri, &encoding));
    }

    fn rpc_accept_model_changed(&self, uri: &URI, e: SerializedModelContentChangedEvent, is_dirty: bool) {
        let uri = uri.clone();
        self.0.call_void(move |target| target.rpc_accept_model_changed(&uri, e, is_dirty));
    }
}

/// O `ExtHostEditorsShape` que ainda não existe quando `MainThreadTextEditors` nasce.
#[derive(Default)]
pub struct LateExtHostEditors(pub Late<dyn ExtHostEditorsShape>);

impl ExtHostEditorsShape for LateExtHostEditors {
    fn rpc_accept_editor_properties_changed(&self, id: &str, props: EditorPropertiesChangeData) {
        let id = id.to_owned();
        self.0.call_void(move |target| target.rpc_accept_editor_properties_changed(&id, props));
    }

    fn rpc_accept_editor_position_data(&self, data: TextEditorPositionData) {
        self.0.call_void(move |target| target.rpc_accept_editor_position_data(data));
    }

    fn rpc_accept_editor_diff_information(&self, id: &str, diff_information: Option<Vec<ITextEditorDiffInformation>>) {
        let id = id.to_owned();
        self.0.call_void(move |target| target.rpc_accept_editor_diff_information(&id, diff_information));
    }
}

/// O `ExtHostDiagnosticsShape` que ainda não existe quando `MainThreadDiagnostics` nasce.
#[derive(Default)]
pub struct LateExtHostDiagnostics(pub Late<dyn ExtHostDiagnosticsShape>);

impl ExtHostDiagnosticsShape for LateExtHostDiagnostics {
    fn rpc_accept_markers_change(&self, data: Vec<(URI, Vec<MarkerData>)>) {
        self.0.call_void(move |target| target.rpc_accept_markers_change(data));
    }
}

/// O `ExtHostLanguagesShape` que ainda não existe quando `MainThreadLanguages` nasce (o construtor dele já entrega os ids de
/// linguagem).
#[derive(Default)]
pub struct LateExtHostLanguages(pub Late<dyn ExtHostLanguagesShape>);

impl ExtHostLanguagesShape for LateExtHostLanguages {
    fn rpc_accept_language_ids(&self, ids: Vec<String>) {
        self.0.call_void(move |target| target.rpc_accept_language_ids(ids));
    }

    fn rpc_accept_syntax_highlighting_theme_changed(&self) {
        self.0.call_void(|target| target.rpc_accept_syntax_highlighting_theme_changed());
    }
}

/// Repassa ao serviço real as chamadas COM retorno do `ExtHostLanguageFeaturesShape` (só valem depois do `bind`).
macro_rules! late_language_features_calls {
    ($($method:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        $(
            fn $method(&self, $($arg: $ty),*) -> $ret {
                self.0.bound().$method($($arg),*)
            }
        )*
    };
}

/// Guarda na fila as chamadas SEM retorno (e sem referência) do `ExtHostLanguageFeaturesShape` feitas antes do `bind`.
macro_rules! late_language_features_queued {
    ($($method:ident($($arg:ident: $ty:ty),*);)*) => {
        $(
            fn $method(&self, $($arg: $ty),*) {
                self.0.call_void(move |target| target.$method($($arg),*));
            }
        )*
    };
}

/// O `ExtHostLanguageFeaturesShape` que ainda não existe quando `MainThreadLanguageFeatures` nasce (o construtor dele, com o serviço de
/// linguagem, já entrega as definições de palavra).
#[derive(Default)]
pub struct LateExtHostLanguageFeatures(pub Late<dyn ExtHostLanguageFeaturesShape>);

impl ExtHostLanguageFeaturesShape for LateExtHostLanguageFeatures {
    late_language_features_calls! {
        rpc_provide_document_symbols(handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<Vec<EditorDocumentSymbol>>>;
        rpc_provide_code_lenses(handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<ICodeLensListDto>>;
        rpc_resolve_code_lens(handle: u32, symbol: ICodeLensDto, token: CancellationToken) -> Promise<Option<ICodeLensDto>>;
        rpc_provide_definition(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        rpc_provide_declaration(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        rpc_provide_implementation(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        rpc_provide_type_definition(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        rpc_provide_hover(handle: u32, resource: &URI, position: EditorPosition, context: Option<HoverContextDto>, token: CancellationToken) -> Promise<Option<HoverWithId>>;
        rpc_provide_document_highlights(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<EditorDocumentHighlight>>>;
        rpc_provide_multi_document_highlights(
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            other_models: Vec<URI>,
            token: CancellationToken
        ) -> Promise<Option<Vec<MultiDocumentHighlightDto>>>;
        rpc_provide_references(handle: u32, resource: &URI, position: EditorPosition, context: ReferenceContext, token: CancellationToken) -> Promise<Option<Vec<ILocationDto>>>;
        rpc_provide_evaluatable_expression(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<EvaluatableExpressionDto>>;
        rpc_provide_inline_values(handle: u32, resource: &URI, range: EditorRange, context: IInlineValueContextDto, token: CancellationToken) -> Promise<Option<Vec<InlineValueDto>>>;
        rpc_provide_linked_editing_ranges(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<ILinkedEditingRangesDto>>;
        rpc_provide_code_actions(
            handle: u32,
            resource: &URI,
            range_or_selection: RangeOrSelection,
            context: CodeActionContext,
            token: CancellationToken
        ) -> Promise<Option<ICodeActionListDto>>;
        rpc_resolve_code_action(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedCodeActionDto>;
        rpc_prepare_document_paste(handle: u32, uri: &URI, ranges: Vec<EditorRange>, data_transfer: DataTransferDto, token: CancellationToken) -> Promise<Option<DataTransferDto>>;
        rpc_provide_paste_edits(
            handle: u32,
            request_id: u32,
            uri: &URI,
            ranges: Vec<EditorRange>,
            data_transfer: DataTransferDto,
            context: IDocumentPasteContextDto,
            token: CancellationToken
        ) -> Promise<Option<Vec<IPasteEditDto>>>;
        rpc_resolve_paste_edit(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedPasteEditDto>;
        rpc_provide_document_formatting_edits(handle: u32, resource: &URI, options: FormattingOptions, token: CancellationToken) -> Promise<Option<Vec<EditorTextEdit>>>;
        rpc_provide_document_range_formatting_edits(
            handle: u32,
            resource: &URI,
            range: EditorRange,
            options: FormattingOptions,
            token: CancellationToken
        ) -> Promise<Option<Vec<EditorTextEdit>>>;
        rpc_provide_document_ranges_formatting_edits(
            handle: u32,
            resource: &URI,
            ranges: Vec<EditorRange>,
            options: FormattingOptions,
            token: CancellationToken
        ) -> Promise<Option<Vec<EditorTextEdit>>>;
        rpc_provide_on_type_formatting_edits(
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            ch: &str,
            options: FormattingOptions,
            token: CancellationToken
        ) -> Promise<Option<Vec<EditorTextEdit>>>;
        rpc_provide_workspace_symbols(handle: u32, search: &str, token: CancellationToken) -> Promise<IWorkspaceSymbolsDto>;
        rpc_resolve_workspace_symbol(handle: u32, symbol: IWorkspaceSymbolDto, token: CancellationToken) -> Promise<Option<IWorkspaceSymbolDto>>;
        rpc_provide_rename_edits(handle: u32, resource: &URI, position: EditorPosition, new_name: &str, token: CancellationToken) -> Promise<Option<RenameEditsDto>>;
        rpc_resolve_rename_location(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<RenameLocationDto>>;
        rpc_supports_automatic_new_symbol_names_trigger_kind(handle: u32) -> Promise<Option<bool>>;
        rpc_provide_new_symbol_names(
            handle: u32,
            resource: &URI,
            range: EditorRange,
            trigger_kind: LanguagesNewSymbolNameTriggerKind,
            token: CancellationToken
        ) -> Promise<Option<Vec<NewSymbolNameDto>>>;
        rpc_provide_document_semantic_tokens(handle: u32, resource: &URI, previous_result_id: u32, token: CancellationToken) -> Promise<Option<VSBuffer>>;
        rpc_provide_document_range_semantic_tokens(handle: u32, resource: &URI, range: EditorRange, token: CancellationToken) -> Promise<Option<VSBuffer>>;
        rpc_provide_completion_items(
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            context: EditorCompletionContext,
            token: CancellationToken
        ) -> Promise<Option<ISuggestResultDto>>;
        rpc_resolve_completion_item(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<ISuggestDataDto>>;
        rpc_provide_signature_help(
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            context: ISignatureHelpContextDto,
            token: CancellationToken
        ) -> Promise<Option<ISignatureHelpDto>>;
        rpc_provide_inlay_hints(handle: u32, resource: &URI, range: EditorRange, token: CancellationToken) -> Promise<Option<IInlayHintsDto>>;
        rpc_resolve_inlay_hint(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<IInlayHintDto>>;
        rpc_provide_document_links(handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<ILinksListDto>>;
        rpc_resolve_document_link(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<ILinkDto>>;
        rpc_provide_document_colors(handle: u32, resource: &URI, token: CancellationToken) -> Promise<Vec<IRawColorInfo>>;
        rpc_provide_color_presentations(handle: u32, resource: &URI, color_info: IRawColorInfo, token: CancellationToken) -> Promise<Option<Vec<IColorPresentationDto>>>;
        rpc_provide_folding_ranges(handle: u32, resource: &URI, context: FoldingContext, token: CancellationToken) -> Promise<Option<Vec<EditorFoldingRange>>>;
        rpc_provide_selection_ranges(handle: u32, resource: &URI, positions: Vec<EditorPosition>, token: CancellationToken) -> Promise<Vec<Vec<EditorSelectionRange>>>;
        rpc_prepare_call_hierarchy(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<ICallHierarchyItemDto>>>;
        rpc_provide_call_hierarchy_incoming_calls(handle: u32, session_id: &str, item_id: &str, token: CancellationToken) -> Promise<Option<Vec<IIncomingCallDto>>>;
        rpc_provide_call_hierarchy_outgoing_calls(handle: u32, session_id: &str, item_id: &str, token: CancellationToken) -> Promise<Option<Vec<IOutgoingCallDto>>>;
        rpc_prepare_type_hierarchy(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<ITypeHierarchyItemDto>>>;
        rpc_provide_type_hierarchy_supertypes(handle: u32, session_id: &str, item_id: &str, token: CancellationToken) -> Promise<Option<Vec<ITypeHierarchyItemDto>>>;
        rpc_provide_type_hierarchy_subtypes(handle: u32, session_id: &str, item_id: &str, token: CancellationToken) -> Promise<Option<Vec<ITypeHierarchyItemDto>>>;
        rpc_provide_document_on_drop_edits(
            handle: u32,
            request_id: u32,
            resource: &URI,
            position: EditorPosition,
            data_transfer: DataTransferDto,
            token: CancellationToken
        ) -> Promise<Option<Vec<IDocumentDropEditDto>>>;
        rpc_resolve_drop_edit(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedDropEditDto>;
    }

    late_language_features_queued! {
        rpc_release_code_lenses(handle: u32, cache_id: u32);
        rpc_release_hover(handle: u32, id: u32);
        rpc_release_code_actions(handle: u32, cache_id: u32);
        rpc_release_paste_edits(handle: u32, cache_id: u32);
        rpc_release_workspace_symbols(handle: u32, id: u32);
        rpc_release_document_semantic_tokens(handle: u32, semantic_coloring_result_id: u32);
        rpc_release_completion_items(handle: u32, id: u32);
        rpc_release_signature_help(handle: u32, id: u32);
        rpc_release_inlay_hints(handle: u32, id: u32);
        rpc_release_document_links(handle: u32, id: u32);
        rpc_release_document_on_drop_edits(handle: u32, cache_id: u32);
        rpc_set_word_definitions(word_definitions: Vec<ILanguageWordDefinitionDto>);
        rpc_accept_inline_completions_unification_state(state: IInlineCompletionsUnificationState);
    }

    fn rpc_release_call_hierarchy(&self, handle: u32, session_id: &str) {
        let session_id = session_id.to_owned();
        self.0.call_void(move |target| target.rpc_release_call_hierarchy(handle, &session_id));
    }

    fn rpc_release_type_hierarchy(&self, handle: u32, session_id: &str) {
        let session_id = session_id.to_owned();
        self.0.call_void(move |target| target.rpc_release_type_hierarchy(handle, &session_id));
    }
}

/// O `ExtHostTerminalServiceShape` que ainda não existe quando `MainThreadTerminalService` nasce (o construtor dele já entrega os
/// terminais abertos, o ativo e o perfil padrão). As chamadas com retorno de uma promessa vazia que chegam antes do `bind` ficam na
/// fila e devolvem a promessa já resolvida.
#[derive(Default)]
pub struct LateExtHostTerminalService(pub Late<dyn ExtHostTerminalServiceShape>);

impl LateExtHostTerminalService {
    /// Uma chamada que devolve `Promise<()>`: entrega já se o serviço existe, senão guarda para o `bind` e devolve a promessa resolvida.
    fn deferred(&self, call: impl FnOnce(&dyn ExtHostTerminalServiceShape) -> Promise<()> + 'static) -> Promise<()> {
        let target = self.0.target.borrow().clone();
        match target {
            Some(target) => call(&*target),
            None => {
                self.0.call_void(move |target| drop(call(target)));
                Promise::resolved(())
            }
        }
    }
}

impl IDisposable for LateExtHostTerminalService {
    fn dispose(&self) {}
}

impl ExtHostTerminalServiceShape for LateExtHostTerminalService {
    fn rpc_accept_active_terminal_changed(&self, id: Option<u32>) -> Promise<()> {
        self.deferred(move |target| target.rpc_accept_active_terminal_changed(id))
    }

    fn rpc_accept_terminal_process_data(&self, id: u32, data: &str) -> Promise<()> {
        let data = data.to_owned();
        self.deferred(move |target| target.rpc_accept_terminal_process_data(id, &data))
    }

    fn rpc_accept_terminal_dimensions(&self, id: u32, cols: i32, rows: i32) -> Promise<()> {
        self.deferred(move |target| target.rpc_accept_terminal_dimensions(id, cols, rows))
    }

    fn rpc_accept_did_execute_command(&self, id: u32, command: ITerminalCommandDto) -> Promise<()> {
        self.deferred(move |target| target.rpc_accept_did_execute_command(id, command))
    }

    fn rpc_accept_terminal_maximum_dimensions(&self, id: u32, cols: i32, rows: i32) -> Promise<()> {
        self.deferred(move |target| target.rpc_accept_terminal_maximum_dimensions(id, cols, rows))
    }

    fn rpc_accept_terminal_title_change(&self, id: u32, name: &str) -> Promise<()> {
        let name = name.to_owned();
        self.deferred(move |target| target.rpc_accept_terminal_title_change(id, &name))
    }

    fn rpc_accept_terminal_closed(&self, id: u32, exit_code: Option<i32>, exit_reason: PlatformExitReason) -> Promise<()> {
        self.deferred(move |target| target.rpc_accept_terminal_closed(id, exit_code, exit_reason))
    }

    fn rpc_accept_terminal_opened(&self, id: u32, ext_host_terminal_id: Option<&str>, name: &str, shell_launch_config: IShellLaunchConfigDto) {
        let (ext_host_terminal_id, name) = (ext_host_terminal_id.map(str::to_owned), name.to_owned());
        self.0.call_void(move |target| target.rpc_accept_terminal_opened(id, ext_host_terminal_id.as_deref(), &name, shell_launch_config));
    }

    fn rpc_accept_terminal_process_id(&self, id: u32, process_id: i32) -> Promise<()> {
        self.deferred(move |target| target.rpc_accept_terminal_process_id(id, process_id))
    }

    fn rpc_start_extension_terminal(&self, id: u32, initial_dimensions: Option<ITerminalDimensionsDto>) -> Promise<Option<ITerminalLaunchError>> {
        self.0.bound().rpc_start_extension_terminal(id, initial_dimensions)
    }

    fn rpc_accept_process_ack_data_event(&self, id: u32, char_count: usize) {
        self.0.call_void(move |target| target.rpc_accept_process_ack_data_event(id, char_count));
    }

    fn rpc_accept_process_input(&self, id: u32, data: &str) {
        let data = data.to_owned();
        self.0.call_void(move |target| target.rpc_accept_process_input(id, &data));
    }

    fn rpc_accept_terminal_interaction(&self, id: u32) {
        self.0.call_void(move |target| target.rpc_accept_terminal_interaction(id));
    }

    fn rpc_accept_terminal_selection(&self, id: u32, selection: Option<&str>) {
        let selection = selection.map(str::to_owned);
        self.0.call_void(move |target| target.rpc_accept_terminal_selection(id, selection.as_deref()));
    }

    fn rpc_accept_process_resize(&self, id: u32, cols: i32, rows: i32) {
        self.0.call_void(move |target| target.rpc_accept_process_resize(id, cols, rows));
    }

    fn rpc_accept_process_shutdown(&self, id: u32, immediate: bool) {
        self.0.call_void(move |target| target.rpc_accept_process_shutdown(id, immediate));
    }

    fn rpc_accept_process_request_initial_cwd(&self, id: u32) {
        self.0.call_void(move |target| target.rpc_accept_process_request_initial_cwd(id));
    }

    fn rpc_accept_process_request_cwd(&self, id: u32) {
        self.0.call_void(move |target| target.rpc_accept_process_request_cwd(id));
    }

    fn rpc_accept_process_request_latency(&self, id: u32) -> Promise<u32> {
        self.0.bound().rpc_accept_process_request_latency(id)
    }

    fn rpc_accept_terminal_shell_type(&self, id: u32, shell_type: TerminalShellType) {
        self.0.call_void(move |target| target.rpc_accept_terminal_shell_type(id, shell_type));
    }

    fn rpc_provide_terminal_completions(&self, id: &str, options: ITerminalCompletionContextDto, token: CancellationToken) -> Promise<Option<TerminalCompletionListDto>> {
        self.0.bound().rpc_provide_terminal_completions(id, options, token)
    }

    fn rpc_provide_terminal_quick_fixes(&self, id: &str, match_result: TerminalCommandMatchResultDto, token: CancellationToken) -> Promise<Option<OneOrMany<TerminalQuickFix>>> {
        self.0.bound().rpc_provide_terminal_quick_fixes(id, match_result, token)
    }

    fn rpc_create_contributed_profile_terminal(&self, id: &str, options: ICreateContributedTerminalProfileOptions) -> Promise<()> {
        self.0.bound().rpc_create_contributed_profile_terminal(id, options)
    }

    fn rpc_provide_links(&self, terminal_id: u32, line: &str) -> Promise<Vec<ITerminalLinkDto>> {
        self.0.bound().rpc_provide_links(terminal_id, line)
    }

    fn rpc_activate_link(&self, terminal_id: u32, link_id: u32) {
        self.0.call_void(move |target| target.rpc_activate_link(terminal_id, link_id));
    }

    fn rpc_init_environment_variable_collections(&self, collections: Vec<(String, ISerializableEnvironmentVariableCollection)>) {
        self.0.call_void(move |target| target.rpc_init_environment_variable_collections(collections));
    }

    fn rpc_accept_default_profile(&self, profile: ITerminalProfile, automation_profile: ITerminalProfile) {
        self.0.call_void(move |target| target.rpc_accept_default_profile(profile, automation_profile));
    }
}

/// O `ExtHostTreeViewsShape` que ainda não existe quando `MainThreadTreeViews` nasce.
#[derive(Default)]
pub struct LateExtHostTreeViews(pub Late<dyn ExtHostTreeViewsShape>);

impl IDisposable for LateExtHostTreeViews {
    fn dispose(&self) {}
}

impl ExtHostTreeViewsShape for LateExtHostTreeViews {
    fn rpc_get_children(&self, tree_view_id: &str, tree_item_handles: Option<Vec<String>>) -> Promise<Option<Vec<TreeChildrenBatch>>> {
        self.0.bound().rpc_get_children(tree_view_id, tree_item_handles)
    }

    #[allow(clippy::too_many_arguments)]
    fn rpc_handle_drop(
        &self,
        destination_view_id: &str,
        request_id: f64,
        tree_data_transfer_dto: DataTransferDto,
        target_item_handle: Option<String>,
        token: CancellationToken,
        operation_uuid: Option<String>,
        source_view_id: Option<String>,
        source_tree_item_handles: Option<Vec<String>>,
    ) -> Promise<()> {
        self.0.bound().rpc_handle_drop(
            destination_view_id,
            request_id,
            tree_data_transfer_dto,
            target_item_handle,
            token,
            operation_uuid,
            source_view_id,
            source_tree_item_handles,
        )
    }

    fn rpc_handle_drag(&self, source_view_id: &str, source_tree_item_handles: Vec<String>, operation_uuid: &str, token: CancellationToken) -> Promise<Option<DataTransferDto>> {
        self.0.bound().rpc_handle_drag(source_view_id, source_tree_item_handles, operation_uuid, token)
    }

    fn rpc_release_drag_operation(&self, operation_uuid: &str) {
        let operation_uuid = operation_uuid.to_owned();
        self.0.call_void(move |target| target.rpc_release_drag_operation(&operation_uuid));
    }

    fn rpc_has_resolve(&self, tree_view_id: &str) -> Promise<bool> {
        self.0.bound().rpc_has_resolve(tree_view_id)
    }

    fn rpc_resolve(&self, tree_view_id: &str, tree_item_handle: &str, token: CancellationToken) -> Promise<Option<ITreeItem>> {
        self.0.bound().rpc_resolve(tree_view_id, tree_item_handle, token)
    }

    fn rpc_set_expanded(&self, tree_view_id: &str, tree_item_handle: &str, expanded: bool) {
        let (tree_view_id, tree_item_handle) = (tree_view_id.to_owned(), tree_item_handle.to_owned());
        self.0.call_void(move |target| target.rpc_set_expanded(&tree_view_id, &tree_item_handle, expanded));
    }

    fn rpc_set_selection_and_focus(&self, tree_view_id: &str, selected_handles: Vec<String>, focused_handle: &str) {
        let (tree_view_id, focused_handle) = (tree_view_id.to_owned(), focused_handle.to_owned());
        self.0.call_void(move |target| target.rpc_set_selection_and_focus(&tree_view_id, selected_handles, &focused_handle));
    }

    fn rpc_set_visible(&self, tree_view_id: &str, is_visible: bool) {
        let tree_view_id = tree_view_id.to_owned();
        self.0.call_void(move |target| target.rpc_set_visible(&tree_view_id, is_visible));
    }

    fn rpc_change_checkbox_state(&self, tree_view_id: &str, checkbox_update: Vec<CheckboxUpdate>) {
        let tree_view_id = tree_view_id.to_owned();
        self.0.call_void(move |target| target.rpc_change_checkbox_state(&tree_view_id, checkbox_update));
    }
}

/// O `ExtHostSCMShape` que ainda não existe quando o cliente `MainThreadSCM` do workbench nasce. Todas as chamadas dele têm retorno (a
/// thread principal só as faz depois de montado o host), exceto o valor da caixa de entrada, que fica na fila se chegar antes do `bind`.
#[derive(Default)]
pub struct LateExtHostSCM(pub Late<dyn ExtHostSCMShape>);

impl ExtHostSCMShape for LateExtHostSCM {
    fn rpc_provide_original_resource(&self, source_control_handle: u32, uri: &URI, token: CancellationToken) -> Promise<Option<URI>> {
        self.0.bound().rpc_provide_original_resource(source_control_handle, uri, token)
    }

    fn rpc_provide_secondary_original_resource(&self, source_control_handle: u32, uri: &URI, token: CancellationToken) -> Promise<Option<URI>> {
        self.0.bound().rpc_provide_secondary_original_resource(source_control_handle, uri, token)
    }

    fn rpc_on_input_box_value_change(&self, source_control_handle: u32, value: &str) {
        let value = value.to_owned();
        self.0.call_void(move |target| target.rpc_on_input_box_value_change(source_control_handle, &value));
    }

    fn rpc_execute_resource_command(&self, source_control_handle: u32, group_handle: u32, handle: u32, preserve_focus: bool) -> Promise<()> {
        self.0.bound().rpc_execute_resource_command(source_control_handle, group_handle, handle, preserve_focus)
    }

    fn rpc_validate_input(&self, source_control_handle: u32, value: &str, cursor_position: f64) -> Promise<Option<(StringOrMarkdown, f64)>> {
        self.0.bound().rpc_validate_input(source_control_handle, value, cursor_position)
    }

    fn rpc_set_selected_source_control(&self, selected_source_control_handle: Option<u32>) -> Promise<()> {
        self.0.bound().rpc_set_selected_source_control(selected_source_control_handle)
    }

    fn rpc_provide_history_item_refs(&self, source_control_handle: u32, history_item_refs: Option<Vec<String>>, token: CancellationToken) -> Promise<Option<Vec<SCMHistoryItemRefDto>>> {
        self.0.bound().rpc_provide_history_item_refs(source_control_handle, history_item_refs, token)
    }

    fn rpc_provide_history_items(&self, source_control_handle: u32, options: ISCMHistoryOptions, token: CancellationToken) -> Promise<Option<Vec<SCMHistoryItemDto>>> {
        self.0.bound().rpc_provide_history_items(source_control_handle, options, token)
    }

    fn rpc_provide_history_item_changes(
        &self,
        source_control_handle: u32,
        history_item_id: &str,
        history_item_parent_id: Option<&str>,
        token: CancellationToken,
    ) -> Promise<Option<Vec<SCMHistoryItemChangeDto>>> {
        self.0.bound().rpc_provide_history_item_changes(source_control_handle, history_item_id, history_item_parent_id, token)
    }

    fn rpc_resolve_history_item(&self, source_control_handle: u32, history_item_id: &str, token: CancellationToken) -> Promise<Option<SCMHistoryItemDto>> {
        self.0.bound().rpc_resolve_history_item(source_control_handle, history_item_id, token)
    }

    fn rpc_resolve_history_item_chat_context(&self, source_control_handle: u32, history_item_id: &str, token: CancellationToken) -> Promise<Option<String>> {
        self.0.bound().rpc_resolve_history_item_chat_context(source_control_handle, history_item_id, token)
    }

    fn rpc_resolve_history_item_change_range_chat_context(
        &self,
        source_control_handle: u32,
        history_item_id: &str,
        history_item_parent_id: &str,
        path: &str,
        token: CancellationToken,
    ) -> Promise<Option<String>> {
        self.0.bound().rpc_resolve_history_item_change_range_chat_context(source_control_handle, history_item_id, history_item_parent_id, path, token)
    }

    fn rpc_resolve_history_item_refs_common_ancestor(&self, source_control_handle: u32, history_item_refs: Vec<String>, token: CancellationToken) -> Promise<Option<String>> {
        self.0.bound().rpc_resolve_history_item_refs_common_ancestor(source_control_handle, history_item_refs, token)
    }

    fn rpc_provide_artifact_groups(&self, source_control_handle: u32, token: CancellationToken) -> Promise<Option<Vec<SCMArtifactGroupDto>>> {
        self.0.bound().rpc_provide_artifact_groups(source_control_handle, token)
    }

    fn rpc_provide_artifacts(&self, source_control_handle: u32, group: &str, token: CancellationToken) -> Promise<Option<Vec<SCMArtifactDto>>> {
        self.0.bound().rpc_provide_artifacts(source_control_handle, group, token)
    }
}

/// O `ExtHostCommentsShape` que ainda não existe quando o cliente `MainThreadComments` do workbench nasce. `$deleteCommentThread` (a
/// única chamada sem retorno) fica na fila se chegar antes do `bind`.
#[derive(Default)]
pub struct LateExtHostComments(pub Late<dyn ExtHostCommentsShape>);

impl ExtHostCommentsShape for LateExtHostComments {
    fn rpc_create_comment_thread_template(&self, comment_controller_handle: u32, uri: &URI, range: Option<EditorRange>, editor_id: Option<&str>) -> Promise<()> {
        self.0.bound().rpc_create_comment_thread_template(comment_controller_handle, uri, range, editor_id)
    }

    fn rpc_update_comment_thread_template(&self, comment_controller_handle: u32, thread_handle: u32, range: EditorRange) -> Promise<()> {
        self.0.bound().rpc_update_comment_thread_template(comment_controller_handle, thread_handle, range)
    }

    fn rpc_update_comment_thread(&self, comment_controller_handle: u32, thread_handle: u32, changes: CommentThreadChanges) -> Promise<()> {
        self.0.bound().rpc_update_comment_thread(comment_controller_handle, thread_handle, changes)
    }

    fn rpc_delete_comment_thread(&self, comment_controller_handle: u32, comment_thread_handle: u32) {
        self.0.call_void(move |target| target.rpc_delete_comment_thread(comment_controller_handle, comment_thread_handle));
    }

    fn rpc_provide_commenting_ranges(&self, comment_controller_handle: u32, uri: &URI, token: CancellationToken) -> Promise<Option<CommentingRangesDto>> {
        self.0.bound().rpc_provide_commenting_ranges(comment_controller_handle, uri, token)
    }

    fn rpc_toggle_reaction(&self, comment_controller_handle: u32, thread_handle: u32, uri: &URI, comment: CommentChanges, reaction: CommentReaction) -> Promise<()> {
        self.0.bound().rpc_toggle_reaction(comment_controller_handle, thread_handle, uri, comment, reaction)
    }

    fn rpc_set_active_comment(&self, controller_handle: u32, comment_info: Option<ActiveCommentInfo>) -> Promise<()> {
        self.0.bound().rpc_set_active_comment(controller_handle, comment_info)
    }
}

/// O `ExtHostEditorTabsShape` que ainda não existe quando o cliente `MainThreadEditorTabs` do workbench nasce (ele empurra o modelo de
/// abas inicial ao subir). As três chamadas são sem retorno e ficam na fila até o `bind`.
#[derive(Default)]
pub struct LateExtHostEditorTabs(pub Late<dyn je_workbench::api::common::ext_host_protocol::ExtHostEditorTabsShape>);

impl je_workbench::api::common::ext_host_protocol::ExtHostEditorTabsShape for LateExtHostEditorTabs {
    fn rpc_accept_editor_tab_model(&self, tab_groups: Vec<je_workbench::api::common::ext_host_protocol::EditorTabGroupDto>) {
        self.0.call_void(move |target| target.rpc_accept_editor_tab_model(tab_groups));
    }

    fn rpc_accept_tab_group_update(&self, group_dto: je_workbench::api::common::ext_host_protocol::EditorTabGroupDto) {
        self.0.call_void(move |target| target.rpc_accept_tab_group_update(group_dto));
    }

    fn rpc_accept_tab_operation(&self, operation: je_workbench::api::common::ext_host_protocol::TabOperation) {
        self.0.call_void(move |target| target.rpc_accept_tab_operation(operation));
    }
}

/// O `ExtHostAuthenticationShape` que ainda não existe quando o cliente `MainThreadAuthentication` do workbench nasce. Todas as chamadas
/// têm retorno, então só acontecem depois de montado o host.
#[derive(Default)]
pub struct LateExtHostAuthentication(pub Late<dyn ExtHostAuthenticationShape>);

impl ExtHostAuthenticationShape for LateExtHostAuthentication {
    fn rpc_get_sessions(&self, id: &str, scopes: Option<Vec<String>>, options: IAuthenticationGetSessionsOptions) -> Promise<Vec<AuthenticationSession>> {
        self.0.bound().rpc_get_sessions(id, scopes, options)
    }

    fn rpc_create_session(&self, id: &str, scopes: Vec<String>, options: IAuthenticationCreateSessionOptions) -> Promise<AuthenticationSession> {
        self.0.bound().rpc_create_session(id, scopes, options)
    }

    fn rpc_get_sessions_from_challenges(&self, id: &str, constraint: IAuthenticationConstraint, options: IAuthenticationGetSessionsOptions) -> Promise<Vec<AuthenticationSession>> {
        self.0.bound().rpc_get_sessions_from_challenges(id, constraint, options)
    }

    fn rpc_create_session_from_challenges(&self, id: &str, constraint: IAuthenticationConstraint, options: IAuthenticationCreateSessionOptions) -> Promise<AuthenticationSession> {
        self.0.bound().rpc_create_session_from_challenges(id, constraint, options)
    }

    fn rpc_remove_session(&self, id: &str, session_id: &str) -> Promise<()> {
        self.0.bound().rpc_remove_session(id, session_id)
    }

    fn rpc_on_did_change_authentication_sessions(&self, id: &str, label: &str, extension_id_filter: Option<Vec<String>>) -> Promise<()> {
        self.0.bound().rpc_on_did_change_authentication_sessions(id, label, extension_id_filter)
    }

    fn rpc_on_did_unregister_authentication_provider(&self, id: &str) -> Promise<()> {
        self.0.bound().rpc_on_did_unregister_authentication_provider(id)
    }
}

/// O `ExtHostNotebookShape` que ainda não existe quando o cliente `MainThreadNotebookDocumentsAndEditors` do workbench nasce (o construtor
/// dele já entrega o delta inicial de documentos e editores). As chamadas sem retorno (o delta e a liberação dos itens da barra de status)
/// ficam na fila se chegarem antes do `bind`; as com retorno só acontecem depois de montado o host.
#[derive(Default)]
pub struct LateExtHostNotebook(pub Late<dyn ExtHostNotebookShape>);

impl ExtHostNotebookDocumentsAndEditorsShape for LateExtHostNotebook {
    fn rpc_accept_document_and_editors_delta(&self, delta: SerializableObjectWithBuffers<INotebookDocumentsAndEditorsDelta>) {
        self.0.call_void(move |target| target.rpc_accept_document_and_editors_delta(delta));
    }
}

impl ExtHostNotebookShape for LateExtHostNotebook {
    fn rpc_provide_notebook_cell_status_bar_items(&self, handle: u32, uri: &URI, index: usize, token: CancellationToken) -> Promise<Option<INotebookCellStatusBarListDto>> {
        self.0.bound().rpc_provide_notebook_cell_status_bar_items(handle, uri, index, token)
    }

    fn rpc_release_notebook_cell_status_bar_items(&self, id: u32) {
        self.0.call_void(move |target| target.rpc_release_notebook_cell_status_bar_items(id));
    }

    fn rpc_data_to_notebook(&self, handle: u32, data: VSBuffer, token: CancellationToken) -> Promise<SerializableObjectWithBuffers<NotebookDataDto>> {
        self.0.bound().rpc_data_to_notebook(handle, data, token)
    }

    fn rpc_notebook_to_data(&self, handle: u32, data: SerializableObjectWithBuffers<NotebookDataDto>, token: CancellationToken) -> Promise<VSBuffer> {
        self.0.bound().rpc_notebook_to_data(handle, data, token)
    }

    fn rpc_save_notebook(&self, handle: u32, uri: &URI, version_id: i64, options: WriteFileOptions, token: CancellationToken) -> Promise<NotebookSaveResult> {
        self.0.bound().rpc_save_notebook(handle, uri, version_id, options, token)
    }
}

/// O `ExtHostNotebookDocumentsShape` que ainda não existe quando o cliente `MainThreadNotebookDocuments` do workbench nasce. As três chamadas
/// são sem retorno e ficam na fila se chegarem antes do `bind`.
#[derive(Default)]
pub struct LateExtHostNotebookDocuments(pub Late<dyn ExtHostNotebookDocumentsShape>);

impl ExtHostNotebookDocumentsShape for LateExtHostNotebookDocuments {
    fn rpc_accept_model_changed(&self, uri: &URI, event: SerializableObjectWithBuffers<NotebookCellsChangedEventDto>, is_dirty: bool, new_metadata: Option<je_base::dynamic::Dynamic>) {
        let uri = uri.clone();
        self.0.call_void(move |target| target.rpc_accept_model_changed(&uri, event, is_dirty, new_metadata));
    }

    fn rpc_accept_dirty_state_changed(&self, uri: &URI, is_dirty: bool) {
        let uri = uri.clone();
        self.0.call_void(move |target| target.rpc_accept_dirty_state_changed(&uri, is_dirty));
    }

    fn rpc_accept_model_saved(&self, uri: &URI) {
        let uri = uri.clone();
        self.0.call_void(move |target| target.rpc_accept_model_saved(&uri));
    }
}

/// O `ExtHostNotebookDocumentSaveParticipantShape` que ainda não existe quando o cliente `MainThreadNotebookDocuments` do workbench nasce. O
/// pedido de participação na gravação só chega com o host já ligado.
#[derive(Default)]
pub struct LateExtHostNotebookDocumentSaveParticipant(pub Late<dyn ExtHostNotebookDocumentSaveParticipantShape>);

impl ExtHostNotebookDocumentSaveParticipantShape for LateExtHostNotebookDocumentSaveParticipant {
    fn rpc_participate_in_save(&self, resource: &URI, reason: je_workbench::common::editor::SaveReason, token: CancellationToken) -> Promise<bool> {
        self.0.bound().rpc_participate_in_save(resource, reason, token)
    }
}

/// O `ExtHostNotebookKernelsShape` que ainda não existe quando o cliente `MainThreadNotebookKernels` do workbench nasce. A associação de
/// notebook e a mensagem do renderizador são sem retorno e ficam na fila se chegarem antes do `bind`.
#[derive(Default)]
pub struct LateExtHostNotebookKernels(pub Late<dyn ExtHostNotebookKernelsShape>);

impl ExtHostNotebookKernelsShape for LateExtHostNotebookKernels {
    fn rpc_accept_notebook_association(&self, handle: u32, uri: &URI, value: bool) {
        let uri = uri.clone();
        self.0.call_void(move |target| target.rpc_accept_notebook_association(handle, &uri, value));
    }

    fn rpc_execute_cells(&self, handle: u32, uri: &URI, handles: Vec<i64>) -> Promise<()> {
        self.0.bound().rpc_execute_cells(handle, uri, handles)
    }

    fn rpc_cancel_cells(&self, handle: u32, uri: &URI, handles: Vec<i64>) -> Promise<()> {
        self.0.bound().rpc_cancel_cells(handle, uri, handles)
    }

    fn rpc_accept_kernel_message_from_renderer(&self, handle: u32, editor_id: &str, message: je_base::dynamic::Dynamic) {
        let editor_id = editor_id.to_owned();
        self.0.call_void(move |target| target.rpc_accept_kernel_message_from_renderer(handle, &editor_id, message));
    }

    fn rpc_provide_kernel_source_actions(&self, handle: u32, token: CancellationToken) -> Promise<Vec<INotebookKernelSourceAction>> {
        self.0.bound().rpc_provide_kernel_source_actions(handle, token)
    }

    fn rpc_provide_variables(
        &self,
        handle: u32,
        request_id: &str,
        notebook_uri: &URI,
        parent_id: Option<u32>,
        kind: VariablesRequestKindDto,
        start: usize,
        token: CancellationToken,
    ) -> Promise<()> {
        self.0.bound().rpc_provide_variables(handle, request_id, notebook_uri, parent_id, kind, start, token)
    }
}

/// O `ExtHostDebugServiceShape` que ainda não existe quando o cliente `MainThreadDebugService` do workbench nasce. As chamadas sem
/// retorno (`$sendDAMessage`, `$acceptBreakpointsDelta`, `$disposeDebugVisualizers`, `$disposeVisualizedTree`) ficam na fila se chegarem
/// antes do `bind`; as com retorno só acontecem depois de montado o host.
#[derive(Default)]
pub struct LateExtHostDebugService(pub Late<dyn ExtHostDebugServiceShape>);

impl ExtHostDebugServiceShape for LateExtHostDebugService {
    fn rpc_substitute_variables(&self, folder: Option<URI>, config: IConfig) -> Promise<IConfig> {
        self.0.bound().rpc_substitute_variables(folder, config)
    }

    fn rpc_run_in_terminal(&self, args: serde_json::Value, session_id: &str) -> Promise<Option<i64>> {
        self.0.bound().rpc_run_in_terminal(args, session_id)
    }

    fn rpc_start_da_session(&self, handle: u32, session: IDebugSessionDto) -> Promise<()> {
        self.0.bound().rpc_start_da_session(handle, session)
    }

    fn rpc_stop_da_session(&self, handle: u32) -> Promise<()> {
        self.0.bound().rpc_stop_da_session(handle)
    }

    fn rpc_send_da_message(&self, handle: u32, message: ProtocolMessage) {
        self.0.call_void(move |target| target.rpc_send_da_message(handle, message));
    }

    fn rpc_resolve_debug_configuration(&self, handle: u32, folder: Option<URI>, debug_configuration: IConfig, token: CancellationToken) -> Promise<Option<IConfig>> {
        self.0.bound().rpc_resolve_debug_configuration(handle, folder, debug_configuration, token)
    }

    fn rpc_resolve_debug_configuration_with_substituted_variables(
        &self,
        handle: u32,
        folder: Option<URI>,
        debug_configuration: IConfig,
        token: CancellationToken,
    ) -> Promise<Option<IConfig>> {
        self.0.bound().rpc_resolve_debug_configuration_with_substituted_variables(handle, folder, debug_configuration, token)
    }

    fn rpc_provide_debug_configurations(&self, handle: u32, folder: Option<URI>, token: CancellationToken) -> Promise<Vec<IConfig>> {
        self.0.bound().rpc_provide_debug_configurations(handle, folder, token)
    }

    fn rpc_provide_debug_adapter(&self, handle: u32, session: IDebugSessionDto) -> Promise<IAdapterDescriptor> {
        self.0.bound().rpc_provide_debug_adapter(handle, session)
    }

    fn rpc_accept_debug_session_started(&self, session: IDebugSessionDto) -> Promise<()> {
        self.0.bound().rpc_accept_debug_session_started(session)
    }

    fn rpc_accept_debug_session_terminated(&self, session: IDebugSessionDto) -> Promise<()> {
        self.0.bound().rpc_accept_debug_session_terminated(session)
    }

    fn rpc_accept_debug_session_active_changed(&self, session: Option<IDebugSessionDto>) -> Promise<()> {
        self.0.bound().rpc_accept_debug_session_active_changed(session)
    }

    fn rpc_accept_debug_session_custom_event(&self, session: IDebugSessionDto, event: serde_json::Value) -> Promise<()> {
        self.0.bound().rpc_accept_debug_session_custom_event(session, event)
    }

    fn rpc_accept_breakpoints_delta(&self, delta: IBreakpointsDeltaDto) {
        self.0.call_void(move |target| target.rpc_accept_breakpoints_delta(delta));
    }

    fn rpc_accept_debug_session_name_changed(&self, session: IDebugSessionDto, name: &str) -> Promise<()> {
        self.0.bound().rpc_accept_debug_session_name_changed(session, name)
    }

    fn rpc_accept_stack_frame_focus(&self, focus: Option<StackFocusDto>) -> Promise<()> {
        self.0.bound().rpc_accept_stack_frame_focus(focus)
    }

    fn rpc_provide_debug_visualizers(&self, extension_id: &str, id: &str, context: IDebugVisualizationContext, token: CancellationToken) -> Promise<Vec<IDebugVisualization>> {
        self.0.bound().rpc_provide_debug_visualizers(extension_id, id, context, token)
    }

    fn rpc_resolve_debug_visualizer(&self, id: u64, token: CancellationToken) -> Promise<MainThreadDebugVisualization> {
        self.0.bound().rpc_resolve_debug_visualizer(id, token)
    }

    fn rpc_execute_debug_visualizer_command(&self, id: u64) -> Promise<()> {
        self.0.bound().rpc_execute_debug_visualizer_command(id)
    }

    fn rpc_dispose_debug_visualizers(&self, ids: Vec<u64>) {
        self.0.call_void(move |target| target.rpc_dispose_debug_visualizers(ids));
    }

    fn rpc_get_visualizer_tree_item(&self, tree_id: &str, element: IDebugVisualizationContext) -> Promise<Option<IDebugVisualizationTreeItem>> {
        self.0.bound().rpc_get_visualizer_tree_item(tree_id, element)
    }

    fn rpc_get_visualizer_tree_item_children(&self, tree_id: &str, element: u64) -> Promise<Vec<IDebugVisualizationTreeItem>> {
        self.0.bound().rpc_get_visualizer_tree_item_children(tree_id, element)
    }

    fn rpc_edit_visualizer_tree_item(&self, element: u64, value: &str) -> Promise<Option<IDebugVisualizationTreeItem>> {
        self.0.bound().rpc_edit_visualizer_tree_item(element, value)
    }

    fn rpc_dispose_visualized_tree(&self, element: u64) {
        self.0.call_void(move |target| target.rpc_dispose_visualized_tree(element));
    }
}

/// O `ExtHostStorageShape` que ainda não existe quando o `MainThreadStorage` do workbench nasce.
#[derive(Default)]
pub struct LateExtHostStorage(pub Late<dyn ExtHostStorageShape>);

impl ExtHostStorageShape for LateExtHostStorage {
    fn rpc_accept_value(&self, shared: bool, extension_id: &str, value: &str) {
        let (extension_id, value) = (extension_id.to_owned(), value.to_owned());
        self.0.call_void(move |target| target.rpc_accept_value(shared, &extension_id, &value));
    }
}

/// O `ExtHostSecretStateShape` que ainda não existe quando o `MainThreadSecretState` do workbench nasce.
#[derive(Default)]
pub struct LateExtHostSecretState(pub Late<dyn ExtHostSecretStateShape>);

impl ExtHostSecretStateShape for LateExtHostSecretState {
    fn rpc_on_did_change_password(&self, extension_id: &str, key: &str) -> Promise<()> {
        self.0.bound().rpc_on_did_change_password(extension_id, key)
    }
}

/// Os encaminhadores `Late*` registrados no contexto em processo, para o host ligar os serviços reais depois.
pub struct LateForwarders {
    pub storage: Rc<LateExtHostStorage>,
    pub secret_state: Rc<LateExtHostSecretState>,
    pub editor_tabs: Rc<LateExtHostEditorTabs>,
    pub scm: Rc<LateExtHostSCM>,
    pub comments: Rc<LateExtHostComments>,
    pub authentication: Rc<LateExtHostAuthentication>,
    pub notebook: Rc<LateExtHostNotebook>,
    pub notebook_documents: Rc<LateExtHostNotebookDocuments>,
    pub notebook_save_participant: Rc<LateExtHostNotebookDocumentSaveParticipant>,
    pub notebook_kernels: Rc<LateExtHostNotebookKernels>,
    pub debug_service: Rc<LateExtHostDebugService>,
    pub terminal_service: Rc<LateExtHostTerminalService>,
    pub tree_views: Rc<LateExtHostTreeViews>,
    pub workspace: Rc<LateExtHostWorkspace>,
    pub configuration: Rc<LateExtHostConfiguration>,
    pub status_bar: Rc<LateExtHostStatusBar>,
    pub quick_open: Rc<LateExtHostQuickOpen>,
    pub output: Rc<LateExtHostOutputService>,
    pub progress: Rc<LateExtHostProgress>,
    pub file_system_events: Rc<LateExtHostFileSystemEventService>,
    pub file_system: Rc<LateExtHostFileSystem>,
    pub documents_and_editors: Rc<LateExtHostDocumentsAndEditors>,
    pub documents: Rc<LateExtHostDocuments>,
    pub editors: Rc<LateExtHostEditors>,
    pub diagnostics: Rc<LateExtHostDiagnostics>,
    pub languages: Rc<LateExtHostLanguages>,
    pub language_features: Rc<LateExtHostLanguageFeatures>,
}

impl LateForwarders {
    /// Cria os encaminhadores e os registra em `local` (onde os clientes do workbench os encontram com `get_proxy`).
    pub fn register(local: &LocalContext) -> LateForwarders {
        let storage = Rc::new(LateExtHostStorage::default());
        let as_storage: Rc<dyn ExtHostStorageShape> = storage.clone();
        local.set_erased(ExtHostContext::EXT_HOST_STORAGE.sid, Rc::new(as_storage));
        let secret_state = Rc::new(LateExtHostSecretState::default());
        let as_secret_state: Rc<dyn ExtHostSecretStateShape> = secret_state.clone();
        local.set_erased(ExtHostContext::EXT_HOST_SECRET_STATE.sid, Rc::new(as_secret_state));
        let workspace = Rc::new(LateExtHostWorkspace::default());
        let configuration = Rc::new(LateExtHostConfiguration::default());
        let status_bar = Rc::new(LateExtHostStatusBar::default());
        let quick_open = Rc::new(LateExtHostQuickOpen::default());
        let output = Rc::new(LateExtHostOutputService::default());
        let progress = Rc::new(LateExtHostProgress::default());
        let file_system_events = Rc::new(LateExtHostFileSystemEventService::default());
        let file_system = Rc::new(LateExtHostFileSystem::default());
        let as_file_system: Rc<dyn ExtHostFileSystemShape> = file_system.clone();
        local.set_erased(ExtHostContext::EXT_HOST_FILE_SYSTEM.sid, Rc::new(as_file_system));
        let documents_and_editors = Rc::new(LateExtHostDocumentsAndEditors::default());
        let documents = Rc::new(LateExtHostDocuments::default());
        let editors = Rc::new(LateExtHostEditors::default());
        let diagnostics = Rc::new(LateExtHostDiagnostics::default());
        let languages = Rc::new(LateExtHostLanguages::default());
        let language_features = Rc::new(LateExtHostLanguageFeatures::default());
        let terminal_service = Rc::new(LateExtHostTerminalService::default());
        let tree_views = Rc::new(LateExtHostTreeViews::default());
        let debug_service = Rc::new(LateExtHostDebugService::default());
        let as_debug_service: Rc<dyn ExtHostDebugServiceShape> = debug_service.clone();
        local.set_erased(ExtHostContext::EXT_HOST_DEBUG_SERVICE.sid, Rc::new(as_debug_service));
        let scm = Rc::new(LateExtHostSCM::default());
        let as_scm: Rc<dyn ExtHostSCMShape> = scm.clone();
        local.set_erased(ExtHostContext::EXT_HOST_SCM.sid, Rc::new(as_scm));
        let comments = Rc::new(LateExtHostComments::default());
        let as_comments: Rc<dyn ExtHostCommentsShape> = comments.clone();
        local.set_erased(ExtHostContext::EXT_HOST_COMMENTS.sid, Rc::new(as_comments));
        let editor_tabs = Rc::new(LateExtHostEditorTabs::default());
        let as_editor_tabs: Rc<dyn je_workbench::api::common::ext_host_protocol::ExtHostEditorTabsShape> = editor_tabs.clone();
        local.set_erased(ExtHostContext::EXT_HOST_EDITOR_TABS.sid, Rc::new(as_editor_tabs));
        let authentication = Rc::new(LateExtHostAuthentication::default());
        let as_authentication: Rc<dyn ExtHostAuthenticationShape> = authentication.clone();
        local.set_erased(ExtHostContext::EXT_HOST_AUTHENTICATION.sid, Rc::new(as_authentication));
        let notebook = Rc::new(LateExtHostNotebook::default());
        let as_notebook: Rc<dyn ExtHostNotebookShape> = notebook.clone();
        local.set_erased(ExtHostContext::EXT_HOST_NOTEBOOK.sid, Rc::new(as_notebook));
        let notebook_documents = Rc::new(LateExtHostNotebookDocuments::default());
        let as_notebook_documents: Rc<dyn ExtHostNotebookDocumentsShape> = notebook_documents.clone();
        local.set_erased(ExtHostContext::EXT_HOST_NOTEBOOK_DOCUMENTS.sid, Rc::new(as_notebook_documents));
        let notebook_save_participant = Rc::new(LateExtHostNotebookDocumentSaveParticipant::default());
        let as_notebook_save_participant: Rc<dyn ExtHostNotebookDocumentSaveParticipantShape> = notebook_save_participant.clone();
        local.set_erased(ExtHostContext::EXT_HOST_NOTEBOOK_DOCUMENT_SAVE_PARTICIPANT.sid, Rc::new(as_notebook_save_participant));
        let notebook_kernels = Rc::new(LateExtHostNotebookKernels::default());
        let as_notebook_kernels: Rc<dyn ExtHostNotebookKernelsShape> = notebook_kernels.clone();
        local.set_erased(ExtHostContext::EXT_HOST_NOTEBOOK_KERNELS.sid, Rc::new(as_notebook_kernels));
        let as_terminal_service: Rc<dyn ExtHostTerminalServiceShape> = terminal_service.clone();
        let as_tree_views: Rc<dyn ExtHostTreeViewsShape> = tree_views.clone();
        local.set_erased(ExtHostContext::EXT_HOST_TERMINAL_SERVICE.sid, Rc::new(as_terminal_service));
        local.set_erased(ExtHostContext::EXT_HOST_TREE_VIEWS.sid, Rc::new(as_tree_views));
        let as_workspace: Rc<dyn ExtHostWorkspaceShape> = workspace.clone();
        let as_configuration: Rc<dyn ExtHostConfigurationShape> = configuration.clone();
        let as_status_bar: Rc<dyn ExtHostStatusBarShape> = status_bar.clone();
        let as_quick_open: Rc<dyn ExtHostQuickOpenShape> = quick_open.clone();
        let as_output: Rc<dyn ExtHostOutputServiceShape> = output.clone();
        let as_progress: Rc<dyn ExtHostProgressShape> = progress.clone();
        let as_file_system_events: Rc<dyn ExtHostFileSystemEventServiceShape> = file_system_events.clone();
        let as_documents_and_editors: Rc<dyn ExtHostDocumentsAndEditorsShape> = documents_and_editors.clone();
        let as_documents: Rc<dyn ExtHostDocumentsShape> = documents.clone();
        let as_editors: Rc<dyn ExtHostEditorsShape> = editors.clone();
        let as_diagnostics: Rc<dyn ExtHostDiagnosticsShape> = diagnostics.clone();
        let as_languages: Rc<dyn ExtHostLanguagesShape> = languages.clone();
        let as_language_features: Rc<dyn ExtHostLanguageFeaturesShape> = language_features.clone();
        local.set_erased(ExtHostContext::EXT_HOST_WORKSPACE.sid, Rc::new(as_workspace));
        local.set_erased(ExtHostContext::EXT_HOST_CONFIGURATION.sid, Rc::new(as_configuration));
        local.set_erased(ExtHostContext::EXT_HOST_STATUS_BAR.sid, Rc::new(as_status_bar));
        local.set_erased(ExtHostContext::EXT_HOST_QUICK_OPEN.sid, Rc::new(as_quick_open));
        local.set_erased(ExtHostContext::EXT_HOST_OUTPUT_SERVICE.sid, Rc::new(as_output));
        local.set_erased(ExtHostContext::EXT_HOST_PROGRESS.sid, Rc::new(as_progress));
        local.set_erased(ExtHostContext::EXT_HOST_FILE_SYSTEM_EVENT_SERVICE.sid, Rc::new(as_file_system_events));
        local.set_erased(ExtHostContext::EXT_HOST_DOCUMENTS_AND_EDITORS.sid, Rc::new(as_documents_and_editors));
        local.set_erased(ExtHostContext::EXT_HOST_DOCUMENTS.sid, Rc::new(as_documents));
        local.set_erased(ExtHostContext::EXT_HOST_EDITORS.sid, Rc::new(as_editors));
        local.set_erased(ExtHostContext::EXT_HOST_DIAGNOSTICS.sid, Rc::new(as_diagnostics));
        local.set_erased(ExtHostContext::EXT_HOST_LANGUAGES.sid, Rc::new(as_languages));
        local.set_erased(ExtHostContext::EXT_HOST_LANGUAGE_FEATURES.sid, Rc::new(as_language_features));
        LateForwarders { storage, secret_state, editor_tabs, scm, comments, authentication, notebook, notebook_documents, notebook_save_participant, notebook_kernels, debug_service, workspace, configuration, status_bar, quick_open, output, progress, file_system_events, file_system, documents_and_editors, documents, editors, diagnostics, languages, language_features, terminal_service, tree_views }
    }
}

/// O contexto que os serviços do host enxergam: o que o workbench registrou em processo ([`LocalContext`]) vem primeiro, o resto
/// (comandos, extensões, telemetria) é o RPC.
pub struct BridgedContext<'a> {
    rpc: &'a dyn IMainContext,
    local: Rc<LocalContext>,
}

impl<'a> BridgedContext<'a> {
    pub fn new(rpc: &'a dyn IMainContext, local: Rc<LocalContext>) -> Self {
        Self { rpc, local }
    }
}

impl IMainContext for BridgedContext<'_> {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        if self.local.contains(sid) { self.local.get_proxy_erased(sid) } else { self.rpc.get_proxy_erased(sid) }
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>) {
        if self.local.contains(sid) { self.local.set_erased(sid, value) } else { self.rpc.set_erased(sid, value) }
    }
}
