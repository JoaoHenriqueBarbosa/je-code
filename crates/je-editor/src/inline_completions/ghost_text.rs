//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/ghostText.ts`: `GhostText`, `GhostTextPart`,
//! `GhostTextReplacement` e as comparações de união.
//!
//! Notas de tradução:
//! - Texto é UTF-16 (`JsString`), como no JavaScript: colunas e comprimentos contam unidades.
//! - `GhostText | GhostTextReplacement` é o enum [`GhostTextOrReplacement`]; o `instanceof` vira `match`.
//! - `IGhostTextLine` é [`GhostTextLine`]; `LineDecoration.equalsArr` é a igualdade de `Vec<LineDecoration>`.
//! - `assertFn(() => checkAdjacentItems(...))` do construtor de `GhostText` é `assert_fn` de `je_base::assert`.
//! - `TextReplacement`, `TextEdit` (`edits/textEdit.ts`) e `ColumnRange` (`ranges/columnRange.ts`) vêm dos
//!   módulos próprios ([`crate::edits::text_edit`], [`crate::column_range`]).

use je_base::JsString;
use je_base::arrays;
use je_base::assert::{assert_fn, check_adjacent_items};
use je_base::strings::split_lines;

use crate::column_range::ColumnRange;
use crate::edits::text_edit::{TextEdit, TextReplacement};
use crate::inline_decorations::InlineDecoration;
use crate::line_decorations::LineDecoration;
use crate::position::Position;
use crate::range::Range;

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
        .to_string_lossy()
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
        .apply_to_string(capped_line_text);

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
            .to_string_lossy()
        } else {
            TextEdit::new(vec![TextReplacement::new(replace_range, joined)]).apply_to_string(document_text).to_string_lossy()
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

impl GhostTextOrReplacement {
    /// `ghostText.lineNumber` de qualquer das duas variantes.
    pub fn line_number(&self) -> i32 {
        match self {
            GhostTextOrReplacement::GhostText(g) => g.line_number,
            GhostTextOrReplacement::Replacement(r) => r.line_number,
        }
    }

    /// `ghostText.parts` de qualquer das duas variantes.
    pub fn parts(&self) -> &[GhostTextPart] {
        match self {
            GhostTextOrReplacement::GhostText(g) => &g.parts,
            GhostTextOrReplacement::Replacement(r) => &r.parts,
        }
    }

    /// `ghostText.isEmpty()` de qualquer das duas variantes.
    pub fn is_empty(&self) -> bool {
        match self {
            GhostTextOrReplacement::GhostText(g) => g.is_empty(),
            GhostTextOrReplacement::Replacement(r) => r.is_empty(),
        }
    }

    /// `ghostText.lineCount` de qualquer das duas variantes.
    pub fn line_count(&self) -> i32 {
        match self {
            GhostTextOrReplacement::GhostText(g) => g.line_count(),
            GhostTextOrReplacement::Replacement(r) => r.line_count(),
        }
    }

    /// `ghostText.renderForScreenReader(lineText)` de qualquer das duas variantes.
    pub fn render_for_screen_reader(&self, line_text: &[u16]) -> JsString {
        match self {
            GhostTextOrReplacement::GhostText(g) => g.render_for_screen_reader(line_text),
            GhostTextOrReplacement::Replacement(r) => r.render_for_screen_reader(line_text),
        }
    }
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
