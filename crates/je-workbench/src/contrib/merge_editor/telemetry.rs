//! Porte de `src/vs/workbench/contrib/mergeEditor/browser/telemetry.ts`.
//!
//! Notas de tradução:
//! - `@ITelemetryService` chega por `new` (`Rc<dyn ITelemetryService>`).
//! - Os tipos genéricos de `publicLog2<{...}, {...}>` (formato dos dados e a classificação para o linter de telemetria)
//!   não têm leitor em Rust; o que sai é só o objeto de dados, como um [`Dynamic`].
//! - Os argumentos que o original recebe como objeto literal são structs com os mesmos campos.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_platform::telemetry::telemetry::ITelemetryService;

use super::model::modified_base_range::InputNumber;

pub struct MergeEditorTelemetry {
    telemetry_service: Rc<dyn ITelemetryService>,
}

pub struct MergeEditorOpenedArgs {
    pub conflict_count: usize,
    pub combinable_conflict_count: usize,

    pub base_visible: bool,
    pub is_column_view: bool,
    pub base_top: bool,
}

pub struct LayoutChangeArgs {
    pub base_visible: bool,
    pub is_column_view: bool,
    pub base_top: bool,
}

pub struct MergeEditorClosedArgs {
    pub conflict_count: usize,
    pub combinable_conflict_count: usize,

    pub duration_opened_secs: f64,
    pub remaining_conflict_count: usize,
    pub accepted: bool,

    pub conflicts_resolved_with_base: usize,
    pub conflicts_resolved_with_input1: usize,
    pub conflicts_resolved_with_input2: usize,
    pub conflicts_resolved_with_smart_combination: usize,

    pub manually_solved_conflict_count_that_equal_none: usize,
    pub manually_solved_conflict_count_that_equal_smart_combine: usize,
    pub manually_solved_conflict_count_that_equal_input1: usize,
    pub manually_solved_conflict_count_that_equal_input2: usize,

    pub manually_solved_conflict_count_that_equal_none_and_started_with_base: usize,
    pub manually_solved_conflict_count_that_equal_none_and_started_with_input1: usize,
    pub manually_solved_conflict_count_that_equal_none_and_started_with_input2: usize,
    pub manually_solved_conflict_count_that_equal_none_and_started_with_both_non_smart: usize,
    pub manually_solved_conflict_count_that_equal_none_and_started_with_both_smart: usize,
}

fn number(value: usize) -> Dynamic {
    Dynamic::Number(value as f64)
}

impl MergeEditorTelemetry {
    pub fn new(telemetry_service: Rc<dyn ITelemetryService>) -> Self {
        Self { telemetry_service }
    }

    fn log(&self, event_name: &str, data: Vec<(&str, Dynamic)>) {
        self.telemetry_service.public_log2(event_name, Some(Dynamic::object(data)));
    }

    pub fn report_merge_editor_opened(&self, args: MergeEditorOpenedArgs) {
        self.log(
            "mergeEditor.opened",
            vec![
                ("conflictCount", number(args.conflict_count)),
                ("combinableConflictCount", number(args.combinable_conflict_count)),
                ("baseVisible", Dynamic::Bool(args.base_visible)),
                ("isColumnView", Dynamic::Bool(args.is_column_view)),
                ("baseTop", Dynamic::Bool(args.base_top)),
            ],
        );
    }

    pub fn report_layout_change(&self, args: LayoutChangeArgs) {
        self.log(
            "mergeEditor.layoutChanged",
            vec![
                ("baseVisible", Dynamic::Bool(args.base_visible)),
                ("isColumnView", Dynamic::Bool(args.is_column_view)),
                ("baseTop", Dynamic::Bool(args.base_top)),
            ],
        );
    }

    pub fn report_merge_editor_closed(&self, args: MergeEditorClosedArgs) {
        self.log(
            "mergeEditor.closed",
            vec![
                ("conflictCount", number(args.conflict_count)),
                ("combinableConflictCount", number(args.combinable_conflict_count)),
                ("durationOpenedSecs", Dynamic::Number(args.duration_opened_secs)),
                ("remainingConflictCount", number(args.remaining_conflict_count)),
                ("accepted", Dynamic::Bool(args.accepted)),
                ("conflictsResolvedWithBase", number(args.conflicts_resolved_with_base)),
                ("conflictsResolvedWithInput1", number(args.conflicts_resolved_with_input1)),
                ("conflictsResolvedWithInput2", number(args.conflicts_resolved_with_input2)),
                ("conflictsResolvedWithSmartCombination", number(args.conflicts_resolved_with_smart_combination)),
                ("manuallySolvedConflictCountThatEqualNone", number(args.manually_solved_conflict_count_that_equal_none)),
                ("manuallySolvedConflictCountThatEqualSmartCombine", number(args.manually_solved_conflict_count_that_equal_smart_combine)),
                ("manuallySolvedConflictCountThatEqualInput1", number(args.manually_solved_conflict_count_that_equal_input1)),
                ("manuallySolvedConflictCountThatEqualInput2", number(args.manually_solved_conflict_count_that_equal_input2)),
                (
                    "manuallySolvedConflictCountThatEqualNoneAndStartedWithBase",
                    number(args.manually_solved_conflict_count_that_equal_none_and_started_with_base),
                ),
                (
                    "manuallySolvedConflictCountThatEqualNoneAndStartedWithInput1",
                    number(args.manually_solved_conflict_count_that_equal_none_and_started_with_input1),
                ),
                (
                    "manuallySolvedConflictCountThatEqualNoneAndStartedWithInput2",
                    number(args.manually_solved_conflict_count_that_equal_none_and_started_with_input2),
                ),
                (
                    "manuallySolvedConflictCountThatEqualNoneAndStartedWithBothNonSmart",
                    number(args.manually_solved_conflict_count_that_equal_none_and_started_with_both_non_smart),
                ),
                (
                    "manuallySolvedConflictCountThatEqualNoneAndStartedWithBothSmart",
                    number(args.manually_solved_conflict_count_that_equal_none_and_started_with_both_smart),
                ),
            ],
        );
    }

    pub fn report_accept_invoked(&self, input_number: InputNumber, other_accepted: bool) {
        self.log(
            "mergeEditor.action.accept",
            vec![("otherAccepted", Dynamic::Bool(other_accepted)), ("isInput1", Dynamic::Bool(input_number == InputNumber::One))],
        );
    }

    pub fn report_smart_combination_invoked(&self, other_accepted: bool) {
        self.log("mergeEditor.action.smartCombination", vec![("otherAccepted", Dynamic::Bool(other_accepted))]);
    }

    pub fn report_remove_invoked(&self, input_number: InputNumber, other_accepted: bool) {
        self.log(
            "mergeEditor.action.remove",
            vec![("otherAccepted", Dynamic::Bool(other_accepted)), ("isInput1", Dynamic::Bool(input_number == InputNumber::One))],
        );
    }

    pub fn report_reset_to_base_invoked(&self) {
        self.log("mergeEditor.action.resetToBase", Vec::new());
    }

    pub fn report_navigation_to_next_conflict(&self) {
        self.log("mergeEditor.action.goToNextConflict", Vec::new());
    }

    pub fn report_navigation_to_previous_conflict(&self) {
        self.log("mergeEditor.action.goToPreviousConflict", Vec::new());
    }

    pub fn report_conflict_counter_clicked(&self) {
        self.log("mergeEditor.action.conflictCounterClicked", Vec::new());
    }
}
