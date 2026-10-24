//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/notebookClipboard.test.ts` (suíte `Notebook Clipboard`), caso a caso e na mesma ordem. Os 4 casos ativos estão portados.
//!
//! Casos de fora (`test.skip` no original, que não os executa, e por isso não viram `#[test]`): `Cut multiple selected cells`,
//! `Cut should take folding info into account`, `Copy should take folding info into account`, `#119773, cut last item should not focus on
//! the top first cell` e `#119771, undo paste should restore selections`. Eles usam a `NotebookClipboardContribution` (a ligação com as
//! ações de copiar, colar e recortar do editor de texto), que depende do `IEditorService` e dos comandos de edição do widget.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso, e `await withTestNotebook(cells, async (editor, viewModel,
//!   _ds, accessor) => ...)` é o `with_test_notebook` com o corpo síncrono (`await viewModel.undo()` é o `undo()`, que o porte executa na hora).
//! - `accessor.get(IClipboardService)` é o `TestClipboardService` de `je-editor/tests/support`, e o `INotebookService` mockado (`setToCopy` e
//!   `getToCopy`) é o [`NotebookServiceMock`], que guarda as células copiadas em `to_copy` e responde `{ items, isCopy: true }`.
//! - `assert.deepStrictEqual(_toCopy, [cell.model, ...])` compara as células por identidade do `Rc`.
//! - `runCopyCells(accessor, editor, cell)` é `run_copy_cells(clipboard, notebook_service, editor, cell)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_clipboard_service.rs"]
mod test_clipboard_service;

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::VSBufferReadableStream;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_workbench::contrib::notebook::browser::contrib::clipboard::notebook_clipboard::{run_copy_cells, run_cut_cells};
use je_workbench::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{
    CellKind, INotebookContributionData, INotebookRendererInfo, INotebookStaticPreloadInfo, ISelectionIndexState, ISelectionState, NotebookExtensionDescription,
};
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use je_workbench::contrib::notebook::common::notebook_service::{INotebookSerializer, INotebookService, NotebookCellsToCopy, SimpleNotebookProviderInfo};
use support::test_notebook_editor::{MockNotebookCell, empty_metadata, with_test_notebook};
use test_clipboard_service::TestClipboardService;

fn absent(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

fn range(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

fn md(source: &'static str) -> MockNotebookCell {
    (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
}

/// `new class extends mock<INotebookService>() { setToCopy; getToCopy }`.
#[derive(Default)]
struct NotebookServiceMock {
    to_copy: RefCell<Vec<Rc<NotebookCellTextModel>>>,
}

impl INotebookService for NotebookServiceMock {
    fn can_resolve(&self, _view_type: &str) -> Promise<bool> { absent("canResolve") }
    fn on_add_view_type(&self) -> Event<String> { absent("onAddViewType") }
    fn on_will_remove_view_type(&self) -> Event<String> { absent("onWillRemoveViewType") }
    fn on_did_change_output_renderers(&self) -> Event<()> { absent("onDidChangeOutputRenderers") }
    fn on_will_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>> { absent("onWillAddNotebookDocument") }
    fn on_did_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>> { absent("onDidAddNotebookDocument") }
    fn on_will_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>> { absent("onWillRemoveNotebookDocument") }
    fn on_did_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>> { absent("onDidRemoveNotebookDocument") }
    fn register_notebook_serializer(&self, _view_type: &str, _extension_data: NotebookExtensionDescription, _serializer: Rc<dyn INotebookSerializer>) -> DisposableRef { absent("registerNotebookSerializer") }
    fn with_notebook_data_provider(&self, _view_type: &str) -> Promise<Rc<SimpleNotebookProviderInfo>> { absent("withNotebookDataProvider") }
    fn try_get_data_provider_sync(&self, _view_type: &str) -> Option<Rc<SimpleNotebookProviderInfo>> { absent("tryGetDataProviderSync") }
    fn get_view_type_provider(&self, _view_type: &str) -> Option<String> { absent("getViewTypeProvider") }
    fn get_renderer_info(&self, _id: &str) -> Option<Rc<dyn INotebookRendererInfo>> { absent("getRendererInfo") }
    fn get_renderers(&self) -> Vec<Rc<dyn INotebookRendererInfo>> { absent("getRenderers") }
    fn get_static_preloads(&self, _view_type: &str) -> Vec<INotebookStaticPreloadInfo> { absent("getStaticPreloads") }
    fn create_notebook_text_model(&self, _view_type: &str, _uri: &URI, _stream: Option<VSBufferReadableStream>) -> Promise<Rc<NotebookTextModel>> { absent("createNotebookTextModel") }
    fn get_notebook_text_model(&self, _uri: &URI) -> Option<Rc<NotebookTextModel>> { absent("getNotebookTextModel") }
    fn get_notebook_text_models(&self) -> Vec<Rc<NotebookTextModel>> { absent("getNotebookTextModels") }
    fn list_notebook_documents(&self) -> Vec<Rc<NotebookTextModel>> { absent("listNotebookDocuments") }
    fn register_contributed_notebook_type(&self, _view_type: &str, _data: INotebookContributionData) -> DisposableRef { absent("registerContributedNotebookType") }
    fn has_supported_notebooks(&self, _resource: &URI) -> bool { absent("hasSupportedNotebooks") }
    fn get_notebook_provider_resource_roots(&self) -> Vec<URI> { absent("getNotebookProviderResourceRoots") }
    fn set_to_copy(&self, items: Vec<Rc<NotebookCellTextModel>>, _is_copy: bool) {
        *self.to_copy.borrow_mut() = items;
    }
    fn get_to_copy(&self) -> Option<NotebookCellsToCopy> {
        Some(NotebookCellsToCopy { items: self.to_copy.borrow().clone(), is_copy: true })
    }
    fn clear_editor_cache(&self) { absent("clearEditorCache") }
}

fn same_models(actual: &[Rc<NotebookCellTextModel>], expected: &[Rc<NotebookCellTextModel>]) -> bool {
    actual.len() == expected.len() && actual.iter().zip(expected).all(|(a, b)| Rc::ptr_eq(a, b))
}

fn index_state(focus: ICellRange, selections: Vec<ICellRange>) -> ISelectionState {
    ISelectionState::Index(ISelectionIndexState { focus, selections })
}

#[test]
fn copy_cell_from_ui_still_works_if_the_target_cell_is_not_part_of_a_selection() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![md("# header 1"), md("paragraph 1"), md("paragraph 2")], |editor, view_model, _ds, _services| {
        let notebook_service = NotebookServiceMock::default();
        let clipboard = TestClipboardService::new();

        view_model.update_selections_state(&index_state(range(0, 1), vec![range(0, 2)]), "model");
        assert!(run_copy_cells(&clipboard, &notebook_service, editor.as_ref(), view_model.cell_at(0).as_ref()));
        assert!(same_models(&notebook_service.get_to_copy().unwrap().items, &[view_model.cell_at(0).unwrap().model(), view_model.cell_at(1).unwrap().model()]));

        assert!(run_copy_cells(&clipboard, &notebook_service, editor.as_ref(), view_model.cell_at(2).as_ref()));
        assert_eq!(notebook_service.get_to_copy().unwrap().items.len(), 1);
        assert!(same_models(&notebook_service.get_to_copy().unwrap().items, &[view_model.cell_at(2).unwrap().model()]));
    });
}

#[test]
fn cut_cell_from_ui_still_works_if_the_target_cell_is_not_part_of_a_selection() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![md("# header 1"), md("paragraph 1"), md("paragraph 2"), md("paragraph 3")], |editor, view_model, _ds, _services| {
        let notebook_service = NotebookServiceMock::default();
        let clipboard = TestClipboardService::new();

        view_model.update_selections_state(&index_state(range(0, 1), vec![range(0, 2)]), "model");
        assert!(run_cut_cells(&clipboard, &notebook_service, editor.as_ref(), view_model.cell_at(0).as_ref()));
        assert_eq!(view_model.length(), 2);
        view_model.undo();
        assert_eq!(view_model.length(), 4);

        assert_eq!(view_model.get_focus(), range(0, 1));
        assert_eq!(view_model.get_selections(), vec![range(0, 2)]);
        assert!(run_cut_cells(&clipboard, &notebook_service, editor.as_ref(), view_model.cell_at(2).as_ref()));
        assert_eq!(view_model.length(), 3);
        assert_eq!(view_model.get_focus(), range(0, 1));
        assert_eq!(view_model.cell_at(0).map(|cell| cell.get_text()), Some("# header 1".to_string()));
        assert_eq!(view_model.cell_at(1).map(|cell| cell.get_text()), Some("paragraph 1".to_string()));
        assert_eq!(view_model.cell_at(2).map(|cell| cell.get_text()), Some("paragraph 3".to_string()));

        view_model.undo();
        assert_eq!(view_model.length(), 4);
        view_model.update_selections_state(&index_state(range(2, 3), vec![range(2, 4)]), "model");
        assert_eq!(view_model.get_focus(), range(2, 3));
        assert!(run_cut_cells(&clipboard, &notebook_service, editor.as_ref(), view_model.cell_at(0).as_ref()));
        assert_eq!(view_model.get_focus(), range(1, 2));
        assert_eq!(view_model.get_selections(), vec![range(1, 3)]);
    });
}

#[test]
fn cut_focus_cell_still_works_if_the_focus_is_not_part_of_any_selection() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![md("# header 1"), md("paragraph 1"), md("paragraph 2"), md("paragraph 3")], |editor, view_model, _ds, _services| {
        let notebook_service = NotebookServiceMock::default();
        let clipboard = TestClipboardService::new();

        view_model.update_selections_state(&index_state(range(0, 1), vec![range(2, 4)]), "model");
        assert!(run_cut_cells(&clipboard, &notebook_service, editor.as_ref(), None));
        assert_eq!(view_model.length(), 3);
        assert_eq!(view_model.get_focus(), range(0, 1));
        assert_eq!(view_model.get_selections(), vec![range(1, 3)]);
    });
}

#[test]
fn cut_focus_cell_still_works_if_the_focus_is_not_part_of_any_selection_2() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![md("# header 1"), md("paragraph 1"), md("paragraph 2"), md("paragraph 3")], |editor, view_model, _ds, _services| {
        let notebook_service = NotebookServiceMock::default();
        let clipboard = TestClipboardService::new();

        view_model.update_selections_state(&index_state(range(3, 4), vec![range(0, 2)]), "model");
        assert!(run_cut_cells(&clipboard, &notebook_service, editor.as_ref(), None));
        assert_eq!(view_model.length(), 3);
        assert_eq!(view_model.get_focus(), range(2, 3));
        assert_eq!(view_model.get_selections(), vec![range(0, 2)]);
    });
}
