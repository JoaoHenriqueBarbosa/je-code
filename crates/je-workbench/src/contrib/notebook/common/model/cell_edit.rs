//! Porte de `src/vs/workbench/contrib/notebook/common/model/cellEdit.ts`: os elementos de desfazer e refazer das
//! edições de célula do notebook (mover, trocar células e mudar os metadados de uma célula) e o contrato do delegado
//! que aplica a edição sem mexer na pilha (`ITextCellEditingDelegate`).
//!
//! Divergências:
//! - `ITextCellEditingDelegate` (uma interface com métodos opcionais) é um struct de fechos opcionais, com `Default`
//!   para o delegado vazio; cada fecho recebe `index` como `usize` e `endSelections?` como `Option<ISelectionState>`.
//! - `IResourceUndoRedoElement` é o [`UndoRedoElement`] do je_editor; o `type` fixo (`UndoRedoElementType.Resource`) é o
//!   próprio trait, e `undo` e `redo` são síncronos (o `redo` de `CellMetadataEdit` devolve `void | Promise<void>` e
//!   aqui devolve `()`).
//! - Os `private` do original que o construtor recebe (`fromIndex`, `length`, `diffs`...) são campos privados; os
//!   `public resource` e os `readonly` do `CellMetadataEdit` são campos públicos.
//! - `SpliceCellsEdit.redo` faz `this.diffs.reverse()`, que inverte o array no lugar e portanto muda o que o `undo`
//!   seguinte vê; `diffs` é um `RefCell<Vec<...>>` e a inversão é no lugar.
//! - `throw new Error(...)` é [`errors::throw`].

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::uri::URI;
use je_editor::undo_redo::UndoRedoElement;

use super::notebook_cell_text_model::NotebookCellTextModel;
use crate::contrib::notebook::common::notebook_common::{ISelectionState, NotebookCellMetadata};

/// `ITextCellEditingDelegate`: it should not modify Undo/Redo stack.
#[derive(Default)]
pub struct ITextCellEditingDelegate {
    pub insert_cell: Option<Box<dyn Fn(usize, Rc<NotebookCellTextModel>, Option<ISelectionState>)>>,
    pub delete_cell: Option<Box<dyn Fn(usize, Option<ISelectionState>)>>,
    pub replace_cell: Option<Box<dyn Fn(usize, usize, Vec<Rc<NotebookCellTextModel>>, Option<ISelectionState>)>>,
    /// `moveCell(fromIndex, length, toIndex, beforeSelections, endSelections?)`.
    pub move_cell: Option<Box<dyn Fn(usize, usize, usize, Option<ISelectionState>, Option<ISelectionState>)>>,
    pub update_cell_metadata: Option<Box<dyn Fn(usize, NotebookCellMetadata)>>,
}

pub struct MoveCellEdit {
    pub resource: URI,
    from_index: usize,
    length: usize,
    to_index: usize,
    editing_delegate: ITextCellEditingDelegate,
    befored_selections: Option<ISelectionState>,
    end_selections: Option<ISelectionState>,
}

impl MoveCellEdit {
    pub fn new(
        resource: URI,
        from_index: usize,
        length: usize,
        to_index: usize,
        editing_delegate: ITextCellEditingDelegate,
        befored_selections: Option<ISelectionState>,
        end_selections: Option<ISelectionState>,
    ) -> Self {
        Self { resource, from_index, length, to_index, editing_delegate, befored_selections, end_selections }
    }
}

impl UndoRedoElement for MoveCellEdit {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn label(&self) -> String {
        let label = if self.length == 1 { "Move Cell" } else { "Move Cells" };
        label.to_string()
    }

    fn code(&self) -> String {
        "undoredo.textBufferEdit".to_string()
    }

    fn undo(&self) {
        let Some(move_cell) = &self.editing_delegate.move_cell else {
            errors::throw(Error::new("Notebook Move Cell not implemented for Undo/Redo"));
        };

        move_cell(self.to_index, self.length, self.from_index, self.end_selections.clone(), self.befored_selections.clone());
    }

    fn redo(&self) {
        let Some(move_cell) = &self.editing_delegate.move_cell else {
            errors::throw(Error::new("Notebook Move Cell not implemented for Undo/Redo"));
        };

        move_cell(self.from_index, self.length, self.to_index, self.befored_selections.clone(), self.end_selections.clone());
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `[start, deletedCells, insertedCells]`.
pub type SpliceCellsDiff = (usize, Vec<Rc<NotebookCellTextModel>>, Vec<Rc<NotebookCellTextModel>>);

pub struct SpliceCellsEdit {
    pub resource: URI,
    diffs: RefCell<Vec<SpliceCellsDiff>>,
    editing_delegate: ITextCellEditingDelegate,
    before_handles: Option<ISelectionState>,
    end_handles: Option<ISelectionState>,
}

impl SpliceCellsEdit {
    pub fn new(
        resource: URI,
        diffs: Vec<SpliceCellsDiff>,
        editing_delegate: ITextCellEditingDelegate,
        before_handles: Option<ISelectionState>,
        end_handles: Option<ISelectionState>,
    ) -> Self {
        Self { resource, diffs: RefCell::new(diffs), editing_delegate, before_handles, end_handles }
    }
}

impl UndoRedoElement for SpliceCellsEdit {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn label(&self) -> String {
        // Compute the most appropriate labels
        let diffs = self.diffs.borrow();
        if diffs.len() == 1 && diffs[0].1.is_empty() {
            return if diffs[0].2.len() > 1 { "Insert Cells" } else { "Insert Cell" }.to_string();
        }
        if diffs.len() == 1 && diffs[0].2.is_empty() {
            return if diffs[0].1.len() > 1 { "Delete Cells" } else { "Delete Cell" }.to_string();
        }
        // Default to Insert Cell
        "Insert Cell".to_string()
    }

    fn code(&self) -> String {
        "undoredo.textBufferEdit".to_string()
    }

    fn undo(&self) {
        let Some(replace_cell) = &self.editing_delegate.replace_cell else {
            errors::throw(Error::new("Notebook Replace Cell not implemented for Undo/Redo"));
        };

        let diffs = self.diffs.borrow().clone();
        for diff in diffs {
            replace_cell(diff.0, diff.2.len(), diff.1, self.before_handles.clone());
        }
    }

    fn redo(&self) {
        let Some(replace_cell) = &self.editing_delegate.replace_cell else {
            errors::throw(Error::new("Notebook Replace Cell not implemented for Undo/Redo"));
        };

        // `this.diffs.reverse()` inverte no lugar
        self.diffs.borrow_mut().reverse();
        let diffs = self.diffs.borrow().clone();
        for diff in diffs {
            replace_cell(diff.0, diff.1.len(), diff.2, self.end_handles.clone());
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct CellMetadataEdit {
    pub resource: URI,
    pub index: usize,
    pub old_metadata: NotebookCellMetadata,
    pub new_metadata: NotebookCellMetadata,
    editing_delegate: ITextCellEditingDelegate,
}

impl CellMetadataEdit {
    pub fn new(
        resource: URI,
        index: usize,
        old_metadata: NotebookCellMetadata,
        new_metadata: NotebookCellMetadata,
        editing_delegate: ITextCellEditingDelegate,
    ) -> Self {
        Self { resource, index, old_metadata, new_metadata, editing_delegate }
    }
}

impl UndoRedoElement for CellMetadataEdit {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn label(&self) -> String {
        "Update Cell Metadata".to_string()
    }

    fn code(&self) -> String {
        "undoredo.textBufferEdit".to_string()
    }

    fn undo(&self) {
        let Some(update_cell_metadata) = &self.editing_delegate.update_cell_metadata else {
            return;
        };

        update_cell_metadata(self.index, self.old_metadata.clone());
    }

    fn redo(&self) {
        let Some(update_cell_metadata) = &self.editing_delegate.update_cell_metadata else {
            return;
        };

        update_cell_metadata(self.index, self.new_metadata.clone());
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
