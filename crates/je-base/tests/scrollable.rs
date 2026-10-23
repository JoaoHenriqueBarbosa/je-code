//! Porte de `src/vs/base/test/common/scrollable.test.ts` (suíte `SmoothScrollingOperation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `TestSmoothScrollingOperation` (que herda para expor o `_tick`) é a função `test_operation`, que cria
//!   uma [`SmoothScrollingOperation`] com os mesmos `+10`/`-10` do construtor do original e chama
//!   `tick_at` no lugar de `testTick`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::scrollable::{ISmoothScrollPosition, SmoothScrollingOperation};

const VIEWPORT_HEIGHT: f64 = 800.0;
const ANIMATION_DURATION: f64 = 125.0;
const LINE_HEIGHT: f64 = 20.0;

fn test_operation(from: f64, to: f64, viewport_size: f64, start_time: f64, duration: f64) -> SmoothScrollingOperation {
    let duration = duration + 10.0;
    let start_time = start_time - 10.0;

    SmoothScrollingOperation::new(
        ISmoothScrollPosition { scroll_left: 0.0, scroll_top: from, width: 0.0, height: viewport_size },
        ISmoothScrollPosition { scroll_left: 0.0, scroll_top: to, width: 0.0, height: viewport_size },
        start_time,
        duration,
    )
}

fn extract_lines(scrollable: &SmoothScrollingOperation, now: f64) -> (i64, i64) {
    let scroll_top = scrollable.tick_at(now).scroll_top;
    let scroll_bottom = scroll_top + VIEWPORT_HEIGHT;

    let start_line_number = (scroll_top / LINE_HEIGHT).floor();
    let end_line_number = (scroll_bottom / LINE_HEIGHT).ceil();

    (start_line_number as i64, end_line_number as i64)
}

fn simulate_smooth_scroll(from: f64, to: f64) -> Vec<(i64, i64)> {
    let scrollable = test_operation(from, to, VIEWPORT_HEIGHT, 0.0, ANIMATION_DURATION);

    [0.0, 25.0, 50.0, 75.0, 100.0, 125.0].iter().map(|&now| extract_lines(&scrollable, now)).collect()
}

fn assert_smooth_scroll(from: f64, to: f64, expected: &[(i64, i64)]) {
    let actual = simulate_smooth_scroll(from, to);
    assert_eq!(actual, expected);
}

#[test]
fn scroll_25_lines_40_fit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_smooth_scroll(0.0, 500.0, &[(5, 46), (14, 55), (20, 61), (23, 64), (24, 65), (25, 65)]);
}

#[test]
fn scroll_75_lines_40_fit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_smooth_scroll(0.0, 1500.0, &[(15, 56), (44, 85), (62, 103), (71, 112), (74, 115), (75, 115)]);
}

#[test]
fn scroll_100_lines_40_fit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_smooth_scroll(0.0, 2000.0, &[(20, 61), (59, 100), (82, 123), (94, 135), (99, 140), (100, 140)]);
}

#[test]
fn scroll_125_lines_40_fit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_smooth_scroll(0.0, 2500.0, &[(16, 57), (29, 70), (107, 148), (119, 160), (124, 165), (125, 165)]);
}

#[test]
fn scroll_500_lines_40_fit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_smooth_scroll(0.0, 10000.0, &[(16, 57), (29, 70), (482, 523), (494, 535), (499, 540), (500, 540)]);
}
