//! Porte de `src/vs/workbench/contrib/search/browser/searchTreeModel/match.ts`: um casamento de texto dentro de um
//! arquivo (`MatchImpl`) e a conversão de um resultado de busca em casamentos (`textSearchResultToMatches`).
//!
//! Divergências:
//! - O pai (`_parent`) é um `Weak` do arquivo (o arquivo é dono dos casamentos), e `new MatchImpl(...)` é
//!   [`MatchImpl::new`] (um `Rc`) ou [`MatchImpl::new_base`] (o estado, para quem o embute, como `MatchInNotebook`).
//! - `@memoize preview()` é o cache em `preview_cache`.
//! - O texto de prévia é [`JsString`] (as colunas contam unidades UTF-16, como no JavaScript), e o `id()` é a conversão
//!   com perda desse texto.
//! - `replaceString` lança (pânico com `Error`) se o modelo não tem `replacePattern`, como o `throw` do original.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::js_string::{JsString, substring};
use je_base::strings::lcut;
use je_editor::range::Range;

use super::search_tree_common::{ISearchTreeFileMatch, ISearchTreeMatch, MATCH_PREFIX, MatchPreview};
use crate::services::search::search::{ISearchRange, OneLineRange, TextSearchMatch};

/// `MAX_PREVIEW_CHARS`.
const MAX_PREVIEW_CHARS: i64 = 250;

/// `text.split('\n')`.
pub(crate) fn split_lines_lf(text: &JsString) -> Vec<JsString> {
    text.units().split(|&unit| unit == u16::from(b'\n')).map(JsString::from).collect()
}

/// `text.replace(/\r\n/g, '\n')`.
fn replace_crlf(text: &JsString) -> JsString {
    let units = text.units();
    let mut result: Vec<u16> = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        if units[index] == u16::from(b'\r') && units.get(index + 1) == Some(&u16::from(b'\n')) {
            index += 1;
            continue;
        }
        result.push(units[index]);
        index += 1;
    }
    JsString::from_units(result)
}

/// `text.substr(0, length)`.
fn take_units(text: JsString, length: i64) -> JsString {
    let length = usize::try_from(length.max(0)).unwrap_or(0).min(text.len());
    text.slice(0, length)
}

/// `textSearchResultToMatches`.
pub fn text_search_result_to_matches(
    raw_match: &TextSearchMatch,
    file_match: &Rc<dyn ISearchTreeFileMatch>,
    is_ai_contributed: bool,
) -> Vec<Rc<dyn ISearchTreeMatch>> {
    let preview_lines = split_lines_lf(&raw_match.preview_text);
    raw_match
        .range_locations
        .iter()
        .map(|range_location| {
            let preview_range: ISearchRange = range_location.preview;
            let match_: Rc<dyn ISearchTreeMatch> =
                MatchImpl::new(file_match, preview_lines.clone(), preview_range, range_location.source, is_ai_contributed);
            match_
        })
        .collect()
}

/// `MatchImpl`.
pub struct MatchImpl {
    parent: Weak<dyn ISearchTreeFileMatch>,
    full_preview_lines: Vec<JsString>,
    one_line_preview_text: JsString,
    range_in_preview_text: ISearchRange,
    /// For replace.
    full_preview_range: ISearchRange,
    range: Range,
    id: RefCell<String>,
    is_readonly: bool,
    preview_cache: RefCell<Option<MatchPreview>>,
}

impl MatchImpl {
    pub fn new(
        parent: &Rc<dyn ISearchTreeFileMatch>,
        full_preview_lines: Vec<JsString>,
        full_preview_range: ISearchRange,
        document_range: ISearchRange,
        is_readonly: bool,
    ) -> Rc<MatchImpl> {
        Rc::new(Self::new_base(parent, full_preview_lines, full_preview_range, document_range, is_readonly))
    }

    pub fn new_base(
        parent: &Rc<dyn ISearchTreeFileMatch>,
        full_preview_lines: Vec<JsString>,
        full_preview_range: ISearchRange,
        document_range: ISearchRange,
        is_readonly: bool,
    ) -> MatchImpl {
        let one_line_preview_text = usize::try_from(full_preview_range.start_line_number)
            .ok()
            .and_then(|index| full_preview_lines.get(index))
            .cloned()
            .unwrap_or_default();
        let adjusted_end_col = if full_preview_range.start_line_number == full_preview_range.end_line_number {
            full_preview_range.end_column
        } else {
            one_line_preview_text.len() as i64
        };
        let range_in_preview_text = OneLineRange::new(1, full_preview_range.start_column + 1, adjusted_end_col + 1);

        let range = Range::new(
            (document_range.start_line_number + 1) as i32,
            (document_range.start_column + 1) as i32,
            (document_range.end_line_number + 1) as i32,
            (document_range.end_column + 1) as i32,
        );

        let this = Self {
            parent: Rc::downgrade(parent),
            full_preview_lines,
            one_line_preview_text,
            range_in_preview_text,
            full_preview_range,
            range,
            id: RefCell::new(String::new()),
            is_readonly,
            preview_cache: RefCell::new(None),
        };
        let id = format!("{}{}>{}{}", MATCH_PREFIX, parent.resource().to_string(false).to_string_lossy(), this.range, this.get_match_string());
        this.set_id(id);
        this
    }

    /// `_id` é `protected` no original: `MatchInNotebook` o redefine.
    pub(crate) fn set_id(&self, id: String) {
        *self.id.borrow_mut() = id;
    }

    /// O pai vivo (o arquivo é dono dos casamentos, então ele existe enquanto o casamento é usado).
    pub fn file_match(&self) -> Rc<dyn ISearchTreeFileMatch> {
        self.parent.upgrade().expect("o arquivo dono do casamento está vivo")
    }

    fn compute_preview(&self) -> MatchPreview {
        let units = self.one_line_preview_text.units();
        let full_before = JsString::from(substring(units, 0, self.range_in_preview_text.start_column - 1));
        let before = lcut(full_before.units(), 26, &[0x2026]);

        let mut inside = self.get_match_string();
        let mut after = JsString::from(substring(units, self.range_in_preview_text.end_column - 1, units.len() as i64));

        let mut chars_remaining = MAX_PREVIEW_CHARS - before.len() as i64;
        inside = take_units(inside, chars_remaining);
        chars_remaining -= inside.len() as i64;
        after = take_units(after, chars_remaining);

        MatchPreview { before, full_before, inside, after }
    }
}

impl ISearchTreeMatch for MatchImpl {
    fn id(&self) -> String {
        self.id.borrow().clone()
    }

    fn parent(&self) -> Rc<dyn ISearchTreeFileMatch> {
        self.file_match()
    }

    fn text(&self) -> JsString {
        self.one_line_preview_text.clone()
    }

    fn range(&self) -> Range {
        self.range
    }

    fn preview(&self) -> MatchPreview {
        if let Some(preview) = self.preview_cache.borrow().clone() {
            return preview;
        }
        let preview = self.compute_preview();
        *self.preview_cache.borrow_mut() = Some(preview.clone());
        preview
    }

    fn replace_string(&self) -> JsString {
        let search_model = self.parent().parent().search_model();
        let Some(replace_pattern) = search_model.replace_pattern() else {
            errors::throw(Error::new("searchModel.replacePattern must be set before accessing replaceString"))
        };
        let preserve_case = search_model.preserve_case();

        let full_match_text = self.full_match_text(false);
        if let Some(replace_string) = replace_pattern.get_replace_string(full_match_text.units(), preserve_case) {
            return replace_string;
        }

        // Search/find normalize line endings - check whether \r prevents regex from matching
        let full_match_text_without_cr = replace_crlf(&full_match_text);
        if full_match_text_without_cr != full_match_text
            && let Some(replace_string) = replace_pattern.get_replace_string(full_match_text_without_cr.units(), preserve_case)
        {
            return replace_string;
        }

        // If match string is not matching then regex pattern has a lookahead expression
        let context_match_text_with_surrounding_content = self.full_match_text(true);
        if let Some(replace_string) = replace_pattern.get_replace_string(context_match_text_with_surrounding_content.units(), preserve_case) {
            return replace_string;
        }

        // Search/find normalize line endings, this time in full context
        let context_match_text_without_cr = replace_crlf(&context_match_text_with_surrounding_content);
        if context_match_text_without_cr != context_match_text_with_surrounding_content
            && let Some(replace_string) = replace_pattern.get_replace_string(context_match_text_without_cr.units(), preserve_case)
        {
            return replace_string;
        }

        // Match string is still not matching. Could be unsupported matches (multi-line).
        replace_pattern.pattern().clone()
    }

    fn full_match_text(&self, include_surrounding: bool) -> JsString {
        let this_match_preview_lines: Vec<JsString> = if include_surrounding {
            self.full_preview_lines.clone()
        } else {
            let len = self.full_preview_lines.len();
            let end = usize::try_from(self.full_preview_range.end_line_number + 1).unwrap_or(0).min(len);
            let start = usize::try_from(self.full_preview_range.start_line_number).unwrap_or(0).min(end);
            let mut lines: Vec<JsString> = self.full_preview_lines[start..end].to_vec();
            if let Some(last) = lines.last_mut() {
                let end_column = usize::try_from(self.full_preview_range.end_column).unwrap_or(0).min(last.len());
                *last = last.slice(0, end_column);
            }
            if let Some(first) = lines.first_mut() {
                let start_column = usize::try_from(self.full_preview_range.start_column).unwrap_or(0).min(first.len());
                *first = first.slice(start_column, first.len());
            }
            lines
        };

        let mut joined = JsString::new();
        for (index, line) in this_match_preview_lines.iter().enumerate() {
            if index > 0 {
                joined.push_unit(u16::from(b'\n'));
            }
            joined.push_units(line.units());
        }
        joined
    }

    fn range_in_preview(&self) -> ISearchRange {
        // convert to editor's base 1 positions.
        ISearchRange {
            start_column: self.full_preview_range.start_column + 1,
            end_column: self.full_preview_range.end_column + 1,
            ..self.full_preview_range
        }
    }

    fn full_preview_lines(&self) -> Vec<JsString> {
        let len = self.full_preview_lines.len();
        let end = usize::try_from(self.full_preview_range.end_line_number + 1).unwrap_or(0).min(len);
        let start = usize::try_from(self.full_preview_range.start_line_number).unwrap_or(0).min(end);
        self.full_preview_lines[start..end].to_vec()
    }

    fn get_match_string(&self) -> JsString {
        JsString::from(substring(
            self.one_line_preview_text.units(),
            self.range_in_preview_text.start_column - 1,
            self.range_in_preview_text.end_column - 1,
        ))
    }

    fn is_readonly(&self) -> bool {
        self.is_readonly
    }
}
