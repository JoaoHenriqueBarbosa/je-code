//! Porte de `src/vs/workbench/contrib/codeEditor/test/browser/documentSymbolsOutline.test.ts` (suíte `DocumentSymbolsOutline
//! adapters`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o `LeakGuard`; a `store` do original é a [`DisposableStore`] dele. Os casos
//!   `async` rodam em `run_with_faked_timers`, e o `await timeout(n)` é o do relógio virtual.
//! - **`createModelServices` e `instantiateTextModel`.** O modelo é o de `test_text_model` (linguagem `plaintext`); o
//!   `ILanguageFeaturesService.documentSymbolProvider` é um [`LanguageFeatureRegistry`] novo por caso, com o provedor registrado
//!   para `'*'`; o `OutlineModelService` é o real, com o `LanguageFeatureDebounceService` de teste e um [`ModelRemovedSource`]
//!   que nenhum caso dispara.
//! - **`instantiation.stub(...)`.** Os serviços entram direto no [`DocumentSymbolsOutlineServices`]: o `IMarkerDecorationsService`
//!   é o dublê sem marcadores ([`NoMarkers`]), o `ITextResourceConfigurationService` devolve `true` para qualquer chave
//!   ([`AlwaysTrueConfiguration`]) e o `IConfigurationService` é o `TestConfigurationService`.
//! - **`mock<IEditorService>()` com `openEditor`.** É o [`RecordingEditorService`]: todo membro que o caso não usa lança `X is not
//!   a function`; `open_editor_in` grava o recurso, as opções e o grupo (`SIDE_GROUP` é `PreferredGroup::Side`).
//! - **`ICustomTextEditorNavigation`.** É o [`FakeNavigation`], com `updateSelection`, as chamadas de `revealRange` gravadas e o
//!   `dispose` que dispara `onDidDispose`.
//! - **`outline.activeElement as OutlineElement`.** O elemento do outline é o `Rc<dyn Any>` do elemento do modelo, e
//!   [`symbol_name`] o recupera por `downcast`.
//! - **`assert.deepStrictEqual(calls, [...])`** são `assert_eq!` sobre as gravações ([`RevealCall`], [`OpenedEditor`]); as seleções
//!   do `selection: range` (`IRange`) são [`TextEditorSelection`].
//! - **Último caso, `stub(editor, 'saveViewState')` e `restoreViewState` do sinon.** O espião não existe: a prova de que o estado
//!   é salvo uma vez e restaurado ao descartar é comportamental (o cursor volta ao lugar onde estava ao capturar). O
//!   `openCodeEditor` é o [`RecordingCodeEditorService`], que grava a entrada, o editor de origem e o `sideBySide`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

/// O suporte de editor do `je-editor/tests/support` que o `withAsyncTestCodeEditor` precisa, no mesmo módulo pai (os arquivos se
/// referem uns aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod mock_environment_service;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use editor_support::mock_environment_service::MockEnvironmentService;
use editor_support::test_code_editor::{TestCodeEditorInstantiationOptions, with_async_test_code_editor};
use editor_support::test_text_model::create_text_model;
use je_base::JsString;
use je_base::async_::{Barrier, async_fn};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, none, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::edit_operation::EditOperation;
use je_editor::language_feature_debounce::LanguageFeatureDebounceService;
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_providers::{DocumentSymbol, DocumentSymbolProvider, ProviderResult, SymbolKind};
use je_editor::language_selector::LanguageSelector;
use je_editor::model::ModelDecoration;
use je_editor::opener_service::{CodeEditorHandle, ICodeEditorService, TextResourceEditorInput};
use je_editor::outline_model::{IOutlineModelService, ModelRemovedSource, OutlineElement as SymbolElement, OutlineModelService};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::services::marker_decorations::{IMarkerDecorationsService, MarkerLimit};
use je_editor::text_model::TextModel;
use je_editor::text_resource_configuration_service::{ITextResourceConfigurationChangeEvent, ITextResourceConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationTarget, ConfigurationValue};
use je_platform::log::NullLogService;
use je_platform::markers::Marker;
use je_platform::opener::editor_types::{EditorOptions, TextEditorSelection, TextEditorSelectionRevealType};
use je_ui::Ui;
use je_workbench::common::editor::{EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, IEditorPane, UntypedEditorInput};
use je_workbench::contrib::code_editor::outline::document_symbol_editor::{code_editor_document_symbol_adapter, custom_text_editor_document_symbol_adapter};
use je_workbench::contrib::code_editor::outline::document_symbols_outline::{DocumentSymbolsOutline, DocumentSymbolsOutlineServices};
use je_workbench::contrib::custom_editor::custom_text_editor_navigation::ICustomTextEditorNavigation;
use je_workbench::services::editor::editor_groups_service::PreferredGroup;
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::services::outline::outline::{IOutline, OutlineElement, OutlineTarget};

/// O membro que o objeto parcial do original não define: chamá-lo é um `TypeError`.
fn not_defined(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `SymbolKind.String` sem etiquetas.
fn symbol(name: &str, range: Range, selection_range: Range, children: Option<Vec<DocumentSymbol>>) -> DocumentSymbol {
    DocumentSymbol {
        name: name.to_owned(),
        detail: String::new(),
        kind: SymbolKind::String,
        tags: Vec::new(),
        container_name: None,
        range,
        selection_range,
        children,
    }
}

/// O `documentSymbolProvider` do `setupOutline`: `[{ name: model.getLineContent(1), ..., children: [Child] }, Other]`.
struct SymbolProvider {
    calls: Rc<Cell<usize>>,
}

impl DocumentSymbolProvider for SymbolProvider {
    fn provide_document_symbols(&self, model: &Rc<TextModel>, _token: &CancellationToken) -> ProviderResult<Vec<DocumentSymbol>> {
        self.calls.set(self.calls.get() + 1);
        let child = symbol("Child", Range::new(2, 1, 3, 5), Range::new(2, 1, 2, 9), None);
        let parent = symbol(&model.get_line_content(1).to_string_lossy(), Range::new(1, 1, 3, 5), Range::new(1, 1, 1, 9), Some(vec![child]));
        let other = symbol("Other", Range::new(4, 1, 4, 8), Range::new(4, 1, 4, 8), None);
        Promise::resolved(Some(vec![parent, other]))
    }
}

/// O `IModelService` do `OutlineModelService`, só com `onModelRemoved`, que nenhum caso dispara.
struct NoModelRemoved {
    on_model_removed: Rc<Emitter<Rc<TextModel>>>,
}

impl ModelRemovedSource for NoModelRemoved {
    fn on_model_removed(&self) -> Event<Rc<TextModel>> {
        self.on_model_removed.event()
    }
}

/// `instantiation.stub(IMarkerDecorationsService, { onDidChangeMarker: Event.None, getLiveMarkers: () => [] })`.
struct NoMarkers;

impl IMarkerDecorationsService for NoMarkers {
    fn on_did_change_marker(&self) -> Event<Rc<TextModel>> {
        Event::none()
    }

    fn on_did_change_marker_limit(&self) -> Event<Rc<TextModel>> {
        not_defined("onDidChangeMarkerLimit")
    }

    fn get_marker(&self, _uri: &URI, _decoration: &ModelDecoration) -> Option<Marker> {
        not_defined("getMarker")
    }

    fn get_live_markers(&self, _uri: &URI) -> Vec<(Range, Marker)> {
        Vec::new()
    }

    fn get_marker_limits(&self, _uri: &URI) -> Option<MarkerLimit> {
        not_defined("getMarkerLimits")
    }

    fn add_marker_suppression(&self, _uri: &URI, _range: Range) -> DisposableRef {
        not_defined("addMarkerSuppression")
    }
}

/// `new class extends mock<ITextResourceConfigurationService>() { override getValue<T>(): T { return true as T; } }`.
struct AlwaysTrueConfiguration;

impl ITextResourceConfigurationService for AlwaysTrueConfiguration {
    fn on_did_change_configuration(&self) -> Event<Rc<dyn ITextResourceConfigurationChangeEvent>> {
        not_defined("onDidChangeConfiguration")
    }

    fn get_value(&self, _resource: Option<&URI>, _section: Option<&str>) -> Dynamic {
        Dynamic::Bool(true)
    }

    fn get_value_at(&self, _resource: Option<&URI>, _position: Option<&Position>, _section: Option<&str>) -> Dynamic {
        not_defined("getValue")
    }

    fn inspect(&self, _resource: Option<&URI>, _position: Option<&Position>, _section: &str) -> Rc<dyn ConfigurationValue> {
        not_defined("inspect")
    }

    fn update_value(&self, _resource: Option<&URI>, _key: &str, _value: Dynamic, _configuration_target: Option<ConfigurationTarget>) -> Promise<()> {
        not_defined("updateValue")
    }
}

/// `{ handle, range, selection, preserveFocus }`.
#[derive(Clone, Debug, PartialEq)]
struct RevealCall {
    handle: String,
    range: Range,
    selection: Option<Selection>,
    preserve_focus: bool,
}

/// Os argumentos de `editorService.openEditor(input, group)`.
#[derive(Clone, Debug, PartialEq)]
struct OpenedEditor {
    resource: Option<URI>,
    preserve_focus: Option<bool>,
    pinned: Option<bool>,
    override_id: Option<String>,
    selection: Option<TextEditorSelection>,
    selection_reveal_type: Option<TextEditorSelectionRevealType>,
    side_group: bool,
}

/// `new class extends mock<IEditorService>() { override async openEditor(input, group) { opened.push([input, group]); } }`.
struct RecordingEditorService {
    opened: Rc<RefCell<Vec<OpenedEditor>>>,
}

impl IEditorService for RecordingEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        not_defined("visibleEditorPanes")
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        not_defined("activeEditor")
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        not_defined("activeTextEditorControl")
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        not_defined("activeTextEditorLanguageId")
    }

    fn open_editor(&self, _editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        not_defined("openEditor")
    }

    fn open_editor_in(&self, editor: UntypedEditorInput, group: Option<PreferredGroup>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        let options = editor.options.clone();
        self.opened.borrow_mut().push(OpenedEditor {
            resource: editor.resource.clone(),
            preserve_focus: options.as_ref().and_then(|options| options.preserve_focus),
            pinned: options.as_ref().and_then(|options| options.pinned),
            override_id: options.as_ref().and_then(|options| options.override_id()),
            selection: options.as_ref().and_then(|options| options.selection),
            selection_reveal_type: options.as_ref().and_then(|options| options.selection_reveal_type),
            side_group: matches!(group, Some(PreferredGroup::Side)),
        });
        Promise::resolved(None)
    }

    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        not_defined("findEditors")
    }

    fn save(&self, _editors: Vec<EditorIdentifier>, _options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        not_defined("save")
    }

    fn save_all(&self, _options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        not_defined("saveAll")
    }

    fn get_editors(&self, _order: EditorsOrder) -> Vec<EditorIdentifier> {
        not_defined("getEditors")
    }

    fn open_editors(&self, _editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        not_defined("openEditors")
    }

    fn is_visible(&self, _editor: &EditorInputRef) -> bool {
        not_defined("isVisible")
    }
}

/// O `ICustomTextEditorNavigation` do `create`.
struct FakeNavigation {
    handle: String,
    model: Rc<TextModel>,
    selection: Cell<Option<Selection>>,
    on_did_change_selection: Rc<Emitter<()>>,
    on_did_dispose: Rc<Emitter<()>>,
    calls: Rc<RefCell<Vec<RevealCall>>>,
}

impl FakeNavigation {
    /// `updateSelection(value)`.
    fn update_selection(&self, value: Option<Selection>) {
        self.selection.set(value);
        self.on_did_change_selection.fire(());
    }
}

impl IDisposable for FakeNavigation {
    /// `this._onDidDispose.fire(); super.dispose();`.
    fn dispose(&self) {
        self.on_did_dispose.fire(());
        self.on_did_change_selection.dispose();
        self.on_did_dispose.dispose();
    }
}

impl ICustomTextEditorNavigation for FakeNavigation {
    fn model(&self) -> Rc<TextModel> {
        Rc::clone(&self.model)
    }

    fn selection(&self) -> Option<Selection> {
        self.selection.get()
    }

    fn on_did_change_selection(&self) -> Event<()> {
        self.on_did_change_selection.event()
    }

    fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose.event()
    }

    fn reveal_range(&self, range: Range, selection: Option<Selection>, preserve_focus: bool, _token: CancellationToken) -> Promise<()> {
        self.calls.borrow_mut().push(RevealCall { handle: self.handle.clone(), range, selection, preserve_focus });
        Promise::resolved(())
    }

    fn capture_view_state(&self) -> DisposableRef {
        none()
    }
}

/// O que o `create` devolve: `{ navigation, outline }`.
struct Created {
    navigation: Rc<FakeNavigation>,
    outline: Rc<DocumentSymbolsOutline>,
}

/// O que o `setupOutline` devolve: `{ create, model, calls, opened, providerCalls }`.
struct Fixture {
    store: Rc<DisposableStore>,
    services: DocumentSymbolsOutlineServices,
    editor_service: Rc<dyn IEditorService>,
    model: Rc<TextModel>,
    calls: Rc<RefCell<Vec<RevealCall>>>,
    opened: Rc<RefCell<Vec<OpenedEditor>>>,
    provider_calls: Rc<Cell<usize>>,
}

impl Fixture {
    /// `providerCalls()`.
    fn provider_calls(&self) -> usize {
        self.provider_calls.get()
    }

    /// `create(handle, selection, target)`.
    async fn create(&self, handle: &str, selection: Option<Selection>, target: OutlineTarget) -> Created {
        let navigation = Rc::new(FakeNavigation {
            handle: handle.to_owned(),
            model: Rc::clone(&self.model),
            selection: Cell::new(selection),
            on_did_change_selection: Emitter::new(),
            on_did_dispose: Emitter::new(),
            calls: Rc::clone(&self.calls),
        });
        self.store.add_rc(Rc::clone(&navigation));
        let barrier = Barrier::new();
        let adapter = custom_text_editor_document_symbol_adapter(Rc::clone(&navigation) as Rc<dyn ICustomTextEditorNavigation>, "test.richMarkdown", Rc::clone(&self.editor_service));
        let outline = DocumentSymbolsOutline::new(adapter, target, barrier.clone(), self.services.clone());
        self.store.add_rc(Rc::clone(&outline));
        barrier.wait().await.expect("o primeiro carregamento termina");
        Created { navigation, outline }
    }
}

/// `setupOutline()`.
fn setup_outline(guard: &LeakGuard) -> Fixture {
    let store = guard.store();
    let registry: LanguageFeatureRegistry<dyn DocumentSymbolProvider> = LanguageFeatureRegistry::new(None);
    let model = create_text_model("# Parent\n## Child\nbody\n# Other");
    let owned_model = Rc::clone(&model);
    store.add(to_disposable(move || owned_model.dispose()));
    let provider_calls = Rc::new(Cell::new(0));
    store.add(registry.register(LanguageSelector::Id("*".to_owned()), Rc::new(SymbolProvider { calls: Rc::clone(&provider_calls) })));

    let environment = MockEnvironmentService::new(true, false);
    let model_removed = NoModelRemoved { on_model_removed: store.add_rc(Emitter::new()) };
    let outline_model_service = Rc::new(OutlineModelService::new(registry.clone(), &LanguageFeatureDebounceService::new(NullLogService::shared(), &*environment), &model_removed));
    store.add_rc(Rc::clone(&outline_model_service));
    let outline_model_service: Rc<dyn IOutlineModelService> = outline_model_service;

    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    let services = DocumentSymbolsOutlineServices {
        ui: Ui::new(),
        language_features: registry,
        outline_model_service,
        configuration_service,
        marker_decorations_service: Rc::new(NoMarkers),
        text_resource_configuration_service: Rc::new(AlwaysTrueConfiguration),
    };

    let opened = Rc::new(RefCell::new(Vec::new()));
    let editor_service: Rc<dyn IEditorService> = Rc::new(RecordingEditorService { opened: Rc::clone(&opened) });
    Fixture { store, services, editor_service, model, calls: Rc::new(RefCell::new(Vec::new())), opened, provider_calls }
}

/// `(outline.activeElement as OutlineElement).symbol.name`.
fn symbol_name(element: Option<OutlineElement>) -> String {
    let element: Rc<dyn Any> = element.expect("há um elemento ativo");
    element.downcast::<SymbolElement>().ok().expect("o elemento ativo é um OutlineElement do modelo").symbol.name.clone()
}

/// `await outline.reveal(...)`: o `Promise<void> | void` do `reveal`.
async fn settle(result: MaybePromise<()>) {
    if let MaybePromise::Promise(promise) = result {
        promise.await.expect("o reveal não rejeita");
    }
}

/// `{ preserveFocus: true, ... }`.
fn editor_options(preserve_focus: Option<bool>, pinned: Option<bool>) -> EditorOptions {
    EditorOptions { preserve_focus, pinned, ..EditorOptions::default() }
}

/// `{ range, text }` de `model.applyEdits`.
fn replace(range: Range, text: &str) -> EditOperation {
    EditOperation::replace(range, Some(JsString::from(text)))
}

#[test]
fn reuses_symbol_providers_and_cached_text_model_outline_across_split_instances() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let fixture = setup_outline(&guard);
        let first = fixture.create("first", Some(Selection::new(2, 1, 2, 1)), OutlineTarget::OUTLINE_PANE).await;
        let second = fixture.create("second", Some(Selection::new(4, 1, 4, 1)), OutlineTarget::BREADCRUMBS).await;
        assert_eq!(fixture.provider_calls(), 1);
        assert_eq!(symbol_name(first.outline.active_element()), "Child");
        assert_eq!(symbol_name(second.outline.active_element()), "Other");
        settle(first.outline.reveal(&first.outline.active_element().unwrap(), &editor_options(Some(true), None), false, false)).await;
        settle(second.outline.reveal(&second.outline.active_element().unwrap(), &EditorOptions::default(), false, true)).await;
        assert_eq!(
            *fixture.calls.borrow(),
            vec![
                RevealCall { handle: "first".to_owned(), range: Range::new(2, 1, 3, 5), selection: Some(Selection::new(2, 1, 2, 1)), preserve_focus: true },
                RevealCall { handle: "second".to_owned(), range: Range::new(4, 1, 4, 8), selection: Some(Selection::new(4, 1, 4, 8)), preserve_focus: false },
            ]
        );
    });
}

#[test]
fn opening_a_symbol_to_the_side_preserves_the_rich_editor_selection_and_editor_options() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let fixture = setup_outline(&guard);
        let Created { outline, .. } = fixture.create("panel", Some(Selection::new(2, 1, 2, 1)), OutlineTarget::OUTLINE_PANE).await;
        settle(outline.reveal(&outline.active_element().unwrap(), &editor_options(Some(true), Some(true)), true, false)).await;
        assert_eq!(*fixture.calls.borrow(), Vec::<RevealCall>::new());
        assert_eq!(
            *fixture.opened.borrow(),
            vec![OpenedEditor {
                resource: Some(fixture.model.uri()),
                preserve_focus: Some(true),
                pinned: Some(true),
                override_id: Some("test.richMarkdown".to_owned()),
                selection: Some(TextEditorSelection { start_line_number: 2.0, start_column: 1.0, end_line_number: Some(2.0), end_column: Some(1.0) }),
                selection_reveal_type: Some(TextEditorSelectionRevealType::NearTopIfOutsideViewport),
                side_group: true,
            }]
        );
    });
}

#[test]
fn selection_drives_breadcrumb_ancestry_and_clearing_does_not_remove_symbols() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let fixture = setup_outline(&guard);
        let Created { navigation, outline } = fixture.create("panel", None, OutlineTarget::BREADCRUMBS).await;
        assert!(!outline.is_empty());
        assert_eq!(outline.config().breadcrumbs_data_source.get_breadcrumb_elements().len(), 0);
        let active_changes = Rc::new(Cell::new(0));
        let counter = Rc::clone(&active_changes);
        fixture.store.add(outline.on_did_change().listen(move |event| {
            if event.affect_only_active_element {
                counter.set(counter.get() + 1);
            }
        }));
        navigation.update_selection(Some(Selection::new(3, 1, 3, 1)));
        timeout(200).await.unwrap();
        let labels: Vec<String> = outline.config().breadcrumbs_data_source.get_breadcrumb_elements().into_iter().map(|item| item.label).collect();
        assert_eq!(labels, vec!["# Parent".to_owned(), "Child".to_owned()]);
        navigation.update_selection(None);
        timeout(200).await.unwrap();
        assert!(outline.active_element().is_none());
        assert!(!outline.is_empty());
        assert_eq!(outline.config().breadcrumbs_data_source.get_breadcrumb_elements().len(), 0);
        assert_eq!(active_changes.get(), 2);
    });
}

#[test]
fn live_text_edits_refresh_the_shared_outline_without_a_hidden_code_editor() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let fixture = setup_outline(&guard);
        let Created { outline, .. } = fixture.create("panel", Some(Selection::new(1, 1, 1, 1)), OutlineTarget::OUTLINE_PANE).await;
        let updated = outline.on_did_change().filter(|event| !event.affect_only_active_element, None).to_promise(None);
        fixture.model.apply_edits(&[replace(Range::new(1, 1, 1, 9), "# Renamed")]);
        updated.await.unwrap();
        assert_eq!(symbol_name(outline.active_element()), "# Renamed");
        assert_eq!(fixture.provider_calls(), 2);
    });
}

#[test]
fn preview_scrolls_without_changing_selection_and_disposal_cancels_cursor_listeners() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let fixture = setup_outline(&guard);
        let Created { navigation, outline } = fixture.create("panel", Some(Selection::new(2, 1, 2, 1)), OutlineTarget::OUTLINE_PANE).await;
        let preview = outline.preview(&outline.active_element().unwrap());
        preview.dispose();
        assert_eq!(
            *fixture.calls.borrow(),
            vec![RevealCall { handle: "panel".to_owned(), range: Range::new(2, 1, 3, 5), selection: None, preserve_focus: true }]
        );
        let changes = Rc::new(Cell::new(0));
        let counter = Rc::clone(&changes);
        fixture.store.add(outline.on_did_change().listen(move |_| counter.set(counter.get() + 1)));
        navigation.dispose();
        fixture.model.apply_edits(&[replace(Range::new(1, 1, 1, 1), "x")]);
        outline.dispose();
        timeout(400).await.unwrap();
        assert_eq!(changes.get(), 0);
    });
}

/// `new class extends mock<ICodeEditorService>() { override async openCodeEditor(input, source, sideBySide) { calls.push(...); return editor; } }`.
struct RecordingCodeEditorService {
    calls: Rc<RefCell<Vec<(TextResourceEditorInput, Option<CodeEditorHandle>, Option<bool>)>>>,
    editor: CodeEditorHandle,
}

impl ICodeEditorService for RecordingCodeEditorService {
    fn get_focused_code_editor(&self) -> Option<CodeEditorHandle> {
        not_defined("getFocusedCodeEditor")
    }

    fn open_code_editor(&self, input: TextResourceEditorInput, source: Option<CodeEditorHandle>, side_by_side: Option<bool>) -> Promise<Option<CodeEditorHandle>> {
        self.calls.borrow_mut().push((input, source, side_by_side));
        Promise::resolved(Some(Rc::clone(&self.editor)))
    }
}

/// Se dois `Rc<dyn Any>` apontam para o mesmo objeto (a identidade que o `deepStrictEqual` do original compara).
fn same_object(a: &CodeEditorHandle, b: &CodeEditorHandle) -> bool {
    std::ptr::eq(Rc::as_ptr(a).cast::<()>(), Rc::as_ptr(b).cast::<()>())
}

#[test]
fn ordinary_code_editor_navigation_retains_open_code_editor_selection_focus_and_side_by_side_options() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let _guard = guard;
        with_async_test_code_editor("first\nsecond", TestCodeEditorInstantiationOptions::default(), move |editor, _view_model, _services| {
            async_fn(async move {
                let widget = Rc::clone(editor.widget());
                let calls = Rc::new(RefCell::new(Vec::new()));
                let editor_handle: CodeEditorHandle = widget.clone();
                let code_service = Rc::new(RecordingCodeEditorService { calls: Rc::clone(&calls), editor: Rc::clone(&editor_handle) });
                let adapter = code_editor_document_symbol_adapter(Rc::clone(&widget), code_service);
                let model = ICodeEditor::get_model(widget.as_ref()).expect("o editor tem modelo");
                let range = Range::new(2, 1, 2, 4);
                adapter.reveal(&model.uri(), range, range, &editor_options(Some(true), Some(true)), true).await?;

                let calls = calls.borrow();
                assert_eq!(calls.len(), 1);
                let (input, source, side_by_side) = &calls[0];
                assert_eq!(input.resource, model.uri());
                let options = input.options.as_ref().expect("o input tem opções");
                assert_eq!((options.editor.preserve_focus, options.editor.pinned), (Some(true), Some(true)));
                assert_eq!(options.selection, Some(TextEditorSelection { start_line_number: 2.0, start_column: 1.0, end_line_number: Some(2.0), end_column: Some(4.0) }));
                assert_eq!(options.selection_reveal_type, Some(TextEditorSelectionRevealType::NearTopIfOutsideViewport));
                assert!(same_object(source.as_ref().expect("o editor de origem"), &editor_handle));
                assert_eq!(*side_by_side, Some(true));

                // `saveViewState` uma vez e `restoreViewState` com o estado salvo, ao descartar
                ICodeEditor::set_position(widget.as_ref(), Position::new(1, 3), None);
                let state = adapter.capture_view_state();
                ICodeEditor::set_position(widget.as_ref(), Position::new(2, 2), None);
                state.dispose();
                let restored = ICodeEditor::get_position(widget.as_ref()).expect("o editor tem posição");
                assert_eq!((restored.line_number, restored.column), (1, 3));

                let has_highlight = || model.get_all_decorations(0, false, false).iter().any(|decoration| decoration.options.description == "document-symbols-outline-range-highlight");
                let preview = adapter.preview(range);
                assert!(has_highlight());
                preview.dispose();
                assert!(!has_highlight());
                Ok(())
            })
        })
        .await
        .unwrap();
    });
}
