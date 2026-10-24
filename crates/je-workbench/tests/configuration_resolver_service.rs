//! Porte de `src/vs/workbench/services/configurationResolver/test/electron-browser/configurationResolverService.test.ts`
//! (suítes `Configuration Resolver Service` e `ConfigurationResolverExpression`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **Casos `async`** rodam em `run_with_faked_timers`; o `setup` é [`Fixture::new`] e o guarda de vazamento é o
//!   `ensure_no_disposables_are_leaked_in_test_suite()`.
//! - **Dublês locais do arquivo** (`TestEditorServiceWithActiveEditor`, `MockCommandService`, `MockLabelService`,
//!   `MockPathService`, `MockInputsConfigurationService`): o último é o `TestConfigurationService` com os ganchos
//!   `stub_get_value_with` e `stub_inspect_with` (o `override getValue`/`inspect` da subclasse).
//! - **`assert.rejects(...)` sem `await`** do original é aqui um `await` com `is_err()`.
//! - **`deepStrictEqual`** é `objects::equals` sobre `Dynamic` (e a ordem das chaves, quando o original a confere).
//! - **Ramos de Windows** são o `native` que troca `/` por `\`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle;
use je_base::lifecycle::DisposableRef;
use je_base::objects;
use je_base::path::PathModule;
use je_base::platform::{OperatingSystem, Platform};
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::selection::Selection;
use je_platform::commands::commands::{CommandEvent, ICommandService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::{TestConfigurationService, TestConfigurationValue};
use je_platform::configuration::variables::ConfigurationVariable;
use je_platform::configuration::ConfigurationValue;
use je_platform::label::label::{
    FormatterChangeEvent, ILabelService, LabelFormatter, LabelSeparator, ResourceLabelFormatter, UriLabelOptions, WorkspaceLabelOptions, WorkspaceLabelTarget,
};
use je_platform::path::path_service::{IPathService, IResourcePathProvider, PathFlavor};
use je_platform::resources::{host_environment, uri_file};
use je_platform::workspace::{WorkspaceFolder, test_workspace};
use je_workbench::common::editor::{EditorInputOrUntyped, UntypedEditorInput};
use je_workbench::services::configuration_resolver::browser::base_configuration_resolver_service::{BaseConfigurationResolverService, ResolverAppContext};
use je_workbench::services::configuration_resolver::common::configuration_resolver::IConfigurationResolverService;
use je_workbench::services::configuration_resolver::common::configuration_resolver_expression::{ConfigurationResolverExpression, Replacement};
use je_workbench::services::editor::editor_service::{ActiveTextEditorControl, IActiveCodeEditor, IEditorService};
use je_workbench::services::extensions::extensions::IExtensionDescription;
use je_workbench::test::browser::workbench_test_services::{TestExtensionService, TestQuickInputService};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestStorageService};

const MOCK_LINE_NUMBER: i32 = 10;

fn is_windows() -> bool {
    Platform::detect().is_windows
}

/// O caminho como o `platform.isWindows` o escreve.
fn native(path: &str) -> String {
    if is_windows() { path.replace('/', "\\") } else { path.to_string() }
}

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

fn num(value: f64) -> Dynamic {
    Dynamic::Number(value)
}

fn obj<const N: usize>(entries: [(&str, Dynamic); N]) -> Dynamic {
    Dynamic::object(entries)
}

fn assert_deep_equal(actual: &Dynamic, expected: &Dynamic) {
    assert!(objects::equals(actual, expected), "esperado {expected:?}, veio {actual:?}");
}

fn code_editor() -> Rc<dyn IActiveCodeEditor> {
    struct FakeCodeEditor;
    impl IActiveCodeEditor for FakeCodeEditor {
        fn get_selection(&self) -> Option<Selection> {
            Some(Selection::new(MOCK_LINE_NUMBER, 1, MOCK_LINE_NUMBER, 10))
        }
        fn get_value_in_range(&self, _selection: &Selection) -> Option<JsString> {
            None
        }
        fn has_widget_focus(&self) -> bool {
            false
        }
    }
    Rc::new(FakeCodeEditor)
}

/// `TestEditorServiceWithActiveEditor`.
struct TestEditorServiceWithActiveEditor;

impl IEditorService for TestEditorServiceWithActiveEditor {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn je_workbench::common::editor::IEditorPane>> {
        Vec::new()
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        Some(EditorInputOrUntyped::Untyped(UntypedEditorInput::from_resource(URI::parse("file:///VSCode/workspaceLocation/file", false))))
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        Some(ActiveTextEditorControl::Code(code_editor()))
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        unreachable!("TestEditorServiceWithActiveEditor.activeTextEditorLanguageId não é exercitado por esta suíte")
    }

    fn open_editor(&self, _editor: UntypedEditorInput) -> Promise<Option<Rc<dyn je_workbench::common::editor::IEditorPane>>> {
        unreachable!("TestEditorServiceWithActiveEditor.openEditor não é exercitado por esta suíte")
    }

    /// Não é exercitado por esta suíte.
    fn find_editors(&self, _resource: &je_base::uri::URI, _options: Option<je_workbench::services::editor::editor_service::FindEditorOptions>) -> Vec<je_workbench::common::editor::EditorIdentifier> {
        unreachable!("TestEditorServiceWithActiveEditor.findEditors não é exercitado por esta suíte")
    }

    /// Não é exercitado por esta suíte.
    fn save(
        &self,
        _editors: Vec<je_workbench::common::editor::EditorIdentifier>,
        _options: Option<je_workbench::services::editor::editor_service::SaveEditorsOptions>,
    ) -> Promise<je_workbench::services::editor::editor_service::SaveEditorsResult> {
        unreachable!("TestEditorServiceWithActiveEditor.save não é exercitado por esta suíte")
    }

    /// Não é exercitado por esta suíte.
    fn save_all(&self, _options: Option<je_workbench::services::editor::editor_service::SaveAllEditorsOptions>) -> Promise<je_workbench::services::editor::editor_service::SaveEditorsResult> {
        unreachable!("TestEditorServiceWithActiveEditor.saveAll não é exercitado por esta suíte")
    }
}

/// `MockCommandService`.
struct MockCommandService {
    call_count: Cell<usize>,
}

impl ICommandService for MockCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.call_count.set(self.call_count.get() + 1);

        let mut result = format!("{command_id}-result");
        if let Some(first) = args.first()
            && !first.is_nullish()
            && let Dynamic::String(value) = first.get_str("value")
            && !value.is_empty()
        {
            result = value.to_string_lossy();
        }

        Promise::resolved(Dynamic::string(&result))
    }
}

fn not_implemented() -> ! {
    errors::throw(Error::new("Method not implemented."))
}

/// `MockLabelService`.
struct MockLabelService;

impl ILabelService for MockLabelService {
    fn get_uri_label(&self, resource: &URI, _options: &UriLabelOptions) -> String {
        let env = host_environment();
        PathModule::native(env.as_ref()).normalize(&je_platform::resources::fs_path(resource))
    }
    fn get_uri_basename_label(&self, _resource: &URI) -> String {
        not_implemented()
    }
    fn get_workspace_label(&self, _workspace: &WorkspaceLabelTarget, _options: Option<&WorkspaceLabelOptions>) -> String {
        not_implemented()
    }
    fn get_host_label(&self, _scheme: &str, _authority: Option<&str>) -> String {
        not_implemented()
    }
    fn get_host_tooltip(&self, _scheme: &str, _authority: Option<&str>) -> Option<String> {
        not_implemented()
    }
    fn get_separator(&self, _scheme: &str, _authority: Option<&str>) -> LabelSeparator {
        not_implemented()
    }
    fn get_uri_home(&self, _resource: &URI) -> Option<URI> {
        None
    }
    fn register_formatter(&self, _formatter: LabelFormatter) -> DisposableRef {
        not_implemented()
    }
    fn on_did_change_formatters(&self) -> Event<FormatterChangeEvent> {
        Event::none()
    }
    fn register_cached_formatter(&self, _formatter: ResourceLabelFormatter) -> DisposableRef {
        not_implemented()
    }
}

/// `MockPathService`.
struct MockPathService;

impl IPathService for MockPathService {
    fn path(&self) -> Promise<PathFlavor> {
        errors::throw(Error::new("Property not implemented"))
    }
    fn default_uri_scheme(&self) -> String {
        "file".to_string()
    }
    fn file_uri(&self, _path: &str) -> Promise<URI> {
        not_implemented()
    }
    fn user_home(&self) -> Promise<URI> {
        Promise::resolved(self.local_user_home())
    }
    fn local_user_home(&self) -> URI {
        uri_file("c:\\users\\username")
    }
    fn has_valid_basename(&self, _resource: &URI, _basename: Option<&str>) -> Promise<bool> {
        not_implemented()
    }
    fn has_valid_basename_for_os(&self, _resource: &URI, _os: OperatingSystem, _basename: Option<&str>) -> bool {
        not_implemented()
    }
    fn get_operating_system(&self, _resource: &URI) -> Promise<Option<OperatingSystem>> {
        not_implemented()
    }
    fn get_path(&self, _resource: &URI) -> Promise<Option<PathFlavor>> {
        not_implemented()
    }
    fn register_path_provider(&self, _scheme: &str, _provider: Rc<dyn IResourcePathProvider>) -> DisposableRef {
        lifecycle::none()
    }
    fn resolved_user_home(&self) -> Option<URI> {
        None
    }
}

/// `MockInputsConfigurationService`: o `TestConfigurationService` com `getValue` e `inspect` sobrescritos.
fn mock_inputs_configuration_service() -> Rc<TestConfigurationService> {
    let service = TestConfigurationService::new(None);
    service.stub_get_value_with(|section, _overrides| {
        if section != Some("tasks") {
            return Dynamic::Undefined;
        }
        obj([(
            "inputs",
            Dynamic::array([
                obj([("id", text("input1")), ("type", text("promptString")), ("description", text("Enterinput1")), ("default", text("default input1"))]),
                obj([
                    ("id", text("input2")),
                    ("type", text("pickString")),
                    ("description", text("Enterinput1")),
                    ("default", text("option2")),
                    ("options", Dynamic::array([text("option1"), text("option2"), text("option3")])),
                ]),
                obj([
                    ("id", text("input3")),
                    ("type", text("promptString")),
                    ("description", text("Enterinput3")),
                    ("default", text("default input3")),
                    ("provide", Dynamic::Bool(true)),
                    ("password", Dynamic::Bool(true)),
                ]),
                obj([("id", text("input4")), ("type", text("command")), ("command", text("command1")), ("args", obj([("value", text("arg for command"))]))]),
            ]),
        )])
    });
    service.stub_inspect_with(|_key, _overrides| {
        let value: Rc<dyn ConfigurationValue> = Rc::new(TestConfigurationValue { override_identifiers: Some(Vec::new()), ..TestConfigurationValue::default() });
        value
    });
    service
}

fn null_context() -> ResolverAppContext {
    ResolverAppContext { get_app_root: Rc::new(|| None), get_exec_path: Rc::new(|| None) }
}

fn env_variables() -> ProcessEnvironment {
    ProcessEnvironment::from([("key1".to_string(), "Value for key1".to_string()), ("key2".to_string(), "Value for key2".to_string())])
}

struct Fixture {
    guard: LeakGuard,
    service: Rc<dyn IConfigurationResolverService>,
    command_service: Rc<MockCommandService>,
    quick_input_service: Rc<TestQuickInputService>,
    extension_service: Rc<TestExtensionService>,
    workspace: WorkspaceFolder,
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let containing_workspace = test_workspace(&[URI::parse("file:///VSCode/workspaceLocation", false)]);
        let workspace = containing_workspace.folders()[0].clone();
        let command_service = Rc::new(MockCommandService { call_count: Cell::new(0) });
        let quick_input_service = TestQuickInputService::new();
        let extension_service = TestExtensionService::new();
        let service = Self::service_with(&guard, mock_inputs_configuration_service(), TestContextService::new(Some(containing_workspace), None), &command_service, &quick_input_service, &extension_service);
        Self { guard, service, command_service, quick_input_service, extension_service, workspace }
    }

    fn service_with(
        guard: &LeakGuard,
        configuration_service: Rc<TestConfigurationService>,
        context: Rc<TestContextService>,
        command_service: &Rc<MockCommandService>,
        quick_input_service: &Rc<TestQuickInputService>,
        extension_service: &Rc<TestExtensionService>,
    ) -> Rc<dyn IConfigurationResolverService> {
        let configuration_service: Rc<dyn IConfigurationService> = configuration_service;
        Rc::new(BaseConfigurationResolverService::new(
            null_context(),
            Promise::resolved(env_variables()),
            Rc::new(TestEditorServiceWithActiveEditor),
            configuration_service,
            command_service.clone(),
            context,
            quick_input_service.clone(),
            Rc::new(MockLabelService),
            Rc::new(MockPathService),
            extension_service.clone(),
            Rc::new(guard.add_rc(TestStorageService::new())),
        ))
    }

    /// `new TestConfigurationResolverService(nullContext, ..., configurationService, ..., new TestContextService(), ...)`.
    fn with_configuration(&self, configuration: Dynamic) -> Rc<dyn IConfigurationResolverService> {
        Self::service_with(
            &self.guard,
            TestConfigurationService::new(Some(configuration)),
            TestContextService::new(None, None),
            &self.command_service,
            &self.quick_input_service,
            &self.extension_service,
        )
    }

    async fn resolve(&self, folder: Option<&WorkspaceFolder>, value: &str) -> String {
        resolve_text(&self.service, folder, value).await
    }
}

async fn resolve_text(service: &Rc<dyn IConfigurationResolverService>, folder: Option<&WorkspaceFolder>, value: &str) -> String {
    match service.resolve_async(folder, &text(value)).await.expect("a resolução deveria funcionar") {
        Dynamic::String(resolved) => resolved.to_string_lossy(),
        other => panic!("esperava texto, veio {other:?}"),
    }
}

async fn assert_rejects(service: &Rc<dyn IConfigurationResolverService>, folder: Option<&WorkspaceFolder>, value: &str) {
    assert!(service.resolve_async(folder, &text(value)).await.is_err(), "{value} deveria ser rejeitado");
}

fn config_of<const N: usize>(entries: [(&str, Dynamic); N]) -> Dynamic {
    obj(entries)
}

fn font_config() -> Dynamic {
    config_of([("editor", obj([("fontFamily", text("foo"))])), ("terminal", obj([("integrated", obj([("fontFamily", text("bar"))]))]))])
}

fn attach_config(process_id: &str) -> Dynamic {
    obj([
        ("name", text("Attach to Process")),
        ("type", text("node")),
        ("request", text("attach")),
        ("processId", text(process_id)),
        ("port", num(5858.0)),
        ("sourceMaps", Dynamic::Bool(false)),
        ("outDir", Dynamic::Null),
    ])
}

fn attach_expected(process_id: &str) -> Dynamic {
    attach_config(process_id)
}

fn variables(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

fn expected_program() -> &'static str {
    let platform = Platform::detect();
    if platform.is_windows {
        "windows.exe"
    } else if platform.is_macintosh {
        "osx.sh"
    } else {
        "linux.sh"
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Configuration Resolver Service
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn substitute_one() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_eq!(f.resolve(Some(&f.workspace), "abc ${workspaceFolder} xyz").await, format!("abc {} xyz", native("/VSCode/workspaceLocation")));
    });
}

#[test]
fn does_not_preserve_platform_config_even_when_not_matched() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let obj_value = obj([
            ("program", text("osx.sh")),
            ("windows", obj([("program", text("windows.exe"))])),
            ("linux", obj([("program", text("linux.sh"))])),
        ]);
        let config = f.service.resolve_async(Some(&f.workspace), &obj_value).await.unwrap();

        let expected = expected_program();
        assert!(config.get_str("windows").is_undefined());
        assert!(config.get_str("osx").is_undefined());
        assert!(config.get_str("linux").is_undefined());
        assert_deep_equal(&config.get_str("program"), &text(expected));
    });
}

#[test]
fn apples_platform_specific_config() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let obj_value = obj([
            ("windows", obj([("program", text("windows.exe"))])),
            ("osx", obj([("program", text("osx.sh"))])),
            ("linux", obj([("program", text("linux.sh"))])),
        ]);
        let original_obj = json_stringify(&obj_value);
        let config = f.service.resolve_async(Some(&f.workspace), &obj_value).await.unwrap();

        assert_deep_equal(&config.get_str("program"), &text(expected_program()));
        assert!(config.get_str("windows").is_undefined());
        assert!(config.get_str("osx").is_undefined());
        assert!(config.get_str("linux").is_undefined());
        assert_eq!(json_stringify(&obj_value), original_obj); // did not mutate original
    });
}

#[test]
fn workspace_folder_with_argument() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_eq!(f.resolve(Some(&f.workspace), "abc ${workspaceFolder:workspaceLocation} xyz").await, format!("abc {} xyz", native("/VSCode/workspaceLocation")));
    });
}

#[test]
fn workspace_folder_with_invalid_argument() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_rejects(&f.service, Some(&f.workspace), "abc ${workspaceFolder:invalidLocation} xyz").await;
    });
}

#[test]
fn workspace_folder_with_undefined_workspace_folder() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_rejects(&f.service, None, "abc ${workspaceFolder} xyz").await;
    });
}

#[test]
fn workspace_folder_with_argument_and_undefined_workspace_folder() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_eq!(f.resolve(None, "abc ${workspaceFolder:workspaceLocation} xyz").await, format!("abc {} xyz", native("/VSCode/workspaceLocation")));
    });
}

#[test]
fn workspace_folder_with_invalid_argument_and_undefined_workspace_folder() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_rejects(&f.service, None, "abc ${workspaceFolder:invalidLocation} xyz").await;
    });
}

#[test]
fn workspace_root_folder_name() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_eq!(f.resolve(Some(&f.workspace), "abc ${workspaceRootFolderName} xyz").await, "abc workspaceLocation xyz");
    });
}

#[test]
fn current_selected_line_number() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_eq!(f.resolve(Some(&f.workspace), "abc ${lineNumber} xyz").await, format!("abc {MOCK_LINE_NUMBER} xyz"));
    });
}

#[test]
fn relative_file() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_eq!(f.resolve(Some(&f.workspace), "abc ${relativeFile} xyz").await, "abc file xyz");
    });
}

#[test]
fn relative_file_with_argument() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_eq!(f.resolve(Some(&f.workspace), "abc ${relativeFile:workspaceLocation} xyz").await, "abc file xyz");
    });
}

#[test]
fn relative_file_with_invalid_argument() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_rejects(&f.service, Some(&f.workspace), "abc ${relativeFile:invalidLocation} xyz").await;
    });
}

#[test]
fn relative_file_with_undefined_workspace_folder() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_eq!(f.resolve(None, "abc ${relativeFile} xyz").await, format!("abc {} xyz", native("/VSCode/workspaceLocation/file")));
    });
}

#[test]
fn relative_file_with_argument_and_undefined_workspace_folder() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_eq!(f.resolve(None, "abc ${relativeFile:workspaceLocation} xyz").await, "abc file xyz");
    });
}

#[test]
fn relative_file_with_invalid_argument_and_undefined_workspace_folder() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_rejects(&f.service, None, "abc ${relativeFile:invalidLocation} xyz").await;
    });
}

#[test]
fn substitute_many() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let folder = native("/VSCode/workspaceLocation");
        assert_eq!(f.resolve(Some(&f.workspace), "${workspaceFolder} - ${workspaceFolder}").await, format!("{folder} - {folder}"));
    });
}

#[test]
fn substitute_one_env_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_eq!(
            f.resolve(Some(&f.workspace), "abc ${workspaceFolder} ${env:key1} xyz").await,
            format!("abc {} Value for key1 xyz", native("/VSCode/workspaceLocation"))
        );
    });
}

#[test]
fn substitute_many_env_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let folder = native("/VSCode/workspaceLocation");
        assert_eq!(
            f.resolve(Some(&f.workspace), "${workspaceFolder} - ${workspaceFolder} ${env:key1} - ${env:key2}").await,
            format!("{folder} - {folder} Value for key1 - Value for key2")
        );
    });
}

#[test]
fn disallows_nested_keys_77289() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        assert_eq!(f.resolve(Some(&f.workspace), "${env:key1} ${env:key1${env:key2}}").await, "Value for key1 ");
    });
}

#[test]
fn supports_extension_dir() {
    let f = Fixture::new();
    f.extension_service.stub_get_extension("publisher.extId", IExtensionDescription { extension_location: uri_file("/some/path") });
    run_with_faked_timers(async move {
        assert_eq!(f.resolve(Some(&f.workspace), "${extensionInstallFolder:publisher.extId}").await, je_platform::resources::fs_path(&uri_file("/some/path")));
    });
}

// test('substitute keys and values in object') está comentado no original.

#[test]
fn substitute_one_env_variable_using_platform_case_sensitivity() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let expected = if is_windows() { "Value for key1 - Value for key1" } else { "Value for key1 - " };
        assert_eq!(f.resolve(Some(&f.workspace), "${env:key1} - ${env:Key1}").await, expected);
    });
}

#[test]
fn substitute_one_configuration_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let service = f.with_configuration(font_config());
        assert_eq!(resolve_text(&service, Some(&f.workspace), "abc ${config:editor.fontFamily} xyz").await, "abc foo xyz");
    });
}

#[test]
fn inlines_an_array_245718() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let service = f.with_configuration(obj([("editor", obj([("fontFamily", Dynamic::array([text("foo"), text("bar")]))]))]));
        assert_eq!(resolve_text(&service, Some(&f.workspace), "abc ${config:editor.fontFamily} xyz").await, "abc foo,bar xyz");
    });
}

#[test]
fn substitute_configuration_variable_with_undefined_workspace_folder() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let service = f.with_configuration(obj([("editor", obj([("fontFamily", text("foo"))]))]));
        assert_eq!(resolve_text(&service, None, "abc ${config:editor.fontFamily} xyz").await, "abc foo xyz");
    });
}

#[test]
fn substitute_many_configuration_variables() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let service = f.with_configuration(font_config());
        assert_eq!(
            resolve_text(&service, Some(&f.workspace), "abc ${config:editor.fontFamily} ${config:terminal.integrated.fontFamily} xyz").await,
            "abc foo bar xyz"
        );
    });
}

#[test]
fn substitute_one_env_variable_and_a_configuration_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let service = f.with_configuration(font_config());
        assert_eq!(
            resolve_text(&service, Some(&f.workspace), "abc ${config:editor.fontFamily} ${workspaceFolder} ${env:key1} xyz").await,
            format!("abc foo {} Value for key1 xyz", native("/VSCode/workspaceLocation"))
        );
    });
}

#[test]
fn recursively_resolve_variables() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let service = f.with_configuration(obj([("key1", text("key1=${config:key2}")), ("key2", text("key2=${config:key3}")), ("key3", text("we did it!"))]));
        assert_eq!(resolve_text(&service, Some(&f.workspace), "${config:key1}").await, "key1=key2=we did it!");
    });
}

#[test]
fn substitute_many_env_variable_and_a_configuration_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let service = f.with_configuration(font_config());
        let folder = native("/VSCode/workspaceLocation");
        assert_eq!(
            resolve_text(
                &service,
                Some(&f.workspace),
                "${config:editor.fontFamily} ${config:terminal.integrated.fontFamily} ${workspaceFolder} - ${workspaceFolder} ${env:key1} - ${env:key2}"
            )
            .await,
            format!("foo bar {folder} - {folder} Value for key1 - Value for key2")
        );
    });
}

#[test]
fn mixed_types_of_configuration_variables() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let service = f.with_configuration(obj([
            ("editor", obj([("fontFamily", text("foo")), ("lineNumbers", num(123.0)), ("insertSpaces", Dynamic::Bool(false))])),
            ("terminal", obj([("integrated", obj([("fontFamily", text("bar"))]))])),
            (
                "json",
                obj([("schemas", Dynamic::array([obj([("fileMatch", Dynamic::array([text("/myfile"), text("/myOtherfile")])), ("url", text("schemaURL"))])]))]),
            ),
        ]));
        assert_eq!(
            resolve_text(&service, Some(&f.workspace), "abc ${config:editor.fontFamily} ${config:editor.lineNumbers} ${config:editor.insertSpaces} xyz").await,
            "abc foo 123 false xyz"
        );
    });
}

#[test]
fn uses_original_variable_as_fallback() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let service = f.with_configuration(obj([("editor", obj([]))]));
        assert_eq!(resolve_text(&service, Some(&f.workspace), "abc ${unknownVariable} xyz").await, "abc ${unknownVariable} xyz");
        assert_eq!(resolve_text(&service, Some(&f.workspace), "abc ${env:unknownVariable} xyz").await, "abc  xyz");
    });
}

#[test]
fn configuration_variables_with_invalid_accessor() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let service = f.with_configuration(obj([("editor", obj([("fontFamily", text("foo"))]))]));

        for value in [
            "abc ${env} xyz",
            "abc ${env:} xyz",
            "abc ${config} xyz",
            "abc ${config:} xyz",
            "abc ${config:editor} xyz",
            "abc ${config:editor..fontFamily} xyz",
            "abc ${config:editor.none.none2} xyz",
        ] {
            assert_rejects(&service, Some(&f.workspace), value).await;
        }
    });
}

#[test]
fn a_single_command_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let result = f.service.resolve_with_interaction_replace(None, &attach_config("${command:command1}"), None, None, None).await.unwrap();
        assert_deep_equal(&result, &attach_expected("command1-result"));
        assert_eq!(1, f.command_service.call_count.get());
    });
}

#[test]
fn an_old_style_command_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let command_variables = variables(&[("commandVariable1", "command1")]);
        let result = f.service.resolve_with_interaction_replace(None, &attach_config("${command:commandVariable1}"), None, Some(&command_variables), None).await.unwrap();
        assert_deep_equal(&result, &attach_expected("command1-result"));
        assert_eq!(1, f.command_service.call_count.get());
    });
}

#[test]
fn multiple_new_and_old_style_command_variables() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let configuration = obj([
            ("name", text("Attach to Process")),
            ("type", text("node")),
            ("request", text("attach")),
            ("processId", text("${command:commandVariable1}")),
            ("pid", text("${command:command2}")),
            ("sourceMaps", Dynamic::Bool(false)),
            ("outDir", text("src/${command:command2}")),
            ("env", obj([("processId", text("__${command:command2}__"))])),
        ]);
        let command_variables = variables(&[("commandVariable1", "command1")]);

        let result = f.service.resolve_with_interaction_replace(None, &configuration, None, Some(&command_variables), None).await.unwrap();
        let expected = obj([
            ("name", text("Attach to Process")),
            ("type", text("node")),
            ("request", text("attach")),
            ("processId", text("command1-result")),
            ("pid", text("command2-result")),
            ("sourceMaps", Dynamic::Bool(false)),
            ("outDir", text("src/command2-result")),
            ("env", obj([("processId", text("__command2-result__"))])),
        ]);

        assert_eq!(result.own_keys(), expected.own_keys());
        for property in result.own_keys() {
            assert_deep_equal(&result.get(&property), &expected.get(&property));
        }
        assert_eq!(2, f.command_service.call_count.get());
    });
}

#[test]
fn a_command_variable_that_relies_on_resolved_env_vars() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let configuration = obj([
            ("name", text("Attach to Process")),
            ("type", text("node")),
            ("request", text("attach")),
            ("processId", text("${command:commandVariable1}")),
            ("value", text("${env:key1}")),
        ]);
        let command_variables = variables(&[("commandVariable1", "command1")]);

        let result = f.service.resolve_with_interaction_replace(None, &configuration, None, Some(&command_variables), None).await.unwrap();
        assert_deep_equal(
            &result,
            &obj([
                ("name", text("Attach to Process")),
                ("type", text("node")),
                ("request", text("attach")),
                ("processId", text("Value for key1")),
                ("value", text("Value for key1")),
            ]),
        );
        assert_eq!(1, f.command_service.call_count.get());
    });
}

#[test]
fn a_single_prompt_input_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let result = f.service.resolve_with_interaction_replace(Some(&f.workspace), &attach_config("${input:input1}"), Some("tasks"), None, None).await.unwrap();
        assert_deep_equal(&result, &attach_expected("resolvedEnterinput1"));
        assert_eq!(0, f.command_service.call_count.get());
    });
}

#[test]
fn a_single_pick_input_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let result = f.service.resolve_with_interaction_replace(Some(&f.workspace), &attach_config("${input:input2}"), Some("tasks"), None, None).await.unwrap();
        assert_deep_equal(&result, &attach_expected("selectedPick"));
        assert_eq!(0, f.command_service.call_count.get());
    });
}

#[test]
fn a_single_command_input_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let result = f.service.resolve_with_interaction_replace(Some(&f.workspace), &attach_config("${input:input4}"), Some("tasks"), None, None).await.unwrap();
        assert_deep_equal(&result, &attach_expected("arg for command"));
        assert_eq!(1, f.command_service.call_count.get());
    });
}

#[test]
fn several_input_variables_and_command() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let configuration = obj([
            ("name", text("${input:input3}")),
            ("type", text("${command:command1}")),
            ("request", text("${input:input1}")),
            ("processId", text("${input:input2}")),
            ("command", text("${input:input4}")),
            ("port", num(5858.0)),
            ("sourceMaps", Dynamic::Bool(false)),
            ("outDir", Dynamic::Null),
        ]);

        let result = f.service.resolve_with_interaction_replace(Some(&f.workspace), &configuration, Some("tasks"), None, None).await.unwrap();
        assert_deep_equal(
            &result,
            &obj([
                ("name", text("resolvedEnterinput3")),
                ("type", text("command1-result")),
                ("request", text("resolvedEnterinput1")),
                ("processId", text("selectedPick")),
                ("command", text("arg for command")),
                ("port", num(5858.0)),
                ("sourceMaps", Dynamic::Bool(false)),
                ("outDir", Dynamic::Null),
            ]),
        );
        assert_eq!(2, f.command_service.call_count.get());
    });
}

#[test]
fn input_variable_with_undefined_workspace_folder() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let result = f.service.resolve_with_interaction_replace(None, &attach_config("${input:input1}"), Some("tasks"), None, None).await.unwrap();
        assert_deep_equal(&result, &attach_expected("resolvedEnterinput1"));
        assert_eq!(0, f.command_service.call_count.get());
    });
}

#[test]
fn contributed_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let build_task = "npm: compile";
        f.service.contribute_variable("defaultBuildTask", Rc::new(move || Promise::resolved(Some(build_task.to_string()))));
        let result = f.service.resolve_with_interaction_replace(Some(&f.workspace), &obj([("name", text("${defaultBuildTask}"))]), None, None, None).await.unwrap();
        assert_deep_equal(&result, &obj([("name", text(build_task))]));
    });
}

#[test]
fn contributed_task_var_variable() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let url = "http://localhost:5678";
        f.service.contribute_variable("taskVar:componentExplorerUrl", Rc::new(move || Promise::resolved(Some(url.to_string()))));
        let result = f.service.resolve_with_interaction_replace(Some(&f.workspace), &obj([("url", text("${taskVar:componentExplorerUrl}/___explorer"))]), None, None, None).await.unwrap();
        assert_deep_equal(&result, &obj([("url", text(&format!("{url}/___explorer")))]));
    });
}

#[test]
fn resolve_with_environment() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let env = ProcessEnvironment::from([("VAR_1".to_string(), "VAL_1".to_string()), ("VAR_2".to_string(), "VAL_2".to_string())]);
        let resolved_result = f.service.resolve_with_environment(&env, None, "echo ${env:VAR_1}${env:VAR_2}").await.unwrap();
        assert_eq!(resolved_result, "echo VAL_1VAL_2");
    });
}

#[test]
fn substitution_in_object_key() {
    let f = Fixture::new();
    run_with_faked_timers(async move {
        let configuration = obj([
            ("name", text("Test")),
            ("mappings", obj([("pos1", text("value1")), ("${workspaceFolder}/test1", text("${workspaceFolder}/test2")), ("pos3", text("value3"))])),
        ]);

        let result = f.service.resolve_with_interaction_replace(Some(&f.workspace), &configuration, Some("tasks"), None, None).await.unwrap();
        let folder = native("/VSCode/workspaceLocation");
        assert_deep_equal(
            &result,
            &obj([
                ("name", text("Test")),
                (
                    "mappings",
                    Dynamic::object([
                        ("pos1".to_string(), text("value1")),
                        (format!("{folder}/test1"), text(&format!("{folder}/test2"))),
                        ("pos3".to_string(), text("value3")),
                    ]),
                ),
            ]),
        );
        assert_eq!(0, f.command_service.call_count.get());
    });
}

#[test]
fn canceled_input() {
    let f = Fixture::new();
    f.quick_input_service.stub_input_resolves(None);
    run_with_faked_timers(async move {
        let result = f.service.resolve_with_interaction_replace(Some(&f.workspace), &attach_config("${input:input1}"), Some("tasks"), None, None).await.unwrap();
        assert!(result.is_undefined());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// ConfigurationResolverExpression
// ---------------------------------------------------------------------------------------------------------------------

fn replacement(id: &str, inner: &str, name: &str, arg: &str) -> Replacement {
    ConfigurationVariable { id: JsString::from(id), inner: JsString::from(inner), name: JsString::from(name), arg: Some(JsString::from(arg)) }
}

fn unresolved_of(expr: &Rc<ConfigurationResolverExpression>) -> Vec<Replacement> {
    expr.unresolved().collect()
}

fn names(replacements: &[Replacement]) -> Vec<String> {
    replacements.iter().map(|r| r.name.to_string_lossy()).collect()
}

#[test]
fn parse_empty_object() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&obj([]));
    assert_eq!(unresolved_of(&expr).len(), 0);
    assert_deep_equal(&expr.to_object(), &obj([]));
}

#[test]
fn parse_simple_string() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&obj([("value", text("${env:HOME}"))]));
    let unresolved = unresolved_of(&expr);
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].name, "env");
    assert_eq!(unresolved[0].arg, Some(JsString::from("HOME")));
}

#[test]
fn parse_string_with_argument_and_colon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&obj([("value", text("${config:path:to:value}"))]));
    let unresolved = unresolved_of(&expr);
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].name, "config");
    assert_eq!(unresolved[0].arg, Some(JsString::from("path:to:value")));
}

#[test]
fn parse_object_with_nested_variables() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&obj([
        ("name", text("${env:USERNAME}")),
        ("path", text("${env:HOME}/folder")),
        ("settings", obj([("value", text("${config:path}"))])),
        ("array", Dynamic::array([text("${env:TERM}"), obj([("key", text("${env:KEY}"))])])),
    ]));

    let unresolved = unresolved_of(&expr);
    assert_eq!(unresolved.len(), 5);
    let mut sorted = names(&unresolved);
    sorted.sort();
    assert_eq!(sorted, ["config", "env", "env", "env", "env"]);
}

#[test]
fn resolve_and_get_result() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&obj([("name", text("${env:USERNAME}")), ("path", text("${env:HOME}/folder"))]));

    expr.resolve(&replacement("${env:USERNAME}", "env:USERNAME", "env", "USERNAME"), "testuser");
    expr.resolve(&replacement("${env:HOME}", "env:HOME", "env", "HOME"), "/home/testuser");

    assert_deep_equal(&expr.to_object(), &obj([("name", text("testuser")), ("path", text("/home/testuser/folder"))]));
}

#[test]
fn keeps_unresolved_variables() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&obj([("name", text("${env:USERNAME}"))]));
    assert_deep_equal(&expr.to_object(), &obj([("name", text("${env:USERNAME}"))]));
}

#[test]
fn deduplicates_identical_variables() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&obj([("first", text("${env:HOME}")), ("second", text("${env:HOME}"))]));

    let unresolved = unresolved_of(&expr);
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].name, "env");
    assert_eq!(unresolved[0].arg, Some(JsString::from("HOME")));

    expr.resolve(&unresolved[0], "/home/user");
    assert_deep_equal(&expr.to_object(), &obj([("first", text("/home/user")), ("second", text("/home/user"))]));
}

#[test]
fn handles_root_string_value() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&text("abc ${env:HOME} xyz"));
    let unresolved = unresolved_of(&expr);
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].name, "env");
    assert_eq!(unresolved[0].arg, Some(JsString::from("HOME")));

    expr.resolve(&unresolved[0], "/home/user");
    assert_deep_equal(&expr.to_object(), &text("abc /home/user xyz"));
}

#[test]
fn handles_root_string_value_with_multiple_variables() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&text("${env:HOME}/folder${env:SHELL}"));
    assert_eq!(unresolved_of(&expr).len(), 2);

    expr.resolve(&replacement("${env:HOME}", "env:HOME", "env", "HOME"), "/home/user");
    expr.resolve(&replacement("${env:SHELL}", "env:SHELL", "env", "SHELL"), "/bin/bash");
    assert_deep_equal(&expr.to_object(), &text("/home/user/folder/bin/bash"));
}

#[test]
fn handles_root_string_with_escaped_variables() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&text("abc ${env:HOME${env:USER}} xyz"));
    let unresolved = unresolved_of(&expr);
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].name, "env");
    assert_eq!(unresolved[0].arg, Some(JsString::from("HOME${env:USER}")));
}

#[test]
fn resolves_nested_values() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&obj([("name", text("${env:REDIRECTED}")), ("key that is ${env:REDIRECTED}", text("cool!"))]));

    for r in expr.unresolved() {
        if r.arg == Some(JsString::from("REDIRECTED")) {
            expr.resolve(&r, "username: ${env:USERNAME}");
        } else if r.arg == Some(JsString::from("USERNAME")) {
            expr.resolve(&r, "testuser");
        }
    }

    assert_deep_equal(&expr.to_object(), &obj([("name", text("username: testuser")), ("key that is username: testuser", text("cool!"))]));
}

#[test]
fn resolves_nested_values_2_245798() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&obj([(
        "env",
        obj([("SITE", text("${input:site}")), ("TLD", text("${input:tld}")), ("HOST", text("${input:host}"))]),
    )]));

    for r in expr.unresolved() {
        match r.arg.as_ref().map(JsString::to_string_lossy).as_deref() {
            Some("site") => expr.resolve(&r, "example"),
            Some("tld") => expr.resolve(&r, "com"),
            Some("host") => expr.resolve(&r, "local.${input:site}.${input:tld}"),
            _ => {}
        }
    }

    assert_deep_equal(&expr.to_object(), &obj([("env", obj([("SITE", text("example")), ("TLD", text("com")), ("HOST", text("local.example.com"))]))]));
}

#[test]
fn out_of_order_key_resolution_248550() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ConfigurationResolverExpression::parse(&obj([("${input:key}", text("${input:value}"))]));

    for r in expr.unresolved() {
        if r.arg == Some(JsString::from("key")) {
            expr.resolve(&r, "the-key");
        }
    }
    for r in expr.unresolved() {
        if r.arg == Some(JsString::from("value")) {
            expr.resolve(&r, "the-value");
        }
    }

    assert_deep_equal(&expr.to_object(), &obj([("the-key", text("the-value"))]));
}
