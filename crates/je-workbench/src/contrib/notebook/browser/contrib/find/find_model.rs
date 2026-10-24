//! Porte de `src/vs/workbench/contrib/notebook/browser/contrib/find/findModel.ts`: o `CellFindMatchModel` (as ocorrências de uma célula) e o
//! `FindModel` (que liga o estado de busca ao editor, calcula as ocorrências, guarda a atual e a navega) ficam em `notebook_find_model.rs`.
//!
//! Divergências: `CellFindMatchModel implements CellFindMatchWithIndex` e a conversão para a forma de dados é
//! [`CellFindMatchModel::to_find_match_with_index`]. O tipo da célula é o parâmetro `C`: `Rc<dyn ICellViewModel>` para a busca em arquivos
//! e o `CellViewModel` para o `FindModel` do editor.

use std::rc::Rc;

use je_editor::model::FindMatch;

use crate::contrib::notebook::browser::notebook_browser::{CellFindMatchWithIndex, CellWebviewFindMatch, ICellViewModel};

/// Uma ocorrência: de conteúdo (do editor) ou de webview (da saída).
#[derive(Clone, Debug)]
pub enum CellMatch {
    Content(FindMatch),
    Webview(CellWebviewFindMatch),
}

/// `CellFindMatchModel`.
#[derive(Clone)]
pub struct CellFindMatchModel<C = Rc<dyn ICellViewModel>> {
    pub cell: C,
    pub index: usize,
    pub content_matches: Vec<FindMatch>,
    pub webview_matches: Vec<CellWebviewFindMatch>,
}

impl<C> CellFindMatchModel<C> {
    pub fn new(cell: C, index: usize, content_matches: Vec<FindMatch>, webview_matches: Vec<CellWebviewFindMatch>) -> Self {
        Self { cell, index, content_matches, webview_matches }
    }

    /// `get length()`.
    pub fn length(&self) -> usize {
        self.content_matches.len() + self.webview_matches.len()
    }

    /// `getMatch(index)`.
    pub fn get_match(&self, index: usize) -> CellMatch {
        if index >= self.length() {
            je_base::errors::throw(je_base::errors::Error::new("NotebookCellFindMatch: index out of range"));
        }

        if index < self.content_matches.len() {
            return CellMatch::Content(self.content_matches[index].clone());
        }

        CellMatch::Webview(self.webview_matches[index - self.content_matches.len()].clone())
    }
}

impl CellFindMatchModel<Rc<dyn ICellViewModel>> {
    pub fn to_find_match_with_index(&self) -> CellFindMatchWithIndex {
        CellFindMatchWithIndex {
            index: self.index,
            cell: self.cell.clone(),
            content_matches: self.content_matches.clone(),
            webview_matches: self.webview_matches.clone(),
        }
    }
}
