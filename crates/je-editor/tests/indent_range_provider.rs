//! Porte de `src/vs/editor/contrib/folding/test/browser/indentRangeProvider.test.ts` (suítes `Indentation Folding` e `Folding with regions`), caso a caso e na mesma ordem.
//!
//! O quarto argumento de `r` (`marker`) não é usado no original e foi omitido.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::folding::indent_range_provider::compute_ranges;
use je_editor::language_configuration::FoldingMarkers;
use support::re;
use support::test_text_model::create_text_model;

#[derive(Debug, PartialEq, Eq)]
struct ExpectedIndentRange {
    start_line_number: u32,
    end_line_number: u32,
    parent_index: i32,
}

fn assert_ranges(lines: &[&str], expected: &[ExpectedIndentRange], offside: bool, markers: Option<&FoldingMarkers>) {
    let model = create_text_model(&lines.join("\n"));
    let actual = compute_ranges(&model, offside, markers, None);

    let mut actual_ranges: Vec<ExpectedIndentRange> = Vec::new();
    for i in 0..actual.length() {
        actual_ranges.push(r(actual.get_start_line_number(i), actual.get_end_line_number(i), actual.get_parent_index(i)));
    }
    assert_eq!(actual_ranges, expected);
    model.dispose();
}

fn r(start_line_number: u32, end_line_number: u32, parent_index: i32) -> ExpectedIndentRange {
    ExpectedIndentRange { start_line_number, end_line_number, parent_index }
}

// suite('Indentation Folding')

#[test]
fn fold_one_level() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let range = ["A", "  A", "  A", "  A"];
    assert_ranges(&range, &[r(1, 4, -1)], true, None);
    assert_ranges(&range, &[r(1, 4, -1)], false, None);
}

#[test]
fn fold_two_levels() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let range = ["A", "  A", "  A", "    A", "    A"];
    assert_ranges(&range, &[r(1, 5, -1), r(3, 5, 0)], true, None);
    assert_ranges(&range, &[r(1, 5, -1), r(3, 5, 0)], false, None);
}

#[test]
fn fold_three_levels() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let range = ["A", "  A", "    A", "      A", "A"];
    assert_ranges(&range, &[r(1, 4, -1), r(2, 4, 0), r(3, 4, 1)], true, None);
    assert_ranges(&range, &[r(1, 4, -1), r(2, 4, 0), r(3, 4, 1)], false, None);
}

#[test]
fn fold_decreasing_indent() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let range = ["    A", "  A", "A"];
    assert_ranges(&range, &[], true, None);
    assert_ranges(&range, &[], false, None);
}

#[test]
fn fold_java() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "class A {",
            /* 2*/ "  void foo() {",
            /* 3*/ "    console.log();",
            /* 4*/ "    console.log();",
            /* 5*/ "  }",
            /* 6*/ "",
            /* 7*/ "  void bar() {",
            /* 8*/ "    console.log();",
            /* 9*/ "  }",
            /*10*/ "}",
            /*11*/ "interface B {",
            /*12*/ "  void bar();",
            /*13*/ "}",
        ],
        &[r(1, 9, -1), r(2, 4, 0), r(7, 8, 0), r(11, 12, -1)],
        false,
        None,
    );
}

#[test]
fn fold_javadoc() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "/**",
            /* 2*/ " * Comment",
            /* 3*/ " */",
            /* 4*/ "class A {",
            /* 5*/ "  void foo() {",
            /* 6*/ "  }",
            /* 7*/ "}",
        ],
        &[r(1, 3, -1), r(4, 6, -1)],
        false,
        None,
    );
}

#[test]
fn fold_whitespace_java() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "class A {",
            /* 2*/ "",
            /* 3*/ "  void foo() {",
            /* 4*/ "     ",
            /* 5*/ "     return 0;",
            /* 6*/ "  }",
            /* 7*/ "      ",
            /* 8*/ "}",
        ],
        &[r(1, 7, -1), r(3, 5, 0)],
        false,
        None,
    );
}

#[test]
fn fold_whitespace_python() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "def a:",
            /* 2*/ "  pass",
            /* 3*/ "   ",
            /* 4*/ "  def b:",
            /* 5*/ "    pass",
            /* 6*/ "  ",
            /* 7*/ "      ",
            /* 8*/ "def c: # since there was a deintent here",
        ],
        &[r(1, 5, -1), r(4, 5, 0)],
        true,
        None,
    );
}

#[test]
fn fold_tabs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "class A {",
            /* 2*/ "\t\t",
            /* 3*/ "\tvoid foo() {",
            /* 4*/ "\t \t//hello",
            /* 5*/ "\t    return 0;",
            /* 6*/ "  \t}",
            /* 7*/ "      ",
            /* 8*/ "}",
        ],
        &[r(1, 7, -1), r(3, 5, 0)],
        false,
        None,
    );
}

// suite('Folding with regions')

fn markers() -> FoldingMarkers {
    FoldingMarkers { start: re(r"^\s*#region\b", ""), end: re(r"^\s*#endregion\b", "") }
}

#[test]
fn inside_region_indented() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "class A {",
            /* 2*/ "  #region",
            /* 3*/ "  void foo() {",
            /* 4*/ "     ",
            /* 5*/ "     return 0;",
            /* 6*/ "  }",
            /* 7*/ "  #endregion",
            /* 8*/ "}",
        ],
        &[r(1, 7, -1), r(2, 7, 0), r(3, 5, 1)],
        false,
        Some(&markers()),
    );
}

#[test]
fn inside_region_not_indented() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "var x;",
            /* 2*/ "#region",
            /* 3*/ "void foo() {",
            /* 4*/ "     ",
            /* 5*/ "     return 0;",
            /* 6*/ "  }",
            /* 7*/ "#endregion",
            /* 8*/ "",
        ],
        &[r(2, 7, -1), r(3, 6, 0)],
        false,
        Some(&markers()),
    );
}

#[test]
fn empty_regions() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "var x;",
            /* 2*/ "#region",
            /* 3*/ "#endregion",
            /* 4*/ "#region",
            /* 5*/ "",
            /* 6*/ "#endregion",
            /* 7*/ "var y;",
        ],
        &[r(2, 3, -1), r(4, 6, -1)],
        false,
        Some(&markers()),
    );
}

#[test]
fn nested_regions() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "var x;",
            /* 2*/ "#region",
            /* 3*/ "#region",
            /* 4*/ "",
            /* 5*/ "#endregion",
            /* 6*/ "#endregion",
            /* 7*/ "var y;",
        ],
        &[r(2, 6, -1), r(3, 5, 0)],
        false,
        Some(&markers()),
    );
}

#[test]
fn nested_regions_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "class A {",
            /* 2*/ "  #region",
            /* 3*/ "",
            /* 4*/ "  #region",
            /* 5*/ "",
            /* 6*/ "  #endregion",
            /* 7*/ "  // comment",
            /* 8*/ "  #endregion",
            /* 9*/ "}",
        ],
        &[r(1, 8, -1), r(2, 8, 0), r(4, 6, 1)],
        false,
        Some(&markers()),
    );
}

#[test]
fn incomplete_regions() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "class A {",
            /* 2*/ "#region",
            /* 3*/ "  // comment",
            /* 4*/ "}",
        ],
        &[r(2, 3, -1)],
        false,
        Some(&markers()),
    );
}

#[test]
fn incomplete_regions_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "",
            /* 2*/ "#region",
            /* 3*/ "#region",
            /* 4*/ "#region",
            /* 5*/ "  // comment",
            /* 6*/ "#endregion",
            /* 7*/ "#endregion",
            /* 8*/ " // hello",
        ],
        &[r(3, 7, -1), r(4, 6, 0)],
        false,
        Some(&markers()),
    );
}

#[test]
fn indented_region_before() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "if (x)",
            /* 2*/ "  return;",
            /* 3*/ "",
            /* 4*/ "#region",
            /* 5*/ "  // comment",
            /* 6*/ "#endregion",
        ],
        &[r(1, 3, -1), r(4, 6, -1)],
        false,
        Some(&markers()),
    );
}

#[test]
fn indented_region_before_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "if (x)",
            /* 2*/ "  log();",
            /* 3*/ "",
            /* 4*/ "    #region",
            /* 5*/ "      // comment",
            /* 6*/ "    #endregion",
        ],
        &[r(1, 6, -1), r(2, 6, 0), r(4, 6, 1)],
        false,
        Some(&markers()),
    );
}

#[test]
fn indented_region_in_between() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "#region",
            /* 2*/ "  // comment",
            /* 3*/ "  if (x)",
            /* 4*/ "    return;",
            /* 5*/ "",
            /* 6*/ "#endregion",
        ],
        &[r(1, 6, -1), r(3, 5, 0)],
        false,
        Some(&markers()),
    );
}

#[test]
fn indented_region_after() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "#region",
            /* 2*/ "  // comment",
            /* 3*/ "",
            /* 4*/ "#endregion",
            /* 5*/ "  if (x)",
            /* 6*/ "    return;",
        ],
        &[r(1, 4, -1), r(5, 6, -1)],
        false,
        Some(&markers()),
    );
}

#[test]
fn with_off_side() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "#region",
            /* 2*/ "  ",
            /* 3*/ "",
            /* 4*/ "#endregion",
            /* 5*/ "",
        ],
        &[r(1, 4, -1)],
        true,
        Some(&markers()),
    );
}

#[test]
fn nested_with_off_side() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "#region",
            /* 2*/ "  ",
            /* 3*/ "#region",
            /* 4*/ "",
            /* 5*/ "#endregion",
            /* 6*/ "",
            /* 7*/ "#endregion",
            /* 8*/ "",
        ],
        &[r(1, 7, -1), r(3, 5, 0)],
        true,
        Some(&markers()),
    );
}

#[test]
fn issue_35981() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "function thisFoldsToEndOfPage() {",
            /* 2*/ "  const variable = []",
            /* 3*/ "    // #region",
            /* 4*/ "    .reduce((a, b) => a,[]);",
            /* 5*/ "}",
            /* 6*/ "",
            /* 7*/ "function thisFoldsProperly() {",
            /* 8*/ "  const foo = \"bar\"",
            /* 9*/ "}",
        ],
        &[r(1, 4, -1), r(2, 4, 0), r(7, 8, -1)],
        false,
        Some(&markers()),
    );
}

#[test]
fn misspelled_markers() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "#Region",
            /* 2*/ "#endregion",
            /* 3*/ "#regionsandmore",
            /* 4*/ "#endregion",
            /* 5*/ "#region",
            /* 6*/ "#end region",
            /* 7*/ "#region",
            /* 8*/ "#endregionff",
        ],
        &[],
        true,
        Some(&markers()),
    );
}

#[test]
fn issue_79359() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "#region",
            /* 2*/ "",
            /* 3*/ "class A",
            /* 4*/ "  foo",
            /* 5*/ "",
            /* 6*/ "class A",
            /* 7*/ "  foo",
            /* 8*/ "",
            /* 9*/ "#endregion",
        ],
        &[r(1, 9, -1), r(3, 4, 0), r(6, 7, 0)],
        true,
        Some(&markers()),
    );
}

#[test]
fn markers_with_stateful_flags() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_ranges(
        &[
            /* 1*/ "#region",
            /* 2*/ "content",
            /* 3*/ "#endregion",
        ],
        &[r(1, 3, -1)],
        false,
        Some(&FoldingMarkers { start: re(r"^\s*#region\b", "g"), end: re(r"^\s*#endregion\b", "g") }),
    );

    assert_ranges(
        &[
            /* 1*/ "#REGION",
            /* 2*/ "content",
            /* 3*/ "#endregion",
        ],
        &[r(1, 3, -1)],
        false,
        Some(&FoldingMarkers { start: re(r"^\s*#region\b", "gi"), end: re(r"^\s*#endregion\b", "g") }),
    );

    assert_ranges(
        &[
            /* 1*/ "#REGION",
            /* 2*/ "content",
            /* 3*/ "#ENDREGION",
        ],
        &[],
        false,
        Some(&FoldingMarkers { start: re(r"^\s*#region\b", "gi"), end: re(r"^\s*#endregion\b", "g") }),
    );

    assert_ranges(
        &[
            /* 1*/ "#REGION",
            /* 2*/ "content",
            /* 3*/ "#ENDREGION",
        ],
        &[],
        false,
        Some(&FoldingMarkers { start: re(r"^\s*#region\b", "g"), end: re(r"^\s*#endregion\b", "gi") }),
    );
}
