//! Porte de `src/vs/workbench/test/browser/parts/editor/diffEditorCommandsService.test.ts` (suíte `DiffEditorCommandsService`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o `LeakGuard` de cada caso. Os casos `async` rodam em
//!   `run_with_faked_timers`, e `await` é `.await.unwrap()`.
//! - **`mock<T>()`** são tipos de teste com só os membros que o original sobrescreve (os demais lançam `... is not a function`,
//!   como o `mock`). `mock<ICodeEditor>()` é o [`IActiveCodeEditor`] de cada lado, `mock<IDiffEditor>()` é o [`IDiffEditor`],
//!   `mock<IEditorService>()` é o [`TestEditorService`], `mock<ITextResourceConfigurationService>()` é o
//!   [`TestTextResourceConfigurationService`] e `mock<IContextKeyService>()` é o `MockContextKeyService` (que devolve
//!   `undefined` para a chave de contexto, como o `getContextKeyValue` do original).
//! - **`setup`.** `focusCalls`, `goToDiffCalls` e `resetWidthBasedLayoutCalls` são o [`Recorder`] de cada caso (um por teste,
//!   que repete o `setup`); o `focusCalls = []` do meio do caso do `toggle` é o `clear` do registro.
//! - **`createTextDiffEditor`.** O `Object.create(TextDiffEditor.prototype)` com o `getControl` trocado é o
//!   [`TextDiffEditor::new`] com o controle (sem entrada e sem grupo, como o objeto do original).
//! - **O painel do editor de diff de vários arquivos.** O `new class extends mock<IEditorPane>() { getControl; resetDiffEditorWidthBasedLayout }`
//!   é o [`MultiDiffPane`], que devolve o controle em `get_diff_editor_control` e se oferece como
//!   [`IDiffEditorWidthBasedLayoutReset`].
//! - **`IWrite`.** O `value: unknown` das escritas é sempre booleano nos casos; o recurso é guardado como a `URI` em texto
//!   (`resource.toString()`), que é o que a comparação profunda do original vê.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::selection::Selection;
use je_editor::text_resource_configuration_service::{ITextResourceConfigurationChangeEvent, ITextResourceConfigurationService};
use je_editor::position::Position;
use je_platform::configuration::{ConfigurationTarget, ConfigurationValue};
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::resources::uri_file;
use je_workbench::browser::parts::editor::diff_editor_commands_service::{
    DiffEditorCommandsService, DiffEditorViewMode, FocusTextDiffEditorMode, IDiffEditorCommandsService,
};
use je_workbench::browser::parts::editor::text_diff_editor::{GoToDiffTarget, IDiffEditor, IDiffEditorWidthBasedLayoutReset, TextDiffEditor};
use je_workbench::common::editor::{EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, GroupIdentifier, IEditorPane, UntypedEditorInput};
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, IActiveCodeEditor, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};

fn not_defined(member: &str) -> ! {
    panic!("{member} is not a function")
}

/// `resource.toString()`.
fn res(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

//#region Dublês

/// `IWrite`: `{ resource, key, value }`.
#[derive(Debug, PartialEq)]
struct Write {
    resource: Option<String>,
    key: String,
    value: bool,
}

fn write(resource: &URI, key: &str, value: bool) -> Write {
    Write { resource: Some(res(resource)), key: key.to_owned(), value }
}

/// `focusCalls`, `goToDiffCalls` e `resetWidthBasedLayoutCalls`.
#[derive(Default)]
struct Recorder {
    focus_calls: RefCell<Vec<&'static str>>,
    go_to_diff_calls: RefCell<Vec<GoToDiffTarget>>,
    reset_width_based_layout_calls: Cell<usize>,
}

/// `createOriginalEditor(hasFocus)`: `focus`, `hasTextFocus` e `hasWidgetFocus`.
struct OriginalSide {
    recorder: Rc<Recorder>,
    has_focus: bool,
}

impl IActiveCodeEditor for OriginalSide {
    fn get_selection(&self) -> Option<Selection> {
        not_defined("getSelection")
    }

    fn get_value_in_range(&self, _selection: &Selection) -> Option<JsString> {
        not_defined("getModel")
    }

    fn has_widget_focus(&self) -> bool {
        self.has_focus
    }

    fn has_text_focus(&self) -> bool {
        self.has_focus
    }

    fn focus(&self) {
        self.recorder.focus_calls.borrow_mut().push("original");
    }

    fn get_model_uri(&self) -> Option<URI> {
        not_defined("getModel")
    }
}

/// `createModifiedEditor(model, hasFocus)`: `getModel`, `focus`, `hasTextFocus` e `hasWidgetFocus`.
struct ModifiedSide {
    recorder: Rc<Recorder>,
    model_uri: Option<URI>,
    has_focus: bool,
}

impl IActiveCodeEditor for ModifiedSide {
    fn get_selection(&self) -> Option<Selection> {
        not_defined("getSelection")
    }

    fn get_value_in_range(&self, _selection: &Selection) -> Option<JsString> {
        not_defined("getValueInRange")
    }

    fn has_widget_focus(&self) -> bool {
        self.has_focus
    }

    fn has_text_focus(&self) -> bool {
        false
    }

    fn focus(&self) {
        self.recorder.focus_calls.borrow_mut().push("modified");
    }

    fn get_model_uri(&self) -> Option<URI> {
        self.model_uri.clone()
    }
}

/// `createDiffEditorControl(model, originalFocused, modifiedFocused)`.
struct DiffControl {
    recorder: Rc<Recorder>,
    model_uri: Option<URI>,
    original_focused: bool,
    modified_focused: bool,
}

impl IDiffEditor for DiffControl {
    fn get_original_editor(&self) -> Rc<dyn IActiveCodeEditor> {
        Rc::new(OriginalSide { recorder: self.recorder.clone(), has_focus: self.original_focused })
    }

    fn get_modified_editor(&self) -> Rc<dyn IActiveCodeEditor> {
        Rc::new(ModifiedSide { recorder: self.recorder.clone(), model_uri: self.model_uri.clone(), has_focus: self.modified_focused })
    }

    fn go_to_diff(&self, target: GoToDiffTarget) {
        self.recorder.go_to_diff_calls.borrow_mut().push(target);
    }

    fn reset_width_based_layout(&self) {
        self.recorder.reset_width_based_layout_calls.set(self.recorder.reset_width_based_layout_calls.get() + 1);
    }
}

fn create_diff_editor_control(recorder: &Rc<Recorder>, model_uri: Option<URI>, original_focused: bool, modified_focused: bool) -> Rc<dyn IDiffEditor> {
    Rc::new(DiffControl { recorder: recorder.clone(), model_uri, original_focused, modified_focused })
}

/// `createTextDiffEditor(control)`.
fn create_text_diff_editor(control: Rc<dyn IDiffEditor>) -> Rc<dyn IEditorPane> {
    TextDiffEditor::new(Some(control))
}

/// O painel do caso do editor de diff de vários arquivos: `getControl` e `resetDiffEditorWidthBasedLayout`.
struct MultiDiffPane {
    control: Rc<dyn IDiffEditor>,
    reset_all_calls: Cell<usize>,
}

impl IEditorPane for MultiDiffPane {
    fn input(&self) -> Option<EditorInputRef> {
        None
    }

    fn group_id(&self) -> GroupIdentifier {
        not_defined("group")
    }

    fn get_view_state(&self) -> Option<Dynamic> {
        not_defined("getViewState")
    }

    fn on_did_change_control(&self) -> Event<()> {
        not_defined("onDidChangeControl")
    }

    fn get_diff_editor_control(&self) -> Option<Rc<dyn IDiffEditor>> {
        Some(self.control.clone())
    }

    fn as_diff_editor_width_based_layout_reset(&self) -> Option<&dyn IDiffEditorWidthBasedLayoutReset> {
        Some(self)
    }
}

impl IDiffEditorWidthBasedLayoutReset for MultiDiffPane {
    fn reset_diff_editor_width_based_layout(&self) {
        self.reset_all_calls.set(self.reset_all_calls.get() + 1);
    }
}

/// `new class extends mock<IEditorService>() { activeEditorPane; activeEditor = undefined; visibleEditorPanes = []; visibleEditors = [] }`.
struct TestEditorService {
    active_editor_pane: Option<Rc<dyn IEditorPane>>,
}

impl IEditorService for TestEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        Vec::new()
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        None
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

    fn active_editor_pane(&self) -> Option<Rc<dyn IEditorPane>> {
        self.active_editor_pane.clone()
    }

    fn visible_editors(&self) -> Vec<EditorInputRef> {
        Vec::new()
    }
}

/// `new class extends mock<ITextResourceConfigurationService>() { getValue() { return true } updateValue(resource, key, value) }`.
struct TestTextResourceConfigurationService {
    resource_writes: Rc<RefCell<Vec<Write>>>,
}

impl ITextResourceConfigurationService for TestTextResourceConfigurationService {
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

    fn update_value(&self, resource: Option<&URI>, key: &str, value: Dynamic, _configuration_target: Option<ConfigurationTarget>) -> Promise<()> {
        let Dynamic::Bool(value) = value else { panic!("o valor das escritas dos casos é booleano") };
        self.resource_writes.borrow_mut().push(Write { resource: resource.map(res), key: key.to_owned(), value });
        Promise::resolved(())
    }
}

/// `createService(activeEditorPane)`.
fn create_service(active_editor_pane: Option<Rc<dyn IEditorPane>>) -> (DiffEditorCommandsService, Rc<RefCell<Vec<Write>>>) {
    let resource_writes: Rc<RefCell<Vec<Write>>> = Rc::default();

    let editor_service = Rc::new(TestEditorService { active_editor_pane });
    let text_resource_configuration_service = Rc::new(TestTextResourceConfigurationService { resource_writes: resource_writes.clone() });
    let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();

    let service = DiffEditorCommandsService::new(editor_service, text_resource_configuration_service, context_key_service);
    (service, resource_writes)
}

//#endregion

#[test]
fn navigate_in_diff_editor_goes_to_the_next_previous_change_of_the_active_text_diff_editor() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let recorder = Rc::new(Recorder::default());
    let control = create_diff_editor_control(&recorder, None, false, false);
    let (service, _) = create_service(Some(create_text_diff_editor(control)));

    service.navigate_in_diff_editor(&[], true);
    service.navigate_in_diff_editor(&[], false);

    assert_eq!(*recorder.go_to_diff_calls.borrow(), vec![GoToDiffTarget::Next, GoToDiffTarget::Previous]);
}

#[test]
fn navigate_in_diff_editor_is_a_no_op_when_there_is_no_active_text_diff_editor() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let recorder = Rc::new(Recorder::default());
    let (service, _) = create_service(None);

    service.navigate_in_diff_editor(&[], true);

    assert_eq!(*recorder.go_to_diff_calls.borrow(), Vec::<GoToDiffTarget>::new());
}

#[test]
fn focus_in_diff_editor_focuses_the_requested_side() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let recorder = Rc::new(Recorder::default());
    let control = create_diff_editor_control(&recorder, None, false, false);
    let (service, _) = create_service(Some(create_text_diff_editor(control)));

    service.focus_in_diff_editor(&[], FocusTextDiffEditorMode::Original);
    service.focus_in_diff_editor(&[], FocusTextDiffEditorMode::Modified);

    assert_eq!(*recorder.focus_calls.borrow(), vec!["original", "modified"]);
}

#[test]
fn focus_in_diff_editor_toggle_focuses_the_other_side() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let recorder = Rc::new(Recorder::default());
    let modified_focused_control = create_diff_editor_control(&recorder, None, false, true);
    let (service1, _) = create_service(Some(create_text_diff_editor(modified_focused_control)));
    service1.focus_in_diff_editor(&[], FocusTextDiffEditorMode::Toggle);
    assert_eq!(*recorder.focus_calls.borrow(), vec!["original"]);

    recorder.focus_calls.borrow_mut().clear();

    let original_focused_control = create_diff_editor_control(&recorder, None, false, false);
    let (service2, _) = create_service(Some(create_text_diff_editor(original_focused_control)));
    service2.focus_in_diff_editor(&[], FocusTextDiffEditorMode::Toggle);
    assert_eq!(*recorder.focus_calls.borrow(), vec!["modified"]);
}

#[test]
fn toggle_diff_ignore_trim_whitespace_flips_the_setting_for_the_modified_side_model() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async {
        let recorder = Rc::new(Recorder::default());
        let model = uri_file("/foo.txt");
        let control = create_diff_editor_control(&recorder, Some(model.clone()), false, false);
        let (service, resource_writes) = create_service(Some(create_text_diff_editor(control)));

        service.toggle_diff_ignore_trim_whitespace(&[]).await.unwrap();

        assert_eq!(*resource_writes.borrow(), vec![write(&model, "diffEditor.ignoreTrimWhitespace", false)]);
    });
}

#[test]
fn toggle_diff_ignore_trim_whitespace_is_a_no_op_when_there_is_no_modified_model() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async {
        let recorder = Rc::new(Recorder::default());
        let control = create_diff_editor_control(&recorder, None, false, false);
        let (service, resource_writes) = create_service(Some(create_text_diff_editor(control)));

        service.toggle_diff_ignore_trim_whitespace(&[]).await.unwrap();

        assert_eq!(*resource_writes.borrow(), Vec::<Write>::new());
    });
}

#[test]
fn sets_explicit_and_automatic_diff_view_modes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async {
        let recorder = Rc::new(Recorder::default());
        let model = uri_file("/foo.txt");
        let control = create_diff_editor_control(&recorder, Some(model.clone()), false, false);
        let (service, resource_writes) = create_service(Some(create_text_diff_editor(control)));

        service.set_view_mode(&[], DiffEditorViewMode::Inline).await.unwrap();
        service.set_view_mode(&[], DiffEditorViewMode::SideBySide).await.unwrap();
        service.set_view_mode(&[], DiffEditorViewMode::Automatic).await.unwrap();

        assert_eq!(
            (resource_writes.take(), recorder.reset_width_based_layout_calls.get()),
            (
                vec![
                    write(&model, "diffEditor.renderSideBySide", false),
                    write(&model, "diffEditor.renderSideBySide", true),
                    write(&model, "diffEditor.useInlineViewWhenSpaceIsLimited", false),
                    write(&model, "diffEditor.renderSideBySide", true),
                    write(&model, "diffEditor.useInlineViewWhenSpaceIsLimited", true),
                ],
                1,
            )
        );
    });
}

#[test]
fn sets_the_view_mode_for_a_multi_diff_editor_pane_and_resets_all_embedded_layouts() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async {
        let recorder = Rc::new(Recorder::default());
        let model = uri_file("/foo.txt");
        let control = create_diff_editor_control(&recorder, Some(model.clone()), false, false);
        let pane = Rc::new(MultiDiffPane { control, reset_all_calls: Cell::new(0) });
        let active_editor_pane: Rc<dyn IEditorPane> = pane.clone();
        let (service, resource_writes) = create_service(Some(active_editor_pane));

        service.set_view_mode(&[], DiffEditorViewMode::Automatic).await.unwrap();

        assert_eq!(
            (resource_writes.take(), pane.reset_all_calls.get(), recorder.reset_width_based_layout_calls.get()),
            (
                vec![write(&model, "diffEditor.renderSideBySide", true), write(&model, "diffEditor.useInlineViewWhenSpaceIsLimited", true)],
                1,
                0,
            )
        );
    });
}
