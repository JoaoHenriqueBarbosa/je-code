//! Porte de `src/vs/workbench/api/test/browser/mainThreadCustomEditors.test.ts` (suíte `MainThreadCustomEditors`), caso a caso e na mesma ordem.
//!
//! Fora deste arquivo (e por quê): os 2 casos que montam um `WebviewEditor` (o painel de editor de webview), `custom editor pane
//! accepts missing options when switching back to an open editor` e `custom editor pane exposes live navigation selections for
//! editor type switches`. O `WebviewEditor` e o `workbenchInstantiationService` (um workbench de testes completo, com o
//! `EditorPane`, o `IHostService`, o `IWorkbenchLayoutService` e o serviço de chaves de contexto) não foram portados, e o que
//! esses casos medem (`setOptions(undefined)` no painel e o `getSelection().restore(...)` dele) é do painel; a parte lógica que
//! o painel lê da entrada (`getSelection` com a seleção viva da navegação) é a [`TextEditorPaneSelection`] que o
//! `CustomEditorInput` devolve.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso (`store` é o `guard.add_rc`); os
//!   casos `async` rodam em `run_with_faked_timers`, e o `timeout(0)` é o `common::timeout(0)`. O `setup`/`teardown` que troca o
//!   tratador de erros inesperados é o [`capture_unexpected_errors`] (por teste, num thread só).
//! - **`TestInstantiationService`.** O porte injeta os serviços por struct (sem `@IFoo`), então o `instantiationService.stub(...)`
//!   é a montagem explícita das structs de serviços: [`MainThreadCustomEditorsServices`] (e a [`CustomEditorModelServices`] dele),
//!   [`CustomEditorInputServices`] e [`CustomEditorDiffInputServices`]. Os `stub(IFoo, { ... })` são os dublês abaixo, cada um com
//!   só o que o original sobrescreve (o resto falha com `panic!`, como o método ausente do objeto lançaria `TypeError`). Os
//!   serviços reais do original que o teste usa são os reais aqui também: `CustomEditorModelManager`, `WorkingCopyService`,
//!   `InMemoryStorageService`, `TestFileService`, `MockLabelService`, `TestEditorGroupsService`, `TestEditorService`,
//!   `TestEnvironmentService` e `TestPathService`.
//! - O `IThemeService` não foi portado: o [`TestThemeService`] implementa o [`IWebviewThemeService`] mínimo da entrada. O
//!   `IWebviewService` (que o teste não stuba) é um dublê que falha ao ser chamado: só `copy()` e `create` o usam.
//! - `mock<MainThreadWebviewPanels>()` é o [`MockMainThreadWebviewPanels`] ([`IMainThreadWebviewPanels`]) e `mock<MainThreadWebviews>()`
//!   é o [`MockMainThreadWebviews`] ([`IMainThreadWebviews`]). `SingleProxyRPCProtocol(proxy)` é o [`MapMainContext`] com o dublê do
//!   host de extensões registrado no identificador `ExtHostCustomEditors`.
//! - `createTextModel('text', undefined, undefined, resource)` é o `create_text_model_with`, e o `IResolvedTextEditorModel` do
//!   original (`textEditorModel` e `isReadonly() => false`) é o [`ResolvedTextEditorModel`].
//! - `assert.rejects(promise, /mensagem/)` é a conferência da mensagem do erro da promessa; `assert.deepStrictEqual` de objetos
//!   é o `assert_eq!` das tuplas e dos vetores.
//! - A seleção `{ startLineNumber: 1, startColumn: 1 }` de `applyOptions` é o [`TextEditorSelection`] (com `endLineNumber` e
//!   `endColumn` ausentes), e `$onDidChangeCustomTextEditorSelection(handle, selection)` leva o [`Selection`] (que o
//!   `ISelection` do original descreve); a conferência `input.navigation?.selection === selection` é a igualdade de valor.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;
#[path = "support/webview_support.rs"]
mod webview_support;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable, none, to_disposable};
use je_base::path::Platform;
use je_base::promise::{MaybePromise, Promise, Resolver};
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::dialogs::{ConfirmResult, FileNameOrResource, IFileDialogService, OpenDialogOptions, PickAndOpenOptions, SaveDialogOptions};
use je_platform::opener::editor_types::TextEditorSelection;
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_editor::undo_redo::{
    IUndoRedoService, PastFutureElements, ResourceEditStackSnapshot, UndoRedoElementRef, UndoRedoGroup, UndoRedoSource, UndoRedoTarget, UriComparisonKeyComputer,
};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_theme::ThemeType;
use je_workbench::api::browser::main_thread_custom_editors::{CustomEditorModelServices, MainThreadCustomEditors, MainThreadCustomEditorsServices};
use je_workbench::api::browser::main_thread_webview_panels::IMainThreadWebviewPanels;
use je_workbench::api::browser::main_thread_webviews::{IMainThreadWebviews, MainThreadWebviewOptions};
use je_workbench::api::common::ext_host_protocol::{
    CreateCustomDocumentResult, CustomEditorDiffInitData, CustomEditorProviderCapabilities, CustomEditorResolveInitData, CustomEditorSideBySideDiffInitData,
    CustomEditorSideBySideDiffWebviewHandles, CustomTextEditorNavigationResult, EditorGroupColumn, ExtHostContext, ExtHostCustomEditorsShape, IMainContext,
    MainThreadCustomEditorsShape, WebviewExtensionDescription as WebviewExtensionDescriptionDto, WebviewPanelOptions,
};
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::{IEditorPane, TextEditorOptions};
use je_workbench::contrib::custom_editor::custom_editor::{
    CustomEditorCapabilities, CustomEditorInfo, CustomEditorInfoCollection, ICustomEditorModelManager, ICustomEditorService,
};
use je_workbench::contrib::custom_editor::custom_editor_diff_input::{CustomEditorDiffInput, CustomEditorDiffInputInitInfo, CustomEditorDiffInputServices};
use je_workbench::contrib::custom_editor::custom_editor_input::{CustomEditorInput, CustomEditorInputInitInfo, CustomEditorInputOptions, CustomEditorInputServices};
use je_workbench::contrib::custom_editor::custom_editor_model_manager::CustomEditorModelManager;
use je_workbench::contrib::webview::browser_webview::{
    IOverlayWebview, IWebview, IWebviewService, WebviewContentOptions, WebviewExtensionDescription, WebviewInitInfo, WebviewMessageReceivedEvent, WebviewOptions,
};
use je_workbench::contrib::webview_panel::webview_editor_input::{IWebviewThemeService, WebviewIconPath, WebviewInput};
use je_workbench::contrib::webview_panel::webview_workbench_service::{
    IWebViewShowOptions, IWebviewWorkbenchService, OpenRevivedWebviewOptions, WebviewResolver,
};
use je_workbench::services::editor::custom_editor_label_service::ICustomEditorLabelService;
use je_workbench::services::editor::editor_groups_service::GroupRef;
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService};
use je_workbench::services::outline::outline::{IOutline, IOutlineCreator, IOutlineService, OutlineTarget};
use je_workbench::services::textfile::collaborators::{IProgress, IWorkingCopyFileService, ProgressStep, WorkingCopyFileEvent};
use je_workbench::services::textfile::textfiles::{
    EncodedContent, IResourceEncodings, ISaveErrorHandler, ITextFileEditorModel, ITextFileEditorModelManager, ITextFileSaveParticipant, ITextFileService,
    ReadTextFileEncodingOptions, ReadTextFileOptions, ResolvedDecoding, ResolvedEncoding, TextFileContent, TextFileCreateOperation, TextFileEditorModelResolveOrCreateOptions,
    TextFileResolveEvent, TextFileSaveAsOptions, TextFileSaveEvent, TextFileSaveOptions, TextFileSaveParticipantContext, TextFileStreamContent, TextOrSnapshot,
    WriteTextFileOptions,
};
use je_workbench::services::textfile::collaborators::FileOperationUndoRedoInfo;
use je_workbench::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference};
use je_workbench::services::untitled::untitled_text_editor_model::IUntitledTextEditorModel;
use je_workbench::services::untitled::untitled_text_editor_service::{
    IUntitledTextEditorModelManager, IUntitledTextEditorService, UntitledTextEditorModelSaveEvent, UntitledTextEditorOptions,
};
use je_workbench::services::working_copy::working_copy_service::WorkingCopyService;
use je_workbench::test::browser::workbench_test_services::{MockLabelService, TestEditorGroupsService, TestEditorService, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::{NullFilesConfigurationService, TestFileService};
use je_base::buffer::VSBufferReadableStream;
use je_base::stream::ReadableStream;
use je_editor::text_model::TextModel;
use je_platform::files::files::FileStatWithMetadata;
use test_text_model::create_text_model_with;
use webview_support::MapMainContext;

const VIEW_TYPE: &str = "test.customEditor";

fn resource() -> URI {
    URI::file("/workspace/file.custom", Platform::Posix)
}

/// O `new Error(message)` de uma chamada que rejeita.
fn rejected<T: Clone + 'static>(message: &str) -> Promise<T> {
    Promise::rejected(Error::new(message))
}

// ---- erros inesperados (`setup` e `teardown`)

/// `setup`: `unexpectedErrors = []` e `setUnexpectedErrorHandler(error => unexpectedErrors.push(error.message))`.
fn capture_unexpected_errors() -> Rc<RefCell<Vec<String>>> {
    let unexpected_errors: Rc<RefCell<Vec<String>>> = Rc::default();
    let captured = unexpected_errors.clone();
    errors::set_unexpected_error_handler(move |error| captured.borrow_mut().push(error.message.clone()));
    unexpected_errors
}

// ---- o host de extensões

/// O argumento de `createCustomDocument` do original: `{ path, attempt, token }`.
struct CreateRequest {
    path: String,
    attempt: u32,
    token: CancellationToken,
}

/// O que o caso configura do host de extensões (o argumento de `createCustomEditors`).
struct ExtHost {
    create_custom_document: Box<dyn Fn(CreateRequest) -> Promise<CreateCustomDocumentResult>>,
    resolve_custom_editor: Option<Box<dyn Fn() -> Promise<()>>>,
    capabilities: CustomEditorProviderCapabilities,
    text_editor: bool,
    resolve_navigation: Option<Box<dyn Fn(&str, CancellationToken) -> Promise<Option<CustomTextEditorNavigationResult>>>>,
}

impl ExtHost {
    fn new(create_custom_document: impl Fn(CreateRequest) -> Promise<CreateCustomDocumentResult> + 'static) -> Self {
        Self {
            create_custom_document: Box::new(create_custom_document),
            resolve_custom_editor: None,
            capabilities: CustomEditorProviderCapabilities::default(),
            text_editor: false,
            resolve_navigation: None,
        }
    }

    /// `createCustomDocument: async () => ({ editable: false })`.
    fn not_editable() -> Self {
        Self::new(|_| Promise::resolved(CreateCustomDocumentResult { editable: false }))
    }

    fn text(mut self) -> Self {
        self.text_editor = true;
        self
    }

    fn navigation(mut self) -> Self {
        self.capabilities.supports_navigation = Some(true);
        self
    }
}

/// O `mock<ExtHostCustomEditorsShape>()` de `createCustomEditors`: os seis métodos que o original sobrescreve respondem, e o resto
/// falha.
struct ExtHostProxy {
    ext_host: ExtHost,
    calls: Rc<RefCell<Vec<String>>>,
    handles: Rc<RefCell<Vec<String>>>,
    create_attempts: RefCell<HashMap<String, u32>>,
}

impl ExtHostProxy {
    /// `handleName(handle)`.
    fn handle_name(&self, handle: &str) -> String {
        let index = self.handles.borrow().iter().position(|candidate| candidate == handle).map_or(-1, |index| index as i64);
        format!("webview#{}", index + 1)
    }

    fn path(resource: &URI) -> String {
        resource.path().to_string_lossy()
    }
}

impl ExtHostCustomEditorsShape for ExtHostProxy {
    fn rpc_resolve_custom_text_editor_navigation(&self, handle: &str, _view_type: &str, _resource: &URI, token: CancellationToken) -> Promise<Option<CustomTextEditorNavigationResult>> {
        self.calls.borrow_mut().push(format!("resolveNavigation({})", self.handle_name(handle)));
        match &self.ext_host.resolve_navigation {
            Some(resolve_navigation) => resolve_navigation(handle, token),
            None => Promise::resolved(Some(CustomTextEditorNavigationResult { selection: None })),
        }
    }

    fn rpc_dispose_custom_text_editor_navigation(&self, handle: &str) {
        self.calls.borrow_mut().push(format!("disposeNavigation({})", self.handle_name(handle)));
    }

    fn rpc_reveal_custom_text_editor_range(&self, _handle: &str, _range: Range, _selection: Option<Selection>, _preserve_focus: bool, _token: CancellationToken) -> Promise<()> {
        panic!("$revealCustomTextEditorRange is not a function");
    }

    fn rpc_capture_custom_text_editor_view_state(&self, _handle: &str, _state_id: u32) -> Promise<()> {
        panic!("$captureCustomTextEditorViewState is not a function");
    }

    fn rpc_restore_custom_text_editor_view_state(&self, _handle: &str, _state_id: u32, _token: CancellationToken) -> Promise<()> {
        panic!("$restoreCustomTextEditorViewState is not a function");
    }

    fn rpc_release_custom_text_editor_view_state(&self, _handle: &str, _state_id: u32) {
        panic!("$releaseCustomTextEditorViewState is not a function");
    }

    fn rpc_resolve_custom_editor(
        &self,
        resource: &URI,
        new_webview_handle: &str,
        _view_type: &str,
        _init_data: CustomEditorResolveInitData,
        _position: EditorGroupColumn,
        _cancellation: CancellationToken,
    ) -> Promise<()> {
        self.calls.borrow_mut().push(format!("$resolveCustomEditor({}, {})", Self::path(resource), self.handle_name(new_webview_handle)));
        match &self.ext_host.resolve_custom_editor {
            Some(resolve_custom_editor) => resolve_custom_editor(),
            None => Promise::resolved(()),
        }
    }

    fn rpc_resolve_custom_editor_inline_diff(
        &self,
        original_resource: &URI,
        modified_resource: &URI,
        new_webview_handle: &str,
        _view_type: &str,
        _init_data: CustomEditorDiffInitData,
        _position: EditorGroupColumn,
        _cancellation: CancellationToken,
    ) -> Promise<()> {
        self.calls.borrow_mut().push(format!(
            "$resolveCustomEditorInlineDiff({}, {}, {})",
            Self::path(original_resource),
            Self::path(modified_resource),
            self.handle_name(new_webview_handle)
        ));
        Promise::resolved(())
    }

    fn rpc_resolve_custom_editor_side_by_side_diff(
        &self,
        _original_resource: &URI,
        _modified_resource: &URI,
        _webview_handles: CustomEditorSideBySideDiffWebviewHandles,
        _view_type: &str,
        _init_data: CustomEditorSideBySideDiffInitData,
        _position: EditorGroupColumn,
        _cancellation: CancellationToken,
    ) -> Promise<()> {
        panic!("$resolveCustomEditorSideBySideDiff is not a function");
    }

    fn rpc_create_custom_document(
        &self,
        resource: &URI,
        _view_type: &str,
        _backup_id: Option<String>,
        _untitled_document_data: Option<VSBuffer>,
        cancellation: CancellationToken,
    ) -> Promise<CreateCustomDocumentResult> {
        let path = Self::path(resource);
        let attempt = {
            let mut attempts = self.create_attempts.borrow_mut();
            let attempt = attempts.get(&path).copied().unwrap_or(0) + 1;
            attempts.insert(path.clone(), attempt);
            attempt
        };
        self.calls.borrow_mut().push(format!("$createCustomDocument({path})"));
        (self.ext_host.create_custom_document)(CreateRequest { path, attempt, token: cancellation })
    }

    fn rpc_dispose_custom_document(&self, resource: &URI, _view_type: &str) -> Promise<()> {
        self.calls.borrow_mut().push(format!("$disposeCustomDocument({})", Self::path(resource)));
        Promise::resolved(())
    }

    fn rpc_undo(&self, _resource: &URI, _view_type: &str, _edit_id: u32, _is_dirty: bool) -> Promise<()> {
        panic!("$undo is not a function");
    }

    fn rpc_redo(&self, _resource: &URI, _view_type: &str, _edit_id: u32, _is_dirty: bool) -> Promise<()> {
        panic!("$redo is not a function");
    }

    fn rpc_revert(&self, _resource: &URI, _view_type: &str, _cancellation: CancellationToken) -> Promise<()> {
        panic!("$revert is not a function");
    }

    fn rpc_dispose_edits(&self, _resource: &URI, _view_type: &str, _edit_ids: Vec<u32>) -> Result<(), Error> {
        panic!("$disposeEdits is not a function");
    }

    fn rpc_on_save(&self, _resource: &URI, _view_type: &str, _cancellation: CancellationToken) -> Promise<()> {
        panic!("$onSave is not a function");
    }

    fn rpc_on_save_as(&self, _resource: &URI, _view_type: &str, _target_resource: &URI, _cancellation: CancellationToken) -> Promise<()> {
        panic!("$onSaveAs is not a function");
    }

    fn rpc_backup(&self, _resource: &URI, _view_type: &str, _cancellation: CancellationToken) -> Promise<String> {
        panic!("$backup is not a function");
    }

    fn rpc_on_move_custom_editor(&self, _handle: &str, _new_resource: &URI, _view_type: &str) -> Promise<()> {
        panic!("$onMoveCustomEditor is not a function");
    }
}

// ---- os dublês de MainThread

/// `new class extends mock<MainThreadWebviewPanels>() { webviewInputs; addWebviewInput }`.
struct MockMainThreadWebviewPanels {
    calls: Rc<RefCell<Vec<String>>>,
    handles: Rc<RefCell<Vec<String>>>,
    webview_inputs: RefCell<Vec<(String, Rc<WebviewInput>)>>,
}

impl IMainThreadWebviewPanels for MockMainThreadWebviewPanels {
    fn webview_inputs(&self) -> Vec<Rc<WebviewInput>> {
        self.webview_inputs.borrow().iter().map(|(_, input)| input.clone()).collect()
    }

    fn add_webview_input(&self, handle: &str, input: Rc<WebviewInput>, _options: MainThreadWebviewOptions) {
        self.handles.borrow_mut().push(handle.to_owned());
        {
            let mut inputs = self.webview_inputs.borrow_mut();
            inputs.retain(|(existing, _)| existing != handle);
            inputs.push((handle.to_owned(), input));
        }
        let name = {
            let handles = self.handles.borrow();
            format!("webview#{}", handles.iter().position(|candidate| candidate == handle).map_or(0, |index| index + 1))
        };
        self.calls.borrow_mut().push(format!("addWebviewInput({name})"));
    }
}

/// `new class extends mock<MainThreadWebviews>() { getWebviewResolvedFailedContent(viewType) }`.
struct MockMainThreadWebviews;

impl IMainThreadWebviews for MockMainThreadWebviews {
    fn add_webview(&self, _handle: &str, _webview: Rc<dyn IWebview>, _options: MainThreadWebviewOptions) -> Result<(), Error> {
        panic!("addWebview is not a function");
    }

    fn get_webview_resolved_failed_content(&self, view_type: &str) -> String {
        format!("failed to load {view_type}")
    }
}

// ---- o webview

/// `createWebview()`: `mock<IOverlayWebview>()` com `html`, `origin`, `options`, `contentOptions`, `extension` e `onDidDispose`.
struct OverlayWebview {
    html: RefCell<Option<String>>,
    origin: RefCell<String>,
    options: RefCell<WebviewOptions>,
    content_options: RefCell<WebviewContentOptions>,
    extension: RefCell<Option<WebviewExtensionDescription>>,
    on_did_dispose: Rc<je_base::event::Emitter<()>>,
}

impl OverlayWebview {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            html: RefCell::new(None),
            origin: RefCell::new(String::new()),
            options: RefCell::new(WebviewOptions::default()),
            content_options: RefCell::new(WebviewContentOptions::default()),
            extension: RefCell::new(None),
            on_did_dispose: je_base::event::Emitter::new(),
        })
    }
}

impl IDisposable for OverlayWebview {
    /// `dispose() { onDidDispose.fire(); onDidDispose.dispose(); }`.
    fn dispose(&self) {
        self.on_did_dispose.fire(());
        self.on_did_dispose.dispose();
    }
}

impl IWebview for OverlayWebview {
    fn provided_view_type(&self) -> Option<String> {
        panic!("providedViewType is not defined");
    }

    fn origin(&self) -> String {
        self.origin.borrow().clone()
    }

    fn set_origin(&self, origin: &str) {
        *self.origin.borrow_mut() = origin.to_owned();
    }

    fn set_html(&self, html: &str) {
        *self.html.borrow_mut() = Some(html.to_owned());
    }

    fn set_title(&self, _title: &str) {
        panic!("setTitle is not a function");
    }

    fn content_options(&self) -> WebviewContentOptions {
        self.content_options.borrow().clone()
    }

    fn set_content_options(&self, options: WebviewContentOptions) {
        *self.content_options.borrow_mut() = options;
    }

    fn extension(&self) -> Option<WebviewExtensionDescription> {
        self.extension.borrow().clone()
    }

    fn set_extension(&self, extension: Option<WebviewExtensionDescription>) {
        *self.extension.borrow_mut() = extension;
    }

    fn state(&self) -> Option<String> {
        None
    }

    fn set_state(&self, _state: Option<String>) {
        panic!("state is not writable");
    }

    fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose.event()
    }

    fn on_did_click_link(&self) -> Event<String> {
        panic!("onDidClickLink is not defined");
    }

    fn on_missing_csp(&self) -> Event<ExtensionIdentifier> {
        panic!("onMissingCsp is not defined");
    }

    fn on_message(&self) -> Event<WebviewMessageReceivedEvent> {
        panic!("onMessage is not defined");
    }

    fn post_message(&self, _message: Dynamic, _transfer: Option<Vec<Vec<u8>>>) -> Promise<bool> {
        panic!("postMessage is not a function");
    }
}

impl IOverlayWebview for OverlayWebview {
    fn options(&self) -> WebviewOptions {
        self.options.borrow().clone()
    }

    fn set_options(&self, options: WebviewOptions) {
        *self.options.borrow_mut() = options;
    }
}

/// `new TestThemeService()`.
struct TestThemeService;

impl IWebviewThemeService for TestThemeService {
    fn on_did_color_theme_change(&self) -> Event<()> {
        Event::none()
    }

    fn color_theme_type(&self) -> ThemeType {
        ThemeType::Dark
    }
}

/// O `IWebviewService` que o teste não stuba.
struct UnstubbedWebviewService;

impl IWebviewService for UnstubbedWebviewService {
    fn create_webview_overlay(&self, _init_info: WebviewInitInfo) -> Rc<dyn IOverlayWebview> {
        panic!("Unable to resolve IWebviewService");
    }
}

// ---- os serviços do workbench

/// `new class extends mock<IWebviewWorkbenchService>() { registerResolver; resolveWebview }`.
struct WebviewWorkbenchService {
    resolvers: Rc<RefCell<Vec<Rc<dyn WebviewResolver>>>>,
}

impl IWebviewWorkbenchService for WebviewWorkbenchService {
    fn on_did_change_active_webview_editor(&self) -> Event<Option<Rc<WebviewInput>>> {
        panic!("onDidChangeActiveWebviewEditor is not defined");
    }

    fn open_webview(&self, _webview_init_info: WebviewInitInfo, _view_type: &str, _title: &str, _icon_path: Option<WebviewIconPath>, _show_options: IWebViewShowOptions) -> Rc<WebviewInput> {
        panic!("openWebview is not a function");
    }

    fn open_revived_webview(&self, _options: OpenRevivedWebviewOptions) -> Rc<WebviewInput> {
        panic!("openRevivedWebview is not a function");
    }

    fn reveal_webview(&self, _webview: &Rc<WebviewInput>, _group: GroupRef, _preserve_focus: bool) {
        panic!("revealWebview is not a function");
    }

    fn register_resolver(&self, resolver: Rc<dyn WebviewResolver>) -> DisposableRef {
        self.resolvers.borrow_mut().push(resolver);
        none()
    }

    fn should_persist(&self, _input: &Rc<WebviewInput>) -> bool {
        panic!("shouldPersist is not a function");
    }

    fn resolve_webview(&self, webview: Rc<WebviewInput>, token: CancellationToken) -> Promise<()> {
        let resolver = {
            let resolvers = self.resolvers.borrow();
            resolvers.iter().find(|resolver| resolver.can_resolve(&webview)).cloned()
        };
        match resolver {
            Some(resolver) => resolver.resolve_webview(webview, token),
            None => Promise::resolved(()),
        }
    }
}

/// `new class extends mock<ICustomEditorService>() { models; registerCustomEditorCapabilities; getCustomEditorCapabilities }`.
struct CustomEditorService {
    models: Rc<CustomEditorModelManager>,
}

impl ICustomEditorService for CustomEditorService {
    fn models(&self) -> Rc<dyn ICustomEditorModelManager> {
        self.models.clone()
    }

    fn get_custom_editor(&self, _view_type: &str) -> Option<CustomEditorInfo> {
        panic!("getCustomEditor is not a function");
    }

    fn get_all_custom_editors(&self, _resource: &URI) -> CustomEditorInfoCollection {
        panic!("getAllCustomEditors is not a function");
    }

    fn get_contributed_custom_editors(&self, _resource: &URI) -> CustomEditorInfoCollection {
        panic!("getContributedCustomEditors is not a function");
    }

    fn get_user_configured_custom_editors(&self, _resource: &URI) -> CustomEditorInfoCollection {
        panic!("getUserConfiguredCustomEditors is not a function");
    }

    fn register_custom_editor_capabilities(&self, _view_type: &str, _options: CustomEditorCapabilities) -> DisposableRef {
        none()
    }

    fn get_custom_editor_capabilities(&self, _view_type: &str) -> Option<CustomEditorCapabilities> {
        None
    }
}

/// `{ activateByEvent: async () => { }, onWillStop: Event.None }`.
struct ExtensionService;

impl IExtensionService for ExtensionService {
    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        Promise::resolved(())
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        panic!("activationEventIsDone is not a function");
    }

    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        panic!("getExtension is not a function");
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        panic!("onDidRegisterExtensions is not defined");
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        panic!("whenInstalledExtensionsRegistered is not a function");
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        panic!("extensions is not defined");
    }
}

/// `{ registerWorkingCopyProvider: () => Disposable.None, onWillRunWorkingCopyFileOperation: Event.None }`.
struct WorkingCopyFileService;

impl IWorkingCopyFileService for WorkingCopyFileService {
    fn on_will_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        Event::none()
    }

    fn on_did_fail_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        panic!("onDidFailWorkingCopyFileOperation is not defined");
    }

    fn on_did_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        panic!("onDidRunWorkingCopyFileOperation is not defined");
    }
}

/// `{ asCanonicalUri: (uri: URI) => uri }`.
struct UriIdentityService;

impl IUriIdentityService for UriIdentityService {
    fn ext_uri(&self) -> ExtUri {
        panic!("extUri is not defined");
    }

    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

/// `{ get: () => undefined }`.
struct UntitledTextEditorService;

impl IUntitledTextEditorService for UntitledTextEditorService {
    fn on_did_save(&self) -> Event<UntitledTextEditorModelSaveEvent> {
        panic!("onDidSave is not defined");
    }

    fn on_did_change_dirty(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        panic!("onDidChangeDirty is not defined");
    }

    fn on_did_change_encoding(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        panic!("onDidChangeEncoding is not defined");
    }

    fn on_did_change_label(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        panic!("onDidChangeLabel is not defined");
    }

    fn on_did_create(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        panic!("onDidCreate is not defined");
    }

    fn on_will_dispose(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        panic!("onWillDispose is not defined");
    }

    fn create(&self, _options: Option<UntitledTextEditorOptions>) -> Rc<dyn IUntitledTextEditorModel> {
        panic!("create is not a function");
    }

    fn get(&self, _resource: &URI) -> Option<Rc<dyn IUntitledTextEditorModel>> {
        None
    }

    fn get_value(&self, _resource: &URI) -> Option<String> {
        panic!("getValue is not a function");
    }

    fn resolve(&self, _options: Option<UntitledTextEditorOptions>) -> Promise<Rc<dyn IUntitledTextEditorModel>> {
        panic!("resolve is not a function");
    }

    fn is_untitled_with_associated_resource(&self, _resource: &URI) -> bool {
        panic!("isUntitledWithAssociatedResource is not a function");
    }

    fn can_dispose(&self, _model: &Rc<dyn IUntitledTextEditorModel>) -> Option<Promise<()>> {
        panic!("canDispose is not a function");
    }
}

/// `{ getName: () => undefined, onDidChange: Event.None }`.
struct CustomEditorLabelService;

impl ICustomEditorLabelService for CustomEditorLabelService {
    fn on_did_change(&self) -> Event<()> {
        Event::none()
    }

    fn get_name(&self, _resource: &URI) -> Option<String> {
        None
    }
}

/// `{}` para `IFileDialogService`.
struct FileDialogService;

impl IFileDialogService for FileDialogService {
    fn default_file_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        panic!("defaultFilePath is not a function")
    }

    fn default_folder_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        panic!("defaultFolderPath is not a function")
    }

    fn default_workspace_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        panic!("defaultWorkspacePath is not a function")
    }

    fn pick_file_folder_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        panic!("pickFileFolderAndOpen is not a function")
    }

    fn pick_file_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        panic!("pickFileAndOpen is not a function")
    }

    fn pick_folder_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        panic!("pickFolderAndOpen is not a function")
    }

    fn pick_workspace_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        panic!("pickWorkspaceAndOpen is not a function")
    }

    fn pick_file_to_save(&self, _default_uri: &URI, _available_file_systems: Option<Vec<String>>) -> Promise<Option<URI>> {
        panic!("pickFileToSave is not a function")
    }

    fn preferred_home(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        panic!("preferredHome is not a function")
    }

    fn show_save_dialog(&self, _options: SaveDialogOptions) -> Promise<Option<URI>> {
        panic!("showSaveDialog is not a function")
    }

    fn show_save_confirm(&self, _file_names_or_resources: &[FileNameOrResource]) -> Promise<ConfirmResult> {
        panic!("showSaveConfirm is not a function")
    }

    fn show_open_dialog(&self, _options: OpenDialogOptions) -> Promise<Option<Vec<URI>>> {
        panic!("showOpenDialog is not a function")
    }
}

/// `{}` para `IUndoRedoService`.
struct UndoRedoService;

impl IUndoRedoService for UndoRedoService {
    fn register_uri_comparison_key_computer(&self, _scheme: &str, _computer: Rc<dyn UriComparisonKeyComputer>) -> DisposableRef {
        panic!("registerUriComparisonKeyComputer is not a function");
    }

    fn get_uri_comparison_key(&self, _resource: &URI) -> String {
        panic!("getUriComparisonKey is not a function");
    }

    fn push_element(&self, _element: UndoRedoElementRef, _group: Option<&UndoRedoGroup>, _source: Option<&UndoRedoSource>) {
        panic!("pushElement is not a function");
    }

    fn get_last_element(&self, _resource: &URI) -> Option<UndoRedoElementRef> {
        panic!("getLastElement is not a function");
    }

    fn get_elements(&self, _resource: &URI) -> PastFutureElements {
        panic!("getElements is not a function");
    }

    fn set_elements_valid_flag(&self, _resource: &URI, _is_valid: bool, _filter: &dyn Fn(&UndoRedoElementRef) -> bool) {
        panic!("setElementsValidFlag is not a function");
    }

    fn remove_elements(&self, _resource: &URI) {
        panic!("removeElements is not a function");
    }

    fn create_snapshot(&self, _resource: &URI) -> ResourceEditStackSnapshot {
        panic!("createSnapshot is not a function");
    }

    fn restore_snapshot(&self, _snapshot: &ResourceEditStackSnapshot) {
        panic!("restoreSnapshot is not a function");
    }

    fn can_undo(&self, _target: UndoRedoTarget<'_>) -> bool {
        panic!("canUndo is not a function");
    }

    fn undo(&self, _target: UndoRedoTarget<'_>) -> MaybePromise<()> {
        panic!("undo is not a function");
    }

    fn can_redo(&self, _target: UndoRedoTarget<'_>) -> bool {
        panic!("canRedo is not a function");
    }

    fn redo(&self, _target: UndoRedoTarget<'_>) -> MaybePromise<()> {
        panic!("redo is not a function");
    }
}

/// `{ notifyOutlineChanged: () => { outlineChanges++; } }`.
struct OutlineService {
    outline_changes: Rc<Cell<usize>>,
}

impl IOutlineService for OutlineService {
    fn on_did_change(&self) -> Event<()> {
        panic!("onDidChange is not defined");
    }

    fn can_create_outline(&self, _editor: &dyn IEditorPane) -> bool {
        panic!("canCreateOutline is not a function");
    }

    fn create_outline(&self, _editor: Rc<dyn IEditorPane>, _target: OutlineTarget, _token: CancellationToken) -> Promise<Option<Rc<dyn IOutline>>> {
        panic!("createOutline is not a function");
    }

    fn register_outline_creator(&self, _creator: Rc<dyn IOutlineCreator>) -> DisposableRef {
        panic!("registerOutlineCreator is not a function");
    }

    fn notify_outline_changed(&self) {
        self.outline_changes.set(self.outline_changes.get() + 1);
    }
}

/// O `IResolvedTextEditorModel` do original: `textEditorModel` e `isReadonly() => false`.
struct ResolvedTextEditorModel {
    text_model: Rc<TextModel>,
}

impl IDisposable for ResolvedTextEditorModel {
    fn dispose(&self) {}
}

impl IEditorModel for ResolvedTextEditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        panic!("onWillDispose is not defined");
    }

    fn resolve(&self) -> Promise<()> {
        panic!("resolve is not a function");
    }

    fn is_resolved(&self) -> bool {
        panic!("isResolved is not a function");
    }

    fn is_disposed(&self) -> bool {
        panic!("isDisposed is not a function");
    }
}

impl ITextEditorModel for ResolvedTextEditorModel {
    fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        Some(self.text_model.clone())
    }

    fn is_readonly(&self) -> je_workbench::services::files_configuration::files_configuration_service::Readonly {
        je_workbench::services::files_configuration::files_configuration_service::Readonly::Bool(false)
    }

    fn create_snapshot(&self) -> Option<Box<dyn je_editor::model::TextSnapshot>> {
        panic!("createSnapshot is not a function");
    }
}

/// `{ createModelReference: async () => ({ object: resolved, dispose: () => { } }) }`.
struct TextModelService {
    resolved: Rc<ResolvedTextEditorModel>,
}

impl ITextModelService for TextModelService {
    fn create_model_reference(&self, _resource: &URI) -> Promise<TextModelReference> {
        let resolved: Rc<dyn ITextEditorModel> = self.resolved.clone();
        Promise::resolved(TextModelReference::new(resolved, to_disposable(|| {})))
    }
}

/// `files: new class extends mock<ITextFileEditorModelManager>() { get() { return undefined; } onDidChangeDirty = Event.None }`.
struct TextFileEditorModelManager;

impl ITextFileEditorModelManager for TextFileEditorModelManager {
    fn on_did_create(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        panic!("onDidCreate is not defined");
    }

    fn on_did_resolve(&self) -> Event<TextFileResolveEvent> {
        panic!("onDidResolve is not defined");
    }

    fn on_did_change_dirty(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        Event::none()
    }

    fn on_did_change_readonly(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        panic!("onDidChangeReadonly is not defined");
    }

    fn on_did_remove(&self) -> Event<URI> {
        panic!("onDidRemove is not defined");
    }

    fn on_did_change_orphaned(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        panic!("onDidChangeOrphaned is not defined");
    }

    fn on_did_change_encoding(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        panic!("onDidChangeEncoding is not defined");
    }

    fn on_did_save_error(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        panic!("onDidSaveError is not defined");
    }

    fn on_did_save(&self) -> Event<TextFileSaveEvent> {
        panic!("onDidSave is not defined");
    }

    fn on_did_revert(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        panic!("onDidRevert is not defined");
    }

    fn models(&self) -> Vec<Rc<dyn ITextFileEditorModel>> {
        panic!("models is not defined");
    }

    fn set_save_error_handler(&self, _handler: Rc<dyn ISaveErrorHandler>) {
        panic!("setSaveErrorHandler is not a function");
    }

    fn save_error_handler(&self) -> Rc<dyn ISaveErrorHandler> {
        panic!("saveErrorHandler is not defined");
    }

    fn get(&self, _resource: &URI) -> Option<Rc<dyn ITextFileEditorModel>> {
        None
    }

    fn resolve(&self, _resource: &URI, _options: Option<TextFileEditorModelResolveOrCreateOptions>) -> Promise<Rc<dyn ITextFileEditorModel>> {
        panic!("resolve is not a function");
    }

    fn add_save_participant(&self, _participant: Rc<dyn ITextFileSaveParticipant>) -> DisposableRef {
        panic!("addSaveParticipant is not a function");
    }

    fn run_save_participants(
        &self,
        _model: Rc<dyn ITextFileEditorModel>,
        _context: TextFileSaveParticipantContext,
        _progress: Rc<dyn IProgress<ProgressStep>>,
        _token: CancellationToken,
    ) -> Promise<()> {
        panic!("runSaveParticipants is not a function");
    }

    fn can_dispose(&self, _model: Rc<dyn ITextFileEditorModel>) -> Option<Promise<()>> {
        panic!("canDispose is not a function");
    }
}

/// `{ files: ..., isDirty: () => false }`.
struct TextFileService;

impl ITextFileService for TextFileService {
    fn files(&self) -> Rc<dyn ITextFileEditorModelManager> {
        Rc::new(TextFileEditorModelManager)
    }

    fn untitled(&self) -> Rc<dyn IUntitledTextEditorModelManager> {
        panic!("untitled is not defined");
    }

    fn encoding(&self) -> Rc<dyn IResourceEncodings> {
        panic!("encoding is not defined");
    }

    fn is_dirty(&self, _resource: &URI) -> bool {
        false
    }

    fn save(&self, _resource: &URI, _options: Option<TextFileSaveOptions>) -> Promise<Option<URI>> {
        panic!("save is not a function");
    }

    fn save_as(&self, _resource: &URI, _target_resource: Option<&URI>, _options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        panic!("saveAs is not a function");
    }

    fn revert(&self, _resource: &URI, _options: Option<je_workbench::common::editor::RevertOptions>) -> Promise<()> {
        panic!("revert is not a function");
    }

    fn read(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileContent> {
        panic!("read is not a function");
    }

    fn read_stream(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileStreamContent> {
        panic!("readStream is not a function");
    }

    fn write(&self, _resource: &URI, _value: TextOrSnapshot, _options: Option<WriteTextFileOptions>) -> Promise<FileStatWithMetadata> {
        panic!("write is not a function");
    }

    fn create(&self, _operations: Vec<TextFileCreateOperation>, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        panic!("create is not a function");
    }

    fn get_encoded_readable(&self, _resource: Option<&URI>, _value: Option<TextOrSnapshot>, _options: Option<WriteTextFileOptions>) -> Promise<Option<EncodedContent>> {
        panic!("getEncodedReadable is not a function");
    }

    fn get_decoded_stream(&self, _resource: Option<&URI>, _value: VSBufferReadableStream, _options: Option<ReadTextFileEncodingOptions>) -> Promise<Rc<dyn ReadableStream<String>>> {
        panic!("getDecodedStream is not a function");
    }

    fn get_encoding(&self, _resource: &URI) -> String {
        panic!("getEncoding is not a function");
    }

    fn resolve_decoding(&self, _resource: Option<&URI>, _options: Option<ReadTextFileEncodingOptions>) -> Promise<ResolvedDecoding> {
        panic!("resolveDecoding is not a function");
    }

    fn resolve_encoding(&self, _resource: Option<&URI>, _options: Option<WriteTextFileOptions>) -> Promise<ResolvedEncoding> {
        panic!("resolveEncoding is not a function");
    }

    fn validate_detected_encoding(&self, _resource: Option<&URI>, _detected_encoding: &str, _options: Option<ReadTextFileEncodingOptions>) -> Promise<String> {
        panic!("validateDetectedEncoding is not a function");
    }
}

// ---- `createCustomEditors`

/// O que `createCustomEditors(extHost)` devolve (menos o que o caso não usa).
struct Fixture {
    calls: Rc<RefCell<Vec<String>>>,
    models: Rc<CustomEditorModelManager>,
    resolvers: Rc<RefCell<Vec<Rc<dyn WebviewResolver>>>>,
    custom_editors: Rc<MainThreadCustomEditors>,
    outline_changes: Rc<Cell<usize>>,
    input_services: Rc<CustomEditorInputServices>,
    diff_services: Rc<CustomEditorDiffInputServices>,
    store: Rc<je_base::lifecycle::DisposableStore>,
}

/// Um painel criado por `createInput` ou `createDiffInput`: a entrada e o webview.
struct Created {
    input: Rc<WebviewInput>,
    webview: Rc<OverlayWebview>,
}

fn extension() -> WebviewExtensionDescriptionDto {
    WebviewExtensionDescriptionDto { id: ExtensionIdentifier::new("test.extension"), location: URI::file("/extensions/test", Platform::Posix) }
}

/// Registers a custom editor provider backed by a fake extension host and returns the calls that reach it.
fn create_custom_editors(store: &LeakGuard, ext_host: ExtHost) -> Fixture {
    let calls: Rc<RefCell<Vec<String>>> = Rc::default();
    let handles: Rc<RefCell<Vec<String>>> = Rc::default();

    let text_editor = ext_host.text_editor;
    let capabilities = ext_host.capabilities;
    let proxy = Rc::new(ExtHostProxy { ext_host, calls: calls.clone(), handles: handles.clone(), create_attempts: RefCell::new(HashMap::new()) });

    let main_thread_web_view_panels = Rc::new(MockMainThreadWebviewPanels { calls: calls.clone(), handles, webview_inputs: RefCell::new(Vec::new()) });

    let resolvers: Rc<RefCell<Vec<Rc<dyn WebviewResolver>>>> = Rc::default();
    let webview_workbench_service: Rc<dyn IWebviewWorkbenchService> = Rc::new(WebviewWorkbenchService { resolvers: resolvers.clone() });

    let models = Rc::new(CustomEditorModelManager::new());
    let custom_editor_service: Rc<dyn ICustomEditorService> = Rc::new(CustomEditorService { models: models.clone() });

    let storage_service = store.add_rc(InMemoryStorageService::new());
    let storage: Rc<dyn StorageService> = Rc::new(storage_service.clone());
    let working_copy_service = store.add_rc(WorkingCopyService::new());
    let file_service = store.add_rc(TestFileService::new());
    let label_service = MockLabelService::new(None);
    let outline_changes = Rc::new(Cell::new(0));
    let resource = resource();
    let text_model = text_editor.then(|| store.add_rc(create_text_model_with("text", None, RelaxedTextModelCreationOptions::default(), Some(resource.clone()))));
    let text_model_service: Rc<dyn ITextModelService> = match text_model {
        Some(text_model) => Rc::new(TextModelService { resolved: Rc::new(ResolvedTextEditorModel { text_model }) }),
        None => Rc::new(UnstubbedTextModelService),
    };

    let model_services = CustomEditorModelServices {
        file_dialog_service: Rc::new(FileDialogService),
        file_service: file_service.clone(),
        label_service: label_service.clone(),
        undo_redo_service: Rc::new(UndoRedoService),
        environment_service: TestEnvironmentService::new(),
        path_service: TestPathService::new(None, None),
        text_model_service,
        text_file_service: Rc::new(TextFileService),
    };

    let context: Rc<dyn IMainContext> = Rc::new(MapMainContext::default());
    let proxy_shape: Rc<dyn ExtHostCustomEditorsShape> = proxy;
    context.set(&ExtHostContext::EXT_HOST_CUSTOM_EDITORS, proxy_shape);

    let custom_editors = store.add_rc(MainThreadCustomEditors::new(
        &*context,
        Rc::new(MockMainThreadWebviews),
        main_thread_web_view_panels,
        MainThreadCustomEditorsServices {
            extension_service: Rc::new(ExtensionService),
            storage_service: storage,
            working_copy_service: working_copy_service.clone(),
            working_copy_file_service: Rc::new(WorkingCopyFileService),
            custom_editor_service: custom_editor_service.clone(),
            editor_group_service: TestEditorGroupsService::new(None),
            editor_service: Rc::new(TestEditorService),
            webview_workbench_service: webview_workbench_service.clone(),
            uri_identity_service: Rc::new(UriIdentityService),
            untitled_text_editor_service: Rc::new(UntitledTextEditorService),
            outline_service: Rc::new(OutlineService { outline_changes: outline_changes.clone() }),
            model: model_services.clone(),
        },
    ));

    if text_editor {
        custom_editors.rpc_register_text_editor_provider(extension(), VIEW_TYPE, WebviewPanelOptions::default(), capabilities, false);
    } else {
        custom_editors.rpc_register_custom_editor_provider(extension(), VIEW_TYPE, WebviewPanelOptions::default(), capabilities, false, false);
    }

    let files_configuration_service: Rc<dyn je_workbench::services::files_configuration::files_configuration_service::IFilesConfigurationService> =
        Rc::new(NullFilesConfigurationService);
    let theme_service: Rc<dyn IWebviewThemeService> = Rc::new(TestThemeService);
    let input_services = Rc::new(CustomEditorInputServices {
        theme_service: theme_service.clone(),
        webview_workbench_service: webview_workbench_service.clone(),
        label_service: label_service.clone(),
        custom_editor_service: custom_editor_service.clone(),
        file_dialog_service: Rc::new(FileDialogService),
        undo_redo_service: Rc::new(UndoRedoService),
        file_service: file_service.clone(),
        files_configuration_service: files_configuration_service.clone(),
        editor_groups_service: TestEditorGroupsService::new(None),
        custom_editor_label_service: Rc::new(CustomEditorLabelService),
        untitled_text_editor_service: Rc::new(UntitledTextEditorService),
        webview_service: Rc::new(UnstubbedWebviewService),
    });
    let diff_services = Rc::new(CustomEditorDiffInputServices {
        theme_service,
        webview_workbench_service,
        custom_editor_service,
        files_configuration_service,
        file_dialog_service: Rc::new(FileDialogService),
        undo_redo_service: Rc::new(UndoRedoService),
        webview_service: Rc::new(UnstubbedWebviewService),
    });

    Fixture { calls, models, resolvers, custom_editors, outline_changes, input_services, diff_services, store: store.store() }
}

/// O `ITextModelService` quando o caso não é de texto (o original não o stuba).
struct UnstubbedTextModelService;

impl ITextModelService for UnstubbedTextModelService {
    fn create_model_reference(&self, _resource: &URI) -> Promise<TextModelReference> {
        panic!("Unable to resolve ITextModelService");
    }
}

impl Fixture {
    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    fn create_input(&self) -> Created {
        let webview = OverlayWebview::new();
        let input = self.store.add_rc(CustomEditorInput::new(
            CustomEditorInputInitInfo { resource: resource(), view_type: VIEW_TYPE.to_owned(), webview_title: None, preferred_name: Some("file.custom".to_owned()), icon_path: None },
            webview.clone(),
            CustomEditorInputOptions::default(),
            self.input_services.clone(),
        ));
        Created { input, webview }
    }

    fn create_diff_input(&self, original_resource: URI) -> Created {
        let webview = OverlayWebview::new();
        let input = self.store.add_rc(CustomEditorDiffInput::new(
            CustomEditorDiffInputInitInfo {
                original_resource,
                modified_resource: resource(),
                view_type: VIEW_TYPE.to_owned(),
                label: Some("file.custom".to_owned()),
                description: None,
                icon_path: None,
            },
            webview.clone(),
            self.diff_services.clone(),
        ));
        Created { input, webview }
    }

    fn outline_changes(&self) -> usize {
        self.outline_changes.get()
    }
}

/// `input.navigation`.
fn navigation(input: &Rc<WebviewInput>) -> Option<Rc<dyn je_workbench::contrib::custom_editor::custom_text_editor_navigation::ICustomTextEditorNavigation>> {
    input.kind_as::<CustomEditorInput>().expect("a entrada é um CustomEditorInput").navigation()
}

/// `input.resolve()`.
fn resolve(input: &Rc<WebviewInput>) -> Promise<Option<Rc<dyn IEditorModel>>> {
    EditorInput::resolve(&**input)
}

fn has_call_starting_with(calls: &[String], prefix: &str) -> bool {
    calls.iter().any(|call| call.starts_with(prefix))
}

// ---- casos

#[test]
fn custom_text_navigation_resolves_only_after_its_editor_and_invalidates_outline_capability_on_disposal() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let fixture = create_custom_editors(&store, ExtHost::not_editable().text().navigation());
        let Created { input, .. } = fixture.create_input();
        assert!(navigation(&input).is_none());
        resolve(&input).await.unwrap();
        assert!(navigation(&input).is_some());
        assert_eq!(fixture.outline_changes(), 1);
        assert_eq!(
            fixture.calls(),
            ["addWebviewInput(webview#1)", "$resolveCustomEditor(/workspace/file.custom, webview#1)", "resolveNavigation(webview#1)"]
        );
        input
            .apply_options(
                &TextEditorOptions {
                    selection: Some(TextEditorSelection { start_line_number: 1.0, start_column: 1.0, end_line_number: None, end_column: None }),
                    ..TextEditorOptions::default()
                },
                CancellationToken::Cancelled,
            )
            .await
            .unwrap();
        input.dispose();
        assert!(navigation(&input).is_none());
        assert_eq!(fixture.outline_changes(), 2);
        assert_eq!(fixture.calls().last().map(String::as_str), Some("disposeNavigation(webview#1)"));
        assert!(unexpected_errors.borrow().is_empty());
    });
}

#[test]
fn unregistering_a_text_provider_removes_navigation_from_still_open_panels() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let fixture = create_custom_editors(&store, ExtHost::not_editable().text().navigation());
        let first = fixture.create_input().input;
        let second = fixture.create_input().input;
        resolve(&first).await.unwrap();
        resolve(&second).await.unwrap();
        let (first_navigation, second_navigation) = (navigation(&first).expect("navegação"), navigation(&second).expect("navegação"));
        assert!(!Rc::ptr_eq(&first_navigation, &second_navigation));
        fixture.custom_editors.rpc_unregister_editor_provider(VIEW_TYPE);
        assert!(navigation(&first).is_none());
        assert!(navigation(&second).is_none());
        assert_eq!(fixture.outline_changes(), 4);
        assert!(unexpected_errors.borrow().is_empty());
    });
}

#[test]
fn navigation_resolution_is_canceled_when_its_panel_closes_before_completion() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let pending_token: Rc<RefCell<Option<CancellationToken>>> = Rc::default();
        let complete: Rc<RefCell<Option<Resolver<Option<CustomTextEditorNavigationResult>>>>> = Rc::default();
        let mut ext_host = ExtHost::not_editable().text().navigation();
        ext_host.resolve_navigation = Some(Box::new({
            let (pending_token, complete) = (pending_token.clone(), complete.clone());
            move |_handle, token| {
                *pending_token.borrow_mut() = Some(token);
                let complete = complete.clone();
                Promise::with_executor(move |resolver| *complete.borrow_mut() = Some(resolver))
            }
        }));
        let fixture = create_custom_editors(&store, ext_host);
        let Created { input, .. } = fixture.create_input();
        let resolution = resolve(&input);
        while complete.borrow().is_none() {
            timeout(0).await.unwrap();
        }
        input.dispose();
        assert!(pending_token.borrow().as_ref().is_some_and(|token| token.is_cancellation_requested()));
        resolution.await.unwrap();
        let complete = complete.borrow().clone().expect("resolver");
        complete.resolve(Some(CustomTextEditorNavigationResult { selection: None }));
        assert!(navigation(&input).is_none());
        assert_eq!(fixture.outline_changes(), 0);
        assert!(unexpected_errors.borrow().is_empty());
    });
}

#[test]
fn binary_custom_editors_and_custom_text_diff_panels_do_not_resolve_navigation() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let binary = create_custom_editors(&store, ExtHost::not_editable().navigation());
        let binary_input = binary.create_input().input;
        resolve(&binary_input).await.unwrap();
        let mut text_ext_host = ExtHost::not_editable().text().navigation();
        text_ext_host.capabilities.supports_inline_diff = Some(true);
        let text = create_custom_editors(&store, text_ext_host);
        let diff_input = text.create_diff_input(URI::file("/workspace/original.custom", Platform::Posix)).input;
        resolve(&diff_input).await.unwrap();
        assert!(!has_call_starting_with(&binary.calls(), "resolveNavigation"));
        assert!(!has_call_starting_with(&text.calls(), "resolveNavigation"));
        binary_input.dispose();
        diff_input.dispose();
        timeout(0).await.unwrap();
    });
}

#[test]
fn navigation_errors_leave_the_custom_panel_intact_without_a_text_editor_fallback() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let mut ext_host = ExtHost::not_editable().text().navigation();
        ext_host.resolve_navigation = Some(Box::new(|_handle, _token| rejected("navigation failed")));
        let fixture = create_custom_editors(&store, ext_host);
        let Created { input, webview } = fixture.create_input();
        resolve(&input).await.unwrap();
        assert!(navigation(&input).is_none());
        assert_eq!(*webview.html.borrow(), None);
        assert_eq!(*unexpected_errors.borrow(), ["navigation failed"]);
        input.dispose();
        timeout(0).await.unwrap();
    });
}

#[test]
fn selection_notifications_during_navigation_resolution_supersede_its_initial_snapshot() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let publish: Rc<RefCell<Option<Resolver<Option<CustomTextEditorNavigationResult>>>>> = Rc::default();
        let panel_handle: Rc<RefCell<Option<String>>> = Rc::default();
        let mut ext_host = ExtHost::not_editable().text().navigation();
        ext_host.resolve_navigation = Some(Box::new({
            let (publish, panel_handle) = (publish.clone(), panel_handle.clone());
            move |handle, _token| {
                *panel_handle.borrow_mut() = Some(handle.to_owned());
                let publish = publish.clone();
                Promise::with_executor(move |resolver| *publish.borrow_mut() = Some(resolver))
            }
        }));
        let fixture = create_custom_editors(&store, ext_host);
        let Created { input, .. } = fixture.create_input();
        let resolution = resolve(&input);
        while publish.borrow().is_none() {
            timeout(0).await.unwrap();
        }
        let selection = Selection::new(1, 1, 1, 3);
        let handle = panel_handle.borrow().clone().expect("handle");
        fixture.custom_editors.rpc_on_did_change_custom_text_editor_selection(&handle, Some(selection));
        let publish = publish.borrow().clone().expect("resolver");
        publish.resolve(Some(CustomTextEditorNavigationResult { selection: None }));
        resolution.await.unwrap();
        assert_eq!(navigation(&input).and_then(|navigation| navigation.selection()), Some(selection));
        input.dispose();
        timeout(0).await.unwrap();
    });
}

#[test]
fn creates_the_document_before_resolving_the_editor_and_disposes_it_with_the_editor() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let custom_editors = create_custom_editors(&store, ExtHost::not_editable());

        let Created { input, .. } = custom_editors.create_input();
        resolve(&input).await.unwrap();
        input.dispose();
        timeout(0).await.unwrap();

        assert_eq!(
            (custom_editors.calls(), unexpected_errors.borrow().clone()),
            (
                vec![
                    "addWebviewInput(webview#1)".to_owned(),
                    "$createCustomDocument(/workspace/file.custom)".to_owned(),
                    "$resolveCustomEditor(/workspace/file.custom, webview#1)".to_owned(),
                    "$disposeCustomDocument(/workspace/file.custom)".to_owned(),
                ],
                Vec::<String>::new()
            )
        );
    });
}

#[test]
fn sets_an_error_page_instead_of_rejecting_when_the_extension_fails_to_resolve_the_editor() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let mut ext_host = ExtHost::not_editable();
        ext_host.resolve_custom_editor = Some(Box::new(|| rejected("Could not render the editor")));
        let custom_editors = create_custom_editors(&store, ext_host);

        let Created { input, webview } = custom_editors.create_input();
        let resolver = custom_editors.resolvers.borrow().iter().find(|resolver| resolver.can_resolve(&input)).cloned().expect("o resolvedor do provedor");
        resolver.resolve_webview(input.clone(), CancellationToken::None).await.unwrap();
        let html = webview.html.borrow().clone();
        input.dispose();
        timeout(0).await.unwrap();

        assert_eq!(
            (custom_editors.calls(), html, unexpected_errors.borrow().clone()),
            (
                vec![
                    "addWebviewInput(webview#1)".to_owned(),
                    "$createCustomDocument(/workspace/file.custom)".to_owned(),
                    "$resolveCustomEditor(/workspace/file.custom, webview#1)".to_owned(),
                    "$disposeCustomDocument(/workspace/file.custom)".to_owned(),
                ],
                Some(format!("failed to load {VIEW_TYPE}")),
                vec!["Could not render the editor".to_owned()]
            )
        );
    });
}

/// `createCustomDocument: async ({ attempt }) => { if (attempt === 1) throw new Error('Could not open the document'); ... }`.
fn fails_on_first_attempt() -> ExtHost {
    ExtHost::new(|request| {
        if request.attempt == 1 {
            return rejected("Could not open the document");
        }
        Promise::resolved(CreateCustomDocumentResult { editable: false })
    })
}

#[test]
fn asks_the_extension_again_when_a_new_editor_opens_a_document_that_failed_to_open_250622() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let custom_editors = create_custom_editors(&store, fails_on_first_attempt());

        let failed = custom_editors.create_input();
        let error = resolve(&failed.input).await.err().expect("deveria rejeitar");
        assert!(error.message.contains("Could not open the document"));
        failed.input.dispose();

        let reopened = custom_editors.create_input();
        resolve(&reopened.input).await.unwrap();
        reopened.input.dispose();
        timeout(0).await.unwrap();

        assert_eq!(
            (custom_editors.calls(), unexpected_errors.borrow().clone()),
            (
                vec![
                    "addWebviewInput(webview#1)".to_owned(),
                    "$createCustomDocument(/workspace/file.custom)".to_owned(),
                    "addWebviewInput(webview#2)".to_owned(),
                    "$createCustomDocument(/workspace/file.custom)".to_owned(),
                    "$resolveCustomEditor(/workspace/file.custom, webview#2)".to_owned(),
                    "$disposeCustomDocument(/workspace/file.custom)".to_owned(),
                ],
                Vec::<String>::new()
            )
        );
    });
}

#[test]
fn asks_the_extension_again_when_retrying_an_editor_whose_document_failed_to_open_250622() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let custom_editors = create_custom_editors(&store, fails_on_first_attempt());

        let Created { input, .. } = custom_editors.create_input();
        let error = resolve(&input).await.err().expect("deveria rejeitar");
        assert!(error.message.contains("Could not open the document"));
        resolve(&input).await.unwrap();
        input.dispose();
        timeout(0).await.unwrap();

        assert_eq!(
            (custom_editors.calls(), unexpected_errors.borrow().clone()),
            (
                vec![
                    "addWebviewInput(webview#1)".to_owned(),
                    "$createCustomDocument(/workspace/file.custom)".to_owned(),
                    "$createCustomDocument(/workspace/file.custom)".to_owned(),
                    "$resolveCustomEditor(/workspace/file.custom, webview#1)".to_owned(),
                    "$disposeCustomDocument(/workspace/file.custom)".to_owned(),
                ],
                Vec::<String>::new()
            )
        );
    });
}

#[test]
fn retries_an_editor_whose_shared_document_was_canceled_by_closing_another_editor_250622() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let custom_editors = create_custom_editors(
            &store,
            ExtHost::new(|request| {
                if request.attempt == 1 {
                    return Promise::with_executor(move |resolver| {
                        let listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
                        let listener_slot = listener.clone();
                        *listener.borrow_mut() = Some(request.token.on_cancellation_requested().listen(move |_| {
                            let listener = listener_slot.borrow_mut().take();
                            if let Some(listener) = listener {
                                listener.dispose();
                            }
                            resolver.reject(Error::cancellation());
                        }));
                    });
                }
                Promise::resolved(CreateCustomDocumentResult { editable: false })
            }),
        );

        let closed = custom_editors.create_input();
        let remaining = custom_editors.create_input();
        let resolving = [resolve(&closed.input), resolve(&remaining.input)];
        timeout(0).await.unwrap();
        closed.input.dispose();
        let mut results = Vec::new();
        for promise in resolving {
            results.push(match promise.await {
                Ok(_) => "fulfilled".to_owned(),
                Err(error) => error.message,
            });
        }

        resolve(&remaining.input).await.unwrap();
        remaining.input.dispose();
        timeout(0).await.unwrap();

        assert_eq!(
            (results, custom_editors.calls(), unexpected_errors.borrow().clone()),
            (
                vec!["fulfilled".to_owned(), "Canceled".to_owned()],
                vec![
                    "addWebviewInput(webview#1)".to_owned(),
                    "$createCustomDocument(/workspace/file.custom)".to_owned(),
                    "addWebviewInput(webview#2)".to_owned(),
                    "$createCustomDocument(/workspace/file.custom)".to_owned(),
                    "$resolveCustomEditor(/workspace/file.custom, webview#2)".to_owned(),
                    "$disposeCustomDocument(/workspace/file.custom)".to_owned(),
                ],
                Vec::<String>::new()
            )
        );
    });
}

#[test]
fn retries_an_inline_diff_whose_original_document_failed_to_open_250622() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let unexpected_errors = capture_unexpected_errors();
    run_with_faked_timers(async move {
        let original_resource = URI::file("/workspace/original.custom", Platform::Posix);
        let original_path = original_resource.path().to_string_lossy();
        let mut ext_host = ExtHost::new(move |request| {
            if request.path == original_path && request.attempt == 1 {
                return rejected("Could not open the original document");
            }
            Promise::resolved(CreateCustomDocumentResult { editable: false })
        });
        ext_host.capabilities.supports_inline_diff = Some(true);
        let custom_editors = create_custom_editors(&store, ext_host);

        let Created { input, .. } = custom_editors.create_diff_input(original_resource);
        let error = resolve(&input).await.err().expect("deveria rejeitar");
        assert!(error.message.contains("Could not open the original document"));
        resolve(&input).await.unwrap();
        input.dispose();
        timeout(0).await.unwrap();

        assert_eq!(
            (custom_editors.calls(), unexpected_errors.borrow().clone()),
            (
                vec![
                    "addWebviewInput(webview#1)".to_owned(),
                    "$createCustomDocument(/workspace/file.custom)".to_owned(),
                    "$createCustomDocument(/workspace/original.custom)".to_owned(),
                    "$disposeCustomDocument(/workspace/file.custom)".to_owned(),
                    "$createCustomDocument(/workspace/file.custom)".to_owned(),
                    "$createCustomDocument(/workspace/original.custom)".to_owned(),
                    "$resolveCustomEditorInlineDiff(/workspace/original.custom, /workspace/file.custom, webview#1)".to_owned(),
                    "$disposeCustomDocument(/workspace/original.custom)".to_owned(),
                    "$disposeCustomDocument(/workspace/file.custom)".to_owned(),
                ],
                Vec::<String>::new()
            )
        );
    });
}
