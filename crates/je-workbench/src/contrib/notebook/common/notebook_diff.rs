//! Porte de `src/vs/workbench/contrib/notebook/common/notebookDiff.ts`: o mapeamento das células de um notebook
//! original e de um modificado (`computeDiff`), a partir das mudanças que o `LcsDiff` achou.
//!
//! Divergências:
//! - `CellDiffInfo` (união discriminada por `type`) é o enum [`CellDiffInfo`]; os índices são `usize`.
//! - `ICell` e `{ readonly cells: readonly ICell[] }` são o trait [`ICell`] e uma fatia de `Rc<dyn ICell>`: as células
//!   são compartilhadas com o dono (o worker de notebook lê o hash delas depois de mudar os metadados internos).
//! - `firstChangeIndex` é `i64` (`-1` quando nenhuma célula mudou).
//! - O `NotebookWorker` (`services/notebookWebWorker.ts`) repete `CellDiffInfo` e `ICell` por cópia; o porte usa os
//!   daqui (DRY).

use std::rc::Rc;

use je_base::diff::DiffChange;

use super::model::notebook_cell_text_model::NotebookCellTextModel;
use super::notebook_common::{CellKind, ICell as CommonCell, INotebookDiffResult};

/// `CellDiffInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellDiffInfo {
    Unchanged { original_cell_index: usize, modified_cell_index: usize },
    Modified { original_cell_index: usize, modified_cell_index: usize },
    Delete { original_cell_index: usize },
    Insert { modified_cell_index: usize },
}

/// `ICell`.
pub trait ICell {
    fn cell_kind(&self) -> CellKind;
    fn get_hash_value(&self) -> i32;
    fn equal(&self, cell: &dyn ICell) -> bool;

    /// O `NotebookCellTextModel` por trás da célula: o `equal(b: NotebookCellTextModel)` do original compara com outro
    /// `NotebookCellTextModel` (o TypeScript aceita pela tipagem estrutural), e o Rust precisa do tipo concreto.
    fn as_notebook_cell_text_model(&self) -> Option<&NotebookCellTextModel> {
        None
    }
}

/// O `NotebookCellTextModel` como a célula de `computeDiff` (o `INotebookTextModel.cells` do `NotebookDiffViewModel`).
impl ICell for NotebookCellTextModel {
    fn cell_kind(&self) -> CellKind {
        CommonCell::cell_kind(self)
    }

    fn get_hash_value(&self) -> i32 {
        CommonCell::get_hash_value(self)
    }

    fn equal(&self, cell: &dyn ICell) -> bool {
        cell.as_notebook_cell_text_model().is_some_and(|other| NotebookCellTextModel::equal(self, other))
    }

    fn as_notebook_cell_text_model(&self) -> Option<&NotebookCellTextModel> {
        Some(self)
    }
}

/// O `{ cellDiffInfo, firstChangeIndex }` que `computeDiff` devolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputedDiff {
    pub cell_diff_info: Vec<CellDiffInfo>,
    pub first_change_index: i64,
}

/// `computeDiff`.
pub fn compute_diff(original_cells: &[Rc<dyn ICell>], modified_cells: &[Rc<dyn ICell>], diff_result: &INotebookDiffResult) -> ComputedDiff {
    let cell_changes = &diff_result.cells_diff.changes;
    let mut cell_diff_info: Vec<CellDiffInfo> = Vec::new();
    let mut original_cell_index: i64 = 0;
    let mut modified_cell_index: i64 = 0;

    let mut first_change_index: i64 = -1;

    for change in cell_changes {
        // common cells
        for j in 0..(i64::from(change.original_start) - original_cell_index).max(0) {
            let original_cell = &original_cells[(original_cell_index + j) as usize];
            let modified_cell = &modified_cells[(modified_cell_index + j) as usize];
            let original = (original_cell_index + j) as usize;
            let modified = (modified_cell_index + j) as usize;
            if original_cell.get_hash_value() == modified_cell.get_hash_value() {
                cell_diff_info.push(CellDiffInfo::Unchanged { original_cell_index: original, modified_cell_index: modified });
            } else {
                if first_change_index == -1 {
                    first_change_index = cell_diff_info.len() as i64;
                }
                cell_diff_info.push(CellDiffInfo::Modified { original_cell_index: original, modified_cell_index: modified });
            }
        }

        let modified_lcs = compute_modified_lcs(change, original_cells, modified_cells);
        if !modified_lcs.is_empty() && first_change_index == -1 {
            first_change_index = cell_diff_info.len() as i64;
        }

        cell_diff_info.extend(modified_lcs);
        original_cell_index = i64::from(change.original_start) + i64::from(change.original_length);
        modified_cell_index = i64::from(change.modified_start) + i64::from(change.modified_length);
    }

    for i in original_cell_index.max(0) as usize..original_cells.len() {
        cell_diff_info.push(CellDiffInfo::Unchanged {
            original_cell_index: i,
            modified_cell_index: (i as i64 - original_cell_index + modified_cell_index) as usize,
        });
    }

    ComputedDiff { cell_diff_info, first_change_index }
}

fn compute_modified_lcs(change: &DiffChange, original_cells: &[Rc<dyn ICell>], modified_cells: &[Rc<dyn ICell>]) -> Vec<CellDiffInfo> {
    let mut result: Vec<CellDiffInfo> = Vec::new();
    let original_start = change.original_start as usize;
    let modified_start = change.modified_start as usize;
    let original_length = change.original_length as usize;
    let modified_length = change.modified_length as usize;
    // modified cells
    let modified_len = original_length.min(modified_length);

    for j in 0..modified_len {
        let original_cell = &original_cells[original_start + j];
        let modified_cell = &modified_cells[modified_start + j];
        if original_cell.cell_kind() != modified_cell.cell_kind() {
            result.push(CellDiffInfo::Delete { original_cell_index: original_start + j });
            result.push(CellDiffInfo::Insert { modified_cell_index: modified_start + j });
        } else {
            let is_the_same = original_cell.equal(modified_cell.as_ref());
            let (original_cell_index, modified_cell_index) = (original_start + j, modified_start + j);
            result.push(if is_the_same {
                CellDiffInfo::Unchanged { original_cell_index, modified_cell_index }
            } else {
                CellDiffInfo::Modified { original_cell_index, modified_cell_index }
            });
        }
    }

    for j in modified_len..original_length {
        // deletion
        result.push(CellDiffInfo::Delete { original_cell_index: original_start + j });
    }

    for j in modified_len..modified_length {
        result.push(CellDiffInfo::Insert { modified_cell_index: modified_start + j });
    }

    result
}
