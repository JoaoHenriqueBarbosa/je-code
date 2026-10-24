//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/find.test.ts` (suíte `Notebook Find`), caso a caso e na mesma ordem. Os 6 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso, e `await withTestNotebook(cells, async (editor, viewModel,
//!   _ds, accessor) => ...)` é o `with_test_notebook` com o corpo síncrono (sob o relógio simulado).
//! - `await found`, a promessa que resolve no primeiro `onFindReplaceStateChange` com `matchesCount`, é o [`MatchesCountWatcher`] (um
//!   ouvinte que marca um flag) seguido de [`settle`], que dispara o atraso de 20 ms do `FindModel` e esvazia as microtarefas.
//! - `editor` (o `mock<IActiveNotebookEditorDelegate>` do original) é o [`FindTestEditor`] sobre o editor de teste, com os membros que o
//!   `FindModel` lê: `find` (o `viewModel.find(...)` sem as células sem ocorrência), `getCellIndex`, `cellAt`, `getFocus` e `getLength`;
//!   `deltaCellDecorations` devolve `[]`, `focusElement` e `revealInView` não fazem nada, e `changeModelDecorations` devolve `null`
//!   (sem chamar o callback) a menos que o caso chame `setupEditorForTest`, que o troca pelo acessor que aplica as decorações nas
//!   células (sem olhar as antigas, como o original).
//! - O `configurationService` com `inspect()` devolvendo `USUAL_WORD_SEPARATORS` é o [`WordSeparatorsConfigurationService`].
//! - `new TestCell(viewModel.viewType, 3, '# next paragraph 1', 'markdown', CellKind.Code, [], languageService)` usado como `ICellDto2` é o
//!   `cell_dto()` do `TestCell`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_editor::find::find_state::{FindReplaceState, INewFindReplaceState};
use je_editor::model::{FindMatch, ValidAnnotatedEditOperation};
use je_editor::range::Range;
use je_editor::word_helper::USUAL_WORD_SEPARATORS;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::models::{ConfigurationChangeEvent, ConfigurationKeys};
use je_platform::configuration::{
    ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue,
};
use je_workbench::contrib::notebook::browser::contrib::find::find_filters::NotebookFindFilters;
use je_workbench::contrib::notebook::browser::contrib::find::find_model::{CellFindMatchModel, CellMatch};
use je_workbench::contrib::notebook::browser::contrib::find::notebook_find_match_decoration_model::{IFindDecorationsEditor, IModelDecorationsChangeAccessor};
use je_workbench::contrib::notebook::browser::contrib::find::notebook_find_model::{CellMatches, FindModel, FindOption, INotebookFindEditor};
use je_workbench::contrib::notebook::browser::notebook_browser::{CellWebviewFindMatch, SearchPreviewInfo, SearchPreviewRange};
use je_workbench::contrib::notebook::browser::notebook_view_events::NotebookCellStateChangedEvent;
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::{
    CellViewModel, ICellModelDecorations, ICellModelDeltaDecorations, NotebookDeltaDecoration, NotebookViewModel,
};
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, ICellEditOperation, ICellReplaceEdit, INotebookFindOptions};
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::test_notebook_editor::{ActiveTestNotebookEditorDelegate, MockNotebookCell, TestCell, empty_metadata, with_test_notebook};

fn absent(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `{ value: USUAL_WORD_SEPARATORS }`.
struct LiteralConfigurationValue;

impl ConfigurationValue for LiteralConfigurationValue {
    fn default_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn application_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn user_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn user_local_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn user_remote_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn workspace_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn workspace_folder_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn memory_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn policy_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn value(&self) -> Dynamic {
        Dynamic::string(USUAL_WORD_SEPARATORS)
    }
    fn override_identifiers(&self) -> Option<Vec<JsString>> {
        None
    }
}

/// `new class extends TestConfigurationService { override inspect() { return configurationValue; } }`.
struct WordSeparatorsConfigurationService {
    inner: Rc<TestConfigurationService>,
}

impl IConfigurationService for WordSeparatorsConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.inner.on_did_change_configuration()
    }
    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        self.inner.get_configuration_data()
    }
    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        self.inner.get_value(section, overrides)
    }
    fn update_value(&self, key: &str, value: Dynamic, overrides: Option<ConfigurationUpdateOverrides>, target: Option<ConfigurationTarget>) -> Promise<()> {
        self.inner.update_value(key, value, overrides, target)
    }
    fn inspect(&self, _key: &str, _overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        Rc::new(LiteralConfigurationValue)
    }
    fn reload_configuration(&self) -> Promise<()> {
        self.inner.reload_configuration()
    }
    fn keys(&self) -> ConfigurationKeys {
        self.inner.keys()
    }
}

/// O acessor de `setupEditorForTest`: aplica as decorações novas na célula dona, sem olhar as antigas.
struct CellDecorationsAccessor {
    view_model: Rc<NotebookViewModel>,
}

impl IModelDecorationsChangeAccessor for CellDecorationsAccessor {
    fn delta_decorations(&self, _old_decorations: &[ICellModelDecorations], new_decorations: &[ICellModelDeltaDecorations]) -> Vec<ICellModelDecorations> {
        let mut ret: Vec<ICellModelDecorations> = Vec::new();
        for dec in new_decorations {
            let cell = self.view_model.view_cells().into_iter().find(|cell| cell.handle() == dec.owner_id);
            let decorations = cell.map(|cell| cell.base().delta_model_decorations(&[], dec.decorations.clone())).unwrap_or_default();

            if !decorations.is_empty() {
                ret.push(ICellModelDecorations { owner_id: dec.owner_id, decorations });
            }
        }

        ret
    }
}

/// O `editor` do caso.
struct FindTestEditor {
    editor: Rc<ActiveTestNotebookEditorDelegate>,
    view_model: Rc<NotebookViewModel>,
    /// `setupEditorForTest` trocou o `changeModelDecorations`.
    decorations_applied: Cell<bool>,
}

impl FindTestEditor {
    /// `setupEditorForTest(editor, viewModel)`.
    fn setup_editor_for_test(&self) {
        self.decorations_applied.set(true);
    }
}

impl IFindDecorationsEditor for FindTestEditor {
    fn change_model_decorations(&self, callback: &mut dyn FnMut(&dyn IModelDecorationsChangeAccessor)) {
        if self.decorations_applied.get() {
            callback(&CellDecorationsAccessor { view_model: self.view_model.clone() });
        }
        // `changeModelDecorations() { return null; }`
    }

    fn delta_cell_decorations(&self, _old_decorations: &[String], _new_decorations: Vec<NotebookDeltaDecoration>) -> Vec<String> {
        Vec::new()
    }

    fn find_highlight_current(&self, _match_index: usize, _owner_id: &str) -> Promise<Option<f64>> {
        absent("findHighlightCurrent")
    }

    fn find_un_highlight_current(&self, _match_index: usize, _owner_id: &str) {
        absent("findUnHighlightCurrent")
    }

    fn find_stop(&self, _owner_id: &str) {
        absent("findStop")
    }
}

impl INotebookFindEditor for FindTestEditor {
    fn get_id(&self) -> String {
        self.editor.get_id()
    }

    fn has_model(&self) -> bool {
        true
    }

    fn text_model(&self) -> Rc<NotebookTextModel> {
        self.editor.text_model.clone()
    }

    fn on_did_change_model(&self) -> Event<Option<Rc<NotebookTextModel>>> {
        Event::none()
    }

    fn on_did_change_cell_state(&self) -> Event<NotebookCellStateChangedEvent> {
        Event::none()
    }

    fn get_view_model(&self) -> Option<Rc<NotebookViewModel>> {
        Some(self.view_model.clone())
    }

    fn find(&self, query: &str, options: &INotebookFindOptions, _token: CancellationToken) -> Promise<Vec<CellMatches>> {
        let find_matches = self
            .view_model
            .find(query, options)
            .into_iter()
            .map(|m| CellFindMatchModel::new(m.cell, m.index, m.content_matches, Vec::new()))
            .filter(|m| m.length() > 0)
            .collect();
        Promise::resolved(find_matches)
    }

    fn get_length(&self) -> usize {
        self.view_model.length()
    }

    fn get_focus(&self) -> ICellRange {
        self.view_model.get_focus()
    }

    fn get_cell_index(&self, cell: &CellViewModel) -> i64 {
        self.view_model.get_cell_index(cell)
    }

    fn cell_at(&self, index: i64) -> Option<CellViewModel> {
        self.view_model.cell_at(index)
    }

    fn focus_element(&self, _cell: &CellViewModel) {}

    fn reveal_cell_offset_in_center(&self, _cell: &CellViewModel, _offset: f64) {
        absent("revealCellOffsetInCenter")
    }

    fn set_cell_editor_selection(&self, _cell: &CellViewModel, _range: &Range) {}

    fn reveal_in_view(&self, _cell: &CellViewModel) -> Promise<()> {
        Promise::resolved(())
    }

    fn reveal_range_in_center_if_outside_viewport_async(&self, _cell: &CellViewModel, _range: &Range) -> Promise<()> {
        Promise::resolved(())
    }
}

/// O `new Promise<boolean>(resolve => state.onFindReplaceStateChange(e => { if (e.matchesCount) { resolve(true); } }))`.
struct MatchesCountWatcher {
    found: Rc<Cell<bool>>,
    listener: DisposableRef,
}

impl MatchesCountWatcher {
    fn new(state: &FindReplaceState<NotebookFindFilters>) -> Self {
        let found = Rc::new(Cell::new(false));
        let flag = found.clone();
        let listener = state.on_find_replace_state_change().listen(move |e| {
            if e.matches_count {
                flag.set(true);
            }
        });
        Self { found, listener }
    }

    /// `await found`.
    fn wait(&self) {
        settle();
        assert!(self.found.get(), "a busca não chegou a atualizar a contagem de ocorrências");
        self.listener.dispose();
    }
}

/// Dispara o atraso do `FindModel` e esvazia as microtarefas.
fn settle() {
    runtime::drain_fake_timers();
}

fn md(source: &'static str) -> MockNotebookCell {
    (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
}

fn state_change(state: &FindReplaceState<NotebookFindFilters>, is_revealed: Option<bool>, search_string: Option<&str>) {
    state.change(INewFindReplaceState { is_revealed, search_string: search_string.map(JsString::from), ..Default::default() }, true, None);
}

/// O começo comum: o editor, o estado e o `FindModel`.
fn with_find_model(
    cells: Vec<MockNotebookCell>,
    setup_editor: bool,
    body: impl FnOnce(&Rc<FindTestEditor>, &Rc<NotebookViewModel>, &Rc<FindReplaceState<NotebookFindFilters>>, &Rc<FindModel>, &dyn Fn() -> Rc<dyn je_editor::language_service::ILanguageService>),
) {
    with_test_notebook(cells, |editor, view_model, _ds, services| {
        let find_editor = Rc::new(FindTestEditor { editor: editor.clone(), view_model: view_model.clone(), decorations_applied: Cell::new(false) });
        if setup_editor {
            find_editor.setup_editor_for_test();
        }
        let test_configuration = TestConfigurationService::new(None);
        let configuration_service: Rc<dyn IConfigurationService> = Rc::new(WordSeparatorsConfigurationService { inner: test_configuration.clone() });
        let state = FindReplaceState::<NotebookFindFilters>::new();
        let editor_dyn: Rc<dyn INotebookFindEditor> = find_editor.clone();
        let model = FindModel::new(editor_dyn, state.clone(), configuration_service);
        let language_service = || services.language_service();

        body(&find_editor, view_model, &state, &model, &language_service);

        settle();
        model.dispose();
        state.dispose();
    });
}

#[test]
fn update_find_matches_basics() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_find_model(vec![md("# header 1"), md("paragraph 1"), md("paragraph 2")], false, |editor, view_model, state, model, language_service| {
        let found = MatchesCountWatcher::new(state);
        state_change(state, Some(true), None);
        state_change(state, None, Some("1"));
        found.wait();
        assert_eq!(model.find_matches().len(), 2);
        assert_eq!(model.current_match(), 0);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 1);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 0);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 1);

        assert_eq!(editor.text_model().length(), 3);

        let found2 = MatchesCountWatcher::new(state);
        let new_cell = TestCell::new(&editor.text_model().notebook_type(), 3, "# next paragraph 1", "markdown", CellKind::Code, Vec::new(), language_service());
        editor.text_model().apply_edits(
            &[ICellEditOperation::Replace(ICellReplaceEdit { index: 3, count: 0, cells: vec![new_cell.cell_dto()] })],
            true,
            None,
            &|| None,
            None,
            true,
        );
        found2.wait();
        assert_eq!(editor.text_model().length(), 4);
        assert_eq!(model.find_matches().len(), 3);
        assert_eq!(model.current_match(), 1);
        new_cell.dispose();
        let _ = view_model;
    });
}

#[test]
fn update_find_matches_basics_2() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let cells = vec![md("# header 1"), md("paragraph 1.1"), md("paragraph 1.2"), md("paragraph 1.3"), md("paragraph 2")];
    with_find_model(cells, true, |editor, _view_model, state, model, _language_service| {
        let found = MatchesCountWatcher::new(state);
        state_change(state, Some(true), None);
        state_change(state, None, Some("1"));
        found.wait();
        // find matches is not necessarily find results
        assert_eq!(model.find_matches().len(), 4);
        assert_eq!(model.current_match(), 0);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 1);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 2);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 3);

        let found2 = MatchesCountWatcher::new(state);
        editor.text_model().apply_edits(&[ICellEditOperation::Replace(ICellReplaceEdit { index: 2, count: 1, cells: Vec::new() })], true, None, &|| None, None, true);
        found2.wait();
        assert_eq!(model.find_matches().len(), 3);

        assert_eq!(model.current_match(), 0);
        model.find(FindOption::Previous(true));
        assert_eq!(model.current_match(), 3);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 0);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 1);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 2);
    });
}

#[test]
fn update_find_matches_basics_3() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let cells = vec![md("# header 1"), md("paragraph 1.1"), md("paragraph 1.2"), md("paragraph 1.3"), md("paragraph 2")];
    with_find_model(cells, true, |editor, _view_model, state, model, _language_service| {
        let found = MatchesCountWatcher::new(state);
        state_change(state, Some(true), None);
        state_change(state, None, Some("1"));
        found.wait();
        // find matches is not necessarily find results
        assert_eq!(model.find_matches().len(), 4);
        assert_eq!(model.current_match(), 0);
        model.find(FindOption::Previous(true));
        assert_eq!(model.current_match(), 4);

        let found2 = MatchesCountWatcher::new(state);
        editor.text_model().apply_edits(&[ICellEditOperation::Replace(ICellReplaceEdit { index: 2, count: 1, cells: Vec::new() })], true, None, &|| None, None, true);
        found2.wait();
        assert_eq!(model.find_matches().len(), 3);
        assert_eq!(model.current_match(), 0);
        model.find(FindOption::Previous(true));
        assert_eq!(model.current_match(), 3);
        model.find(FindOption::Previous(true));
        assert_eq!(model.current_match(), 2);
    });
}

#[test]
fn update_find_matches_112748() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let cells = vec![md("# header 1"), md("paragraph 1.1"), md("paragraph 1.2"), md("paragraph 1.3"), md("paragraph 2")];
    with_find_model(cells, true, |_editor, view_model, state, model, _language_service| {
        let found = MatchesCountWatcher::new(state);
        state_change(state, Some(true), None);
        state_change(state, None, Some("1"));
        found.wait();
        // find matches is not necessarily find results
        assert_eq!(model.find_matches().len(), 4);
        assert_eq!(model.current_match(), 0);
        model.find(FindOption::Previous(false));
        model.find(FindOption::Previous(false));
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 3);
        let found2 = MatchesCountWatcher::new(state);
        let text_buffer = view_model.view_cells()[1].base().text_buffer();
        text_buffer.borrow_mut().apply_edits(
            &[ValidAnnotatedEditOperation {
                identifier: None,
                range: Range::new(1, 1, 1, 14),
                text: Some(JsString::from("")),
                force_move_markers: false,
                is_auto_whitespace_edit: false,
                is_tracked: false,
            }],
            false,
            true,
        );
        // cell content updates, recompute
        let _ = model.research();
        found2.wait();
        assert_eq!(model.current_match(), 1);
    });
}

#[test]
fn reset_when_match_not_found_127198() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_find_model(vec![md("# header 1"), md("paragraph 1"), md("paragraph 2")], false, |editor, _view_model, state, model, _language_service| {
        let found = MatchesCountWatcher::new(state);
        state_change(state, Some(true), None);
        state_change(state, None, Some("1"));
        found.wait();
        assert_eq!(model.find_matches().len(), 2);
        assert_eq!(model.current_match(), 0);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 1);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 0);
        model.find(FindOption::Previous(false));
        assert_eq!(model.current_match(), 1);

        assert_eq!(editor.text_model().length(), 3);

        let found2 = MatchesCountWatcher::new(state);
        state_change(state, None, Some("3"));
        found2.wait();
        assert_eq!(model.current_match(), -1);
        assert_eq!(model.find_matches().len(), 0);
    });
}

#[test]
fn cell_find_match_model() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![md("# header 1"), ("print(1)", "typescript", CellKind::Code, Vec::new(), Some(empty_metadata()))], |_editor, view_model, _ds, _services| {
        let md_cell = view_model.cell_at(0).expect("cell 0");
        let mut md_model = CellFindMatchModel::new(md_cell, 0, Vec::new(), Vec::new());
        assert_eq!(md_model.length(), 0);

        md_model.content_matches.push(FindMatch { range: Range::new(1, 1, 1, 2), matches: Some(Vec::new()) });
        assert_eq!(md_model.length(), 1);
        let preview = |index: usize| CellWebviewFindMatch {
            index,
            search_preview_info: Some(SearchPreviewInfo { line: JsString::from(""), range: SearchPreviewRange { start: 0, end: 0 } }),
        };
        md_model.webview_matches.push(preview(0));
        md_model.webview_matches.push(preview(1));

        assert_eq!(md_model.length(), 3);
        assert!(matches!(md_model.get_match(0), CellMatch::Content(found) if found == md_model.content_matches[0]));
        assert!(matches!(md_model.get_match(1), CellMatch::Webview(found) if found == md_model.webview_matches[0]));
        assert!(matches!(md_model.get_match(2), CellMatch::Webview(found) if found == md_model.webview_matches[1]));
    });
}
