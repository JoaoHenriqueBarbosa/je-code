//! Porte de `src/vs/workbench/contrib/preferences/test/browser/settingsEditor2.test.ts` (suíte `SettingsEditor2`), caso a caso e
//! na mesma ordem.
//!
//! Casos de fora (a suíte aninhada `Settings refresh`, 18 casos entre os `for` expandidos: `resolved sandbox policies refresh
//! policy-filtered results and removal without configuration changes`, `outbound policy changes refresh managed results
//! independently of sandbox enablement`, `{key} policy changes refresh managed results and unlock when not restricted` (5),
//! `configuration changes do not rebuild assignment-filtered results`, `assignment changes refresh counts, empty state and
//! ARIA when no setting is focused`, `defers assignment refresh for {test.assigned,test.unassigned} until the focused
//! setting loses focus`, `defers assignment refresh while a context menu is focused`, `assignment changes do not rebuild when
//! the assignment filter is inactive`, `batches sequential assignment resolutions into one editor rebuild`, `advanced
//! assigned settings survive editor filtering while unassigned settings do not`, `assignment filter includes advanced
//! settings in the resolved TOC and category filtering`, `advanced assignment exemption applies to text search and preserves
//! policy filtering`): todos montam um `SettingsEditor2` real (`EditorPane` de DOM, com `SplitView`, `SettingsTree`, `TOCTree`,
//! `SuggestEnabledInputWithHistory`, foco e `blur` de elementos), além do `ExperimentalSettingsService`, do
//! `ManagedSettingsPresentationService` e do `IManagedSettingsService` com as chaves de sandbox, que o porte não tem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda no começo de cada caso, com o store dele no lugar do `store`.
//! - **`isSettingsSearchUpToDate(..., undefined, '')`.** O `renderedSearchQuery` indefinido é `None`.
//! - **`assert.rejects(delayer.trigger(() => { }))`.** A promessa de `trigger` é esperada com o `await` depois do `cancel`,
//!   e a rejeição tem de ser um erro de cancelamento.

use je_base::async_::{Delayer, MaybePromise};
use je_base::errors;
use je_base::symbols::Delay;
use je_workbench::contrib::preferences::settings_editor2::is_settings_search_up_to_date;

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};

#[test]
fn allows_focus_when_search_is_idle_and_query_matches_rendered_results() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(is_settings_search_up_to_date(false, Some("font"), "font"));
    assert!(is_settings_search_up_to_date(false, Some(""), ""));
}

#[test]
fn trims_the_current_search_value_before_comparing() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(is_settings_search_up_to_date(false, Some("font"), "  font  "));
    assert!(!is_settings_search_up_to_date(false, Some("font"), " font size "));
}

#[test]
fn blocks_focus_while_a_debounced_search_is_pending() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!is_settings_search_up_to_date(true, Some("font"), "font"));
    assert!(!is_settings_search_up_to_date(true, Some(""), ""));
}

#[test]
fn blocks_focus_when_rendered_results_are_stale() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!is_settings_search_up_to_date(false, Some("font"), "theme"));
    assert!(!is_settings_search_up_to_date(false, Some("font"), ""));
    assert!(!is_settings_search_up_to_date(false, None, ""));
}

#[test]
fn uses_delayer_is_triggered_return_value_regression_327360() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let delayer = store.add_rc(std::rc::Rc::new(Delayer::<()>::new(Delay::Millis(1000))));

        assert!(!delayer.is_triggered());
        assert!(is_settings_search_up_to_date(delayer.is_triggered(), Some("font"), "font"));

        let pending = delayer.trigger(|| MaybePromise::Value(()), None);
        assert!(delayer.is_triggered());
        assert!(!is_settings_search_up_to_date(delayer.is_triggered(), Some("font"), "font"));

        delayer.cancel();
        let outcome = pending.await;
        assert!(outcome.as_ref().is_err_and(errors::is_cancellation_error), "assert.rejects: o trigger cancelado rejeita");
        assert!(!delayer.is_triggered());
        assert!(is_settings_search_up_to_date(delayer.is_triggered(), Some("font"), "font"));
    });
}
