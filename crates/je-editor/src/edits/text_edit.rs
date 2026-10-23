//! Porte de `src/vs/editor/common/core/edits/textEdit.ts`: `TextEdit` e `TextReplacement`.
//!
//! Notas de tradução:
//! - o texto é UTF-16 ([`JsString`]), como no JavaScript: colunas e comprimentos contam unidades. Só
//!   `TextLength.ofText` conta pontos de código, porque o original itera com `for (const c of text)`;
//! - `AbstractText` é o trait de [`crate::text::abstract_text`], recebido como `&dyn AbstractText`. Onde o
//!   original aceita `AbstractText | string` (`toString`), o texto cru entra por um [`StringText`];
//! - `Position | Range` (retorno de `mapPosition`) é [`PositionOrRange`];
//! - `Number.MAX_SAFE_INTEGER` das colunas vira `i32::MAX`, a maior coluna que o `Range` representa;
//! - `TextEdit.toString(text)` é [`TextEdit::describe`] (sem texto) e [`TextEdit::describe_on`] (com texto); o
//!   `toString()` de `TextReplacement` é o `Display`;
//! - `TextReplacement.equals` existe como estática e como instância no original, e a estática só repassa; fica
//!   a de instância ([`TextReplacement::equals`]);
//! - `TextEdit.toReplacement(text)` é idêntico a `TextReplacement.joinReplacements(replacements, text)`, e por
//!   isso chama-o;
//! - `TextEdit.fromStringEdit` e `TextReplacement.fromStringReplacement` aceitam qualquer edição e substituição
//!   de string (`BaseStringEdit`, `BaseStringReplacement`), como a tipagem estrutural do original;
//! - a função local `rangeFromPositions` do original é privada aqui também.

use std::fmt;

use je_base::JsString;
use je_base::arrays::{self, sort_by_compare};
use je_base::assert::{assert_fn, check_adjacent_items};
use je_base::errors::{self, Error};
use je_base::js_string::{index_of, substring};
use je_base::strings::{common_prefix_length, common_suffix_length};


use super::string_edit::{BaseStringEdit, BaseStringReplacement, replace_all_units};
use crate::edit_operation::SingleEditOperation;
use crate::position::Position;
use crate::position_to_offset::{PositionOffsetTransformerBase, TextLength};
use crate::range::Range;
use crate::text::abstract_text::{AbstractText, StringText};

/// `Position | Range`: o que `TextEdit.mapPosition` devolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionOrRange {
    Position(Position),
    Range(Range),
}

impl PositionOrRange {
    /// `p instanceof Position ? p : p.getStartPosition()`.
    pub fn start(self) -> Position {
        match self {
            PositionOrRange::Position(p) => p,
            PositionOrRange::Range(r) => r.get_start_position(),
        }
    }

    /// `p instanceof Position ? p : p.getEndPosition()`.
    pub fn end(self) -> Position {
        match self {
            PositionOrRange::Position(p) => p,
            PositionOrRange::Range(r) => r.get_end_position(),
        }
    }
}

/// `a + b` de strings.
fn concat(a: &JsString, b: &JsString) -> JsString {
    let mut result = a.clone();
    result.push_units(b.units());
    result
}

/// `text.replaceAll('\r\n', '\n')`.
fn normalize_crlf(text: &JsString) -> JsString {
    JsString::from_units(replace_all_units(text.units(), &[13, 10], &[10]))
}

/// Um conjunto de substituições paralelas, ordenadas e sem sobreposição, aplicadas todas de uma vez.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub replacements: Vec<TextReplacement>,
}

impl TextEdit {
    pub fn from_string_edit<E: BaseStringEdit>(edit: &E, initial_state: &dyn AbstractText) -> TextEdit
    where
        E::Replacement: BaseStringReplacement,
    {
        let edits = edit
            .replacements()
            .iter()
            .map(|e| TextReplacement::from_string_replacement(e, initial_state))
            .collect();
        TextEdit::new(edits)
    }

    pub fn replace(original_range: Range, new_text: impl Into<JsString>) -> TextEdit {
        TextEdit::new(vec![TextReplacement::new(original_range, new_text)])
    }

    pub fn delete(range: Range) -> TextEdit {
        TextEdit::new(vec![TextReplacement::new(range, "")])
    }

    pub fn insert(position: Position, new_text: impl Into<JsString>) -> TextEdit {
        TextEdit::new(vec![TextReplacement::new(Range::from_positions(position, position), new_text)])
    }

    pub fn from_parallel_replacements_unsorted(replacements: &[TextReplacement]) -> TextEdit {
        let mut r = replacements.to_vec();
        sort_by_compare(&mut r, |a, b| {
            f64::from(Range::compare_ranges_using_starts(Some(&a.range), Some(&b.range)))
        });
        TextEdit::new(r)
    }

    /// O construtor confere (como `assertFn`) que as substituições estão ordenadas e não se sobrepõem.
    pub fn new(replacements: Vec<TextReplacement>) -> Self {
        assert_fn(|| {
            check_adjacent_items(&replacements, |a, b| {
                a.range.get_end_position().is_before_or_equal(&b.range.get_start_position())
            })
        });
        Self { replacements }
    }

    /// Junta as edições que se tocam e remove as vazias.
    pub fn normalize(&self) -> TextEdit {
        let mut replacements: Vec<TextReplacement> = Vec::new();
        for r in &self.replacements {
            if let Some(last) = replacements.last_mut()
                && last.range.get_end_position() == r.range.get_start_position()
            {
                *last = TextReplacement::new(last.range.plus_range(&r.range), concat(&last.text, &r.text));
            } else if !r.is_empty() {
                replacements.push(r.clone());
            }
        }
        TextEdit::new(replacements)
    }

    pub fn map_position(&self, position: Position) -> PositionOrRange {
        let mut line_delta = 0;
        let mut cur_line = 0;
        let mut column_delta_in_cur_line = 0;

        for replacement in &self.replacements {
            let start = replacement.range.get_start_position();

            if position.is_before_or_equal(&start) {
                break;
            }

            let end = replacement.range.get_end_position();
            let len = TextLength::of_text(&replacement.text.to_string_lossy());
            if position.is_before(&end) {
                let start_pos = Position::new(
                    start.line_number + line_delta,
                    start.column + if start.line_number + line_delta == cur_line { column_delta_in_cur_line } else { 0 },
                );
                let end_pos = len.add_to_position(start_pos);
                return PositionOrRange::Range(range_from_positions(start_pos, end_pos));
            }

            if start.line_number + line_delta != cur_line {
                column_delta_in_cur_line = 0;
            }

            line_delta += len.line_count - (replacement.range.end_line_number - replacement.range.start_line_number);

            if len.line_count == 0 {
                if end.line_number != start.line_number {
                    column_delta_in_cur_line += len.column_count - (end.column - 1);
                } else {
                    column_delta_in_cur_line += len.column_count - (end.column - start.column);
                }
            } else {
                column_delta_in_cur_line = len.column_count;
            }
            cur_line = end.line_number + line_delta;
        }

        PositionOrRange::Position(Position::new(
            position.line_number + line_delta,
            position.column + if position.line_number + line_delta == cur_line { column_delta_in_cur_line } else { 0 },
        ))
    }

    pub fn map_range(&self, range: &Range) -> Range {
        let start = self.map_position(range.get_start_position()).start();
        let end = self.map_position(range.get_end_position()).end();

        range_from_positions(start, end)
    }

    // TODO do original: `doc` não é necessário para isso!
    pub fn inverse_map_position(&self, position_after_edit: Position, doc: &dyn AbstractText) -> PositionOrRange {
        let reversed = self.inverse(doc);
        reversed.map_position(position_after_edit)
    }

    pub fn inverse_map_range(&self, range: &Range, doc: &dyn AbstractText) -> Range {
        let reversed = self.inverse(doc);
        reversed.map_range(range)
    }

    pub fn apply(&self, text: &dyn AbstractText) -> JsString {
        let mut result = JsString::new();
        let mut last_edit_end = Position::new(1, 1);
        for replacement in &self.replacements {
            let edit_range = replacement.range;
            let edit_start = edit_range.get_start_position();
            let edit_end = edit_range.get_end_position();

            let r = range_from_positions(last_edit_end, edit_start);
            if !r.is_empty() {
                result.push_units(text.get_value_of_range(&r).units());
            }
            result.push_units(replacement.text.units());
            last_edit_end = edit_end;
        }
        let r = range_from_positions(last_edit_end, text.end_position_exclusive());
        if !r.is_empty() {
            result.push_units(text.get_value_of_range(&r).units());
        }
        result
    }

    pub fn apply_to_string(&self, str: impl Into<JsString>) -> JsString {
        let str_text = StringText::new(str);
        self.apply(&str_text)
    }

    pub fn inverse(&self, doc: &dyn AbstractText) -> TextEdit {
        let ranges = self.get_new_ranges();
        TextEdit::new(
            self.replacements
                .iter()
                .enumerate()
                .map(|(idx, e)| TextReplacement::new(ranges[idx], doc.get_value_of_range(&e.range)))
                .collect(),
        )
    }

    pub fn get_new_ranges(&self) -> Vec<Range> {
        let mut new_ranges: Vec<Range> = Vec::new();
        let mut previous_edit_end_line_number = 0;
        let mut line_offset = 0;
        let mut column_offset = 0;
        for replacement in &self.replacements {
            let text_length = TextLength::of_text(&replacement.text.to_string_lossy());
            let new_range_start = Position::new(
                replacement.range.start_line_number + line_offset,
                replacement.range.start_column
                    + if replacement.range.start_line_number == previous_edit_end_line_number { column_offset } else { 0 },
            );
            let new_range = text_length.create_range(new_range_start);
            new_ranges.push(new_range);
            line_offset = new_range.end_line_number - replacement.range.end_line_number;
            column_offset = new_range.end_column - replacement.range.end_column;
            previous_edit_end_line_number = replacement.range.end_line_number;
        }
        new_ranges
    }

    pub fn to_replacement(&self, text: &dyn AbstractText) -> TextReplacement {
        TextReplacement::join_replacements(&self.replacements, text)
    }

    pub fn equals(&self, other: &TextEdit) -> bool {
        arrays::equals(Some(self.replacements.as_slice()), Some(other.replacements.as_slice()), |a, b| a.equals(b))
    }

    /// Combina duas edições numa só com o mesmo efeito.
    /// AVISO do original: escrito por IA, mas bem testado.
    ///
    /// Invariante:
    /// ```text
    /// other.applyToString(this.applyToString(s0)) = this.compose(other).applyToString(s0)
    /// ```
    pub fn compose(&self, other: &TextEdit) -> TextEdit {
        let edits1 = self.normalize();
        let edits2 = other.normalize();

        if edits1.replacements.is_empty() {
            return edits2;
        }
        if edits2.replacements.is_empty() {
            return edits1;
        }

        let mut result_replacements: Vec<TextReplacement> = Vec::new();

        let mut head = ComposeHead::new(&edits1.replacements);
        let mut current_pos_in_s1 = Position::new(1, 1);

        for r2 in &edits2.replacements {
            let r2_start = r2.range.get_start_position();
            let r2_end = r2.range.get_end_position();

            loop {
                if current_pos_in_s1 == r2_start {
                    break;
                }
                head.ensure();

                if head.is_infinite {
                    let (dist_line, dist_col) = distance(current_pos_in_s1, r2_start);

                    current_pos_in_s1 = r2_start;

                    if dist_line == 0 {
                        head.src_start.column += dist_col;
                    } else {
                        head.src_start.line_number += dist_line;
                        head.src_start.column = dist_col + 1;
                    }
                    break;
                }

                let head_end_in_s1 = head.end_in_s1(current_pos_in_s1);

                if r2_start.is_before(&head_end_in_s1) {
                    let (split_len_line, split_len_col) = distance(current_pos_in_s1, r2_start);
                    let (remaining_len_line, remaining_len_col) = head.remaining_after(split_len_line, split_len_col);

                    let split = head.text.as_ref().map(|t| split_text(t, split_len_line, split_len_col));
                    if let Some((t1, t2)) = split {
                        result_replacements.push(TextReplacement::new(head.src_range(), t1));

                        head.text = Some(t2);
                        head.length_line = remaining_len_line;
                        head.length_col = remaining_len_col;

                        head.src_start = head.src_end;
                    } else {
                        head.src_start = head.advance_src_start(split_len_line, split_len_col);

                        head.length_line = remaining_len_line;
                        head.length_col = remaining_len_col;
                    }
                    current_pos_in_s1 = r2_start;
                    break;
                }

                if let Some(text) = &head.text {
                    result_replacements.push(TextReplacement::new(head.src_range(), text.clone()));
                }

                current_pos_in_s1 = head_end_in_s1;
                head.has_value = false;
            }

            let mut consumed_start: Option<Position> = None;
            let mut consumed_end = Position::new(0, 0);

            loop {
                if current_pos_in_s1 == r2_end {
                    break;
                }
                head.ensure();

                if head.is_infinite {
                    let (dist_line, dist_col) = distance(current_pos_in_s1, r2_end);

                    let range_in_s0_end = if dist_line == 0 {
                        Position::new(head.src_start.line_number, head.src_start.column + dist_col)
                    } else {
                        Position::new(head.src_start.line_number + dist_line, dist_col + 1)
                    };

                    consumed_start.get_or_insert(head.src_start);
                    consumed_end = range_in_s0_end;

                    current_pos_in_s1 = r2_end;

                    head.src_start = range_in_s0_end;
                    break;
                }

                let head_end_in_s1 = head.end_in_s1(current_pos_in_s1);

                if r2_end.is_before(&head_end_in_s1) {
                    let (split_len_line, split_len_col) = distance(current_pos_in_s1, r2_end);
                    let (remaining_len_line, remaining_len_col) = head.remaining_after(split_len_line, split_len_col);

                    let split = head.text.as_ref().map(|t| split_text(t, split_len_line, split_len_col));
                    if let Some((_, t2)) = split {
                        consumed_start.get_or_insert(head.src_start);
                        consumed_end = head.src_end;

                        head.text = Some(t2);
                        head.length_line = remaining_len_line;
                        head.length_col = remaining_len_col;

                        head.src_start = head.src_end;
                    } else {
                        let split_pos = head.advance_src_start(split_len_line, split_len_col);

                        consumed_start.get_or_insert(head.src_start);
                        consumed_end = split_pos;

                        head.src_start = split_pos;

                        head.length_line = remaining_len_line;
                        head.length_col = remaining_len_col;
                    }
                    current_pos_in_s1 = r2_end;
                    break;
                }

                consumed_start.get_or_insert(head.src_start);
                consumed_end = head.src_end;

                current_pos_in_s1 = head_end_in_s1;
                head.has_value = false;
            }

            if let Some(start) = consumed_start {
                result_replacements.push(TextReplacement::new(
                    Range::new(start.line_number, start.column, consumed_end.line_number, consumed_end.column),
                    r2.text.clone(),
                ));
            } else {
                head.ensure();
                let insert_pos_s0 = head.src_start;
                result_replacements.push(TextReplacement::new(
                    Range::new(insert_pos_s0.line_number, insert_pos_s0.column, insert_pos_s0.line_number, insert_pos_s0.column),
                    r2.text.clone(),
                ));
            }
        }

        loop {
            head.ensure();
            if head.is_infinite {
                break;
            }
            if let Some(text) = &head.text {
                result_replacements.push(TextReplacement::new(head.src_range(), text.clone()));
            }
            head.has_value = false;
        }

        TextEdit::new(result_replacements).normalize()
    }

    /// `toString()` sem texto: uma substituição por linha.
    pub fn describe(&self) -> String {
        self.replacements.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
    }

    /// `toString(text)` com um texto (com uma string crua, passe um [`StringText`]): cada substituição com o
    /// contexto ao redor, o trecho trocado e o texto novo, abreviados.
    pub fn describe_on(&self, text: &dyn AbstractText) -> JsString {
        let mut result = JsString::new();
        for (i, r) in self.replacements.iter().enumerate() {
            if i > 0 {
                result.push_unit(u16::from(b'\n'));
            }
            let max_length = 10;
            let original_text = text.get_value_of_range(&r.range);

            // Texto antes da edição
            let before_range = Range::from_positions(
                Position::new((r.range.start_line_number - 1).max(1), 1),
                r.range.get_start_position(),
            );
            let mut before_text = text.get_value_of_range(&before_range);
            if before_text.len() > max_length {
                let mut abbreviated = JsString::from("...");
                abbreviated.push_units(substring(
                    before_text.units(),
                    (before_text.len() - max_length) as i64,
                    before_text.len() as i64,
                ));
                before_text = abbreviated;
            }

            // Texto depois da edição
            let after_range = Range::from_positions(
                r.range.get_end_position(),
                Position::new(r.range.end_line_number + 1, 1),
            );
            let mut after_text = text.get_value_of_range(&after_range);
            if after_text.len() > max_length {
                let mut abbreviated = JsString::from(substring(after_text.units(), 0, max_length as i64));
                abbreviated.push_units(&[46, 46, 46]);
                after_text = abbreviated;
            }

            // Formata o texto trocado e o texto novo
            let replaced_text = abbreviate_middle(&original_text, max_length);
            let new_text = abbreviate_middle(&r.text, max_length);

            result.push_units(before_text.units());
            result.push_unit(0x2770); // ❰
            if replaced_text.is_empty() {
                result.push_units(new_text.units());
            } else {
                result.push_units(replaced_text.units());
                result.push_unit(0x21A6); // ↦
                result.push_units(new_text.units());
            }
            result.push_unit(0x2771); // ❱
            result.push_units(after_text.units());
        }
        result
    }
}

impl fmt::Display for TextEdit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

/// O texto longo demais vira `começo...fim`, com `max_length / 2` unidades de cada ponta.
fn abbreviate_middle(text: &JsString, max_length: usize) -> JsString {
    if text.len() <= max_length {
        return text.clone();
    }
    let half_max = max_length / 2;
    let mut result = JsString::from(substring(text.units(), 0, half_max as i64));
    result.push_units(&[46, 46, 46]);
    result.push_units(substring(text.units(), (text.len() - half_max) as i64, text.len() as i64));
    result
}

/// A distância de `from` até `to` como (linhas, colunas), a que o `compose` usa em todo lugar.
fn distance(from: Position, to: Position) -> (i32, i32) {
    if from.line_number == to.line_number {
        (0, to.column - from.column)
    } else {
        (to.line_number - from.line_number, to.column - 1)
    }
}

/// `splitText` local do `compose`: divide `text` depois de `len_line` linhas e `len_col` colunas.
fn split_text(text: &JsString, len_line: i32, len_col: i32) -> (JsString, JsString) {
    if len_line == 0 && len_col == 0 {
        return (JsString::new(), text.clone());
    }
    let mut line = 0;
    let mut offset = 0usize;
    while line < len_line {
        let Some(idx) = index_of(text.units(), &[10], offset) else {
            errors::throw(Error::bug_indicating(Some("Text length mismatch")));
        };
        offset = idx + 1;
        line += 1;
    }
    offset += len_col as usize;
    let len = text.len() as i64;
    (
        JsString::from(substring(text.units(), 0, offset as i64)),
        JsString::from(substring(text.units(), offset as i64, len)),
    )
}

/// O estado do `compose`: a "cabeça" da edição 1 que está sendo consumida (o trecho sem mudança entre duas
/// substituições, a própria substituição, ou o resto infinito depois da última) e o cursor na edição 1.
struct ComposeHead<'a> {
    edits1: &'a [TextReplacement],
    edit1_idx: usize,
    last_edit1_end_s0: Position,

    src_start: Position,
    src_end: Position,
    text: Option<JsString>,
    length_line: i32,
    length_col: i32,

    has_value: bool,
    is_infinite: bool,
}

impl<'a> ComposeHead<'a> {
    fn new(edits1: &'a [TextReplacement]) -> Self {
        Self {
            edits1,
            edit1_idx: 0,
            last_edit1_end_s0: Position::new(1, 1),
            src_start: Position::new(0, 0),
            src_end: Position::new(0, 0),
            text: None,
            length_line: 0,
            length_col: 0,
            has_value: false,
            is_infinite: false,
        }
    }

    /// `new Range(headSrcRangeStartLine, headSrcRangeStartCol, headSrcRangeEndLine, headSrcRangeEndCol)`.
    fn src_range(&self) -> Range {
        Range::new(self.src_start.line_number, self.src_start.column, self.src_end.line_number, self.src_end.column)
    }

    /// O fim da cabeça no espaço da edição 1 aplicada, a partir de `current`.
    fn end_in_s1(&self, current: Position) -> Position {
        if self.length_line == 0 {
            Position::new(current.line_number, current.column + self.length_col)
        } else {
            Position::new(current.line_number + self.length_line, self.length_col + 1)
        }
    }

    /// O que sobra da cabeça depois de consumir (`split_len_line`, `split_len_col`) dela.
    fn remaining_after(&self, split_len_line: i32, split_len_col: i32) -> (i32, i32) {
        if split_len_line == self.length_line {
            (0, self.length_col - split_len_col)
        } else {
            (self.length_line - split_len_line, self.length_col)
        }
    }

    /// O início da cabeça no texto original depois de consumir (`split_len_line`, `split_len_col`) dela.
    fn advance_src_start(&self, split_len_line: i32, split_len_col: i32) -> Position {
        if split_len_line == 0 {
            Position::new(self.src_start.line_number, self.src_start.column + split_len_col)
        } else {
            Position::new(self.src_start.line_number + split_len_line, split_len_col + 1)
        }
    }

    /// `ensureHead`.
    fn ensure(&mut self) {
        if self.has_value {
            return;
        }

        let edits1 = self.edits1;
        if self.edit1_idx < edits1.len() {
            let next_edit = &edits1[self.edit1_idx];
            let next_edit_start = next_edit.range.get_start_position();

            let gap_is_empty = self.last_edit1_end_s0 == next_edit_start;

            if !gap_is_empty {
                self.src_start = self.last_edit1_end_s0;
                self.src_end = next_edit_start;

                self.text = None;

                if self.last_edit1_end_s0.line_number == next_edit_start.line_number {
                    self.length_line = 0;
                    self.length_col = next_edit_start.column - self.last_edit1_end_s0.column;
                } else {
                    self.length_line = next_edit_start.line_number - self.last_edit1_end_s0.line_number;
                    self.length_col = next_edit_start.column - 1;
                }

                self.has_value = true;
                self.last_edit1_end_s0 = next_edit_start;
            } else {
                let next_edit_end = next_edit.range.get_end_position();
                self.src_start = next_edit_start;
                self.src_end = next_edit_end;

                self.text = Some(next_edit.text.clone());

                let mut line = 0;
                let mut column = 0;
                for &unit in next_edit.text.units() {
                    if unit == 10 {
                        line += 1;
                        column = 0;
                    } else {
                        column += 1;
                    }
                }
                self.length_line = line;
                self.length_col = column;

                self.has_value = true;
                self.last_edit1_end_s0 = next_edit_end;
                self.edit1_idx += 1;
            }
        } else {
            self.is_infinite = true;
            self.src_start = self.last_edit1_end_s0;
            self.has_value = true;
        }
    }
}

/// A troca do texto de `range` por `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextReplacement {
    pub range: Range,
    pub text: JsString,
}

impl TextReplacement {
    pub fn join_replacements(replacements: &[TextReplacement], initial_value: &dyn AbstractText) -> TextReplacement {
        if replacements.is_empty() {
            errors::throw(Error::bug_indicating(None));
        }
        if replacements.len() == 1 {
            return replacements[0].clone();
        }

        let start_pos = replacements[0].range.get_start_position();
        let end_pos = replacements[replacements.len() - 1].range.get_end_position();

        let mut new_text = JsString::new();

        for (i, cur_edit) in replacements.iter().enumerate() {
            new_text.push_units(cur_edit.text.units());
            if let Some(next_edit) = replacements.get(i + 1) {
                let gap_range = Range::from_positions(cur_edit.range.get_end_position(), next_edit.range.get_start_position());
                let gap_text = initial_value.get_value_of_range(&gap_range);
                new_text.push_units(gap_text.units());
            }
        }
        TextReplacement::new(Range::from_positions(start_pos, end_pos), new_text)
    }

    pub fn from_string_replacement<R: BaseStringReplacement>(
        replacement: &R,
        initial_state: &dyn AbstractText,
    ) -> TextReplacement {
        TextReplacement::new(
            initial_state.get_transformer().get_range(replacement.replace_range()),
            replacement.new_text().clone(),
        )
    }

    pub fn delete(range: Range) -> TextReplacement {
        TextReplacement::new(range, "")
    }

    pub fn new(range: Range, text: impl Into<JsString>) -> Self {
        Self { range, text: text.into() }
    }

    pub fn is_empty(&self) -> bool {
        self.range.is_empty() && self.text.is_empty()
    }

    pub fn equals(&self, other: &TextReplacement) -> bool {
        self.range == other.range && self.text == other.text
    }

    pub fn to_single_edit_operation(&self) -> SingleEditOperation {
        SingleEditOperation::new(self.range, Some(self.text.clone()), false)
    }

    pub fn to_edit(&self) -> TextEdit {
        TextEdit::new(vec![self.clone()])
    }

    pub fn extend_to_cover_range(&self, range: &Range, initial_value: &dyn AbstractText) -> TextReplacement {
        if self.range.contains_range(range) {
            return self.clone();
        }

        let new_range = self.range.plus_range(range);
        let text_before = initial_value
            .get_value_of_range(&Range::from_positions(new_range.get_start_position(), self.range.get_start_position()));
        let text_after = initial_value
            .get_value_of_range(&Range::from_positions(self.range.get_end_position(), new_range.get_end_position()));
        let new_text = concat(&concat(&text_before, &self.text), &text_after);
        TextReplacement::new(new_range, new_text)
    }

    pub fn extend_to_full_line(&self, initial_value: &dyn AbstractText) -> TextReplacement {
        let new_range = Range::new(
            self.range.start_line_number,
            1,
            self.range.end_line_number,
            initial_value.get_transformer().get_line_length(self.range.end_line_number) + 1,
        );
        self.extend_to_cover_range(&new_range, initial_value)
    }

    pub fn remove_common_prefix_and_suffix(&self, text: &dyn AbstractText) -> TextReplacement {
        let prefix = self.remove_common_prefix(text);
        prefix.remove_common_suffix(text)
    }

    pub fn remove_common_prefix(&self, text: &dyn AbstractText) -> TextReplacement {
        let normalized_original_text = normalize_crlf(&text.get_value_of_range(&self.range));
        let normalized_modified_text = normalize_crlf(&self.text);

        let common_prefix_len =
            common_prefix_length(normalized_original_text.units(), normalized_modified_text.units());
        let start = TextLength::of_text_units(substring(normalized_original_text.units(), 0, common_prefix_len as i64))
            .add_to_position(self.range.get_start_position());

        let new_text = JsString::from(substring(
            normalized_modified_text.units(),
            common_prefix_len as i64,
            normalized_modified_text.len() as i64,
        ));
        let range = Range::from_positions(start, self.range.get_end_position());
        TextReplacement::new(range, new_text)
    }

    pub fn remove_common_suffix(&self, text: &dyn AbstractText) -> TextReplacement {
        let normalized_original_text = normalize_crlf(&text.get_value_of_range(&self.range));
        let normalized_modified_text = normalize_crlf(&self.text);

        let common_suffix_len =
            common_suffix_length(normalized_original_text.units(), normalized_modified_text.units());
        let end = TextLength::of_text_units(substring(
            normalized_original_text.units(),
            0,
            normalized_original_text.len() as i64 - common_suffix_len as i64,
        ))
        .add_to_position(self.range.get_start_position());

        let new_text = JsString::from(substring(
            normalized_modified_text.units(),
            0,
            normalized_modified_text.len() as i64 - common_suffix_len as i64,
        ));
        let range = Range::from_positions(self.range.get_start_position(), end);
        TextReplacement::new(range, new_text)
    }

    pub fn is_effective_deletion(&self, text: &dyn AbstractText) -> bool {
        let new_text = normalize_crlf(&self.text);
        let existing_text = normalize_crlf(&text.get_value_of_range(&self.range));
        let l = common_prefix_length(new_text.units(), existing_text.units());
        let new_text = substring(new_text.units(), l as i64, new_text.len() as i64);
        let existing_text = substring(existing_text.units(), l as i64, existing_text.len() as i64);
        let r = common_suffix_length(new_text, existing_text);

        new_text.len() == r
    }
}

impl fmt::Display for TextReplacement {
    /// `toString()`: `(l1,c1 -> l2,c2): "texto"`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let start = self.range.get_start_position();
        let end = self.range.get_end_position();
        write!(f, "({},{} -> {},{}): \"{}\"", start.line_number, start.column, end.line_number, end.column, self.text)
    }
}

/// `rangeFromPositions` local do original.
fn range_from_positions(start: Position, end: Position) -> Range {
    if start.line_number == end.line_number && start.column == i32::MAX {
        return Range::from_positions(end, end);
    } else if !start.is_before_or_equal(&end) {
        errors::throw(Error::bug_indicating(Some("start must be before end")));
    }
    Range::new(start.line_number, start.column, end.line_number, end.column)
}
