//! Porte de `src/vs/editor/contrib/folding/test/browser/hiddenRangeModel.test.ts` (suíte `Hidden Range Model`), caso a caso e na mesma ordem.
//!
//! `TestDecorationProvider`, que o original importa do `foldingModel.test.ts`, está em
//! `support/test_folding_decoration_provider.rs`. `try/finally` não existe: os `dispose` rodam no fim do caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_editor::folding::folding_model::FoldingModel;
use je_editor::folding::hidden_range_model::HiddenRangeModel;
use je_editor::folding::indent_range_provider::compute_ranges;
use je_editor::range::Range;
use support::test_folding_decoration_provider::TestDecorationProvider;
use support::test_text_model::create_text_model;

#[derive(Debug, PartialEq, Eq)]
struct ExpectedRange {
    start_line_number: i32,
    end_line_number: i32,
}

fn r(start_line_number: i32, end_line_number: i32) -> ExpectedRange {
    ExpectedRange { start_line_number, end_line_number }
}

fn assert_ranges(actual: &[Range], expected_regions: &[ExpectedRange], message: &str) {
    let actual: Vec<ExpectedRange> = actual.iter().map(|r| ExpectedRange {
        start_line_number: r.start_line_number,
        end_line_number: r.end_line_number,
    }).collect();
    assert_eq!(actual, expected_regions, "{message}");
}

#[test]
fn has_ranges() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "/**",
        /* 2*/ " * Comment",
        /* 3*/ " */",
        /* 4*/ "class A {",
        /* 5*/ "  void foo() {",
        /* 6*/ "    if (true) {",
        /* 7*/ "      //hello",
        /* 8*/ "    }",
        /* 9*/ "  }",
        /*10*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let folding_model = FoldingModel::new(text_model.clone(), TestDecorationProvider::new(text_model.clone()));
    let hidden_range_model = HiddenRangeModel::new(folding_model.clone());

    assert!(!hidden_range_model.has_ranges());

    let ranges = compute_ranges(&text_model, false, None, None);
    folding_model.update(&ranges, None);

    folding_model.toggle_collapse_state(vec![
        folding_model.get_region_at_line(1).unwrap(),
        folding_model.get_region_at_line(6).unwrap(),
    ]);
    assert_ranges(&hidden_range_model.hidden_ranges(), &[r(2, 3), r(7, 7)], "");

    assert!(hidden_range_model.has_ranges());
    assert!(!hidden_range_model.is_hidden(1));
    assert!(hidden_range_model.is_hidden(2));
    assert!(hidden_range_model.is_hidden(3));
    assert!(!hidden_range_model.is_hidden(4));
    assert!(!hidden_range_model.is_hidden(5));
    assert!(!hidden_range_model.is_hidden(6));
    assert!(hidden_range_model.is_hidden(7));
    assert!(!hidden_range_model.is_hidden(8));
    assert!(!hidden_range_model.is_hidden(9));
    assert!(!hidden_range_model.is_hidden(10));

    folding_model.toggle_collapse_state(vec![folding_model.get_region_at_line(4).unwrap()]);
    assert_ranges(&hidden_range_model.hidden_ranges(), &[r(2, 3), r(5, 9)], "");

    assert!(hidden_range_model.has_ranges());
    assert!(!hidden_range_model.is_hidden(1));
    assert!(hidden_range_model.is_hidden(2));
    assert!(hidden_range_model.is_hidden(3));
    assert!(!hidden_range_model.is_hidden(4));
    assert!(hidden_range_model.is_hidden(5));
    assert!(hidden_range_model.is_hidden(6));
    assert!(hidden_range_model.is_hidden(7));
    assert!(hidden_range_model.is_hidden(8));
    assert!(hidden_range_model.is_hidden(9));
    assert!(!hidden_range_model.is_hidden(10));

    folding_model.toggle_collapse_state(vec![
        folding_model.get_region_at_line(1).unwrap(),
        folding_model.get_region_at_line(6).unwrap(),
        folding_model.get_region_at_line(4).unwrap(),
    ]);
    assert_ranges(&hidden_range_model.hidden_ranges(), &[], "");
    assert!(!hidden_range_model.has_ranges());
    assert!(!hidden_range_model.is_hidden(1));
    assert!(!hidden_range_model.is_hidden(2));
    assert!(!hidden_range_model.is_hidden(3));
    assert!(!hidden_range_model.is_hidden(4));
    assert!(!hidden_range_model.is_hidden(5));
    assert!(!hidden_range_model.is_hidden(6));
    assert!(!hidden_range_model.is_hidden(7));
    assert!(!hidden_range_model.is_hidden(8));
    assert!(!hidden_range_model.is_hidden(9));
    assert!(!hidden_range_model.is_hidden(10));

    text_model.dispose();
    hidden_range_model.dispose();
}
