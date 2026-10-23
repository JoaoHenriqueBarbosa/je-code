//! Porte de `src/vs/editor/contrib/folding/test/browser/foldingModel.test.ts` (suíte `Folding Model`), caso a caso e na mesma ordem.
//!
//! Diferenças de forma em relação ao original:
//! - `TestDecorationProvider` está em `support/test_folding_decoration_provider.rs`, porque o
//!   `hiddenRangeModel.test.ts` o importa deste arquivo e cada teste do Rust é um programa separado.
//! - `assertDecorations` faz `foldingModel.decorationProvider as TestDecorationProvider`; aqui o teste
//!   guarda o `Rc<TestDecorationProvider>` ao lado do modelo e lê as decorações dele.
//! - `r(startLineNumber, endLineNumber, isCollapsed = false)` e `setCollapseStateLevelsDown(model, doCollapse,
//!   levels = Number.MAX_VALUE, lineNumbers?)` têm valores padrão; aqui todos os argumentos são explícitos
//!   (`Number.MAX_VALUE` é `usize::MAX`).
//! - `new RegExp('^\\s*' + escapeRegExpCharacters('/*'))` é `^\s*/\*` (o escape só acrescenta a barra
//!   invertida antes do `*`).
//! - Os literais `{ startLineNumber, ..., source }` de `FoldRange` dos casos `removeManualRanges` passam por
//!   `fold_range`, para não repetir os cinco campos.
//! - `try/finally` não existe: o `dispose` do modelo de texto roda no fim do caso, e um caso que reprova
//!   não o chama (o guarda de vazamento não confere quando o teste já está em pânico).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::edit_operation::EditOperation;
use je_editor::folding::folding_model::{self as folding, FoldingModel, RegionFilter};
use je_editor::folding::folding_ranges::{FoldRange, FoldSource, FoldingRegion, FoldingRegions};
use je_editor::folding::indent_range_provider::compute_ranges;
use je_editor::folding::to_selected_lines;
use je_editor::language_configuration::FoldingMarkers;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;
use support::re;
use support::test_folding_decoration_provider::{ExpectedDecoration, ExpectedDecorationType, TestDecorationProvider};
use support::test_text_model::create_text_model;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExpectedRegion {
    start_line_number: u32,
    end_line_number: u32,
    is_collapsed: bool,
}

fn r(start_line_number: u32, end_line_number: u32, is_collapsed: bool) -> ExpectedRegion {
    ExpectedRegion { start_line_number, end_line_number, is_collapsed }
}

fn d(line: i32, r#type: ExpectedDecorationType) -> ExpectedDecoration {
    ExpectedDecoration { line, r#type }
}

fn region_markers() -> FoldingMarkers {
    FoldingMarkers { start: re(r"^//#region$", ""), end: re(r"^//#endregion$", "") }
}

fn indented_region_markers() -> FoldingMarkers {
    FoldingMarkers { start: re(r"^\s*//#region$", ""), end: re(r"^\s*//#endregion$", "") }
}

fn fold_range(start_line_number: u32, end_line_number: u32, source: FoldSource) -> FoldRange {
    FoldRange { start_line_number, end_line_number, r#type: None, is_collapsed: false, source }
}

fn reversed(regions: &[ExpectedRegion]) -> Vec<ExpectedRegion> {
    regions.iter().rev().copied().collect()
}

/// `(r, level) => level === expected`.
fn level_is(expected: usize) -> impl Fn(&FoldingRegion, usize) -> bool {
    move |_, level| level == expected
}

fn new_folding_model(text_model: &Rc<TextModel>) -> (Rc<FoldingModel>, Rc<TestDecorationProvider>) {
    let provider = TestDecorationProvider::new(text_model.clone());
    (FoldingModel::new(text_model.clone(), provider.clone()), provider)
}

fn assert_region(actual: Option<FoldingRegion>, expected: Option<ExpectedRegion>, message: &str) {
    assert_eq!(actual.is_some(), expected.is_some(), "{message}");
    if let (Some(actual), Some(expected)) = (actual, expected) {
        assert_eq!(actual.start_line_number(), expected.start_line_number, "{message}");
        assert_eq!(actual.end_line_number(), expected.end_line_number, "{message}");
        assert_eq!(actual.is_collapsed(), expected.is_collapsed, "{message}");
    }
}

fn assert_folded_ranges(folding_model: &FoldingModel, expected_regions: &[ExpectedRegion], message: &str) {
    let mut actual_ranges: Vec<ExpectedRegion> = Vec::new();
    let actual = folding_model.regions();
    for i in 0..actual.length() {
        if actual.is_collapsed(i) {
            actual_ranges.push(r(actual.get_start_line_number(i), actual.get_end_line_number(i), false));
        }
    }
    assert_eq!(actual_ranges, expected_regions, "{message}");
}

fn assert_ranges(folding_model: &FoldingModel, expected_regions: &[ExpectedRegion], message: &str) {
    let mut actual_ranges: Vec<ExpectedRegion> = Vec::new();
    let actual = folding_model.regions();
    for i in 0..actual.length() {
        actual_ranges.push(r(actual.get_start_line_number(i), actual.get_end_line_number(i), actual.is_collapsed(i)));
    }
    assert_eq!(actual_ranges, expected_regions, "{message}");
}

fn assert_decorations(provider: &TestDecorationProvider, expected_decoration: &[ExpectedDecoration], message: &str) {
    assert_eq!(provider.get_decorations(), expected_decoration, "{message}");
}

fn assert_regions(actual: &[FoldingRegion], expected_regions: &[ExpectedRegion], message: &str) {
    let actual: Vec<ExpectedRegion> = actual.iter().map(|r| ExpectedRegion {
        start_line_number: r.start_line_number(),
        end_line_number: r.end_line_number(),
        is_collapsed: r.is_collapsed(),
    }).collect();
    assert_eq!(actual, expected_regions, "{message}");
}

#[test]
fn get_region_at_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "/**",
        /* 2*/ " * Comment",
        /* 3*/ " */",
        /* 4*/ "class A {",
        /* 5*/ "  void foo() {",
        /* 6*/ "    // comment {",
        /* 7*/ "  }",
        /* 8*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, None, None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 3, false);
    let r2 = r(4, 7, false);
    let r3 = r(5, 6, false);

    assert_ranges(&folding_model, &[r1, r2, r3], "");

    assert_region(folding_model.get_region_at_line(1), Some(r1), "1");
    assert_region(folding_model.get_region_at_line(2), Some(r1), "2");
    assert_region(folding_model.get_region_at_line(3), Some(r1), "3");
    assert_region(folding_model.get_region_at_line(4), Some(r2), "4");
    assert_region(folding_model.get_region_at_line(5), Some(r3), "5");
    assert_region(folding_model.get_region_at_line(6), Some(r3), "5");
    assert_region(folding_model.get_region_at_line(7), Some(r2), "6");
    assert_region(folding_model.get_region_at_line(8), None, "7");

    text_model.dispose();
}

#[test]
fn collapse() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "/**",
        /* 2*/ " * Comment",
        /* 3*/ " */",
        /* 4*/ "class A {",
        /* 5*/ "  void foo() {",
        /* 6*/ "    // comment {",
        /* 7*/ "  }",
        /* 8*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, None, None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 3, false);
    let r2 = r(4, 7, false);
    let r3 = r(5, 6, false);

    assert_ranges(&folding_model, &[r1, r2, r3], "");

    folding_model.toggle_collapse_state(vec![folding_model.get_region_at_line(1).unwrap()]);
    folding_model.update(&ranges, None);

    assert_ranges(&folding_model, &[r(1, 3, true), r2, r3], "");

    folding_model.toggle_collapse_state(vec![folding_model.get_region_at_line(5).unwrap()]);
    folding_model.update(&ranges, None);

    assert_ranges(&folding_model, &[r(1, 3, true), r2, r(5, 6, true)], "");

    folding_model.toggle_collapse_state(vec![folding_model.get_region_at_line(7).unwrap()]);
    folding_model.update(&ranges, None);

    assert_ranges(&folding_model, &[r(1, 3, true), r(4, 7, true), r(5, 6, true)], "");

    text_model.dispose();
    text_model.dispose();
}

#[test]
fn update() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "/**",
        /* 2*/ " * Comment",
        /* 3*/ " */",
        /* 4*/ "class A {",
        /* 5*/ "  void foo() {",
        /* 6*/ "    // comment {",
        /* 7*/ "  }",
        /* 8*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, None, None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 3, false);
    let r2 = r(4, 7, false);
    let r3 = r(5, 6, false);

    assert_ranges(&folding_model, &[r1, r2, r3], "");
    folding_model.toggle_collapse_state(vec![
        folding_model.get_region_at_line(2).unwrap(),
        folding_model.get_region_at_line(5).unwrap(),
    ]);

    text_model.apply_edits(&[EditOperation::insert(Position::new(4, 1), "//hello\n")]);

    folding_model.update(&compute_ranges(&text_model, false, None, None), None);

    assert_ranges(&folding_model, &[r(1, 3, true), r(5, 8, false), r(6, 7, true)], "");

    text_model.dispose();
}

#[test]
fn delete() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "function foo() {",
        /* 2*/ "  switch (x) {",
        /* 3*/ "    case 1:",
        /* 4*/ "      //hello1",
        /* 5*/ "      break;",
        /* 6*/ "    case 2:",
        /* 7*/ "      //hello2",
        /* 8*/ "      break;",
        /* 9*/ "    case 3:",
        /*10*/ "      //hello3",
        /*11*/ "      break;",
        /*12*/ "  }",
        /*13*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, None, None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 12, false);
    let r2 = r(2, 11, false);
    let r3 = r(3, 5, false);
    let r4 = r(6, 8, false);
    let r5 = r(9, 11, false);

    assert_ranges(&folding_model, &[r1, r2, r3, r4, r5], "");
    folding_model.toggle_collapse_state(vec![folding_model.get_region_at_line(6).unwrap()]);

    text_model.apply_edits(&[EditOperation::delete(Range::new(6, 11, 9, 0))]);

    folding_model.update(
        &compute_ranges(&text_model, true, None, None),
        Some(&to_selected_lines(Some(&[Selection::new(7, 1, 7, 1)]))),
    );

    assert_ranges(&folding_model, &[r(1, 9, false), r(2, 8, false), r(3, 5, false), r(6, 8, false)], "");

    text_model.dispose();
}

#[test]
fn get_regions_inside() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "/**",
        /* 2*/ " * Comment",
        /* 3*/ " */",
        /* 4*/ "class A {",
        /* 5*/ "  void foo() {",
        /* 6*/ "    // comment {",
        /* 7*/ "  }",
        /* 8*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, None, None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 3, false);
    let r2 = r(4, 7, false);
    let r3 = r(5, 6, false);

    assert_ranges(&folding_model, &[r1, r2, r3], "");
    let region1 = folding_model.get_region_at_line(r1.start_line_number);
    let region2 = folding_model.get_region_at_line(r2.start_line_number);
    let region3 = folding_model.get_region_at_line(r3.start_line_number);

    assert_regions(&folding_model.get_regions_inside(None, None), &[r1, r2, r3], "1");
    assert_regions(&folding_model.get_regions_inside(region1.as_ref(), None), &[], "2");
    assert_regions(&folding_model.get_regions_inside(region2.as_ref(), None), &[r3], "3");
    assert_regions(&folding_model.get_regions_inside(region3.as_ref(), None), &[], "4");

    text_model.dispose();
}

#[test]
fn get_regions_inside_with_level() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "//#region",
        /* 2*/ "//#endregion",
        /* 3*/ "class A {",
        /* 4*/ "  void foo() {",
        /* 5*/ "    if (true) {",
        /* 6*/ "        return;",
        /* 7*/ "    }",
        /* 8*/ "    if (true) {",
        /* 9*/ "      return;",
        /*10*/ "    }",
        /*11*/ "  }",
        /*12*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, Some(&region_markers()), None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 2, false);
    let r2 = r(3, 11, false);
    let r3 = r(4, 10, false);
    let r4 = r(5, 6, false);
    let r5 = r(8, 9, false);

    let region1 = folding_model.get_region_at_line(r1.start_line_number);
    let region2 = folding_model.get_region_at_line(r2.start_line_number);
    let region3 = folding_model.get_region_at_line(r3.start_line_number);

    assert_ranges(&folding_model, &[r1, r2, r3, r4, r5], "");

    let (level1, level2, level3) = (level_is(1), level_is(2), level_is(3));
    assert_regions(&folding_model.get_regions_inside(None, Some(RegionFilter::WithLevel(&level1))), &[r1, r2], "1");
    assert_regions(&folding_model.get_regions_inside(None, Some(RegionFilter::WithLevel(&level2))), &[r3], "2");
    assert_regions(&folding_model.get_regions_inside(None, Some(RegionFilter::WithLevel(&level3))), &[r4, r5], "3");

    assert_regions(&folding_model.get_regions_inside(region2.as_ref(), Some(RegionFilter::WithLevel(&level1))), &[r3], "4");
    assert_regions(&folding_model.get_regions_inside(region2.as_ref(), Some(RegionFilter::WithLevel(&level2))), &[r4, r5], "5");
    assert_regions(&folding_model.get_regions_inside(region3.as_ref(), Some(RegionFilter::WithLevel(&level1))), &[r4, r5], "6");

    let hides_line_9 = |r: &FoldingRegion, _level: usize| r.hides_line(9);
    assert_regions(&folding_model.get_regions_inside(region2.as_ref(), Some(RegionFilter::WithLevel(&hides_line_9))), &[r3, r5], "7");

    assert_regions(&folding_model.get_regions_inside(region1.as_ref(), Some(RegionFilter::WithLevel(&level1))), &[], "8");

    text_model.dispose();
}

#[test]
fn get_region_at_line_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "//#region",
        /* 2*/ "class A {",
        /* 3*/ "  void foo() {",
        /* 4*/ "    if (true) {",
        /* 5*/ "      //hello",
        /* 6*/ "    }",
        /* 7*/ "",
        /* 8*/ "  }",
        /* 9*/ "}",
        /*10*/ "//#endregion",
        /*11*/ "",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, Some(&region_markers()), None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 10, false);
    let r2 = r(2, 8, false);
    let r3 = r(3, 7, false);
    let r4 = r(4, 5, false);

    assert_ranges(&folding_model, &[r1, r2, r3, r4], "");

    assert_regions(&folding_model.get_all_regions_at_line(1, None), &[r1], "1");
    assert_regions(&folding_model.get_all_regions_at_line(2, None), &reversed(&[r1, r2]), "2");
    assert_regions(&folding_model.get_all_regions_at_line(3, None), &reversed(&[r1, r2, r3]), "3");
    assert_regions(&folding_model.get_all_regions_at_line(4, None), &reversed(&[r1, r2, r3, r4]), "4");
    assert_regions(&folding_model.get_all_regions_at_line(5, None), &reversed(&[r1, r2, r3, r4]), "5");
    assert_regions(&folding_model.get_all_regions_at_line(6, None), &reversed(&[r1, r2, r3]), "6");
    assert_regions(&folding_model.get_all_regions_at_line(7, None), &reversed(&[r1, r2, r3]), "7");
    assert_regions(&folding_model.get_all_regions_at_line(8, None), &reversed(&[r1, r2]), "8");
    assert_regions(&folding_model.get_all_regions_at_line(9, None), &[r1], "9");
    assert_regions(&folding_model.get_all_regions_at_line(10, None), &[r1], "10");
    assert_regions(&folding_model.get_all_regions_at_line(11, None), &[], "10");

    text_model.dispose();
}

#[test]
fn set_collapse_state_recursivly() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "//#region",
        /* 2*/ "//#endregion",
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
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, Some(&region_markers()), None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 2, false);
    let r2 = r(3, 12, false);
    let r3 = r(4, 11, false);
    let r4 = r(5, 6, false);
    let r5 = r(9, 10, false);
    assert_ranges(&folding_model, &[r1, r2, r3, r4, r5], "");

    folding::set_collapse_state_levels_down(&folding_model, true, usize::MAX, Some(&[4]));
    assert_folded_ranges(&folding_model, &[r3, r4, r5], "1");

    folding::set_collapse_state_levels_down(&folding_model, false, usize::MAX, Some(&[8]));
    assert_folded_ranges(&folding_model, &[], "2");

    folding::set_collapse_state_levels_down(&folding_model, true, usize::MAX, Some(&[12]));
    assert_folded_ranges(&folding_model, &[r2, r3, r4, r5], "1");

    folding::set_collapse_state_levels_down(&folding_model, false, usize::MAX, Some(&[7]));
    assert_folded_ranges(&folding_model, &[r2], "1");

    folding::set_collapse_state_levels_down(&folding_model, false, usize::MAX, None);
    assert_folded_ranges(&folding_model, &[], "1");

    folding::set_collapse_state_levels_down(&folding_model, true, usize::MAX, None);
    assert_folded_ranges(&folding_model, &[r1, r2, r3, r4, r5], "1");

    text_model.dispose();
}

#[test]
fn set_collapse_state_at_level() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "//#region",
        /* 2*/ "//#endregion",
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
        /*13*/ "  //#region",
        /*14*/ "  const bar = 9;",
        /*15*/ "  //#endregion",
        /*16*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, Some(&indented_region_markers()), None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 2, false);
    let r2 = r(3, 15, false);
    let r3 = r(4, 11, false);
    let r4 = r(5, 6, false);
    let r5 = r(9, 10, false);
    let r6 = r(13, 15, false);
    assert_ranges(&folding_model, &[r1, r2, r3, r4, r5, r6], "");

    folding::set_collapse_state_at_level(&folding_model, 1, true, &[]);
    assert_folded_ranges(&folding_model, &[r1, r2], "1");

    folding::set_collapse_state_at_level(&folding_model, 1, false, &[5]);
    assert_folded_ranges(&folding_model, &[r2], "2");

    folding::set_collapse_state_at_level(&folding_model, 1, false, &[1]);
    assert_folded_ranges(&folding_model, &[], "3");

    folding::set_collapse_state_at_level(&folding_model, 2, true, &[]);
    assert_folded_ranges(&folding_model, &[r3, r6], "4");

    folding::set_collapse_state_at_level(&folding_model, 2, false, &[5, 6]);
    assert_folded_ranges(&folding_model, &[r3], "5");

    folding::set_collapse_state_at_level(&folding_model, 3, true, &[4, 9]);
    assert_folded_ranges(&folding_model, &[r3, r4], "6");

    folding::set_collapse_state_at_level(&folding_model, 3, false, &[4, 9]);
    assert_folded_ranges(&folding_model, &[r3], "7");

    text_model.dispose();
}

#[test]
fn set_collapse_state_levels_down() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "//#region",
        /* 2*/ "//#endregion",
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
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, Some(&region_markers()), None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 2, false);
    let r2 = r(3, 12, false);
    let r3 = r(4, 11, false);
    let r4 = r(5, 6, false);
    let r5 = r(9, 10, false);
    assert_ranges(&folding_model, &[r1, r2, r3, r4, r5], "");

    folding::set_collapse_state_levels_down(&folding_model, true, 1, Some(&[4]));
    assert_folded_ranges(&folding_model, &[r3], "1");

    folding::set_collapse_state_levels_down(&folding_model, true, 2, Some(&[4]));
    assert_folded_ranges(&folding_model, &[r3, r4, r5], "2");

    folding::set_collapse_state_levels_down(&folding_model, false, 2, Some(&[3]));
    assert_folded_ranges(&folding_model, &[r4, r5], "3");

    folding::set_collapse_state_levels_down(&folding_model, false, 2, Some(&[2]));
    assert_folded_ranges(&folding_model, &[r4, r5], "4");

    folding::set_collapse_state_levels_down(&folding_model, true, 4, Some(&[2]));
    assert_folded_ranges(&folding_model, &[r1, r4, r5], "5");

    folding::set_collapse_state_levels_down(&folding_model, false, 4, Some(&[2, 3]));
    assert_folded_ranges(&folding_model, &[], "6");

    text_model.dispose();
}

#[test]
fn set_collapse_state_levels_up() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "//#region",
        /* 2*/ "//#endregion",
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
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, Some(&region_markers()), None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 2, false);
    let r2 = r(3, 12, false);
    let r3 = r(4, 11, false);
    let r4 = r(5, 6, false);
    let r5 = r(9, 10, false);
    assert_ranges(&folding_model, &[r1, r2, r3, r4, r5], "");

    folding::set_collapse_state_levels_up(&folding_model, true, 1, &[4]);
    assert_folded_ranges(&folding_model, &[r3], "1");

    folding::set_collapse_state_levels_up(&folding_model, true, 2, &[4]);
    assert_folded_ranges(&folding_model, &[r2, r3], "2");

    folding::set_collapse_state_levels_up(&folding_model, false, 4, &[1, 3, 4]);
    assert_folded_ranges(&folding_model, &[], "3");

    folding::set_collapse_state_levels_up(&folding_model, true, 2, &[10]);
    assert_folded_ranges(&folding_model, &[r3, r5], "4");

    text_model.dispose();
}

#[test]
fn set_collapse_state_up() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "//#region",
        /* 2*/ "//#endregion",
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
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, Some(&region_markers()), None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 2, false);
    let r2 = r(3, 12, false);
    let r3 = r(4, 11, false);
    let r4 = r(5, 6, false);
    let r5 = r(9, 10, false);
    assert_ranges(&folding_model, &[r1, r2, r3, r4, r5], "");

    folding::set_collapse_state_up(&folding_model, true, &[5]);
    assert_folded_ranges(&folding_model, &[r4], "1");

    folding::set_collapse_state_up(&folding_model, true, &[5]);
    assert_folded_ranges(&folding_model, &[r3, r4], "2");

    folding::set_collapse_state_up(&folding_model, true, &[4]);
    assert_folded_ranges(&folding_model, &[r2, r3, r4], "2");

    text_model.dispose();
}

#[test]
fn set_collapse_state_for_matching_lines() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "/**",
        /* 2*/ " * the class",
        /* 3*/ " */",
        /* 4*/ "class A {",
        /* 5*/ "  /**",
        /* 6*/ "   * the foo",
        /* 7*/ "   */",
        /* 8*/ "  void foo() {",
        /* 9*/ "    /*",
        /*10*/ "     * the comment",
        /*11*/ "     */",
        /*12*/ "  }",
        /*13*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, Some(&region_markers()), None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 3, false);
    let r2 = r(4, 12, false);
    let r3 = r(5, 7, false);
    let r4 = r(8, 11, false);
    let r5 = r(9, 11, false);
    assert_ranges(&folding_model, &[r1, r2, r3, r4, r5], "");

    let reg_exp = re(r"^\s*/\*", "");
    folding::set_collapse_state_for_matching_lines(&folding_model, &reg_exp, true);
    assert_folded_ranges(&folding_model, &[r1, r3, r5], "1");

    text_model.dispose();
}

#[test]
fn set_collapse_state_for_rest() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "//#region",
        /* 2*/ "//#endregion",
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
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, Some(&region_markers()), None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 2, false);
    let r2 = r(3, 12, false);
    let r3 = r(4, 11, false);
    let r4 = r(5, 6, false);
    let r5 = r(9, 10, false);
    assert_ranges(&folding_model, &[r1, r2, r3, r4, r5], "");

    folding::set_collapse_state_for_rest(&folding_model, true, &[5]);
    assert_folded_ranges(&folding_model, &[r1, r5], "1");

    folding::set_collapse_state_for_rest(&folding_model, false, &[5]);
    assert_folded_ranges(&folding_model, &[], "2");

    folding::set_collapse_state_for_rest(&folding_model, true, &[1]);
    assert_folded_ranges(&folding_model, &[r2, r3, r4, r5], "3");

    folding::set_collapse_state_for_rest(&folding_model, true, &[3]);
    assert_folded_ranges(&folding_model, &[r1, r2, r3, r4, r5], "3");

    text_model.dispose();
}

#[test]
fn folding_decoration() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    use ExpectedDecorationType::{Collapsed, Expanded, Hidden};
    let lines = [
        /* 1*/ "class A {",
        /* 2*/ "  void foo() {",
        /* 3*/ "    if (true) {",
        /* 4*/ "      hoo();",
        /* 5*/ "    }",
        /* 6*/ "  }",
        /* 7*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, None, None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 6, false);
    let r2 = r(2, 5, false);
    let r3 = r(3, 4, false);

    assert_ranges(&folding_model, &[r1, r2, r3], "");
    assert_decorations(&provider, &[d(1, Expanded), d(2, Expanded), d(3, Expanded)], "");

    folding_model.toggle_collapse_state(vec![folding_model.get_region_at_line(2).unwrap()]);

    assert_ranges(&folding_model, &[r1, r(2, 5, true), r3], "");
    assert_decorations(&provider, &[d(1, Expanded), d(2, Collapsed), d(3, Hidden)], "");

    folding_model.update(&ranges, None);

    assert_ranges(&folding_model, &[r1, r(2, 5, true), r3], "");
    assert_decorations(&provider, &[d(1, Expanded), d(2, Collapsed), d(3, Hidden)], "");

    folding_model.toggle_collapse_state(vec![folding_model.get_region_at_line(1).unwrap()]);

    assert_ranges(&folding_model, &[r(1, 6, true), r(2, 5, true), r3], "");
    assert_decorations(&provider, &[d(1, Collapsed), d(2, Hidden), d(3, Hidden)], "");

    folding_model.update(&ranges, None);

    assert_ranges(&folding_model, &[r(1, 6, true), r(2, 5, true), r3], "");
    assert_decorations(&provider, &[d(1, Collapsed), d(2, Hidden), d(3, Hidden)], "");

    folding_model.toggle_collapse_state(vec![
        folding_model.get_region_at_line(1).unwrap(),
        folding_model.get_region_at_line(3).unwrap(),
    ]);

    assert_ranges(&folding_model, &[r1, r(2, 5, true), r(3, 4, true)], "");
    assert_decorations(&provider, &[d(1, Expanded), d(2, Collapsed), d(3, Hidden)], "");

    folding_model.update(&ranges, None);

    assert_ranges(&folding_model, &[r1, r(2, 5, true), r(3, 4, true)], "");
    assert_decorations(&provider, &[d(1, Expanded), d(2, Collapsed), d(3, Hidden)], "");

    text_model.dispose();
    text_model.dispose();
}

#[test]
fn fold_jumping() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "class A {",
        /* 2*/ "  void foo() {",
        /* 3*/ "    if (1) {",
        /* 4*/ "      a();",
        /* 5*/ "    } else if (2) {",
        /* 6*/ "      if (true) {",
        /* 7*/ "        b();",
        /* 8*/ "      }",
        /* 9*/ "    } else {",
        /*10*/ "      c();",
        /*11*/ "    }",
        /*12*/ "  }",
        /*13*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, None, None);
    folding_model.update(&ranges, None);

    let r1 = r(1, 12, false);
    let r2 = r(2, 11, false);
    let r3 = r(3, 4, false);
    let r4 = r(5, 8, false);
    let r5 = r(6, 7, false);
    let r6 = r(9, 10, false);
    assert_ranges(&folding_model, &[r1, r2, r3, r4, r5, r6], "");

    // Test jump to parent.
    assert_eq!(folding::get_parent_fold_line(7, &folding_model), Some(6));
    assert_eq!(folding::get_parent_fold_line(6, &folding_model), Some(5));
    assert_eq!(folding::get_parent_fold_line(5, &folding_model), Some(2));
    assert_eq!(folding::get_parent_fold_line(2, &folding_model), Some(1));
    assert_eq!(folding::get_parent_fold_line(1, &folding_model), None);

    // Test jump to previous.
    assert_eq!(folding::get_previous_fold_line(10, &folding_model), Some(9));
    assert_eq!(folding::get_previous_fold_line(9, &folding_model), Some(5));
    assert_eq!(folding::get_previous_fold_line(5, &folding_model), Some(3));
    assert_eq!(folding::get_previous_fold_line(3, &folding_model), None);
    // Test when not on a folding region start line.
    assert_eq!(folding::get_previous_fold_line(4, &folding_model), Some(3));
    assert_eq!(folding::get_previous_fold_line(7, &folding_model), Some(6));
    assert_eq!(folding::get_previous_fold_line(8, &folding_model), Some(6));

    // Test jump to next.
    assert_eq!(folding::get_next_fold_line(3, &folding_model), Some(5));
    assert_eq!(folding::get_next_fold_line(5, &folding_model), Some(9));
    assert_eq!(folding::get_next_fold_line(9, &folding_model), None);
    // Test when not on a folding region start line.
    assert_eq!(folding::get_next_fold_line(4, &folding_model), Some(5));
    assert_eq!(folding::get_next_fold_line(7, &folding_model), Some(9));
    assert_eq!(folding::get_next_fold_line(8, &folding_model), Some(9));

    text_model.dispose();
}

#[test]
fn fold_jumping_issue_129503() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "",
        /* 2*/ "if True:",
        /* 3*/ "  print(1)",
        /* 4*/ "if True:",
        /* 5*/ "  print(1)",
        /* 6*/ "",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = compute_ranges(&text_model, false, None, None);
    folding_model.update(&ranges, None);

    let r1 = r(2, 3, false);
    let r2 = r(4, 6, false);
    assert_ranges(&folding_model, &[r1, r2], "");

    // Test jump to next.
    assert_eq!(folding::get_next_fold_line(1, &folding_model), Some(2));
    assert_eq!(folding::get_next_fold_line(2, &folding_model), Some(4));
    assert_eq!(folding::get_next_fold_line(3, &folding_model), Some(4));
    assert_eq!(folding::get_next_fold_line(4, &folding_model), None);
    assert_eq!(folding::get_next_fold_line(5, &folding_model), None);
    assert_eq!(folding::get_next_fold_line(6, &folding_model), None);

    // Test jump to previous.
    assert_eq!(folding::get_previous_fold_line(1, &folding_model), None);
    assert_eq!(folding::get_previous_fold_line(2, &folding_model), None);
    assert_eq!(folding::get_previous_fold_line(3, &folding_model), Some(2));
    assert_eq!(folding::get_previous_fold_line(4, &folding_model), Some(2));
    assert_eq!(folding::get_previous_fold_line(5, &folding_model), Some(4));
    assert_eq!(folding::get_previous_fold_line(6, &folding_model), Some(4));

    text_model.dispose();
}

#[test]
fn remove_manual_ranges_cursor_on_manual_range_removes_innermost_only() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "class A {",
        /* 2*/ "  void foo() {",
        /* 3*/ "    if (true) {",
        /* 4*/ "      return;",
        /* 5*/ "    }",
        /* 6*/ "  }",
        /* 7*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    // Set up ranges: outer provider range + two nested manual ranges
    let ranges = [
        fold_range(1, 6, FoldSource::Provider),
        fold_range(2, 5, FoldSource::UserDefined),
        fold_range(3, 4, FoldSource::UserDefined),
    ];
    folding_model.update(&FoldingRegions::from_fold_ranges(&ranges), None);
    assert_ranges(&folding_model, &[r(1, 6, false), r(2, 5, false), r(3, 4, false)], "");

    // Cursor on line 4 (inside innermost manual range 3-4): should remove only 3-4
    folding_model.remove_manual_ranges(&[Range::new(4, 1, 4, 1)]);
    assert_ranges(&folding_model, &[r(1, 6, false), r(2, 5, false)], "");

    // Cursor on line 3 (inside remaining manual range 2-5): should remove only 2-5
    folding_model.remove_manual_ranges(&[Range::new(3, 1, 3, 1)]);
    assert_ranges(&folding_model, &[r(1, 6, false)], "");

    text_model.dispose();
}

#[test]
fn remove_manual_ranges_cursor_skips_provider_ranges_to_remove_nearest_manual_range() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "class A {",
        /* 2*/ "  void foo() {",
        /* 3*/ "    return;",
        /* 4*/ "  }",
        /* 5*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);
    let ranges = [fold_range(1, 5, FoldSource::UserDefined), fold_range(2, 4, FoldSource::Provider)];
    folding_model.update(&FoldingRegions::from_fold_ranges(&ranges), None);

    folding_model.remove_manual_ranges(&[Range::new(3, 1, 3, 1)]);
    assert_ranges(&folding_model, &[r(2, 4, false)], "");

    text_model.dispose();
}

#[test]
fn remove_manual_ranges_cursor_not_on_manual_range_removes_all_manual_ranges() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "// header",
        /* 2*/ "class A {",
        /* 3*/ "  void foo() {",
        /* 4*/ "  }",
        /* 5*/ "}",
        /* 6*/ "// footer",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    // Provider range at 2-4, manual range at 3-4
    let ranges = [fold_range(2, 4, FoldSource::Provider), fold_range(3, 4, FoldSource::UserDefined)];
    folding_model.update(&FoldingRegions::from_fold_ranges(&ranges), None);
    assert_ranges(&folding_model, &[r(2, 4, false), r(3, 4, false)], "");

    // A single-line selection outside manual ranges should preserve them
    folding_model.remove_manual_ranges(&[Range::new(6, 1, 6, 2)]);
    assert_ranges(&folding_model, &[r(2, 4, false), r(3, 4, false)], "");

    // Cursor on line 6 (not inside any manual range): should remove all manual ranges
    folding_model.remove_manual_ranges(&[Range::new(6, 1, 6, 1)]);
    assert_ranges(&folding_model, &[r(2, 4, false)], "");

    text_model.dispose();
}

#[test]
fn remove_manual_ranges_single_line_selection_removes_all_intersecting_manual_ranges() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "class A {",
        /* 2*/ "  void foo() {",
        /* 3*/ "    if (true) {",
        /* 4*/ "      return;",
        /* 5*/ "    }",
        /* 6*/ "  }",
        /* 7*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);
    let ranges = [
        fold_range(1, 6, FoldSource::Provider),
        fold_range(2, 5, FoldSource::UserDefined),
        fold_range(3, 4, FoldSource::UserDefined),
    ];
    folding_model.update(&FoldingRegions::from_fold_ranges(&ranges), None);

    folding_model.remove_manual_ranges(&[Range::new(4, 1, 4, 2)]);
    assert_ranges(&folding_model, &[r(1, 6, false)], "");

    text_model.dispose();
}

#[test]
fn remove_manual_ranges_selection_range_removes_intersecting_manual_ranges() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = [
        /* 1*/ "class A {",
        /* 2*/ "  void foo() {",
        /* 3*/ "  }",
        /* 4*/ "  void bar() {",
        /* 5*/ "  }",
        /* 6*/ "}",
    ];

    let text_model = create_text_model(&lines.join("\n"));
    let (folding_model, _provider) = new_folding_model(&text_model);

    let ranges = [
        fold_range(1, 5, FoldSource::Provider),
        fold_range(2, 3, FoldSource::UserDefined),
        fold_range(4, 5, FoldSource::UserDefined),
    ];
    folding_model.update(&FoldingRegions::from_fold_ranges(&ranges), None);
    assert_ranges(&folding_model, &[r(1, 5, false), r(2, 3, false), r(4, 5, false)], "");

    // Selection spanning lines 2-3: removes only the first manual range
    folding_model.remove_manual_ranges(&[Range::new(2, 1, 3, 1)]);
    assert_ranges(&folding_model, &[r(1, 5, false), r(4, 5, false)], "");

    text_model.dispose();
}
