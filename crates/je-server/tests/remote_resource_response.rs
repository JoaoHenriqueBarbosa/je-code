//! Porte de `src/vs/server/test/node/remoteResourceResponse.test.ts` (suíte `Remote resource response`), caso a caso e na
//! mesma ordem.
//!
//! Nota de tradução: o objeto `{ nativeWorkbench, webEndpoint, webview, untrusted, vary }` com valores `undefined` é uma
//! tupla de `Option<String>` na mesma ordem.

use je_server::remote_resource_response::get_remote_resource_response_headers;

#[test]
fn allows_trusted_workbench_and_configured_web_endpoint_origins() {
    let allowed_web_endpoint_origin = "https://example.vscode-cdn.net";
    let get_headers = |origin: Option<&str>| get_remote_resource_response_headers(origin, |candidate| candidate == allowed_web_endpoint_origin);
    let allow_origin = |origin: Option<&str>| get_headers(origin).get("Access-Control-Allow-Origin").cloned();

    assert_eq!(
        (
            allow_origin(Some("vscode-file://vscode-app")),
            allow_origin(Some(allowed_web_endpoint_origin)),
            allow_origin(Some("vscode-webview://01234567-89ab-cdef-0123-456789abcdef")),
            allow_origin(Some("https://example.com")),
            get_headers(None).get("Vary").cloned(),
        ),
        (
            Some("vscode-file://vscode-app".to_owned()),
            Some(allowed_web_endpoint_origin.to_owned()),
            None,
            None,
            Some("Origin".to_owned()),
        )
    );
}
