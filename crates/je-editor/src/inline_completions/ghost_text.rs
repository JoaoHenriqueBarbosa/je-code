//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/ghostText.ts`: `GhostText`, `GhostTextPart`,
//! `GhostTextReplacement` e as comparações de união.
//!
//! Notas de tradução:
//! - Texto é UTF-16 (`JsString`), como no JavaScript: colunas e comprimentos contam unidades.
//! - `GhostText | GhostTextReplacement` é o enum [`GhostTextOrReplacement`]; o `instanceof` vira `match`.
//! - `IGhostTextLine` é [`GhostTextLine`]; `LineDecoration.equalsArr` é a igualdade de `Vec<LineDecoration>`.
//! - `assertFn(() => checkAdjacentItems(...))` do construtor de `GhostText` é `assert_fn` de `je_base::assert`.
//!
//! Tipos de apoio portados aqui porque ainda não existem no crate (sair daqui quando ganharem módulo próprio,
//! em `edits/textEdit.ts` e `ranges/columnRange.ts`):
//! - [`TextReplacement`] e [`TextEdit`]: só o que o `ghostText.ts` e o `computeGhostText.ts` usam (construtor,
//!   `delete`, `apply`/`applyToString`). `TextEdit::apply` trabalha sobre o texto em unidades UTF-16.
//! - [`ColumnRange`]: completo (`fromOffsetRange`, `toRange`, `equals`, `toZeroBasedOffsetRange`).

use je_base::JsString;
use je_base::arrays;
use je_base::assert::{assert_fn, check_adjacent_items};
use je_base::errors::{self, Error};
use je_base::strings::split_lines;

use crate::inline_decorations::InlineDecoration;
use crate::line_decorations::LineDecoration;
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::position_to_offset::{PositionOffsetTransformer, PositionOffsetTransformerBase};
use crate::range::Range;

/// `TextReplacement` (`textEdit.ts`): troca o texto de `range` por `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextReplacement {
    pub range: Range,
    pub text: JsString,
}

impl TextReplacement {
    pub fn new(range: Range, text: impl Into<JsString>) -> Self {
        Self { range, text: text.into() }
    }

    /// `TextReplacement.delete(range)`.
    pub fn delete(range: Range) -> Self {
        Self::new(range, "")
    }

    /// `isEmpty`.
    pub fn is_empty(&self) -> bool {
        self.range.is_empty() && self.text.is_empty()
    }
}

/// `TextEdit` (`textEdit.ts`): substituições paralelas, ordenadas e sem sobreposição.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub replacements: Vec<TextReplacement>,
}

impl TextEdit {
    pub fn new(replacements: Vec<TextReplacement>) -> Self {
        assert_fn(|| {
            check_adjacent_items(&replacements, |a, b| a.range.get_end_position().is_before_or_equal(&b.range.get_start_position()))
        });
        Self { replacements }
    }

    /// `apply(text)` sobre o texto em unidades UTF-16.
    pub fn apply(&self, text: &[u16]) -> JsString {
        let transformer = PositionOffsetTransformer::new(&String::from_utf16_lossy(text));
        let mut result = JsString::new();
        let mut last_edit_end = 0usize;
        for replacement in &self.replacements {
            let start = transformer.get_offset(replacement.range.get_start_position()) as usize;
            let end = transformer.get_offset(replacement.range.get_end_position()) as usize;
            if last_edit_end < start {
                result.push_units(&text[last_edit_end..start]);
            }
            result.push_units(replacement.text.units());
            last_edit_end = end;
        }
        if last_edit_end < text.len() {
            result.push_units(&text[last_edit_end..]);
        }
        result
    }

    /// `applyToString(str)`.
    pub fn apply_to_string(&self, str: &str) -> String {
        let units: Vec<u16> = str.encode_utf16().collect();
        self.apply(&units).to_string_lossy()
    }
}

/// `ColumnRange` (`columnRange.ts`): intervalo de colunas, a partir de 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnRange {
    /// A partir de 1.
    pub start_column: i32,
    pub end_column_exclusive: i32,
}

impl ColumnRange {
    /// `ColumnRange.fromOffsetRange`.
    pub fn from_offset_range(offset_range: OffsetRange) -> Self {
        Self::new(offset_range.start + 1, offset_range.end_exclusive + 1)
    }

    /// O construtor lança `BugIndicatingError` se o início vier depois do fim.
    pub fn new(start_column: i32, end_column_exclusive: i32) -> Self {
        if start_column > end_column_exclusive {
            errors::throw(Error::bug_indicating(Some(&format!(
                "startColumn {start_column} cannot be after endColumnExclusive {end_column_exclusive}"
            ))));
        }
        Self { start_column, end_column_exclusive }
    }

    /// `toRange(lineNumber)`.
    pub fn to_range(&self, line_number: i32) -> Range {
        Range::new(line_number, self.start_column, line_number, self.end_column_exclusive)
    }

    /// `equals`.
    pub fn equals(&self, other: &ColumnRange) -> bool {
        self == other
    }

    /// `toZeroBasedOffsetRange`.
    pub fn to_zero_based_offset_range(&self) -> OffsetRange {
        OffsetRange::new(self.start_column - 1, self.end_column_exclusive - 1)
    }
}

/// `'\n'` entre as linhas (`lines.map(l => l.line).join('\n')`).
fn join_lines<'a>(lines: impl IntoIterator<Item = &'a [u16]>) -> JsString {
    let mut out = JsString::new();
    for (i, line) in lines.into_iter().enumerate() {
        if i > 0 {
            out.push_unit(u16::from(b'\n'));
        }
        out.push_units(line);
    }
    out
}

/// `[` + `text` + `]`.
fn bracketed(text: &JsString) -> JsString {
    let mut out = JsString::new();
    out.push_unit(u16::from(b'['));
    out.push_units(text.units());
    out.push_unit(u16::from(b']'));
    out
}

/// `GhostText`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhostText {
    pub line_number: i32,
    pub parts: Vec<GhostTextPart>,
}

impl GhostText {
    pub fn new(line_number: i32, parts: Vec<GhostTextPart>) -> Self {
        assert_fn(|| check_adjacent_items(&parts, |p1, p2| p1.column <= p2.column));
        Self { line_number, parts }
    }

    pub fn equals(&self, other: &GhostText) -> bool {
        self.line_number == other.line_number
            && self.parts.len() == other.parts.len()
            && self.parts.iter().zip(&other.parts).all(|(part, other_part)| part.equals(other_part))
    }

    /// Só usado em teste e depuração.
    pub fn render(&self, document_text: &str, debug: bool) -> String {
        TextEdit::new(
            self.parts
                .iter()
                .map(|p| {
                    let joined = p.joined_text();
                    TextReplacement::new(
                        Range::from_positions(Position::new(self.line_number, p.column), Position::new(self.line_number, p.column)),
                        if debug { bracketed(&joined) } else { joined },
                    )
                })
                .collect(),
        )
        .apply_to_string(document_text)
    }

    pub fn render_for_screen_reader(&self, line_text: &[u16]) -> JsString {
        let (Some(first_part), Some(last_part)) = (self.parts.first(), self.parts.last()) else {
            return JsString::new();
        };

        // `lineText.substr(0, lastPart.column - 1)`
        let capped_len = usize::try_from(last_part.column - 1).unwrap_or(0).min(line_text.len());
        let capped_line_text = &line_text[..capped_len];
        let text = TextEdit::new(
            self.parts
                .iter()
                .map(|p| {
                    TextReplacement::new(
                        Range::from_positions(Position::new(1, p.column), Position::new(1, p.column)),
                        p.joined_text(),
                    )
                })
                .collect(),
        )
        .apply(capped_line_text);

        // `text.substring(this.parts[0].column - 1)`
        je_base::js_string::substring(text.units(), i64::from(first_part.column - 1), text.len() as i64).into()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.iter().all(|p| p.lines.is_empty())
    }

    /// `lineCount`.
    pub fn line_count(&self) -> i32 {
        1 + self.parts.iter().map(|p| p.lines.len() as i32 - 1).sum::<i32>()
    }
}

/// `IGhostTextLine`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhostTextLine {
    pub line: JsString,
    pub line_decorations: Vec<LineDecoration>,
}

/// `GhostTextPart`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhostTextPart {
    pub column: i32,
    pub text: JsString,
    /// Indica se esta parte é uma prévia de uma sugestão em pré-visualização.
    pub preview: bool,
    pub lines: Vec<GhostTextLine>,
}

impl GhostTextPart {
    /// `inline_decorations` é o `_inlineDecorations` do construtor (padrão `[]`).
    pub fn new(column: i32, text: impl Into<JsString>, preview: bool, inline_decorations: Vec<InlineDecoration>) -> Self {
        let text = text.into();
        let lines = split_lines(text.units())
            .into_iter()
            .enumerate()
            .map(|(i, line)| GhostTextLine {
                line: JsString::from(line),
                line_decorations: LineDecoration::filter(&inline_decorations, i as i32 + 1, 1, line.len() as i32 + 1),
            })
            .collect();
        Self { column, text, preview, lines }
    }

    /// `p.lines.map(line => line.line).join('\n')`.
    fn joined_text(&self) -> JsString {
        join_lines(self.lines.iter().map(|l| l.line.units()))
    }

    pub fn equals(&self, other: &GhostTextPart) -> bool {
        self.column == other.column
            && self.lines.len() == other.lines.len()
            && self.lines.iter().zip(&other.lines).all(|(line, other_line)| {
                line.line == other_line.line && line.line_decorations == other_line.line_decorations
            })
    }
}

/// `GhostTextReplacement`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhostTextReplacement {
    pub line_number: i32,
    pub column_range: ColumnRange,
    pub text: JsString,
    pub additional_reserved_line_count: i32,
    pub parts: Vec<GhostTextPart>,
    pub new_lines: Vec<JsString>,
}

impl GhostTextReplacement {
    pub fn new(line_number: i32, column_range: ColumnRange, text: impl Into<JsString>, additional_reserved_line_count: i32) -> Self {
        let text = text.into();
        let parts = vec![GhostTextPart::new(column_range.end_column_exclusive, text.clone(), false, Vec::new())];
        let new_lines = split_lines(text.units()).into_iter().map(JsString::from).collect();
        Self { line_number, column_range, text, additional_reserved_line_count, parts, new_lines }
    }

    pub fn render_for_screen_reader(&self, _line_text: &[u16]) -> JsString {
        join_lines(self.new_lines.iter().map(|l| l.units()))
    }

    pub fn render(&self, document_text: &str, debug: bool) -> String {
        let replace_range = self.column_range.to_range(self.line_number);
        let joined = self.render_for_screen_reader(&[]);

        if debug {
            let mut close = JsString::from(")");
            close.push_units(bracketed(&joined).units());
            TextEdit::new(vec![
                TextReplacement::new(Range::from_positions(replace_range.get_start_position(), replace_range.get_start_position()), "("),
                TextReplacement::new(Range::from_positions(replace_range.get_end_position(), replace_range.get_end_position()), close),
            ])
            .apply_to_string(document_text)
        } else {
            TextEdit::new(vec![TextReplacement::new(replace_range, joined)]).apply_to_string(document_text)
        }
    }

    /// `lineCount`.
    pub fn line_count(&self) -> i32 {
        self.new_lines.len() as i32
    }

    pub fn is_empty(&self) -> bool {
        self.parts.iter().all(|p| p.lines.is_empty())
    }

    pub fn equals(&self, other: &GhostTextReplacement) -> bool {
        self.line_number == other.line_number
            && self.column_range.equals(&other.column_range)
            && self.new_lines == other.new_lines
            && self.additional_reserved_line_count == other.additional_reserved_line_count
    }
}

/// `GhostTextOrReplacement`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GhostTextOrReplacement {
    GhostText(GhostText),
    Replacement(GhostTextReplacement),
}

/// `ghostTextsOrReplacementsEqual`.
pub fn ghost_texts_or_replacements_equal(
    a: Option<&[GhostTextOrReplacement]>,
    b: Option<&[GhostTextOrReplacement]>,
) -> bool {
    arrays::equals(a, b, |x, y| ghost_text_or_replacement_equals(Some(x), Some(y)))
}

/// `ghostTextOrReplacementEquals`.
pub fn ghost_text_or_replacement_equals(a: Option<&GhostTextOrReplacement>, b: Option<&GhostTextOrReplacement>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(GhostTextOrReplacement::GhostText(a)), Some(GhostTextOrReplacement::GhostText(b))) => a.equals(b),
        (Some(GhostTextOrReplacement::Replacement(a)), Some(GhostTextOrReplacement::Replacement(b))) => a.equals(b),
        _ => false,
    }
}
