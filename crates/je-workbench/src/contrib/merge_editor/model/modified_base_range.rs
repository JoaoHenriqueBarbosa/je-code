//! Porte de `src/vs/workbench/contrib/mergeEditor/browser/model/modifiedBaseRange.ts`.
//!
//! Notas de tradução:
//! - `ITextModel` é o `Rc<TextModel>` do je-editor.
//! - `ModifiedBaseRangeState` (a união das seis classes do original) é um `enum`: `Base`, `Input1`, `Input2`,
//!   `Both { first_input, smart_combination }` e `Unrecognized`. O `kind` é o [`ModifiedBaseRangeStateKind`], o
//!   `equals` é o `PartialEq` e o `toString()` é o `Display`. O `toString` de `Both` devolve `2✓` e o
//!   `withInputValue(2, false)` de `Input1` devolve `Input2`, como no original (que repete o `Input2` na união).
//! - `InputNumber` (`1 | 2`) é o `enum` [`InputNumber`].
//! - Os caches `smartInput1LineRangeEdit`... (`null` = não calculado, `undefined` = sem edição) são
//!   `RefCell<Option<Option<LineRangeEdit>>>`.
//! - `new BugIndicatingError(...)` levantado é o pânico de [`errors::throw`].
//! - `ModifiedBaseRangeState` é `StrictEquals` pela igualdade de valor (o `===` do original compara a identidade, e cada
//!   `withInputValue` cria um objeto novo; a diferença só poupa notificações redundantes).
//! - O `sort` do original é estável, como o `sort_by` do Rust.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use je_base::JsString;
use je_base::observable::StrictEquals;
use je_base::arrays;
use je_base::errors::{self, Error};
use je_base::strings::split_lines;
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::TextModel;

use super::editing::{LineRangeEdit, RangeEdit};
use super::line_range::MergeEditorLineRange;
use super::mapping::{DetailedLineRangeMapping, MappingAlignment};

/// Descreve as modificações na entrada 1 e na entrada 2 para um intervalo da base.
///
/// A UI oferece aplicar todas as mudanças da entrada 1, da 2 ou das duas. Imutável (os caches são só memória).
#[derive(Clone)]
pub struct ModifiedBaseRange {
    pub base_range: MergeEditorLineRange,
    pub base_text_model: Rc<TextModel>,
    pub input1_range: MergeEditorLineRange,
    pub input1_text_model: Rc<TextModel>,
    /// Da base para a entrada 1.
    pub input1_diffs: Vec<DetailedLineRangeMapping>,
    pub input2_range: MergeEditorLineRange,
    pub input2_text_model: Rc<TextModel>,
    /// Da base para a entrada 2.
    pub input2_diffs: Vec<DetailedLineRangeMapping>,

    pub input1_combined_diff: Option<DetailedLineRangeMapping>,
    pub input2_combined_diff: Option<DetailedLineRangeMapping>,
    pub is_equal_change: bool,

    smart_input1_line_range_edit: RefCell<Option<Option<LineRangeEdit>>>,
    smart_input2_line_range_edit: RefCell<Option<Option<LineRangeEdit>>>,
    dumb_input1_line_range_edit: RefCell<Option<Option<LineRangeEdit>>>,
    dumb_input2_line_range_edit: RefCell<Option<Option<LineRangeEdit>>>,
}

/// O que `getEditForBase` devolve.
pub struct EditForBase {
    pub edit: Option<LineRangeEdit>,
    pub effective_state: ModifiedBaseRangeState,
}

impl ModifiedBaseRange {
    pub fn from_diffs(
        diffs1: &[DetailedLineRangeMapping],
        diffs2: &[DetailedLineRangeMapping],
        base_text_model: &Rc<TextModel>,
        input1_text_model: &Rc<TextModel>,
        input2_text_model: &Rc<TextModel>,
    ) -> Vec<ModifiedBaseRange> {
        let alignments = MappingAlignment::compute(diffs1, diffs2);
        alignments
            .into_iter()
            .map(|a| {
                ModifiedBaseRange::new(
                    a.input_range,
                    base_text_model.clone(),
                    a.output1_range,
                    input1_text_model.clone(),
                    a.output1_line_mappings,
                    a.output2_range,
                    input2_text_model.clone(),
                    a.output2_line_mappings,
                )
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_range: MergeEditorLineRange,
        base_text_model: Rc<TextModel>,
        input1_range: MergeEditorLineRange,
        input1_text_model: Rc<TextModel>,
        input1_diffs: Vec<DetailedLineRangeMapping>,
        input2_range: MergeEditorLineRange,
        input2_text_model: Rc<TextModel>,
        input2_diffs: Vec<DetailedLineRangeMapping>,
    ) -> Self {
        let input1_combined_diff = DetailedLineRangeMapping::join_all(&input1_diffs);
        let input2_combined_diff = DetailedLineRangeMapping::join_all(&input2_diffs);
        let is_equal_change =
            arrays::equals(Some(&input1_diffs), Some(&input2_diffs), |a, b| a.get_line_edit().equals(&b.get_line_edit()));
        if input1_diffs.is_empty() && input2_diffs.is_empty() {
            errors::throw(Error::bug_indicating(Some("must have at least one diff")));
        }
        Self {
            base_range,
            base_text_model,
            input1_range,
            input1_text_model,
            input1_diffs,
            input2_range,
            input2_text_model,
            input2_diffs,
            input1_combined_diff,
            input2_combined_diff,
            is_equal_change,
            smart_input1_line_range_edit: RefCell::new(None),
            smart_input2_line_range_edit: RefCell::new(None),
            dumb_input1_line_range_edit: RefCell::new(None),
            dumb_input2_line_range_edit: RefCell::new(None),
        }
    }

    pub fn get_input_range(&self, input_number: InputNumber) -> MergeEditorLineRange {
        match input_number {
            InputNumber::One => self.input1_range,
            InputNumber::Two => self.input2_range,
        }
    }

    pub fn get_input_combined_diff(&self, input_number: InputNumber) -> Option<&DetailedLineRangeMapping> {
        match input_number {
            InputNumber::One => self.input1_combined_diff.as_ref(),
            InputNumber::Two => self.input2_combined_diff.as_ref(),
        }
    }

    pub fn get_input_diffs(&self, input_number: InputNumber) -> &[DetailedLineRangeMapping] {
        match input_number {
            InputNumber::One => &self.input1_diffs,
            InputNumber::Two => &self.input2_diffs,
        }
    }

    pub fn is_conflicting(&self) -> bool {
        !self.input1_diffs.is_empty() && !self.input2_diffs.is_empty()
    }

    pub fn can_be_combined(&self) -> bool {
        self.smart_combine_inputs(InputNumber::One).is_some()
    }

    pub fn is_order_relevant(&self) -> bool {
        let (Some(input1), Some(input2)) =
            (self.smart_combine_inputs(InputNumber::One), self.smart_combine_inputs(InputNumber::Two))
        else {
            return false;
        };
        !input1.equals(&input2)
    }

    pub fn get_edit_for_base(&self, state: &ModifiedBaseRangeState) -> EditForBase {
        let mut diffs: Vec<(&DetailedLineRangeMapping, InputNumber)> = Vec::new();
        if state.includes_input1()
            && let Some(diff) = &self.input1_combined_diff
        {
            diffs.push((diff, InputNumber::One));
        }
        if state.includes_input2()
            && let Some(diff) = &self.input2_combined_diff
        {
            diffs.push((diff, InputNumber::Two));
        }

        if diffs.is_empty() {
            return EditForBase { edit: None, effective_state: ModifiedBaseRangeState::Base };
        }
        if diffs.len() == 1 {
            return EditForBase {
                edit: Some(diffs[0].0.get_line_edit()),
                effective_state: ModifiedBaseRangeState::Base.with_input_value(diffs[0].1, true, false),
            };
        }

        let ModifiedBaseRangeState::Both { first_input, smart_combination } = *state else {
            errors::throw(Error::bug_indicating(None));
        };

        let smart_combined_edit =
            if smart_combination { self.smart_combine_inputs(first_input) } else { self.dumb_combine_inputs(first_input) };
        if let Some(edit) = smart_combined_edit {
            return EditForBase { edit: Some(edit), effective_state: *state };
        }

        let other = get_other_input_number(first_input);
        EditForBase {
            edit: Some(diffs[other.number() - 1].0.get_line_edit()),
            effective_state: ModifiedBaseRangeState::Base.with_input_value(other, true, false),
        }
    }

    fn smart_cache(&self, first_input: InputNumber) -> &RefCell<Option<Option<LineRangeEdit>>> {
        match first_input {
            InputNumber::One => &self.smart_input1_line_range_edit,
            InputNumber::Two => &self.smart_input2_line_range_edit,
        }
    }

    fn dumb_cache(&self, first_input: InputNumber) -> &RefCell<Option<Option<LineRangeEdit>>> {
        match first_input {
            InputNumber::One => &self.dumb_input1_line_range_edit,
            InputNumber::Two => &self.dumb_input2_line_range_edit,
        }
    }

    fn smart_combine_inputs(&self, first_input: InputNumber) -> Option<LineRangeEdit> {
        if let Some(cached) = self.smart_cache(first_input).borrow().as_ref() {
            return cached.clone();
        }

        let mut combined_diffs: Vec<(&super::mapping::RangeMapping, InputNumber)> = Vec::new();
        for diffs in &self.input1_diffs {
            combined_diffs.extend(diffs.range_mappings.iter().map(|diff| (diff, InputNumber::One)));
        }
        for diffs in &self.input2_diffs {
            combined_diffs.extend(diffs.range_mappings.iter().map(|diff| (diff, InputNumber::Two)));
        }
        combined_diffs.sort_by(|(a_diff, a_input), (b_diff, b_input)| {
            let by_start = Range::compare_ranges_using_starts(Some(&a_diff.input_range), Some(&b_diff.input_range)).cmp(&0);
            by_start.then_with(|| {
                let rank = |input: &InputNumber| if *input == first_input { 1 } else { 2 };
                rank(a_input).cmp(&rank(b_input))
            })
        });

        let sorted_edits: Vec<RangeEdit> = combined_diffs
            .iter()
            .map(|(diff, input)| {
                let source_text_model = match input {
                    InputNumber::One => &self.input1_text_model,
                    InputNumber::Two => &self.input2_text_model,
                };
                RangeEdit::new(diff.input_range, source_text_model.get_value_in_range(&diff.output_range))
            })
            .collect();

        let result = edits_to_line_range_edit(&self.base_range, &sorted_edits, &self.base_text_model);
        *self.smart_cache(first_input).borrow_mut() = Some(result.clone());
        result
    }

    fn dumb_combine_inputs(&self, first_input: InputNumber) -> Option<LineRangeEdit> {
        if let Some(cached) = self.dumb_cache(first_input).borrow().as_ref() {
            return cached.clone();
        }

        let mut input1_lines = self.input1_range.get_lines(&self.input1_text_model);
        let mut input2_lines = self.input2_range.get_lines(&self.input2_text_model);
        if first_input == InputNumber::Two {
            std::mem::swap(&mut input1_lines, &mut input2_lines);
        }

        input1_lines.extend(input2_lines);
        let result = Some(LineRangeEdit::new(self.base_range, input1_lines));
        *self.dumb_cache(first_input).borrow_mut() = Some(result.clone());
        result
    }
}

fn edits_to_line_range_edit(range: &MergeEditorLineRange, sorted_edits: &[RangeEdit], text_model: &TextModel) -> Option<LineRangeEdit> {
    let mut text = JsString::new();
    let starts_line_before = range.start_line_number > 1;
    let mut current_position = if starts_line_before {
        Position::new(range.start_line_number - 1, text_model.get_line_max_column(range.start_line_number - 1))
    } else {
        Position::new(range.start_line_number, 1)
    };

    for edit in sorted_edits {
        let diff_start = edit.range.get_start_position();
        if !current_position.is_before_or_equal(&diff_start) {
            return None;
        }
        let mut original_text = text_model.get_value_in_range(&Range::from_positions(current_position, diff_start));
        if diff_start.line_number > text_model.get_line_count() {
            // assert diffStart.lineNumber === textModel.getLineCount() + 1
            // getValueInRange não inclui esta quebra de linha virtual, pois o documento acaba na linha anterior.
            // endsLineAfter será falso.
            original_text.push_unit(u16::from(b'\n'));
        }
        text.push_units(original_text.units());
        text.push_units(edit.new_text.units());
        current_position = edit.range.get_end_position();
    }

    let ends_line_after = range.end_line_number_exclusive <= text_model.get_line_count();
    let end = if ends_line_after {
        Position::new(range.end_line_number_exclusive, 1)
    } else {
        Position::new(range.end_line_number_exclusive - 1, MAX_SAFE_SMALL_INTEGER)
    };

    let original_text = text_model.get_value_in_range(&Range::from_positions(current_position, end));
    text.push_units(original_text.units());

    let mut lines: Vec<JsString> = split_lines(text.units()).into_iter().map(JsString::from).collect();
    if starts_line_before {
        if lines.first().is_none_or(|l| !l.is_empty()) {
            return None;
        }
        lines.remove(0);
    }
    if ends_line_after {
        if lines.last().is_none_or(|l| !l.is_empty()) {
            return None;
        }
        lines.pop();
    }
    Some(LineRangeEdit::new(*range, lines))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifiedBaseRangeStateKind {
    Base,
    Input1,
    Input2,
    Both,
    Unrecognized,
}

/// `InputNumber` (`1 | 2`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputNumber {
    One = 1,
    Two = 2,
}

impl InputNumber {
    pub fn number(self) -> usize {
        self as usize
    }
}

pub fn get_other_input_number(input_number: InputNumber) -> InputNumber {
    match input_number {
        InputNumber::One => InputNumber::Two,
        InputNumber::Two => InputNumber::One,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputState {
    Excluded = 0,
    First = 1,
    Second = 2,
    Unrecognized = 3,
}

/// O estado de um intervalo modificado da base.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifiedBaseRangeState {
    Base,
    Input1,
    Input2,
    Both { first_input: InputNumber, smart_combination: bool },
    Unrecognized,
}

impl ModifiedBaseRangeState {
    pub fn kind(&self) -> ModifiedBaseRangeStateKind {
        match self {
            ModifiedBaseRangeState::Base => ModifiedBaseRangeStateKind::Base,
            ModifiedBaseRangeState::Input1 => ModifiedBaseRangeStateKind::Input1,
            ModifiedBaseRangeState::Input2 => ModifiedBaseRangeStateKind::Input2,
            ModifiedBaseRangeState::Both { .. } => ModifiedBaseRangeStateKind::Both,
            ModifiedBaseRangeState::Unrecognized => ModifiedBaseRangeStateKind::Unrecognized,
        }
    }

    pub fn includes_input1(&self) -> bool {
        matches!(self, ModifiedBaseRangeState::Input1 | ModifiedBaseRangeState::Both { .. })
    }

    pub fn includes_input2(&self) -> bool {
        matches!(self, ModifiedBaseRangeState::Input2 | ModifiedBaseRangeState::Both { .. })
    }

    pub fn includes_input(&self, input_number: InputNumber) -> bool {
        match input_number {
            InputNumber::One => self.includes_input1(),
            InputNumber::Two => self.includes_input2(),
        }
    }

    pub fn is_input_included(&self, input_number: InputNumber) -> bool {
        self.includes_input(input_number)
    }

    pub fn swap(&self) -> ModifiedBaseRangeState {
        match self {
            ModifiedBaseRangeState::Base => ModifiedBaseRangeState::Base,
            ModifiedBaseRangeState::Input1 => ModifiedBaseRangeState::Input2,
            ModifiedBaseRangeState::Input2 => ModifiedBaseRangeState::Input1,
            ModifiedBaseRangeState::Both { first_input, smart_combination } => {
                ModifiedBaseRangeState::Both { first_input: get_other_input_number(*first_input), smart_combination: *smart_combination }
            }
            ModifiedBaseRangeState::Unrecognized => ModifiedBaseRangeState::Unrecognized,
        }
    }

    pub fn with_input_value(&self, input_number: InputNumber, value: bool, smart_combination: bool) -> ModifiedBaseRangeState {
        match self {
            ModifiedBaseRangeState::Base => match (input_number, value) {
                (InputNumber::One, true) => ModifiedBaseRangeState::Input1,
                (InputNumber::Two, true) => ModifiedBaseRangeState::Input2,
                _ => ModifiedBaseRangeState::Base,
            },
            ModifiedBaseRangeState::Input1 => match input_number {
                InputNumber::One => {
                    if value {
                        ModifiedBaseRangeState::Input1
                    } else {
                        ModifiedBaseRangeState::Base
                    }
                }
                InputNumber::Two => {
                    if value {
                        ModifiedBaseRangeState::Both { first_input: InputNumber::One, smart_combination }
                    } else {
                        ModifiedBaseRangeState::Input2
                    }
                }
            },
            ModifiedBaseRangeState::Input2 => match input_number {
                InputNumber::Two => {
                    if value {
                        ModifiedBaseRangeState::Input2
                    } else {
                        ModifiedBaseRangeState::Base
                    }
                }
                InputNumber::One => {
                    if value {
                        ModifiedBaseRangeState::Both { first_input: InputNumber::Two, smart_combination }
                    } else {
                        ModifiedBaseRangeState::Input2
                    }
                }
            },
            ModifiedBaseRangeState::Both { .. } => {
                if value {
                    return *self;
                }
                match input_number {
                    InputNumber::One => ModifiedBaseRangeState::Input2,
                    InputNumber::Two => ModifiedBaseRangeState::Input1,
                }
            }
            ModifiedBaseRangeState::Unrecognized => {
                if !value {
                    return *self;
                }
                match input_number {
                    InputNumber::One => ModifiedBaseRangeState::Input1,
                    InputNumber::Two => ModifiedBaseRangeState::Input2,
                }
            }
        }
    }

    pub fn equals(&self, other: &ModifiedBaseRangeState) -> bool {
        self == other
    }

    pub fn toggle(&self, input_number: InputNumber) -> ModifiedBaseRangeState {
        self.with_input_value(input_number, !self.includes_input(input_number), true)
    }

    pub fn get_input(&self, input_number: InputNumber) -> InputState {
        if let ModifiedBaseRangeState::Both { first_input, .. } = self {
            return if input_number == *first_input { InputState::First } else { InputState::Second };
        }
        if !self.is_input_included(input_number) {
            return InputState::Excluded;
        }
        InputState::First
    }
}

impl StrictEquals for ModifiedBaseRangeState {
    /// O `===` do original compara a identidade do objeto (cada `withInputValue` cria um novo); aqui a igualdade é a do
    /// valor (`equals`), que só deixa de notificar quando o estado novo é igual ao antigo.
    fn strict_equals(&self, other: &Self) -> bool {
        self.equals(other)
    }
}

impl fmt::Display for ModifiedBaseRangeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ModifiedBaseRangeState::Base => "base",
            ModifiedBaseRangeState::Input1 => "1✓",
            ModifiedBaseRangeState::Input2 | ModifiedBaseRangeState::Both { .. } => "2✓",
            ModifiedBaseRangeState::Unrecognized => "unrecognized",
        })
    }
}
