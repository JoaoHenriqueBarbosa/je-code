//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/contrib/find/findModel.ts`: só `CellFindMatchModel`, que
//! `notebookSearchModel.ts` monta para decorar as ocorrências de uma célula.
//!
//! Divergências: `CellFindMatchModel implements CellFindMatchWithIndex` e a conversão para a forma de dados é
//! [`CellFindMatchModel::to_find_match_with_index`].

use std::rc::Rc;

use je_editor::model::FindMatch;

use crate::contrib::notebook::browser::notebook_browser::{CellFindMatchWithIndex, CellWebviewFindMatch, ICellViewModel};

/// `CellFindMatchModel`.
pub struct CellFindMatchModel {
    pub cell: Rc<dyn ICellViewModel>,
    pub index: usize,
    pub content_matches: Vec<FindMatch>,
    pub webview_matches: Vec<CellWebviewFindMatch>,
}

impl CellFindMatchModel {
    pub fn new(cell: Rc<dyn ICellViewModel>, index: usize, content_matches: Vec<FindMatch>, webview_matches: Vec<CellWebviewFindMatch>) -> Self {
        Self { cell, index, content_matches, webview_matches }
    }

    pub fn to_find_match_with_index(&self) -> CellFindMatchWithIndex {
        CellFindMatchWithIndex {
            index: self.index,
            cell: self.cell.clone(),
            content_matches: self.content_matches.clone(),
            webview_matches: self.webview_matches.clone(),
        }
    }
}
