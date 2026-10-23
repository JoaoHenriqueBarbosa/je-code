//! Porte de `src/vs/editor/test/common/view/overviewZoneManager.test.ts` (suíte `Editor View - OverviewZoneManager`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::overview_zone_manager::{ColorZone, OverviewRulerZone, OverviewZoneManager};

fn zones() -> Vec<OverviewRulerZone> {
    vec![
        OverviewRulerZone::new(1, 1, 0.0, "1"),
        OverviewRulerZone::new(10, 10, 0.0, "2"),
        OverviewRulerZone::new(30, 31, 0.0, "3"),
        OverviewRulerZone::new(50, 50, 0.0, "4"),
    ]
}

#[test]
fn pixel_ratio_1_dom_height_600() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_count = 50.0;
    let line_height = 20.0;
    let mut manager = OverviewZoneManager::new(move |line_number| line_height * f64::from(line_number));
    manager.set_dom_width(30.0);
    manager.set_dom_height(600.0);
    manager.set_outer_height(line_count * line_height);
    manager.set_line_height(line_height);
    manager.set_pixel_ratio(1.0);

    manager.set_zones(zones());

    // one line = 12, but cap is at 6
    assert_eq!(
        manager.resolve_color_zones(),
        vec![
            ColorZone::new(12, 24, 1),
            ColorZone::new(120, 132, 2),  // 120 -> 132
            ColorZone::new(360, 384, 3),  // 360 -> 372 [360 -> 384]
            ColorZone::new(588, 600, 4),  // 588 -> 600
        ]
    );
}

#[test]
fn pixel_ratio_1_dom_height_300() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_count = 50.0;
    let line_height = 20.0;
    let mut manager = OverviewZoneManager::new(move |line_number| line_height * f64::from(line_number));
    manager.set_dom_width(30.0);
    manager.set_dom_height(300.0);
    manager.set_outer_height(line_count * line_height);
    manager.set_line_height(line_height);
    manager.set_pixel_ratio(1.0);

    manager.set_zones(zones());

    // one line = 6, cap is at 6
    assert_eq!(
        manager.resolve_color_zones(),
        vec![
            ColorZone::new(6, 12, 1),
            ColorZone::new(60, 66, 2),    // 60 -> 66
            ColorZone::new(180, 192, 3),  // 180 -> 192
            ColorZone::new(294, 300, 4),  // 294 -> 300
        ]
    );
}

#[test]
fn pixel_ratio_2_dom_height_300() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_count = 50.0;
    let line_height = 20.0;
    let mut manager = OverviewZoneManager::new(move |line_number| line_height * f64::from(line_number));
    manager.set_dom_width(30.0);
    manager.set_dom_height(300.0);
    manager.set_outer_height(line_count * line_height);
    manager.set_line_height(line_height);
    manager.set_pixel_ratio(2.0);

    manager.set_zones(zones());

    // one line = 6, cap is at 12
    assert_eq!(
        manager.resolve_color_zones(),
        vec![
            ColorZone::new(12, 24, 1),
            ColorZone::new(120, 132, 2),  // 120 -> 132
            ColorZone::new(360, 384, 3),  // 360 -> 384
            ColorZone::new(588, 600, 4),  // 588 -> 600
        ]
    );
}
