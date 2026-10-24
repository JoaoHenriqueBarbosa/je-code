//! Porte de `src/vs/workbench/contrib/notebook/common/model/notebookTextModel.ts`: o modelo de texto do notebook
//! (`NotebookTextModel`), com as células, as edições em lote (`applyEdits`), o desfazer e refazer das edições, os
//! eventos de mudança de conteúdo, os identificadores de versão, o snapshot e a busca em células.
//!
//! Divergências:
//! - **Injeção de dependências**: não há `@IUndoRedoService` e companhia; o construtor recebe os serviços. O
//!   `IUndoRedoService` é o [`UndoRedoService`] concreto do je_editor, o `IModelService` é o [`ModelService`] (o
//!   `onModelAdded` não está no recorte `IModelService`), e os demais são os traits do porte.
//! - **`Disposable`** é o campo `base`; o modelo nasce como `Rc<Self>` (`Rc::new_cyclic`) e os fechos dos eventos e dos
//!   elementos de desfazer guardam um `Weak` dele (uma `NotebookCellTextModel` num fecho do próprio evento também é
//!   `Weak`), para o undo service e os emitters não prenderem o modelo.
//! - **`StackOperation`** é um `IWorkspaceUndoRedoElement` no original (vários recursos numa operação). O
//!   `UndoRedoService` do porte ainda não tem o elemento de workspace, e o `StackOperation` tem um recurso só (a URI do
//!   notebook), então é um [`UndoRedoElement`] de recurso; empilhar, desfazer, refazer e a verificação do `tag`
//!   (`getLastElement(...).tag === 'notebookUndoRedoElement'`, aqui um `downcast_ref`) são os mesmos. O estado é
//!   compartilhado (`Rc<RefCell<...>>`), porque `appendPreviousOperation` reabre a operação que já está na pilha.
//! - **`UndoRedoGroup`** passou a ser `Clone` (o contador de ordem é compartilhado), porque o elemento guarda o grupo
//!   que `applyEdits` recebeu e o empilha depois, quando a referência do chamador não existe mais.
//! - **`undo` e `redo`** são síncronos (o `await` das operações do `StackOperation` não espera nada).
//! - **`try`/`finally`** com exceções são pânicos: o `pause`/`resume` do `PauseableEmitter` é um guarda com `Drop`, e o
//!   `catch (err) { ...; throw err }` de `applyEdits` é `errors::catch` seguido de `errors::throw`.
//! - **`NotebookEventEmitter`** (`extends PauseableEmitter`) usa dois acessores que o `PauseableEmitter` do je_base
//!   ganhou para a `_eventQueue` (`protected` no original): `is_event_queue_empty` e `for_each_queued_event`.
//! - **Índices**: os índices de edição e de evento são `usize`; os que podem valer `-1` no original (`cellIndex` das
//!   edições, o resultado de `findIndex`) são `i64` internamente. Uma célula ausente num evento (índice `-1` no
//!   original, só alcançável com a célula já removida) vira `usize::MAX`. Um índice de edição fora do intervalo
//!   (`Number.MAX_VALUE` ou `-1` no original) é `usize::MAX`, que cai no mesmo `_assertIndex` e lança o mesmo erro.
//! - **`Array#sort` de `_doApplyEdits`**: o comparador do original não é uma ordem total (um `end` indefinido devolve
//!   `-1` mesmo contra outro indefinido), e o `sort_by` do Rust pode entrar em pânico com isso; a ordenação é o
//!   `sort_by_compare` do `je_base` (o TimSort do V8, com a mesma sequência de comparações).
//! - **`cellDto.outputs ||= []`** de `_replaceCells` não existe: `outputs` é um `Vec`, nunca ausente.
//! - **Buffer de texto da célula** em `_replaceCells`: o `TextModel` do porte guarda o buffer sem `Rc`, então
//!   `cell.resetTextBuffer(cell.textModel.getTextBuffer())` recebe um buffer novo com o valor do modelo (registrado no
//!   modelo do notebook). O caminho só roda quando já existe um `TextModel` para a URI de uma célula nova.
//! - **`Object.freeze`** (em `_changeCellMetadata`) congela o objeto no lugar, como o original, e o compartilha.
//! - **Resultados da busca**: `{ cell, match }` e `{ cell, matches }` são [`NotebookCellMatch`] e
//!   [`NotebookCellMatches`]; `{ cellIndex, position }` é [`NotebookCellPosition`].
//! - **`ILanguageDetectionService`** é obrigatório aqui (o original o injeta sempre) e a célula o recebe como `Some`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::arrays::sort_by_compare;
use je_base::JsString;
use je_base::diff::{ISequence, LcsDiff, SequenceElements};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, EmitterOptions, Event, PauseableEmitter};
use je_base::hash::hash;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, dispose_all};
use je_base::network::schemas;
use je_base::objects;
use je_base::uri::URI;
use je_editor::language_service::ILanguageService;
use je_editor::model::FindMatch;
use je_editor::model_service::ModelService;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextSource, create_text_buffer};
use je_editor::text_model_search::SearchParams;
use je_editor::undo_redo::{UndoRedoElement, UndoRedoGroup};
use je_editor::undo_redo_service::UndoRedoService;
use je_platform::resources::ext_uri;

use super::cell_edit::{CellMetadataEdit, ITextCellEditingDelegate, MoveCellEdit, SpliceCellsEdit, SpliceCellsDiff};
use super::notebook_cell_output_text_model::NotebookCellOutputTextModel;
use super::notebook_cell_text_model::{CellContentChange, NotebookCellTextModel};
use crate::contrib::notebook::common::notebook_common::{
    CellEditType, CellUri, ICell, ICellDto2, ICellEditOperation, ICellOutput, INotebookSnapshotOptions, INotebookTextModel, IOutputDto,
    IOutputItemDto, ISelectionState, NotebookCellContentChangeEvent, NotebookCellDefaultCollapseConfig, NotebookCellExecutionState,
    NotebookCellInternalMetadata, NotebookCellMetadata, NotebookCellOutputsSplice, NotebookCellTextModelSplice, NotebookCellsChangeInternalMetadataEvent,
    NotebookCellsChangeLanguageEvent, NotebookCellsChangeMetadataEvent, NotebookCellsChangeMimeEvent, NotebookCellsModelChangedEvent,
    NotebookCellsModelMoveEvent, NotebookData, NotebookDocumentChangeMetadataEvent, NotebookDocumentMetadata, NotebookDocumentUnknownChangeEvent,
    NotebookOutputChangedEvent, NotebookOutputItemChangedEvent, NotebookRawContentChange, NotebookRawContentEvent, NotebookTextModelChangedEvent,
    NotebookTextModelWillAddRemoveEvent, NullablePartialNotebookCellInternalMetadata, NullablePartialNotebookCellMetadata, TransientOptions,
    diff as diff_lists,
};
use crate::contrib::notebook::common::notebook_execution_state_service::INotebookExecutionStateService;
use crate::contrib::notebook::common::notebook_kernel_service::INotebookTextModelLike;
use crate::contrib::notebook::common::notebook_logging_service::INotebookLoggingService;
use crate::services::language_detection::language_detection_worker_service::ILanguageDetectionService;
use crate::services::working_copy::file_working_copy::SnapshotContext;

type CellRc = Rc<NotebookCellTextModel>;

/// `[start, deleteCount, items]` com as células concretas.
type CellSplice = (usize, usize, Vec<CellRc>);

/// Uma célula comparada por identidade (o `===` do original), para o `diff` de listas de células.
#[derive(Clone)]
struct CellRef(CellRc);

impl PartialEq for CellRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// `{ cellIndex: number; position: Position }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotebookCellPosition {
    pub cell_index: usize,
    pub position: Position,
}

/// `{ cell: NotebookCellTextModel; match: FindMatch }`.
#[derive(Clone)]
pub struct NotebookCellMatch {
    pub cell: Rc<NotebookCellTextModel>,
    pub match_: FindMatch,
}

/// `{ cell: NotebookCellTextModel; matches: FindMatch[] }`.
#[derive(Clone)]
pub struct NotebookCellMatches {
    pub cell: Rc<NotebookCellTextModel>,
    pub matches: Vec<FindMatch>,
}

/// `-1` no original; aqui o índice de edição `usize` vai a `i64` sem estourar (`usize::MAX` é inválido nos dois).
fn to_i64(index: usize) -> i64 {
    i64::try_from(index).unwrap_or(i64::MAX)
}

fn to_icells(cells: &[CellRc]) -> Vec<Rc<dyn ICell>> {
    cells.iter().map(|cell| -> Rc<dyn ICell> { cell.clone() }).collect()
}

fn to_icell_splices(splices: &[CellSplice]) -> Vec<NotebookCellTextModelSplice<Rc<dyn ICell>>> {
    splices.iter().map(|(start, delete_count, cells)| (*start, *delete_count, to_icells(cells))).collect()
}

/// `{}`.
fn empty_object() -> Dynamic {
    Dynamic::Object(ObjectRef::new())
}

/// `Object.freeze(value)`: congela no lugar e devolve o mesmo valor.
fn freeze(value: &Dynamic) -> Dynamic {
    match value {
        Dynamic::Object(object) => object.freeze(),
        Dynamic::Array(array) => array.freeze(),
        _ => {}
    }
    value.clone()
}

/// `new NotebookCellOutputTextModel(op)`.
fn new_output(op: &IOutputDto) -> Rc<dyn ICellOutput> {
    NotebookCellOutputTextModel::new(op.clone())
}

/// `hasKey(edit, { index: true })`.
fn edit_index(edit: &ICellEditOperation) -> Option<usize> {
    match edit {
        ICellEditOperation::Replace(edit) => Some(edit.index),
        ICellEditOperation::Output(edit) => Some(edit.index),
        ICellEditOperation::Metadata(edit) => Some(edit.index),
        ICellEditOperation::PartialMetadata(edit) => Some(edit.index),
        ICellEditOperation::PartialInternalMetadata(edit) => Some(edit.index),
        ICellEditOperation::CellLanguage(edit) => Some(edit.index),
        ICellEditOperation::Move(edit) => Some(edit.index),
        _ => None,
    }
}

/// `hasKey(edit, { handle: true })`.
fn edit_handle(edit: &ICellEditOperation) -> Option<i64> {
    match edit {
        ICellEditOperation::OutputByHandle(edit) => Some(edit.handle),
        ICellEditOperation::PartialMetadataByHandle(edit) => Some(edit.handle),
        ICellEditOperation::PartialInternalMetadataByHandle(edit) => Some(edit.handle),
        _ => None,
    }
}

/// `hasKey(edit, { outputId: true })`.
fn edit_output_id(edit: &ICellEditOperation) -> Option<&str> {
    match edit {
        ICellEditOperation::OutputItems(edit) => Some(edit.output_id.as_str()),
        _ => None,
    }
}

/// `hasKey(edit, { outputs: true })` e o `append`: as edições de saída por índice e por handle.
fn output_edit(edit: &ICellEditOperation) -> Option<(&Vec<IOutputDto>, Option<bool>)> {
    match edit {
        ICellEditOperation::Output(edit) => Some((&edit.outputs, edit.append)),
        ICellEditOperation::OutputByHandle(edit) => Some((&edit.outputs, edit.append)),
        _ => None,
    }
}

fn output_edit_mut(edit: &mut ICellEditOperation) -> Option<(&mut Vec<IOutputDto>, &mut Option<bool>)> {
    match edit {
        ICellEditOperation::Output(edit) => Some((&mut edit.outputs, &mut edit.append)),
        ICellEditOperation::OutputByHandle(edit) => Some((&mut edit.outputs, &mut edit.append)),
        _ => None,
    }
}

/// O elemento de desfazer de recurso que o original escreve como classe anônima (`new class implements
/// IResourceUndoRedoElement`) para a mudança de metadados do documento e a de linguagem da célula.
struct ResourceUndoRedoElement {
    resource: URI,
    label: &'static str,
    undo_fn: Box<dyn Fn()>,
    redo_fn: Box<dyn Fn()>,
}

impl UndoRedoElement for ResourceUndoRedoElement {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn label(&self) -> String {
        self.label.to_string()
    }

    fn code(&self) -> String {
        "undoredo.textBufferEdit".to_string()
    }

    fn undo(&self) {
        (self.undo_fn)();
    }

    fn redo(&self) {
        (self.redo_fn)();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `merge` do `NotebookEventEmitter`: junta os eventos enfileirados num só.
fn merge_change_events(events: Vec<NotebookTextModelChangedEvent>) -> NotebookTextModelChangedEvent {
    let mut events = events.into_iter();
    let mut merged = events.next().expect("a fusão recebe ao menos um evento");

    for event in events {
        merged.raw_events.extend(event.raw_events);
        merged.version_id = event.version_id;
        if event.end_selection_state.is_some() {
            merged.end_selection_state = event.end_selection_state;
        }
        if event.synchronous.is_some() {
            merged.synchronous = event.synchronous;
        }
    }

    merged
}

/// `NotebookEventEmitter extends PauseableEmitter<NotebookTextModelChangedEvent>`.
struct NotebookEventEmitter {
    inner: Rc<PauseableEmitter<NotebookTextModelChangedEvent>>,
}

impl NotebookEventEmitter {
    fn new() -> Rc<Self> {
        let merge: Rc<dyn Fn(Vec<NotebookTextModelChangedEvent>) -> NotebookTextModelChangedEvent> = Rc::new(merge_change_events);
        Rc::new(Self { inner: PauseableEmitter::with_options(EmitterOptions::default(), Some(merge)) })
    }

    fn is_empty(&self) -> bool {
        self.inner.is_event_queue_empty()
    }

    fn is_dirty_event(&self) -> bool {
        let mut dirty = false;
        self.inner.for_each_queued_event(|event| {
            if event.raw_events.iter().any(|raw_event| !raw_event.transient) {
                dirty = true;
            }
        });

        dirty
    }
}

impl Deref for NotebookEventEmitter {
    type Target = PauseableEmitter<NotebookTextModelChangedEvent>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl IDisposable for NotebookEventEmitter {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

/// O `pause()` seguido do `try { ... } finally { resume() }` do original: retoma o emitter ao sair, inclusive num pânico.
struct PauseGuard {
    emitter: Rc<NotebookEventEmitter>,
}

impl PauseGuard {
    fn new(emitter: &Rc<NotebookEventEmitter>) -> Self {
        emitter.pause();
        Self { emitter: emitter.clone() }
    }
}

impl Drop for PauseGuard {
    fn drop(&mut self) {
        self.emitter.resume();
    }
}

struct StackOperationState {
    operations: Vec<Rc<dyn UndoRedoElement>>,
    begin_selection_state: Option<ISelectionState>,
    result_selection_state: Option<ISelectionState>,
    begin_alternative_version_id: String,
    result_alternative_version_id: String,
}

/// `StackOperation`. Clonar compartilha o estado (ver as divergências).
#[derive(Clone)]
struct StackOperation {
    uri: URI,
    text_model: Weak<NotebookTextModel>,
    undo_redo_group: Option<UndoRedoGroup>,
    pauseable_emitter: Rc<NotebookEventEmitter>,
    post_undo_redo: Rc<dyn Fn(&str)>,
    state: Rc<RefCell<StackOperationState>>,
}

impl StackOperation {
    fn new(
        uri: URI,
        text_model: Weak<NotebookTextModel>,
        undo_redo_group: Option<UndoRedoGroup>,
        pauseable_emitter: Rc<NotebookEventEmitter>,
        post_undo_redo: Rc<dyn Fn(&str)>,
        selection_state: Option<ISelectionState>,
        begin_alternative_version_id: String,
    ) -> Self {
        Self {
            uri,
            text_model,
            undo_redo_group,
            pauseable_emitter,
            post_undo_redo,
            state: Rc::new(RefCell::new(StackOperationState {
                operations: Vec::new(),
                begin_selection_state: selection_state,
                result_selection_state: None,
                result_alternative_version_id: begin_alternative_version_id.clone(),
                begin_alternative_version_id,
            })),
        }
    }

    fn is_empty(&self) -> bool {
        self.state.borrow().operations.is_empty()
    }

    fn push_end_state(&self, alternative_version_id: &str, selection_state: Option<ISelectionState>) {
        // https://github.com/microsoft/vscode/issues/207523
        let mut state = self.state.borrow_mut();
        state.result_alternative_version_id = alternative_version_id.to_string();
        let previous_result_selection_state = state.result_selection_state.take();
        state.result_selection_state = selection_state.or(previous_result_selection_state);
    }

    fn push_edit_operation(
        &self,
        element: Rc<dyn UndoRedoElement>,
        begin_selection_state: Option<ISelectionState>,
        result_selection_state: Option<ISelectionState>,
        alternative_version_id: &str,
    ) {
        let mut state = self.state.borrow_mut();
        if state.operations.is_empty() {
            state.begin_selection_state = state.begin_selection_state.take().or(begin_selection_state);
        }
        state.operations.push(element);
        state.result_selection_state = result_selection_state;
        state.result_alternative_version_id = alternative_version_id.to_string();
    }

    fn version_id(&self) -> i64 {
        self.text_model.upgrade().map_or(0, |text_model| text_model.version_id())
    }
}

impl UndoRedoElement for StackOperation {
    fn resource(&self) -> URI {
        self.uri.clone()
    }

    fn label(&self) -> String {
        let state = self.state.borrow();
        if state.operations.len() == 1 { state.operations[0].label() } else { "edit".to_string() }
    }

    fn code(&self) -> String {
        let state = self.state.borrow();
        if state.operations.len() == 1 { state.operations[0].code() } else { "undoredo.notebooks.stackOperation".to_string() }
    }

    fn undo(&self) {
        let _resume = PauseGuard::new(&self.pauseable_emitter);
        let operations = self.state.borrow().operations.clone();
        for operation in operations.iter().rev() {
            operation.undo();
        }
        let (alternative_version_id, end_selection_state) = {
            let state = self.state.borrow();
            (state.begin_alternative_version_id.clone(), state.begin_selection_state.clone())
        };
        (self.post_undo_redo)(&alternative_version_id);
        self.pauseable_emitter.fire(NotebookTextModelChangedEvent {
            raw_events: Vec::new(),
            synchronous: None,
            version_id: self.version_id(),
            end_selection_state,
        });
    }

    fn redo(&self) {
        let _resume = PauseGuard::new(&self.pauseable_emitter);
        let operations = self.state.borrow().operations.clone();
        for operation in operations.iter() {
            operation.redo();
        }
        let (alternative_version_id, end_selection_state) = {
            let state = self.state.borrow();
            (state.result_alternative_version_id.clone(), state.result_selection_state.clone())
        };
        (self.post_undo_redo)(&alternative_version_id);
        self.pauseable_emitter.fire(NotebookTextModelChangedEvent {
            raw_events: Vec::new(),
            synchronous: None,
            version_id: self.version_id(),
            end_selection_state,
        });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct NotebookOperationManager {
    uri: URI,
    text_model: Weak<NotebookTextModel>,
    undo_service: Rc<UndoRedoService>,
    pauseable_emitter: Rc<NotebookEventEmitter>,
    post_undo_redo: Rc<dyn Fn(&str)>,
    pending_stack_operation: RefCell<Option<StackOperation>>,
    is_appending: Cell<bool>,
}

impl NotebookOperationManager {
    fn is_undo_stack_empty(&self) -> bool {
        self.pending_stack_operation.borrow().as_ref().is_none_or(StackOperation::is_empty)
    }

    fn push_stack_element(&self, alternative_version_id: &str, selection_state: Option<ISelectionState>) {
        let pending = self.pending_stack_operation.borrow().clone();
        if let Some(pending) = pending
            && !pending.is_empty()
        {
            pending.push_end_state(alternative_version_id, selection_state);
            if !self.is_appending.get() {
                self.undo_service.push_element(Rc::new(pending.clone()), pending.undo_redo_group.as_ref(), None);
            }
        }
        self.is_appending.set(false);
        *self.pending_stack_operation.borrow_mut() = None;
    }

    fn get_or_create_edit_stack_element(
        &self,
        begin_selection_state: Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
        alternative_version_id: &str,
    ) -> StackOperation {
        let mut pending = self.pending_stack_operation.borrow_mut();
        pending
            .get_or_insert_with(|| {
                StackOperation::new(
                    self.uri.clone(),
                    self.text_model.clone(),
                    undo_redo_group.cloned(),
                    self.pauseable_emitter.clone(),
                    self.post_undo_redo.clone(),
                    begin_selection_state,
                    alternative_version_id.to_string(),
                )
            })
            .clone()
    }

    fn append_previous_operation(&self) -> bool {
        let previous = self.undo_service.get_last_element(&self.uri);
        if let Some(previous) = previous
            && let Some(previous) = previous.as_any().downcast_ref::<StackOperation>()
        {
            *self.pending_stack_operation.borrow_mut() = Some(previous.clone());
            self.is_appending.set(true);
            return true;
        }
        false
    }

    fn push_edit_operation(
        &self,
        element: Rc<dyn UndoRedoElement>,
        begin_selection_state: Option<ISelectionState>,
        result_selection_state: Option<ISelectionState>,
        alternative_version_id: &str,
        undo_redo_group: Option<&UndoRedoGroup>,
    ) {
        let pending_stack_operation = self.get_or_create_edit_stack_element(begin_selection_state.clone(), undo_redo_group, alternative_version_id);
        pending_stack_operation.push_edit_operation(element, begin_selection_state, result_selection_state, alternative_version_id);
    }
}

struct TransformedEdit {
    edit: ICellEditOperation,
    cell_index: i64,
    end: Option<i64>,
    original_index: usize,
}

pub struct NotebookTextModel {
    base: Rc<Disposable>,
    this: Weak<NotebookTextModel>,
    view_type: String,
    uri: URI,
    undo_service: Rc<UndoRedoService>,
    model_service: Rc<ModelService>,
    language_service: Rc<dyn ILanguageService>,
    language_detection_service: Rc<dyn ILanguageDetectionService>,
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    notebook_logging_service: Rc<dyn INotebookLoggingService>,

    is_disposed: Cell<bool>,
    on_will_dispose: Rc<Emitter<()>>,
    on_will_add_remove_cells: Rc<Emitter<NotebookTextModelWillAddRemoveEvent>>,
    on_did_change_content: Rc<Emitter<NotebookTextModelChangedEvent>>,
    cell_handle_pool: Cell<i64>,
    cell_listeners: RefCell<HashMap<i64, DisposableRef>>,
    cells: RefCell<Vec<CellRc>>,
    default_collapse_config: Cell<Option<NotebookCellDefaultCollapseConfig>>,

    metadata: RefCell<NotebookDocumentMetadata>,
    transient_options: RefCell<TransientOptions>,
    version_id: Cell<i64>,

    /// This alternative id is only for non-cell-content changes.
    notebook_specific_alternative_id: Cell<i64>,

    /// Unlike, versionId, this can go down (via undo) or go to previous values (via redo)
    alternative_version_id: RefCell<String>,
    operation_manager: NotebookOperationManager,
    pauseable_emitter: Rc<NotebookEventEmitter>,

    new_cells_from_last_edit: RefCell<HashSet<i64>>,
}

impl NotebookTextModel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        view_type: &str,
        uri: URI,
        cells: Vec<ICellDto2>,
        metadata: NotebookDocumentMetadata,
        options: TransientOptions,
        undo_service: Rc<UndoRedoService>,
        model_service: Rc<ModelService>,
        language_service: Rc<dyn ILanguageService>,
        language_detection_service: Rc<dyn ILanguageDetectionService>,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
        notebook_logging_service: Rc<dyn INotebookLoggingService>,
    ) -> Rc<Self> {
        let text_model = Rc::new_cyclic(|this: &Weak<NotebookTextModel>| {
            let base = Disposable::new();
            let on_will_dispose = Emitter::<()>::new();
            base.register(on_will_dispose.clone());
            let on_will_add_remove_cells = Emitter::<NotebookTextModelWillAddRemoveEvent>::new();
            base.register(on_will_add_remove_cells.clone());
            let on_did_change_content = Emitter::<NotebookTextModelChangedEvent>::new();
            base.register(on_did_change_content.clone());

            let pauseable_emitter = NotebookEventEmitter::new();
            let post_undo_redo: Rc<dyn Fn(&str)> = {
                let this = this.clone();
                Rc::new(move |alternative_version_id: &str| {
                    if let Some(text_model) = this.upgrade() {
                        text_model.increase_version_id(true);
                        text_model.overwrite_alternative_version_id(alternative_version_id);
                    }
                })
            };
            let operation_manager = NotebookOperationManager {
                uri: uri.clone(),
                text_model: this.clone(),
                undo_service: undo_service.clone(),
                pauseable_emitter: pauseable_emitter.clone(),
                post_undo_redo,
                pending_stack_operation: RefCell::new(None),
                is_appending: Cell::new(false),
            };

            Self {
                base,
                this: this.clone(),
                view_type: view_type.to_string(),
                uri,
                undo_service,
                model_service,
                language_service,
                language_detection_service,
                notebook_execution_state_service,
                notebook_logging_service,
                is_disposed: Cell::new(false),
                on_will_dispose,
                on_will_add_remove_cells,
                on_did_change_content,
                cell_handle_pool: Cell::new(0),
                cell_listeners: RefCell::new(HashMap::new()),
                cells: RefCell::new(Vec::new()),
                default_collapse_config: Cell::new(None),
                metadata: RefCell::new(metadata),
                transient_options: RefCell::new(options),
                version_id: Cell::new(0),
                notebook_specific_alternative_id: Cell::new(0),
                alternative_version_id: RefCell::new("1".to_string()),
                operation_manager,
                pauseable_emitter,
                new_cells_from_last_edit: RefCell::new(HashSet::new()),
            }
        });

        text_model.initialize(cells, None);

        let this = text_model.this.clone();
        text_model.base.register(text_model.model_service.on_model_added().listen(move |model| {
            if let Some(text_model) = this.upgrade() {
                text_model.maybe_update_cell_text_model(model);
            }
        }));

        text_model.base.register(text_model.pauseable_emitter.clone());

        let this = text_model.this.clone();
        text_model.base.register(text_model.pauseable_emitter.event().listen(move |event| {
            if !event.raw_events.is_empty()
                && let Some(text_model) = this.upgrade()
            {
                text_model.on_did_change_content.fire(event.clone());
            }
        }));

        text_model
            .notebook_logging_service
            .trace("notebookTextModel", &format!("Initialized notebook text model for {}", text_model.uri.to_string(false).to_string_lossy()));

        text_model
    }

    pub fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    pub fn on_will_add_remove_cells(&self) -> Event<NotebookTextModelWillAddRemoveEvent> {
        self.on_will_add_remove_cells.event()
    }

    pub fn on_did_change_content(&self) -> Event<NotebookTextModelChangedEvent> {
        self.on_did_change_content.event()
    }

    pub fn view_type(&self) -> String {
        self.view_type.clone()
    }

    pub fn uri(&self) -> URI {
        self.uri.clone()
    }

    pub fn metadata(&self) -> NotebookDocumentMetadata {
        self.metadata.borrow().clone()
    }

    pub fn set_metadata(&self, metadata: NotebookDocumentMetadata) {
        *self.metadata.borrow_mut() = metadata;
    }

    pub fn transient_options(&self) -> TransientOptions {
        self.transient_options.borrow().clone()
    }

    pub fn set_transient_options(&self, transient_options: TransientOptions) {
        *self.transient_options.borrow_mut() = transient_options;
    }

    pub fn length(&self) -> usize {
        self.cells.borrow().len()
    }

    pub fn cells(&self) -> Vec<Rc<NotebookCellTextModel>> {
        self.cells.borrow().clone()
    }

    pub fn version_id(&self) -> i64 {
        self.version_id.get()
    }

    pub fn alternative_version_id(&self) -> String {
        self.alternative_version_id.borrow().clone()
    }

    pub fn notebook_type(&self) -> String {
        self.view_type.clone()
    }

    pub fn set_cell_collapse_default(&self, collapse_config: Option<NotebookCellDefaultCollapseConfig>) {
        self.default_collapse_config.set(collapse_config);
    }

    /// Cria uma célula do modelo a partir do DTO, com o próximo handle.
    fn create_cell(&self, cell_dto: ICellDto2) -> CellRc {
        let cell_handle = self.cell_handle_pool.get();
        self.cell_handle_pool.set(cell_handle + 1);
        let cell_uri = CellUri::generate(&self.uri, cell_handle);
        let default_eol = self.model_service.get_creation_options(&cell_dto.language, Some(&cell_uri), false).default_eol;
        NotebookCellTextModel::new(
            cell_uri,
            cell_handle,
            cell_dto,
            self.transient_options.borrow().clone(),
            self.language_service.clone(),
            default_eol,
            self.default_collapse_config.get(),
            Some(self.language_detection_service.clone()),
            self.notebook_logging_service.clone(),
        )
    }

    /// `this._cellListeners`: o ouvinte do conteúdo da célula (o `dirtyStateListener`).
    fn bind_cell_listener(&self, cell: &CellRc) -> DisposableRef {
        let this = self.this.clone();
        let event = cell.on_did_change_content();
        let cell = Rc::downgrade(cell);
        event.listen(move |change| {
            if let (Some(text_model), Some(cell)) = (this.upgrade(), cell.upgrade()) {
                text_model.bind_cell_content_handler(&cell, change);
            }
        })
    }

    pub fn initialize(&self, cells: Vec<ICellDto2>, trigger_dirty: Option<bool>) {
        *self.cells.borrow_mut() = Vec::new();
        self.version_id.set(0);
        self.notebook_specific_alternative_id.set(0);

        let main_cells: Vec<CellRc> = cells.into_iter().map(|cell| self.create_cell(cell)).collect();

        for cell in &main_cells {
            let dirty_state_listener = self.bind_cell_listener(cell);
            self.cell_listeners.borrow_mut().insert(cell.handle(), dirty_state_listener);
            self.base.register(cell.clone());
        }

        self.cells.borrow_mut().extend(main_cells);
        let alternative_version_id = self.generate_alternative_id();
        *self.alternative_version_id.borrow_mut() = alternative_version_id;

        if trigger_dirty.unwrap_or(false) {
            self.fire_change(NotebookRawContentChange::Unknown(NotebookDocumentUnknownChangeEvent), false, Some(true), None);
        }
    }

    fn maybe_update_cell_text_model(&self, text_model: &Rc<je_editor::text_model::TextModel>) {
        let text_model_uri = text_model.uri();
        if text_model_uri.scheme() == schemas::VSCODE_NOTEBOOK_CELL
            && let Some(cell_uri) = CellUri::parse(&text_model_uri)
            && ext_uri().is_equal(Some(&cell_uri.notebook), Some(&self.uri), false)
        {
            let cell_idx = self.get_cell_index_by_handle(cell_uri.handle);
            if cell_idx >= 0 {
                let cell = self.cells.borrow().get(cell_idx as usize).cloned();
                if let Some(cell) = cell {
                    cell.set_text_model(Some(text_model.clone()));
                }
            }
        }
    }

    /// Dispara um evento de mudança com um único evento cru.
    fn fire_change(&self, event: NotebookRawContentChange<Rc<dyn ICell>>, transient: bool, synchronous: Option<bool>, end_selection_state: Option<ISelectionState>) {
        self.pauseable_emitter.fire(NotebookTextModelChangedEvent {
            raw_events: vec![NotebookRawContentEvent { event, transient }],
            version_id: self.version_id.get(),
            synchronous,
            end_selection_state,
        });
    }

    /// O índice da célula na lista (`this._cells.indexOf(cell)`); `usize::MAX` quando ausente (ver as divergências).
    fn index_of(&self, cell: &CellRc) -> usize {
        self.cells.borrow().iter().position(|candidate| Rc::ptr_eq(candidate, cell)).unwrap_or(usize::MAX)
    }

    fn bind_cell_content_handler(&self, cell: &CellRc, change: &CellContentChange) {
        self.increase_version_id(matches!(change, CellContentChange::Content | CellContentChange::Model(_)));
        let index = usize::try_from(self.get_cell_index_by_handle(cell.handle())).unwrap_or(usize::MAX);
        match change {
            CellContentChange::Content => {
                self.fire_change(NotebookRawContentChange::ChangeCellContent(NotebookCellContentChangeEvent { index }), false, Some(true), None);
            }
            CellContentChange::Language => {
                self.fire_change(
                    NotebookRawContentChange::ChangeCellLanguage(NotebookCellsChangeLanguageEvent { index, language: cell.language() }),
                    false,
                    Some(true),
                    None,
                );
            }
            CellContentChange::Mime => {
                self.fire_change(
                    NotebookRawContentChange::ChangeCellMime(NotebookCellsChangeMimeEvent { index, mime: cell.mime() }),
                    false,
                    Some(true),
                    None,
                );
            }
            CellContentChange::Model(_) => {
                self.fire_change(NotebookRawContentChange::ChangeCellContent(NotebookCellContentChangeEvent { index }), false, Some(true), None);
            }
        }
    }

    fn generate_alternative_id(&self) -> String {
        let cells = self.cells.borrow();
        let handles: Vec<String> = cells.iter().map(|cell| format!("{},{}", cell.handle(), cell.alternative_id())).collect();
        format!("{}_{}", self.notebook_specific_alternative_id.get(), handles.join(";"))
    }

    pub fn push_stack_element(&self) {
        // https://github.com/microsoft/vscode/issues/207523
    }

    /// `this.cells.findIndex(c => c.handle === handle)`.
    fn get_cell_index_by_handle(&self, handle: i64) -> i64 {
        self.cells.borrow().iter().position(|cell| cell.handle() == handle).map_or(-1, to_i64)
    }

    fn get_cell_index_with_output_id_handle_from_edits(&self, output_id: &str, raw_edits: &[ICellEditOperation]) -> i64 {
        let edit = raw_edits.iter().find(|edit| output_edit(edit).is_some_and(|(outputs, _)| outputs.iter().any(|output| output.output_id == output_id)));
        if let Some(edit) = edit {
            if let Some(index) = edit_index(edit) {
                return to_i64(index);
            } else if let Some(handle) = edit_handle(edit) {
                let cell_index = self.get_cell_index_by_handle(handle);
                self.assert_index(cell_index, None);
                return cell_index;
            }
        }

        -1
    }

    fn get_cell_index_with_output_id_handle(&self, output_id: &str) -> i64 {
        self.cells
            .borrow()
            .iter()
            .position(|cell| cell.outputs().iter().any(|output| output.output_id() == output_id))
            .map_or(-1, to_i64)
    }

    pub fn reset(&self, cells: Vec<ICellDto2>, metadata: NotebookDocumentMetadata, transient_options: TransientOptions) {
        self.set_transient_options(transient_options);
        let executions = self.notebook_execution_state_service.get_cell_executions_for_notebook(&self.uri);
        let executing_cell_handles: Vec<i64> =
            executions.iter().filter(|exe| exe.state() == NotebookCellExecutionState::Executing).map(|exe| exe.cell_handle()).collect();
        let mut edits = NotebookTextModel::compute_edits(self, &cells, &executing_cell_handles);
        edits.push(ICellEditOperation::DocumentMetadata(crate::contrib::notebook::common::notebook_common::IDocumentMetadataEdit { metadata }));

        self.apply_edits(&edits, true, None, &|| None, None, false);
    }

    pub fn create_snapshot(&self, options: &INotebookSnapshotOptions) -> NotebookData {
        let transient_options = options.transient_options.clone().unwrap_or_else(|| self.transient_options());
        let mut data = NotebookData {
            metadata: objects::filter(&self.metadata.borrow(), |key, _| {
                !transient_options.transient_document_metadata.get(&key.to_string_lossy()).copied().unwrap_or(false)
            }),
            cells: Vec::new(),
        };

        let mut output_size: u64 = 0;
        for cell in self.cells() {
            let mut cell_data = ICellDto2 {
                cell_kind: cell.cell_kind(),
                language: cell.language(),
                mime: cell.mime(),
                source: cell.get_value(),
                outputs: Vec::new(),
                metadata: None,
                internal_metadata: Some(cell.internal_metadata()),
                collapse_state: None,
            };

            if matches!(options.context, SnapshotContext::Backup) && options.output_size_limit > 0 {
                for output in cell.outputs() {
                    for item in output.outputs() {
                        output_size += item.data.byte_length() as u64;
                    }
                }
                if output_size > options.output_size_limit {
                    errors::throw(Error::new("Notebook too large to backup"));
                }
            }

            cell_data.outputs =
                if !transient_options.transient_outputs { cell.outputs().iter().map(|output| output.as_dto()).collect() } else { Vec::new() };
            cell_data.metadata = Some(objects::filter(&cell.metadata(), |key, _| {
                !transient_options.transient_cell_metadata.get(&key.to_string_lossy()).copied().unwrap_or(false)
            }));

            data.cells.push(cell_data);
        }

        data
    }

    pub fn restore_snapshot(&self, snapshot: NotebookData, transient_options: Option<TransientOptions>) {
        let transient_options = transient_options.unwrap_or_else(|| self.transient_options());
        self.reset(snapshot.cells, snapshot.metadata, transient_options);
    }

    pub fn compute_edits(model: &NotebookTextModel, cells: &[ICellDto2], executing_handles: &[i64]) -> Vec<ICellEditOperation> {
        let mut edits: Vec<ICellEditOperation> = Vec::new();
        let is_executing = |cell: &NotebookCellTextModel| executing_handles.contains(&cell.handle());
        let model_cells = model.cells();

        let common_prefix = Self::common_prefix(&model_cells, model_cells.len(), 0, cells, cells.len(), 0, &is_executing);

        if common_prefix > 0 {
            for i in 0..common_prefix {
                edits.push(ICellEditOperation::Metadata(crate::contrib::notebook::common::notebook_common::ICellMetadataEdit {
                    index: i,
                    metadata: cells[i].metadata.clone().unwrap_or_else(empty_object),
                }));
                edits.extend(Self::compute_output_edit(i, &model_cells[i].outputs(), &cells[i].outputs));
            }
        }

        if model_cells.len() == cells.len() && common_prefix == model_cells.len() {
            return edits;
        }

        let common_suffix = Self::common_suffix(
            &model_cells,
            model_cells.len() - common_prefix,
            common_prefix,
            cells,
            cells.len() - common_prefix,
            common_prefix,
            &is_executing,
        );

        use crate::contrib::notebook::common::notebook_common::ICellReplaceEdit;
        if common_suffix > 0 {
            edits.push(ICellEditOperation::Replace(ICellReplaceEdit {
                index: common_prefix,
                count: model_cells.len() - common_prefix - common_suffix,
                cells: cells[common_prefix..cells.len() - common_suffix].to_vec(),
            }));
        } else if common_prefix > 0 {
            edits.push(ICellEditOperation::Replace(ICellReplaceEdit {
                index: common_prefix,
                count: model_cells.len() - common_prefix,
                cells: cells[common_prefix..].to_vec(),
            }));
        } else {
            edits.push(ICellEditOperation::Replace(ICellReplaceEdit { index: 0, count: model_cells.len(), cells: cells.to_vec() }));
        }

        if common_suffix > 0 {
            // has same suffix
            for i in (1..=common_suffix).rev() {
                edits.push(ICellEditOperation::Metadata(crate::contrib::notebook::common::notebook_common::ICellMetadataEdit {
                    index: model_cells.len() - i,
                    metadata: cells[cells.len() - i].metadata.clone().unwrap_or_else(empty_object),
                }));
                edits.extend(Self::compute_output_edit(model_cells.len() - i, &model_cells[model_cells.len() - i].outputs(), &cells[cells.len() - i].outputs));
            }
        }

        edits
    }

    fn compute_output_edit(index: usize, a: &[Rc<dyn ICellOutput>], b: &[IOutputDto]) -> Vec<ICellEditOperation> {
        use crate::contrib::notebook::common::notebook_common::{ICellOutputEdit, ICellOutputItemEdit};
        if a.len() != b.len() {
            return vec![ICellEditOperation::Output(ICellOutputEdit { index, outputs: b.to_vec(), append: Some(false) })];
        }

        if a.is_empty() {
            // no output
            return Vec::new();
        }

        // same length
        b.iter()
            .enumerate()
            .map(|(i, output)| {
                ICellEditOperation::OutputItems(ICellOutputItemEdit { output_id: a[i].output_id(), items: output.outputs.clone(), append: Some(false) })
            })
            .collect()
    }

    fn common_prefix(
        a: &[CellRc],
        a_len: usize,
        a_delta: usize,
        b: &[ICellDto2],
        b_len: usize,
        b_delta: usize,
        is_executing: &dyn Fn(&NotebookCellTextModel) -> bool,
    ) -> usize {
        let max_result = a_len.min(b_len);
        let mut result = 0;
        let mut i = 0;
        while i < max_result && a[a_delta + i].fast_equal(&b[b_delta + i], is_executing(&*a[a_delta + i])) {
            result += 1;
            i += 1;
        }

        result
    }

    fn common_suffix(
        a: &[CellRc],
        a_len: usize,
        a_delta: usize,
        b: &[ICellDto2],
        b_len: usize,
        b_delta: usize,
        is_executing: &dyn Fn(&NotebookCellTextModel) -> bool,
    ) -> usize {
        let max_result = a_len.min(b_len);
        let mut result = 0;
        let mut i = 0;
        while i < max_result && a[a_delta + a_len - i - 1].fast_equal(&b[b_delta + b_len - i - 1], is_executing(&*a[a_delta + a_len - i - 1])) {
            result += 1;
            i += 1;
        }
        result
    }

    fn is_only_editing_metadata_on_new_cells(&self, raw_edits: &[ICellEditOperation]) -> bool {
        for edit in raw_edits {
            if edit.edit_type() == CellEditType::PartialInternalMetadata {
                continue;
            }
            if edit.edit_type() != CellEditType::Metadata && edit.edit_type() != CellEditType::PartialMetadata {
                return false;
            }

            if let Some(index) = edit_index(edit) {
                let handle = match self.cells.borrow().get(index) {
                    Some(cell) => cell.handle(),
                    None => errors::throw(Error::new("Cannot read properties of undefined (reading 'handle')")),
                };
                if !self.new_cells_from_last_edit.borrow().contains(&handle) {
                    return false;
                }
            }
            if let Some(handle) = edit_handle(edit)
                && !self.new_cells_from_last_edit.borrow().contains(&handle)
            {
                return false;
            }
        }

        true
    }

    pub fn apply_edits(
        &self,
        raw_edits: &[ICellEditOperation],
        synchronous: bool,
        begin_selection_state: Option<ISelectionState>,
        end_selections_computer: &dyn Fn() -> Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
        compute_undo_redo: bool,
    ) -> bool {
        let mut compute_undo_redo = compute_undo_redo;
        self.notebook_logging_service.trace("textModelEdits", &format!("Begin applying {} raw edits", raw_edits.len()));
        let _resume = PauseGuard::new(&self.pauseable_emitter);

        self.operation_manager.push_stack_element(&self.alternative_version_id(), None);

        if compute_undo_redo && self.is_only_editing_metadata_on_new_cells(raw_edits) {
            if !self.operation_manager.append_previous_operation() {
                // we can't append the previous operation, so just don't compute undo/redo
                compute_undo_redo = false;
            }
        } else if compute_undo_redo {
            self.new_cells_from_last_edit.borrow_mut().clear();
        }

        let outcome = errors::catch(|| self.do_apply_edits(raw_edits, synchronous, compute_undo_redo, begin_selection_state.clone(), undo_redo_group));
        if let Err(err) = &outcome {
            self.notebook_logging_service.error("textModelEdits", &format!("Error while applying edits: {err}"));
        }

        if !self.pauseable_emitter.is_empty() {
            // Update selection and versionId after applying edits.
            let end_selections = end_selections_computer();
            self.increase_version_id(self.operation_manager.is_undo_stack_empty() && !self.pauseable_emitter.is_dirty_event());

            // Finalize undo element
            self.operation_manager.push_stack_element(&self.alternative_version_id(), end_selections.clone());

            // Broadcast changes
            self.pauseable_emitter.fire(NotebookTextModelChangedEvent {
                raw_events: Vec::new(),
                version_id: self.version_id.get(),
                synchronous: Some(synchronous),
                end_selection_state: end_selections,
            });
            self.notebook_logging_service.trace("textModelEdits", &format!("End applying {} raw edits", raw_edits.len()));
        }

        match outcome {
            Ok(()) => true,
            Err(err) => errors::throw(err),
        }
    }

    fn do_apply_edits(
        &self,
        raw_edits: &[ICellEditOperation],
        synchronous: bool,
        compute_undo_redo: bool,
        begin_selection_state: Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
    ) {
        let mut edits_with_details: Vec<TransformedEdit> = Vec::new();
        for (index, edit) in raw_edits.iter().enumerate() {
            let cell_index: i64;
            if let Some(edit_index) = edit_index(edit) {
                cell_index = to_i64(edit_index);
            } else if let Some(handle) = edit_handle(edit) {
                cell_index = self.get_cell_index_by_handle(handle);
                self.assert_index(cell_index, Some(format!("editType: {}, key: handle", edit.edit_type() as i32).as_str()));
            } else if let Some(output_id) = edit_output_id(edit) {
                let mut found_index = self.get_cell_index_with_output_id_handle(output_id);
                if self.index_is_invalid(found_index) {
                    // The referenced output may have been created in this batch of edits
                    found_index = self.get_cell_index_with_output_id_handle_from_edits(output_id, &raw_edits[..index]);
                }

                if self.index_is_invalid(found_index) {
                    // It's possible for an edit to refer to an output which was just cleared, ignore it without throwing
                    continue;
                }
                cell_index = found_index;
            } else if edit.edit_type() != CellEditType::DocumentMetadata {
                errors::throw(Error::new(format!("Invalid cell edit: {edit:?}")));
            } else {
                cell_index = -1;
            }

            let end = match edit {
                ICellEditOperation::DocumentMetadata(_) => None,
                ICellEditOperation::Replace(replace) => Some(to_i64(replace.index.saturating_add(replace.count))),
                _ => Some(cell_index),
            };
            edits_with_details.push(TransformedEdit { edit: edit.clone(), cell_index, end, original_index: index });
        }

        // compress all edits which have no side effects on cell index
        let mut merged_edits = Self::merge_cell_edits(edits_with_details);
        sort_by_compare(&mut merged_edits, |a, b| {
            let Some(a_end) = a.end else {
                return -1.0;
            };
            let Some(b_end) = b.end else {
                return 1.0;
            };

            let by_end = b_end - a_end;
            (if by_end != 0 { by_end } else { b.original_index as i64 - a.original_index as i64 }) as f64
        });

        let mut grouped: Vec<Vec<TransformedEdit>> = Vec::new();
        for curr in merged_edits {
            let same_group = grouped.last().is_some_and(|last| last[0].cell_index == curr.cell_index);
            if same_group {
                grouped.last_mut().expect("o grupo existe").push(curr);
            } else {
                grouped.push(vec![curr]);
            }
        }

        let mut flatten_edits: Vec<TransformedEdit> = Vec::new();
        for edits_on_same_index in grouped {
            let mut replace_edits: Vec<TransformedEdit> = Vec::new();
            let mut other_edits: Vec<TransformedEdit> = Vec::new();

            for edit in edits_on_same_index {
                if edit.edit.edit_type() == CellEditType::Replace {
                    replace_edits.push(edit);
                } else {
                    other_edits.push(edit);
                }
            }

            other_edits.reverse();
            flatten_edits.extend(other_edits);
            flatten_edits.extend(replace_edits);
        }

        for TransformedEdit { edit, cell_index, .. } in flatten_edits {
            match &edit {
                ICellEditOperation::Replace(edit) => {
                    self.replace_cells(edit.index, edit.count, edit.cells.clone(), synchronous, compute_undo_redo, begin_selection_state.clone(), undo_redo_group);
                }
                ICellEditOperation::Output(edit) => {
                    self.apply_output_edit(cell_index, &edit.outputs, edit.append, compute_undo_redo);
                }
                ICellEditOperation::OutputByHandle(edit) => {
                    self.apply_output_edit(cell_index, &edit.outputs, edit.append, compute_undo_redo);
                }
                ICellEditOperation::OutputItems(edit) => {
                    self.assert_index(cell_index, None);
                    let cell = self.cells.borrow()[cell_index as usize].clone();
                    if edit.append.unwrap_or(false) {
                        self.append_notebook_cell_output_items(&cell, &edit.output_id, edit.items.clone());
                    } else {
                        self.replace_notebook_cell_output_items(&cell, &edit.output_id, edit.items.clone());
                    }
                }
                ICellEditOperation::Metadata(edit) => {
                    self.assert_index(to_i64(edit.index), None);
                    let cell = self.cells.borrow()[edit.index].clone();
                    self.change_cell_metadata(&cell, edit.metadata.clone(), compute_undo_redo, begin_selection_state.clone(), undo_redo_group);
                }
                ICellEditOperation::PartialMetadata(edit) => {
                    self.apply_partial_metadata_edit(cell_index, &edit.metadata, compute_undo_redo, begin_selection_state.clone(), undo_redo_group);
                }
                ICellEditOperation::PartialMetadataByHandle(edit) => {
                    self.apply_partial_metadata_edit(cell_index, &edit.metadata, compute_undo_redo, begin_selection_state.clone(), undo_redo_group);
                }
                ICellEditOperation::PartialInternalMetadata(edit) => {
                    self.assert_index(cell_index, None);
                    let cell = self.cells.borrow()[cell_index as usize].clone();
                    self.change_cell_internal_metadata_partial(&cell, &edit.internal_metadata);
                }
                ICellEditOperation::PartialInternalMetadataByHandle(edit) => {
                    self.assert_index(cell_index, None);
                    let cell = self.cells.borrow()[cell_index as usize].clone();
                    self.change_cell_internal_metadata_partial(&cell, &edit.internal_metadata);
                }
                ICellEditOperation::CellLanguage(edit) => {
                    self.assert_index(to_i64(edit.index), None);
                    let cell = self.cells.borrow()[edit.index].clone();
                    self.change_cell_language(&cell, &edit.language, compute_undo_redo, begin_selection_state.clone(), undo_redo_group);
                }
                ICellEditOperation::DocumentMetadata(edit) => {
                    self.update_notebook_cell_metadata(edit.metadata.clone(), compute_undo_redo, begin_selection_state.clone(), undo_redo_group);
                }
                ICellEditOperation::Move(edit) => {
                    self.move_cell_to_idx(edit.index, edit.length, edit.new_idx, synchronous, compute_undo_redo, begin_selection_state.clone(), None, undo_redo_group);
                }
            }
        }
    }

    /// A edição `CellEditType.Output` (por índice ou por handle).
    fn apply_output_edit(&self, cell_index: i64, outputs: &[IOutputDto], append: Option<bool>, compute_undo_redo: bool) {
        self.assert_index(cell_index, None);
        let cell = self.cells.borrow()[cell_index as usize].clone();
        if append.unwrap_or(false) {
            let start = cell.outputs().len();
            self.splice_notebook_cell_outputs(
                &cell,
                NotebookCellOutputsSplice { start, delete_count: 0, new_outputs: outputs.iter().map(new_output).collect() },
                true,
                compute_undo_redo,
            );
        } else {
            self.splice_notebook_cell_outputs2(&cell, outputs, compute_undo_redo);
        }
    }

    /// A edição `CellEditType.PartialMetadata` (por índice ou por handle).
    fn apply_partial_metadata_edit(
        &self,
        cell_index: i64,
        metadata: &NullablePartialNotebookCellMetadata,
        compute_undo_redo: bool,
        begin_selection_state: Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
    ) {
        self.assert_index(cell_index, None);
        let cell = self.cells.borrow()[cell_index as usize].clone();
        self.change_cell_metadata_partial(&cell, metadata, compute_undo_redo, begin_selection_state, undo_redo_group);
    }

    fn merge_cell_edits(raw_edits: Vec<TransformedEdit>) -> Vec<TransformedEdit> {
        let mut merged_edits: Vec<TransformedEdit> = Vec::new();

        for edit in raw_edits {
            let next = output_edit(&edit.edit).map(|(outputs, append)| (outputs.clone(), append.unwrap_or(false)));
            let did_merge = match (merged_edits.last_mut(), next) {
                (Some(last), Some((next_outputs, true))) if last.cell_index == edit.cell_index => match output_edit_mut(&mut last.edit) {
                    Some((outputs, append)) => {
                        if append.unwrap_or(false) {
                            outputs.extend(next_outputs);
                            true
                        } else if outputs.is_empty() {
                            // last cell is not append and is clear outputs
                            *append = Some(false);
                            *outputs = next_outputs;
                            true
                        } else {
                            false
                        }
                    }
                    None => false,
                },
                _ => false,
            };

            if !did_merge {
                merged_edits.push(edit);
            }
        }

        merged_edits
    }

    #[allow(clippy::too_many_arguments)]
    fn replace_cells(
        &self,
        index: usize,
        count: usize,
        cell_dtos: Vec<ICellDto2>,
        synchronous: bool,
        compute_undo_redo: bool,
        begin_selection_state: Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
    ) {
        if count == 0 && cell_dtos.is_empty() {
            return;
        }

        let old_view_cells: Vec<CellRc> = self.cells.borrow().clone();
        let old_set: HashSet<i64> = old_view_cells.iter().map(|cell| cell.handle()).collect();

        // prepare remove
        for i in index..index.saturating_add(count).min(old_view_cells.len()) {
            let listener = self.cell_listeners.borrow_mut().remove(&old_view_cells[i].handle());
            if let Some(listener) = listener {
                listener.dispose();
            }
        }

        // prepare add
        let mut cells: Vec<CellRc> = Vec::new();
        for cell_dto in cell_dtos {
            let language = cell_dto.language.clone();
            let source = cell_dto.source.clone();
            let cell = self.create_cell(cell_dto);
            let cell_uri = cell.uri();
            if let Some(text_model) = self.model_service.get_model(&cell_uri) {
                cell.set_text_model(Some(text_model.clone()));
                cell.set_language(&language);
                text_model.set_value(TextSource::Text(source));
                let default_eol = self.model_service.get_creation_options(&language, Some(&cell_uri), false).default_eol;
                let text_buffer = create_text_buffer(TextSource::Text(text_model.get_value()), default_eol);
                self.base.register(text_buffer.disposable());
                cell.reset_text_buffer(Rc::new(RefCell::new(text_buffer)));
            }
            let dirty_state_listener = self.bind_cell_listener(&cell);

            self.new_cells_from_last_edit.borrow_mut().insert(cell.handle());
            self.cell_listeners.borrow_mut().insert(cell.handle(), dirty_state_listener);
            self.base.register(cell.clone());
            cells.push(cell);
        }

        // compute change
        let mut cells_copy: Vec<CellRc> = self.cells.borrow().clone();
        let splice_start = index.min(cells_copy.len());
        let splice_end = splice_start.saturating_add(count).min(cells_copy.len());
        cells_copy.splice(splice_start..splice_end, cells.iter().cloned()).for_each(drop);
        let before: Vec<CellRef> = self.cells.borrow().iter().cloned().map(CellRef).collect();
        let after: Vec<CellRef> = cells_copy.iter().cloned().map(CellRef).collect();
        let diffs: Vec<CellSplice> = diff_lists(&before, &after, |cell| old_set.contains(&cell.0.handle()), None)
            .into_iter()
            .map(|splice| (splice.start, splice.delete_count, splice.to_insert.into_iter().map(|cell| cell.0).collect()))
            .collect();
        self.on_will_add_remove_cells.fire(NotebookTextModelWillAddRemoveEvent {
            raw_event: NotebookCellsModelChangedEvent { changes: to_icell_splices(&diffs) },
        });

        // make change
        *self.cells.borrow_mut() = cells_copy;

        let undo_diff: Vec<SpliceCellsDiff> = diffs
            .iter()
            .map(|diff| {
                let deleted_cells: Vec<CellRc> = old_view_cells.iter().skip(diff.0).take(diff.1).cloned().collect();

                (diff.0, deleted_cells, diff.2.clone())
            })
            .collect();

        if compute_undo_redo {
            let editing_delegate = ITextCellEditingDelegate {
                insert_cell: Some(Box::new({
                    let this = self.this.clone();
                    move |index: usize, cell: CellRc, end_selections: Option<ISelectionState>| {
                        if let Some(text_model) = this.upgrade() {
                            text_model.insert_new_cell(index, vec![cell], true, end_selections);
                        }
                    }
                })),
                delete_cell: Some(Box::new({
                    let this = self.this.clone();
                    move |index: usize, end_selections: Option<ISelectionState>| {
                        if let Some(text_model) = this.upgrade() {
                            text_model.remove_cell(index, 1, true, end_selections);
                        }
                    }
                })),
                replace_cell: Some(Box::new({
                    let this = self.this.clone();
                    move |index: usize, count: usize, cells: Vec<CellRc>, end_selections: Option<ISelectionState>| {
                        if let Some(text_model) = this.upgrade() {
                            text_model.replace_new_cells(index, count, cells, true, end_selections);
                        }
                    }
                })),
                ..ITextCellEditingDelegate::default()
            };
            self.operation_manager.push_edit_operation(
                Rc::new(SpliceCellsEdit::new(self.uri.clone(), undo_diff, editing_delegate, None, None)),
                begin_selection_state,
                None,
                &self.alternative_version_id(),
                undo_redo_group,
            );
        }

        // should be deferred
        self.fire_change(
            NotebookRawContentChange::ModelChange(NotebookCellsModelChangedEvent { changes: to_icell_splices(&diffs) }),
            false,
            Some(synchronous),
            None,
        );
    }

    fn increase_version_id(&self, transient: bool) {
        self.version_id.set(self.version_id.get() + 1);
        if !transient {
            self.notebook_specific_alternative_id.set(self.version_id.get());
        }
        let alternative_version_id = self.generate_alternative_id();
        *self.alternative_version_id.borrow_mut() = alternative_version_id;
    }

    fn overwrite_alternative_version_id(&self, new_alternative_version_id: &str) {
        *self.alternative_version_id.borrow_mut() = new_alternative_version_id.to_string();
        // `Number(newAlternativeVersionId.substring(0, newAlternativeVersionId.indexOf('_')))`: sem `_`, o `indexOf` é -1 e o
        // `substring(0, -1)` é vazio
        let prefix = new_alternative_version_id.find('_').map_or("", |position| &new_alternative_version_id[..position]);
        self.notebook_specific_alternative_id.set(prefix.parse::<i64>().unwrap_or(0));
    }

    fn update_notebook_cell_metadata(
        &self,
        metadata: NotebookDocumentMetadata,
        compute_undo_redo: bool,
        begin_selection_state: Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
    ) {
        let old_metadata = self.metadata();
        let trigger_dirty_change = self.is_document_metadata_changed(&old_metadata, &metadata);

        if trigger_dirty_change && compute_undo_redo {
            let undo_fn: Box<dyn Fn()> = {
                let this = self.this.clone();
                let old_metadata = old_metadata.clone();
                let begin_selection_state = begin_selection_state.clone();
                let undo_redo_group = undo_redo_group.cloned();
                Box::new(move || {
                    if let Some(text_model) = this.upgrade() {
                        text_model.update_notebook_cell_metadata(old_metadata.clone(), false, begin_selection_state.clone(), undo_redo_group.as_ref());
                    }
                })
            };
            let redo_fn: Box<dyn Fn()> = {
                let this = self.this.clone();
                let metadata = metadata.clone();
                let begin_selection_state = begin_selection_state.clone();
                let undo_redo_group = undo_redo_group.cloned();
                Box::new(move || {
                    if let Some(text_model) = this.upgrade() {
                        text_model.update_notebook_cell_metadata(metadata.clone(), false, begin_selection_state.clone(), undo_redo_group.as_ref());
                    }
                })
            };
            self.operation_manager.push_edit_operation(
                Rc::new(ResourceUndoRedoElement { resource: self.uri.clone(), label: "Update Cell Metadata", undo_fn, redo_fn }),
                begin_selection_state,
                None,
                &self.alternative_version_id(),
                undo_redo_group,
            );
        }

        self.set_metadata(metadata);
        self.fire_change(
            NotebookRawContentChange::ChangeDocumentMetadata(NotebookDocumentChangeMetadataEvent { metadata: self.metadata() }),
            !trigger_dirty_change,
            Some(true),
            None,
        );
    }

    fn insert_new_cell(&self, index: usize, cells: Vec<CellRc>, synchronous: bool, end_selections: Option<ISelectionState>) {
        for cell in &cells {
            let dirty_state_listener = self.bind_cell_listener(cell);
            self.cell_listeners.borrow_mut().insert(cell.handle(), dirty_state_listener);
        }

        let changes: Vec<CellSplice> = vec![(index, 0, cells.clone())];
        self.on_will_add_remove_cells.fire(NotebookTextModelWillAddRemoveEvent {
            raw_event: NotebookCellsModelChangedEvent { changes: to_icell_splices(&changes) },
        });
        {
            let mut all_cells = self.cells.borrow_mut();
            let at = index.min(all_cells.len());
            all_cells.splice(at..at, cells).for_each(drop);
        }
        self.fire_change(
            NotebookRawContentChange::ModelChange(NotebookCellsModelChangedEvent { changes: to_icell_splices(&changes) }),
            false,
            Some(synchronous),
            end_selections,
        );
    }

    fn remove_cell(&self, index: usize, count: usize, synchronous: bool, end_selections: Option<ISelectionState>) {
        for i in index..index.saturating_add(count) {
            let handle = match self.cells.borrow().get(i) {
                Some(cell) => cell.handle(),
                None => errors::throw(Error::new("Cannot read properties of undefined (reading 'handle')")),
            };
            let listener = self.cell_listeners.borrow_mut().remove(&handle);
            if let Some(listener) = listener {
                listener.dispose();
            }
        }
        let changes: Vec<CellSplice> = vec![(index, count, Vec::new())];
        self.on_will_add_remove_cells.fire(NotebookTextModelWillAddRemoveEvent {
            raw_event: NotebookCellsModelChangedEvent { changes: to_icell_splices(&changes) },
        });
        {
            let mut all_cells = self.cells.borrow_mut();
            let start = index.min(all_cells.len());
            let end = index.saturating_add(count).min(all_cells.len());
            all_cells.drain(start..end).for_each(drop);
        }
        self.fire_change(
            NotebookRawContentChange::ModelChange(NotebookCellsModelChangedEvent { changes: to_icell_splices(&changes) }),
            false,
            Some(synchronous),
            end_selections,
        );
    }

    fn replace_new_cells(&self, index: usize, count: usize, cells: Vec<CellRc>, synchronous: bool, end_selections: Option<ISelectionState>) {
        for i in index..index.saturating_add(count) {
            let handle = match self.cells.borrow().get(i) {
                Some(cell) => cell.handle(),
                None => errors::throw(Error::new("Cannot read properties of undefined (reading 'handle')")),
            };
            let listener = self.cell_listeners.borrow_mut().remove(&handle);
            if let Some(listener) = listener {
                listener.dispose();
            }
        }

        for cell in &cells {
            let dirty_state_listener = self.bind_cell_listener(cell);
            self.cell_listeners.borrow_mut().insert(cell.handle(), dirty_state_listener);
        }

        let changes: Vec<CellSplice> = vec![(index, count, cells.clone())];
        self.on_will_add_remove_cells.fire(NotebookTextModelWillAddRemoveEvent {
            raw_event: NotebookCellsModelChangedEvent { changes: to_icell_splices(&changes) },
        });
        {
            let mut all_cells = self.cells.borrow_mut();
            let start = index.min(all_cells.len());
            let end = index.saturating_add(count).min(all_cells.len());
            all_cells.splice(start..end, cells).for_each(drop);
        }
        self.fire_change(
            NotebookRawContentChange::ModelChange(NotebookCellsModelChangedEvent { changes: to_icell_splices(&changes) }),
            false,
            Some(synchronous),
            end_selections,
        );
    }

    fn is_document_metadata_changed(&self, a: &NotebookDocumentMetadata, b: &NotebookDocumentMetadata) -> bool {
        let transient_options = self.transient_options.borrow();
        let mut keys: Vec<JsString> = Vec::new();
        for key in a.own_keys().into_iter().chain(b.own_keys()) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }

        for key in keys {
            let is_transient = transient_options.transient_document_metadata.get(&key.to_string_lossy()).copied().unwrap_or(false);
            let a_value = if a.is_nullish() { Dynamic::Undefined } else { a.get(&key) };
            let b_value = if b.is_nullish() { Dynamic::Undefined } else { b.get(&key) };
            if key == "custom" {
                if !self.custom_metadata_equal(&a_value, &b_value) && !is_transient {
                    return true;
                }
            } else if !a_value.strict_equals(&b_value) && !is_transient {
                return true;
            }
        }

        false
    }

    fn is_cell_metadata_changed(&self, a: &NotebookCellMetadata, b: &NotebookCellMetadata) -> bool {
        let transient_options = self.transient_options.borrow();
        let mut keys: Vec<JsString> = Vec::new();
        for key in a.own_keys().into_iter().chain(b.own_keys()) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }

        for key in keys {
            let a_value = if a.is_nullish() { Dynamic::Undefined } else { a.get(&key) };
            let b_value = if b.is_nullish() { Dynamic::Undefined } else { b.get(&key) };
            if !a_value.strict_equals(&b_value) && !transient_options.transient_cell_metadata.get(&key.to_string_lossy()).copied().unwrap_or(false) {
                return true;
            }
        }

        false
    }

    fn custom_metadata_equal(&self, a: &Dynamic, b: &Dynamic) -> bool {
        use je_base::dynamic::Truthy;
        if !a.is_truthy() && !b.is_truthy() {
            // both of them are nullish or undefined
            return true;
        }

        if !a.is_truthy() || !b.is_truthy() {
            return false;
        }

        let a_props = a.own_keys();
        let b_props = b.own_keys();

        if a_props.len() != b_props.len() {
            return false;
        }

        for prop_name in &a_props {
            if !a.get(prop_name).strict_equals(&b.get(prop_name)) {
                return false;
            }
        }

        true
    }

    fn change_cell_metadata_partial(
        &self,
        cell: &CellRc,
        metadata: &NullablePartialNotebookCellMetadata,
        compute_undo_redo: bool,
        begin_selection_state: Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
    ) {
        let new_object = ObjectRef::from_entries(cell.metadata().own_entries());
        for (key, value) in metadata.own_entries() {
            let value = if value.is_nullish() { Dynamic::Undefined } else { value };
            new_object.set(&key, value);
        }

        self.change_cell_metadata(cell, Dynamic::Object(new_object), compute_undo_redo, begin_selection_state, undo_redo_group);
    }

    fn change_cell_metadata(
        &self,
        cell: &CellRc,
        metadata: NotebookCellMetadata,
        compute_undo_redo: bool,
        begin_selection_state: Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
    ) {
        let trigger_dirty_change = self.is_cell_metadata_changed(&cell.metadata(), &metadata);

        if trigger_dirty_change && compute_undo_redo {
            let index = self.index_of(cell);
            let editing_delegate = ITextCellEditingDelegate {
                update_cell_metadata: Some(Box::new({
                    let this = self.this.clone();
                    let begin_selection_state = begin_selection_state.clone();
                    let undo_redo_group = undo_redo_group.cloned();
                    move |index: usize, new_metadata: NotebookCellMetadata| {
                        let Some(text_model) = this.upgrade() else {
                            return;
                        };
                        let cell = text_model.cells.borrow().get(index).cloned();
                        let Some(cell) = cell else {
                            return;
                        };
                        text_model.change_cell_metadata(&cell, new_metadata, false, begin_selection_state.clone(), undo_redo_group.as_ref());
                    }
                })),
                ..ITextCellEditingDelegate::default()
            };
            self.operation_manager.push_edit_operation(
                Rc::new(CellMetadataEdit::new(self.uri.clone(), index, freeze(&cell.metadata()), freeze(&metadata), editing_delegate)),
                begin_selection_state,
                None,
                &self.alternative_version_id(),
                undo_redo_group,
            );
        }

        // should be deferred
        cell.set_metadata(metadata);
        self.fire_change(
            NotebookRawContentChange::ChangeCellMetadata(NotebookCellsChangeMetadataEvent { index: self.index_of(cell), metadata: cell.metadata() }),
            !trigger_dirty_change,
            Some(true),
            None,
        );
    }

    fn change_cell_internal_metadata_partial(&self, cell: &CellRc, internal_metadata: &NullablePartialNotebookCellInternalMetadata) {
        let mut new_internal_metadata: NotebookCellInternalMetadata = cell.internal_metadata();
        if let Some(value) = &internal_metadata.internal_id {
            new_internal_metadata.internal_id = value.clone();
        }
        if let Some(value) = &internal_metadata.execution_id {
            new_internal_metadata.execution_id = value.clone();
        }
        if let Some(value) = &internal_metadata.execution_order {
            new_internal_metadata.execution_order = *value;
        }
        if let Some(value) = &internal_metadata.last_run_success {
            new_internal_metadata.last_run_success = *value;
        }
        if let Some(value) = &internal_metadata.run_start_time {
            new_internal_metadata.run_start_time = *value;
        }
        if let Some(value) = &internal_metadata.run_start_time_adjustment {
            new_internal_metadata.run_start_time_adjustment = *value;
        }
        if let Some(value) = &internal_metadata.run_end_time {
            new_internal_metadata.run_end_time = *value;
        }
        if let Some(value) = &internal_metadata.render_duration {
            new_internal_metadata.render_duration = value.clone();
        }
        if let Some(value) = &internal_metadata.error {
            new_internal_metadata.error = value.clone();
        }

        cell.set_internal_metadata(new_internal_metadata);
        self.fire_change(
            NotebookRawContentChange::ChangeCellInternalMetadata(NotebookCellsChangeInternalMetadataEvent {
                index: self.index_of(cell),
                internal_metadata: cell.internal_metadata(),
            }),
            true,
            Some(true),
            None,
        );
    }

    fn change_cell_language(
        &self,
        cell: &CellRc,
        language_id: &str,
        compute_undo_redo: bool,
        begin_selection_state: Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
    ) {
        if cell.language() == language_id {
            return;
        }

        let old_language = cell.language();
        cell.set_language(language_id);

        if compute_undo_redo {
            let undo_fn: Box<dyn Fn()> = {
                let this = self.this.clone();
                let cell = cell.clone();
                let old_language = old_language.clone();
                let begin_selection_state = begin_selection_state.clone();
                let undo_redo_group = undo_redo_group.cloned();
                Box::new(move || {
                    if let Some(text_model) = this.upgrade() {
                        text_model.change_cell_language(&cell, &old_language, false, begin_selection_state.clone(), undo_redo_group.as_ref());
                    }
                })
            };
            let redo_fn: Box<dyn Fn()> = {
                let this = self.this.clone();
                let cell = cell.clone();
                let language_id = language_id.to_string();
                let begin_selection_state = begin_selection_state.clone();
                let undo_redo_group = undo_redo_group.cloned();
                Box::new(move || {
                    if let Some(text_model) = this.upgrade() {
                        text_model.change_cell_language(&cell, &language_id, false, begin_selection_state.clone(), undo_redo_group.as_ref());
                    }
                })
            };
            self.operation_manager.push_edit_operation(
                Rc::new(ResourceUndoRedoElement { resource: self.uri.clone(), label: "Update Cell Language", undo_fn, redo_fn }),
                begin_selection_state,
                None,
                &self.alternative_version_id(),
                undo_redo_group,
            );
        }

        self.fire_change(
            NotebookRawContentChange::ChangeCellLanguage(NotebookCellsChangeLanguageEvent { index: self.index_of(cell), language: language_id.to_string() }),
            false,
            Some(true),
            None,
        );
    }

    fn splice_notebook_cell_outputs2(&self, cell: &CellRc, outputs: &[IOutputDto], compute_undo_redo: bool) {
        if outputs.is_empty() && cell.outputs().is_empty() {
            return;
        }

        if outputs.len() <= 1 {
            self.splice_notebook_cell_outputs(
                cell,
                NotebookCellOutputsSplice { start: 0, delete_count: cell.outputs().len(), new_outputs: outputs.iter().map(new_output).collect() },
                false,
                compute_undo_redo,
            );
            return;
        }

        let current_outputs: Vec<IOutputDto> = cell.outputs().iter().map(|output| output.as_dto()).collect();
        let mut diff = LcsDiff::new(Rc::new(OutputSequence { outputs: current_outputs }), Rc::new(OutputSequence { outputs: outputs.to_vec() }), None);
        let diff_result = diff.compute_diff(false);
        let splices: Vec<NotebookCellOutputsSplice> = diff_result
            .changes
            .iter()
            .map(|change| {
                let modified_start = change.modified_start as usize;
                let modified_length = change.modified_length as usize;
                NotebookCellOutputsSplice {
                    start: change.original_start as usize,
                    delete_count: change.original_length as usize,
                    // create cell output text model only when it's inserted into the notebook document
                    new_outputs: outputs[modified_start..modified_start + modified_length].iter().map(new_output).collect(),
                }
            })
            .collect();
        for splice in splices.into_iter().rev() {
            self.splice_notebook_cell_outputs(cell, splice, false, compute_undo_redo);
        }
    }

    fn splice_notebook_cell_outputs(&self, cell: &CellRc, splice: NotebookCellOutputsSplice, append: bool, _compute_undo_redo: bool) {
        cell.splice_notebook_cell_outputs(splice);
        self.fire_change(
            NotebookRawContentChange::Output(NotebookOutputChangedEvent {
                index: self.index_of(cell),
                outputs: cell.outputs().iter().map(|output| output.as_dto()).collect(),
                append,
            }),
            self.transient_options.borrow().transient_outputs,
            Some(true),
            None,
        );
    }

    fn append_notebook_cell_output_items(&self, cell: &CellRc, output_id: &str, items: Vec<IOutputItemDto>) {
        if cell.change_output_items(output_id, true, items.clone()) {
            self.fire_change(
                NotebookRawContentChange::OutputItem(NotebookOutputItemChangedEvent {
                    index: self.index_of(cell),
                    output_id: output_id.to_string(),
                    output_items: items,
                    append: true,
                }),
                self.transient_options.borrow().transient_outputs,
                Some(true),
                None,
            );
        }
    }

    fn replace_notebook_cell_output_items(&self, cell: &CellRc, output_id: &str, items: Vec<IOutputItemDto>) {
        if cell.change_output_items(output_id, false, items.clone()) {
            self.fire_change(
                NotebookRawContentChange::OutputItem(NotebookOutputItemChangedEvent {
                    index: self.index_of(cell),
                    output_id: output_id.to_string(),
                    output_items: items,
                    append: false,
                }),
                self.transient_options.borrow().transient_outputs,
                Some(true),
                None,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn move_cell_to_idx(
        &self,
        index: usize,
        length: usize,
        new_idx: usize,
        synchronous: bool,
        pushed_to_undo_stack: bool,
        before_selections: Option<ISelectionState>,
        end_selections: Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
    ) -> bool {
        if pushed_to_undo_stack {
            let editing_delegate = ITextCellEditingDelegate {
                move_cell: Some(Box::new({
                    let this = self.this.clone();
                    let undo_redo_group = undo_redo_group.cloned();
                    move |from_index: usize,
                          length: usize,
                          to_index: usize,
                          before_selections: Option<ISelectionState>,
                          end_selections: Option<ISelectionState>| {
                        if let Some(text_model) = this.upgrade() {
                            text_model.move_cell_to_idx(from_index, length, to_index, true, false, before_selections, end_selections, undo_redo_group.as_ref());
                        }
                    }
                })),
                ..ITextCellEditingDelegate::default()
            };
            self.operation_manager.push_edit_operation(
                Rc::new(MoveCellEdit::new(self.uri.clone(), index, length, new_idx, editing_delegate, before_selections.clone(), end_selections.clone())),
                before_selections,
                end_selections.clone(),
                &self.alternative_version_id(),
                undo_redo_group,
            );
        }

        self.assert_index(to_i64(index), None);
        self.assert_index(to_i64(new_idx), None);

        let cells: Vec<CellRc> = {
            let mut all_cells = self.cells.borrow_mut();
            let start = index.min(all_cells.len());
            let end = index.saturating_add(length).min(all_cells.len());
            let moved: Vec<CellRc> = all_cells.drain(start..end).collect();
            let at = new_idx.min(all_cells.len());
            all_cells.splice(at..at, moved.iter().cloned()).for_each(drop);
            moved
        };
        self.fire_change(
            NotebookRawContentChange::Move(NotebookCellsModelMoveEvent { index, length, new_idx, cells: to_icells(&cells) }),
            false,
            Some(synchronous),
            end_selections,
        );

        true
    }

    fn assert_index(&self, index: i64, context: Option<&str>) {
        if self.index_is_invalid(index) {
            let context = context.map_or(String::new(), |context| format!(", {context}"));
            errors::throw(Error::new(format!("model index out of range {index} (cellCount: {}{context})", self.cells.borrow().len())));
        }
    }

    fn index_is_invalid(&self, index: i64) -> bool {
        index < 0 || index >= to_i64(self.cells.borrow().len())
    }

    //#region Find
    pub fn find_next_match(
        &self,
        search_string: &str,
        search_start: &NotebookCellPosition,
        is_regex: bool,
        match_case: bool,
        word_separators: Option<&str>,
        search_end: Option<&NotebookCellPosition>,
    ) -> Option<NotebookCellMatch> {
        // check if search cell index is valid
        self.assert_index(to_i64(search_start.cell_index), None);
        let search_params = SearchParams::new(JsString::from(search_string), is_regex, match_case, word_separators);
        let search_data = search_params.parse_search_request()?;

        let mut cell_index = search_start.cell_index;
        let mut search_start_position = search_start.position;

        let mut search_end_cell = self.cells.borrow().len();

        while cell_index < search_end_cell {
            let cell = match self.cells.borrow().get(cell_index) {
                Some(cell) => cell.clone(),
                None => errors::throw(Error::new("Cannot read properties of undefined (reading 'textBuffer')")),
            };

            // if we have wrapped back to the point of the initial search cell, we search from beginning to the provided searchEnd position
            let wrap_flag = search_end.is_some_and(|end| cell_index == end.cell_index && search_start_position.is_before(&end.position));
            let text_buffer = cell.text_buffer();
            let line_count = text_buffer.borrow().get_line_count() as i32;
            let (end_line_number, end_column) = match (wrap_flag, search_end) {
                (true, Some(end)) => (end.position.line_number, end.position.column),
                _ => (line_count, text_buffer.borrow().get_line_max_column(line_count) as i32),
            };
            let search_range = Range::new(search_start_position.line_number, search_start_position.column, end_line_number, end_column);

            let result = text_buffer.borrow().find_matches_line_by_line(&search_range, &search_data, false, 1);
            if let Some(first) = result.into_iter().next() {
                return Some(NotebookCellMatch { cell, match_: first });
            } else if wrap_flag {
                // this means there are no more valid matches in the notebook
                break;
            }

            // Move to the next cell
            cell_index += 1;

            // wrap if a searchEnd is provided and we are past the end of the notebook
            if let Some(end) = search_end
                && cell_index >= self.cells.borrow().len()
            {
                cell_index = 0;
                search_end_cell = end.cell_index + 1;
            }

            search_start_position = Position::new(1, 1); // Reset position to start of the next cell
        }

        None
    }

    pub fn find_matches(&self, search_string: &str, is_regex: bool, match_case: bool, word_separators: Option<&str>) -> Vec<NotebookCellMatches> {
        let search_params = SearchParams::new(JsString::from(search_string), is_regex, match_case, word_separators);
        let Some(search_data) = search_params.parse_search_request() else {
            return Vec::new();
        };

        let mut results: Vec<NotebookCellMatches> = Vec::new();
        for cell in self.cells() {
            let text_buffer = cell.text_buffer();
            let line_count = text_buffer.borrow().get_line_count() as i32;
            let search_range = Range::new(1, 1, line_count, text_buffer.borrow().get_line_max_column(line_count) as i32);
            let matches = text_buffer.borrow().find_matches_line_by_line(&search_range, &search_data, false, 1000);

            if !matches.is_empty() {
                results.push(NotebookCellMatches { cell, matches });
            }
        }

        results
    }
    //#endregion
}

impl IDisposable for NotebookTextModel {
    fn dispose(&self) {
        if self.is_disposed.get() {
            // NotebookEditorModel can be disposed twice, don't fire onWillDispose again
            return;
        }

        self.is_disposed.set(true);
        self.on_will_dispose.fire(());
        self.undo_service.remove_elements(&self.uri);

        let cell_listeners: Vec<DisposableRef> = self.cell_listeners.borrow_mut().drain().map(|(_, listener)| listener).collect();
        dispose_all(cell_listeners.iter());

        let cells = std::mem::take(&mut *self.cells.borrow_mut());
        dispose_all(cells.iter());
        self.base.dispose();
    }
}

impl INotebookTextModelLike for NotebookTextModel {
    fn uri(&self) -> URI {
        NotebookTextModel::uri(self)
    }

    fn notebook_type(&self) -> String {
        NotebookTextModel::notebook_type(self)
    }
}

impl INotebookTextModel for NotebookTextModel {
    fn view_type(&self) -> String {
        NotebookTextModel::view_type(self)
    }

    fn metadata(&self) -> NotebookDocumentMetadata {
        NotebookTextModel::metadata(self)
    }

    fn set_metadata(&self, metadata: NotebookDocumentMetadata) {
        NotebookTextModel::set_metadata(self, metadata);
    }

    fn transient_options(&self) -> TransientOptions {
        NotebookTextModel::transient_options(self)
    }

    fn version_id(&self) -> i64 {
        NotebookTextModel::version_id(self)
    }

    fn length(&self) -> usize {
        NotebookTextModel::length(self)
    }

    fn cells(&self) -> Vec<Rc<dyn ICell>> {
        to_icells(&NotebookTextModel::cells(self))
    }

    fn reset(&self, cells: Vec<ICellDto2>, metadata: NotebookDocumentMetadata, transient_options: TransientOptions) {
        NotebookTextModel::reset(self, cells, metadata, transient_options);
    }

    fn create_snapshot(&self, options: &INotebookSnapshotOptions) -> NotebookData {
        NotebookTextModel::create_snapshot(self, options)
    }

    fn restore_snapshot(&self, snapshot: NotebookData, transient_options: Option<TransientOptions>) {
        NotebookTextModel::restore_snapshot(self, snapshot, transient_options);
    }

    fn apply_edits(
        &self,
        raw_edits: &[ICellEditOperation],
        synchronous: bool,
        begin_selection_state: Option<ISelectionState>,
        end_selections_computer: &dyn Fn() -> Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
        compute_undo_redo: Option<bool>,
    ) -> bool {
        NotebookTextModel::apply_edits(
            self,
            raw_edits,
            synchronous,
            begin_selection_state,
            end_selections_computer,
            undo_redo_group,
            compute_undo_redo.unwrap_or(false),
        )
    }

    fn on_did_change_content(&self) -> Event<NotebookTextModelChangedEvent> {
        NotebookTextModel::on_did_change_content(self)
    }

    fn on_will_dispose(&self) -> Event<()> {
        NotebookTextModel::on_will_dispose(self)
    }
}

struct OutputSequence {
    outputs: Vec<IOutputDto>,
}

/// O `VSBuffer` como o valor que o `hash` do original percorre: os campos `buffer` (o `Uint8Array`) e `byteLength`.
fn vs_buffer_value(data: &je_base::buffer::VSBuffer) -> Dynamic {
    Dynamic::object([("buffer", Dynamic::typed_array(data.to_vec())), ("byteLength", Dynamic::Number(data.byte_length() as f64))])
}

impl ISequence for OutputSequence {
    fn get_elements(&self) -> SequenceElements {
        SequenceElements::Ints(
            self.outputs
                .iter()
                .map(|output| {
                    hash(&Dynamic::array(
                        output.outputs.iter().map(|item| Dynamic::object([("mime", Dynamic::string(&item.mime)), ("data", vs_buffer_value(&item.data))])),
                    ))
                })
                .collect(),
        )
    }
}
