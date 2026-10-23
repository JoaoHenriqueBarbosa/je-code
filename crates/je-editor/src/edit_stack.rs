//! Porte de `src/vs/editor/common/model/editStack.ts`: os elementos de desfazer de um modelo e a pilha de
//! edições que o `TextModel` usa.
//!
//! Decisões:
//! - O elemento guarda o modelo por `Weak` (o modelo é dono da pilha, a pilha é dona dos elementos) e a URI
//!   do recurso; o `model: ITextModel | URI` do original, usado para reanexar elementos de um modelo
//!   destruído, vira o estado `Detached` com só a URI.
//! - O `EditStack` guarda o modelo por `Weak`, criado junto do modelo (`Rc::new_cyclic`).
//!
//! Pendente: `MultiModelEditStackElement` e `IUndoRedoDelegate`, que são o elemento de workspace (várias
//! URIs numa operação) do `undoRedo.ts`.

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::uri::URI;

use crate::model::{EndOfLineSequence, ValidAnnotatedEditOperation, ValidEditOperation};
use crate::selection::Selection;
use crate::text_change::{TextChange, compress_consecutive_text_changes};
use crate::text_model::TextModel;
use crate::text_model_edit_source::{EditSources, TextModelEditSource};
use crate::undo_redo::{UndoRedoElement, UndoRedoGroup};
use crate::undo_redo_service::UndoRedoService;

/// `SingleModelEditStackData`.
#[derive(Clone, Debug, PartialEq)]
pub struct SingleModelEditStackData {
    pub before_version_id: i64,
    pub after_version_id: i64,
    pub before_eol: EndOfLineSequence,
    pub after_eol: EndOfLineSequence,
    pub before_cursor_state: Option<Vec<Selection>>,
    pub after_cursor_state: Option<Vec<Selection>>,
    pub changes: Vec<TextChange>,
}

fn write_u32(b: &mut [u8], value: u32, offset: usize) -> usize {
    b[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    offset + 4
}

fn read_u32(b: &[u8], offset: usize) -> (u32, usize) {
    (u32::from_be_bytes([b[offset], b[offset + 1], b[offset + 2], b[offset + 3]]), offset + 4)
}

impl SingleModelEditStackData {
    pub fn new(
        before_version_id: i64,
        after_version_id: i64,
        before_eol: EndOfLineSequence,
        after_eol: EndOfLineSequence,
        before_cursor_state: Option<Vec<Selection>>,
        after_cursor_state: Option<Vec<Selection>>,
        changes: Vec<TextChange>,
    ) -> Self {
        Self {
            before_version_id,
            after_version_id,
            before_eol,
            after_eol,
            before_cursor_state,
            after_cursor_state,
            changes,
        }
    }

    pub fn create(model: &TextModel, before_cursor_state: Option<Vec<Selection>>) -> Self {
        let alternative_version_id = model.get_alternative_version_id();
        let eol = EndOfLineSequence::from(model.get_eol());
        Self::new(
            alternative_version_id,
            alternative_version_id,
            eol,
            eol,
            before_cursor_state.clone(),
            before_cursor_state,
            Vec::new(),
        )
    }

    pub fn append(&mut self, text_changes: Vec<TextChange>, after_eol: EndOfLineSequence, after_version_id: i64, after_cursor_state: Option<Vec<Selection>>) {
        if !text_changes.is_empty() {
            let previous = std::mem::take(&mut self.changes);
            self.changes = compress_consecutive_text_changes(Some(previous), text_changes);
        }
        self.after_eol = after_eol;
        self.after_version_id = after_version_id;
        self.after_cursor_state = after_cursor_state;
    }

    fn write_selections_size(selections: &Option<Vec<Selection>>) -> usize {
        4 + 4 * 4 * selections.as_ref().map_or(0, Vec::len)
    }

    fn write_selections(b: &mut [u8], selections: &Option<Vec<Selection>>, mut offset: usize) -> usize {
        offset = write_u32(b, selections.as_ref().map_or(0, Vec::len) as u32, offset);
        for selection in selections.iter().flatten() {
            offset = write_u32(b, selection.selection_start_line_number as u32, offset);
            offset = write_u32(b, selection.selection_start_column as u32, offset);
            offset = write_u32(b, selection.position_line_number as u32, offset);
            offset = write_u32(b, selection.position_column as u32, offset);
        }
        offset
    }

    fn read_selections(b: &[u8], mut offset: usize, dest: &mut Vec<Selection>) -> usize {
        let (count, next) = read_u32(b, offset);
        offset = next;
        for _ in 0..count {
            let (selection_start_line_number, next) = read_u32(b, offset);
            let (selection_start_column, next) = read_u32(b, next);
            let (position_line_number, next) = read_u32(b, next);
            let (position_column, next) = read_u32(b, next);
            offset = next;
            dest.push(Selection::new(
                selection_start_line_number as i32,
                selection_start_column as i32,
                position_line_number as i32,
                position_column as i32,
            ));
        }
        offset
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut necessary_size = 4 // beforeVersionId
            + 4 // afterVersionId
            + 1 // beforeEOL
            + 1 // afterEOL
            + Self::write_selections_size(&self.before_cursor_state)
            + Self::write_selections_size(&self.after_cursor_state)
            + 4; // change count
        for change in &self.changes {
            necessary_size += change.write_size();
        }

        let mut b = vec![0u8; necessary_size];
        let mut offset = 0;
        offset = write_u32(&mut b, self.before_version_id as u32, offset);
        offset = write_u32(&mut b, self.after_version_id as u32, offset);
        b[offset] = self.before_eol as u8;
        offset += 1;
        b[offset] = self.after_eol as u8;
        offset += 1;
        offset = Self::write_selections(&mut b, &self.before_cursor_state, offset);
        offset = Self::write_selections(&mut b, &self.after_cursor_state, offset);
        offset = write_u32(&mut b, self.changes.len() as u32, offset);
        for change in &self.changes {
            offset = change.write(&mut b, offset);
        }
        b
    }

    pub fn deserialize(source: &[u8]) -> Self {
        let b = source;
        let mut offset = 0;
        let (before_version_id, next) = read_u32(b, offset);
        let (after_version_id, next) = read_u32(b, next);
        offset = next;
        let eol_of = |value: u8| if value == 0 { EndOfLineSequence::Lf } else { EndOfLineSequence::CrLf };
        let before_eol = eol_of(b[offset]);
        offset += 1;
        let after_eol = eol_of(b[offset]);
        offset += 1;
        let mut before_cursor_state = Vec::new();
        offset = Self::read_selections(b, offset, &mut before_cursor_state);
        let mut after_cursor_state = Vec::new();
        offset = Self::read_selections(b, offset, &mut after_cursor_state);
        let (change_count, next) = read_u32(b, offset);
        offset = next;
        let mut changes = Vec::new();
        for _ in 0..change_count {
            offset = TextChange::read(b, offset, &mut changes);
        }
        Self::new(
            i64::from(before_version_id),
            i64::from(after_version_id),
            before_eol,
            after_eol,
            Some(before_cursor_state),
            Some(after_cursor_state),
            changes,
        )
    }
}

/// O dado do elemento: aberto (editável) ou fechado (serializado, para ocupar menos memória).
enum ElementData {
    Open(SingleModelEditStackData),
    Closed(Vec<u8>),
}

/// O `model: ITextModel | URI` do elemento.
enum ElementModel {
    Attached(Weak<TextModel>),
    Detached,
}

/// `SingleModelEditStackElement`.
pub struct SingleModelEditStackElement {
    pub label: String,
    pub code: String,
    resource: URI,
    model: RefCell<ElementModel>,
    data: RefCell<ElementData>,
}

impl SingleModelEditStackElement {
    pub fn new(label: &str, code: &str, model: &TextModel, before_cursor_state: Option<Vec<Selection>>) -> Self {
        Self {
            label: label.to_owned(),
            code: code.to_owned(),
            resource: model.uri(),
            model: RefCell::new(ElementModel::Attached(model.downgrade())),
            data: RefCell::new(ElementData::Open(SingleModelEditStackData::create(model, before_cursor_state))),
        }
    }

    /// `matchesResource`.
    pub fn matches_resource(&self, resource: &URI) -> bool {
        self.resource.to_string(false) == resource.to_string(false)
    }

    /// `setModel(model)`.
    pub fn set_model(&self, model: Option<&TextModel>) {
        *self.model.borrow_mut() = match model {
            Some(model) => ElementModel::Attached(model.downgrade()),
            None => ElementModel::Detached,
        };
    }

    /// `canAppend(model)`: o mesmo modelo e o dado ainda aberto.
    pub fn can_append(&self, model: &TextModel) -> bool {
        let same_model = match &*self.model.borrow() {
            ElementModel::Attached(weak) => std::ptr::eq(weak.as_ptr(), model),
            ElementModel::Detached => false,
        };
        same_model && matches!(&*self.data.borrow(), ElementData::Open(_))
    }

    pub fn append(&self, text_changes: Vec<TextChange>, after_eol: EndOfLineSequence, after_version_id: i64, after_cursor_state: Option<Vec<Selection>>) {
        if let ElementData::Open(data) = &mut *self.data.borrow_mut() {
            data.append(text_changes, after_eol, after_version_id, after_cursor_state);
        }
    }

    /// `close()`.
    pub fn close(&self) {
        let mut data = self.data.borrow_mut();
        if let ElementData::Open(open) = &*data {
            *data = ElementData::Closed(open.serialize());
        }
    }

    /// `open()`.
    pub fn open(&self) {
        let mut data = self.data.borrow_mut();
        if let ElementData::Closed(bytes) = &*data {
            *data = ElementData::Open(SingleModelEditStackData::deserialize(bytes));
        }
    }

    /// O dado atual, deserializado se preciso (sem mudar o estado do elemento).
    fn current_data(&self) -> SingleModelEditStackData {
        match &*self.data.borrow() {
            ElementData::Open(data) => data.clone(),
            ElementData::Closed(bytes) => SingleModelEditStackData::deserialize(bytes),
        }
    }

    /// Serializa o dado (o `undo` e o `redo` do original fecham o elemento) e devolve a cópia lida.
    fn closed_data(&self) -> SingleModelEditStackData {
        self.close();
        self.current_data()
    }

    fn attached_model(&self) -> Rc<TextModel> {
        match &*self.model.borrow() {
            ElementModel::Attached(weak) => {
                if let Some(model) = weak.upgrade() {
                    return model;
                }
            }
            ElementModel::Detached => {}
        }
        // não tem modelo
        errors::throw(Error::new("Invalid SingleModelEditStackElement"))
    }

    /// `heapSize()`.
    pub fn heap_size(&self) -> usize {
        self.close();
        match &*self.data.borrow() {
            ElementData::Closed(bytes) => bytes.len() + 168, // custo fixo de heap
            ElementData::Open(_) => 168,
        }
    }
}

impl std::fmt::Display for SingleModelEditStackElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = self.current_data();
        let changes: Vec<String> = data.changes.iter().map(ToString::to_string).collect();
        write!(f, "{}", changes.join(", "))
    }
}

impl UndoRedoElement for SingleModelEditStackElement {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn code(&self) -> String {
        self.code.clone()
    }

    fn undo(&self) {
        let model = self.attached_model();
        let data = self.closed_data();
        model.apply_undo(
            &data.changes,
            data.before_eol,
            data.before_version_id,
            data.before_cursor_state.as_deref(),
        );
    }

    fn redo(&self) {
        let model = self.attached_model();
        let data = self.closed_data();
        model.apply_redo(
            &data.changes,
            data.after_eol,
            data.after_version_id,
            data.after_cursor_state.as_deref(),
        );
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `isEditStackElement` para o elemento de um modelo só.
pub fn as_edit_stack_element(element: &Rc<dyn UndoRedoElement>) -> Option<&SingleModelEditStackElement> {
    element.as_any().downcast_ref::<SingleModelEditStackElement>()
}

/// `EditStack`.
pub struct EditStack {
    model: Weak<TextModel>,
    undo_redo_service: Rc<UndoRedoService>,
}

impl EditStack {
    pub fn new(model: Weak<TextModel>, undo_redo_service: Rc<UndoRedoService>) -> Self {
        Self { model, undo_redo_service }
    }

    fn model(&self) -> Rc<TextModel> {
        self.model.upgrade().expect("o modelo está vivo enquanto a pilha de edições é usada")
    }

    pub fn push_stack_element(&self) {
        if let Some(last_element) = self.undo_redo_service.get_last_element(&self.model().uri())
            && let Some(element) = as_edit_stack_element(&last_element)
        {
            element.close();
        }
    }

    pub fn pop_stack_element(&self) {
        if let Some(last_element) = self.undo_redo_service.get_last_element(&self.model().uri())
            && let Some(element) = as_edit_stack_element(&last_element)
        {
            element.open();
        }
    }

    pub fn clear(&self) {
        self.undo_redo_service.remove_elements(&self.model().uri());
    }

    fn get_or_create_edit_stack_element(&self, model: &TextModel, before_cursor_state: Option<Vec<Selection>>, group: Option<&UndoRedoGroup>) -> Rc<dyn UndoRedoElement> {
        if let Some(last_element) = self.undo_redo_service.get_last_element(&model.uri())
            && as_edit_stack_element(&last_element).is_some_and(|element| element.can_append(model))
        {
            return last_element;
        }
        let new_element: Rc<dyn UndoRedoElement> = Rc::new(SingleModelEditStackElement::new(
            "Typing",
            "undoredo.textBufferEdit",
            model,
            before_cursor_state,
        ));
        self.undo_redo_service.push_element(new_element.clone(), group, None);
        new_element
    }

    pub fn push_eol(&self, eol: EndOfLineSequence) {
        let model = self.model();
        let edit_stack_element = self.get_or_create_edit_stack_element(&model, None, None);
        model.set_eol(eol);
        let element = as_edit_stack_element(&edit_stack_element).expect("o elemento é de um modelo só");
        element.append(Vec::new(), EndOfLineSequence::from(model.get_eol()), model.get_alternative_version_id(), None);
    }

    pub fn push_edit_operation(
        &self,
        before_cursor_state: Option<Vec<Selection>>,
        edit_operations: Vec<ValidAnnotatedEditOperation>,
        cursor_state_computer: Option<crate::model::CursorStateComputer<'_>>,
        group: Option<&UndoRedoGroup>,
        reason: Option<TextModelEditSource>,
    ) -> Option<Vec<Selection>> {
        let model = self.model();
        let edit_stack_element = self.get_or_create_edit_stack_element(&model, before_cursor_state, group);
        let reason = reason.unwrap_or_else(|| EditSources::unknown(Some("pushEditOperation")));
        let inverse_edit_operations = model
            .apply_valid_edits(edit_operations, true, reason)
            .expect("computeUndoEdits devolve as operações inversas");
        let after_cursor_state = Self::compute_cursor_state(cursor_state_computer, &inverse_edit_operations);
        let mut text_changes: Vec<(usize, &TextChange)> = inverse_edit_operations
            .iter()
            .enumerate()
            .map(|(index, op)| (index, &op.text_change))
            .collect();
        text_changes.sort_by(|a, b| a.1.old_position.cmp(&b.1.old_position).then(a.0.cmp(&b.0)));
        let element = as_edit_stack_element(&edit_stack_element).expect("o elemento é de um modelo só");
        element.append(
            text_changes.into_iter().map(|(_, change)| change.clone()).collect(),
            EndOfLineSequence::from(model.get_eol()),
            model.get_alternative_version_id(),
            after_cursor_state.clone(),
        );
        after_cursor_state
    }

    fn compute_cursor_state(cursor_state_computer: Option<crate::model::CursorStateComputer<'_>>, inverse_edit_operations: &[ValidEditOperation]) -> Option<Vec<Selection>> {
        let computer = cursor_state_computer?;
        match errors::catch(std::panic::AssertUnwindSafe(|| computer(inverse_edit_operations))) {
            Ok(selections) => selections,
            Err(error) => {
                errors::on_unexpected_error(&error);
                None
            }
        }
    }
}
