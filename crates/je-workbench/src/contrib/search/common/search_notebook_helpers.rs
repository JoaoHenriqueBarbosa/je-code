//! Porte de `src/vs/workbench/contrib/search/common/searchNotebookHelpers.ts`: os resultados por célula de um notebook
//! fechado (`INotebookCellMatchNoModel`), o prefixo do identificador de célula sem modelo e a conversão dos casamentos
//! do editor em resultados de busca de texto.
//!
//! Divergências:
//! - `INotebookFileMatchNoModel` (um `IFileMatch` com `cellResults`) é o campo `cell_results` do [`IFileMatch`], e
//!   `isINotebookFileMatchNoModel` é [`is_i_notebook_file_match_no_model`]. `IRawClosedNotebookFileMatch` (a forma com
//!   `UriComponents`, do ext host) não existe aqui.
//! - `IReadonlyTextBuffer` é o [`PieceTreeTextBuffer`]; os `Range` do `new TextSearchMatch(..., ranges)` são os
//!   [`SearchRange`].

use je_editor::model::FindMatch;
use je_editor::piece_tree_text_buffer::PieceTreeTextBuffer;

use crate::services::search::search::{IFileMatch, SearchRange, TextSearchMatch};

/// `INotebookCellMatchNoModel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct INotebookCellMatchNoModel {
    pub index: usize,
    pub content_results: Vec<TextSearchMatch>,
    pub webview_results: Vec<TextSearchMatch>,
}

/// `isINotebookFileMatchNoModel`: `'cellResults' in object`.
pub fn is_i_notebook_file_match_no_model(object: &IFileMatch) -> bool {
    object.cell_results.is_some()
}

/// `rawCellPrefix`.
pub const RAW_CELL_PREFIX: &str = "rawCell#";

/// `genericCellMatchesToTextSearchMatches`.
pub fn generic_cell_matches_to_text_search_matches(content_matches: &[FindMatch], buffer: &PieceTreeTextBuffer) -> Vec<TextSearchMatch> {
    let mut previous_end_line = -1;
    let mut context_groupings: Vec<Vec<&FindMatch>> = Vec::new();
    let mut current_context_grouping: Vec<&FindMatch> = Vec::new();

    for found in content_matches {
        if found.range.start_line_number != previous_end_line && !current_context_grouping.is_empty() {
            context_groupings.push(std::mem::take(&mut current_context_grouping));
        }

        current_context_grouping.push(found);
        previous_end_line = found.range.end_line_number;
    }

    if !current_context_grouping.is_empty() {
        context_groupings.push(current_context_grouping);
    }

    context_groupings
        .iter()
        .map(|grouping| {
            let first_line = grouping[0].range.start_line_number;
            let last_line = grouping[grouping.len() - 1].range.end_line_number;
            let mut text = je_base::js_string::JsString::new();
            for line_number in first_line..=last_line {
                text.push_units(buffer.get_line_content(line_number).units());
                text.push_unit(u16::from(b'\n'));
            }
            let ranges: Vec<SearchRange> = grouping
                .iter()
                .map(|m| {
                    SearchRange::new(
                        i64::from(m.range.start_line_number) - 1,
                        i64::from(m.range.start_column) - 1,
                        i64::from(m.range.end_line_number) - 1,
                        i64::from(m.range.end_column) - 1,
                    )
                })
                .collect();
            TextSearchMatch::new(text, &ranges, None, None)
        })
        .collect()
}
