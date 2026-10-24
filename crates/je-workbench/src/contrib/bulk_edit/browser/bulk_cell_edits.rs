//! Porte de `src/vs/workbench/contrib/bulkEdit/browser/bulkCellEdits.ts`: o `ResourceNotebookCellEdit` e o `BulkCellEdits`, que
//! aplica as edições de célula de notebook de um `WorkspaceEdit` pelo `NotebookTextModel.applyEdits`.
//!
//! Divergências:
//! - **`ResourceNotebookCellEdit extends ResourceEdit implements IWorkspaceNotebookCellEdit`.** É a struct com os campos da
//!   interface (e o `metadata` herdado de `ResourceEdit`). Como o `WorkspaceEdit` do editor só tem os itens de texto, de arquivo
//!   e o [`ICustomEdit`], ela viaja como [`ICustomEdit`] (a variante `Custom` de `ResourceEdit`) e o `BulkEdit` a reconhece por
//!   tipo (`instanceof`) pelo [`ICustomEdit::as_any`]. O `undo` e o `redo` do contrato existem só para satisfazê-lo e rejeitam:
//!   a edição não é opaca, quem a aplica e a desfaz é o [`BulkCellEdits`] (com o desfazer do `NotebookTextModel`).
//! - **`ResourceNotebookCellEdit.is(candidate)`.** O `candidate instanceof ResourceNotebookCellEdit` é [`ResourceNotebookCellEdit::is`]
//!   (o `ICustomEdit` que é uma [`ResourceNotebookCellEdit`]); o ramo estrutural (`isUri(resource) && isObject(cellEdit)`) não existe,
//!   porque o tipo `IWorkspaceNotebookCellEdit` já garante os dois campos. `lift` é [`ResourceNotebookCellEdit::lift`].
//! - **Injeção.** Os parâmetros decorados do construtor (`@IEditorService`, `@INotebookEditorModelResolverService`) são o `Deps`
//!   do [`Injectable`], e o construtor direto (que o teste do original chama) é [`BulkCellEdits::new`]. O `throw new Error(Invalid
//!   notebook URI: ...)` do construtor é [`errors::throw`] (um pânico com o [`Error`]).
//! - **`groupBy`.** É o `group_by` do `je-base`, com o `compare` de textos sobre o `toString()` do recurso.
//! - **`applyEdits`.** O `cellEdit` vira o [`ICellEditOperation`] de mesma forma; o `end_selections_computer` é `() => undefined`;
//!   `isReadonly()` é o [`Readonly::is_readonly`](crate::services::files_configuration::files_configuration_service::Readonly::is_readonly).
//! - **`getNotebookEditorFromEditorPane(activeEditorPane)`** é o de `notebook_browser` (o painel responde pelo editor).
//! - `undoRedoSource` é guardado e não lido (o original também não o lê).

use je_base::lifecycle::IDisposable;
use std::any::Any;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::promise::Promise;
use je_base::strings::compare;
use je_base::uri::URI;
use je_editor::code_action::languages::{ICustomEdit, WorkspaceEditMetadata};
use je_editor::undo_redo::{UndoRedoGroup, UndoRedoSource};
use je_instantiation::Injectable;
use je_platform::progress::progress::IProgress;

use crate::contrib::notebook::browser::notebook_browser::get_notebook_editor_from_editor_pane;
use crate::contrib::notebook::common::notebook_common::{
    CellUri, ICellEditOperation, ISelectionIndexState, ISelectionState, IWorkspaceNotebookCellEdit, WorkspaceNotebookCellEditOperation,
};
use crate::contrib::notebook::common::notebook_editor_model_resolver_service::INotebookEditorModelResolverService;
use crate::services::editor::editor_service::IEditorService;

/// `ResourceNotebookCellEdit`.
#[derive(Clone)]
pub struct ResourceNotebookCellEdit {
    pub resource: URI,
    pub cell_edit: WorkspaceNotebookCellEditOperation,
    pub notebook_version_id: Option<i64>,
    /// `ResourceEdit.metadata`.
    pub metadata: Option<WorkspaceEditMetadata>,
}

impl ResourceNotebookCellEdit {
    /// `constructor(resource, cellEdit, notebookVersionId = undefined, metadata?)`.
    pub fn new(resource: URI, cell_edit: WorkspaceNotebookCellEditOperation, notebook_version_id: Option<i64>, metadata: Option<WorkspaceEditMetadata>) -> Self {
        Self { resource, cell_edit, notebook_version_id, metadata }
    }

    /// `ResourceNotebookCellEdit.is(candidate)`.
    pub fn is(candidate: &dyn ICustomEdit) -> bool {
        candidate.as_any().is::<ResourceNotebookCellEdit>()
    }

    /// `ResourceNotebookCellEdit.lift(edit)`.
    pub fn lift(edit: &IWorkspaceNotebookCellEdit) -> Self {
        Self::new(edit.resource.clone(), edit.cell_edit.clone(), edit.notebook_version_id, edit.metadata.clone())
    }

    /// O `instanceof ResourceNotebookCellEdit` com o valor: a edição, se o [`ICustomEdit`] for uma.
    pub fn downcast(candidate: &dyn ICustomEdit) -> Option<&ResourceNotebookCellEdit> {
        candidate.as_any().downcast_ref::<ResourceNotebookCellEdit>()
    }
}

impl ICustomEdit for ResourceNotebookCellEdit {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn metadata(&self) -> Option<WorkspaceEditMetadata> {
        self.metadata.clone()
    }

    fn undo(&self) -> Promise<()> {
        Promise::rejected(Error::new("A edição de célula de notebook não é opaca: o BulkCellEdits a aplica e a desfaz"))
    }

    fn redo(&self) -> Promise<()> {
        Promise::rejected(Error::new("A edição de célula de notebook não é opaca: o BulkCellEdits a aplica e a desfaz"))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Os argumentos estáticos do construtor de [`BulkCellEdits`], na ordem do original.
pub struct BulkCellEditsArgs {
    pub undo_redo_group: UndoRedoGroup,
    pub undo_redo_source: Option<Rc<UndoRedoSource>>,
    pub progress: Rc<dyn IProgress<()>>,
    pub token: CancellationToken,
    pub edits: Vec<ResourceNotebookCellEdit>,
}

/// `BulkCellEdits`.
pub struct BulkCellEdits {
    undo_redo_group: UndoRedoGroup,
    #[allow(dead_code)]
    undo_redo_source: Option<Rc<UndoRedoSource>>,
    progress: Rc<dyn IProgress<()>>,
    token: CancellationToken,
    edits: Vec<ResourceNotebookCellEdit>,
    editor_service: Rc<dyn IEditorService>,
    notebook_model_service: Rc<dyn INotebookEditorModelResolverService>,
}

impl Injectable for BulkCellEdits {
    type Args = BulkCellEditsArgs;
    type Deps = (Rc<dyn IEditorService>, Rc<dyn INotebookEditorModelResolverService>);

    fn create(args: Self::Args, (editor_service, notebook_model_service): Self::Deps) -> Rc<Self> {
        Self::new(args, editor_service, notebook_model_service)
    }
}

impl BulkCellEdits {
    /// O construtor: troca a URI de célula pela do notebook (`CellUri.parse(...).notebook`).
    pub fn new(args: BulkCellEditsArgs, editor_service: Rc<dyn IEditorService>, notebook_model_service: Rc<dyn INotebookEditorModelResolverService>) -> Rc<Self> {
        let edits = args
            .edits
            .into_iter()
            .map(|edit| {
                if edit.resource.scheme() == CellUri::SCHEME {
                    let Some(parsed) = CellUri::parse(&edit.resource) else {
                        errors::throw(Error::new(format!("Invalid notebook URI: {}", edit.resource.to_string(false).to_string_lossy())));
                    };

                    ResourceNotebookCellEdit::new(parsed.notebook, edit.cell_edit, edit.notebook_version_id, edit.metadata)
                } else {
                    edit
                }
            })
            .collect();

        Rc::new(Self {
            undo_redo_group: args.undo_redo_group,
            undo_redo_source: args.undo_redo_source,
            progress: args.progress,
            token: args.token,
            edits,
            editor_service,
            notebook_model_service,
        })
    }

    /// `apply()`: `this` é o `Rc` do objeto criado por `createInstance`.
    pub fn apply(self: &Rc<Self>) -> Promise<Vec<URI>> {
        let this = self.clone();
        async_fn(async move {
            let mut resources: Vec<URI> = Vec::new();
            let edits_by_notebook = je_base::arrays::group_by(&this.edits, |a, b| {
                f64::from(compare(a.resource.to_string(false).units(), b.resource.to_string(false).units()))
            });

            for group in edits_by_notebook {
                if this.token.is_cancellation_requested() {
                    break;
                }
                let first = &group[0];
                let reference = this.notebook_model_service.resolve(&first.resource, None, None).await?;
                let notebook = reference.object.resolved_notebook();

                // check state
                if let Some(notebook_version_id) = first.notebook_version_id
                    && notebook.version_id() != notebook_version_id
                {
                    reference.dispose();
                    return Err(Error::new(format!("Notebook '{}' has changed in the meantime", first.resource.to_string(false).to_string_lossy())));
                }

                // apply edits
                let edits: Vec<ICellEditOperation> = group
                    .iter()
                    .map(|entry| match &entry.cell_edit {
                        WorkspaceNotebookCellEditOperation::PartialMetadata(edit) => ICellEditOperation::PartialMetadata(edit.clone()),
                        WorkspaceNotebookCellEditOperation::DocumentMetadata(edit) => ICellEditOperation::DocumentMetadata(edit.clone()),
                        WorkspaceNotebookCellEditOperation::Replace(edit) => ICellEditOperation::Replace(edit.clone()),
                    })
                    .collect();
                let compute_undo = !reference.object.is_readonly().is_readonly();
                let editor = get_notebook_editor_from_editor_pane(this.editor_service.active_editor_pane().as_ref());
                let initial_selection_state: Option<ISelectionState> = editor.as_ref().and_then(|editor| {
                    let same_notebook = editor.text_model().is_some_and(|text_model| text_model.uri().to_string(false) == notebook.uri().to_string(false));
                    same_notebook.then(|| ISelectionState::Index(ISelectionIndexState { focus: editor.get_focus(), selections: editor.get_selections() }))
                });
                notebook.apply_edits(&edits, true, initial_selection_state, &|| None, Some(&this.undo_redo_group), compute_undo);
                reference.dispose();

                this.progress.report(());

                resources.push(first.resource.clone());
            }

            Ok(resources)
        })
    }
}
