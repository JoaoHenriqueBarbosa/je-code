//! Porte de `src/vs/workbench/contrib/debug/test/browser/debugSource.test.ts` (suíte `Debug - Source`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `mockUriIdentityService` (`mockDebugModel.ts`) é um `UriIdentityService` sobre o `TestFileService`, criado no
//!   escopo do módulo no original, fora do rastreio de vazamentos: aqui o [`mock_uri_identity_service`] é montado antes do
//!   `LeakGuard` de cada caso (e solto depois dele), então nenhum dos seus descartáveis entra no rastreio.
//! - `uri.file(path)` e `isWindows` são a plataforma do hospedeiro (`uri_file` e `Platform::detect().is_windows`).
//! - `checkData` compara `sourceReference` e `sessionId` como `Option` (`undefined` é `None`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::platform::Platform;
use je_base::uri::{URI, UriComponents};
use je_instantiation::Injectable;
use je_platform::log::NullLogService;
use je_platform::resources::uri_file;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::debug::debug_protocol::{Source as ProtocolSource, SourcePresentationHint};
use je_workbench::contrib::debug::debug_source::Source;
use je_workbench::test::common::workbench_test_services::TestFileService;

/// `mockUriIdentityService`.
fn mock_uri_identity_service() -> Rc<UriIdentityService> {
    UriIdentityService::create((), (TestFileService::new(),))
}

fn components(scheme: &str, authority: Option<&str>, path: &str, query: Option<&str>) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(scheme)),
            authority: authority.map(JsString::from),
            path: Some(JsString::from(path)),
            query: query.map(JsString::from),
            ..UriComponents::default()
        },
        false,
    )
}

#[test]
fn from_raw_source() {
    let uri_identity_service = mock_uri_identity_service();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let source = Source::new(
        Some(ProtocolSource {
            name: Some("zz".to_owned()),
            path: Some("/xx/yy/zz".to_owned()),
            source_reference: Some(0),
            presentation_hint: Some(SourcePresentationHint::Emphasize),
            ..ProtocolSource::default()
        }),
        "aDebugSessionId",
        &*uri_identity_service,
        &NullLogService,
    );

    assert_eq!(source.presentation_hint(), Some(SourcePresentationHint::Emphasize));
    assert_eq!(source.name(), "zz");
    assert!(!source.in_memory());
    assert_eq!(source.reference(), Some(0));
    assert_eq!(source.uri.to_string(false), uri_file("/xx/yy/zz").to_string(false));
}

#[test]
fn from_raw_internal_source() {
    let uri_identity_service = mock_uri_identity_service();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let source = Source::new(
        Some(ProtocolSource {
            name: Some("internalModule.js".to_owned()),
            source_reference: Some(11),
            presentation_hint: Some(SourcePresentationHint::Deemphasize),
            ..ProtocolSource::default()
        }),
        "aDebugSessionId",
        &*uri_identity_service,
        &NullLogService,
    );

    assert_eq!(source.presentation_hint(), Some(SourcePresentationHint::Deemphasize));
    assert_eq!(source.name(), "internalModule.js");
    assert!(source.in_memory());
    assert_eq!(source.reference(), Some(11));
    assert_eq!(source.uri.to_string(false), "debug:internalModule.js?session%3DaDebugSessionId%26ref%3D11");
}

#[test]
fn get_encoded_debug_data() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let is_windows = Platform::detect().is_windows;
    let check_data = |uri: &URI, expected_name: &str, expected_path: &str, expected_source_reference: Option<i64>, expected_session_id: Option<&str>| {
        let data = Source::get_encoded_debug_data(uri);
        assert_eq!(data.name, expected_name);
        assert_eq!(data.path, expected_path);
        assert_eq!(data.source_reference, expected_source_reference);
        assert_eq!(data.session_id.as_deref(), expected_session_id);
    };

    check_data(&uri_file("a/b/c/d"), "d", if is_windows { "\\a\\b\\c\\d" } else { "/a/b/c/d" }, None, None);
    check_data(
        &components("file", None, "/my/path/test.js", Some("ref=1&session=2")),
        "test.js",
        if is_windows { "\\my\\path\\test.js" } else { "/my/path/test.js" },
        None,
        None,
    );

    check_data(&components("http", Some("www.example.com"), "/my/path", None), "path", "http://www.example.com/my/path", None, None);
    check_data(&components("debug", Some("www.example.com"), "/my/path", Some("ref=100")), "path", "/my/path", Some(100), None);
    check_data(&components("debug", None, "a/b/c/d.js", Some("session=100")), "d.js", "a/b/c/d.js", None, Some("100"));
    check_data(&components("debug", None, "a/b/c/d/foo.txt", Some("session=100&ref=10")), "foo.txt", "a/b/c/d/foo.txt", Some(10), Some("100"));
}
