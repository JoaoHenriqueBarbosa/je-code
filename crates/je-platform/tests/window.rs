//! Porte de `src/vs/platform/window/test/common/window.test.ts` (suíte `window`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: `assert.deepStrictEqual` é `assert_eq!` sobre tuplas e `Option`; `null` é `None` e `undefined`
//! é `None` (o original compara `undefined` com `undefined`). `[...].map(...)` é um `Vec` coletado. O `URLSearchParams`
//! é o `UrlSearchParams` de `je_base::oauth` e o `URI.from` recebe os `UriComponents`. O `parsed && {...}` do original
//! é um `Option::map`. A guarda de vazamentos é a mesma do `ensureNoDisposablesAreLeakedInTestSuite`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::oauth::UrlSearchParams;
use je_base::uri::{URI, UriComponents};
use je_platform::window::{AgentsWindowDraft, AgentsWindowNewSessionLink, IPoint, get_macos_window_controls_position, parse_external_agents_window_new_session_link_uri};

fn point(x: f64, y: f64) -> Option<IPoint> {
    Some(IPoint { x, y })
}

fn positions(heights: &[f64], os_version: &str, horizontal_inset: Option<f64>) -> Vec<Option<IPoint>> {
    heights.iter().map(|&height| get_macos_window_controls_position(height, os_version, horizontal_inset)).collect()
}

#[test]
fn preserves_legacy_macos_traffic_light_positioning_without_a_fixed_inset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        (positions(&[35.0, 44.0, 16.0], "24.0.0", None), positions(&[35.0, 44.0, 14.0], "25.0.0", None)),
        (vec![point(10.0, 9.0), point(15.0, 14.0), None], vec![point(11.0, 10.0), point(16.0, 15.0), None]),
    );
}

#[test]
fn keeps_macos_traffic_lights_horizontally_fixed_while_centering_different_heights() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        (
            positions(&[35.0, 44.0, 35.0], "24.0.0", Some(10.0)),
            positions(&[35.0, 44.0, 35.0], "25.0.0", Some(11.0)),
            positions(&[44.0, 55.0, 44.0], "24.0.0", Some(15.0)),
            positions(&[44.0, 55.0, 44.0], "25.0.0", Some(16.0)),
        ),
        (
            vec![point(10.0, 9.0), point(10.0, 14.0), point(10.0, 9.0)],
            vec![point(11.0, 10.0), point(11.0, 15.0), point(11.0, 10.0)],
            vec![point(15.0, 14.0), point(15.0, 19.0), point(15.0, 14.0)],
            vec![point(16.0, 15.0), point(16.0, 20.0), point(16.0, 15.0)],
        ),
    );
}

#[test]
fn parses_an_external_agents_window_new_session_link() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut query = UrlSearchParams::new();
    query.append("workspace", "file:///Users/example/project");
    query.append("prompt", "Fix the tests & explain why");
    let link = URI::from(
        &UriComponents {
            scheme: Some("vscode-insiders".into()),
            authority: Some("agents".into()),
            path: Some("/new".into()),
            query: Some(query.to_query_string().as_str().into()),
            fragment: None,
        },
        false,
    );

    let parsed = parse_external_agents_window_new_session_link_uri(&link, "vscode-insiders");

    assert_eq!(
        parsed.map(|parsed| (parsed.workspace_uri.map(|uri| uri.to_string(false).to_string_lossy()), parsed.draft)),
        Some((
            Some("file:///Users/example/project".to_owned()),
            AgentsWindowDraft { input_text: "Fix the tests & explain why".to_owned(), attachments: "[]".to_owned() },
        )),
    );
}

#[test]
fn parses_an_external_agents_window_new_session_link_without_a_workspace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let parsed = parse_external_agents_window_new_session_link_uri("vscode-insiders://agents/new?prompt=Draft%20without%20a%20workspace", "vscode-insiders");

    assert_eq!(
        parsed,
        Some(AgentsWindowNewSessionLink {
            workspace_uri: None,
            draft: AgentsWindowDraft { input_text: "Draft without a workspace".to_owned(), attachments: "[]".to_owned() },
        }),
    );
}

#[test]
fn rejects_invalid_external_agents_window_new_session_links() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let results = [
        parse_external_agents_window_new_session_link_uri("vscode://agents/new?workspace=file%3A%2F%2F%2Fproject&prompt=Fix", "vscode-insiders"),
        parse_external_agents_window_new_session_link_uri("vscode-insiders://extensions/new?workspace=file%3A%2F%2F%2Fproject&prompt=Fix", "vscode-insiders"),
        parse_external_agents_window_new_session_link_uri("vscode-insiders://agents/session?workspace=file%3A%2F%2F%2Fproject&prompt=Fix", "vscode-insiders"),
        parse_external_agents_window_new_session_link_uri("vscode-insiders://agents/new?workspace=file%3A%2F%2F%2Fproject", "vscode-insiders"),
        parse_external_agents_window_new_session_link_uri("vscode-insiders://agents/new?workspace=&prompt=Fix", "vscode-insiders"),
        parse_external_agents_window_new_session_link_uri("vscode-insiders://agents/new?workspace=project&prompt=Fix", "vscode-insiders"),
    ];
    assert_eq!(results, [None, None, None, None, None, None]);
}
