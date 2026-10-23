//! Porte de `src/vs/platform/browserView/test/common/browserViewGroup.test.ts` (suíte
//! `BrowserViewGroup`), caso a caso e na mesma ordem.
//!
//! Divergências: o `deepStrictEqual` do segundo caso sobre o objeto de seis chaves vira um
//! `assert_eq!` sobre a tupla dos seis resultados, na mesma ordem das chaves; `{ type: 'user' }` e
//! `{ type: 'agent', sessionId }` são as variantes de [`BrowserViewOwner`], e `{ type: 'agent' }` sem
//! `sessionId` é `Agent { session_id: None }`. `ensureNoDisposablesAreLeakedInTestSuite` é o
//! [`LeakGuard`] de cada teste.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::browser_view::browser_view::{BrowserViewAudience, BrowserViewOwner};
use je_platform::browser_view::browser_view_group::{BrowserViewGroupFilter, matches_browser_view_group_filter, matches_browser_view_sandbox_session};

fn agent(session_id: &str) -> BrowserViewOwner {
    BrowserViewOwner::Agent { session_id: session_id.to_owned() }
}

fn audience(session_id: Option<&str>) -> BrowserViewAudience {
    BrowserViewAudience::Agent { session_id: session_id.map(str::to_owned) }
}

fn ids(items: &[&str]) -> Option<Vec<String>> {
    Some(items.iter().map(|item| (*item).to_owned()).collect())
}

#[test]
fn sandboxed_visibility_excludes_user_pages_other_agents_and_non_isolated_storage() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            matches_browser_view_sandbox_session(Some(&BrowserViewOwner::User), None, None),
            matches_browser_view_sandbox_session(Some(&BrowserViewOwner::User), Some("session"), Some("session")),
            matches_browser_view_sandbox_session(Some(&agent("other")), Some("session"), Some("session")),
            matches_browser_view_sandbox_session(Some(&agent("session")), None, Some("session")),
            matches_browser_view_sandbox_session(Some(&agent("session")), Some("other"), Some("session")),
            matches_browser_view_sandbox_session(Some(&agent("session")), Some("session"), Some("session")),
        ],
        [true, false, false, false, false, true]
    );
}

#[test]
fn matches_browser_ids_and_audiences() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let session_audience = [audience(Some("session"))];
    let all_agents_audience = [audience(None)];

    let browser_id = matches_browser_view_group_filter("browser", &session_audience, &BrowserViewGroupFilter { browser_ids: ids(&["browser", "other"]), ..Default::default() });
    let other_browser_id = matches_browser_view_group_filter("browser", &session_audience, &BrowserViewGroupFilter { browser_ids: ids(&["other"]), ..Default::default() });
    let session = matches_browser_view_group_filter("browser", &session_audience, &BrowserViewGroupFilter { audience: Some(audience(Some("session"))), ..Default::default() });
    let other_session = matches_browser_view_group_filter("browser", &session_audience, &BrowserViewGroupFilter { audience: Some(audience(Some("other"))), ..Default::default() });
    let all_agents = matches_browser_view_group_filter("browser", &all_agents_audience, &BrowserViewGroupFilter { audience: Some(audience(Some("session"))), ..Default::default() });
    let either = matches_browser_view_group_filter(
        "browser",
        &session_audience,
        &BrowserViewGroupFilter { browser_ids: ids(&["browser"]), audience: Some(audience(Some("other"))), ..Default::default() },
    );

    // browserId, otherBrowserId, sessionAudience, otherSessionAudience, allAgentsAudience, either
    assert_eq!((browser_id, other_browser_id, session, other_session, all_agents, either), (true, false, true, false, true, true));
}
