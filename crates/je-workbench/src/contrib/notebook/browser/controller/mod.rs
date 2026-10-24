//! Porte de `src/vs/workbench/contrib/notebook/browser/controller/coreActions.ts` e do que `cellOperations.ts` lê do editor: os
//! contextos de ação (`INotebookActionContext`, `INotebookCellActionContext`) e o contrato do editor ativo
//! (`IActiveNotebookEditor`).
//!
//! Divergências:
//! - `IActiveNotebookEditor` (uma interface enorme do widget) é o trait [`IActiveNotebookEditor`] só com os membros que
//!   `cellOperations.ts` lê; `hasModel()` é sempre verdadeiro num editor que implementa o trait.
//! - `INotebookActionContext` é uma struct com `cell`, `ui`, `selected_cells` opcionais; `INotebookCellActionContext` (com
//!   `cell` obrigatório) é a mesma struct, e quem a exige lê `cell` com `expect`.
//! - `activeKernel?.supportedLanguages` é [`IActiveNotebookEditor::active_kernel_supported_languages`] (`None` sem kernel).
//! - `focusNotebookCell` é síncrono (o `await` do original só espera a criação do editor de código da célula).

use std::rc::Rc;

use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, NotebookViewModel};
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_range::ICellRange;

pub mod cell_operations;

/// `IActiveNotebookEditor`, só com o que `cellOperations.ts` lê.
pub trait IActiveNotebookEditor {
    /// `getId()`.
    fn get_id(&self) -> String;
    fn text_model(&self) -> Rc<NotebookTextModel>;
    fn is_read_only(&self) -> bool;
    fn get_selections(&self) -> Vec<ICellRange>;
    fn get_focus(&self) -> ICellRange;
    fn get_length(&self) -> usize;
    fn cell_at(&self, index: i64) -> Option<CellViewModel>;
    fn get_cell_index(&self, cell: &CellViewModel) -> i64;
    fn get_cells_in_range(&self, range: Option<ICellRange>) -> Vec<CellViewModel>;
    fn get_view_model(&self) -> Rc<NotebookViewModel>;
    fn get_view_index_by_model_index(&self, index: i64) -> i64;
    fn get_cell_range_from_view_range(&self, start_index: i64, end_index: i64) -> Option<ICellRange>;
    fn reveal_cell_range_in_view(&self, range: ICellRange);
    /// `focusNotebookCell(cell, focusItem)` com `focusItem` em `'editor' | 'container' | 'output'`.
    fn focus_notebook_cell(&self, cell: &CellViewModel, focus_item: &str);
    fn active_kernel_supported_languages(&self) -> Option<Vec<String>>;
}

/// `INotebookActionContext` (e `INotebookCellActionContext`).
#[derive(Clone)]
pub struct INotebookActionContext {
    pub notebook_editor: Rc<dyn IActiveNotebookEditor>,
    pub cell: Option<CellViewModel>,
    pub ui: Option<bool>,
    pub selected_cells: Option<Vec<CellViewModel>>,
}

impl INotebookActionContext {
    pub fn new(notebook_editor: Rc<dyn IActiveNotebookEditor>) -> Self {
        Self { notebook_editor, cell: None, ui: None, selected_cells: None }
    }

    pub fn with_cell(mut self, cell: CellViewModel) -> Self {
        self.cell = Some(cell);
        self
    }

    pub fn with_ui(mut self, ui: bool) -> Self {
        self.ui = Some(ui);
        self
    }
}

/// `INotebookCellActionContext`: o mesmo contexto, com `cell` obrigatório.
pub type INotebookCellActionContext = INotebookActionContext;

/// `expandCellRangesWithHiddenCells(editor, ranges)` de `notebookBrowser.ts`.
pub fn expand_cell_ranges_with_hidden_cells(editor: &dyn IActiveNotebookEditor, ranges: &[ICellRange]) -> Vec<ICellRange> {
    // assuming ranges are sorted and no overlap
    let indexes = crate::contrib::notebook::common::notebook_range::cell_ranges_to_indexes(ranges);
    let mut model_ranges: Vec<ICellRange> = Vec::new();
    for index in indexes {
        if editor.cell_at(index).is_none() {
            continue;
        }

        let view_index = editor.get_view_index_by_model_index(index);
        if view_index < 0 {
            continue;
        }

        let next_view_index = view_index + 1;
        if let Some(range) = editor.get_cell_range_from_view_range(view_index, next_view_index) {
            model_ranges.push(range);
        }
    }

    crate::contrib::notebook::common::notebook_range::reduce_cell_ranges(&model_ranges)
}
