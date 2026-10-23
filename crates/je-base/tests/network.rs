//! Porte de `src/vs/base/test/common/network.test.ts` (suíte `network`), caso a caso e na mesma ordem.
//!
//! O original roda no Node (`isNative` verdadeiro, `isWeb` falso) com `_VSCODE_FILE_ROOT` apontando para
//! a pasta de saída do build. Aqui o contexto do `FileAccess` é injetado com esses valores e uma raiz
//! fixa, e o `isEqual` do `resources` é o do `ExtUri` não enviesado. Como `isWeb` é falso, os casos
//! `(isWeb ? test.skip : test)` rodam todos.

use std::rc::Rc;

use je_base::network::{schemas, FileAccess, FileAccessContext, RemoteAuthorities};
use je_base::path::{FixedEnvironment, Platform};
use je_base::resources::ExtUri;
use je_base::uri::{Field, UriChange, URI};

fn env() -> Rc<FixedEnvironment> {
    Rc::new(FixedEnvironment::new(Platform::Posix, "/workspace/project"))
}

fn file_access() -> FileAccess {
    FileAccess::new(
        env(),
        FileAccessContext {
            is_native: true,
            web_worker_origin: None,
            file_root: Some("file:///workspace/vscode/out/".to_string()),
        },
        RemoteAuthorities::new(false),
    )
}

fn file(path: &str) -> URI {
    URI::file(path, Platform::Posix)
}

fn with(uri: &URI, change: UriChange) -> URI {
    uri.with(&change)
}

#[test]
fn file_access_uri_native() {
    let access = file_access();
    let ext_uri = ExtUri::unbiased(env());

    // asCodeUri() & asFileUri(): simple, without authority
    let mut original_file_uri = file("network.test.ts");
    let mut browser_uri = access.uri_to_browser_uri(&original_file_uri);
    assert!(!browser_uri.authority().is_empty());
    let mut file_uri = access.uri_to_file_uri(&browser_uri);
    assert_eq!(file_uri.authority().len(), 0);
    assert!(ext_uri.is_equal(Some(&original_file_uri), Some(&file_uri), false));

    // asCodeUri() & asFileUri(): with authority
    original_file_uri = with(&file("network.test.ts"), UriChange { authority: "test-authority".into(), ..UriChange::default() });
    browser_uri = access.uri_to_browser_uri(&original_file_uri);
    assert_eq!(browser_uri.authority(), original_file_uri.authority());
    file_uri = access.uri_to_file_uri(&browser_uri);
    assert!(ext_uri.is_equal(Some(&original_file_uri), Some(&file_uri), false));
}

#[test]
fn file_access_module_id_native() {
    let access = file_access();
    let browser_uri = access.as_browser_uri("vs/base/test/node/network.test");
    assert_eq!(browser_uri.scheme(), schemas::VSCODE_FILE_RESOURCE);

    let file_uri = access.as_file_uri("vs/base/test/node/network.test");
    assert_eq!(file_uri.scheme(), schemas::FILE);
}

#[test]
fn file_access_query_and_fragment_is_dropped_native() {
    let access = file_access();
    let original_file_uri =
        with(&file("network.test.ts"), UriChange { query: "foo=bar".into(), fragment: "something".into(), ..UriChange::default() });
    let browser_uri = access.uri_to_browser_uri(&original_file_uri);
    assert_eq!(browser_uri.query(), "");
    assert_eq!(browser_uri.fragment(), "");
}

#[test]
fn file_access_query_and_fragment_is_kept_if_uri_is_already_of_same_scheme_native() {
    let access = file_access();
    let original_file_uri =
        with(&file("network.test.ts"), UriChange { query: "foo=bar".into(), fragment: "something".into(), ..UriChange::default() });
    let browser_uri = access.uri_to_browser_uri(&with(
        &original_file_uri,
        UriChange { scheme: Field::from(schemas::VSCODE_FILE_RESOURCE), ..UriChange::default() },
    ));
    assert_eq!(browser_uri.query(), "foo=bar");
    assert_eq!(browser_uri.fragment(), "something");

    let file_uri = access.uri_to_file_uri(&original_file_uri);
    assert_eq!(file_uri.query(), "foo=bar");
    assert_eq!(file_uri.fragment(), "something");
}

#[test]
fn file_access_web() {
    let access = file_access();
    let original_https_uri = with(&file("network.test.ts"), UriChange { scheme: "https".into(), ..UriChange::default() });
    let browser_uri = access.uri_to_browser_uri(&original_https_uri);
    assert_eq!(original_https_uri.to_string(false), browser_uri.to_string(false));
}

#[test]
fn file_access_remote_uris() {
    let access = file_access();
    let original_remote_uri =
        with(&file("network.test.ts"), UriChange { scheme: Field::from(schemas::VSCODE_REMOTE), ..UriChange::default() });
    let browser_uri = access.uri_to_browser_uri(&original_remote_uri);
    assert_ne!(original_remote_uri.scheme(), browser_uri.scheme());
}
