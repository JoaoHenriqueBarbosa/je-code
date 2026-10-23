//! Porte de `src/vs/editor/test/common/viewLayout/lineDecorations.test.ts` (suíte `Editor ViewLayout - ViewLineParts`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::inline_decorations::{InlineDecoration, InlineDecorationType};
use je_editor::line_decorations::{DecorationSegment, LineDecoration, LineDecorationsNormalizer};
use je_editor::range::Range;

fn normalize(line_content: &str, line_decorations: &[LineDecoration]) -> Vec<DecorationSegment> {
    let content = JsString::from(line_content);
    LineDecorationsNormalizer::normalize(content.units(), line_decorations)
}

const LINE: &str = "abcabcabcabcabcabcabcabcabcabc";

#[test]
fn bug_9827_overlapping_inline_decorations_can_cause_wrong_inline_class_to_be_applied() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = normalize(
        LINE,
        &[
            LineDecoration::new(1, 11, "c1", InlineDecorationType::Regular),
            LineDecoration::new(3, 4, "c2", InlineDecorationType::Regular),
        ],
    );

    assert_eq!(
        result,
        vec![
            DecorationSegment::new(0, 1, "c1", 0),
            DecorationSegment::new(2, 2, "c2 c1", 0),
            DecorationSegment::new(3, 9, "c1", 0),
        ]
    );
}

#[test]
fn issue_3462_no_whitespace_shown_at_the_end_of_a_decorated_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = normalize(
        LINE,
        &[
            LineDecoration::new(15, 21, "mtkw", InlineDecorationType::Regular),
            LineDecoration::new(20, 21, "inline-folded", InlineDecorationType::Regular),
        ],
    );

    assert_eq!(
        result,
        vec![
            DecorationSegment::new(14, 18, "mtkw", 0),
            DecorationSegment::new(19, 19, "mtkw inline-folded", 0),
        ]
    );
}

#[test]
fn issue_3661_link_decoration_bleeds_to_next_line_when_wrapping() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = LineDecoration::filter(
        &[InlineDecoration::new(
            Range::new(2, 12, 3, 30),
            "detected-link",
            InlineDecorationType::Regular,
        )],
        3,
        12,
        500,
    );

    assert_eq!(
        result,
        vec![LineDecoration::new(12, 30, "detected-link", InlineDecorationType::Regular)]
    );
}

#[test]
fn issue_37401_allow_both_before_and_after_decorations_on_empty_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = LineDecoration::filter(
        &[
            InlineDecoration::new(Range::new(4, 1, 4, 2), "before", InlineDecorationType::Before),
            InlineDecoration::new(Range::new(4, 0, 4, 1), "after", InlineDecorationType::After),
        ],
        4,
        1,
        500,
    );

    assert_eq!(
        result,
        vec![
            LineDecoration::new(1, 2, "before", InlineDecorationType::Before),
            LineDecoration::new(0, 1, "after", InlineDecorationType::After),
        ]
    );
}

#[test]
fn empty_width_only_decorations_are_kept_empty_regular_ones_are_not() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = LineDecoration::filter(
        &[
            InlineDecoration::new(Range::new(4, 3, 4, 3), "spacer", InlineDecorationType::WidthOnly),
            InlineDecoration::new(Range::new(4, 3, 4, 3), "regular", InlineDecorationType::Regular),
        ],
        4,
        1,
        500,
    );

    assert_eq!(
        result,
        vec![LineDecoration::new(3, 3, "spacer", InlineDecorationType::WidthOnly)]
    );
}

#[test]
fn width_only_decorations_come_before_other_decorations_at_the_same_position() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut decorations = vec![
        LineDecoration::new(3, 3, "regular", InlineDecorationType::Regular),
        LineDecoration::new(3, 3, "after", InlineDecorationType::After),
        LineDecoration::new(3, 3, "spacer", InlineDecorationType::WidthOnly),
        LineDecoration::new(3, 3, "before", InlineDecorationType::Before),
    ];
    decorations.sort_by(LineDecoration::compare);

    let class_names: Vec<JsString> = decorations.iter().map(|d| d.class_name.clone()).collect();
    assert_eq!(class_names, vec!["spacer", "before", "after", "regular"]);
}

#[test]
fn width_only_decorations_are_not_turned_into_pseudo_elements() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = normalize(
        "abc",
        &[LineDecoration::new(2, 2, "spacer", InlineDecorationType::WidthOnly)],
    );

    // Metadata stays `0`: unlike a before or after decoration, this is the injected content itself.
    assert_eq!(result, vec![DecorationSegment::new(1, 0, "spacer", 0)]);
}

#[test]
fn view_line_parts() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let regular = InlineDecorationType::Regular;

    assert_eq!(
        normalize(
            LINE,
            &[LineDecoration::new(1, 2, "c1", regular), LineDecoration::new(3, 4, "c2", regular)]
        ),
        vec![DecorationSegment::new(0, 0, "c1", 0), DecorationSegment::new(2, 2, "c2", 0)]
    );

    assert_eq!(
        normalize(
            LINE,
            &[LineDecoration::new(1, 3, "c1", regular), LineDecoration::new(3, 4, "c2", regular)]
        ),
        vec![DecorationSegment::new(0, 1, "c1", 0), DecorationSegment::new(2, 2, "c2", 0)]
    );

    assert_eq!(
        normalize(
            LINE,
            &[LineDecoration::new(1, 4, "c1", regular), LineDecoration::new(3, 4, "c2", regular)]
        ),
        vec![DecorationSegment::new(0, 1, "c1", 0), DecorationSegment::new(2, 2, "c1 c2", 0)]
    );

    assert_eq!(
        normalize(
            LINE,
            &[
                LineDecoration::new(1, 4, "c1", regular),
                LineDecoration::new(1, 4, "c1*", regular),
                LineDecoration::new(3, 4, "c2", regular),
            ]
        ),
        vec![
            DecorationSegment::new(0, 1, "c1 c1*", 0),
            DecorationSegment::new(2, 2, "c1 c1* c2", 0),
        ]
    );

    assert_eq!(
        normalize(
            LINE,
            &[
                LineDecoration::new(1, 4, "c1", regular),
                LineDecoration::new(1, 4, "c1*", regular),
                LineDecoration::new(1, 4, "c1**", regular),
                LineDecoration::new(3, 4, "c2", regular),
            ]
        ),
        vec![
            DecorationSegment::new(0, 1, "c1 c1* c1**", 0),
            DecorationSegment::new(2, 2, "c1 c1* c1** c2", 0),
        ]
    );

    assert_eq!(
        normalize(
            LINE,
            &[
                LineDecoration::new(1, 4, "c1", regular),
                LineDecoration::new(1, 4, "c1*", regular),
                LineDecoration::new(1, 4, "c1**", regular),
                LineDecoration::new(3, 4, "c2", regular),
                LineDecoration::new(3, 4, "c2*", regular),
            ]
        ),
        vec![
            DecorationSegment::new(0, 1, "c1 c1* c1**", 0),
            DecorationSegment::new(2, 2, "c1 c1* c1** c2 c2*", 0),
        ]
    );

    assert_eq!(
        normalize(
            LINE,
            &[
                LineDecoration::new(1, 4, "c1", regular),
                LineDecoration::new(1, 4, "c1*", regular),
                LineDecoration::new(1, 4, "c1**", regular),
                LineDecoration::new(3, 4, "c2", regular),
                LineDecoration::new(3, 5, "c2*", regular),
            ]
        ),
        vec![
            DecorationSegment::new(0, 1, "c1 c1* c1**", 0),
            DecorationSegment::new(2, 2, "c1 c1* c1** c2 c2*", 0),
            DecorationSegment::new(3, 3, "c2*", 0),
        ]
    );
}
