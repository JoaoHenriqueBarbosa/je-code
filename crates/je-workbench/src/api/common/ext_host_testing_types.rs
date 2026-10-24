//! Porte da região "Testing" e da região "Test Coverage" de `src/vs/workbench/api/common/extHostTypes.ts`
//! (`TestRunProfileBase`, `TestRunRequest`, `TestMessage`, `TestTag`, `TestMessageStackFrame`, `TestCoverageCount`,
//! `FileCoverage`, `StatementCoverage`, `BranchCoverage`, `DeclarationCoverage`) mais as formas de resultado da API
//! `vscode` que `extHostTypeConverters.ts` monta (`TestResultSnapshot`, `TestRunResult`) e o `TestItem` simples que
//! `TestItem.toPlain` devolve. `TestResultState` e `TestRunProfileKind` já são os de [`super::ext_host_types`].
//!
//! Divergências:
//! - **Objetos da API.** Uma classe do original é um `struct` de campos públicos. `TestRunRequest.profile` é o
//!   `Rc<TestRunProfileImpl>` (o `instanceof TestRunProfileImpl` do original é garantido pelo tipo) e `include` e
//!   `exclude` são listas de [`TestItemImpl`]. `continuous` e `preserveFocus` são `bool` (o literal sem esses campos,
//!   que o original aceita como `undefined`, é `false` aqui, e o valor padrão do construtor é o de [`Default`]).
//! - **`TestMessage.message`** (`string | MarkdownString`) é o [`Documentation`] do porte.
//! - **`number | boolean`** de `executed` é o [`CoverageCountValue`] do `testTypes`, e `Position | Range` é
//!   [`ApiCoverageLocation`]. `validateTestCoverageCount` e o construtor de [`TestCoverageCount`] devolvem [`Result`].
//! - **`FileCoverageDetail`** (`StatementCoverage | DeclarationCoverage`) é um `enum`: o `'branches' in detail` do
//!   original é o `match`. `executionCount` (alias de compatibilidade de `executed`) não existe.
//! - **`TestResultSnapshot`** guarda o `parent` como referência fraca e os `children` numa célula, porque o pai nasce
//!   antes dos filhos. O `taskStates[].state` é `Option<TestResultState>`: `None` é o `0` (`Unset`) que o enum da API
//!   não tem (o original só reinterpreta o número).
//! - **`TestRunResult.getDetailedCoverage`** é uma célula com o fecho, preenchida por `$publishTestResults`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::uri::URI;

use super::ext_host_test_item::TestItemImpl;
use super::ext_host_testing::TestRunProfileImpl;
use super::ext_host_types::{Documentation, Location, Position, Range, TestRunProfileKind};
use crate::contrib::testing::common::test_types::CoverageCountValue;

// #region Testing

/// `TestRunProfileBase`.
#[derive(Clone, Debug)]
pub struct TestRunProfileBase {
    pub controller_id: String,
    pub profile_id: u32,
    pub kind: TestRunProfileKind,
}

impl TestRunProfileBase {
    pub fn new(controller_id: &str, profile_id: u32, kind: TestRunProfileKind) -> TestRunProfileBase {
        TestRunProfileBase { controller_id: controller_id.to_string(), profile_id, kind }
    }
}

/// `TestRunRequest`.
#[derive(Clone)]
pub struct TestRunRequest {
    pub include: Option<Vec<TestItemImpl>>,
    pub exclude: Option<Vec<TestItemImpl>>,
    pub profile: Option<Rc<TestRunProfileImpl>>,
    pub continuous: bool,
    pub preserve_focus: bool,
}

impl TestRunRequest {
    pub fn new(
        include: Option<Vec<TestItemImpl>>,
        exclude: Option<Vec<TestItemImpl>>,
        profile: Option<Rc<TestRunProfileImpl>>,
        continuous: bool,
        preserve_focus: bool,
    ) -> TestRunRequest {
        TestRunRequest { include, exclude, profile, continuous, preserve_focus }
    }
}

impl Default for TestRunRequest {
    /// `new TestRunRequest()`: sem itens nem perfil, `continuous = false` e `preserveFocus = true`.
    fn default() -> TestRunRequest {
        TestRunRequest::new(None, None, None, false, true)
    }
}

/// `TestMessageStackFrame`.
#[derive(Clone, Debug, PartialEq)]
pub struct TestMessageStackFrame {
    pub label: String,
    pub uri: Option<URI>,
    pub position: Option<Position>,
}

/// `TestMessage`.
#[derive(Clone, Debug)]
pub struct TestMessage {
    pub message: Documentation,
    pub expected_output: Option<String>,
    pub actual_output: Option<String>,
    pub location: Option<Location>,
    pub context_value: Option<String>,
    /// proposed:
    pub stack_trace: Option<Vec<TestMessageStackFrame>>,
}

impl TestMessage {
    /// `new TestMessage(message)`.
    pub fn new(message: Documentation) -> TestMessage {
        TestMessage { message, expected_output: None, actual_output: None, location: None, context_value: None, stack_trace: None }
    }

    /// `new TestMessage('texto')`.
    pub fn from_text(message: &str) -> TestMessage {
        TestMessage::new(Documentation::Text(message.to_string()))
    }

    /// `TestMessage.diff(message, expected, actual)`.
    pub fn diff(message: Documentation, expected: &str, actual: &str) -> TestMessage {
        let mut msg = TestMessage::new(message);
        msg.expected_output = Some(expected.to_string());
        msg.actual_output = Some(actual.to_string());
        msg
    }
}

/// `TestTag`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestTag {
    pub id: String,
}

impl TestTag {
    pub fn new(id: &str) -> TestTag {
        TestTag { id: id.to_string() }
    }
}

// #endregion

// #region Test Coverage

/// `TestCoverageCount`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TestCoverageCount {
    pub covered: f64,
    pub total: f64,
}

impl TestCoverageCount {
    /// `new TestCoverageCount(covered, total)`: lança (devolve o erro) se a contagem for inválida.
    pub fn new(covered: f64, total: f64) -> Result<TestCoverageCount, Error> {
        let count = TestCoverageCount { covered, total };
        validate_test_coverage_count(Some(&count))?;
        Ok(count)
    }
}

/// `validateTestCoverageCount`.
pub fn validate_test_coverage_count(cc: Option<&TestCoverageCount>) -> Result<(), Error> {
    let Some(cc) = cc else {
        return Ok(());
    };

    if cc.covered > cc.total {
        return Err(Error::new(format!("The total number of covered items ({}) cannot be greater than the total ({})", cc.covered, cc.total)));
    }

    if cc.total < 0.0 {
        return Err(Error::new(format!("The number of covered items ({}) cannot be negative", cc.total)));
    }

    Ok(())
}

/// `Position | Range`: o `location` de uma cobertura.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ApiCoverageLocation {
    Position(Position),
    Range(Range),
}

/// `StatementCoverage`.
#[derive(Clone, Debug, PartialEq)]
pub struct StatementCoverage {
    pub executed: CoverageCountValue,
    pub location: ApiCoverageLocation,
    pub branches: Vec<BranchCoverage>,
}

impl StatementCoverage {
    pub fn new(executed: CoverageCountValue, location: ApiCoverageLocation, branches: Vec<BranchCoverage>) -> StatementCoverage {
        StatementCoverage { executed, location, branches }
    }
}

/// `BranchCoverage`.
#[derive(Clone, Debug, PartialEq)]
pub struct BranchCoverage {
    pub executed: CoverageCountValue,
    pub location: ApiCoverageLocation,
    pub label: Option<String>,
}

impl BranchCoverage {
    pub fn new(executed: CoverageCountValue, location: ApiCoverageLocation, label: Option<String>) -> BranchCoverage {
        BranchCoverage { executed, location, label }
    }
}

/// `DeclarationCoverage`.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclarationCoverage {
    pub name: String,
    pub executed: CoverageCountValue,
    pub location: ApiCoverageLocation,
}

impl DeclarationCoverage {
    pub fn new(name: &str, executed: CoverageCountValue, location: ApiCoverageLocation) -> DeclarationCoverage {
        DeclarationCoverage { name: name.to_string(), executed, location }
    }
}

/// `FileCoverageDetail`: `StatementCoverage | DeclarationCoverage`.
#[derive(Clone, Debug, PartialEq)]
pub enum FileCoverageDetail {
    Statement(StatementCoverage),
    Declaration(DeclarationCoverage),
}

/// `FileCoverage`.
#[derive(Clone, Debug)]
pub struct FileCoverage {
    pub uri: URI,
    pub statement_coverage: TestCoverageCount,
    pub branch_coverage: Option<TestCoverageCount>,
    pub declaration_coverage: Option<TestCoverageCount>,
    pub includes_tests: Vec<TestItemImpl>,
    pub detailed_coverage: Option<Vec<FileCoverageDetail>>,
}

impl FileCoverage {
    /// `new FileCoverage(uri, statementCoverage, branchCoverage?, declarationCoverage?, includesTests = [])`.
    pub fn new(
        uri: URI,
        statement_coverage: TestCoverageCount,
        branch_coverage: Option<TestCoverageCount>,
        declaration_coverage: Option<TestCoverageCount>,
        includes_tests: Vec<TestItemImpl>,
    ) -> FileCoverage {
        FileCoverage { uri, statement_coverage, branch_coverage, declaration_coverage, includes_tests, detailed_coverage: None }
    }

    /// `FileCoverage.fromDetails(uri, details)`.
    pub fn from_details(uri: URI, details: Vec<FileCoverageDetail>) -> FileCoverage {
        let mut statements = TestCoverageCount { covered: 0.0, total: 0.0 };
        let mut branches = TestCoverageCount { covered: 0.0, total: 0.0 };
        let mut decl = TestCoverageCount { covered: 0.0, total: 0.0 };

        for detail in &details {
            match detail {
                FileCoverageDetail::Statement(statement) => {
                    statements.total += 1.0;
                    statements.covered += if statement.executed.is_truthy() { 1.0 } else { 0.0 };

                    for branch in &statement.branches {
                        branches.total += 1.0;
                        branches.covered += if branch.executed.is_truthy() { 1.0 } else { 0.0 };
                    }
                }
                FileCoverageDetail::Declaration(declaration) => {
                    decl.total += 1.0;
                    decl.covered += if declaration.executed.is_truthy() { 1.0 } else { 0.0 };
                }
            }
        }

        let mut coverage = FileCoverage::new(
            uri,
            statements,
            if branches.total > 0.0 { Some(branches) } else { None },
            if decl.total > 0.0 { Some(decl) } else { None },
            Vec::new(),
        );

        coverage.detailed_coverage = Some(details);

        coverage
    }
}

// #endregion

// #region Resultados e itens simples

/// O `vscode.TestItem` simples que `TestItem.toPlain` devolve: sem pai, sem erro, sem filhos e sem `canResolveChildren`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlainTestItem {
    pub id: String,
    pub label: String,
    pub uri: Option<URI>,
    pub tags: Vec<TestTag>,
    pub range: Option<Range>,
    pub busy: bool,
    pub description: Option<String>,
    pub sort_text: Option<String>,
}

/// O `{ state, duration, messages }` de `TestResultSnapshot.taskStates`.
#[derive(Clone, Debug)]
pub struct TestSnapshotTaskState {
    pub state: Option<super::ext_host_types::TestResultState>,
    pub duration: Option<f64>,
    pub messages: Vec<TestMessage>,
}

/// `vscode.TestResultSnapshot`.
pub struct TestResultSnapshot {
    pub item: PlainTestItem,
    pub parent: Option<Weak<TestResultSnapshot>>,
    pub task_states: Vec<TestSnapshotTaskState>,
    pub children: RefCell<Vec<Rc<TestResultSnapshot>>>,
}

/// `getDetailedCoverage(uri, token)` de `vscode.TestRunResult`.
pub type GetDetailedCoverage = Rc<dyn Fn(&URI, &CancellationToken) -> Promise<Vec<FileCoverageDetail>>>;

/// `vscode.TestRunResult`.
pub struct TestRunResult {
    pub completed_at: f64,
    pub results: Vec<Rc<TestResultSnapshot>>,
    pub get_detailed_coverage: RefCell<Option<GetDetailedCoverage>>,
}

// #endregion
