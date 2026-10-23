//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/computeGhostText.ts`, com
//! `singleTextRemoveCommonPrefix` e `singleTextEditAugments` de `singleTextEditHelpers.ts` (a única dependência
//! do arquivo que ainda não estava portada).
//!
//! Notas de tradução:
//! - Texto é UTF-16 (`JsString`); `substring` é `je_base::js_string::substring` (mesmo limite e troca de índices).
//! - `mode: 'prefix' | 'subword' | 'subwordSmart'` é [`GhostTextMode`].
//! - `lastRequest` (cache de módulo do `cachingDiff`) é um `thread_local` com `RefCell`.
//! - `getElements` devolve um `Int32Array`; aqui é `Vec<i32>`, entregue ao `LcsDiff` por `IntSequence`.
//! - `IDiffChange` é `DiffChange` de `je_base::diff`.
//! - `previewSuffixLength = 0` (parâmetro com padrão) é um argumento explícito.

use crate::cursor_common::CursorSimpleModel;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::diff::{DiffChange, ISequence, LcsDiff, SequenceElements};
use je_base::js_string::substring;
use je_base::strings::{common_prefix_length, get_leading_whitespace};

use crate::edits::string_edit::replace_all_units;
use crate::edits::text_edit::TextReplacement;
use crate::inline_completions::ghost_text::{GhostText, GhostTextPart};
use crate::model::EndOfLinePreference;
use crate::position::Position;
use crate::position_to_offset::TextLength;
use crate::range::Range;
use crate::text_model::TextModel;

/// `'prefix' | 'subword' | 'subwordSmart'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GhostTextMode {
    Prefix,
    Subword,
    SubwordSmart,
}

/// `singleTextRemoveCommonPrefix(edit, model, validModelRange?)`.
pub fn single_text_remove_common_prefix(
    edit: &TextReplacement,
    model: &TextModel,
    valid_model_range: Option<&Range>,
) -> TextReplacement {
    let model_range = match valid_model_range {
        Some(valid) => edit.range.intersect_ranges(valid),
        None => Some(edit.range),
    };
    let Some(model_range) = model_range else {
        return edit.clone();
    };

    // `edit.text.replaceAll('\r\n', '\n')`
    let normalized = replace_all_units(edit.text.units(), &[13, 10], &[10]);
    let value_to_replace = model.get_value_in_range_with(&model_range, EndOfLinePreference::Lf);
    let common_prefix_len = common_prefix_length(value_to_replace.units(), &normalized);
    let prefix = JsString::from(&value_to_replace.units()[..common_prefix_len]);
    let start = TextLength::of_text(&prefix.to_string_lossy()).add_to_position(edit.range.get_start_position());
    let text = JsString::from(&normalized[common_prefix_len..]);
    let range = Range::from_positions(start, edit.range.get_end_position());
    TextReplacement::new(range, text)
}

/// `singleTextEditAugments(edit, base)`.
pub fn single_text_edit_augments(edit: &TextReplacement, base: &TextReplacement) -> bool {
    // The augmented completion must replace the base range, but can replace even more
    edit.text.units().starts_with(base.text.units()) && range_extends(&edit.range, &base.range)
}

fn range_extends(extending_range: &Range, range_to_extend: &Range) -> bool {
    range_to_extend.get_start_position() == extending_range.get_start_position()
        && range_to_extend.get_end_position().is_before_or_equal(&extending_range.get_end_position())
}

/// `previewSuffixLength` define onde dividir `inlineCompletion.text`: com o texto `hello` e o sufixo 2, a parte
/// que não é prévia é `hel` e a prévia é `lo`.
pub fn compute_ghost_text(
    edit: &TextReplacement,
    model: &TextModel,
    mode: GhostTextMode,
    cursor_position: Option<Position>,
    preview_suffix_length: i32,
) -> Option<GhostText> {
    let mut e = single_text_remove_common_prefix(edit, model, None);

    if e.range.end_line_number != e.range.start_line_number {
        // This edit might span multiple lines, but the first lines must be a common prefix.
        return None;
    }

    let source_line = model.get_line_content(e.range.start_line_number);
    let source_indentation_length = get_leading_whitespace(source_line.units(), 0, source_line.len()).len() as i32;

    let suggestion_touches_indentation = e.range.start_column - 1 <= source_indentation_length;
    if suggestion_touches_indentation {
        // source:      ··········[······abc]
        //                         ^^^^^^^^^ inlineCompletion.range
        //              ^^^^^^^^^^ ^^^^^^ sourceIndentationLength
        //                         ^^^^^^ replacedIndentation.length
        //                               ^^^ rangeThatDoesNotReplaceIndentation
        // inlineCompletion.text: '··foo'
        //                         ^^ suggestionAddedIndentationLength
        let suggestion_added_indentation_length = get_leading_whitespace(e.text.units(), 0, e.text.len()).len();

        let replaced_indentation = substring(
            source_line.units(),
            i64::from(e.range.start_column - 1),
            i64::from(source_indentation_length),
        );

        let (start_position, end_position) = (e.range.get_start_position(), e.range.get_end_position());
        let new_start_position = if start_position.column + replaced_indentation.len() as i32 <= end_position.column {
            start_position.delta(0, replaced_indentation.len() as i32)
        } else {
            end_position
        };
        let range_that_does_not_replace_indentation = Range::from_positions(new_start_position, end_position);

        let suggestion_without_indentation_change = if e.text.units().starts_with(replaced_indentation) {
            // Adds more indentation without changing existing indentation: We can add ghost text for this
            JsString::from(&e.text.units()[replaced_indentation.len()..])
        } else {
            // Changes or removes existing indentation. Only add ghost text for the non-indentation part.
            JsString::from(&e.text.units()[suggestion_added_indentation_length..])
        };

        e = TextReplacement::new(range_that_does_not_replace_indentation, suggestion_without_indentation_change);
    }

    // This is a single line string
    let value_to_be_replaced = model.get_value_in_range(&e.range);

    // No ghost text in case the diff would be too slow to compute
    let changes = caching_diff(&value_to_be_replaced, &e.text)?;

    let line_number = e.range.start_line_number;

    let mut parts = Vec::new();

    if mode == GhostTextMode::Prefix {
        let filtered_changes: Vec<&DiffChange> = changes.iter().filter(|c| c.original_length == 0).collect();
        if filtered_changes.len() > 1
            || filtered_changes.len() == 1 && filtered_changes[0].original_start != value_to_be_replaced.len() as i32
        {
            // Prefixes only have a single change.
            return None;
        }
    }

    let preview_start_in_completion_text = e.text.len() as i32 - preview_suffix_length;

    for c in &changes {
        let insert_column = e.range.start_column + c.original_start + c.original_length;

        if mode == GhostTextMode::SubwordSmart
            && let Some(cursor) = cursor_position
            && cursor.line_number == e.range.start_line_number
            && insert_column < cursor.column
        {
            // No ghost text before cursor
            return None;
        }

        if c.original_length > 0 {
            return None;
        }

        if c.modified_length == 0 {
            continue;
        }

        let modified_end = c.modified_start + c.modified_length;
        let non_preview_text_end = c.modified_start.max(modified_end.min(preview_start_in_completion_text));
        let non_preview_text = substring(e.text.units(), i64::from(c.modified_start), i64::from(non_preview_text_end));
        let italic_text = substring(
            e.text.units(),
            i64::from(non_preview_text_end),
            i64::from(c.modified_start.max(modified_end)),
        );

        if !non_preview_text.is_empty() {
            parts.push(GhostTextPart::new(insert_column, JsString::from(non_preview_text), false, Vec::new()));
        }
        if !italic_text.is_empty() {
            parts.push(GhostTextPart::new(insert_column, JsString::from(italic_text), true, Vec::new()));
        }
    }

    Some(GhostText::new(line_number, parts))
}

/// `lastRequest`: o último par de textos diffado e o resultado.
struct LastRequest {
    original_value: JsString,
    new_value: JsString,
    changes: Option<Vec<DiffChange>>,
}

thread_local! {
    static LAST_REQUEST: RefCell<Option<LastRequest>> = const { RefCell::new(None) };
}

fn caching_diff(original_value: &JsString, new_value: &JsString) -> Option<Vec<DiffChange>> {
    let cached = LAST_REQUEST.with(|last| {
        last.borrow().as_ref().and_then(|request| {
            (request.original_value == *original_value && request.new_value == *new_value).then(|| request.changes.clone())
        })
    });
    if let Some(changes) = cached {
        return changes;
    }

    let mut changes = smart_diff(original_value.units(), new_value.units(), true);
    if let Some(current) = &changes {
        let deleted_chars = deleted_characters(current);
        if deleted_chars > 0 {
            // For performance reasons, don't compute diff if there is nothing to improve
            let new_changes = smart_diff(original_value.units(), new_value.units(), false);
            if let Some(new_changes) = new_changes
                && deleted_characters(&new_changes) < deleted_chars
            {
                // Disabling smartness seems to be better here
                changes = Some(new_changes);
            }
        }
    }
    LAST_REQUEST.with(|last| {
        *last.borrow_mut() = Some(LastRequest {
            original_value: original_value.clone(),
            new_value: new_value.clone(),
            changes: changes.clone(),
        });
    });
    changes
}

fn deleted_characters(changes: &[DiffChange]) -> i32 {
    changes.iter().map(|c| c.original_length).sum()
}

/// A sequência de inteiros (`Int32Array`) que o `LcsDiff` recebe.
struct IntSequence(Vec<i32>);

impl ISequence for IntSequence {
    fn get_elements(&self) -> SequenceElements {
        SequenceElements::Ints(self.0.clone())
    }
}

/// When matching `if ()` with `if (f() = 1) { g(); }`,
/// align it like this:        `if (       )`
/// Not like this:             `if (  )`
/// Also not like this:        `if (             )`.
///
/// The parenthesis are preprocessed to ensure that they match correctly.
pub fn smart_diff(original_value: &[u16], new_value: &[u16], smart_bracket_matching: bool) -> Option<Vec<DiffChange>> {
    if original_value.len() > 5000 || new_value.len() > 5000 {
        // We don't want to work on strings that are too big
        return None;
    }

    fn get_max_char_code(val: &[u16]) -> i32 {
        val.iter().copied().map(i32::from).max().unwrap_or(0).max(0)
    }

    let max_char_code = get_max_char_code(original_value).max(get_max_char_code(new_value));
    let get_unique_char_code = |id: i32| -> i32 {
        assert!(id >= 0, "unexpected");
        max_char_code + id + 1
    };

    let get_elements = |source: &[u16]| -> Vec<i32> {
        let mut level = 0i32;
        let mut group = 0i32;
        let mut characters = vec![0i32; source.len()];
        for (i, &unit) in source.iter().enumerate() {
            // TODO support more brackets
            if smart_bracket_matching && unit == u16::from(b'(') {
                let id = group * 100 + level;
                characters[i] = get_unique_char_code(2 * id);
                level += 1;
            } else if smart_bracket_matching && unit == u16::from(b')') {
                level = (level - 1).max(0);
                let id = group * 100 + level;
                characters[i] = get_unique_char_code(2 * id + 1);
                if level == 0 {
                    group += 1;
                }
            } else {
                characters[i] = i32::from(unit);
            }
        }
        characters
    };

    let elements1 = get_elements(original_value);
    let elements2 = get_elements(new_value);

    Some(LcsDiff::new(Rc::new(IntSequence(elements1)), Rc::new(IntSequence(elements2)), None).compute_diff(false).changes)
}
