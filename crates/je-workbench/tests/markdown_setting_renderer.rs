//! Porte de `src/vs/workbench/contrib/markdown/test/browser/markdownSettingRenderer.test.ts` (suíte `Markdown Setting Renderer Test`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`suiteSetup` e `suiteTeardown`.** O registro de configuração é por fio e cada `#[test]` roda no seu, então o `setup` de
//!   cada caso monta o [`Fixture`] (serviço de configuração, serviço de preferências, renderizador e o registro do nó
//!   `examples`) e o `Drop` do [`Fixture`] faz o `deregisterConfigurations([configuration])`. O estado que o original compartilha
//!   entre os casos (o valor de `example.stringSetting`) só é usado pelo terceiro, que o define no começo.
//! - **`MarkdownConfigurationService extends TestConfigurationService`.** Composição: [`MarkdownConfigurationService`] guarda o
//!   `TestConfigurationService`, delega tudo e troca o `updateValue` por `setUserConfiguration(section, { [setting]: value })`.
//! - **`<IPreferencesService>{ getSetting }` e `<IContextMenuService>{}`.** Dublês que só têm o que o original define; os demais
//!   métodos reprovam como o `x is not a function` do JavaScript. `{ publicLog2: () => { } }` é o
//!   `NullTelemetryService`, e `{ writeText: async () => { } }` é o [`TestClipboardService`].
//! - **`ConfigurationScope.APPLICATION`** é `ConfigurationScope::Application.to_dynamic()` no esquema da propriedade.
//! - **`htmlRenderer({ block, raw, pre, text, type })`.** O renderizador lê só o `raw`, que é o argumento.
//! - **`await actions[0].run()`** é `.await` da promessa devolvida, num caso assíncrono (`run_with_real_timers`).
//! - **`assert.strictEqual(actions?.length, 2)`** compara `Option<usize>` com `Some(2)`; `actions[0]` depois do `length`
//!   confere que a lista existe.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::JsString;
use je_base::actions::ActionRef;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::configuration::models::{ConfigurationChangeEvent, ConfigurationKeys};
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, ConfigurationScope, PropertySchema, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::{ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue};
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_ui::Ui;
use je_widgets::context_view_api::{IContextMenuDelegate, IContextMenuProvider};
use je_workbench::contrib::markdown::markdown_setting_renderer::SimpleSettingRenderer;
use je_workbench::services::preferences::preferences::{IOpenSettingsOptions, IPreferencesService, ISetting, SettingSchemaType};

/// `const configuration: IConfigurationNode`.
fn configuration() -> ConfigurationNode {
    let property = |kind: &str, default: Dynamic| {
        PropertySchema::from_entries([("type", Dynamic::string(kind)), ("default", default), ("scope", ConfigurationScope::Application.to_dynamic())])
    };
    ConfigurationNode::new(
        ConfigurationNodeData { id: Some("examples".to_owned()), title: Some("Examples".to_owned()), type_: Some(Dynamic::string("object")), ..ConfigurationNodeData::default() }.with_properties(vec![
            ("example.booleanSetting", property("boolean", Dynamic::Bool(false))),
            ("example.booleanSetting2", property("boolean", Dynamic::Bool(true))),
            ("example.stringSetting", property("string", Dynamic::string("one"))),
            ("example.numberSetting", property("number", Dynamic::Number(3.0))),
        ]),
    )
}

/// `class MarkdownConfigurationService extends TestConfigurationService`.
struct MarkdownConfigurationService {
    inner: Rc<TestConfigurationService>,
}

impl IConfigurationService for MarkdownConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.inner.on_did_change_configuration()
    }

    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        self.inner.get_configuration_data()
    }

    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        self.inner.get_value(section, overrides)
    }

    /// `const [section, setting] = key.split('.'); return this.setUserConfiguration(section, { [setting]: value });`
    fn update_value(&self, key: &str, value: Dynamic, _overrides: Option<ConfigurationUpdateOverrides>, _target: Option<ConfigurationTarget>) -> Promise<()> {
        let mut parts = key.split('.');
        let section = parts.next().unwrap_or_default();
        let setting = parts.next().unwrap_or("undefined");
        self.inner.set_user_configuration(section, Dynamic::object([(setting, value)]), None)
    }

    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        self.inner.inspect(key, overrides)
    }

    fn reload_configuration(&self) -> Promise<()> {
        self.inner.reload_configuration()
    }

    fn keys(&self) -> ConfigurationKeys {
        self.inner.keys()
    }
}

/// `<IPreferencesService>{ getSetting: (setting) => { let type = 'boolean'; if (setting.includes('string')) { type = 'string'; } return { type, key: setting }; } }`.
struct TestPreferencesService;

impl IPreferencesService for TestPreferencesService {
    fn open_user_settings(&self, _options: Option<&IOpenSettingsOptions>) -> Promise<()> {
        panic!("openUserSettings is not a function")
    }

    fn open_application_settings(&self, _options: Option<&IOpenSettingsOptions>) -> Promise<()> {
        panic!("openApplicationSettings is not a function")
    }

    fn get_setting(&self, setting: &str) -> Option<ISetting> {
        let kind = if setting.contains("string") { "string" } else { "boolean" };
        Some(ISetting { r#type: Some(SettingSchemaType::Single(kind.to_owned())), key: setting.to_owned(), ..ISetting::default() })
    }
}

/// `<IContextMenuService>{}`.
struct TestContextMenuService;

impl IContextMenuProvider for TestContextMenuService {
    fn show_context_menu(&self, _delegate: Rc<dyn IContextMenuDelegate>) {
        panic!("showContextMenu is not a function")
    }
}

/// `{ writeText: async () => { } }`.
struct TestClipboardService;

impl IClipboardService for TestClipboardService {
    fn write_text(&self, _text: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn read_text(&self) -> Promise<String> {
        panic!("readText is not a function")
    }

    fn read_find_text(&self) -> Promise<JsString> {
        panic!("readFindText is not a function")
    }

    fn write_find_text(&self, _text: &JsString) -> Promise<()> {
        panic!("writeFindText is not a function")
    }

    fn write_resources(&self, _resources: Vec<URI>) -> Promise<()> {
        panic!("writeResources is not a function")
    }

    fn read_resources(&self) -> Promise<Vec<URI>> {
        panic!("readResources is not a function")
    }

    fn has_resources(&self) -> Promise<bool> {
        panic!("hasResources is not a function")
    }
}

/// O que o `suiteSetup` monta.
struct Fixture {
    configuration_service: Rc<TestConfigurationService>,
    setting_renderer: Rc<SimpleSettingRenderer>,
    registered: ConfigurationNode,
}

impl Drop for Fixture {
    /// `suiteTeardown`: `deregisterConfigurations([configuration])`.
    fn drop(&mut self) {
        if !std::thread::panicking() {
            configuration_registry().deregister_configurations(std::slice::from_ref(&self.registered));
        }
    }
}

/// `suiteSetup`.
fn setup(guard: &LeakGuard) -> Fixture {
    let configuration_service = TestConfigurationService::new(None);
    guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let markdown_configuration_service: Rc<dyn IConfigurationService> = Rc::new(MarkdownConfigurationService { inner: configuration_service.clone() });
    let registered = configuration_registry().register_configuration(configuration());
    let setting_renderer = SimpleSettingRenderer::new(
        markdown_configuration_service,
        Rc::new(TestContextMenuService),
        Rc::new(TestPreferencesService),
        null_telemetry_service(),
        Rc::new(TestClipboardService),
        Ui::new(),
    );
    Fixture { configuration_service, setting_renderer, registered }
}

fn labels(actions: &Option<Vec<ActionRef>>) -> Vec<String> {
    actions.as_ref().map(|actions| actions.iter().map(|action| action.label()).collect()).unwrap_or_default()
}

#[test]
fn render_code_setting_button_with_value() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&guard);

    let html_renderer = fixture.setting_renderer.get_html_renderer();
    let html_no_value = r#"<a href="code-oss://settings/example.booleanSetting" codesetting="true">"#;
    let rendered_html_no_value = html_renderer(html_no_value);
    assert_eq!(
        rendered_html_no_value,
        "<code tabindex=\"0\"><a href=\"code-setting://example.booleanSetting/true\" class=\"codesetting\" title=\"View or change setting\" aria-role=\"button\"><svg width=\"14\" height=\"14\" viewBox=\"0 0 15 15\" xmlns=\"http://www.w3.org/2000/svg\" fill=\"currentColor\"><path d=\"M9.1 4.4L8.6 2H7.4l-.5 2.4-.7.3-2-1.3-.9.8 1.3 2-.2.7-2.4.5v1.2l2.4.5.3.8-1.3 2 .8.8 2-1.3.8.3.4 2.3h1.2l.5-2.4.8-.3 2 1.3.8-.8-1.3-2 .3-.8 2.3-.4V7.4l-2.4-.5-.3-.8 1.3-2-.8-.8-2 1.3-.7-.2zM9.4 1l.5 2.4L12 2.1l2 2-1.4 2.1 2.4.4v2.8l-2.4.5L14 12l-2 2-2.1-1.4-.5 2.4H6.6l-.5-2.4L4 13.9l-2-2 1.4-2.1L1 9.4V6.6l2.4-.5L2.1 4l2-2 2.1 1.4.4-2.4h2.8zm.6 7c0 1.1-.9 2-2 2s-2-.9-2-2 .9-2 2-2 2 .9 2 2zM8 9c.6 0 1-.4 1-1s-.4-1-1-1-1 .4-1 1 .4 1 1 1z\"/></svg>\n\t\t\t<span class=\"separator\"></span>\n\t\t\t<span class=\"setting-name\">example.booleanSetting</span>\n\t\t</a></code>"
    );
}

#[test]
fn actions_with_no_value() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&guard);

    let uri = URI::parse(fixture.setting_renderer.setting_to_uri_string("example.booleanSetting", None), false);
    let actions = fixture.setting_renderer.get_actions(&uri);
    assert_eq!(actions.as_ref().map(Vec::len), Some(2));
    assert_eq!(labels(&actions)[0], "View \"Example: Boolean Setting\" in Settings");
}

#[test]
fn actions_with_value_updating_and_restoring() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&guard);

    run_with_real_timers(async move {
        let configuration_service = fixture.configuration_service.clone();
        let setting_renderer = fixture.setting_renderer.clone();
        configuration_service.set_user_configuration("example", Dynamic::object([("stringSetting", Dynamic::string("two"))]), None).await.expect("setUserConfiguration");
        let uri = URI::parse(setting_renderer.setting_to_uri_string("example.stringSetting", Some("three")), false);

        let current_string_setting = || configuration_service.get_value(Some("example.stringSetting"), None);
        let verify_original_state = |actions: &Option<Vec<ActionRef>>| -> bool {
            assert_eq!(actions.as_ref().map(Vec::len), Some(3));
            let labels = labels(actions);
            assert_eq!(labels[0], "Set \"Example: String Setting\" to \"three\"");
            assert_eq!(labels[1], "View in Settings");
            assert!(current_string_setting().strict_equals(&Dynamic::string("two")));
            true
        };

        let actions = setting_renderer.get_actions(&uri);
        if verify_original_state(&actions) {
            let actions_list = actions.as_ref().expect("as ações existem");
            // Update the value
            actions_list[0].run(&[]).await.expect("run");
            assert!(current_string_setting().strict_equals(&Dynamic::string("three")));
            let actions_updated = setting_renderer.get_actions(&uri);
            assert_eq!(actions_updated.as_ref().map(Vec::len), Some(3));
            assert_eq!(labels(&actions_updated)[0], "Restore value of \"Example: String Setting\"");
            assert_eq!(actions_list[1].label(), "View in Settings");
            assert_eq!(actions_list[2].label(), "Copy Setting ID");
            assert!(current_string_setting().strict_equals(&Dynamic::string("three")));

            // Restore the value
            actions_updated.as_ref().expect("as ações existem")[0].run(&[]).await.expect("run");
            verify_original_state(&setting_renderer.get_actions(&uri));
        }
        drop(fixture);
    });
}
