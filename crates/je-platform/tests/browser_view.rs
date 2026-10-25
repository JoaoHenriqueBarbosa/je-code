//! Porte de `src/vs/platform/browserView/test/common/browserView.test.ts` (suíte `BrowserView`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: `assert.deepStrictEqual` é `assert_eq!`. `URI.file(path)` é `uri_file` e `.with({ query })` é
//! `URI::with` com `UriChange`. Os objetos `{ type: 'agent', sessionId }` são as variantes de `BrowserViewOwner` e
//! `BrowserViewAudience` (`sessionId` omitido é `None`). `Object.fromEntries(Object.values(BrowserViewStorageScope).map(...))`
//! é a lista de pares `(valor de texto, resultado)` percorrida na ordem de declaração do `enum`. A guarda de vazamentos é a
//! mesma do `ensureNoDisposablesAreLeakedInTestSuite`.

mod common;

use common::{ensure_no_disposables_are_leaked_in_test_suite, uri_file};
use je_base::uri::{Field, UriChange};
use je_platform::browser_view::browser_view::{
    BrowserViewAudience, BrowserViewOwner, BrowserViewSessionOptions, BrowserViewSessionSelector, BrowserViewStorageScope, get_agent_browser_view_creation_defaults,
    is_browser_view_associated_resource_navigation, is_browser_view_storage_scope_shareable_with_agent, is_in_memory_storage_scope, matches_browser_view_audience,
    AgentBrowserViewCreationDefaults,
};

const ALL_SCOPES: [BrowserViewStorageScope; 4] =
    [BrowserViewStorageScope::Global, BrowserViewStorageScope::Workspace, BrowserViewStorageScope::Ephemeral, BrowserViewStorageScope::Agent];

fn agent(session_id: Option<&str>) -> BrowserViewAudience {
    BrowserViewAudience::Agent { session_id: session_id.map(str::to_owned) }
}

#[test]
fn allows_navigation_within_an_associated_resource() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let associated_resource = uri_file("/workspace/index.html");
    let with_query = associated_resource.with(&UriChange { query: Field::from("theme=dark"), ..UriChange::default() });
    let with_fragment = associated_resource.with(&UriChange { fragment: Field::from("section"), ..UriChange::default() });

    assert_eq!(
        (
            is_browser_view_associated_resource_navigation(&associated_resource, &associated_resource.to_string(false).to_string_lossy()),
            is_browser_view_associated_resource_navigation(&associated_resource, &with_query.to_string(false).to_string_lossy()),
            is_browser_view_associated_resource_navigation(&associated_resource, &with_fragment.to_string(false).to_string_lossy()),
            is_browser_view_associated_resource_navigation(&associated_resource, &uri_file("/workspace/other.html").to_string(false).to_string_lossy()),
            is_browser_view_associated_resource_navigation(&associated_resource, "https://example.com/"),
        ),
        (true, true, true, false, false),
    );
}

#[test]
fn matches_audiences_against_patterns() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let candidate = agent(Some("session"));

    assert_eq!(
        (
            matches_browser_view_audience(&candidate, &agent(None)),
            matches_browser_view_audience(&candidate, &agent(Some("session"))),
            matches_browser_view_audience(&candidate, &agent(Some("other"))),
        ),
        (true, true, false),
    );
}

#[test]
fn matches_audiences_for_filtered_removal() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        (
            matches_browser_view_audience(&agent(Some("session")), &agent(None)),
            matches_browser_view_audience(&agent(Some("session")), &agent(Some("session"))),
            matches_browser_view_audience(&agent(Some("session")), &agent(Some("other"))),
        ),
        (true, true, false),
    );
}

#[test]
fn configures_agent_storage_affinity_independently_from_ownership() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        (
            get_agent_browser_view_creation_defaults("chat-session", None),
            get_agent_browser_view_creation_defaults("chat-session", Some("chat-session")),
        ),
        (
            AgentBrowserViewCreationDefaults {
                owner: BrowserViewOwner::Agent { session_id: "chat-session".to_owned() },
                initial_audiences: vec![agent(None)],
                session: BrowserViewSessionSelector::Options(BrowserViewSessionOptions::Agent { affinity: None }),
            },
            AgentBrowserViewCreationDefaults {
                owner: BrowserViewOwner::Agent { session_id: "chat-session".to_owned() },
                initial_audiences: vec![agent(None)],
                session: BrowserViewSessionSelector::Options(BrowserViewSessionOptions::Agent { affinity: Some("chat-session".to_owned()) }),
            },
        ),
    );
}

#[test]
fn identifies_in_memory_storage_scopes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        (
            is_in_memory_storage_scope(BrowserViewStorageScope::Global),
            is_in_memory_storage_scope(BrowserViewStorageScope::Workspace),
            is_in_memory_storage_scope(BrowserViewStorageScope::Ephemeral),
            is_in_memory_storage_scope(BrowserViewStorageScope::Agent),
        ),
        (false, false, true, true),
    );
}

#[test]
fn only_shares_agent_storage_when_network_filtering_is_enabled() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let by_scope = |network_filtering_enabled: bool| -> Vec<(&'static str, bool)> {
        ALL_SCOPES.iter().map(|&scope| (scope.as_str(), is_browser_view_storage_scope_shareable_with_agent(scope, network_filtering_enabled))).collect()
    };

    assert_eq!(
        (by_scope(false), by_scope(true)),
        (
            vec![("global", true), ("workspace", true), ("ephemeral", true), ("agent", true)],
            vec![("global", false), ("workspace", false), ("ephemeral", false), ("agent", true)],
        ),
    );
}
