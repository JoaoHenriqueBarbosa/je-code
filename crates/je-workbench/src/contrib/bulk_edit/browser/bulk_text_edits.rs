//! Porte de `src/vs/workbench/contrib/bulkEdit/browser/bulkTextEdits.ts`: o `BulkTextEdits` e as tarefas de edição por
//! modelo (`ModelEditTask` e `EditorEditTask`).
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são o `Deps` do [`Injectable`], e os argumentos estáticos são o
//!   [`BulkTextEditsArgs`].
//! - **Herança.** `EditorEditTask extends ModelEditTask` é a composição: o [`EditorEditTask`] guarda o [`ModelEditTask`] e
//!   os dois implementam o trait [`EditTask`] (o `ModelEditTask[]` do original é a lista de `Rc<dyn EditTask>`). O estado
//!   mutável das tarefas (`_edits`, `_newEol`, `_expectedModelVersionId`) fica em `Cell` e `RefCell`.
//! - **`ISingleSnippetEditOperation`.** É o [`SnippetEditOperation`]: a [`SingleEditOperation`] com `insertAsSnippet` e
//!   `keepWhitespace`.
//! - **`ModelEditTask.addEdit`.** O `textEdit.range` do original pode faltar (`!textEdit.range && !textEdit.text`, e o
//!   `getFullModelRange()` quando falta só o intervalo); o [`TextEdit`] do editor sempre o traz, então esses dois ramos
//!   não existem aqui.
//! - **`ValidationResult`.** É `Result<(), URI>`: o `Err` é o `{ canApply: false, reason }`.
//! - **`IReference<IResolvedTextEditorModel>`.** É o [`TextModelReference`]; o `textEditorModel` dele, que o
//!   `IResolvedTextEditorModel` garante, ausente é erro.
//! - **`_edits` do `BulkTextEdits`.** O `ResourceMap<ResourceTextEdit[]>` é o [`ResourceMap`] de índices sobre a lista de
//!   pares (a iteração segue a ordem de inserção, como a do `Map`).
//! - **`this._editor?.getModel()?.uri.toString() === ...`.** É o [`same_model_uri`].
//! - **`computeMoreMinimalEdits(uri, edits, false)`.** O `pretty` não existe no contrato do porte (o `false` é o único
//!   valor usado aqui).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_base::map::ResourceMap;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceTextEdit;
use je_editor::code_editor_widget::{CodeEditorWidget, EndCursorState, ExecuteEditsSource, ICodeEditor};
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::edit_stack::{MultiModelEditStackElement, SingleModelEditStackElement};
use je_editor::editor_worker::{IEditorWorkerService};
use je_editor::model::{EndOfLineSequence, ValidEditOperation};
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::services_model::IModelService;
use je_editor::snippet::snippet_controller2::{PartialSnippetInsertOptions, SnippetController2};
use je_editor::snippet::snippet_parser::SnippetParser;
use je_editor::snippet::snippet_session::SnippetEdit;
use je_editor::text_model::TextModel;
use je_editor::text_model_edit_source::TextModelEditSource;
use je_editor::undo_redo::{IUndoRedoService, UndoRedoElementRef, UndoRedoGroup, UndoRedoSource};
use je_instantiation::Injectable;
use je_platform::progress::progress::IProgress;

use crate::services::textmodel_resolver::resolver_service::{ITextModelService, TextModelReference};

/// `ISingleSnippetEditOperation`: `ISingleEditOperation & { insertAsSnippet?: boolean; keepWhitespace?: boolean }`.
#[derive(Clone)]
struct SnippetEditOperation {
    operation: SingleEditOperation,
    insert_as_snippet: Option<bool>,
    keep_whitespace: Option<bool>,
}

/// `this._editor?.getModel()?.uri.toString() === model.uri.toString()`.
fn same_model_uri(editor: &CodeEditorWidget, model: &TextModel) -> bool {
    ICodeEditor::get_model(editor).is_some_and(|editor_model| editor_model.uri().to_string(false) == model.uri().to_string(false))
}

/// `_transformSnippetStringToInsertText(edit)`: transform a snippet edit (and only those) into a normal text edit. For that
/// we need to parse the snippet and get its actual text, e.g without placeholder or variable syntaxes.
fn transform_snippet_string_to_insert_text(edit: SnippetEditOperation) -> SnippetEditOperation {
    if !edit.insert_as_snippet.unwrap_or(false) {
        return edit;
    }
    let Some(text) = edit.operation.text.as_ref().filter(|text| !text.units().is_empty()) else {
        return edit;
    };
    let text = SnippetParser::as_insert_text(text.units());
    let mut operation = edit.operation.clone();
    operation.text = Some(text);
    SnippetEditOperation { operation, insert_as_snippet: Some(false), keep_whitespace: edit.keep_whitespace }
}

/// `.map(this._transformSnippetStringToInsertText, this).sort((a, b) => Range.compareRangesUsingStarts(a.range, b.range))`.
fn transform_and_sort(edits: Vec<SnippetEditOperation>) -> Vec<SnippetEditOperation> {
    let mut edits: Vec<SnippetEditOperation> = edits.into_iter().map(transform_snippet_string_to_insert_text).collect();
    edits.sort_by(|a, b| Range::compare_ranges_using_starts(Some(&a.operation.range), Some(&b.operation.range)).cmp(&0));
    edits
}

/// `ModelEditTask`.
pub struct ModelEditTask {
    model_reference: TextModelReference,
    pub model: Rc<TextModel>,
    expected_model_version_id: Cell<Option<i64>>,
    edits: RefCell<Vec<SnippetEditOperation>>,
    new_eol: Cell<Option<EndOfLineSequence>>,
}

impl ModelEditTask {
    pub fn new(model_reference: TextModelReference) -> Result<Self, Error> {
        let model = model_reference
            .object
            .text_editor_model()
            .ok_or_else(|| Error::new("O modelo da referência não foi resolvido"))?;
        Ok(Self { model_reference, model, expected_model_version_id: Cell::new(None), edits: RefCell::new(Vec::new()), new_eol: Cell::new(None) })
    }

    /// `isNoOp()`.
    pub fn is_no_op(&self) -> bool {
        if !self.edits.borrow().is_empty() {
            // contains textual edits
            return false;
        }
        if self.new_eol.get().is_some_and(|new_eol| new_eol != self.model.get_end_of_line_sequence()) {
            // contains an eol change that is a real change
            return false;
        }
        true
    }

    /// `addEdit(resourceEdit)`.
    pub fn add_edit(&self, resource_edit: &ResourceTextEdit) {
        self.expected_model_version_id.set(resource_edit.version_id);
        let text_edit = &resource_edit.text_edit;

        if let Some(eol) = text_edit.eol {
            // honor eol-change
            self.new_eol.set(Some(eol));
        }
        if text_edit.range.is_empty() && text_edit.text.is_empty() {
            // no-op edit (replace empty range with empty text)
            return;
        }

        // create edit operation
        self.edits.borrow_mut().push(SnippetEditOperation {
            operation: EditOperation::replace_move(text_edit.range, Some(JsString::from(text_edit.text.as_str()))),
            insert_as_snippet: text_edit.insert_as_snippet,
            keep_whitespace: text_edit.keep_whitespace,
        });
    }

    /// `validate()`.
    pub fn validate(&self) -> Result<(), URI> {
        match self.expected_model_version_id.get() {
            Some(expected) if self.model.get_version_id() != expected => Err(self.model.uri()),
            _ => Ok(()),
        }
    }

    /// `apply(reason?)`.
    pub fn apply(&self, reason: Option<TextModelEditSource>) {
        if !self.edits.borrow().is_empty() {
            // no editor -> no snippet mode
            let edits = transform_and_sort(self.edits.borrow().clone());
            *self.edits.borrow_mut() = edits.clone();
            let operations: Vec<SingleEditOperation> = edits.into_iter().map(|edit| edit.operation).collect();
            let mut cursor_state_computer = |_: &[ValidEditOperation]| -> Option<Vec<Selection>> { None };
            self.model.push_edit_operations(None, &operations, Some(&mut cursor_state_computer), None, reason);
        }
        if let Some(new_eol) = self.new_eol.get() {
            self.model.push_eol(new_eol);
        }
    }

    /// `dispose()`.
    pub fn dispose(&self) {
        self.model_reference.dispose();
    }
}

/// O que `BulkTextEdits` pede às tarefas: o `ModelEditTask` e o `EditorEditTask` (que o sobrescreve em dois métodos).
pub trait EditTask {
    /// O `ModelEditTask` que o `EditorEditTask` estende.
    fn base(&self) -> &ModelEditTask;

    /// `getBeforeCursorState()`.
    fn get_before_cursor_state(&self) -> Option<Vec<Selection>>;

    /// `apply(reason?)`.
    fn apply(&self, reason: Option<TextModelEditSource>);
}

impl EditTask for ModelEditTask {
    fn base(&self) -> &ModelEditTask {
        self
    }

    fn get_before_cursor_state(&self) -> Option<Vec<Selection>> {
        None
    }

    fn apply(&self, reason: Option<TextModelEditSource>) {
        ModelEditTask::apply(self, reason);
    }
}

/// `EditorEditTask`.
pub struct EditorEditTask {
    base: ModelEditTask,
    editor: Rc<CodeEditorWidget>,
}

impl EditorEditTask {
    pub fn new(model_reference: TextModelReference, editor: Rc<CodeEditorWidget>) -> Result<Self, Error> {
        Ok(Self { base: ModelEditTask::new(model_reference)?, editor })
    }

    /// `_canUseEditor()`.
    fn can_use_editor(&self) -> bool {
        same_model_uri(&self.editor, &self.base.model)
    }
}

impl EditTask for EditorEditTask {
    fn base(&self) -> &ModelEditTask {
        &self.base
    }

    fn get_before_cursor_state(&self) -> Option<Vec<Selection>> {
        if self.can_use_editor() { ICodeEditor::get_selections(&*self.editor) } else { None }
    }

    fn apply(&self, reason: Option<TextModelEditSource>) {
        // Check that the editor is still for the wanted model. It might have changed in the
        // meantime and that means we cannot use the editor anymore (instead we perform the edit through the model)
        if !self.can_use_editor() {
            self.base.apply(None);
            return;
        }

        if !self.base.edits.borrow().is_empty() {
            let snippet_ctrl = SnippetController2::get(&self.editor);
            let edits = self.base.edits.borrow().clone();
            match snippet_ctrl.filter(|_| edits.iter().any(|edit| edit.insert_as_snippet.unwrap_or(false))) {
                Some(snippet_ctrl) => {
                    // some edit is a snippet edit -> use snippet controller and ISnippetEdits
                    let snippet_edits: Vec<SnippetEdit> = edits
                        .iter()
                        .filter_map(|edit| {
                            edit.operation.text.as_ref().map(|text| SnippetEdit {
                                range: edit.operation.range,
                                template: if edit.insert_as_snippet.unwrap_or(false) { text.clone() } else { SnippetParser::escape(text.units()) },
                                keep_whitespace: edit.keep_whitespace,
                            })
                        })
                        .collect();
                    snippet_ctrl.apply(
                        snippet_edits,
                        Some(PartialSnippetInsertOptions { undo_stop_before: Some(false), undo_stop_after: Some(false), ..PartialSnippetInsertOptions::default() }),
                    );
                }
                None => {
                    // normal edit: mixed edits (snippet and normal) -> no snippet mode
                    let edits = transform_and_sort(edits);
                    *self.base.edits.borrow_mut() = edits.clone();
                    let operations: Vec<SingleEditOperation> = edits.into_iter().map(|edit| edit.operation).collect();
                    let source = match reason {
                        Some(reason) => ExecuteEditsSource::Reason(reason),
                        None => ExecuteEditsSource::Name(None),
                    };
                    ICodeEditor::execute_edits(&*self.editor, source, operations, None::<EndCursorState<'_>>);
                }
            }
        }
        if let Some(new_eol) = self.base.new_eol.get()
            && ICodeEditor::has_model(&*self.editor)
            && let Some(model) = ICodeEditor::get_model(&*self.editor)
        {
            model.push_eol(new_eol);
        }
    }
}

/// Os argumentos estáticos do construtor de [`BulkTextEdits`], na ordem do original.
pub struct BulkTextEditsArgs {
    pub label: String,
    pub code: String,
    pub editor: Option<Rc<CodeEditorWidget>>,
    pub undo_redo_group: UndoRedoGroup,
    pub undo_redo_source: Option<Rc<UndoRedoSource>>,
    pub progress: Rc<dyn IProgress<()>>,
    pub token: CancellationToken,
    pub edits: Vec<ResourceTextEdit>,
}

/// `BulkTextEdits`.
pub struct BulkTextEdits {
    label: String,
    code: String,
    editor: Option<Rc<CodeEditorWidget>>,
    undo_redo_group: UndoRedoGroup,
    undo_redo_source: Option<Rc<UndoRedoSource>>,
    progress: Rc<dyn IProgress<()>>,
    token: CancellationToken,
    /// `_edits`: os pares (recurso, edições) na ordem de inserção.
    edits: Vec<(URI, Vec<ResourceTextEdit>)>,
    editor_worker: Rc<dyn IEditorWorkerService>,
    model_service: Rc<dyn IModelService>,
    text_model_resolver_service: Rc<dyn ITextModelService>,
    undo_redo_service: Rc<dyn IUndoRedoService>,
}

impl Injectable for BulkTextEdits {
    type Args = BulkTextEditsArgs;
    type Deps = (Rc<dyn IEditorWorkerService>, Rc<dyn IModelService>, Rc<dyn ITextModelService>, Rc<dyn IUndoRedoService>);

    fn create(args: Self::Args, (editor_worker, model_service, text_model_resolver_service, undo_redo_service): Self::Deps) -> Rc<Self> {
        let mut index: ResourceMap<usize> = ResourceMap::new();
        let mut edits: Vec<(URI, Vec<ResourceTextEdit>)> = Vec::new();
        for edit in args.edits {
            match index.get(&edit.resource).copied() {
                Some(position) => edits[position].1.push(edit),
                None => {
                    index.set(edit.resource.clone(), edits.len());
                    edits.push((edit.resource.clone(), vec![edit]));
                }
            }
        }

        Rc::new(Self {
            label: args.label,
            code: args.code,
            editor: args.editor,
            undo_redo_group: args.undo_redo_group,
            undo_redo_source: args.undo_redo_source,
            progress: args.progress,
            token: args.token,
            edits,
            editor_worker,
            model_service,
            text_model_resolver_service,
            undo_redo_service,
        })
    }
}

impl BulkTextEdits {
    /// `_validateBeforePrepare()`: first check if loaded models were not changed in the meantime.
    fn validate_before_prepare(&self) -> Result<(), Error> {
        for (_, array) in &self.edits {
            for edit in array {
                if let Some(version_id) = edit.version_id
                    && let Some(model) = self.model_service.get_model(&edit.resource)
                    && model.get_version_id() != version_id
                {
                    // model changed in the meantime
                    return Err(Error::new(format!("{} has changed in the meantime", model.uri().to_string(false))));
                }
            }
        }
        Ok(())
    }

    /// Uma tarefa de `_createEditsTasks`: o corpo do `then(async ref => ...)` de cada recurso.
    fn create_edit_task(&self, key: URI, edits: Vec<ResourceTextEdit>, tasks: Rc<RefCell<Vec<Rc<dyn EditTask>>>>) -> Promise<()> {
        let (text_model_resolver_service, editor_worker, editor) = (self.text_model_resolver_service.clone(), self.editor_worker.clone(), self.editor.clone());
        async_fn(async move {
            let reference = text_model_resolver_service.create_model_reference(&key).await?;
            let model = reference
                .object
                .text_editor_model()
                .ok_or_else(|| Error::new("O modelo da referência não foi resolvido"))?;
            let editor = editor.filter(|editor| same_model_uri(editor, &model));
            let task: Rc<dyn EditTask> = match &editor {
                Some(editor) => Rc::new(EditorEditTask::new(reference, editor.clone())?),
                None => Rc::new(ModelEditTask::new(reference)?),
            };
            tasks.borrow_mut().push(task.clone());

            if editor.is_none() {
                for edit in &edits {
                    task.base().add_edit(edit);
                }
                return Ok(());
            }

            // group edits by type (snippet, metadata, or simple) and make simple groups more minimal

            let uri = model.uri();
            let mut start = 0;
            let mut i = 0;
            while i < edits.len() {
                if edits[i].text_edit.insert_as_snippet.unwrap_or(false) || edits[i].metadata.is_some() {
                    Self::make_group_more_minimal(&editor_worker, task.base(), &uri, &edits[start..i]).await?; // grouped edits until now
                    task.base().add_edit(&edits[i]); // this edit
                    start = i + 1;
                }
                i += 1;
            }
            Self::make_group_more_minimal(&editor_worker, task.base(), &uri, &edits[start..i]).await?;
            Ok(())
        })
    }

    /// `makeGroupMoreMinimal(start, end)`.
    async fn make_group_more_minimal(editor_worker: &Rc<dyn IEditorWorkerService>, task: &ModelEditTask, uri: &URI, old_edits: &[ResourceTextEdit]) -> Result<(), Error> {
        let new_edits = editor_worker.compute_more_minimal_edits(uri, Some(old_edits.iter().map(|edit| edit.text_edit.clone()).collect())).await?;
        match new_edits {
            None => old_edits.iter().for_each(|edit| task.add_edit(edit)),
            Some(new_edits) => {
                // All edits in the group have the same version id since we group the edits
                // in the constructor by the resource URI.
                let version_id = old_edits.first().and_then(|edit| edit.version_id);
                for edit in new_edits {
                    task.add_edit(&ResourceTextEdit::new(task.model.uri(), edit, version_id, None));
                }
            }
        }
        Ok(())
    }

    /// `_createEditsTasks()`.
    fn create_edits_tasks(&self) -> Promise<Vec<Rc<dyn EditTask>>> {
        let tasks: Rc<RefCell<Vec<Rc<dyn EditTask>>>> = Rc::default();
        let promises: Vec<Promise<()>> = self.edits.iter().map(|(key, edits)| self.create_edit_task(key.clone(), edits.clone(), tasks.clone())).collect();
        async_fn(async move {
            Promise::all(promises).await?;
            let tasks = tasks.borrow().clone();
            Ok(tasks)
        })
    }

    /// `_validateTasks(tasks)`.
    fn validate_tasks(tasks: &[Rc<dyn EditTask>]) -> Result<(), URI> {
        tasks.iter().try_for_each(|task| task.base().validate())
    }

    /// O corpo do `try` de `apply(reason?)`.
    fn apply_tasks(&self, tasks: &[Rc<dyn EditTask>], reason: Option<TextModelEditSource>) -> Result<Vec<URI>, Error> {
        if self.token.is_cancellation_requested() {
            return Ok(Vec::new());
        }

        let mut resources: Vec<URI> = Vec::new();
        if let Err(reason_uri) = Self::validate_tasks(tasks) {
            return Err(Error::new(format!("{} has changed in the meantime", reason_uri.to_string(false))));
        }
        let source = self.undo_redo_source.as_deref();
        if tasks.len() == 1 {
            // This edit touches a single model => keep things simple
            let task = &tasks[0];
            if !task.base().is_no_op() {
                let single_model_edit_stack_element = Rc::new(SingleModelEditStackElement::new(&self.label, &self.code, &task.base().model, task.get_before_cursor_state()));
                self.undo_redo_service.push_element(UndoRedoElementRef::Resource(single_model_edit_stack_element.clone()), Some(&self.undo_redo_group), source);
                task.apply(reason);
                single_model_edit_stack_element.close();
                resources.push(task.base().model.uri());
            }
            self.progress.report(());
        } else {
            // prepare multi model undo element
            let multi_model_edit_stack_element = Rc::new(MultiModelEditStackElement::new(
                &self.label,
                &self.code,
                tasks.iter().map(|task| Rc::new(SingleModelEditStackElement::new(&self.label, &self.code, &task.base().model, task.get_before_cursor_state()))).collect(),
            ));
            self.undo_redo_service.push_element(UndoRedoElementRef::Workspace(multi_model_edit_stack_element.clone()), Some(&self.undo_redo_group), source);
            for task in tasks {
                task.apply(None);
                self.progress.report(());
                resources.push(task.base().model.uri());
            }
            multi_model_edit_stack_element.close();
        }

        Ok(resources)
    }

    /// `apply(reason?)`: `this` é o `Rc` do objeto criado por `createInstance`.
    pub fn apply(self: &Rc<Self>, reason: Option<TextModelEditSource>) -> Promise<Vec<URI>> {
        let this = self.clone();
        async_fn(async move {
            this.validate_before_prepare()?;
            let tasks = this.create_edits_tasks().await?;

            let result = this.apply_tasks(&tasks, reason);
            for task in &tasks {
                task.base().dispose();
            }
            result
        })
    }
}
