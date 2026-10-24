//! Porte de `src/vs/workbench/test/browser/parts/editor/breadcrumbModel.test.ts` (suíte `Breadcrumb Model`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `teardown(() =>
//!   model.dispose())` é o `add_rc` do guarda, que descarta o modelo ao fim.
//! - Os serviços que a suíte cria uma vez (`workspaceService`, `labelService`, `configService`...) são criados por caso
//!   ([`Suite`]), com o mesmo conteúdo: cada caso do original registra e desfaz os seus formatadores, então nada passa
//!   de um caso para o outro.
//! - `mock<IOutlineService>()` é o [`MockOutlineService`], que falha em qualquer chamada (nenhum caso passa um editor,
//!   então o serviço nunca é lido), e o `mock<IUriIdentityService>()` com `extUri` é o [`TestUriIdentityService`] (o
//!   `asCanonicalUri`, que o `mock` deixa indefinido e o modelo não chama, devolve o próprio URI).
//! - `class extends TestConfigurationService` com `getValue` e `updateValue` sobrescritos é o
//!   `stub_get_value_with`: `breadcrumbs.filePath` e `breadcrumbs.symbolPath` valem `'on'`, e o `super.getValue` das
//!   outras chaves, num serviço vazio, é `undefined`. O `updateValue` que resolve é o padrão do serviço de teste.
//! - `Reflect.get(workspace, '_onDidChangeWorkspaceFolders')` é o
//!   `TestContextService::on_did_change_workspace_folders_emitter`.
//! - `element.label ?? element.uri.path.slice(element.uri.path.lastIndexOf('/') + 1)` e
//!   `element.uri.path.split('/').at(-1)` são o mesmo trecho depois da última barra ([`last_segment`]).
//! - `assert.deepStrictEqual` de objetos e listas de objetos é a comparação de tuplas e listas de tuplas.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::{URI, UriComponents};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::files::FileKind;
use je_platform::label::label::{ILabelService, LabelFormatter, LabelSeparator, ResourceLabelFormatter, ResourceLabelFormatting};
use je_platform::resources::host_environment;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::{IWorkspaceContextService, WorkspaceFolder, WorkspaceFoldersChangeEvent, new_test_workspace};
use je_workbench::browser::parts::editor::breadcrumbs_model::{BreadcrumbsElement, BreadcrumbsModel, BreadcrumbsModelServices, FileElement};
use je_workbench::common::editor::IEditorPane;
use je_workbench::services::outline::outline::{IOutline, IOutlineCreator, IOutlineService, OutlineTarget};
use je_workbench::services::workspaces::workspace_folder_label_service::IWorkspaceFolderLabelService;
use je_workbench::test::browser::workbench_test_services::MockLabelService;
use je_workbench::test::common::workbench_test_services::TestContextService;

/// `new class extends mock<IWorkspaceFolderLabelService>() { getWorkspaceFolderLabel(folder) { ... } }`.
struct TestWorkspaceFolderLabelService;

impl IWorkspaceFolderLabelService for TestWorkspaceFolderLabelService {
    fn get_workspace_folder_label(&self, folder: &WorkspaceFolder, _verbose: Option<bool>) -> Option<String> {
        Some(last_segment(&folder.uri))
    }
}

/// `new class extends mock<IOutlineService>() { }`: toda chamada falha, como o `mock` (que não tem os métodos).
struct MockOutlineService;

fn not_implemented() -> ! {
    errors::throw(Error::new("Method not implemented."))
}

impl IOutlineService for MockOutlineService {
    fn on_did_change(&self) -> Event<()> {
        not_implemented()
    }

    fn can_create_outline(&self, _editor: &dyn IEditorPane) -> bool {
        not_implemented()
    }

    fn create_outline(&self, _editor: Rc<dyn IEditorPane>, _target: OutlineTarget, _token: CancellationToken) -> Promise<Option<Rc<dyn IOutline>>> {
        not_implemented()
    }

    fn register_outline_creator(&self, _creator: Rc<dyn IOutlineCreator>) -> DisposableRef {
        not_implemented()
    }

    fn notify_outline_changed(&self) {
        not_implemented()
    }
}

/// `new class extends mock<IUriIdentityService>() { override readonly extUri = ... }`.
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

/// O trecho do caminho depois da última barra.
fn last_segment(uri: &URI) -> String {
    let path = uri.path().to_string_lossy();
    path[path.rfind('/').map_or(0, |index| index + 1)..].to_owned()
}

/// `element.label ?? <último trecho do caminho>`.
fn display_name(element: &FileElement) -> String {
    element.label.clone().unwrap_or_else(|| last_segment(&element.uri))
}

fn parse(value: &str) -> URI {
    URI::parse(value, false)
}

/// `URI.from({ scheme, authority, path, query })`.
fn uri_from(scheme: &str, authority: Option<&str>, path: &str, query: Option<&str>) -> URI {
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

fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `new WorkspaceFolder({ uri: URI.parse(uri), name, index })`.
fn workspace_folder(uri: &str, name: &str, index: usize) -> WorkspaceFolder {
    WorkspaceFolder::new(parse(uri), name, index, None)
}

/// `new TestContextService(new Workspace('ffff', folders, configuration?))`.
fn context_service(folders: Vec<WorkspaceFolder>, configuration: Option<&str>) -> Rc<TestContextService> {
    TestContextService::new(Some(new_test_workspace("ffff", folders, configuration.map(parse), None)), None)
}

/// `{ scheme, authority?, home, formatting: { label, separator: '/' } }`.
fn home_formatter(scheme: &str, authority: Option<&str>, home: &str, label: &str) -> LabelFormatter {
    LabelFormatter::from(ResourceLabelFormatter {
        scheme: scheme.to_owned(),
        authority: authority.map(str::to_owned),
        home: Some(home.to_owned()),
        priority: None,
        formatting: ResourceLabelFormatting { label: label.to_owned(), separator: LabelSeparator::Slash, ..ResourceLabelFormatting::default() },
    })
}

/// Os serviços que a suíte do original cria uma vez, mais o guarda de vazamentos.
struct Suite {
    guard: LeakGuard,
    workspace_service: Rc<TestContextService>,
    label_service: Rc<MockLabelService>,
    uri_identity_service: Rc<TestUriIdentityService>,
    configuration_service: Rc<TestConfigurationService>,
}

impl Suite {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let uri_identity_service = Rc::new(TestUriIdentityService { ext_uri: ExtUri::new(host_environment(), |_| false) });
        let label_service = MockLabelService::new(Some(uri_identity_service.ext_uri()));
        let configuration_service = TestConfigurationService::new(None);
        configuration_service.stub_get_value_with(|section, _overrides| match section {
            Some("breadcrumbs.filePath") | Some("breadcrumbs.symbolPath") => Dynamic::string("on"),
            _ => Dynamic::Undefined,
        });
        Self {
            guard,
            workspace_service: context_service(vec![workspace_folder("foo:/bar/baz/ws", "ws", 0)], None),
            label_service,
            uri_identity_service,
            configuration_service,
        }
    }

    /// `createModel(resource, workspace, labels, uriIdentity)`, com os serviços da suíte.
    fn create_model(&self, resource: URI) -> Rc<BreadcrumbsModel> {
        self.create_model_with(resource, self.workspace_service.clone(), self.label_service.clone(), self.uri_identity_service.clone())
    }

    fn create_model_with(
        &self,
        resource: URI,
        workspace: Rc<TestContextService>,
        labels: Rc<MockLabelService>,
        uri_identity: Rc<TestUriIdentityService>,
    ) -> Rc<BreadcrumbsModel> {
        let configuration_service: Rc<dyn IConfigurationService> = self.configuration_service.clone();
        let workspace_service: Rc<dyn IWorkspaceContextService> = workspace;
        let label_service: Rc<dyn ILabelService> = labels;
        let uri_identity_service: Rc<dyn IUriIdentityService> = uri_identity;
        self.guard.add_rc(BreadcrumbsModel::new(
            resource,
            None,
            BreadcrumbsModelServices {
                configuration_service,
                workspace_service,
                workspace_folder_label_service: Rc::new(TestWorkspaceFolderLabelService),
                outline_service: Rc::new(MockOutlineService),
                label_service,
                uri_identity_service,
            },
        ))
    }
}

/// `model.getElements() as FileElement[]`.
fn file_elements(model: &BreadcrumbsModel) -> Vec<FileElement> {
    model
        .get_elements()
        .into_iter()
        .map(|element| match element {
            BreadcrumbsElement::File(element) => element,
            BreadcrumbsElement::Outline(_) => panic!("o elemento não é um FileElement"),
        })
        .collect()
}

/// `{ uri: element.uri.toString(), kind: element.kind }` de cada elemento.
fn uris_and_kinds(model: &BreadcrumbsModel) -> Vec<(String, FileKind)> {
    file_elements(model).iter().map(|element| (uri_text(&element.uri), element.kind)).collect()
}

/// `{ uri: element.uri.toString(), kind: element.kind, label: element.label }` de cada elemento.
fn uris_kinds_and_labels(model: &BreadcrumbsModel) -> Vec<(String, FileKind, Option<String>)> {
    file_elements(model).iter().map(|element| (uri_text(&element.uri), element.kind, element.label.clone())).collect()
}

/// `{ name: element.label ?? <último trecho>, kind: element.kind }` de cada elemento.
fn names_and_kinds(model: &BreadcrumbsModel) -> Vec<(String, FileKind)> {
    file_elements(model).iter().map(|element| (display_name(element), element.kind)).collect()
}

/// `element.label ?? <último trecho>` de cada elemento.
fn names(model: &BreadcrumbsModel) -> Vec<String> {
    file_elements(model).iter().map(display_name).collect()
}

/// O último trecho do caminho de cada elemento.
fn last_segments(model: &BreadcrumbsModel) -> Vec<String> {
    file_elements(model).iter().map(|element| last_segment(&element.uri)).collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn file_element_equality_includes_the_rendered_label() {
    let suite = Suite::new();
    let _model = suite.create_model(parse("foo:/bar/baz/ws/file.ts"));
    let uri = parse("foo:/worktrees/project-feature");

    let same_label = FileElement::new(uri.clone(), FileKind::RootFolder, Some("project".to_owned())).equals(&FileElement::new(uri.clone(), FileKind::RootFolder, Some("project".to_owned())));
    let changed_label =
        FileElement::new(uri.clone(), FileKind::RootFolder, Some("project".to_owned())).equals(&FileElement::new(uri, FileKind::RootFolder, Some("renamed-project".to_owned())));

    assert_eq!((same_label, changed_label), (true, false));
}

#[test]
fn only_uri_inside_workspace() {
    let suite = Suite::new();
    let model = suite.create_model(parse("foo:/bar/baz/ws/some/path/file.ts"));
    let elements = file_elements(&model);

    assert_eq!(elements.len(), 3);
    let (one, two, three) = (&elements[0], &elements[1], &elements[2]);
    assert_eq!(one.kind, FileKind::Folder);
    assert_eq!(two.kind, FileKind::Folder);
    assert_eq!(three.kind, FileKind::File);
    assert_eq!(uri_text(&one.uri), "foo:/bar/baz/ws/some");
    assert_eq!(uri_text(&two.uri), "foo:/bar/baz/ws/some/path");
    assert_eq!(uri_text(&three.uri), "foo:/bar/baz/ws/some/path/file.ts");
}

#[test]
fn display_uri_matters_for_file_element() {
    let suite = Suite::new();
    let model = suite.create_model(parse("foo:/bar/baz/ws/some/PATH/file.ts"));
    let elements = file_elements(&model);

    assert_eq!(elements.len(), 3);
    let (one, two, three) = (&elements[0], &elements[1], &elements[2]);
    assert_eq!(one.kind, FileKind::Folder);
    assert_eq!(two.kind, FileKind::Folder);
    assert_eq!(three.kind, FileKind::File);
    assert_eq!(uri_text(&one.uri), "foo:/bar/baz/ws/some");
    assert_eq!(uri_text(&two.uri), "foo:/bar/baz/ws/some/PATH");
    assert_eq!(uri_text(&three.uri), "foo:/bar/baz/ws/some/PATH/file.ts");
}

#[test]
fn only_uri_outside_workspace() {
    let suite = Suite::new();
    let model = suite.create_model(parse("foo:/outside/file.ts"));
    let elements = file_elements(&model);

    assert_eq!(elements.len(), 2);
    let (one, two) = (&elements[0], &elements[1]);
    assert_eq!(one.kind, FileKind::Folder);
    assert_eq!(two.kind, FileKind::File);
    assert_eq!(uri_text(&one.uri), "foo:/outside");
    assert_eq!(uri_text(&two.uri), "foo:/outside/file.ts");
}

#[test]
fn shows_only_the_path_relative_to_a_contributed_resource_label_home() {
    let suite = Suite::new();
    let root = je_platform::resources::uri_file("/Users/test/.copilot/session-state/5ec17bb7-5596-41c5-9d24-4787d8b0a698");
    let registration = suite.label_service.register_formatter(home_formatter(&root.scheme().to_string_lossy(), None, &root.path().to_string_lossy(), "Copilot/Session"));
    let resource = je_platform::resources::uri_file("/Users/test/.copilot/session-state/5ec17bb7-5596-41c5-9d24-4787d8b0a698/folder/file.md");
    let model = suite.create_model(resource);

    assert_eq!(
        (model.is_relative(), names_and_kinds(&model)),
        (
            true,
            vec![
                ("Copilot".to_owned(), FileKind::RootFolder),
                ("Session".to_owned(), FileKind::Folder),
                ("folder".to_owned(), FileKind::Folder),
                ("file.md".to_owned(), FileKind::File),
            ]
        )
    );
    registration.dispose();
}

#[test]
fn shows_only_the_path_relative_to_a_windows_home_with_different_casing() {
    let suite = Suite::new();
    let windows_uri_identity_service = Rc::new(TestUriIdentityService { ext_uri: ExtUri::new(host_environment(), |uri| uri.scheme() == schemas::FILE) });
    let windows_label_service = MockLabelService::new(Some(windows_uri_identity_service.ext_uri()));
    let home = uri_from(schemas::FILE, None, "/c:/Users/test/.copilot/session-state/session-id", None);
    let registration = windows_label_service.register_formatter(home_formatter(&home.scheme().to_string_lossy(), None, &home.path().to_string_lossy(), "Copilot/Session"));
    let resource = uri_from(schemas::FILE, None, "/C:/Users/Test/.copilot/session-state/session-id/files/result.html", None);
    let model = suite.create_model_with(resource, suite.workspace_service.clone(), windows_label_service, windows_uri_identity_service);

    assert_eq!(names(&model), strings(&["Copilot", "Session", "files", "result.html"]));

    registration.dispose();
}

#[test]
fn keeps_the_full_path_when_home_casing_differs_on_a_case_sensitive_scheme() {
    let suite = Suite::new();
    let home = parse("case-sensitive:/Users/test/session-id");
    let registration = suite.label_service.register_formatter(home_formatter(&home.scheme().to_string_lossy(), None, &home.path().to_string_lossy(), "Session"));
    let model = suite.create_model(parse("case-sensitive:/Users/Test/session-id/file.md"));

    assert_eq!(last_segments(&model), strings(&["Users", "Test", "session-id", "file.md"]));

    registration.dispose();
}

#[test]
fn updates_when_a_resource_label_home_is_contributed_after_model_creation() {
    let suite = Suite::new();
    let resource = je_platform::resources::uri_file("/Users/test/.copilot/session-state/5ec17bb7-5596-41c5-9d24-4787d8b0a698/file.md");
    let model = suite.create_model(resource);
    let root = je_platform::resources::uri_file("/Users/test/.copilot/session-state/5ec17bb7-5596-41c5-9d24-4787d8b0a698");
    let registration = suite.label_service.register_formatter(home_formatter(&root.scheme().to_string_lossy(), None, &root.path().to_string_lossy(), "Copilot/Session"));

    assert_eq!(
        names_and_kinds(&model),
        vec![("Copilot".to_owned(), FileKind::RootFolder), ("Session".to_owned(), FileKind::Folder), ("file.md".to_owned(), FileKind::File)]
    );
    registration.dispose();
}

#[test]
fn stops_at_a_resource_label_home_when_the_resource_has_a_query() {
    let suite = Suite::new();
    let home = uri_from("vscode-agent-host", Some("remote"), "/home/.copilot/session-state/session", None);
    let (scheme, authority, home_path) = (home.scheme().to_string_lossy(), home.authority().to_string_lossy(), home.path().to_string_lossy());
    let resource = uri_from(&scheme, Some(authority.as_str()), &format!("{home_path}/file.md"), Some("_ah=metadata"));
    let registration = suite.label_service.register_formatter(home_formatter(&scheme, Some(authority.as_str()), &home_path, "Copilot/Session"));
    let model = suite.create_model(resource);

    assert_eq!(names(&model), strings(&["Copilot", "Session", "file.md"]));
    registration.dispose();
}

#[test]
fn keeps_the_full_path_without_a_contributed_resource_label_home() {
    let suite = Suite::new();
    let resource = je_platform::resources::uri_file("/Users/test/.copilot/session-state/5ec17bb7-5596-41c5-9d24-4787d8b0a698/file.md");
    let model = suite.create_model(resource);

    assert_eq!(
        (model.is_relative(), last_segments(&model)),
        (false, strings(&["Users", "test", ".copilot", "session-state", "5ec17bb7-5596-41c5-9d24-4787d8b0a698", "file.md"]))
    );
}

#[test]
fn omits_workspace_root_in_single_root_vs_code_workspace() {
    let suite = Suite::new();
    let workspace = context_service(vec![workspace_folder("foo:/bar/baz/ws", "ws", 0)], Some("foo:/workspace.code-workspace"));
    let model = suite.create_model_with(parse("foo:/bar/baz/ws/file.ts"), workspace, suite.label_service.clone(), suite.uri_identity_service.clone());

    assert_eq!(uris_and_kinds(&model), vec![("foo:/bar/baz/ws/file.ts".to_owned(), FileKind::File)]);
}

#[test]
fn omits_workspace_root_in_single_root_sessions_window() {
    let suite = Suite::new();
    let workspace = context_service(vec![workspace_folder("foo:/bar/baz/ws", "ws (branch)", 0)], Some("foo:/workspace.code-workspace"));
    let model = suite.create_model_with(parse("foo:/bar/baz/ws/some/file.ts"), workspace, suite.label_service.clone(), suite.uri_identity_service.clone());

    assert_eq!(
        uris_and_kinds(&model),
        vec![("foo:/bar/baz/ws/some".to_owned(), FileKind::Folder), ("foo:/bar/baz/ws/some/file.ts".to_owned(), FileKind::File)]
    );
}

#[test]
fn keeps_workspace_root_when_it_is_the_breadcrumb_resource() {
    let suite = Suite::new();
    let workspace = context_service(vec![workspace_folder("foo:/bar/baz/ws", "ws (branch)", 0)], Some("foo:/workspace.code-workspace"));
    let model = suite.create_model_with(parse("foo:/bar/baz/ws"), workspace, suite.label_service.clone(), suite.uri_identity_service.clone());

    assert_eq!(uris_kinds_and_labels(&model), vec![("foo:/bar/baz/ws".to_owned(), FileKind::RootFolder, Some("ws".to_owned()))]);
}

#[test]
fn shows_plain_workspace_root_in_multi_root_sessions_window() {
    let suite = Suite::new();
    let workspace = context_service(
        vec![workspace_folder("foo:/worktrees/project-feature", "project (feature)", 0), workspace_folder("foo:/worktrees/docs-feature", "docs (feature)", 1)],
        Some("foo:/workspace.code-workspace"),
    );
    let model = suite.create_model_with(parse("foo:/worktrees/docs-feature/guide.md"), workspace, suite.label_service.clone(), suite.uri_identity_service.clone());

    assert_eq!(
        uris_kinds_and_labels(&model),
        vec![
            ("foo:/worktrees/docs-feature".to_owned(), FileKind::RootFolder, Some("docs-feature".to_owned())),
            ("foo:/worktrees/docs-feature/guide.md".to_owned(), FileKind::File, None),
        ]
    );
}

#[test]
fn updates_workspace_root_and_label_when_folders_change() {
    let suite = Suite::new();
    let first_folder = workspace_folder("foo:/worktrees/project-feature", "project (feature)", 0);
    let workspace = context_service(vec![first_folder.clone()], Some("foo:/workspace.code-workspace"));
    let model = suite.create_model_with(parse("foo:/worktrees/project-feature/file.ts"), workspace.clone(), suite.label_service.clone(), suite.uri_identity_service.clone());

    let second_folder = workspace_folder("foo:/worktrees/docs-feature", "docs (feature)", 1);
    workspace.set_workspace(new_test_workspace("ffff", vec![first_folder, second_folder.clone()], Some(parse("foo:/workspace.code-workspace")), None));
    workspace.on_did_change_workspace_folders_emitter().fire(WorkspaceFoldersChangeEvent { added: vec![second_folder], removed: Vec::new(), changed: Vec::new() });

    assert_eq!(
        uris_kinds_and_labels(&model),
        vec![
            ("foo:/worktrees/project-feature".to_owned(), FileKind::RootFolder, Some("project-feature".to_owned())),
            ("foo:/worktrees/project-feature/file.ts".to_owned(), FileKind::File, None),
        ]
    );
}
