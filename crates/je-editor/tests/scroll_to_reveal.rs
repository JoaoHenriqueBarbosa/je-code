//! Porte de `src/vs/editor/contrib/inlineCompletions/test/browser/scrollToReveal.test.ts` (suíte `scrollToReveal`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O resultado `{ newScrollPosition }` vira o `ScrollToReveal` com o campo `new_scroll_position`.
//! - Os números são `i32`, como o `OffsetRange`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::inline_completions::long_distance_hint::scroll_to_reveal;
use je_editor::offset_range::OffsetRange;

#[test]
fn should_not_scroll_when_content_is_already_visible() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content range [20, 30) is fully contained in window [10, 50)
    let result = scroll_to_reveal(10, 40, OffsetRange::new(20, 30));
    assert_eq!(result.new_scroll_position, 10);
}

#[test]
fn should_not_scroll_when_content_exactly_fits_the_visible_window() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content range [10, 50) exactly matches visible window [10, 50)
    let result = scroll_to_reveal(10, 40, OffsetRange::new(10, 50));
    assert_eq!(result.new_scroll_position, 10);
}

#[test]
fn should_scroll_left_when_content_starts_before_visible_window() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content range [5, 15) starts before visible window [20, 60)
    let result = scroll_to_reveal(20, 40, OffsetRange::new(5, 15));
    assert_eq!(result.new_scroll_position, 5);
}

#[test]
fn should_scroll_right_when_content_ends_after_visible_window() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content range [50, 80) ends after visible window [10, 50)
    // New scroll position should be 80 - 40 = 40 so window becomes [40, 80)
    let result = scroll_to_reveal(10, 40, OffsetRange::new(50, 80));
    assert_eq!(result.new_scroll_position, 40);
}

#[test]
fn should_show_start_of_content_when_content_is_larger_than_window() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content range [20, 100) is larger than window width 40
    // Should position at start of content
    let result = scroll_to_reveal(10, 40, OffsetRange::new(20, 100));
    assert_eq!(result.new_scroll_position, 20);
}

#[test]
fn should_handle_edge_case_with_zero_width_content() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Empty content range [25, 25) in window [10, 50)
    let result = scroll_to_reveal(10, 40, OffsetRange::new(25, 25));
    assert_eq!(result.new_scroll_position, 10);
}

#[test]
fn should_handle_edge_case_with_zero_window_width() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Any non-empty content with zero window width should position at content start
    let result = scroll_to_reveal(10, 0, OffsetRange::new(20, 30));
    assert_eq!(result.new_scroll_position, 20);
}

#[test]
fn should_handle_content_at_exact_window_boundaries_left_edge() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content range [10, 20) starts exactly at visible window start [10, 50)
    let result = scroll_to_reveal(10, 40, OffsetRange::new(10, 20));
    assert_eq!(result.new_scroll_position, 10);
}

#[test]
fn should_handle_content_at_exact_window_boundaries_right_edge() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content range [40, 50) ends exactly at visible window end [10, 50)
    let result = scroll_to_reveal(10, 40, OffsetRange::new(40, 50));
    assert_eq!(result.new_scroll_position, 10);
}

#[test]
fn should_scroll_right_when_content_extends_beyond_right_boundary() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content range [40, 60) extends beyond visible window [10, 50)
    // New scroll position should be 60 - 40 = 20 so window becomes [20, 60)
    let result = scroll_to_reveal(10, 40, OffsetRange::new(40, 60));
    assert_eq!(result.new_scroll_position, 20);
}

#[test]
fn should_scroll_left_when_content_extends_beyond_left_boundary() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content range [5, 25) starts before visible window [20, 60)
    // Should position at start of content
    let result = scroll_to_reveal(20, 40, OffsetRange::new(5, 25));
    assert_eq!(result.new_scroll_position, 5);
}

#[test]
fn should_handle_content_overlapping_both_boundaries() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content range [5, 70) overlaps both sides of visible window [20, 60)
    // Since content is larger than window, should position at start of content
    let result = scroll_to_reveal(20, 40, OffsetRange::new(5, 70));
    assert_eq!(result.new_scroll_position, 5);
}

#[test]
fn should_handle_negative_scroll_positions() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Current scroll at -10, window width 40, so visible range [-10, 30)
    // Content [35, 45) is beyond the visible window
    let result = scroll_to_reveal(-10, 40, OffsetRange::new(35, 45));
    assert_eq!(result.new_scroll_position, 5); // 45 - 40 = 5
}

#[test]
fn should_handle_large_numbers() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Test with large numbers to ensure no overflow issues
    let result = scroll_to_reveal(1000000, 500, OffsetRange::new(1000600, 1000700));
    assert_eq!(result.new_scroll_position, 1000200); // 1000700 - 500 = 1000200
}

#[test]
fn should_prioritize_left_scroll_when_content_spans_window_but_starts_before() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content [5, 55) spans wider than window width 40, starting before visible [20, 60)
    // Should position at start of content
    let result = scroll_to_reveal(20, 40, OffsetRange::new(5, 55));
    assert_eq!(result.new_scroll_position, 5);
}

#[test]
fn should_handle_single_character_content_requiring_scroll() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Single character at position [100, 101) with visible window [10, 50)
    let result = scroll_to_reveal(10, 40, OffsetRange::new(100, 101));
    assert_eq!(result.new_scroll_position, 61); // 101 - 40 = 61
}

#[test]
fn should_handle_content_just_barely_outside_visible_area_left() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content [9, 19) with one unit outside visible window [10, 50)
    let result = scroll_to_reveal(10, 40, OffsetRange::new(9, 19));
    assert_eq!(result.new_scroll_position, 9);
}

#[test]
fn should_handle_content_just_barely_outside_visible_area_right() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content [45, 51) with one unit outside visible window [10, 50)
    let result = scroll_to_reveal(10, 40, OffsetRange::new(45, 51));
    assert_eq!(result.new_scroll_position, 11); // 51 - 40 = 11
}

#[test]
fn should_handle_fractional_like_scenarios_with_minimum_window() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Minimum window width 1, content needs to be revealed
    let result = scroll_to_reveal(50, 1, OffsetRange::new(100, 105));
    assert_eq!(result.new_scroll_position, 100); // Content larger than window, show start
}

#[test]
fn should_preserve_scroll_when_content_partially_visible_on_left() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content [5, 25) partially visible in window [20, 60), overlaps [20, 25)
    // Since content starts before window, scroll to show start
    let result = scroll_to_reveal(20, 40, OffsetRange::new(5, 25));
    assert_eq!(result.new_scroll_position, 5);
}

#[test]
fn should_preserve_scroll_when_content_partially_visible_on_right() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Content [45, 65) partially visible in window [20, 60), overlaps [45, 60)
    // Since content extends beyond window, scroll to show end
    let result = scroll_to_reveal(20, 40, OffsetRange::new(45, 65));
    assert_eq!(result.new_scroll_position, 25); // 65 - 40 = 25
}
