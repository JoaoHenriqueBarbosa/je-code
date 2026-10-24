//! Porte de `src/vs/workbench/contrib/webview/test/browser/resourceLoading.test.ts` (suíte `Webview Resource Loading - getResourceToLoad`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `setup` da suíte viram o [`setup`]: o `TestFileService` nasce antes do
//!   `LeakGuard` (como no porte de `debugSource`, onde o serviço de teste fica fora do rastreio), e o `UriIdentityService` e o
//!   `InstantiationService` (o `TestInstantiationService` com os três serviços do `stub`) são registrados no guard.
//! - `sinon.spy(fileService, 'readFileStream')` é `TestFileService::read_file_stream_operations`, o recurso de cada chamada.
//!   `sinon.restore()` do `teardown` não tem equivalente (o registro vive no serviço do caso).
//! - `(cond ? suite.skip : suite)('...')` é um retorno no começo de cada caso quando a condição de pular vale (`isWindows` é a
//!   plataforma do hospedeiro), antes de criar qualquer coisa, como o mocha que nem roda o `setup` de um caso pulado.
//! - `decodeURIComponent(new URL(raw).pathname)` é o [`url_pathname_decoded`]: o caminho da URL com o espaço
//!   percent-encoded (como o parser do WHATWG faz no caminho) e depois decodificado.
//! - `for (const os of [Linux, Macintosh]) test(...)` vira um caso por sistema, os dois sobre o [`posix_remote_containment`].
//! - `JSON.parse(result.query).requestResourcePath` é o [`request_resource_path`].
//! - `await Promise.all(...)` é `Promise::all(...).await`, dentro do `run_with_faked_timers` (o corpo `async` do mocha).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::json::{self, Value};
use je_base::path::Platform;
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange, UriComponents, decode_uri_component};
use je_instantiation::{Injectable, InstantiationService, ServiceCollection};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_platform::resources::{host_platform, uri_file};
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::webview::resource_loading::webview_resource_response::Type;
use je_workbench::contrib::webview::resource_loading::{LoadLocalResourceOptions, StreamResponse, get_resource_to_load, load_local_resource};
use je_workbench::test::common::workbench_test_services::TestFileService;

/// O que o `setup` da suíte deixa em `let`s do módulo.
struct Context {
    _guard: LeakGuard,
    services: Services,
}

/// Os serviços do `setup`, que os casos `async` levam para dentro do corpo (o guard fica fora, até o fim do caso).
#[derive(Clone)]
struct Services {
    instantiation_service: Rc<InstantiationService>,
    file_service: Rc<TestFileService>,
    uri_identity_service: Rc<UriIdentityService>,
}

impl std::ops::Deref for Context {
    type Target = Services;

    fn deref(&self) -> &Services {
        &self.services
    }
}

fn setup() -> Context {
    let file_service = TestFileService::new();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let file_service_service: Rc<dyn IFileService> = file_service.clone();
    let uri_identity_service = guard.add_rc(UriIdentityService::create((), (file_service_service.clone(),)));
    let uri_identity_service_service: Rc<dyn IUriIdentityService> = uri_identity_service.clone();
    let instantiation_service = guard.add_rc(InstantiationService::new(
        ServiceCollection::new()
            .with(I_LOG_SERVICE, NullLogService::shared())
            .with(I_FILE_SERVICE, file_service_service)
            .with(I_URI_IDENTITY_SERVICE, uri_identity_service_service),
    ));
    Context { _guard: guard, services: Services { instantiation_service, file_service, uri_identity_service } }
}

impl Services {
    /// `getResourceToLoad(resource, roots, uriIdentityService, remoteOS)`.
    fn resolve(&self, resource: &URI, roots: &[URI], remote_os: Option<OperatingSystem>) -> Option<URI> {
        get_resource_to_load(resource, roots, &*self.uri_identity_service, remote_os)
    }

    /// `instantiationService.invokeFunction(accessor => loadLocalResource(accessor, resource, { ifNoneMatch: undefined, roots: [root] }, CancellationToken.None))`.
    fn load(&self, resource: &URI, root: &URI) -> Promise<StreamResponse> {
        let options = LoadLocalResourceOptions { if_none_match: None, roots: vec![root.clone()], ..LoadLocalResourceOptions::default() };
        self.instantiation_service.invoke_function(|accessor| load_local_resource(accessor, resource, &options, &CancellationToken::None))
    }

    /// `readFileStream.getCalls().map(call => call.args[0].toString())`.
    fn read_resources(&self) -> Vec<JsString> {
        self.file_service.read_file_stream_operations.borrow().iter().map(|resource| resource.to_string(false)).collect()
    }
}

fn is_windows() -> bool {
    matches!(host_platform(), Platform::Win32)
}

/// `URI.from({ scheme, authority, path })`.
fn uri_from(scheme: &str, authority: &str, path: &str) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(scheme)),
            authority: Some(JsString::from(authority)),
            path: Some(JsString::from(path)),
            ..UriComponents::default()
        },
        false,
    )
}

/// `URI.from({ scheme: Schemas.file, path })`.
fn file_from(path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from("file")), path: Some(JsString::from(path)), ..UriComponents::default() }, false)
}

fn with_path(uri: &URI, path: &str) -> URI {
    uri.with(&UriChange { path: Field::from(path), ..UriChange::default() })
}

fn with_query(uri: &URI, query: &str) -> URI {
    uri.with(&UriChange { query: Field::from(query), ..UriChange::default() })
}

fn with_fragment(uri: &URI, fragment: &str) -> URI {
    uri.with(&UriChange { fragment: Field::from(fragment), ..UriChange::default() })
}

fn with_scheme(uri: &URI, scheme: &str) -> URI {
    uri.with(&UriChange { scheme: Field::from(scheme), ..UriChange::default() })
}

/// `result?.toString()`.
fn to_string(result: &Option<URI>) -> Option<JsString> {
    result.as_ref().map(|uri| uri.to_string(false))
}

/// `decodeURIComponent(text)`.
fn decode(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(&decode_uri_component(&units).expect("decodeURIComponent"))
}

/// `decodeURIComponent(new URL(raw).pathname)`.
fn url_pathname_decoded(raw: &str) -> String {
    let after_scheme = raw.split_once("://").map_or(raw, |(_, rest)| rest);
    let pathname = after_scheme.find('/').map_or("/", |index| &after_scheme[index..]);
    decode(&pathname.replace(' ', "%20"))
}

/// `JSON.parse(result.query).requestResourcePath`.
fn request_resource_path(resource: &URI) -> String {
    let mut errors = Vec::new();
    let Some(Value::Object(object)) = json::parse_str(&resource.query().to_string_lossy(), &mut errors) else {
        panic!("a query não é um objeto JSON");
    };
    match object.get("requestResourcePath") {
        Some(Value::String(path)) => path.to_string_lossy(),
        other => panic!("requestResourcePath ausente: {other:?}"),
    }
}

#[test]
fn returns_resource_when_file_is_under_root() {
    let context = setup();
    let root = uri_file("/home/user/project");
    let resource = uri_file("/home/user/project/file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(to_string(&result), Some(resource.to_string(false)));
}

#[test]
fn returns_resource_when_file_is_in_nested_directory() {
    let context = setup();
    let root = uri_file("/home/user/project");
    let resource = uri_file("/home/user/project/subdir/nested/file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(to_string(&result), Some(resource.to_string(false)));
}

#[test]
fn fails_when_file_is_outside_root() {
    let context = setup();
    let root = uri_file("/home/user/project");
    let resource = uri_file("/home/user/other/file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(result, None);
}

#[test]
fn fails_when_file_is_root() {
    let context = setup();
    let root = uri_file("/home/user/project");
    let result = context.resolve(&root, &[root.clone()], None);
    assert_eq!(result, None);
}

#[test]
fn fails_when_file_is_sibling_of_root_directory() {
    let context = setup();
    let root = uri_file("/home/user/project");
    {
        let resource = uri_file("/home/user/projectOther/file.txt");
        let result = context.resolve(&resource, &[root.clone()], None);
        assert_eq!(result, None);
    }
    {
        let resource = uri_file("/home/user/project.txt");
        let result = context.resolve(&resource, &[root.clone()], None);
        assert_eq!(result, None);
    }
}

#[test]
fn returns_resource_when_root_ends_with_slash() {
    let context = setup();
    let root = uri_file("/home/user/project/");
    let resource = uri_file("/home/user/project/file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(to_string(&result), Some(resource.to_string(false)));
}

#[test]
fn fails_for_sibling_when_root_ends_with_slash() {
    let context = setup();
    let root = uri_file("/home/user/project/");
    let resource = uri_file("/home/user/projectOther/file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(result, None);
}

// UNC paths (windows only)

#[test]
fn unc_returns_resource_when_file_is_under_unc_root() {
    if !is_windows() {
        return;
    }
    let context = setup();
    let root = uri_file("\\\\server\\share\\folder");
    let resource = uri_file("\\\\server\\share\\folder\\file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(to_string(&result), Some(resource.to_string(false)));
}

#[test]
fn unc_returns_resource_with_case_insensitive_comparison_for_unc_paths() {
    if !is_windows() {
        return;
    }
    let context = setup();
    let root = uri_file("\\\\SERVER\\SHARE\\folder");
    let resource = uri_file("\\\\server\\share\\folder\\file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(to_string(&result), Some(resource.to_string(false)));
}

#[test]
fn unc_fails_when_file_is_outside_unc_root() {
    if !is_windows() {
        return;
    }
    let context = setup();
    let root = uri_file("\\\\server\\share\\folder");
    let resource = uri_file("\\\\server\\share\\other\\file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(result, None);
}

#[test]
fn unc_fails_when_unc_server_differs() {
    if !is_windows() {
        return;
    }
    let context = setup();
    let root = uri_file("\\\\server1\\share\\folder");
    let resource = uri_file("\\\\server2\\share\\folder\\file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(result, None);
}

// Windows path handling (windows only)

#[test]
fn windows_denies_parent_segments_ending_in_spaces_from_every_reported_input() {
    if !is_windows() {
        return;
    }
    let context = setup();
    let root = uri_from("file", "", "/c:/Users/alice/project");

    // (source, raw, path)
    let cases = [
        (
            "encoded webview URL",
            "https://file+.vscode-resource.vscode-cdn.net/c:/Users/alice/project/..%20/.ssh/id_rsa",
            url_pathname_decoded("https://file+.vscode-resource.vscode-cdn.net/c:/Users/alice/project/..%20/.ssh/id_rsa"),
        ),
        (
            "literal-space webview URL",
            "https://file+.vscode-resource.vscode-cdn.net/c:/Users/alice/project/.. /.ssh/id_rsa",
            url_pathname_decoded("https://file+.vscode-resource.vscode-cdn.net/c:/Users/alice/project/.. /.ssh/id_rsa"),
        ),
        ("load-resource message", "/c:/Users/alice/project/.. /.ssh/id_rsa", "/c:/Users/alice/project/.. /.ssh/id_rsa".to_owned()),
    ];

    let actual: Vec<_> = cases
        .iter()
        .map(|(source, raw, path)| {
            let resource = file_from(path);
            (*source, *raw, path.clone(), to_string(&context.resolve(&resource, &[root.clone()], None)))
        })
        .collect();
    let expected: Vec<_> = cases
        .iter()
        .map(|(source, raw, _)| (*source, *raw, "/c:/Users/alice/project/.. /.ssh/id_rsa".to_owned(), None))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn windows_does_not_read_a_resource_through_a_parent_segment_ending_in_a_space() {
    if !is_windows() {
        return;
    }
    let context = setup();
    let services = context.services.clone();
    run_with_faked_timers(async move {
        let root = uri_from("file", "", "/c:/Users/alice/project");
        let request_resource = file_from("/c:/Users/alice/project/.. /outside.txt");

        let result = services.load(&request_resource, &root).await.expect("loadLocalResource");

        assert_eq!((result.response_type(), services.read_resources()), (Type::AccessDenied, Vec::<JsString>::new()));
    });
}

#[test]
fn windows_does_not_alias_distinct_roots_ending_in_spaces_or_dots() {
    if !is_windows() {
        return;
    }
    let context = setup();
    let resource = file_from("/c:/Users/alice/project/inside.txt");
    let roots = [
        file_from("/c:/Users/alice/project "),
        file_from("/c:/Users/alice/project."),
        file_from("/c:/Users/alice/project.."),
    ];

    let actual: Vec<Option<URI>> = roots.iter().map(|root| context.resolve(&resource, &[root.clone()], None)).collect();
    assert_eq!(actual, vec![None, None, None]);
}

#[test]
fn windows_reads_distinct_in_root_resources_ending_in_spaces_or_dots_without_rewriting_them() {
    if !is_windows() {
        return;
    }
    let context = setup();
    let services = context.services.clone();
    run_with_faked_timers(async move {
        let root = uri_from("file", "", "/c:/Users/alice/project");
        let request_resources = [
            file_from("/c:/Users/alice/project/nested /inside.txt"),
            file_from("/c:/Users/alice/project/nested./inside.txt"),
            file_from("/c:/Users/alice/project/nested../inside.txt"),
        ];

        let results = Promise::all(request_resources.iter().map(|request_resource| services.load(request_resource, &root)).collect())
            .await
            .expect("Promise.all");

        assert_eq!(
            (results.iter().map(|result| result.response_type()).collect::<Vec<_>>(), services.read_resources()),
            (
                vec![Type::Success, Type::Success, Type::Success],
                request_resources.iter().map(|resource| resource.to_string(false)).collect::<Vec<_>>()
            )
        );
    });
}

// Different authorities

#[test]
fn authorities_returns_resource_when_authorities_match() {
    let context = setup();
    let root = uri_from("test-scheme", "ssh-remote+myserver", "/home/user/project");
    let resource = uri_from("test-scheme", "ssh-remote+myserver", "/home/user/project/file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert!(result.is_some());
}

#[test]
fn authorities_fails_when_authorities_differ() {
    let context = setup();
    let root = uri_from("test-scheme", "ssh-remote+server1", "/home/user/project");
    let resource = uri_from("test-scheme", "ssh-remote+server2", "/home/user/project/file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(result, None);
}

#[test]
fn authorities_handles_empty_authority() {
    let context = setup();
    let root = uri_from("test-scheme", "", "/home/user/project");
    let resource = uri_from("test-scheme", "", "/home/user/project/file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(to_string(&result), Some(resource.to_string(false)));
}

// Different schemes

#[test]
fn schemes_fails_when_schemes_differ() {
    let context = setup();
    let root = uri_from("file", "", "/home/user/project");
    let resource = uri_from("http", "", "/home/user/project/file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(result, None);
}

#[test]
fn schemes_returns_resource_when_schemes_match() {
    let context = setup();
    let root = uri_from("custom-scheme", "", "/home/user/project");
    let resource = uri_from("custom-scheme", "", "/home/user/project/file.txt");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(to_string(&result), Some(resource.to_string(false)));
}

#[test]
fn schemes_normalizes_vscode_remote_scheme() {
    let context = setup();
    let root = uri_from("vscode-remote", "test", "/home/user/project");
    let resource = uri_from("vscode-remote", "test", "/home/user/project/file.txt");
    let result = context.resolve(&resource, &[root], Some(OperatingSystem::Linux));

    let result = result.expect("resultado");
    assert_eq!(result.scheme().to_string_lossy(), "vscode-remote");
    assert_eq!(result.authority().to_string_lossy(), "test");
    assert_eq!(result.path().to_string_lossy(), "/vscode-resource");
    assert_eq!(request_resource_path(&result), "/home/user/project/file.txt");
}

#[test]
fn schemes_fails_for_vscode_remote_path_traversal_with_backslashes() {
    let context = setup();
    let root = uri_from("vscode-remote", "test", "/C:/workspace/project");
    let resource = uri_from("vscode-remote", "test", "/C:/workspace/project/..\\..\\outside.txt");
    let result = context.resolve(&resource, &[root], Some(OperatingSystem::Windows));

    assert_eq!(result, None);
}

// Remote filesystem semantics

/// `remote(path, authority = 'test')`.
fn remote(path: &str) -> URI {
    remote_with_authority(path, "test")
}

fn remote_with_authority(path: &str, authority: &str) -> URI {
    uri_from("vscode-remote", authority, path)
}

#[test]
fn remote_rejects_encoded_windows_traversal_and_linux_check_read_mismatch() {
    let context = setup();
    // (os, root, path)
    let cases = [
        (OperatingSystem::Windows, "/c:/allowed", "/c:/allowed/x%5C..%5C..%5Coutside%5Coutside.txt"),
        (OperatingSystem::Linux, "/home/user/allowed", "/home/user/allowed/a%5Cb/../../outside/outside.txt"),
        (OperatingSystem::Macintosh, "/home/user/allowed", "/home/user/allowed/a%5Cb/../../outside/outside.txt"),
        (OperatingSystem::Windows, "/c:/allowed", "/c:/allowed/sub/..%5C../outside.txt"),
        (OperatingSystem::Linux, "/home/user/allowed", "/home/user/allowed/../outside.txt"),
    ];
    let actual: Vec<Option<URI>> =
        cases.iter().map(|(os, root, path)| context.resolve(&remote(&decode(path)), &[remote(root)], Some(*os))).collect();
    assert_eq!(actual, cases.iter().map(|_| None).collect::<Vec<Option<URI>>>());
}

#[test]
fn remote_forwards_the_canonical_checked_windows_path_and_preserves_posix_backslashes() {
    let context = setup();
    // (os, root, path, expected)
    let cases = [
        (OperatingSystem::Windows, "/c:/allowed", "/c:/allowed/sub\\..\\file.txt", "/c:/allowed/file.txt"),
        (OperatingSystem::Windows, "/c:/allowed\\sub/..", "/c:/allowed/file.txt", "/c:/allowed/file.txt"),
        (OperatingSystem::Windows, "/c:/", "/c:/../file.txt", "/c:/file.txt"),
        (OperatingSystem::Linux, "/home/user/allowed", "/home/user/allowed/x\\..\\..\\file.txt", "/home/user/allowed/x\\..\\..\\file.txt"),
        (
            OperatingSystem::Linux,
            "/home/user/parent\\..\\allowed",
            "/home/user/parent\\..\\allowed/..\\file.txt",
            "/home/user/parent\\..\\allowed/..\\file.txt",
        ),
        (OperatingSystem::Macintosh, "/home/user/allowed", "/home/user/allowed/x\\..\\..\\file.txt", "/home/user/allowed/x\\..\\..\\file.txt"),
        (
            OperatingSystem::Macintosh,
            "/home/user/parent\\..\\allowed",
            "/home/user/parent\\..\\allowed/..\\file.txt",
            "/home/user/parent\\..\\allowed/..\\file.txt",
        ),
        (OperatingSystem::Macintosh, "/home/user/allowed", "/home/user/allowed/a\\b/../file.txt", "/home/user/allowed/file.txt"),
    ];
    let actual: Vec<String> = cases
        .iter()
        .map(|(os, root, path, _)| {
            let result = context.resolve(&remote(path), &[remote(root)], Some(*os));
            let result = result.unwrap_or_else(|| panic!("OS: {os:?}, root: {root}, path: {path}"));
            request_resource_path(&result)
        })
        .collect();
    assert_eq!(actual, cases.iter().map(|(_, _, _, expected)| (*expected).to_owned()).collect::<Vec<_>>());
}

#[test]
fn remote_rejects_unknown_target_os_normalized_root_equality_sibling_paths_and_different_authorities() {
    let context = setup();
    let root = remote("/c:/allowed");
    assert_eq!(
        vec![
            context.resolve(&remote("/c:/allowed/file.txt"), &[root.clone()], None),
            context.resolve(&remote("/c:/allowed/sub/.."), &[root.clone()], Some(OperatingSystem::Windows)),
            context.resolve(&remote("/c:/allowedOther/file.txt"), &[root.clone()], Some(OperatingSystem::Windows)),
            context.resolve(&remote_with_authority("/c:/allowed/file.txt", "other"), &[root.clone()], Some(OperatingSystem::Windows)),
        ],
        vec![None, None, None, None]
    );
}

/// O corpo de `Preserves containment boundaries for POSIX remotes (OS: ${os})`.
fn posix_remote_containment(os: OperatingSystem) {
    let context = setup();
    let root = remote("/home/user/allowed");
    assert_eq!(
        vec![
            context.resolve(&remote("/home/user/allowed/..\\file.txt"), &[root.clone()], None),
            context.resolve(&remote("/home/user/allowed/child/.."), &[root.clone()], Some(os)),
            context.resolve(&remote("/home/user/allowedOther/..\\file.txt"), &[root.clone()], Some(os)),
            context.resolve(&remote_with_authority("/home/user/allowed/..\\file.txt", "other"), &[root.clone()], Some(os)),
            context.resolve(&remote("/home/user/allowed/file.txt"), &[with_path(&root, "/home/user/allowed/child\\..")], Some(os)),
            context.resolve(&remote("/home/user/allowed/..\\file.txt"), &[with_scheme(&root, "vscode-userdata")], Some(os)),
            context.resolve(&remote("/home/user/allowed/..\\file.txt"), &[with_query(&root, "version=1")], Some(os)),
        ],
        vec![None, None, None, None, None, None, None]
    );
}

#[test]
fn remote_preserves_containment_boundaries_for_posix_remotes_os_linux() {
    posix_remote_containment(OperatingSystem::Linux);
}

#[test]
fn remote_preserves_containment_boundaries_for_posix_remotes_os_macintosh() {
    posix_remote_containment(OperatingSystem::Macintosh);
}

#[test]
fn remote_keeps_conservative_containment_for_non_remote_schemes_on_posix_remote_connections() {
    let context = setup();
    let root = uri_from("custom", "", "/allowed");
    let resource = with_path(&root, "/allowed/..\\outside.txt");
    assert_eq!(
        [OperatingSystem::Linux, OperatingSystem::Macintosh]
            .iter()
            .map(|os| context.resolve(&resource, &[root.clone()], Some(*os)))
            .collect::<Vec<Option<URI>>>(),
        vec![None, None]
    );
}

#[test]
fn remote_preserves_literal_backslashes_for_custom_filesystem_schemes() {
    let context = setup();
    let root = uri_from("custom", "", "/allowed");
    let resource = with_path(&root, "/allowed/a\\b/../../outside.txt");
    assert_eq!(context.resolve(&resource, &[root], None), None);
}

// Fragment and query strings

#[test]
fn fragment_preserves_fragment_in_returned_uri() {
    let context = setup();
    let root = uri_file("/home/user/project");
    let resource = with_fragment(&uri_file("/home/user/project/file.txt"), "section1");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(result.map(|uri| uri.fragment().to_string_lossy()), Some("section1".to_owned()));
}

#[test]
fn fragment_preserves_query_in_returned_uri() {
    let context = setup();
    let root = uri_file("/home/user/project");
    let resource = with_query(&uri_file("/home/user/project/file.txt"), "version=2");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(result.map(|uri| uri.query().to_string_lossy()), Some("version=2".to_owned()));
}

#[test]
fn fragment_preserves_both_fragment_and_query() {
    let context = setup();
    let root = uri_file("/home/user/project");
    let resource = with_query(&with_fragment(&uri_file("/home/user/project/file.txt"), "section1"), "version=2");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(result.as_ref().map(|uri| uri.fragment().to_string_lossy()), Some("section1".to_owned()));
    assert_eq!(result.as_ref().map(|uri| uri.query().to_string_lossy()), Some("version=2".to_owned()));
}

#[test]
fn fragment_still_validates_path_containment_with_query_params() {
    let context = setup();
    let root = uri_file("/home/user/project");
    let resource = with_query(&uri_file("/home/user/other/file.txt"), "version=2");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(result, None);
}

#[test]
fn fragment_still_validates_path_containment_with_fragment() {
    let context = setup();
    let root = uri_file("/home/user/project");
    let resource = with_fragment(&uri_file("/home/user/other/file.txt"), "section1");
    let result = context.resolve(&resource, &[root], None);
    assert_eq!(result, None);
}

// Multiple roots

#[test]
fn roots_returns_resource_when_file_is_under_one_of_multiple_roots() {
    let context = setup();
    let roots = [uri_file("/home/user/project1"), uri_file("/home/user/project2"), uri_file("/home/user/project3")];
    let resource = uri_file("/home/user/project2/file.txt");
    let result = context.resolve(&resource, &roots, None);
    assert_eq!(to_string(&result), Some(resource.to_string(false)));
}

#[test]
fn roots_fails_when_file_is_not_under_any_root() {
    let context = setup();
    let roots = [uri_file("/home/user/project1"), uri_file("/home/user/project2")];
    let resource = uri_file("/home/user/other/file.txt");
    let result = context.resolve(&resource, &roots, None);
    assert_eq!(result, None);
}

#[test]
fn roots_returns_resource_matching_first_valid_root() {
    let context = setup();
    let roots = [uri_file("/home/user/project"), uri_file("/home/user/project/subdir")];
    let resource = uri_file("/home/user/project/subdir/file.txt");
    let result = context.resolve(&resource, &roots, None);
    // Should match first root in the list
    assert_eq!(to_string(&result), Some(resource.to_string(false)));
}

#[test]
fn roots_handles_empty_roots_array() {
    let context = setup();
    let resource = uri_file("/home/user/project/file.txt");
    let result = context.resolve(&resource, &[], None);
    assert_eq!(result, None);
}
