//! Porte de `src/vs/editor/contrib/inlineCompletions/test/browser/layout.test.ts` (suíte `Layout - getMaxTowerHeightInAvailableArea`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - Os números do JavaScript em `Size2D` são `f64`; o `OffsetRange` é `i32`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::core_2d::size::Size2D;
use je_editor::inline_completions::towers_layout::get_max_tower_height_in_available_area;
use je_editor::offset_range::OffsetRange;

fn sz(width: f64, height: f64) -> Size2D {
    Size2D::new(width, height)
}

#[test]
fn tower_fits_within_single_available_area() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(5, 15); // width of 10
    let available_tower_areas = [sz(50.0, 30.0)];

    // Should return the available height (30)
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 30.0);
}

#[test]
fn max_height_available_in_area() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(5, 15); // width of 10
    let available_tower_areas = [sz(50.0, 30.0)];

    // Should return the available height (30), even if original tower was 40
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 30.0);
}

#[test]
fn tower_extends_beyond_available_width() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(0, 60); // width of 60
    let available_tower_areas = [sz(50.0, 30.0)];

    // Should return 0 because tower extends beyond available areas
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 0.0);
}

#[test]
fn tower_fits_across_multiple_available_areas() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(10, 40); // width of 30
    let available_tower_areas = [sz(20.0, 30.0), sz(20.0, 25.0), sz(20.0, 30.0)];

    // Should return the minimum height across overlapping areas (25)
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 25.0);
}

#[test]
fn min_height_across_overlapping_areas() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(10, 40); // width of 30
    let available_tower_areas = [
        sz(20.0, 30.0),
        sz(20.0, 15.0), // Shortest area
        sz(20.0, 30.0),
    ];

    // Should return the minimum height (15)
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 15.0);
}

#[test]
fn tower_at_left_edge_of_available_areas() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(0, 10); // width of 10
    let available_tower_areas = [sz(50.0, 30.0)];

    // Should return the available height (30)
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 30.0);
}

#[test]
fn tower_at_right_edge_of_available_areas() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(40, 50); // width of 10
    let available_tower_areas = [sz(50.0, 30.0)];

    // Should return the available height (30)
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 30.0);
}

#[test]
fn tower_exactly_matches_available_area() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(0, 50); // width of 50
    let available_tower_areas = [sz(50.0, 30.0)];

    // Should return the available height (30)
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 30.0);
}

#[test]
fn empty_available_areas() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(0, 10); // width of 10
    let available_tower_areas: [Size2D; 0] = [];

    // Should return 0 for empty areas
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 0.0);
}

#[test]
fn tower_spans_exactly_two_available_areas() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(10, 50); // width of 40
    let available_tower_areas = [sz(30.0, 25.0), sz(30.0, 25.0)];

    // Should return the minimum height across both areas (25)
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 25.0);
}

#[test]
fn tower_starts_at_boundary_between_two_areas() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(30, 50); // width of 20
    let available_tower_areas = [sz(30.0, 25.0), sz(30.0, 25.0)];

    // Should return the height of the second area (25)
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 25.0);
}

#[test]
fn tower_with_varying_height_available_areas() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(0, 50); // width of 50
    let available_tower_areas = [
        sz(10.0, 30.0),
        sz(10.0, 15.0), // Shortest area
        sz(10.0, 25.0),
        sz(10.0, 30.0),
        sz(10.0, 40.0),
    ];

    // Should return the minimum height (15)
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 15.0);
}

#[test]
fn tower_beyond_all_available_areas_to_the_right() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tower_horizontal_range = OffsetRange::new(100, 110); // width of 10
    let available_tower_areas = [sz(50.0, 30.0)];

    // Should return 0 because tower is beyond available areas
    assert_eq!(get_max_tower_height_in_available_area(tower_horizontal_range, &available_tower_areas), 0.0);
}
