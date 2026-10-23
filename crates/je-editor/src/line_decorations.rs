//! Porte de `src/vs/editor/common/viewLayout/lineDecorations.ts`.
//!
//! Texto da linha e nomes de classe são `JsString`/`&[u16]`: colunas e deslocamentos contam unidades UTF-16
//! e a comparação de `className` segue a ordem de unidades do `<` do JavaScript. `LineDecoration.compare`
//! devolve `Ordering` em vez do número do original.

use std::cmp::Ordering;

use je_base::JsString;
use je_base::strings::is_high_surrogate;
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;

use crate::inline_decorations::{InlineDecoration, InlineDecorationType};
use crate::line_part::line_part_metadata;

/// Uma decoração já recortada para uma linha, em colunas (base 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineDecoration {
    pub start_column: i32,
    pub end_column: i32,
    pub class_name: JsString,
    pub r#type: InlineDecorationType,
}

impl LineDecoration {
    pub fn new(
        start_column: i32,
        end_column: i32,
        class_name: impl Into<JsString>,
        r#type: InlineDecorationType,
    ) -> Self {
        Self {
            start_column,
            end_column,
            class_name: class_name.into(),
            r#type,
        }
    }

    pub fn extract_wrapped(arr: &[LineDecoration], start_offset: i32, end_offset: i32) -> Vec<LineDecoration> {
        if arr.is_empty() {
            return Vec::new();
        }
        let start_column = start_offset + 1;
        let end_column = end_offset + 1;
        let line_length = end_offset - start_offset;
        let mut r = Vec::new();
        for dec in arr {
            if dec.end_column <= start_column || dec.start_column >= end_column {
                continue;
            }
            r.push(LineDecoration::new(
                1.max(dec.start_column - start_column + 1),
                (line_length + 1).min(dec.end_column - start_column + 1),
                dec.class_name.clone(),
                dec.r#type,
            ));
        }
        r
    }

    pub fn filter(
        line_decorations: &[InlineDecoration],
        line_number: i32,
        min_line_column: i32,
        max_line_column: i32,
    ) -> Vec<LineDecoration> {
        if line_decorations.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::new();

        for d in line_decorations {
            let range = &d.range;

            if range.end_line_number < line_number || range.start_line_number > line_number {
                // Ignora decorações fora desta linha.
                continue;
            }

            if range.is_empty()
                && matches!(
                    d.r#type,
                    InlineDecorationType::Regular | InlineDecorationType::RegularAffectingLetterSpacing
                )
            {
                // Ignora decorações de intervalo vazio.
                continue;
            }

            let start_column = if range.start_line_number == line_number {
                range.start_column
            } else {
                min_line_column
            };
            let end_column = if range.end_line_number == line_number {
                range.end_column
            } else {
                max_line_column
            };

            result.push(LineDecoration::new(
                start_column,
                end_column,
                d.inline_class_name.clone(),
                d.r#type,
            ));
        }

        result
    }

    fn type_compare(a: InlineDecorationType, b: InlineDecorationType) -> i32 {
        // WidthOnly, Before, After, Regular, RegularAffectingLetterSpacing.
        // As de largura apenas vêm de texto injetado, que renderiza antes de qualquer outra decoração.
        const ORDER: [i32; 5] = [3, 1, 2, 4, 0];
        ORDER[a as usize] - ORDER[b as usize]
    }

    pub fn compare(a: &LineDecoration, b: &LineDecoration) -> Ordering {
        if a.start_column != b.start_column {
            return a.start_column.cmp(&b.start_column);
        }

        if a.end_column != b.end_column {
            return a.end_column.cmp(&b.end_column);
        }

        let type_cmp = LineDecoration::type_compare(a.r#type, b.r#type);
        if type_cmp != 0 {
            return type_cmp.cmp(&0);
        }

        if a.class_name != b.class_name {
            return a.class_name.cmp(&b.class_name);
        }

        Ordering::Equal
    }
}

/// Um trecho contíguo de deslocamentos (ambos inclusivos) com as classes e os metadados que valem nele.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecorationSegment {
    pub start_offset: i32,
    pub end_offset: i32,
    pub class_name: JsString,
    pub metadata: i32,
}

impl DecorationSegment {
    pub fn new(start_offset: i32, end_offset: i32, class_name: impl Into<JsString>, metadata: i32) -> Self {
        Self {
            start_offset,
            end_offset,
            class_name: class_name.into(),
            metadata,
        }
    }
}

/// `Array.prototype.join(' ')` sobre nomes de classe.
fn join_class_names(class_names: &[JsString]) -> JsString {
    let mut out = JsString::new();
    for (i, name) in class_names.iter().enumerate() {
        if i > 0 {
            out.push_unit(u16::from(b' '));
        }
        out.push_units(name.units());
    }
    out
}

#[derive(Default)]
struct Stack {
    stop_offsets: Vec<i32>,
    class_names: Vec<JsString>,
    metadata: Vec<i32>,
}

impl Stack {
    fn new() -> Self {
        Self::default()
    }

    fn count(&self) -> usize {
        self.stop_offsets.len()
    }

    fn metadata_of(metadata: &[i32]) -> i32 {
        metadata.iter().fold(0, |result, m| result | m)
    }

    fn consume_lower_than(&mut self, max_stop_offset: i32, mut next_start_offset: i32, result: &mut Vec<DecorationSegment>) -> i32 {
        while self.count() > 0 && self.stop_offsets[0] < max_stop_offset {
            let mut i = 0;

            // Pega todos os offsets de parada iguais.
            while i + 1 < self.count() && self.stop_offsets[i] == self.stop_offsets[i + 1] {
                i += 1;
            }

            // Na prática consome os primeiros i + 1 elementos da pilha.
            result.push(DecorationSegment::new(
                next_start_offset,
                self.stop_offsets[i],
                join_class_names(&self.class_names),
                Stack::metadata_of(&self.metadata),
            ));
            next_start_offset = self.stop_offsets[i] + 1;

            // Consome-os.
            self.stop_offsets.drain(..i + 1);
            self.class_names.drain(..i + 1);
            self.metadata.drain(..i + 1);
        }

        if self.count() > 0 && next_start_offset < max_stop_offset {
            result.push(DecorationSegment::new(
                next_start_offset,
                max_stop_offset - 1,
                join_class_names(&self.class_names),
                Stack::metadata_of(&self.metadata),
            ));
            next_start_offset = max_stop_offset;
        }

        next_start_offset
    }

    fn insert(&mut self, stop_offset: i32, class_name: JsString, metadata: i32) {
        if self.stop_offsets.last().is_none_or(|&last| last <= stop_offset) {
            // Insere no fim.
            self.stop_offsets.push(stop_offset);
            self.class_names.push(class_name);
            self.metadata.push(metadata);
        } else if let Some(i) = self.stop_offsets.iter().position(|&s| s >= stop_offset) {
            // Posição de inserção de `stop_offset`.
            self.stop_offsets.insert(i, stop_offset);
            self.class_names.insert(i, class_name);
            self.metadata.insert(i, metadata);
        }
    }
}

/// `charCodeAt(column - 2)` é um high surrogate? Fora do texto o original dá `NaN`, que não é.
fn is_high_surrogate_before(line_content: &[u16], column: i32) -> bool {
    column > 1 && line_content.get((column - 2) as usize).is_some_and(|&c| is_high_surrogate(c))
}

pub struct LineDecorationsNormalizer;

impl LineDecorationsNormalizer {
    /// Normaliza as decorações da linha. Decorações sobrepostas geram vários segmentos.
    pub fn normalize(line_content: &[u16], line_decorations: &[LineDecoration]) -> Vec<DecorationSegment> {
        if line_decorations.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::new();

        let mut stack = Stack::new();
        let mut next_start_offset = 0;

        for d in line_decorations {
            let mut start_column = d.start_column;
            let mut end_column = d.end_column;
            let class_name = d.class_name.clone();
            let metadata = match d.r#type {
                InlineDecorationType::Before => line_part_metadata::PSEUDO_BEFORE,
                InlineDecorationType::After => line_part_metadata::PSEUDO_AFTER,
                _ => 0,
            };

            // Se a posição cair no meio de um par surrogate alto-baixo, move para antes do par.
            if is_high_surrogate_before(line_content, start_column) {
                start_column -= 1;
            }

            if is_high_surrogate_before(line_content, end_column) {
                end_column -= 1;
            }

            let current_start_offset = start_column - 1;
            let current_end_offset = end_column - 2;

            next_start_offset = stack.consume_lower_than(current_start_offset, next_start_offset, &mut result);

            if stack.count() == 0 {
                next_start_offset = current_start_offset;
            }
            stack.insert(current_end_offset, class_name, metadata);
        }

        stack.consume_lower_than(MAX_SAFE_SMALL_INTEGER, next_start_offset, &mut result);

        result
    }
}
