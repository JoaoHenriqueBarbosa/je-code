//! Porte de `src/vs/workbench/contrib/search/browser/notebookSearch/searchNotebookHelpers.ts`: os resultados por célula de
//! um notebook aberto (`INotebookCellMatchWithModel`) e as conversões dos casamentos do editor e do webview em resultados
//! de busca de texto.
//!
//! Divergências:
//! - `INotebookCellMatch = INotebookCellMatchWithModel | INotebookCellMatchNoModel` é o enum [`INotebookCellMatch`];
//!   `INotebookFileMatchWithModel` (um `IFileMatch` com `cellResults`) é o próprio [`IFileMatch`], e
//!   `isINotebookFileMatchWithModel` olha se todas as células do `cell_results` têm modelo (vale para a lista vazia, como o
//!   `every` do original).
//! - `INotebookCellMatchWithModel extends INotebookCellMatchNoModel` tem os quatro campos planos.

use std::rc::Rc;

use je_base::js_string::JsString;
use je_editor::model::FindMatch;

use crate::contrib::notebook::browser::notebook_browser::{CellWebviewFindMatch, ICellViewModel};
use crate::contrib::search::common::search_notebook_helpers::{
    INotebookCellMatchNoModel, RAW_CELL_PREFIX, generic_cell_matches_to_text_search_matches,
};
use crate::services::search::search::{IFileMatch, SearchRange, TextSearchMatch};

/// `INotebookCellMatchWithModel`.
#[derive(Clone)]
pub struct INotebookCellMatchWithModel {
    pub index: usize,
    pub content_results: Vec<TextSearchMatch>,
    pub webview_results: Vec<TextSearchMatch>,
    pub cell: Rc<dyn ICellViewModel>,
}

/// `INotebookCellMatch`.
#[derive(Clone)]
pub enum INotebookCellMatch {
    WithModel(INotebookCellMatchWithModel),
    NoModel(INotebookCellMatchNoModel),
}

impl INotebookCellMatch {
    pub fn index(&self) -> usize {
        match self {
            INotebookCellMatch::WithModel(cell_match) => cell_match.index,
            INotebookCellMatch::NoModel(cell_match) => cell_match.index,
        }
    }

    pub fn content_results(&self) -> &[TextSearchMatch] {
        match self {
            INotebookCellMatch::WithModel(cell_match) => &cell_match.content_results,
            INotebookCellMatch::NoModel(cell_match) => &cell_match.content_results,
        }
    }

    pub fn webview_results(&self) -> &[TextSearchMatch] {
        match self {
            INotebookCellMatch::WithModel(cell_match) => &cell_match.webview_results,
            INotebookCellMatch::NoModel(cell_match) => &cell_match.webview_results,
        }
    }
}

/// `getIDFromINotebookCellMatch`.
pub fn get_id_from_i_notebook_cell_match(match_: &INotebookCellMatch) -> String {
    match match_ {
        INotebookCellMatch::WithModel(cell_match) => cell_match.cell.id(),
        INotebookCellMatch::NoModel(cell_match) => format!("{RAW_CELL_PREFIX}{}", cell_match.index),
    }
}

/// `isINotebookFileMatchWithModel`.
pub fn is_i_notebook_file_match_with_model(object: &IFileMatch) -> bool {
    object.cell_results.as_ref().is_some_and(|cell_results| cell_results.iter().all(is_i_notebook_cell_match_with_model))
}

/// `isINotebookCellMatchWithModel`.
pub fn is_i_notebook_cell_match_with_model(object: &INotebookCellMatch) -> bool {
    matches!(object, INotebookCellMatch::WithModel(_))
}

/// `contentMatchesToTextSearchMatches`.
pub fn content_matches_to_text_search_matches(content_matches: &[FindMatch], cell: &Rc<dyn ICellViewModel>) -> Vec<TextSearchMatch> {
    let text_buffer = cell.text_buffer();
    let text_buffer = text_buffer.borrow();
    generic_cell_matches_to_text_search_matches(content_matches, &text_buffer)
}

/// `webviewMatchesToTextSearchMatches`.
pub fn webview_matches_to_text_search_matches(webview_matches: &[CellWebviewFindMatch]) -> Vec<TextSearchMatch> {
    webview_matches
        .iter()
        .filter_map(|raw_match| {
            raw_match.search_preview_info.as_ref().map(|info| {
                TextSearchMatch::new(
                    JsString::from(info.line.units()),
                    &[SearchRange::new(0, info.range.start, 0, info.range.end)],
                    None,
                    Some(raw_match.index),
                )
            })
        })
        .collect()
}
