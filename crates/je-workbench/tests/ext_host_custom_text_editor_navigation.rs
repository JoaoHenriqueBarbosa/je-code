//! Porte de `src/vs/workbench/api/test/common/extHostCustomTextEditorNavigation.test.ts` (suíte `ExtHostCustomTextEditorNavigation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; os casos `async` rodam em
//!   `run_with_faked_timers`. Como em `ext_host_custom_editors.rs`, o `ExtHostDocuments` e os `ExtHostWebviews` e
//!   `ExtHostWebviewPanels` são criados ANTES do guarda (o `setup` roda antes do rastreador existir).
//! - `mock<ExtHostDocuments>()` (com `getDocument` que devolve um documento só com `uri`) é o `ExtHostDocuments` real sobre
//!   um `ExtHostDocumentsAndEditors` que recebe o documento `/test/document` por `$acceptDocumentsAndEditorsDelta`.
//! - `mock<ExtHostWebviews>()` e `mock<ExtHostWebviewPanels>()` (o painel só com `onDidDispose` e um `Map` por handle) são os
//!   reais, sobre o `MockMainThreadWebviewManager::noop()`. O `disposed.fire()` do original vira o
//!   `$onDidDisposeWebviewPanel` do painel (`rpc_on_did_dispose_webview_panel`), que dispara o `onDidDispose` do painel. O
//!   `Emitter<void>` `disposed` é compartilhado entre todos os painéis no original; os casos que o disparam têm um painel só.
//! - `new class extends mock<MainThreadCustomEditorsShape>()` com três `override` é o [`MainThreadCustomEditors`]: os três
//!   métodos respondem e os demais falham com `panic!`, como o método ausente do objeto vazio lançaria `TypeError`.
//! - O provedor literal `{ resolveCustomTextEditor, resolveCustomTextEditorNavigation }` é o [`NavigationProvider`] (o
//!   `typeof resolveCustomTextEditorNavigation === 'function'` é o `as_navigation`); o controlador literal é o [`Controller`].
//! - `selection: new Selection(1, 2, 1, 5)` etc. usam o `Selection` de `ext_host_types`; o `captured: unknown` (`{ scroll: 42,
//!   nonSerializable: () => 1 }`, uma promessa, ou um objeto com callback) é um [`Dynamic`] ou uma [`Promise`] de [`Dynamic`].
//!   `assert.deepStrictEqual(restored, [opaque])` compara a identidade do objeto (`strict_equals`).
//! - `await Promise.resolve()` é `Promise::resolved(()).await`.
//! - `assert.throws(fn, /customTextEditorNavigation/)` é a mensagem do `Err` conter o texto.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/webview_support.rs"]
mod webview_support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::path::Platform;
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::model::Eol;
use je_editor::selection::Selection as EditorSelection;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_custom_editors::{
    CustomEditorRegistrationOptions, CustomTextEditorNavigation, CustomTextEditorNavigationProvider, CustomTextEditorProvider, CustomTextEditorRevealOptions,
    EditorProvider, ExtHostCustomEditors,
};
use je_workbench::api::common::ext_host_document_data::{IExtHostDocumentSaveDelegate, TextDocument};
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::{
    CreateDocumentOptions, CustomEditorProviderCapabilities, CustomEditorResolveInitData, CustomTextEditorNavigationResult, ExtHostCustomEditorsShape,
    ExtHostWebviewPanelsShape, IMainContext, MainContext, MainThreadCustomEditorsShape, MainThreadDocumentsShape, DocumentsAndEditorsDelta,
    ExtHostDocumentsAndEditorsShape, ModelAddedData, OpenDocumentOptions, WebviewContentOptions, WebviewExtensionDescription, WebviewPanelOptions,
};
use je_workbench::api::common::ext_host_types::{Range, Selection};
use je_workbench::api::common::ext_host_webview::ExtHostWebviews;
use je_workbench::api::common::ext_host_webview_panels::{ExtHostWebviewPanel, ExtHostWebviewPanels};
use je_workbench::contrib::webview::webview_common::WebviewRemoteInfo;
use je_workbench::services::extensions::extensions::null_extension_description;
use webview_support::{MapMainContext, MockMainThreadWebviewManager, NullApiDeprecationService};

const VIEW_TYPE: &str = "test.navigation";

/// `URI.file('/test/document')`.
fn resource() -> URI {
    URI::file("/test/document", Platform::Posix)
}

/// O `MainThreadDocuments` que nenhum método do caso chama.
struct UnusedMainThreadDocuments;

impl IDisposable for UnusedMainThreadDocuments {
    fn dispose(&self) {}
}

impl IExtHostDocumentSaveDelegate for UnusedMainThreadDocuments {
    fn rpc_try_save_document(&self, _uri: &URI) -> Promise<bool> {
        panic!("o proxy não deveria ser chamado");
    }
}

impl MainThreadDocumentsShape for UnusedMainThreadDocuments {
    fn rpc_try_create_document(&self, _options: Option<CreateDocumentOptions>) -> Promise<URI> {
        panic!("o proxy não deveria ser chamado");
    }

    fn rpc_try_open_document(&self, _uri: &URI, _options: Option<OpenDocumentOptions>) -> Promise<URI> {
        panic!("o proxy não deveria ser chamado");
    }
}

/// O `proxy` do original: `$onDidChangeCustomTextEditorSelection` guarda em `changes`.
#[derive(Default)]
struct MainThreadCustomEditors {
    changes: RefCell<Vec<(String, Option<EditorSelection>)>>,
}

impl IDisposable for MainThreadCustomEditors {
    fn dispose(&self) {}
}

impl MainThreadCustomEditorsShape for MainThreadCustomEditors {
    fn rpc_on_did_change_custom_text_editor_selection(&self, handle: &str, selection: Option<EditorSelection>) {
        self.changes.borrow_mut().push((handle.to_owned(), selection));
    }

    fn rpc_register_text_editor_provider(
        &self,
        _extension: WebviewExtensionDescription,
        _view_type: &str,
        _options: WebviewPanelOptions,
        _capabilities: CustomEditorProviderCapabilities,
        _serialize_buffers_for_post_message: bool,
    ) {
    }

    fn rpc_register_custom_editor_provider(
        &self,
        _extension: WebviewExtensionDescription,
        _view_type: &str,
        _options: WebviewPanelOptions,
        _capabilities: CustomEditorProviderCapabilities,
        _supports_multiple_editors_per_document: bool,
        _serialize_buffers_for_post_message: bool,
    ) {
        panic!("o proxy não deveria ser chamado");
    }

    fn rpc_unregister_editor_provider(&self, _view_type: &str) {}

    fn rpc_on_did_edit(&self, _resource: &URI, _view_type: &str, _edit_id: u32, _label: Option<&str>) {
        panic!("o proxy não deveria ser chamado");
    }

    fn rpc_on_content_change(&self, _resource: &URI, _view_type: &str) {
        panic!("o proxy não deveria ser chamado");
    }
}

/// O controlador literal `vscode.CustomTextEditorNavigation` do `setupNavigation`.
struct Controller {
    selection: Selection,
    selection_changed: Rc<Emitter<Option<Selection>>>,
    revealed: RefCell<Vec<(Range, CustomTextEditorRevealOptions, bool)>>,
    captured: RefCell<MaybePromise<Dynamic>>,
    restored: RefCell<Vec<Dynamic>>,
    dispose_count: Cell<u32>,
}

impl IDisposable for Controller {
    fn dispose(&self) {
        self.dispose_count.set(self.dispose_count.get() + 1);
    }
}

impl CustomTextEditorNavigation for Controller {
    fn selection(&self) -> Option<Selection> {
        Some(self.selection)
    }

    fn on_did_change_selection(&self) -> Event<Option<Selection>> {
        self.selection_changed.event()
    }

    fn reveal_range(&self, range: &Range, options: CustomTextEditorRevealOptions, token: CancellationToken) -> MaybePromise<()> {
        self.revealed.borrow_mut().push((*range, options, token.is_cancellation_requested()));
        MaybePromise::Value(())
    }

    fn capture_view_state(&self) -> MaybePromise<Dynamic> {
        self.captured.borrow().clone()
    }

    fn restore_view_state(&self, state: Dynamic, _token: CancellationToken) -> MaybePromise<()> {
        self.restored.borrow_mut().push(state);
        MaybePromise::Value(())
    }
}

type ResolveNavigation = Box<dyn Fn() -> MaybePromise<Rc<dyn CustomTextEditorNavigation>>>;

/// O provedor literal `{ resolveCustomTextEditor: () => { resolved = true; }, resolveCustomTextEditorNavigation }`.
struct NavigationProvider {
    resolved: Rc<Cell<bool>>,
    resolve_navigation: ResolveNavigation,
}

impl CustomTextEditorProvider for NavigationProvider {
    fn resolve_custom_text_editor(&self, _document: TextDocument, _panel: Rc<ExtHostWebviewPanel>, _token: CancellationToken) -> MaybePromise<()> {
        self.resolved.set(true);
        MaybePromise::Value(())
    }

    fn as_navigation(&self) -> Option<&dyn CustomTextEditorNavigationProvider> {
        Some(self)
    }
}

impl CustomTextEditorNavigationProvider for NavigationProvider {
    fn resolve_custom_text_editor_navigation(
        &self,
        _document: TextDocument,
        _panel: Rc<ExtHostWebviewPanel>,
        _token: CancellationToken,
    ) -> MaybePromise<Rc<dyn CustomTextEditorNavigation>> {
        (self.resolve_navigation)()
    }
}

/// O `ExtHostCustomEditors` do original sobre o `proxy` e os colaboradores reais.
struct Setup {
    editors: Rc<ExtHostCustomEditors>,
    proxy: Rc<MainThreadCustomEditors>,
    panels: Rc<ExtHostWebviewPanels>,
    webviews: Rc<ExtHostWebviews>,
}

fn setup_editors() -> Setup {
    let rpc: Rc<dyn IMainContext> = Rc::new(MapMainContext::default());
    let proxy = Rc::new(MainThreadCustomEditors::default());
    let proxy_shape: Rc<dyn MainThreadCustomEditorsShape> = proxy.clone();
    rpc.set(&MainContext::MAIN_THREAD_CUSTOM_EDITORS, proxy_shape);
    let documents_proxy: Rc<dyn MainThreadDocumentsShape> = Rc::new(UnusedMainThreadDocuments);
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENTS, documents_proxy);
    MockMainThreadWebviewManager::noop().register_in(&*rpc);

    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(rpc.clone(), NullLogService::shared()));
    documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
        added_documents: Some(vec![ModelAddedData {
            is_dirty: false,
            language_id: "plaintext".to_owned(),
            uri: resource(),
            version_id: 1,
            lines: vec![JsString::from("")],
            eol: Eol::Lf,
            encoding: "utf8".to_owned(),
        }]),
        ..DocumentsAndEditorsDelta::default()
    });
    let documents = ExtHostDocuments::new(&*rpc, documents_and_editors);
    let webviews = ExtHostWebviews::new(&*rpc, WebviewRemoteInfo { is_remote: false, authority: None }, None, NullLogService::shared(), Rc::new(NullApiDeprecationService));
    let panels = ExtHostWebviewPanels::new(&*rpc, webviews.clone(), None);
    let editors = Rc::new(ExtHostCustomEditors::new(&*rpc, documents, None, webviews.clone(), panels.clone()));
    Setup { editors, proxy, panels, webviews }
}

/// O `extension` do original: `nullExtensionDescription` com `enabledApiProposals: ['customTextEditorNavigation']`.
fn navigation_extension() -> je_extensions::scanner::ExtensionDescription {
    let mut extension = null_extension_description();
    extension.manifest.enabled_api_proposals = Some(vec!["customTextEditorNavigation".to_owned()]);
    extension
}

/// O objeto devolvido por `setupNavigation`.
struct Fixture {
    setup: Setup,
    registration: Rc<je_workbench::api::common::ext_host_types::Disposable>,
    controller: Rc<Controller>,
    selection_changed: Rc<Emitter<Option<Selection>>>,
}

impl Fixture {
    /// `open(handle, token)`: `$resolveCustomEditor` e depois `$resolveCustomTextEditorNavigation`.
    fn open(&self, handle: &str, token: CancellationToken) -> Promise<Option<CustomTextEditorNavigationResult>> {
        let editors = self.setup.editors.clone();
        let handle = handle.to_owned();
        async_fn(async move {
            editors
                .rpc_resolve_custom_editor(
                    &resource(),
                    &handle,
                    VIEW_TYPE,
                    CustomEditorResolveInitData { title: String::new(), content_options: WebviewContentOptions::default(), options: WebviewPanelOptions::default(), active: true },
                    0.0,
                    token.clone(),
                )
                .await?;
            editors.rpc_resolve_custom_text_editor_navigation(&handle, VIEW_TYPE, &resource(), token).await
        })
    }

    fn changes(&self) -> Vec<(String, Option<EditorSelection>)> {
        self.setup.proxy.changes.borrow().clone()
    }

    fn revealed(&self) -> Vec<(Range, CustomTextEditorRevealOptions, bool)> {
        self.controller.revealed.borrow().clone()
    }

    fn restored(&self) -> Vec<Dynamic> {
        self.controller.restored.borrow().clone()
    }

    /// `setCapture`.
    fn set_capture(&self, value: MaybePromise<Dynamic>) {
        *self.controller.captured.borrow_mut() = value;
    }

    fn dispose_count(&self) -> u32 {
        self.controller.dispose_count.get()
    }

    /// `fixture.disposed.fire()`: o painel é descartado pela thread principal.
    fn fire_disposed(&self, handle: &str) {
        self.setup.panels.rpc_on_did_dispose_webview_panel(handle);
    }
}

/// O `setupNavigation(resolve?)`. `resolve` é o `resolveCustomTextEditorNavigation` alternativo; sem ele, o provedor confere
/// que `resolveCustomTextEditor` já rodou e devolve o controlador.
fn setup_navigation(store: &Rc<DisposableStore>, resolve: Option<ResolveNavigation>) -> Fixture {
    let setup = setup_editors();
    store.add_rc(setup.webviews.clone());
    store.add_rc(setup.panels.clone());
    let selection_changed = Emitter::<Option<Selection>>::new();
    store.add_rc(selection_changed.clone());
    let controller = Rc::new(Controller {
        selection: Selection::from_coordinates(1.0, 2.0, 1.0, 5.0).unwrap(),
        selection_changed: selection_changed.clone(),
        revealed: RefCell::default(),
        // `{ scroll: 42, nonSerializable: () => 1 }`.
        captured: RefCell::new(MaybePromise::Value(Dynamic::object([
            ("scroll", Dynamic::Number(42.0)),
            ("nonSerializable", Dynamic::Function(FunctionRef::native("nonSerializable", 0, |_| Dynamic::Number(1.0)))),
        ]))),
        restored: RefCell::default(),
        dispose_count: Cell::new(0),
    });
    let resolved = Rc::new(Cell::new(false));
    let resolve_navigation: ResolveNavigation = resolve.unwrap_or_else(|| {
        let (resolved, controller) = (resolved.clone(), controller.clone());
        Box::new(move || {
            assert!(resolved.get());
            let controller: Rc<dyn CustomTextEditorNavigation> = controller.clone();
            MaybePromise::Value(controller)
        })
    });
    let provider = Rc::new(NavigationProvider { resolved: resolved.clone(), resolve_navigation });
    let registration = setup
        .editors
        .register_custom_editor_provider(&navigation_extension(), VIEW_TYPE, EditorProvider::Text(provider), CustomEditorRegistrationOptions::default())
        .unwrap();
    let registration = store.add_rc(Rc::new(registration));
    Fixture { setup, registration, controller, selection_changed }
}

/// `{ selectionStartLineNumber, selectionStartColumn, positionLineNumber, positionColumn }`.
fn editor_selection(start_line: i32, start_column: i32, position_line: i32, position_column: i32) -> EditorSelection {
    EditorSelection::new(start_line, start_column, position_line, position_column)
}

#[test]
fn resolves_after_the_panel_and_reports_directional_selection_and_clearing_per_panel() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let fixture = setup_navigation(&store, None);
        assert_eq!(
            fixture.open("panel", CancellationToken::None).await.unwrap(),
            Some(CustomTextEditorNavigationResult { selection: Some(editor_selection(2, 3, 2, 6)) })
        );
        fixture.selection_changed.fire(Some(Selection::from_coordinates(3.0, 4.0, 2.0, 1.0).unwrap()));
        fixture.selection_changed.fire(None);
        assert_eq!(
            fixture.changes(),
            vec![("panel".to_owned(), Some(editor_selection(4, 5, 3, 2))), ("panel".to_owned(), None)]
        );
    });
}

#[test]
fn preview_has_no_selection_and_captured_opaque_state_never_leaves_the_host() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let fixture = setup_navigation(&store, None);
        fixture.open("panel", CancellationToken::None).await.unwrap();
        let range = je_editor::range::Range::new(2, 3, 4, 5);
        fixture.setup.editors.rpc_reveal_custom_text_editor_range("panel", range, None, true, CancellationToken::None).await.unwrap();
        assert_eq!(
            fixture.revealed(),
            vec![(
                Range::from_coordinates(1.0, 2.0, 3.0, 4.0).unwrap(),
                CustomTextEditorRevealOptions { selection: None, preserve_focus: Some(true) },
                false
            )]
        );
        // `{ callback: () => 42 }`.
        let opaque = Dynamic::object([("callback", Dynamic::Function(FunctionRef::native("callback", 0, |_| Dynamic::Number(42.0))))]);
        fixture.set_capture(MaybePromise::Promise(Promise::resolved(opaque.clone())));
        // `assert.strictEqual(await $captureCustomTextEditorViewState(...), undefined)`: a promessa resolve com `()`.
        fixture.setup.editors.rpc_capture_custom_text_editor_view_state("panel", 1).await.unwrap();
        fixture.setup.editors.rpc_restore_custom_text_editor_view_state("panel", 1, CancellationToken::None).await.unwrap();
        fixture.setup.editors.rpc_restore_custom_text_editor_view_state("panel", 1, CancellationToken::None).await.unwrap();
        let restored = fixture.restored();
        assert_eq!(restored.len(), 1);
        assert!(restored[0].strict_equals(&opaque));
    });
}

#[test]
fn released_asynchronous_captures_cannot_resurrect_retained_states() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let fixture = setup_navigation(&store, None);
        fixture.open("panel", CancellationToken::None).await.unwrap();
        let pending = DeferredPromise::<Dynamic>::new();
        fixture.set_capture(MaybePromise::Promise(pending.promise.clone()));
        let capture = fixture.setup.editors.rpc_capture_custom_text_editor_view_state("panel", 7);
        fixture.setup.editors.rpc_release_custom_text_editor_view_state("panel", 7);
        pending.complete(Dynamic::object([("scroll", Dynamic::Number(10.0))])).await.unwrap();
        capture.await.unwrap();
        fixture.setup.editors.rpc_restore_custom_text_editor_view_state("panel", 7, CancellationToken::None).await.unwrap();
        assert!(fixture.restored().is_empty());
    });
}

#[test]
fn cancellation_consumes_captured_states_and_prevents_reveal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let fixture = setup_navigation(&store, None);
        fixture.open("panel", CancellationToken::None).await.unwrap();
        fixture.setup.editors.rpc_capture_custom_text_editor_view_state("panel", 1).await.unwrap();
        fixture.setup.editors.rpc_restore_custom_text_editor_view_state("panel", 1, CancellationToken::Cancelled).await.unwrap();
        fixture.setup.editors.rpc_restore_custom_text_editor_view_state("panel", 1, CancellationToken::None).await.unwrap();
        fixture
            .setup
            .editors
            .rpc_reveal_custom_text_editor_range("panel", je_editor::range::Range::new(1, 1, 1, 1), None, true, CancellationToken::Cancelled)
            .await
            .unwrap();
        assert!(fixture.restored().is_empty());
        assert!(fixture.revealed().is_empty());
    });
}

#[test]
fn panel_and_provider_disposal_unsubscribe_and_dispose_controllers_exactly_once() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let fixture = setup_navigation(&store, None);
        fixture.open("panel", CancellationToken::None).await.unwrap();
        fixture.fire_disposed("panel");
        fixture.selection_changed.fire(None);
        fixture.registration.dispose();
        assert_eq!(fixture.dispose_count(), 1);
        assert!(fixture.changes().is_empty());
    });
}

#[test]
fn provider_disposal_while_resolving_disposes_the_late_controller() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let pending = Rc::new(DeferredPromise::<Rc<dyn CustomTextEditorNavigation>>::new());
        let fixture = setup_navigation(&store, Some(Box::new({
            let pending = pending.clone();
            move || MaybePromise::Promise(pending.promise.clone())
        })));
        let resolution = fixture.open("panel", CancellationToken::None);
        Promise::resolved(()).await.unwrap();
        fixture.registration.dispose();
        let controller: Rc<dyn CustomTextEditorNavigation> = fixture.controller.clone();
        pending.complete(controller).await.unwrap();
        assert_eq!(resolution.await.unwrap(), None);
        assert_eq!(fixture.dispose_count(), 1);
    });
}

#[test]
fn navigation_requires_the_proposal_before_registering_a_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let _store = guard.store();

    let setup = setup_editors();
    let provider = Rc::new(NavigationProvider {
        resolved: Rc::new(Cell::new(false)),
        resolve_navigation: Box::new(|| MaybePromise::Promise(Promise::rejected(Error::new("not reached")))),
    });
    let result = setup.editors.register_custom_editor_provider(
        &null_extension_description(),
        VIEW_TYPE,
        EditorProvider::Text(provider),
        CustomEditorRegistrationOptions::default(),
    );
    let error = result.err().expect("o registro deveria falhar");
    assert!(error.to_string().contains("customTextEditorNavigation"), "{error}");
    setup.panels.dispose();
    setup.webviews.dispose();
}

#[test]
fn controller_state_handles_are_isolated_between_split_panels() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let fixture = setup_navigation(&store, None);
        fixture.open("first", CancellationToken::None).await.unwrap();
        fixture.open("second", CancellationToken::None).await.unwrap();
        let first = Dynamic::object([("scroll", Dynamic::Number(1.0))]);
        let second = Dynamic::object([("scroll", Dynamic::Number(2.0))]);
        fixture.set_capture(MaybePromise::Value(first.clone()));
        fixture.setup.editors.rpc_capture_custom_text_editor_view_state("first", 1).await.unwrap();
        fixture.set_capture(MaybePromise::Value(second.clone()));
        fixture.setup.editors.rpc_capture_custom_text_editor_view_state("second", 1).await.unwrap();
        fixture.setup.editors.rpc_restore_custom_text_editor_view_state("second", 1, CancellationToken::None).await.unwrap();
        fixture.setup.editors.rpc_restore_custom_text_editor_view_state("first", 1, CancellationToken::None).await.unwrap();
        let restored = fixture.restored();
        assert_eq!(restored.len(), 2);
        assert!(restored[0].strict_equals(&second));
        assert!(restored[1].strict_equals(&first));
    });
}
