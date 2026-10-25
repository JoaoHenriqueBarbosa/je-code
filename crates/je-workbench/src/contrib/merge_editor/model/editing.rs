//! Porte de `src/vs/workbench/contrib/mergeEditor/browser/model/editing.ts`.
//!
//! Notas de tradução:
//! - `IIdentifiedSingleEditOperation` é o [`SingleEditOperation`] do je-editor, que tem um tipo só para as duas
//!   interfaces do original.
//! - `Number.MAX_SAFE_INTEGER` das colunas vira `i32::MAX`, a maior coluna que o `Range` representa (o modelo
//!   limita a coluna ao fim da linha na validação, como no original).
//! - O texto é UTF-16 ([`JsString`]).

use je_base::JsString;
use je_base::arrays;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::range::Range;

use super::line_range::MergeEditorLineRange;

/// Uma edição expressa em linhas inteiras: antes de `range.start_line_number`, apaga `range.length()` linhas e
/// insere `new_lines`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineRangeEdit {
    pub range: MergeEditorLineRange,
    pub new_lines: Vec<JsString>,
}

impl LineRangeEdit {
    pub fn new(range: MergeEditorLineRange, new_lines: Vec<JsString>) -> Self {
        Self { range, new_lines }
    }

    pub fn equals(&self, other: &LineRangeEdit) -> bool {
        self.range == other.range && arrays::equals(Some(&self.new_lines), Some(&other.new_lines), |a, b| a == b)
    }

    pub fn to_edits(&self, model_line_count: i32) -> Vec<SingleEditOperation> {
        LineEdits::new(vec![self.clone()]).to_edits(model_line_count)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangeEdit {
    pub range: Range,
    pub new_text: JsString,
}

impl RangeEdit {
    pub fn new(range: Range, new_text: JsString) -> Self {
        Self { range, new_text }
    }

    pub fn equals(&self, other: &RangeEdit) -> bool {
        self.range == other.range && self.new_text == other.new_text
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineEdits {
    pub edits: Vec<LineRangeEdit>,
}

impl LineEdits {
    pub fn new(edits: Vec<LineRangeEdit>) -> Self {
        Self { edits }
    }

    pub fn to_edits(&self, model_line_count: i32) -> Vec<SingleEditOperation> {
        self.edits
            .iter()
            .map(|e| {
                if e.range.end_line_number_exclusive <= model_line_count {
                    return SingleEditOperation::new(
                        Range::new(e.range.start_line_number, 1, e.range.end_line_number_exclusive, 1),
                        Some(join_with_suffix(&e.new_lines)),
                        false,
                    );
                }

                if e.range.start_line_number == 1 {
                    return SingleEditOperation::new(
                        Range::new(1, 1, model_line_count, i32::MAX),
                        Some(join(&e.new_lines)),
                        false,
                    );
                }

                SingleEditOperation::new(
                    Range::new(e.range.start_line_number - 1, i32::MAX, model_line_count, i32::MAX),
                    Some(join_with_prefix(&e.new_lines)),
                    false,
                )
            })
            .collect()
    }
}

/// `lines.map(s => s + '\n').join('')`.
fn join_with_suffix(lines: &[JsString]) -> JsString {
    let mut text = JsString::new();
    for line in lines {
        text.push_units(line.units());
        text.push_unit(u16::from(b'\n'));
    }
    text
}

/// `lines.join('\n')`.
fn join(lines: &[JsString]) -> JsString {
    let mut text = JsString::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            text.push_unit(u16::from(b'\n'));
        }
        text.push_units(line.units());
    }
    text
}

/// `lines.map(s => '\n' + s).join('')`.
fn join_with_prefix(lines: &[JsString]) -> JsString {
    let mut text = JsString::new();
    for line in lines {
        text.push_unit(u16::from(b'\n'));
        text.push_units(line.units());
    }
    text
}
