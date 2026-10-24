//! Porte de `src/vs/workbench/contrib/testing/test/browser/codeCoverageDecorations.test.ts` (suíte `Code Coverage
//! Decorations`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no começo de cada caso.
//! - `upcastPartial<ITextModel>({ getValueInRange: () => '' })` é o fechamento `get_value_in_range` que devolve `""`.
//! - `assertSnapshot` compara com os arquivos `__snapshots__/Code_Coverage_Decorations_CoverageDetailsModel_N.0.snap`
//!   do oráculo; cada lista `{ range, count }` vira `(&str, f64)` literal, na mesma ordem.
//! - `r.range.toString()` é o `Display` de `Range`; o `count` do ramo vem de `detail.branches[branch].count`.
//! - `new Range(...)` e `new Position(...)` viram `CoverageLocation::Range`/`Position`; o `async` dos casos não espera nada.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use je_editor::position::Position;
use je_editor::range::Range;
use je_workbench::contrib::testing::browser::code_coverage_decorations::{CoverageDetailsModel, CoverageDetailsWithBranch};
use je_workbench::contrib::testing::common::test_types::{
    CoverageCountValue, CoverageDetails, CoverageLocation, IBranchCoverage, IStatementCoverage,
};

fn count_of(detail: &CoverageDetailsWithBranch) -> f64 {
    let value = match detail {
        CoverageDetailsWithBranch::Branch { branch, detail } => detail.branches.as_ref().unwrap()[*branch].count,
        CoverageDetailsWithBranch::Detail(CoverageDetails::Statement(statement)) => statement.count,
        CoverageDetailsWithBranch::Detail(CoverageDetails::Declaration(declaration)) => declaration.count,
    };
    match value {
        CoverageCountValue::Number(n) => n,
        CoverageCountValue::Bool(b) => f64::from(u8::from(b)),
    }
}

fn assert_ranges(model: &CoverageDetailsModel, expected: &[(&str, f64)]) {
    let actual: Vec<(String, f64)> = model.ranges.iter().map(|r| (r.range.to_string(), count_of(&r.metadata.detail))).collect();
    let expected: Vec<(String, f64)> = expected.iter().map(|(range, count)| (range.to_string(), *count)).collect();
    assert_eq!(actual, expected);
}

fn statement(location: CoverageLocation, count: f64, branches: Option<Vec<IBranchCoverage>>) -> CoverageDetails {
    CoverageDetails::Statement(IStatementCoverage { count: CoverageCountValue::Number(count), location, branches })
}

fn range(a: i32, b: i32, c: i32, d: i32) -> CoverageLocation {
    CoverageLocation::Range(Range::new(a, b, c, d))
}

fn new_model(details: Vec<CoverageDetails>) -> CoverageDetailsModel {
    CoverageDetailsModel::new(details, &|_| String::new())
}

#[test]
fn coverage_details_model_1() {
    let _guard = common::ensure_no_disposables_are_leaked_in_test_suite();
    let details = vec![
        statement(range(1, 0, 5, 0), 1.0, None),
        statement(range(2, 0, 3, 0), 2.0, None),
        statement(
            range(4, 0, 6, 0),
            4.0,
            Some(vec![IBranchCoverage { location: Some(range(3, 0, 7, 0)), count: CoverageCountValue::Number(3.0), label: None }]),
        ),
    ];
    let model = new_model(details);
    assert_ranges(
        &model,
        &[
            ("[1,0 -> 2,0]", 1.0),
            ("[2,0 -> 3,0]", 2.0),
            ("[3,0 -> 4,0]", 3.0),
            ("[4,0 -> 6,0]", 4.0),
            ("[6,0 -> 7,0]", 3.0),
            ("[7,0 -> 7,0]", 2.0),
            ("[5,0 -> 7,0]", 1.0),
        ],
    );
}

#[test]
fn coverage_details_model_2() {
    let _guard = common::ensure_no_disposables_are_leaked_in_test_suite();
    let details = vec![
        statement(range(1, 0, 5, 0), 1.0, None),
        statement(range(2, 0, 4, 0), 2.0, None),
        statement(range(3, 0, 3, 5), 3.0, None),
    ];
    let model = new_model(details);
    assert_ranges(
        &model,
        &[("[1,0 -> 2,0]", 1.0), ("[2,0 -> 3,0]", 2.0), ("[3,0 -> 3,5]", 3.0), ("[3,5 -> 4,0]", 2.0), ("[4,0 -> 5,0]", 1.0)],
    );
}

#[test]
fn coverage_details_model_3() {
    let _guard = common::ensure_no_disposables_are_leaked_in_test_suite();
    let details = vec![
        statement(range(1, 0, 5, 0), 1.0, None),
        statement(range(2, 0, 3, 0), 2.0, None),
        statement(range(4, 0, 5, 0), 3.0, None),
    ];
    let model = new_model(details);
    assert_ranges(&model, &[("[1,0 -> 2,0]", 1.0), ("[2,0 -> 3,0]", 2.0), ("[3,0 -> 4,0]", 1.0), ("[4,0 -> 5,0]", 3.0)]);
}

#[test]
fn coverage_details_model_4() {
    let _guard = common::ensure_no_disposables_are_leaked_in_test_suite();
    let details = vec![
        statement(range(1, 0, 5, 0), 1.0, None),
        statement(CoverageLocation::Position(Position::new(2, 0)), 2.0, None),
        statement(range(4, 0, 5, 0), 3.0, None),
        statement(CoverageLocation::Position(Position::new(4, 3)), 4.0, None),
    ];
    let model = new_model(details);
    assert_ranges(
        &model,
        &[
            ("[1,0 -> 2,0]", 1.0),
            ("[2,0 -> 2,2147483647]", 2.0),
            ("[2,2147483647 -> 4,0]", 1.0),
            ("[4,0 -> 4,3]", 3.0),
            ("[4,3 -> 4,2147483647]", 4.0),
            ("[4,2147483647 -> 5,0]", 3.0),
        ],
    );
}

#[test]
fn has_inline_coverage_details_context_key() {
    let _guard = common::ensure_no_disposables_are_leaked_in_test_suite();
    // Test that CoverageDetailsModel with ranges indicates inline coverage is available
    let model_with_ranges = new_model(vec![statement(range(1, 0, 2, 0), 1.0, None)]);

    // Should have ranges available for inline display
    assert!(!model_with_ranges.ranges.is_empty(), "Model with coverage details should have ranges");

    // Test that empty coverage details indicates no inline coverage
    let empty_model = new_model(Vec::new());

    // Should have no ranges available for inline display
    assert!(empty_model.ranges.is_empty(), "Model with no coverage details should have no ranges");
}
