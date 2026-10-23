//! Porte de `src/vs/editor/contrib/folding/test/browser/foldingRanges.test.ts` (suíte `FoldingRanges`), caso a caso e na mesma ordem.
//!
//! `foldRange(from, to, collapsed = undefined, source = provider, type = undefined)` do original tem
//! parâmetros com valor padrão; aqui todos são passados explicitamente (`collapsed || false` vira `bool`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::folding::folding_ranges::{FoldRange, FoldSource, FoldingRegions, MAX_FOLDING_REGIONS};
use je_editor::folding::indent_range_provider::{RangesCollector, compute_ranges};
use je_editor::folding::FoldingLimitReporter;
use je_editor::language_configuration::FoldingMarkers;
use support::re;
use support::test_text_model::create_text_model;

fn markers() -> FoldingMarkers {
    FoldingMarkers { start: re(r"^#region$", ""), end: re(r"^#endregion$", "") }
}

/// `{ limit: MAX_FOLDING_REGIONS, update: () => { } }`.
struct MaxRegionsLimit;

impl FoldingLimitReporter for MaxRegionsLimit {
    fn limit(&self) -> usize {
        MAX_FOLDING_REGIONS
    }

    fn update(&self, _computed: usize, _limited: Option<usize>) {}
}

fn fold_range(from: u32, to: u32, collapsed: bool, source: FoldSource, r#type: Option<&str>) -> FoldRange {
    FoldRange {
        start_line_number: from,
        end_line_number: to,
        r#type: r#type.map(str::to_string),
        is_collapsed: collapsed,
        source,
    }
}

fn assert_equal_ranges(range1: &FoldRange, range2: &FoldRange, msg: &str) {
    assert_eq!(range1.start_line_number, range2.start_line_number, "{msg} start");
    assert_eq!(range1.end_line_number, range2.end_line_number, "{msg} end");
    assert_eq!(range1.r#type, range2.r#type, "{msg} type");
    assert_eq!(range1.is_collapsed, range2.is_collapsed, "{msg} collapsed");
    assert_eq!(range1.source, range2.source, "{msg} source");
}

#[test]
fn test_max_folding_regions() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut lines: Vec<&str> = Vec::new();
    let n_regions = MAX_FOLDING_REGIONS;
    let limit = MaxRegionsLimit;
    let mut collector = RangesCollector::new(&limit);
    for _ in 0..n_regions {
        let start_line_number = lines.len();
        lines.push("#region");
        let end_line_number = lines.len();
        lines.push("#endregion");
        collector.insert_first(start_line_number as u32, end_line_number as u32, 0);
    }
    let model = create_text_model(&lines.join("\n"));
    let actual = collector.to_indent_ranges(&model);
    assert_eq!(actual.length(), n_regions, "len");
    model.dispose();
}

#[test]
fn find_range() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "#region",
        /* 2*/ "#endregion",
        /* 3*/ "class A {",
        /* 4*/ "  void foo() {",
        /* 5*/ "    if (true) {",
        /* 6*/ "        return;",
        /* 7*/ "    }",
        /* 8*/ "",
        /* 9*/ "    if (true) {",
        /*10*/ "      return;",
        /*11*/ "    }",
        /*12*/ "  }",
        /*13*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let actual = compute_ranges(&text_model, false, Some(&markers()), None);
    // let r0 = r(1, 2);
    // let r1 = r(3, 12);
    // let r2 = r(4, 11);
    // let r3 = r(5, 6);
    // let r4 = r(9, 10);

    assert_eq!(actual.find_range(1), 0, "1");
    assert_eq!(actual.find_range(2), 0, "2");
    assert_eq!(actual.find_range(3), 1, "3");
    assert_eq!(actual.find_range(4), 2, "4");
    assert_eq!(actual.find_range(5), 3, "5");
    assert_eq!(actual.find_range(6), 3, "6");
    assert_eq!(actual.find_range(7), 2, "7");
    assert_eq!(actual.find_range(8), 2, "8");
    assert_eq!(actual.find_range(9), 4, "9");
    assert_eq!(actual.find_range(10), 4, "10");
    assert_eq!(actual.find_range(11), 2, "11");
    assert_eq!(actual.find_range(12), 1, "12");
    assert_eq!(actual.find_range(13), -1, "13");
    text_model.dispose();
}

#[test]
fn set_collapsed() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut lines: Vec<&str> = Vec::new();
    let n_regions = 500;
    for _ in 0..n_regions {
        lines.push("#region");
    }
    for _ in 0..n_regions {
        lines.push("#endregion");
    }
    let model = create_text_model(&lines.join("\n"));
    let actual = compute_ranges(&model, false, Some(&markers()), None);
    assert_eq!(actual.length(), n_regions, "len");
    for i in 0..n_regions {
        actual.set_collapsed(i, i % 3 == 0);
    }
    for i in 0..n_regions {
        assert_eq!(actual.is_collapsed(i), i % 3 == 0, "line{i}");
    }
    model.dispose();
}

#[test]
fn sanitize_and_merge_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    use FoldSource::Provider;
    let region_set1 = vec![
        fold_range(0, 100, false, Provider, None),                // invalid, should be removed
        fold_range(1, 100, false, Provider, Some("A")),           // valid
        fold_range(1, 100, false, Provider, Some("Z")),           // invalid, duplicate start
        fold_range(10, 10, false, Provider, None),                // invalid, should be removed
        fold_range(20, 80, false, Provider, Some("C1")),          // valid inside 'B'
        fold_range(22, 80, true, Provider, Some("D1")),           // valid inside 'C1'
        fold_range(90, 101, false, Provider, None),               // invalid, should be removed
    ];
    let region_set2 = vec![
        fold_range(20, 80, true, Provider, None),                 // should merge with C1
        fold_range(18, 80, true, Provider, None),                 // invalid, out of order
        fold_range(21, 81, true, Provider, Some("Z")),            // invalid, overlapping
        fold_range(22, 80, true, Provider, Some("D2")),           // should merge with D1
    ];
    let result = FoldingRegions::sanitize_and_merge((&region_set1).into(), (&region_set2).into(), Some(100), None);
    assert_eq!(result.len(), 3, "result length1");
    assert_equal_ranges(&result[0], &fold_range(1, 100, false, Provider, Some("A")), "A1");
    assert_equal_ranges(&result[1], &fold_range(20, 80, true, Provider, Some("C1")), "C1");
    assert_equal_ranges(&result[2], &fold_range(22, 80, true, Provider, Some("D1")), "D1");
}

#[test]
fn sanitize_and_merge_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    use FoldSource::{Provider, Recovered, UserDefined};
    let region_set1 = vec![
        fold_range(1, 100, false, Provider, Some("a1")),          // valid
        fold_range(2, 100, false, Provider, Some("a2")),          // valid
        fold_range(3, 19, false, Provider, Some("a3")),           // valid
        fold_range(20, 71, false, Provider, Some("a4")),          // overlaps b3
        fold_range(21, 29, false, Provider, Some("a5")),          // valid
        fold_range(81, 91, false, Provider, Some("a6")),          // overlaps b4
    ];
    let region_set2 = vec![
        fold_range(30, 39, true, Provider, Some("b1")),           // valid, will be recovered
        fold_range(40, 49, true, UserDefined, Some("b2")),        // valid
        fold_range(50, 100, true, UserDefined, Some("b3")),       // overlaps a4
        fold_range(80, 90, true, UserDefined, Some("b4")),        // overlaps a6
        fold_range(92, 100, true, UserDefined, Some("b5")),       // valid
    ];
    let result = FoldingRegions::sanitize_and_merge((&region_set1).into(), (&region_set2).into(), Some(100), None);
    assert_eq!(result.len(), 9, "result length1");
    assert_equal_ranges(&result[0], &fold_range(1, 100, false, Provider, Some("a1")), "P1");
    assert_equal_ranges(&result[1], &fold_range(2, 100, false, Provider, Some("a2")), "P2");
    assert_equal_ranges(&result[2], &fold_range(3, 19, false, Provider, Some("a3")), "P3");
    assert_equal_ranges(&result[3], &fold_range(21, 29, false, Provider, Some("a5")), "P4");
    assert_equal_ranges(&result[4], &fold_range(30, 39, true, Recovered, Some("b1")), "P5");
    assert_equal_ranges(&result[5], &fold_range(40, 49, true, UserDefined, Some("b2")), "P6");
    assert_equal_ranges(&result[6], &fold_range(50, 100, true, UserDefined, Some("b3")), "P7");
    assert_equal_ranges(&result[7], &fold_range(80, 90, true, UserDefined, Some("b4")), "P8");
    assert_equal_ranges(&result[8], &fold_range(92, 100, true, UserDefined, Some("b5")), "P9");
}

#[test]
fn sanitize_and_merge_3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    use FoldSource::{Provider, Recovered};
    let region_set1 = vec![
        fold_range(1, 100, false, Provider, Some("a1")),          // valid
        fold_range(10, 29, false, Provider, Some("a2")),          // matches manual hidden
        fold_range(35, 39, true, Recovered, Some("a3")),          // valid
    ];
    let region_set2 = vec![
        fold_range(10, 29, true, Recovered, Some("b1")),          // matches a
        fold_range(20, 28, true, Provider, Some("b2")),           // should remain
        fold_range(30, 39, true, Recovered, Some("b3")),          // should remain
    ];
    let result = FoldingRegions::sanitize_and_merge((&region_set1).into(), (&region_set2).into(), Some(100), None);
    assert_eq!(result.len(), 5, "result length3");
    assert_equal_ranges(&result[0], &fold_range(1, 100, false, Provider, Some("a1")), "R1");
    assert_equal_ranges(&result[1], &fold_range(10, 29, true, Provider, Some("a2")), "R2");
    assert_equal_ranges(&result[2], &fold_range(20, 28, true, Recovered, Some("b2")), "R3");
    assert_equal_ranges(&result[3], &fold_range(30, 39, true, Recovered, Some("b3")), "R3");
    assert_equal_ranges(&result[4], &fold_range(35, 39, true, Recovered, Some("a3")), "R4");
}

#[test]
fn sanitize_and_merge_4() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    use FoldSource::{Provider, Recovered};
    let region_set1 = vec![
        fold_range(1, 100, false, Provider, Some("a1")),          // valid
    ];
    let region_set2 = vec![
        fold_range(20, 28, true, Provider, Some("b1")),           // hidden
        fold_range(30, 38, true, Provider, Some("b2")),           // hidden
    ];
    let result = FoldingRegions::sanitize_and_merge((&region_set1).into(), (&region_set2).into(), Some(100), None);
    assert_eq!(result.len(), 3, "result length4");
    assert_equal_ranges(&result[0], &fold_range(1, 100, false, Provider, Some("a1")), "R1");
    assert_equal_ranges(&result[1], &fold_range(20, 28, true, Recovered, Some("b1")), "R2");
    assert_equal_ranges(&result[2], &fold_range(30, 38, true, Recovered, Some("b2")), "R3");
}
