//! Porte de `src/vs/workbench/contrib/mergeEditor/browser/model/mapping.ts`.
//!
//! Notas de tradução:
//! - `MappingAlignment<T extends LineRangeMapping>` vira um tipo genérico sobre o trait
//!   [`AsLineRangeMapping`], que dá acesso ao `LineRangeMapping` base.
//! - `toString()` vira `Display`.
//! - O `sort` do original é estável, como o `sort_by_key` do Rust.
//! - `DetailedLineRangeMapping extends LineRangeMapping` vira um tipo que contém o [`LineRangeMapping`] (o trait
//!   [`AsLineRangeMapping`] o expõe) e guarda o `TextModel` (`ITextModel`) de entrada e de saída em `Rc`; o
//!   `join` de instância é `join_with` e o estático é `join`, como no tipo base.

use std::fmt;

use je_base::assert::{assert_fn, check_adjacent_items};
use je_base::errors::{self, Error};
use je_editor::position::Position;
use je_editor::position_to_offset::TextLength;
use je_editor::range::Range;

use std::rc::Rc;

use je_base::JsString;
use je_editor::text_model::TextModel;

use super::editing::LineRangeEdit;
use super::line_range::MergeEditorLineRange;
use super::range_utils::{range_contains_position, range_is_before_or_touching};

/// Representa o mapeamento de um intervalo de linhas de entrada para um de saída.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRangeMapping {
    pub input_range: MergeEditorLineRange,
    pub output_range: MergeEditorLineRange,
}

impl LineRangeMapping {
    pub fn join(mappings: &[LineRangeMapping]) -> Option<LineRangeMapping> {
        mappings.iter().fold(None, |acc, cur| Some(match acc {
            Some(acc) => acc.join_with(cur),
            None => *cur,
        }))
    }

    pub fn new(input_range: MergeEditorLineRange, output_range: MergeEditorLineRange) -> Self {
        Self { input_range, output_range }
    }

    pub fn extend_input_range(&self, extended_input_range: &MergeEditorLineRange) -> LineRangeMapping {
        if !extended_input_range.contains_range(&self.input_range) {
            errors::throw(Error::bug_indicating(None));
        }

        let start_delta = extended_input_range.start_line_number - self.input_range.start_line_number;
        let end_delta = extended_input_range.end_line_number_exclusive - self.input_range.end_line_number_exclusive;
        LineRangeMapping::new(
            *extended_input_range,
            MergeEditorLineRange::from_length(
                self.output_range.start_line_number + start_delta,
                self.output_range.length() - start_delta + end_delta,
            ),
        )
    }

    /// O `join` de instância do original (o estático é [`LineRangeMapping::join`]).
    pub fn join_with(&self, other: &LineRangeMapping) -> LineRangeMapping {
        LineRangeMapping::new(self.input_range.join(&other.input_range), self.output_range.join(&other.output_range))
    }

    pub fn resulting_delta_from_original_to_modified(&self) -> i32 {
        self.output_range.end_line_number_exclusive - self.input_range.end_line_number_exclusive
    }

    pub fn add_output_line_delta(&self, delta: i32) -> LineRangeMapping {
        LineRangeMapping::new(self.input_range, self.output_range.delta(delta))
    }

    pub fn add_input_line_delta(&self, delta: i32) -> LineRangeMapping {
        LineRangeMapping::new(self.input_range.delta(delta), self.output_range)
    }

    pub fn reverse(&self) -> LineRangeMapping {
        LineRangeMapping::new(self.output_range, self.input_range)
    }
}

impl fmt::Display for LineRangeMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.input_range, self.output_range)
    }
}

/// O que `MappingAlignment` exige de `T extends LineRangeMapping`.
pub trait AsLineRangeMapping {
    fn as_line_range_mapping(&self) -> &LineRangeMapping;
}

impl AsLineRangeMapping for LineRangeMapping {
    fn as_line_range_mapping(&self) -> &LineRangeMapping {
        self
    }
}

/// Representa um mapeamento monótono total de intervalos de linhas de um documento para outro.
#[derive(Clone, Debug)]
pub struct DocumentLineRangeMap {
    /// Os mapeamentos de intervalos de linhas que definem este mapeamento de documento.
    /// O espaço entre dois intervalos de entrada precisa ser igual ao espaço entre dois de saída.
    /// Esses buracos funcionam como uma sequência densa de mapeamentos de linha 1:1.
    pub line_range_mappings: Vec<LineRangeMapping>,
    pub input_line_count: i32,
}

impl DocumentLineRangeMap {
    pub fn between_outputs(
        input_to_output1: &[LineRangeMapping],
        input_to_output2: &[LineRangeMapping],
        input_line_count: i32,
    ) -> DocumentLineRangeMap {
        let alignments = MappingAlignment::compute(input_to_output1, input_to_output2);
        let mappings = alignments.iter().map(|m| LineRangeMapping::new(m.output1_range, m.output2_range)).collect();
        DocumentLineRangeMap::new(mappings, input_line_count)
    }

    pub fn new(line_range_mappings: Vec<LineRangeMapping>, input_line_count: i32) -> Self {
        assert_fn(|| {
            check_adjacent_items(&line_range_mappings, |m1, m2| {
                m1.input_range.is_before(&m2.input_range)
                    && m1.output_range.is_before(&m2.output_range)
                    && m2.input_range.start_line_number - m1.input_range.end_line_number_exclusive
                        == m2.output_range.start_line_number - m1.output_range.end_line_number_exclusive
            })
        });
        Self { line_range_mappings, input_line_count }
    }

    pub fn project(&self, line_number: i32) -> LineRangeMapping {
        let last_before = self.line_range_mappings.iter().rev().find(|r| r.input_range.start_line_number <= line_number);
        let Some(last_before) = last_before else {
            return LineRangeMapping::new(
                MergeEditorLineRange::from_length(line_number, 1),
                MergeEditorLineRange::from_length(line_number, 1),
            );
        };

        if last_before.input_range.contains(line_number) {
            return *last_before;
        }
        let containing_range = MergeEditorLineRange::from_length(line_number, 1);
        let mapped_range = MergeEditorLineRange::from_length(
            line_number + last_before.output_range.end_line_number_exclusive - last_before.input_range.end_line_number_exclusive,
            1,
        );
        LineRangeMapping::new(containing_range, mapped_range)
    }

    pub fn output_line_count(&self) -> i32 {
        let diff = self
            .line_range_mappings
            .last()
            .map_or(0, |last| last.output_range.end_line_number_exclusive - last.input_range.end_line_number_exclusive);
        self.input_line_count + diff
    }

    pub fn reverse(&self) -> DocumentLineRangeMap {
        DocumentLineRangeMap::new(self.line_range_mappings.iter().map(|r| r.reverse()).collect(), self.output_line_count())
    }
}

/// Alinha dois mapeamentos com um intervalo de entrada comum.
#[derive(Clone, Debug)]
pub struct MappingAlignment<T: AsLineRangeMapping> {
    pub input_range: MergeEditorLineRange,
    pub output1_range: MergeEditorLineRange,
    pub output1_line_mappings: Vec<T>,
    pub output2_range: MergeEditorLineRange,
    pub output2_line_mappings: Vec<T>,
}

impl<T: AsLineRangeMapping + Clone> MappingAlignment<T> {
    pub fn compute(from_input_to_output1: &[T], from_input_to_output2: &[T]) -> Vec<MappingAlignment<T>> {
        // `source` é 0 para a primeira lista e 1 para a segunda.
        let mut combined_diffs: Vec<(usize, T)> = from_input_to_output1
            .iter()
            .map(|diff| (0, diff.clone()))
            .chain(from_input_to_output2.iter().map(|diff| (1, diff.clone())))
            .collect();
        combined_diffs.sort_by_key(|(_, diff)| diff.as_line_range_mapping().input_range.start_line_number);

        let mut current_diffs: [Vec<T>; 2] = [Vec::new(), Vec::new()];
        let mut delta_from_base_to_input = [0i32; 2];

        let mut alignments: Vec<MappingAlignment<T>> = Vec::new();

        let mut current_input_range: Option<MergeEditorLineRange> = None;

        for (source, diff) in combined_diffs {
            let range = diff.as_line_range_mapping().input_range;
            if let Some(current) = current_input_range
                && !current.intersects_or_touches(&range)
            {
                Self::push_and_reset(&current, &mut current_diffs, &delta_from_base_to_input, &mut alignments);
                current_input_range = None;
            }
            delta_from_base_to_input[source] = diff.as_line_range_mapping().resulting_delta_from_original_to_modified();
            current_input_range = Some(match current_input_range {
                Some(current) => current.join(&range),
                None => range,
            });
            current_diffs[source].push(diff);
        }
        if let Some(current) = current_input_range {
            Self::push_and_reset(&current, &mut current_diffs, &delta_from_base_to_input, &mut alignments);
        }

        alignments
    }

    /// O `pushAndReset` do original; `input_range` é o `currentInputRange` em uso.
    fn push_and_reset(
        input_range: &MergeEditorLineRange,
        current_diffs: &mut [Vec<T>; 2],
        delta_from_base_to_input: &[i32; 2],
        alignments: &mut Vec<MappingAlignment<T>>,
    ) {
        let joined = |diffs: &[T]| {
            let mappings: Vec<LineRangeMapping> = diffs.iter().map(|d| *d.as_line_range_mapping()).collect();
            LineRangeMapping::join(&mappings)
        };
        let mapping1 = joined(&current_diffs[0])
            .unwrap_or_else(|| LineRangeMapping::new(*input_range, input_range.delta(delta_from_base_to_input[0])));
        let mapping2 = joined(&current_diffs[1])
            .unwrap_or_else(|| LineRangeMapping::new(*input_range, input_range.delta(delta_from_base_to_input[1])));

        let [diffs1, diffs2] = std::mem::take(current_diffs);
        alignments.push(MappingAlignment {
            input_range: *input_range,
            output1_range: mapping1.extend_input_range(input_range).output_range,
            output1_line_mappings: diffs1,
            output2_range: mapping2.extend_input_range(input_range).output_range,
            output2_line_mappings: diffs2,
        });
    }
}

impl<T: AsLineRangeMapping> fmt::Display for MappingAlignment<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} <- {} -> {}", self.output1_range, self.input_range, self.output2_range)
    }
}

/// Um mapeamento de intervalos de linhas com os mapeamentos de intervalo internos.
#[derive(Clone)]
pub struct DetailedLineRangeMapping {
    pub mapping: LineRangeMapping,
    pub input_text_model: Rc<TextModel>,
    pub output_text_model: Rc<TextModel>,
    pub range_mappings: Vec<RangeMapping>,
}

impl std::fmt::Debug for DetailedLineRangeMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DetailedLineRangeMapping")
            .field("mapping", &self.mapping)
            .field("range_mappings", &self.range_mappings)
            .finish()
    }
}

impl AsLineRangeMapping for DetailedLineRangeMapping {
    fn as_line_range_mapping(&self) -> &LineRangeMapping {
        &self.mapping
    }
}

impl DetailedLineRangeMapping {
    pub fn join_all(mappings: &[DetailedLineRangeMapping]) -> Option<DetailedLineRangeMapping> {
        mappings.iter().fold(None, |acc, cur| {
            Some(match acc {
                Some(acc) => acc.join_with(cur),
                None => cur.clone(),
            })
        })
    }

    pub fn new(
        input_range: MergeEditorLineRange,
        input_text_model: Rc<TextModel>,
        output_range: MergeEditorLineRange,
        output_text_model: Rc<TextModel>,
        range_mappings: Option<Vec<RangeMapping>>,
    ) -> Self {
        let range_mappings = range_mappings
            .unwrap_or_else(|| vec![RangeMapping::new(input_range.to_exclusive_range(), output_range.to_exclusive_range())]);
        Self { mapping: LineRangeMapping::new(input_range, output_range), input_text_model, output_text_model, range_mappings }
    }

    pub fn input_range(&self) -> MergeEditorLineRange {
        self.mapping.input_range
    }

    pub fn output_range(&self) -> MergeEditorLineRange {
        self.mapping.output_range
    }

    pub fn add_output_line_delta(&self, delta: i32) -> DetailedLineRangeMapping {
        DetailedLineRangeMapping::new(
            self.input_range(),
            self.input_text_model.clone(),
            self.output_range().delta(delta),
            self.output_text_model.clone(),
            Some(self.range_mappings.iter().map(|d| d.add_output_line_delta(delta)).collect()),
        )
    }

    pub fn add_input_line_delta(&self, delta: i32) -> DetailedLineRangeMapping {
        DetailedLineRangeMapping::new(
            self.input_range().delta(delta),
            self.input_text_model.clone(),
            self.output_range(),
            self.output_text_model.clone(),
            Some(self.range_mappings.iter().map(|d| d.add_input_line_delta(delta)).collect()),
        )
    }

    /// O `join` de instância do original: os `rangeMappings` do resultado são o padrão (um só, do intervalo inteiro).
    pub fn join_with(&self, other: &DetailedLineRangeMapping) -> DetailedLineRangeMapping {
        DetailedLineRangeMapping::new(
            self.input_range().join(&other.input_range()),
            self.input_text_model.clone(),
            self.output_range().join(&other.output_range()),
            self.output_text_model.clone(),
            None,
        )
    }

    pub fn get_line_edit(&self) -> LineRangeEdit {
        LineRangeEdit::new(self.input_range(), self.get_output_lines())
    }

    pub fn get_reverse_line_edit(&self) -> LineRangeEdit {
        LineRangeEdit::new(self.output_range(), self.get_input_lines())
    }

    fn get_output_lines(&self) -> Vec<JsString> {
        self.output_range().get_lines(&self.output_text_model)
    }

    fn get_input_lines(&self) -> Vec<JsString> {
        self.input_range().get_lines(&self.input_text_model)
    }
}

/// Representa o mapeamento de um intervalo de entrada para um de saída.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeMapping {
    pub input_range: Range,
    pub output_range: Range,
}

impl RangeMapping {
    pub fn new(input_range: Range, output_range: Range) -> Self {
        Self { input_range, output_range }
    }

    pub fn add_output_line_delta(&self, delta_lines: i32) -> RangeMapping {
        RangeMapping::new(
            self.input_range,
            Range::new(
                self.output_range.start_line_number + delta_lines,
                self.output_range.start_column,
                self.output_range.end_line_number + delta_lines,
                self.output_range.end_column,
            ),
        )
    }

    pub fn add_input_line_delta(&self, delta_lines: i32) -> RangeMapping {
        RangeMapping::new(
            Range::new(
                self.input_range.start_line_number + delta_lines,
                self.input_range.start_column,
                self.input_range.end_line_number + delta_lines,
                self.input_range.end_column,
            ),
            self.output_range,
        )
    }

    pub fn reverse(&self) -> RangeMapping {
        RangeMapping::new(self.output_range, self.input_range)
    }
}

impl fmt::Display for RangeMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // O original deixa um TODO para tornar este formato o `Range.toString` padrão.
        fn range_to_string(range: &Range) -> String {
            format!("[{}:{}, {}:{})", range.start_line_number, range.start_column, range.end_line_number, range.end_column)
        }

        write!(f, "{} -> {}", range_to_string(&self.input_range), range_to_string(&self.output_range))
    }
}

/// Representa um mapeamento monótono total de intervalos de um documento para outro.
#[derive(Clone, Debug)]
pub struct DocumentRangeMap {
    /// Os mapeamentos de intervalos que definem este mapeamento de documento. Pode ter buracos.
    pub range_mappings: Vec<RangeMapping>,
    pub input_line_count: i32,
}

impl DocumentRangeMap {
    pub fn new(range_mappings: Vec<RangeMapping>, input_line_count: i32) -> Self {
        assert_fn(|| {
            check_adjacent_items(&range_mappings, |m1, m2| {
                range_is_before_or_touching(&m1.input_range, &m2.input_range)
                    && range_is_before_or_touching(&m1.output_range, &m2.output_range)
            })
        });
        Self { range_mappings, input_line_count }
    }

    pub fn project(&self, position: Position) -> RangeMapping {
        let last_before = self.range_mappings.iter().rev().find(|r| r.input_range.get_start_position().is_before_or_equal(&position));
        let Some(last_before) = last_before else {
            return RangeMapping::new(Range::from_positions(position, position), Range::from_positions(position, position));
        };

        if range_contains_position(&last_before.input_range, &position) {
            return *last_before;
        }

        let dist = TextLength::between_positions(last_before.input_range.get_end_position(), position);
        let output_pos = dist.add_to_position(last_before.output_range.get_end_position());

        RangeMapping::new(Range::from_positions(position, position), Range::from_positions(output_pos, output_pos))
    }

    pub fn project_range(&self, range: &Range) -> RangeMapping {
        let start = self.project(range.get_start_position());
        let end = self.project(range.get_end_position());
        RangeMapping::new(start.input_range.plus_range(&end.input_range), start.output_range.plus_range(&end.output_range))
    }

    pub fn output_line_count(&self) -> i32 {
        let diff = self
            .range_mappings
            .last()
            .map_or(0, |last| last.output_range.end_line_number - last.input_range.end_line_number);
        self.input_line_count + diff
    }

    pub fn reverse(&self) -> DocumentRangeMap {
        DocumentRangeMap::new(self.range_mappings.iter().map(|m| m.reverse()).collect(), self.output_line_count())
    }
}
