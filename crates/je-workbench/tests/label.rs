//! Porte de `src/vs/workbench/services/label/test/browser/label.test.ts` (suítes `URI Label`, `multi-root workspace` e
//! `workspace at FSP root`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O `setup` de cada suíte é declarado antes do
//!   `ensure...`, então o que ele cria não é rastreado (e, em `URI Label` e `workspace at FSP root`, nem é descartado).
//!   Aqui o `setup` é a função [`setup`] chamada antes do guarda. Em `multi-root workspace` o `teardown` descarta o
//!   store de `disposables`: é o `Drop` de [`MultiRoot`].
//! - **`uriIdentityService`** é o [`TestUriIdentityService`] (um `IUriIdentityService` com o `ExtUri` dado e
//!   `asCanonicalUri` identidade), e `new LabelService(...)` é `LabelService::create((), (...))`, o construtor do
//!   serviço (as dependências em tupla, na ordem do original).
//! - **Formatadores.** Os literais de `ResourceLabelFormatter` e `ResourceLabelTemplateFormatter` usam as structs com
//!   `..Default::default()`; os campos que o original omite (`undefined`) são `None`. `Event.None` é `Event::none()`.
//!   `sep` é o separador nativo do `path`, e `isWindows` o `is_windows` da plataforma detectada.
//! - **Valores.** `assert.deepStrictEqual({ a, b }, { ... })` é a comparação de tuplas (`undefined` é `None`);
//!   `Object.entries(tests).forEach(...)` percorre uma lista de pares na mesma ordem. `encodeURIComponent` é o
//!   `encode_uri_component` do `je-base`, e o JSON da consulta é o texto que o `JSON.stringify` produziria.
//! - **`label caching`.** O `Memento` lê o mesmo objeto que o serviço grava (os mementos são compartilhados por id), e
//!   os objetos esperados são escritos por extenso em `Dynamic`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::json::Value;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::path::PathModule;
use je_base::platform::Platform;
use je_base::resources::ExtUri;
use je_base::uri::{Field, URI, UriChange, UriComponents, encode_uri_component};
use je_instantiation::Injectable;
use je_platform::js::to_json;
use je_platform::label::label::{
    ILabelService, LabelFormatter, LabelSeparator, ResourceLabelFormatter, ResourceLabelFormatting, ResourceLabelFormattingContext,
    ResourceLabelTemplateFormatter, UriLabelOptions,
};
use je_platform::resources::{ext_uri, host_environment, uri_file};
use je_platform::storage::{AbstractStorageService, StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::{WorkspaceFolder, default_test_workspace, new_test_workspace};
use je_workbench::common::memento::Memento;
use je_workbench::services::label::label_service::LabelService;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestPathService, TestRemoteAgentService};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestLifecycleService, TestStorageService};

/// Um `IUriIdentityService` com o `ExtUri` dado.
struct TestUriIdentityService {
    ext_uri: ExtUri,
}

impl IUriIdentityService for TestUriIdentityService {
    fn ext_uri(&self) -> ExtUri {
        self.ext_uri.clone()
    }

    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

/// O `uriIdentityService` do topo do arquivo: `extUri: resources.extUri`.
fn uri_identity_service() -> Rc<dyn IUriIdentityService> {
    Rc::new(TestUriIdentityService { ext_uri: ExtUri::unbiased(host_environment()) })
}

fn storage_service(storage: &Rc<AbstractStorageService>) -> Rc<dyn StorageService> {
    Rc::new(storage.clone())
}

/// `new LabelService(TestEnvironmentService, context, path, new TestRemoteAgentService(), storage, lifecycle, uriIdentity)`.
fn new_label_service(
    context: Rc<TestContextService>,
    path_service: Rc<TestPathService>,
    storage: &Rc<AbstractStorageService>,
    lifecycle: Rc<TestLifecycleService>,
    uri_identity: Rc<dyn IUriIdentityService>,
) -> Rc<LabelService> {
    LabelService::create((), (TestEnvironmentService::new(), context, path_service, Rc::new(TestRemoteAgentService), storage_service(storage), lifecycle, uri_identity))
}

/// `sep` do `path` nativo.
fn sep() -> &'static str {
    PathModule::native(host_environment().as_ref()).sep()
}

fn label_separator(text: &str) -> LabelSeparator {
    LabelSeparator::from_text(text).expect("separador válido")
}

fn is_windows() -> bool {
    Platform::detect().is_windows
}

fn parse(text: &str) -> URI {
    URI::parse(text, false)
}

fn from_components(scheme: Option<&str>, authority: Option<&str>, path: Option<&str>) -> URI {
    URI::from(
        &UriComponents { scheme: scheme.map(JsString::from), authority: authority.map(JsString::from), path: path.map(JsString::from), ..UriComponents::default() },
        false,
    )
}

fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn with_path(uri: &URI, path: String) -> URI {
    uri.with(&UriChange { path: Field::Set(JsString::from(path)), ..UriChange::default() })
}

/// `labelService.getUriLabel(resource)`.
fn label(service: &LabelService, resource: &URI) -> String {
    service.get_uri_label(resource, &UriLabelOptions::default())
}

/// `labelService.getUriLabel(resource, { relative })`.
fn label_relative(service: &LabelService, resource: &URI, relative: bool) -> String {
    service.get_uri_label(resource, &UriLabelOptions { relative: Some(relative), ..UriLabelOptions::default() })
}

fn home_text(service: &LabelService, resource: &URI) -> Option<String> {
    service.get_uri_home(resource).map(|home| uri_text(&home))
}

fn formatting(label: &str, separator: LabelSeparator) -> ResourceLabelFormatting {
    ResourceLabelFormatting { label: label.to_string(), separator, ..ResourceLabelFormatting::default() }
}

/// `{ scheme, formatting: { label, separator: '/' } }`.
fn scheme_formatter(scheme: &str, formatting: ResourceLabelFormatting) -> LabelFormatter {
    ResourceLabelFormatter { scheme: scheme.to_string(), formatting, ..ResourceLabelFormatter::default() }.into()
}

/// O formatador de `home` estático: `{ scheme, authority?, home, priority?, formatting: { label, separator: '/' } }`.
fn static_home_formatter(scheme: &str, authority: Option<&str>, home: &str, priority: bool, label: &str) -> LabelFormatter {
    ResourceLabelFormatter {
        scheme: scheme.to_string(),
        authority: authority.map(str::to_string),
        home: Some(home.to_string()),
        priority: priority.then_some(true),
        formatting: formatting(label, LabelSeparator::Slash),
    }
    .into()
}

/// O formatador de `home` com modelo: `{ home, onDidChangeFormatting: Event.None, formatting }`.
fn template_formatter(home: URI, resolve: impl Fn(&ResourceLabelFormattingContext) -> Option<ResourceLabelFormatting> + 'static) -> LabelFormatter {
    ResourceLabelTemplateFormatter { home, on_did_change_formatting: Event::none(), formatting: Rc::new(resolve) }.into()
}

/// `formatting: () => ({ label, separator: '/' })`.
fn fixed_formatting(label: &'static str) -> impl Fn(&ResourceLabelFormattingContext) -> Option<ResourceLabelFormatting> {
    move |_| Some(formatting(label, LabelSeparator::Slash))
}

/// `encodeURIComponent(text)`.
fn encode_component(text: &str) -> String {
    String::from_utf16_lossy(&encode_uri_component(&text.encode_utf16().collect::<Vec<u16>>()))
}

// ---------------------------------------------------------------------------------------------
// URI Label
// ---------------------------------------------------------------------------------------------

mod uri_label {
    use super::*;

    struct Fixture {
        label_service: Rc<LabelService>,
        storage_service: Rc<AbstractStorageService>,
    }

    fn setup() -> Fixture {
        let storage_service = TestStorageService::new();
        let label_service = new_label_service(
            TestContextService::new(None, None),
            TestPathService::new(Some(uri_file("/foobar")), None),
            &storage_service,
            TestLifecycleService::new(),
            uri_identity_service(),
        );
        Fixture { label_service, storage_service }
    }

    /// `new LabelService(TestEnvironmentService, new TestContextService(), new TestPathService(), ...)` com o
    /// `IUriIdentityService` dado, tudo dentro de `store`.
    fn label_service_in(store: &DisposableStore, uri_identity: Rc<dyn IUriIdentityService>) -> Rc<LabelService> {
        let storage = store.add_rc(TestStorageService::new());
        let lifecycle = store.add_rc(TestLifecycleService::new());
        store.add_rc(new_label_service(TestContextService::new(None, None), TestPathService::new(None, None), &storage, lifecycle, uri_identity))
    }

    #[test]
    fn custom_scheme() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        f.label_service.register_formatter(scheme_formatter(
            "vscode",
            ResourceLabelFormatting {
                tildify: Some(true),
                normalize_drive_letter: Some(true),
                ..formatting("LABEL/${path}/${authority}/END", LabelSeparator::Slash)
            },
        ));

        let uri1 = parse("vscode://microsoft.com/1/2/3/4/5");
        assert_eq!(label_relative(&f.label_service, &uri1, false), "LABEL//1/2/3/4/5/microsoft.com/END");
        assert_eq!(f.label_service.get_uri_basename_label(&uri1), "END");
    }

    #[test]
    fn file_scheme() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        f.label_service.register_formatter(scheme_formatter(
            "file",
            ResourceLabelFormatting {
                tildify: Some(!is_windows()),
                normalize_drive_letter: Some(is_windows()),
                ..formatting("${path}", label_separator(sep()))
            },
        ));

        let workspace_folder = default_test_workspace().folders()[0].uri.clone();
        let uri1 = with_path(&workspace_folder, format!("{}/a/b/c/d", workspace_folder.path().to_string_lossy()));
        assert_eq!(label_relative(&f.label_service, &uri1, true), if is_windows() { "a\\b\\c\\d" } else { "a/b/c/d" });
        assert_eq!(label_relative(&f.label_service, &uri1, false), if is_windows() { "C:\\testWorkspace\\a\\b\\c\\d" } else { "/testWorkspace/a/b/c/d" });
        assert_eq!(f.label_service.get_uri_basename_label(&uri1), "d");

        let uri2 = uri_file("c:\\1/2/3");
        assert_eq!(label_relative(&f.label_service, &uri2, false), if is_windows() { "C:\\1\\2\\3" } else { "/c:\\1/2/3" });
        assert_eq!(f.label_service.get_uri_basename_label(&uri2), "3");
    }

    #[test]
    fn separator_label() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        f.label_service.register_formatter(scheme_formatter(
            "vscode",
            ResourceLabelFormatting {
                tildify: Some(true),
                normalize_drive_letter: Some(true),
                ..formatting("LABEL\\${path}\\${authority}\\END", LabelSeparator::Backslash)
            },
        ));

        let uri1 = parse("vscode://microsoft.com/1/2/3/4/5");
        assert_eq!(label_relative(&f.label_service, &uri1, false), "LABEL\\\\1\\2\\3\\4\\5\\microsoft.com\\END");
        assert_eq!(f.label_service.get_uri_basename_label(&uri1), "END");
    }

    #[test]
    fn returns_the_most_specific_registered_resource_label_home() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let outer = uri_file("/home/test/.agent");
        let inner = uri_file("/home/test/.agent/sessions/session-id");
        let outer_registration = f.label_service.register_formatter(static_home_formatter(&outer.scheme().to_string_lossy(), None, &outer.path().to_string_lossy(), false, "Agent"));
        let inner_registration = f.label_service.register_formatter(static_home_formatter(&inner.scheme().to_string_lossy(), None, &inner.path().to_string_lossy(), false, "Session"));

        let resource = uri_file("/home/test/.agent/sessions/session-id/file.md");
        assert_eq!(
            (home_text(&f.label_service, &resource), label(&f.label_service, &resource)),
            (Some(uri_text(&inner)), "Session/file.md".to_string())
        );

        inner_registration.dispose();
        assert_eq!(
            (home_text(&f.label_service, &resource), label(&f.label_service, &resource)),
            (Some(uri_text(&outer)), "Agent/sessions/session-id/file.md".to_string())
        );
        outer_registration.dispose();
    }

    #[test]
    fn ignores_resource_label_homes_for_a_different_authority() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mismatched_registration = f.label_service.register_formatter(static_home_formatter("test", Some("other"), "/sessions/session-id", false, "Session"));

        assert_eq!(f.label_service.get_uri_home(&parse("test://current/sessions/session-id/file.md")), None);

        mismatched_registration.dispose();
    }

    #[test]
    fn preserves_resource_label_home_authority_and_priority_precedence() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let resource = parse("test://current/sessions/session-id/file.md");
        let without_authority = f.label_service.register_formatter(static_home_formatter("test", None, "/sessions/session-id", false, "No Authority"));
        let glob_authority = f.label_service.register_formatter(static_home_formatter("test", Some("cur*"), "/sessions/session-id", false, "Glob Authority"));
        let exact_authority = f.label_service.register_formatter(static_home_formatter("test", Some("current"), "/sessions/session-id", false, "Exact Authority"));
        let priority_exact_authority = f.label_service.register_formatter(static_home_formatter("test", Some("CURRENT"), "/sessions/session-id", true, "Priority Exact Authority"));

        assert_eq!(label(&f.label_service, &resource), "Priority Exact Authority/file.md");
        priority_exact_authority.dispose();
        assert_eq!(label(&f.label_service, &resource), "Exact Authority/file.md");
        exact_authority.dispose();
        assert_eq!(label(&f.label_service, &resource), "Glob Authority/file.md");
        glob_authority.dispose();
        assert_eq!(label(&f.label_service, &resource), "No Authority/file.md");
        without_authority.dispose();
    }

    #[test]
    fn resource_label_home_paths_are_case_sensitive() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let registration = f.label_service.register_formatter(static_home_formatter("test", Some("current"), "/Sessions/session-id", false, "Session"));

        assert_eq!(
            (label(&f.label_service, &parse("test://current/Sessions/session-id/file.md")), label(&f.label_service, &parse("test://current/sessions/session-id/file.md"))),
            ("Session/file.md".to_string(), format!("{0}sessions{0}session-id{0}file.md", sep()))
        );

        registration.dispose();
    }

    #[test]
    fn resource_label_home_paths_use_uri_identity_casing() {
        let _f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = DisposableStore::new();
        let case_insensitive_label_service = label_service_in(&store, Rc::new(TestUriIdentityService { ext_uri: ExtUri::ignore_path_case(host_environment()) }));
        let home = from_components(Some("file"), None, Some("/c:/Users/test/.copilot/session-state/session-id"));
        store.add(case_insensitive_label_service.register_formatter(static_home_formatter(
            &home.scheme().to_string_lossy(),
            None,
            &home.path().to_string_lossy(),
            false,
            "Session",
        )));
        let resource = from_components(Some("file"), None, Some("/C:/Users/Test/.copilot/session-state/session-id/files/result.html"));

        assert_eq!(
            (home_text(&case_insensitive_label_service, &resource), label(&case_insensitive_label_service, &resource)),
            (Some(uri_text(&home)), "Session/files/result.html".to_string())
        );

        store.dispose();
    }

    #[test]
    fn uri_home_templates_use_uri_identity_casing() {
        let _f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = DisposableStore::new();
        let case_insensitive_label_service = label_service_in(&store, Rc::new(TestUriIdentityService { ext_uri: ExtUri::ignore_path_case(host_environment()) }));
        store.add(case_insensitive_label_service.register_formatter(template_formatter(
            from_components(Some("file"), None, Some("/c:/Users/test/.copilot/session-state/${sessionId}")),
            fixed_formatting("Session"),
        )));
        let resource = from_components(Some("file"), None, Some("/C:/Users/Test/.copilot/session-state/session-id/files/result.html"));

        assert_eq!(
            (home_text(&case_insensitive_label_service, &resource), label(&case_insensitive_label_service, &resource)),
            (
                Some(uri_text(&from_components(Some("file"), None, Some("/C:/Users/Test/.copilot/session-state/session-id")))),
                "Session/files/result.html".to_string()
            )
        );

        store.dispose();
    }

    #[test]
    fn uri_home_templates_read_uri_identity_casing_at_lookup_time() {
        let _f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let ignore_path_casing = Rc::new(Cell::new(false));
        let store = DisposableStore::new();
        let casing = ignore_path_casing.clone();
        let dynamic_label_service = label_service_in(&store, Rc::new(TestUriIdentityService { ext_uri: ExtUri::new(host_environment(), move |_| casing.get()) }));
        store.add(dynamic_label_service.register_formatter(template_formatter(parse("test:/sessions/K/${sessionId}"), fixed_formatting("Session"))));
        let resource = parse("test:/sessions/K/session-id/file.md");

        // Medido no `LabelService` do VS Code 1.143.1 rodando em Node (esbuild sobre a fonte, com o mesmo `ExtUri`): a
        // primeira consulta, com a caixa do caminho respeitada, já devolve o home (`/sessions/K/session-id`), porque o
        // segmento `K` do modelo e o do recurso têm a mesma caixa. O `undefined` que o teste do original afirma não
        // acontece; a segunda consulta (caixa ignorada) devolve o mesmo home.
        assert_eq!(dynamic_label_service.get_uri_home(&resource).map(|home| home.path().to_string_lossy()), Some("/sessions/K/session-id".to_string()));
        ignore_path_casing.set(true);
        assert_eq!(dynamic_label_service.get_uri_home(&resource).map(|home| home.path().to_string_lossy()), Some("/sessions/K/session-id".to_string()));

        store.dispose();
    }

    #[test]
    fn resolves_uri_home_templates_to_concrete_formatting() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let resolver_calls = Rc::new(Cell::new(0u32));
        let static_registration = f.label_service.register_formatter(static_home_formatter("test", Some("current"), "/sessions/session-id", false, "Static Session"));
        let calls = resolver_calls.clone();
        let registration = f.label_service.register_formatter(template_formatter(from_components(Some("test"), Some("current"), Some("/sessions/${sessionId}")), move |context| {
            calls.set(calls.get() + 1);
            let session_id = context.parameters.get("sessionId");
            (session_id.map(String::as_str) == Some("session-id")).then(|| formatting("Session ${path}", LabelSeparator::Slash))
        }));
        let resource = parse("test://current/sessions/session-id/files/result.md");

        assert_eq!(
            (
                home_text(&f.label_service, &resource),
                label(&f.label_service, &resource),
                label(&f.label_service, &parse("test://current/unrelated/file.md")),
                resolver_calls.get()
            ),
            (
                Some("test://current/sessions/session-id".to_string()),
                "Session ${path}/files/result.md".to_string(),
                format!("{0}unrelated{0}file.md", sep()),
                2
            )
        );
        registration.dispose();
        assert_eq!(label(&f.label_service, &resource), "Static Session/files/result.md");
        static_registration.dispose();
    }

    #[test]
    fn uri_home_templates_without_an_authority_match_any_authority() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let registration = f.label_service.register_formatter(template_formatter(from_components(Some("test"), None, Some("/sessions/${sessionId}")), |context| {
            Some(formatting(context.parameters.get("sessionId").map_or("", String::as_str), LabelSeparator::Slash))
        }));

        assert_eq!(label(&f.label_service, &parse("test://remote/sessions/session-id/file.md")), "session-id/file.md");

        registration.dispose();
    }

    #[test]
    fn uri_home_formatters_support_authority_roots() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let registration = f.label_service.register_formatter(template_formatter(from_components(Some("test"), Some("current"), None), fixed_formatting("Root")));
        let resource = parse("test://current/file.md");

        assert_eq!(
            (home_text(&f.label_service, &resource), label(&f.label_service, &resource)),
            (Some("test://current".to_string()), "Root/file.md".to_string())
        );

        registration.dispose();
    }

    #[test]
    fn uri_home_templates_do_not_capture_dot_path_segments() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let resolver_calls = Rc::new(Cell::new(0u32));
        let calls = resolver_calls.clone();
        let registration = f.label_service.register_formatter(template_formatter(parse("test://current/sessions/${sessionId}"), move |_| {
            calls.set(calls.get() + 1);
            Some(formatting("Session", LabelSeparator::Slash))
        }));

        let current = parse("test://current/sessions/./file.md");
        let parent = parse("test://current/sessions/../file.md");
        assert_eq!(
            (
                f.label_service.get_uri_home(&current),
                label(&f.label_service, &current),
                f.label_service.get_uri_home(&parent),
                label(&f.label_service, &parent),
                label(&f.label_service, &parse("test://current/sessions/.session/file.md")),
                resolver_calls.get()
            ),
            (None, format!("{0}sessions{0}file.md", sep()), None, format!("{}file.md", sep()), "Session/file.md".to_string(), 1)
        );

        registration.dispose();
    }

    #[test]
    fn uri_home_templates_resolve_all_parameters() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let registration = f.label_service.register_formatter(template_formatter(parse("test://current/orgs/${org}/sessions/${sessionId}"), |context| {
            Some(formatting(&format!("{}/{}", context.parameters["org"], context.parameters["sessionId"]), LabelSeparator::Slash))
        }));
        let resource = parse("test://current/orgs/acme/sessions/session-id/file.md");

        assert_eq!(
            (f.label_service.get_uri_home(&resource).map(|home| home.path().to_string_lossy()), label(&f.label_service, &resource)),
            (Some("/orgs/acme/sessions/session-id".to_string()), "acme/session-id/file.md".to_string())
        );

        registration.dispose();
    }

    #[test]
    fn uri_home_templates_support_trailing_separators() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let registration = f.label_service.register_formatter(template_formatter(parse("test://current/sessions/${sessionId}/"), fixed_formatting("Session")));

        assert_eq!(
            (label(&f.label_service, &parse("test://current/sessions/session-id")), label(&f.label_service, &parse("test://current/sessions/session-id/file.md"))),
            ("Session".to_string(), "Session/file.md".to_string())
        );

        registration.dispose();
    }

    #[test]
    fn equally_specific_uri_home_templates_use_registration_order() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let home = parse("test://current/sessions/${sessionId}");
        let first = f.label_service.register_formatter(template_formatter(home.clone(), fixed_formatting("First")));
        let second = f.label_service.register_formatter(template_formatter(home, fixed_formatting("Second")));
        let resource = parse("test://current/sessions/session-id/file.md");

        assert_eq!(label(&f.label_service, &resource), "First/file.md");
        first.dispose();
        assert_eq!(label(&f.label_service, &resource), "Second/file.md");

        second.dispose();
    }

    #[test]
    fn one_uri_home_template_formats_all_known_sessions_without_resolving_unrelated_resources() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let formatter_changes = Rc::new(Cell::new(0u32));
        let resolver_calls = Rc::new(Cell::new(0u32));
        let changes = formatter_changes.clone();
        let listener = f.label_service.on_did_change_formatters().listen(move |_| changes.set(changes.get() + 1));
        let on_did_change = Emitter::<()>::new();
        let labels: Rc<RefCell<HashMap<String, String>>> = Rc::new(RefCell::new((0..100).map(|index| (format!("session-{index}"), format!("Session {index}"))).collect()));
        let calls = resolver_calls.clone();
        let known_labels = labels.clone();
        let registration = f.label_service.register_formatter(
            ResourceLabelTemplateFormatter {
                home: parse("test://current/sessions/${sessionId}"),
                on_did_change_formatting: on_did_change.event(),
                formatting: Rc::new(move |context: &ResourceLabelFormattingContext| {
                    calls.set(calls.get() + 1);
                    let session_label = known_labels.borrow().get(context.parameters.get("sessionId").map_or("", String::as_str)).cloned();
                    session_label.map(|session_label| formatting(&session_label, LabelSeparator::Slash))
                }),
            }
            .into(),
        );

        assert_eq!(
            (
                formatter_changes.get(),
                home_text(&f.label_service, &parse("test://current/sessions/session-42/file.md")),
                label(&f.label_service, &parse("test://current/sessions/session-42/file.md")),
                label(&f.label_service, &parse("test://current/unrelated/file.md")),
                resolver_calls.get()
            ),
            (
                1,
                Some("test://current/sessions/session-42".to_string()),
                "Session 42/file.md".to_string(),
                format!("{0}unrelated{0}file.md", sep()),
                2
            )
        );

        labels.borrow_mut().insert("session-42".to_string(), "Renamed Session".to_string());
        on_did_change.fire(());
        assert_eq!(
            (formatter_changes.get(), label(&f.label_service, &parse("test://current/sessions/session-42/file.md"))),
            (2, "Renamed Session/file.md".to_string())
        );

        registration.dispose();
        on_did_change.fire(());
        assert_eq!(formatter_changes.get(), 3);
        on_did_change.dispose();
        listener.dispose();
    }

    #[test]
    fn resource_label_homes_take_precedence_over_ordinary_formatter_changes() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let resource = uri_file("/home/test/.agent/sessions/session-id/file.md");
        let first = f.label_service.register_formatter(scheme_formatter("file", formatting("FIRST${path}", LabelSeparator::Slash)));
        let home = f.label_service.register_formatter(template_formatter(uri_file("/home/test/.agent/sessions/${sessionId}"), fixed_formatting("Session")));

        assert_eq!(label(&f.label_service, &resource), "Session/file.md");

        first.dispose();
        let second = f.label_service.register_formatter(scheme_formatter("file", formatting("SECOND${path}", LabelSeparator::Slash)));
        assert_eq!(label(&f.label_service, &resource), "Session/file.md");

        home.dispose();
        second.dispose();
    }

    #[test]
    fn no_prefix_skips_resource_label_homes() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let resource = uri_file("/home/test/.agent/sessions/session-id/file.md");
        let home = f.label_service.register_formatter(template_formatter(uri_file("/home/test/.agent/sessions/${sessionId}"), fixed_formatting("Agent/Session")));

        let no_prefix = UriLabelOptions { no_prefix: Some(true), ..UriLabelOptions::default() };
        assert_eq!(f.label_service.get_uri_label(&resource, &no_prefix), resource.fs_path(Platform::detect().path_flavor()).to_string_lossy());

        home.dispose();
    }

    #[test]
    fn keeps_equivalent_resource_label_home_registrations_until_all_are_disposed() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let root = uri_file("/home/test/.agent/sessions/session-id");
        let (scheme, path) = (root.scheme().to_string_lossy(), root.path().to_string_lossy());
        let first = f.label_service.register_formatter(static_home_formatter(&scheme, None, &path, false, "Session"));
        let second = f.label_service.register_formatter(static_home_formatter(&scheme, None, &path, false, "Session"));

        first.dispose();
        let home = f.label_service.get_uri_home(&uri_file("/home/test/.agent/sessions/session-id/file.md")).expect("o home continua registrado");
        assert_eq!(label(&f.label_service, &home), "Session");

        second.dispose();
        assert_eq!(f.label_service.get_uri_home(&uri_file("/home/test/.agent/sessions/session-id/file.md")), None);
    }

    #[test]
    fn custom_authority() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        f.label_service.register_formatter(
            ResourceLabelFormatter {
                scheme: "vscode".to_string(),
                authority: Some("micro*".to_string()),
                formatting: formatting("LABEL/${path}/${authority}/END", LabelSeparator::Slash),
                ..ResourceLabelFormatter::default()
            }
            .into(),
        );

        let uri1 = parse("vscode://microsoft.com/1/2/3/4/5");
        assert_eq!(label_relative(&f.label_service, &uri1, false), "LABEL//1/2/3/4/5/microsoft.com/END");
        assert_eq!(f.label_service.get_uri_basename_label(&uri1), "END");
    }

    #[test]
    fn mulitple_authority() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        for (authority, label) in [("not_matching_but_long", "first"), ("microsof*", "second"), ("mi*", "third")] {
            f.label_service.register_formatter(
                ResourceLabelFormatter {
                    scheme: "vscode".to_string(),
                    authority: Some(authority.to_string()),
                    formatting: formatting(label, LabelSeparator::Slash),
                    ..ResourceLabelFormatter::default()
                }
                .into(),
            );
        }

        // Make sure the most specific authority is picked
        let uri1 = parse("vscode://microsoft.com/1/2/3/4/5");
        assert_eq!(label_relative(&f.label_service, &uri1, false), "second");
        assert_eq!(f.label_service.get_uri_basename_label(&uri1), "second");
    }

    /// O formatador de `custom query*`: `LABEL${query.prefix}: ${query.path}/END`.
    fn register_query_formatter(f: &Fixture) {
        f.label_service.register_formatter(scheme_formatter(
            "vscode",
            ResourceLabelFormatting {
                tildify: Some(true),
                normalize_drive_letter: Some(true),
                ..formatting("LABEL${query.prefix}: ${query.path}/END", LabelSeparator::Slash)
            },
        ));
    }

    #[test]
    fn custom_query() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        register_query_formatter(&f);

        let uri1 = parse(&format!("vscode://microsoft.com/1/2/3/4/5?{}", encode_component(r#"{"prefix":"prefix","path":"path"}"#)));
        assert_eq!(label_relative(&f.label_service, &uri1, false), "LABELprefix: path/END");
    }

    #[test]
    fn custom_query_without_value() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        register_query_formatter(&f);

        let uri1 = parse(&format!("vscode://microsoft.com/1/2/3/4/5?{}", encode_component(r#"{"path":"path"}"#)));
        assert_eq!(label_relative(&f.label_service, &uri1, false), "LABEL: path/END");
    }

    #[test]
    fn custom_query_without_query_json() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        register_query_formatter(&f);

        let uri1 = parse("vscode://microsoft.com/1/2/3/4/5?path=foo");
        assert_eq!(label_relative(&f.label_service, &uri1, false), "LABEL: /END");
    }

    #[test]
    fn custom_query_without_query() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        register_query_formatter(&f);

        let uri1 = parse("vscode://microsoft.com/1/2/3/4/5");
        assert_eq!(label_relative(&f.label_service, &uri1, false), "LABEL: /END");
    }

    /// `{ formatting: { label: `${path} (${scheme})`, separator: '/' }, scheme }`.
    fn make_formatter(scheme: &str) -> ResourceLabelFormatter {
        ResourceLabelFormatter { scheme: scheme.to_string(), formatting: formatting(&format!("${{path}} ({scheme})"), LabelSeparator::Slash), ..ResourceLabelFormatter::default() }
    }

    /// O mesmo formatador como objeto dinâmico, escrito por extenso.
    fn formatter_object(scheme: &str) -> Dynamic {
        Dynamic::object([
            ("formatting", Dynamic::object([("label", Dynamic::string(&format!("${{path}} ({scheme})"))), ("separator", Dynamic::string("/"))])),
            ("scheme", Dynamic::string(scheme)),
        ])
    }

    /// O texto canônico de um valor JSON, com as chaves dos objetos ordenadas.
    fn canonical(value: &Value) -> String {
        match value {
            Value::Null => "null".to_string(),
            Value::Bool(flag) => flag.to_string(),
            Value::Number(number) => number.to_string(),
            Value::String(text) => format!("{:?}", text.to_string_lossy()),
            Value::Array(items) => format!("[{}]", items.iter().map(canonical).collect::<Vec<_>>().join(",")),
            Value::Object(object) => {
                let mut entries: Vec<(String, String)> = object.iter().map(|(name, item)| (name.to_string_lossy(), canonical(item))).collect();
                entries.sort();
                format!("{{{}}}", entries.iter().map(|(name, item)| format!("{name:?}:{item}")).collect::<Vec<_>>().join(","))
            }
        }
    }

    /// `assert.deepStrictEqual(m, { formatters: [...] })`, ou `{}` sem a lista.
    fn assert_cached_formatters(m: &Dynamic, expected: Option<Vec<Dynamic>>) {
        let expected = Dynamic::object(expected.map(|formatters| ("formatters", Dynamic::array(formatters))));
        assert_eq!(canonical(&to_json(m).expect("objeto")), canonical(&to_json(&expected).expect("objeto")));
    }

    #[test]
    fn label_caching() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let m = Dynamic::Object(Memento::new("cachedResourceLabelFormatters2", storage_service(&f.storage_service)).get_memento(StorageScope::Profile, StorageTarget::Machine));
        assert_cached_formatters(&m, None);

        // registers a new formatter:
        f.label_service.register_cached_formatter(make_formatter("a"));
        assert_cached_formatters(&m, Some(vec![formatter_object("a")]));

        // registers a 2nd formatter:
        f.label_service.register_cached_formatter(make_formatter("b"));
        assert_cached_formatters(&m, Some(vec![formatter_object("b"), formatter_object("a")]));

        // promotes a formatter on re-register:
        f.label_service.register_cached_formatter(make_formatter("a"));
        assert_cached_formatters(&m, Some(vec![formatter_object("a"), formatter_object("b")]));

        // no-ops if already in first place:
        f.label_service.register_cached_formatter(make_formatter("a"));
        assert_cached_formatters(&m, Some(vec![formatter_object("a"), formatter_object("b")]));

        // limits the cache:
        for i in 0..100 {
            f.label_service.register_cached_formatter(make_formatter(&format!("i{i}")));
        }
        let mut expected: Vec<Dynamic> = Vec::new();
        for i in 50..100 {
            expected.insert(0, formatter_object(&format!("i{i}")));
        }
        assert_cached_formatters(&m, Some(expected));

        if let Dynamic::Object(memento) = &m {
            memento.delete(&JsString::from("formatters"));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// multi-root workspace
// ---------------------------------------------------------------------------------------------

mod multi_root_workspace {
    use super::*;

    /// O `setup` da suíte; o `teardown` (`disposables.clear()`) é o `Drop`.
    struct MultiRoot {
        label_service: Rc<LabelService>,
        disposables: Rc<DisposableStore>,
    }

    impl Drop for MultiRoot {
        fn drop(&mut self) {
            self.disposables.dispose();
        }
    }

    /// `new LabelService(TestEnvironmentService, context, new TestPathService(), new TestRemoteAgentService(),
    /// disposables.add(new TestStorageService()), disposables.add(new TestLifecycleService()), uriIdentityService)`.
    fn label_service_for(disposables: &DisposableStore, context: Rc<TestContextService>, path_service: Rc<TestPathService>) -> Rc<LabelService> {
        let storage = disposables.add_rc(TestStorageService::new());
        let lifecycle = disposables.add_rc(TestLifecycleService::new());
        disposables.add_rc(new_label_service(context, path_service, &storage, lifecycle, uri_identity_service()))
    }

    fn setup() -> MultiRoot {
        let sources = uri_file("folder1/src");
        let tests = uri_file("folder1/test");
        let other = uri_file("folder2");

        let disposables = DisposableStore::new();
        let workspace = new_test_workspace(
            "test-workspace",
            vec![
                WorkspaceFolder::new(sources, "Sources", 0, None),
                WorkspaceFolder::new(tests, "Tests", 1, None),
                WorkspaceFolder::new(other.clone(), ext_uri().basename(&other, None), 2, None),
            ],
            None,
            None,
        );
        let label_service = label_service_for(&disposables, TestContextService::new(Some(workspace), None), TestPathService::new(None, None));
        MultiRoot { label_service, disposables }
    }

    #[test]
    fn labels_of_files_in_multiroot_workspaces_are_the_foldername_followed_by_offset_from_the_folder() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        f.label_service.register_formatter(scheme_formatter(
            "file",
            ResourceLabelFormatting {
                tildify: Some(false),
                normalize_drive_letter: Some(false),
                authority_prefix: Some("//".to_string()),
                workspace_suffix: Some(String::new()),
                ..formatting("${authority}${path}", LabelSeparator::Slash)
            },
        ));

        let tests = [
            ("folder1/src/file", "Sources \u{2022} file"),
            ("folder1/src/folder/file", "Sources \u{2022} folder/file"),
            ("folder1/src", "Sources"),
            ("folder1/other", "/folder1/other"),
            ("folder2/other", "folder2 \u{2022} other"),
        ];

        for (path, expected) in tests {
            let generated = label_relative(&f.label_service, &uri_file(path), true);
            assert_eq!(generated, expected);
        }
    }

    #[test]
    fn labels_with_context_after_path() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        f.label_service.register_formatter(scheme_formatter("file", formatting("${path} (${scheme})", LabelSeparator::Slash)));

        let tests = [
            ("folder1/src/file", "Sources \u{2022} file (file)"),
            ("folder1/src/folder/file", "Sources \u{2022} folder/file (file)"),
            ("folder1/src", "Sources"),
            ("folder1/other", "/folder1/other (file)"),
            ("folder2/other", "folder2 \u{2022} other (file)"),
        ];

        for (path, expected) in tests {
            let generated = label_relative(&f.label_service, &uri_file(path), true);
            assert_eq!(generated, expected, "{path}");
        }
    }

    #[test]
    fn strip_path_starting_separator() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        f.label_service.register_formatter(scheme_formatter(
            "file",
            ResourceLabelFormatting { strip_path_starting_separator: Some(true), ..formatting("${path}", LabelSeparator::Slash) },
        ));

        let tests = [("folder1/src/file", "Sources \u{2022} file"), ("other/blah", "other/blah")];

        for (path, expected) in tests {
            let generated = label_relative(&f.label_service, &uri_file(path), true);
            assert_eq!(generated, expected, "{path}");
        }
    }

    #[test]
    fn strip_path_segments_strips_leading_path_segments() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        f.label_service.register_formatter(scheme_formatter(
            "vscode-agent-host",
            ResourceLabelFormatting { strip_path_segments: Some(2), ..formatting("${path}", LabelSeparator::Slash) },
        ));

        let uri = from_components(Some("vscode-agent-host"), Some("my-server"), Some("/file//home/user/project/file.ts"));
        let generated = label_relative(&f.label_service, &uri, false);
        assert_eq!(generated, "/home/user/project/file.ts");
    }

    #[test]
    fn strip_path_segments_combined_with_strip_path_starting_separator() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        f.label_service.register_formatter(scheme_formatter(
            "vscode-agent-host",
            ResourceLabelFormatting { strip_path_segments: Some(2), strip_path_starting_separator: Some(true), ..formatting("${path}", LabelSeparator::Slash) },
        ));

        let uri = from_components(Some("vscode-agent-host"), Some("my-server"), Some("/file//home/user/file.ts"));
        let generated = label_relative(&f.label_service, &uri, false);
        assert_eq!(generated, "home/user/file.ts");
    }

    #[test]
    fn strip_path_segments_with_fewer_segments_than_requested() {
        let f = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        f.label_service.register_formatter(scheme_formatter(
            "test-strip",
            ResourceLabelFormatting { strip_path_segments: Some(5), ..formatting("${path}", LabelSeparator::Slash) },
        ));

        let uri = from_components(Some("test-strip"), None, Some("/a/b"));
        let generated = label_relative(&f.label_service, &uri, false);
        // Should strip as many as possible without crashing
        assert_eq!(generated, "/b");
    }

    #[test]
    fn relative_label_without_formatter() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut f = setup();
        let root_folder = parse("myscheme://myauthority/");

        f.label_service = label_service_for(
            &f.disposables,
            TestContextService::new(Some(new_test_workspace("test-workspace", vec![WorkspaceFolder::new(root_folder.clone(), "FSProotFolder", 0, None)], None, None)), None),
            TestPathService::new(None, Some(&root_folder.scheme().to_string_lossy())),
        );

        let generated = label_relative(&f.label_service, &parse("myscheme://myauthority/some/folder/test.txt"), true);
        if is_windows() {
            assert_eq!(generated, "some\\folder\\test.txt");
        } else {
            assert_eq!(generated, "some/folder/test.txt");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// workspace at FSP root
// ---------------------------------------------------------------------------------------------

mod workspace_at_fsp_root {
    use super::*;

    fn setup() -> Rc<LabelService> {
        let root_folder = parse("myscheme://myauthority/");

        let storage = TestStorageService::new();
        let label_service = new_label_service(
            TestContextService::new(Some(new_test_workspace("test-workspace", vec![WorkspaceFolder::new(root_folder, "FSProotFolder", 0, None)], None, None)), None),
            TestPathService::new(None, None),
            &storage,
            TestLifecycleService::new(),
            uri_identity_service(),
        );
        label_service.register_formatter(scheme_formatter(
            "myscheme",
            ResourceLabelFormatting {
                tildify: Some(false),
                normalize_drive_letter: Some(false),
                workspace_suffix: Some(String::new()),
                authority_prefix: Some(String::new()),
                strip_path_starting_separator: Some(false),
                ..formatting("${scheme}://${authority}${path}", LabelSeparator::Slash)
            },
        ));
        label_service
    }

    #[test]
    fn non_relative_label() {
        let label_service = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();

        let tests = [
            ("myscheme://myauthority/myFile1.txt", "myscheme://myauthority/myFile1.txt"),
            ("myscheme://myauthority/folder/myFile2.txt", "myscheme://myauthority/folder/myFile2.txt"),
        ];

        for (uri_string, expected) in tests {
            let generated = label_relative(&label_service, &parse(uri_string), false);
            assert_eq!(generated, expected);
        }
    }

    #[test]
    fn relative_label() {
        let label_service = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();

        let tests = [("myscheme://myauthority/myFile1.txt", "myFile1.txt"), ("myscheme://myauthority/folder/myFile2.txt", "folder/myFile2.txt")];

        for (uri_string, expected) in tests {
            let generated = label_relative(&label_service, &parse(uri_string), true);
            assert_eq!(generated, expected);
        }
    }

    #[test]
    fn relative_label_with_explicit_path_separator() {
        let label_service = setup();
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();

        let uri = parse("myscheme://myauthority/some/folder/test.txt");
        let with_separator = |separator: LabelSeparator| label_service.get_uri_label(&uri, &UriLabelOptions { relative: Some(true), separator: Some(separator), ..UriLabelOptions::default() });
        assert_eq!(with_separator(LabelSeparator::Slash), "some/folder/test.txt");

        assert_eq!(with_separator(LabelSeparator::Backslash), "some\\folder\\test.txt");
    }
}
