//! Porte de `src/vs/base/test/browser/ui/scrollbar/scrollbarState.test.ts` (suíte `ScrollbarState`), caso a caso e na mesma ordem.

mod support;

use je_widgets::scrollbar::scrollbar_state::ScrollbarState;
use support::ensure_no_disposables_are_leaked_in_test_suite;

#[test]
fn inflates_slider_size() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut actual = ScrollbarState::new(0.0, 14.0, 0.0, 339.0, 42423.0, 32787.0);

    assert_eq!(actual.get_arrow_size(), 0.0);
    assert_eq!(actual.get_scroll_position(), 32787.0);
    assert_eq!(actual.get_rectangle_large_size(), 339.0);
    assert_eq!(actual.get_rectangle_small_size(), 14.0);
    assert!(actual.is_needed());
    assert_eq!(actual.get_slider_size(), 20.0);
    assert_eq!(actual.get_slider_position(), 249.0);

    assert_eq!(actual.get_desired_scroll_position_from_offset(259.0), 32849.0);

    // 259 is greater than 230 so page down, 32787 + 339 =  33126
    assert_eq!(actual.get_desired_scroll_position_from_offset_paged(259.0), 33126.0);

    actual.set_scroll_position(32849.0);
    assert_eq!(actual.get_arrow_size(), 0.0);
    assert_eq!(actual.get_scroll_position(), 32849.0);
    assert_eq!(actual.get_rectangle_large_size(), 339.0);
    assert_eq!(actual.get_rectangle_small_size(), 14.0);
    assert!(actual.is_needed());
    assert_eq!(actual.get_slider_size(), 20.0);
    assert_eq!(actual.get_slider_position(), 249.0);
}

#[test]
fn inflates_slider_size_with_arrows() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut actual = ScrollbarState::new(12.0, 14.0, 0.0, 339.0, 42423.0, 32787.0);

    assert_eq!(actual.get_arrow_size(), 12.0);
    assert_eq!(actual.get_scroll_position(), 32787.0);
    assert_eq!(actual.get_rectangle_large_size(), 339.0);
    assert_eq!(actual.get_rectangle_small_size(), 14.0);
    assert!(actual.is_needed());
    assert_eq!(actual.get_slider_size(), 20.0);
    assert_eq!(actual.get_slider_position(), 230.0);

    assert_eq!(actual.get_desired_scroll_position_from_offset(240.0 + 12.0), 32811.0);

    // 240 + 12 = 252; greater than 230 so page down, 32787 + 339 =  33126
    assert_eq!(actual.get_desired_scroll_position_from_offset_paged(240.0 + 12.0), 33126.0);

    actual.set_scroll_position(32811.0);
    assert_eq!(actual.get_arrow_size(), 12.0);
    assert_eq!(actual.get_scroll_position(), 32811.0);
    assert_eq!(actual.get_rectangle_large_size(), 339.0);
    assert_eq!(actual.get_rectangle_small_size(), 14.0);
    assert!(actual.is_needed());
    assert_eq!(actual.get_slider_size(), 20.0);
    assert_eq!(actual.get_slider_position(), 230.0);
}
