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
    CheckboxUpdate, ExtHostTerminalServiceShape, ExtHostTreeViewsShape, ICreateContributedTerminalProfileOptions, ITerminalCommandDto, ITerminalCompletionContextDto,
    ITerminalDimensionsDto, ITerminalLinkDto, IShellLaunchConfigDto, TerminalCommandMatchResultDto, TerminalCompletionListDto, TerminalQuickFix, TreeChildrenBatch,
};
use je_workbench::common::views::ITreeItem;
use je_workbench::api::common::ext_host_protocol::{
    DocumentsAndEditorsDelta, EditorPropertiesChangeData, ExtHostConfigurationShape, ExtHostContext, ExtHostDocumentsAndEditorsShape, ExtHostDocumentsShape,
    ExtHostEditorsShape, ExtHostOutputServiceShape, ExtHostProgressShape, ExtHostQuickOpenShape, ExtHostStatusBarShape, ExtHostWorkspaceShape, IConfigurationInitData,
    IMainContext, IRawFileMatch2, ITextEditorDiffInformation, IWorkspaceData, SerializedModelContentChangedEvent, StatusBarItemDto, TextEditorPositionData,
};

use crate::api::env::HostEnvironment;
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

    /// O `IClipboardService` do workbench, que `vscode.env.clipboard` usa. Sem ele o membro não existe.
    fn clipboard_service(&self) -> Option<Rc<dyn IClipboardService>> {
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

/// Os encaminhadores `Late*` registrados no contexto em processo, para o host ligar os serviços reais depois.
pub struct LateForwarders {
    pub terminal_service: Rc<LateExtHostTerminalService>,
    pub tree_views: Rc<LateExtHostTreeViews>,
    pub workspace: Rc<LateExtHostWorkspace>,
    pub configuration: Rc<LateExtHostConfiguration>,
    pub status_bar: Rc<LateExtHostStatusBar>,
    pub quick_open: Rc<LateExtHostQuickOpen>,
    pub output: Rc<LateExtHostOutputService>,
    pub progress: Rc<LateExtHostProgress>,
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
        let workspace = Rc::new(LateExtHostWorkspace::default());
        let configuration = Rc::new(LateExtHostConfiguration::default());
        let status_bar = Rc::new(LateExtHostStatusBar::default());
        let quick_open = Rc::new(LateExtHostQuickOpen::default());
        let output = Rc::new(LateExtHostOutputService::default());
        let progress = Rc::new(LateExtHostProgress::default());
        let documents_and_editors = Rc::new(LateExtHostDocumentsAndEditors::default());
        let documents = Rc::new(LateExtHostDocuments::default());
        let editors = Rc::new(LateExtHostEditors::default());
        let diagnostics = Rc::new(LateExtHostDiagnostics::default());
        let languages = Rc::new(LateExtHostLanguages::default());
        let language_features = Rc::new(LateExtHostLanguageFeatures::default());
        let terminal_service = Rc::new(LateExtHostTerminalService::default());
        let tree_views = Rc::new(LateExtHostTreeViews::default());
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
        local.set_erased(ExtHostContext::EXT_HOST_DOCUMENTS_AND_EDITORS.sid, Rc::new(as_documents_and_editors));
        local.set_erased(ExtHostContext::EXT_HOST_DOCUMENTS.sid, Rc::new(as_documents));
        local.set_erased(ExtHostContext::EXT_HOST_EDITORS.sid, Rc::new(as_editors));
        local.set_erased(ExtHostContext::EXT_HOST_DIAGNOSTICS.sid, Rc::new(as_diagnostics));
        local.set_erased(ExtHostContext::EXT_HOST_LANGUAGES.sid, Rc::new(as_languages));
        local.set_erased(ExtHostContext::EXT_HOST_LANGUAGE_FEATURES.sid, Rc::new(as_language_features));
        LateForwarders { workspace, configuration, status_bar, quick_open, output, progress, documents_and_editors, documents, editors, diagnostics, languages, language_features, terminal_service, tree_views }
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
