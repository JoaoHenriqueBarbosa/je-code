//! Porte de `src/vs/workbench/contrib/testing/common/testingContextKeys.ts`: as chaves de contexto do contrib testing.
//!
//! Divergências:
//! - **`namespace TestingContextKeys`** é este módulo: `TestingContextKeys.isRunning` é `testing_context_keys::is_running()`.
//!   Cada chave é uma função que devolve o `RawContextKey` (o mesmo padrão de `tasks.rs`), criado uma vez por thread.
//! - **`RawContextKey<T>`** não é genérico no porte; o tipo do valor é o do `Dynamic` do valor padrão.
//! - **`{ type, description }`** é `ContextKeyMeta::Info`.
//! - **`capabilityToContextKey`** é [`capability_to_context_key`].

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_platform::contextkey::{ContextKeyMeta, RawContextKey};

use super::constants::{TestExplorerViewMode, TestExplorerViewSorting};
use super::test_types::{TestRunProfileBitset, test_run_profile_bitset};

fn boolean(description: String) -> ContextKeyMeta {
    ContextKeyMeta::Info { ty: "boolean".to_string(), description }
}

fn string(description: String) -> ContextKeyMeta {
    ContextKeyMeta::Info { ty: "string".to_string(), description }
}

macro_rules! context_keys {
    ($($name:ident, $slot:ident: ($key:expr, $default:expr, $meta:expr);)*) => {
        thread_local! {
            $(static $slot: RawContextKey = RawContextKey::new($key, $default, $meta);)*
        }

        $(
            pub fn $name() -> RawContextKey {
                $slot.with(Clone::clone)
            }
        )*
    };
}

context_keys! {
    provider_count, PROVIDER_COUNT: ("testing.providerCount", 0, ContextKeyMeta::None);
    can_refresh_tests, CAN_REFRESH_TESTS: ("testing.canRefresh", false, boolean(localize("testing.canRefresh", "Indicates whether any test controller has an attached refresh handler.", &[])));
    is_refreshing_tests, IS_REFRESHING_TESTS: ("testing.isRefreshing", false, boolean(localize("testing.isRefreshing", "Indicates whether any test controller is currently refreshing tests.", &[])));
    is_continuous_mode_on, IS_CONTINUOUS_MODE_ON: ("testing.isContinuousModeOn", false, boolean(localize("testing.isContinuousModeOn", "Indicates whether continuous test mode is on.", &[])));
    has_debuggable_tests, HAS_DEBUGGABLE_TESTS: ("testing.hasDebuggableTests", false, boolean(localize("testing.hasDebuggableTests", "Indicates whether any test controller has registered a debug configuration", &[])));
    has_runnable_tests, HAS_RUNNABLE_TESTS: ("testing.hasRunnableTests", false, boolean(localize("testing.hasRunnableTests", "Indicates whether any test controller has registered a run configuration", &[])));
    has_coverable_tests, HAS_COVERABLE_TESTS: ("testing.hasCoverableTests", false, boolean(localize("testing.hasCoverableTests", "Indicates whether any test controller has registered a coverage configuration", &[])));
    has_non_default_profile, HAS_NON_DEFAULT_PROFILE: ("testing.hasNonDefaultProfile", false, boolean(localize("testing.hasNonDefaultConfig", "Indicates whether any test controller has registered a non-default configuration", &[])));
    has_configurable_profile, HAS_CONFIGURABLE_PROFILE: ("testing.hasConfigurableProfile", false, boolean(localize("testing.hasConfigurableConfig", "Indicates whether any test configuration can be configured", &[])));
    supports_continuous_run, SUPPORTS_CONTINUOUS_RUN: ("testing.supportsContinuousRun", false, boolean(localize("testing.supportsContinuousRun", "Indicates whether continuous test running is supported", &[])));
    is_parent_running_continuously, IS_PARENT_RUNNING_CONTINUOUSLY: ("testing.isParentRunningContinuously", false, boolean(localize("testing.isParentRunningContinuously", "Indicates whether the parent of a test is continuously running, set in the menu context of test items", &[])));
    active_editor_has_tests, ACTIVE_EDITOR_HAS_TESTS: ("testing.activeEditorHasTests", false, boolean(localize("testing.activeEditorHasTests", "Indicates whether any tests are present in the current editor", &[])));
    cursor_inside_test_range, CURSOR_INSIDE_TEST_RANGE: ("testing.cursorInsideTestRange", false, boolean(localize("testing.cursorInsideTestRange", "Whether the cursor is currently inside a test range", &[])));
    is_test_coverage_open, IS_TEST_COVERAGE_OPEN: ("testing.isTestCoverageOpen", false, boolean(localize("testing.isTestCoverageOpen", "Indicates whether a test coverage report is open", &[])));
    has_coverage_in_file, HAS_COVERAGE_IN_FILE: ("testing.hasCoverageInFile", false, boolean(localize("testing.hasCoverageInFile", "Indicates coverage has been reported in the curent editor.", &[])));
    has_per_test_coverage, HAS_PER_TEST_COVERAGE: ("testing.hasPerTestCoverage", false, boolean(localize("testing.hasPerTestCoverage", "Indicates whether per-test coverage is available", &[])));
    has_inline_coverage_details, HAS_INLINE_COVERAGE_DETAILS: ("testing.hasInlineCoverageDetails", false, boolean(localize("testing.hasInlineCoverageDetails", "Indicates whether detailed per-line coverage is available for inline display", &[])));
    is_coverage_filtered_to_test, IS_COVERAGE_FILTERED_TO_TEST: ("testing.isCoverageFilteredToTest", false, boolean(localize("testing.isCoverageFilteredToTest", "Indicates whether coverage has been filterd to a single test", &[])));
    coverage_toolbar_enabled, COVERAGE_TOOLBAR_ENABLED: ("testing.coverageToolbarEnabled", true, boolean(localize("testing.coverageToolbarEnabled", "Indicates whether the coverage toolbar is enabled", &[])));
    inline_coverage_enabled, INLINE_COVERAGE_ENABLED: ("testing.inlineCoverageEnabled", false, boolean(localize("testing.inlineCoverageEnabled", "Indicates whether inline coverage is shown", &[])));
    can_go_to_related_code, CAN_GO_TO_RELATED_CODE: ("testing.canGoToRelatedCode", false, boolean(localize("testing.canGoToRelatedCode", "Whether a controller implements a capability to find code related to a test", &[])));
    can_go_to_related_test, CAN_GO_TO_RELATED_TEST: ("testing.canGoToRelatedTest", false, boolean(localize("testing.canGoToRelatedTest", "Whether a controller implements a capability to find tests related to code", &[])));
    peek_has_stack, PEEK_HAS_STACK: ("testing.peekHasStack", false, boolean(localize("testing.peekHasStack", "Whether the message shown in a peek view has a stack trace", &[])));

    has_any_results, HAS_ANY_RESULTS: ("testing.hasAnyResults", false, ContextKeyMeta::None);
    view_mode, VIEW_MODE: ("testing.explorerViewMode", TestExplorerViewMode::List.as_str(), ContextKeyMeta::None);
    view_sorting, VIEW_SORTING: ("testing.explorerViewSorting", TestExplorerViewSorting::ByLocation.as_str(), ContextKeyMeta::None);
    is_running, IS_RUNNING: ("testing.isRunning", false, ContextKeyMeta::None);
    is_in_peek, IS_IN_PEEK: ("testing.isInPeek", false, ContextKeyMeta::None);
    is_peek_visible, IS_PEEK_VISIBLE: ("testing.isPeekVisible", false, ContextKeyMeta::None);

    peek_item_type, PEEK_ITEM_TYPE: ("peekItemType", Dynamic::Undefined, string(localize("testing.peekItemType", "Type of the item in the output peek view. Either a \"test\", \"message\", \"task\", or \"result\".", &[])));
    controller_id, CONTROLLER_ID: ("controllerId", Dynamic::Undefined, string(localize("testing.controllerId", "Controller ID of the current test item", &[])));
    test_item_ext_id, TEST_ITEM_EXT_ID: ("testId", Dynamic::Undefined, string(localize("testing.testId", "ID of the current test item, set when creating or opening menus on test items", &[])));
    test_item_has_uri, TEST_ITEM_HAS_URI: ("testing.testItemHasUri", false, boolean(localize("testing.testItemHasUri", "Boolean indicating whether the test item has a URI defined", &[])));
    test_item_is_hidden, TEST_ITEM_IS_HIDDEN: ("testing.testItemIsHidden", false, boolean(localize("testing.testItemIsHidden", "Boolean indicating whether the test item is hidden", &[])));
    test_message_context, TEST_MESSAGE_CONTEXT: ("testMessage", Dynamic::Undefined, string(localize("testing.testMessage", "Value set in `testMessage.contextValue`, available in editor/content and testing/message/context", &[])));
    test_result_outdated, TEST_RESULT_OUTDATED: ("testResultOutdated", Dynamic::Undefined, boolean(localize("testing.testResultOutdated", "Value available in editor/content and testing/message/context when the result is outdated", &[])));
    test_result_state, TEST_RESULT_STATE: ("testResultState", Dynamic::Undefined, string(localize("testing.testResultState", "Value available testing/item/result indicating the state of the item.", &[])));
    test_profile_context_group, TEST_PROFILE_CONTEXT_GROUP: ("testing.profile.context.group", Dynamic::Undefined, string(localize("testing.profile.context.group", "Type of menu where the configure testing profile submenu exists. Either \"run\", \"debug\", or \"coverage\"", &[])));
}

/// `capabilityToContextKey[bitset]`.
pub fn capability_to_context_key(bitset: TestRunProfileBitset) -> Option<RawContextKey> {
    match bitset {
        test_run_profile_bitset::RUN => Some(has_runnable_tests()),
        test_run_profile_bitset::COVERAGE => Some(has_coverable_tests()),
        test_run_profile_bitset::DEBUG => Some(has_debuggable_tests()),
        test_run_profile_bitset::HAS_NON_DEFAULT_PROFILE => Some(has_non_default_profile()),
        test_run_profile_bitset::HAS_CONFIGURABLE => Some(has_configurable_profile()),
        test_run_profile_bitset::SUPPORTS_CONTINUOUS_RUN => Some(supports_continuous_run()),
        _ => None,
    }
}
