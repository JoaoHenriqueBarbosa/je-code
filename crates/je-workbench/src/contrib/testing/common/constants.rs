//! Porte de `src/vs/workbench/contrib/testing/common/constants.ts`: os identificadores do contrib testing (viewlet,
//! views, comandos), os modos do explorador e os textos dos estados.
//!
//! Divergências:
//! - **`const enum` de texto** (`Testing`, `TestCommandId`) são structs sem campos com as constantes associadas.
//! - **`TestExplorerViewMode` e `TestExplorerViewSorting`** são enums com o texto do original em `as_str`. O valor
//!   `Tree = 'true'` é o do original.
//! - **`testStateNames` e `testConfigurationGroupNames`** são [`test_state_name`] e [`test_configuration_group_name`].
//! - **`localize({ key, comment }, ...)`** é `localize(key, ...)`: o comentário não tem efeito sem pacote de idioma.

use je_base::icon_labels::strip_icons;
use je_base::nls::localize;

use super::test_types::{TestResultState, TestRunProfileBitset, test_run_profile_bitset};

/// `Testing`.
pub struct Testing;

impl Testing {
    // marked as "extension" so that any existing test extensions are assigned to it.
    pub const VIEWLET_ID: &'static str = "workbench.view.extension.test";
    pub const EXPLORER_VIEW_ID: &'static str = "workbench.view.testing";
    pub const OUTPUT_PEEK_CONTRIBUTION_ID: &'static str = "editor.contrib.testingOutputPeek";
    pub const DECORATIONS_CONTRIBUTION_ID: &'static str = "editor.contrib.testingDecorations";
    pub const COVERAGE_DECORATIONS_CONTRIBUTION_ID: &'static str = "editor.contrib.coverageDecorations";
    pub const COVERAGE_VIEW_ID: &'static str = "workbench.view.testCoverage";

    pub const RESULTS_PANEL_ID: &'static str = "workbench.panel.testResults";
    pub const RESULTS_VIEW_ID: &'static str = "workbench.panel.testResults.view";

    pub const MESSAGE_LANGUAGE_ID: &'static str = "vscodeInternalTestMessage";
}

/// `TestExplorerViewMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestExplorerViewMode {
    List,
    Tree,
}

impl TestExplorerViewMode {
    pub fn as_str(self) -> &'static str {
        match self {
            TestExplorerViewMode::List => "list",
            TestExplorerViewMode::Tree => "true",
        }
    }
}

/// `TestExplorerViewSorting`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestExplorerViewSorting {
    ByLocation,
    ByStatus,
    ByDuration,
}

impl TestExplorerViewSorting {
    pub fn as_str(self) -> &'static str {
        match self {
            TestExplorerViewSorting::ByLocation => "location",
            TestExplorerViewSorting::ByStatus => "status",
            TestExplorerViewSorting::ByDuration => "duration",
        }
    }
}

/// `testStateNames[state]`.
pub fn test_state_name(state: TestResultState) -> String {
    match state {
        TestResultState::Errored => localize("testState.errored", "Errored", &[]),
        TestResultState::Failed => localize("testState.failed", "Failed", &[]),
        TestResultState::Passed => localize("testState.passed", "Passed", &[]),
        TestResultState::Queued => localize("testState.queued", "Queued", &[]),
        TestResultState::Running => localize("testState.running", "Running", &[]),
        TestResultState::Skipped => localize("testState.skipped", "Skipped", &[]),
        TestResultState::Unset => localize("testState.unset", "Not yet run", &[]),
    }
}

/// `labelForTestInState`: label then the unit tests state, for example "Addition Tests (Running)".
pub fn label_for_test_in_state(label: &str, state: TestResultState) -> String {
    let stripped = strip_icons(&label.encode_utf16().collect::<Vec<u16>>()).to_string_lossy();
    localize("testing.treeElementLabel", "{0} ({1})", &[stripped.as_str(), test_state_name(state).as_str()])
}

/// `testConfigurationGroupNames[group]`.
pub fn test_configuration_group_name(group: TestRunProfileBitset) -> Option<String> {
    match group {
        test_run_profile_bitset::DEBUG => Some(localize("testGroup.debug", "Debug", &[])),
        test_run_profile_bitset::RUN => Some(localize("testGroup.run", "Run", &[])),
        test_run_profile_bitset::COVERAGE => Some(localize("testGroup.coverage", "Coverage", &[])),
        _ => None,
    }
}

/// `TestCommandId`.
pub struct TestCommandId;

impl TestCommandId {
    pub const CANCEL_TEST_REFRESH_ACTION: &'static str = "testing.cancelTestRefresh";
    pub const CANCEL_TEST_RUN_ACTION: &'static str = "testing.cancelRun";
    pub const CLEAR_TEST_RESULTS_ACTION: &'static str = "testing.clearTestResults";
    pub const COLLAPSE_ALL_ACTION: &'static str = "testing.collapseAll";
    pub const CONFIGURE_TEST_PROFILES_ACTION: &'static str = "testing.configureProfile";
    pub const CONTINOUS_RUN_USING_FOR_TEST: &'static str = "testing.continuousRunUsingForTest";
    pub const COVERAGE_AT_CURSOR: &'static str = "testing.coverageAtCursor";
    pub const COVERAGE_BY_URI: &'static str = "testing.coverage.uri";
    pub const COVERAGE_CLEAR: &'static str = "testing.coverage.close";
    pub const COVERAGE_CURRENT_FILE: &'static str = "testing.coverageCurrentFile";
    pub const COVERAGE_FILTER_TO_TEST: &'static str = "testing.coverageFilterToTest";
    pub const COVERAGE_FILTER_TO_TEST_IN_EDITOR: &'static str = "testing.coverageFilterToTestInEditor";
    pub const COVERAGE_GO_TO_NEXT_MISSED_LINE: &'static str = "testing.coverage.goToNextMissedLine";
    pub const COVERAGE_GO_TO_PREVIOUS_MISSED_LINE: &'static str = "testing.coverage.goToPreviousMissedLine";
    pub const COVERAGE_LAST_RUN: &'static str = "testing.coverageLastRun";
    pub const COVERAGE_SELECTED_ACTION: &'static str = "testing.coverageSelected";
    pub const COVERAGE_TOGGLE_IN_EXPLORER: &'static str = "testing.toggleCoverageInExplorer";
    pub const COVERAGE_TOGGLE_TOOLBAR: &'static str = "testing.coverageToggleToolbar";
    pub const COVERAGE_VIEW_CHANGE_SORTING: &'static str = "testing.coverageViewChangeSorting";
    pub const COVERAGE_VIEW_COLLAPSE_ALL: &'static str = "testing.coverageViewCollapseAll";
    pub const DEBUG_ACTION: &'static str = "testing.debug";
    pub const DEBUG_ALL_ACTION: &'static str = "testing.debugAll";
    pub const DEBUG_AT_CURSOR: &'static str = "testing.debugAtCursor";
    pub const DEBUG_BY_URI: &'static str = "testing.debug.uri";
    pub const DEBUG_CURRENT_FILE: &'static str = "testing.debugCurrentFile";
    pub const DEBUG_FAILED_TESTS: &'static str = "testing.debugFailTests";
    pub const DEBUG_FAILED_FROM_LAST_RUN: &'static str = "testing.debugFailedFromLastRun";
    pub const DEBUG_LAST_RUN: &'static str = "testing.debugLastRun";
    pub const DEBUG_SELECTED_ACTION: &'static str = "testing.debugSelected";
    pub const FILTER_ACTION: &'static str = "workbench.actions.treeView.testExplorer.filter";
    pub const GET_EXPLORER_SELECTION: &'static str = "_testing.getExplorerSelection";
    pub const GET_SELECTED_PROFILES: &'static str = "testing.getSelectedProfiles";
    pub const GO_TO_TEST: &'static str = "testing.editFocusedTest";
    pub const GO_TO_RELATED_TEST: &'static str = "testing.goToRelatedTest";
    pub const PEEK_RELATED_TEST: &'static str = "testing.peekRelatedTest";
    pub const GO_TO_RELATED_CODE: &'static str = "testing.goToRelatedCode";
    pub const PEEK_RELATED_CODE: &'static str = "testing.peekRelatedCode";
    pub const HIDE_TEST_ACTION: &'static str = "testing.hideTest";
    pub const OPEN_COVERAGE: &'static str = "testing.openCoverage";
    pub const OPEN_OUTPUT_PEEK: &'static str = "testing.openOutputPeek";
    pub const REFRESH_TESTS_ACTION: &'static str = "testing.refreshTests";
    pub const RE_RUN_FAILED_TESTS: &'static str = "testing.reRunFailTests";
    pub const RE_RUN_FAILED_FROM_LAST_RUN: &'static str = "testing.reRunFailedFromLastRun";
    pub const RE_RUN_LAST_RUN: &'static str = "testing.reRunLastRun";
    pub const RUN_ACTION: &'static str = "testing.run";
    pub const RUN_ALL_ACTION: &'static str = "testing.runAll";
    pub const RUN_ALL_WITH_COVERAGE_ACTION: &'static str = "testing.coverageAll";
    pub const RUN_AT_CURSOR: &'static str = "testing.runAtCursor";
    pub const RUN_BY_URI: &'static str = "testing.run.uri";
    pub const RUN_CURRENT_FILE: &'static str = "testing.runCurrentFile";
    pub const RUN_SELECTED_ACTION: &'static str = "testing.runSelected";
    pub const RUN_USING_PROFILE_ACTION: &'static str = "testing.runUsing";
    pub const RUN_WITH_COVERAGE_ACTION: &'static str = "testing.coverage";
    pub const SEARCH_FOR_TEST_EXTENSION: &'static str = "testing.searchForTestExtension";
    pub const SELECT_DEFAULT_TEST_PROFILES: &'static str = "testing.selectDefaultTestProfiles";
    pub const SHOW_MOST_RECENT_OUTPUT_ACTION: &'static str = "testing.showMostRecentOutput";
    pub const START_CONTINOUS_RUN: &'static str = "testing.startContinuousRun";
    pub const START_CONTINOUS_RUN_FROM_EXTENSION: &'static str = "testing.startContinuousRunFromExtension";
    pub const STOP_CONTINOUS_RUN_FROM_EXTENSION: &'static str = "testing.stopContinuousRunFromExtension";
    pub const STOP_CONTINOUS_RUN: &'static str = "testing.stopContinuousRun";
    pub const TESTING_SORT_BY_DURATION_ACTION: &'static str = "testing.sortByDuration";
    pub const TESTING_SORT_BY_LOCATION_ACTION: &'static str = "testing.sortByLocation";
    pub const TESTING_SORT_BY_STATUS_ACTION: &'static str = "testing.sortByStatus";
    pub const TESTING_VIEW_AS_LIST_ACTION: &'static str = "testing.viewAsList";
    pub const TESTING_VIEW_AS_TREE_ACTION: &'static str = "testing.viewAsTree";
    pub const TOGGLE_CONTINOUS_RUN_FOR_TEST: &'static str = "testing.toggleContinuousRunForTest";
    pub const TOGGLE_RESULTS_VIEW_LAYOUT_ACTION: &'static str = "testing.toggleResultsViewLayout";
    pub const TOGGLE_INLINE_TEST_OUTPUT: &'static str = "testing.toggleInlineTestOutput";
    pub const UNHIDE_ALL_TESTS_ACTION: &'static str = "testing.unhideAllTests";
    pub const UNHIDE_TEST_ACTION: &'static str = "testing.unhideTest";
}
