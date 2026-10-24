//! Porte de `src/vs/workbench/contrib/bulkEdit/test/browser/bulkCellEdits.test.ts` (suíte `BulkCellEdits`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso. O `store.add(new TestEditorService())` é o
//!   serviço de editores do caso, que não guarda estado (o [`NoActivePaneEditorService`] é o `TestEditorService` do original, com
//!   `activeEditorPane` indefinido: o `TestEditorService` do porte não o define e o padrão do trait falha).
//! - **`mockObject<NotebookTextModel>()`.** O `NotebookTextModel` é a classe concreta do porte, sem como ser um dublê: o caso usa
//!   um notebook de verdade, com uma célula de código (o `Replace { index: 0, count: 1, cells: [] }` do caso a remove), criado por
//!   `create_test_notebook_editor`. O original só confere o argumento do `resolve`, e o `applyEdits` do dublê não faz nada; aqui o
//!   `applyEdits` roda de verdade sobre o notebook, sem que o caso confira o efeito (a asserção é a mesma).
//! - `mockObject<IResolvedNotebookEditorModel>()({ notebook })` com `isReadonly.returns(false)` é o [`ResolvedModel`] (o
//!   `notebook` e `isReadonly() { return false }`); `mockObject<INotebookEditorModelResolverService>()` com
//!   `resolve.returns({ object, dispose: () => { } })` é o [`RecordingResolver`], que guarda os argumentos de `resolve`
//!   (`resolve.args[0]`). O que o original não define é `unreachable!` (o `TypeError`).
//! - `new BulkCellEdits(...)` é [`BulkCellEdits::new`]. O `throw` do construtor (caso `throws for invalid cell URI`) é o pânico de
//!   `errors::throw`, que `errors::catch` converte no `Err` que o `assert.rejects` do original espera; o `runTest` é `async` no
//!   original, então o `throw` do construtor já chega como rejeição.
//! - `assert.strictEqual(resolveArgs[0].toString(), resolveUri.toString())` compara o `to_string(false)` das duas URIs.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable, Reference};
use je_base::promise::Promise;
use je_base::path::Platform;
use je_base::uri::{Field, URI, UriChange};
use je_editor::undo_redo::{UndoRedoGroup, UndoRedoSource};
use je_platform::progress::progress::IProgress;
use je_workbench::common::editor::{
    EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, IEditorPane, RevertOptions, SaveOptions, UntypedEditorInput,
};
use je_workbench::contrib::bulk_edit::browser::bulk_cell_edits::{BulkCellEdits, BulkCellEditsArgs, ResourceNotebookCellEdit};
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{
    CellKind, CellUri, ICellReplaceEdit, INotebookEditorModel, INotebookLoadOptions, INotebookTextModel, IResolvedNotebookEditorModel,
    NotebookEditorModelCreationOptions, WorkspaceNotebookCellEditOperation,
};
use je_workbench::contrib::notebook::common::notebook_editor_model_resolver_service::{
    INotebookConflictEvent, INotebookEditorModelResolverService, IUntitledNotebookResource, NotebookModelReference,
};
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::working_copy::working_copy::WorkingCopySaveEvent;
use support::test_notebook_editor::{create_test_notebook_editor, setup_notebook_services};

/// `const progress: IProgress<void> = { report: _ => { } }`.
struct NoProgress;

impl IProgress<()> for NoProgress {
    fn report(&self, _item: ()) {}
}

/// O `TestEditorService` do original: nenhum painel ativo, nenhum editor.
struct NoActivePaneEditorService;

impl IEditorService for NoActivePaneEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        Vec::new()
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        None
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        None
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        None
    }

    fn open_editor(&self, _editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        Promise::resolved(None)
    }

    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        Vec::new()
    }

    fn save(&self, _editors: Vec<EditorIdentifier>, _options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        panic!("Method not implemented.")
    }

    fn save_all(&self, _options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        panic!("Method not implemented.")
    }

    fn get_editors(&self, _order: EditorsOrder) -> Vec<EditorIdentifier> {
        Vec::new()
    }

    fn open_editors(&self, _editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        panic!("not implemented")
    }

    fn is_visible(&self, _editor: &EditorInputRef) -> bool {
        false
    }

    /// `activeEditorPane: IVisibleEditorPane | undefined` nasce `undefined`.
    fn active_editor_pane(&self) -> Option<Rc<dyn IEditorPane>> {
        None
    }
}

/// `mockObject<IResolvedNotebookEditorModel>()({ notebook })` com `isReadonly.returns(false)`.
struct ResolvedModel {
    notebook: Rc<NotebookTextModel>,
}

impl IDisposable for ResolvedModel {
    fn dispose(&self) {}
}

impl INotebookEditorModel for ResolvedModel {
    fn on_did_change_dirty(&self) -> Event<()> {
        unreachable!("onDidChangeDirty não é definido no mock")
    }

    fn on_did_save(&self) -> Event<WorkingCopySaveEvent> {
        unreachable!("onDidSave não é definido no mock")
    }

    fn on_did_change_orphaned(&self) -> Event<()> {
        unreachable!("onDidChangeOrphaned não é definido no mock")
    }

    fn on_did_change_readonly(&self) -> Event<()> {
        unreachable!("onDidChangeReadonly não é definido no mock")
    }

    fn on_did_revert_untitled(&self) -> Event<()> {
        unreachable!("onDidRevertUntitled não é definido no mock")
    }

    fn resource(&self) -> URI {
        unreachable!("resource não é definido no mock")
    }

    fn view_type(&self) -> String {
        unreachable!("viewType não é definido no mock")
    }

    fn notebook(&self) -> Option<Rc<dyn INotebookTextModel>> {
        Some(self.notebook.clone())
    }

    fn has_error_state(&self) -> bool {
        unreachable!("hasErrorState não é definido no mock")
    }

    fn is_resolved(&self) -> bool {
        unreachable!("isResolved não é definido no mock")
    }

    fn is_dirty(&self) -> bool {
        unreachable!("isDirty não é definido no mock")
    }

    fn is_modified(&self) -> bool {
        unreachable!("isModified não é definido no mock")
    }

    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(false)
    }

    fn is_orphaned(&self) -> bool {
        unreachable!("isOrphaned não é definido no mock")
    }

    fn has_associated_file_path(&self) -> bool {
        unreachable!("hasAssociatedFilePath não é definido no mock")
    }

    fn load(&self, _options: Option<INotebookLoadOptions>) -> Promise<Rc<dyn IResolvedNotebookEditorModel>> {
        unreachable!("load não é definido no mock")
    }

    fn save(&self, _options: Option<SaveOptions>) -> Promise<bool> {
        unreachable!("save não é definido no mock")
    }

    fn save_as(&self, _target: &URI) -> Promise<Option<UntypedEditorInput>> {
        unreachable!("saveAs não é definido no mock")
    }

    fn revert(&self, _options: Option<RevertOptions>) -> Promise<()> {
        unreachable!("revert não é definido no mock")
    }
}

impl IResolvedNotebookEditorModel for ResolvedModel {
    fn resolved_notebook(&self) -> Rc<NotebookTextModel> {
        self.notebook.clone()
    }
}

/// `mockObject<INotebookEditorModelResolverService>()` com `resolve.returns({ object: notebookEditorModel, dispose: () => { } })`.
struct RecordingResolver {
    model: Rc<ResolvedModel>,
    /// `resolve.args`: a URI de cada chamada de `resolve`.
    resolve_args: RefCell<Vec<URI>>,
}

impl INotebookEditorModelResolverService for RecordingResolver {
    fn on_did_save_notebook(&self) -> Event<URI> {
        unreachable!("onDidSaveNotebook não é definido no mock")
    }

    fn on_did_change_dirty(&self) -> Event<Rc<dyn IResolvedNotebookEditorModel>> {
        unreachable!("onDidChangeDirty não é definido no mock")
    }

    fn on_will_fail_with_conflict(&self) -> Event<INotebookConflictEvent> {
        unreachable!("onWillFailWithConflict não é definido no mock")
    }

    fn is_dirty(&self, _resource: &URI) -> bool {
        unreachable!("isDirty não é definido no mock")
    }

    fn create_untitled_notebook_text_model(&self, _view_type: &str) -> Promise<Rc<NotebookTextModel>> {
        unreachable!("createUntitledNotebookTextModel não é definido no mock")
    }

    fn resolve(&self, resource: &URI, _view_type: Option<&str>, _creation_options: Option<NotebookEditorModelCreationOptions>) -> Promise<NotebookModelReference> {
        self.resolve_args.borrow_mut().push(resource.clone());
        Promise::resolved(Reference::immortal(self.model.clone() as Rc<dyn IResolvedNotebookEditorModel>))
    }

    fn resolve_untitled(&self, _resource: &IUntitledNotebookResource, _view_type: &str, _creation_options: Option<NotebookEditorModelCreationOptions>) -> Promise<NotebookModelReference> {
        unreachable!("resolve (untitled) não é definido no mock")
    }
}

/// `runTest(inputUri, resolveUri)`: o `Err` é a rejeição do `async` do original.
async fn run_test(input_uri: URI, resolve_uri: URI) -> Result<(), Error> {
    let progress: Rc<dyn IProgress<()>> = Rc::new(NoProgress);
    let editor_service: Rc<dyn IEditorService> = Rc::new(NoActivePaneEditorService);

    let disposables = DisposableStore::new();
    let services = setup_notebook_services(&disposables);
    let notebook_editor = create_test_notebook_editor(&services, &disposables, vec![("", "javascript", CellKind::Code, Vec::new(), None)]);
    let notebook = notebook_editor.editor.text_model.clone();

    let notebook_editor_model = Rc::new(ResolvedModel { notebook: notebook.clone() });
    let notebook_service = Rc::new(RecordingResolver { model: notebook_editor_model, resolve_args: RefCell::new(Vec::new()) });

    let edits = vec![ResourceNotebookCellEdit::new(
        input_uri,
        WorkspaceNotebookCellEditOperation::Replace(ICellReplaceEdit { index: 0, count: 1, cells: Vec::new() }),
        None,
        None,
    )];

    let outcome = async {
        let bce = errors::catch(|| {
            BulkCellEdits::new(
                BulkCellEditsArgs { undo_redo_group: UndoRedoGroup::new(), undo_redo_source: Some(Rc::new(UndoRedoSource::new())), progress, token: CancellationToken::None, edits },
                editor_service,
                notebook_service.clone(),
            )
        })?;
        bce.apply().await?;

        let resolve_args = notebook_service.resolve_args.borrow();
        assert_eq!(resolve_args[0].to_string(false), resolve_uri.to_string(false));
        Ok::<(), Error>(())
    }
    .await;

    notebook_editor.editor.dispose();
    notebook_editor.view_model.dispose();
    notebook.dispose();
    disposables.dispose();
    outcome
}

fn notebook_uri() -> URI {
    URI::file("/foo/bar.ipynb", Platform::Posix)
}

#[test]
fn works_with_notebook_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let notebook_uri = notebook_uri();
        run_test(notebook_uri.clone(), notebook_uri).await.expect("o BulkCellEdits aplica");
    });
}

#[test]
fn maps_cell_uri_to_notebook_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let notebook_uri = notebook_uri();
        run_test(CellUri::generate(&notebook_uri, 5), notebook_uri).await.expect("o BulkCellEdits aplica");
    });
}

#[test]
fn throws_for_invalid_cell_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let notebook_uri = notebook_uri();
        let bad_cell_uri = CellUri::generate(&notebook_uri, 5).with(&UriChange { fragment: Field::Clear, ..UriChange::default() });
        assert!(run_test(bad_cell_uri, notebook_uri).await.is_err());
    });
}
