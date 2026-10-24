//! Porte de `src/vs/workbench/services/search/common/searchHelpers.ts`: converte os casamentos do editor
//! (`FindMatch`) em resultados de busca de texto e acrescenta as linhas de contexto de um modelo.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `ITextModel` entra pelo trait [`SearchTextModel`], com só os dois métodos que o arquivo lê
//!   (`getLineContent` e `getLineCount`): o original também só os usa, e é o que o teste simula. O
//!   [`TextModel`] o implementa.
//! - linhas e colunas do editor são `i32`; as do resultado de busca são `i64` (como em
//!   [`SearchRange`](super::search::SearchRange)).
//! - `ITextSearchMatch[]` é `&[TextSearchMatch]` (o único tipo que implementa a interface aqui) e o
//!   `ITextSearchQuery` é o enum [`ITextSearchQuery`]; só o `surroundingContext` dele é lido.
//! - `previewOptions?` é `Option<&ITextSearchPreviewOptions>`.

use je_base::js_string::JsString;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::model::FindMatch;
use je_editor::text_model::TextModel;

use super::search::{
    ITextSearchContext, ITextSearchPreviewOptions, ITextSearchQuery, ITextSearchResult, SearchRange, TextSearchMatch,
};

/// O pedaço do `ITextModel` que o `searchHelpers` lê.
pub trait SearchTextModel {
    fn get_line_content(&self, line_number: i32) -> JsString;
    fn get_line_count(&self) -> i32;
}

impl SearchTextModel for TextModel {
    fn get_line_content(&self, line_number: i32) -> JsString {
        CursorSimpleModel::get_line_content(self, line_number)
    }

    fn get_line_count(&self) -> i32 {
        CursorSimpleModel::get_line_count(self)
    }
}

fn editor_match_to_text_search_result(
    matches: &[FindMatch],
    model: &dyn SearchTextModel,
    preview_options: Option<&ITextSearchPreviewOptions>,
) -> TextSearchMatch {
    let first_line = matches[0].range.start_line_number;
    let last_line = matches[matches.len() - 1].range.end_line_number;

    // `lineTexts.join('\n') + '\n'`
    let mut text = JsString::new();
    for line_number in first_line..=last_line {
        text.push_units(model.get_line_content(line_number).units());
        text.push_unit(u16::from(b'\n'));
    }

    let ranges: Vec<SearchRange> = matches
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
    TextSearchMatch::new(text, &ranges, preview_options, None)
}

/// Combina um conjunto de `FindMatch` em um conjunto de `TextSearchMatch`. Eles são agrupados pelos casamentos
/// que começam na linha em que o casamento anterior termina.
pub fn editor_matches_to_text_search_results(
    matches: &[FindMatch],
    model: &dyn SearchTextModel,
    preview_options: Option<&ITextSearchPreviewOptions>,
) -> Vec<TextSearchMatch> {
    let mut previous_end_line = -1;
    let mut grouped_matches: Vec<Vec<FindMatch>> = Vec::new();
    for found in matches {
        if found.range.start_line_number != previous_end_line {
            grouped_matches.push(Vec::new());
        }

        grouped_matches.last_mut().expect("o grupo acabou de ser criado").push(found.clone());
        previous_end_line = found.range.end_line_number;
    }

    grouped_matches
        .iter()
        .map(|same_line_matches| editor_match_to_text_search_result(same_line_matches, model, preview_options))
        .collect()
}

pub fn get_text_search_match_with_model_context(
    matches: &[TextSearchMatch],
    model: &dyn SearchTextModel,
    query: &ITextSearchQuery,
) -> Vec<ITextSearchResult> {
    let mut results: Vec<ITextSearchResult> = Vec::new();
    // `typeof query.surroundingContext === 'number' && query.surroundingContext > 0`
    let surrounding_context = query.surrounding_context().filter(|&context| context > 0);
    let context_line = |zero_based_line: i64| {
        ITextSearchResult::Context(ITextSearchContext {
            text: model.get_line_content((zero_based_line + 1) as i32),
            line_number: zero_based_line + 1,
        })
    };

    let mut prev_line: i64 = -1;
    for (i, found) in matches.iter().enumerate() {
        let (match_start_line, match_end_line) = get_match_start_end(found);
        if let Some(context) = surrounding_context {
            let before_context_start_line = (prev_line + 1).max(match_start_line - context);
            for b in before_context_start_line..match_start_line {
                results.push(context_line(b));
            }
        }

        results.push(ITextSearchResult::Match(found.clone()));

        // `Number.MAX_VALUE` quando não há próximo casamento.
        let next_match_start_line = matches.get(i + 1).map_or(i64::MAX, |next| get_match_start_end(next).0);
        if let Some(context) = surrounding_context {
            let after_context_to_line = (next_match_start_line - 1)
                .min(match_end_line + context)
                .min(i64::from(model.get_line_count()) - 1);
            for a in (match_end_line + 1)..=after_context_to_line {
                results.push(context_line(a));
            }
        }

        prev_line = match_end_line;
    }

    results
}

/// A linha inicial do primeiro intervalo de origem e a final do último.
fn get_match_start_end(found: &TextSearchMatch) -> (i64, i64) {
    let match_ranges: Vec<SearchRange> = found.range_locations.iter().map(|e| e.source).collect();
    (match_ranges[0].start_line_number, match_ranges[match_ranges.len() - 1].end_line_number)
}
