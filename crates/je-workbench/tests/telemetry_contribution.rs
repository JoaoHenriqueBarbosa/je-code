//! Porte de `src/vs/workbench/contrib/telemetry/test/browser/telemetry.contribution.test.ts` (suíte
//! `ConfigurationTelemetryContribution`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo de cada caso, e `store.add(...)` é o `add_rc`
//!   dele. Os serviços entram direto no construtor, sem contêiner de injeção.
//! - `new class extends mock<I...>() { ... }` são os dublês abaixo: cada um define só os membros do original, e os outros
//!   falham com `panic!`, como o `TypeError` do método ausente.
//! - `TestSettings` (`Record<string, boolean | number | string>`) é uma lista de pares `(chave, Dynamic)`, e o
//!   `createModel(values)` é o `ConfigurationModelParser` com o `parse_raw` da lista.
//! - O `Configuration` de dez argumentos e o `new Workspace('telemetry-test', [], false, null, () => false)` são os do porte,
//!   com as mesmas camadas: o padrão traz o vidro fosco ligado e a opacidade 80, e `application`, `user` e `workspace`
//!   trazem o que o caso passa.
//! - `events: { name, data }[]` guarda o nome e o objeto `{ settingValue, source }` do `publicLog2`; o `deepStrictEqual`
//!   compara o nome, o `settingValue` (`undefined` é `None`) e o `source`.
//! - Os `for` sobre escopo e combinações viram um teste por combinação, na ordem do original (`user`, depois `application`),
//!   que chamam as funções auxiliares do caso; `String(opacity)` é o `JSON.stringify` do número, o mesmo texto do JavaScript
//!   para um número finito.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::map::ResourceMap;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::models::{
    Configuration, ConfigurationChangeEvent, ConfigurationKeys, ConfigurationModel, ConfigurationModelParser,
};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{
    ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue,
};
use je_platform::log::NullLogService;
use je_platform::telemetry::telemetry::{CommonPropertyValue, ITelemetryData, ITelemetryService, TelemetryLevel};
use je_platform::user_data_profile::user_data_profile::{
    DidChangeProfilesEvent, IUserDataProfilesService, UserDataProfile, UserDataProfileOptions, UserDataProfileUpdateOptions,
};
use je_platform::workspace::{AnyWorkspaceIdentifier, Workspace};
use je_workbench::contrib::telemetry::ConfigurationTelemetryContribution;
use je_workbench::services::layout::layout_service::LayoutSettings;

/// O membro que o objeto `mock<...>()` do original não define: chamá-lo é um `TypeError`.
fn not_a_function(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `TestSettings`.
type TestSettings = Vec<(String, Dynamic)>;

/// `settings` de `reportSettings`.
#[derive(Default)]
struct Settings {
    application: Option<TestSettings>,
    user: Option<TestSettings>,
    workspace: Option<TestSettings>,
}

/// Um elemento de `events`, na forma que o `assert.deepStrictEqual` do original compara.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Event2 {
    name: String,
    setting_value: Option<String>,
    source: String,
}

fn event(name: &str, setting_value: Option<String>, source: &str) -> Event2 {
    Event2 { name: name.to_string(), setting_value, source: source.to_string() }
}

type Events = Rc<RefCell<Vec<Event2>>>;

/// `new class extends mock<IConfigurationService>() { keys, inspect, getConfigurationData }`.
struct ModelConfigurationService {
    model: Configuration,
    workspace: Rc<Workspace>,
}

impl IConfigurationService for ModelConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        not_a_function("onDidChangeConfiguration")
    }

    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        Some(self.model.to_data())
    }

    fn get_value(&self, _section: Option<&str>, _overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        not_a_function("getValue")
    }

    fn update_value(&self, _key: &str, _value: Dynamic, _overrides: Option<ConfigurationUpdateOverrides>, _target: Option<ConfigurationTarget>) -> Promise<()> {
        not_a_function("updateValue")
    }

    fn inspect(&self, key: &str, _overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        Rc::new(self.model.inspect(key, &ConfigurationOverrides::default(), Some(&self.workspace)))
    }

    fn reload_configuration(&self) -> Promise<()> {
        not_a_function("reloadConfiguration")
    }

    fn keys(&self) -> ConfigurationKeys {
        self.model.keys(Some(&self.workspace))
    }
}

/// `new class extends mock<ITelemetryService>() { publicLog2 }`.
struct RecordingTelemetryService {
    events: Events,
}

impl ITelemetryService for RecordingTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        not_a_function("telemetryLevel")
    }

    fn session_id(&self) -> String {
        not_a_function("sessionId")
    }

    fn machine_id(&self) -> String {
        not_a_function("machineId")
    }

    fn sqm_id(&self) -> String {
        not_a_function("sqmId")
    }

    fn dev_device_id(&self) -> String {
        not_a_function("devDeviceId")
    }

    fn first_session_date(&self) -> String {
        not_a_function("firstSessionDate")
    }

    fn send_error_telemetry(&self) -> bool {
        not_a_function("sendErrorTelemetry")
    }

    fn public_log(&self, _event_name: &str, _data: Option<ITelemetryData>) {
        not_a_function("publicLog")
    }

    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        let data = data.expect("o evento traz os dados");
        let setting_value = match data.get_str("settingValue") {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        };
        let Dynamic::String(source) = data.get_str("source") else { panic!("o evento traz o source") };
        self.events.borrow_mut().push(Event2 { name: event_name.to_string(), setting_value, source: source.to_string_lossy() });
    }

    fn public_log_error(&self, _error_event_name: &str, _data: Option<ITelemetryData>) {
        not_a_function("publicLogError")
    }

    fn public_log_error2(&self, _event_name: &str, _data: Option<Dynamic>) {
        not_a_function("publicLogError2")
    }

    fn set_experiment_property(&self, _name: &str, _value: &str) {
        not_a_function("setExperimentProperty")
    }

    fn set_common_property(&self, _name: &str, _value: Option<CommonPropertyValue>) {
        not_a_function("setCommonProperty")
    }
}

/// `new class extends mock<IUserDataProfilesService>() { }`.
struct MockUserDataProfilesService;

impl IUserDataProfilesService for MockUserDataProfilesService {
    fn profiles_home(&self) -> URI {
        not_a_function("profilesHome")
    }

    fn default_profile(&self) -> UserDataProfile {
        not_a_function("defaultProfile")
    }

    fn on_did_change_profiles(&self) -> Event<DidChangeProfilesEvent> {
        not_a_function("onDidChangeProfiles")
    }

    fn profiles(&self) -> Vec<UserDataProfile> {
        not_a_function("profiles")
    }

    fn on_did_reset_workspaces(&self) -> Event<()> {
        not_a_function("onDidResetWorkspaces")
    }

    fn create_named_profile(&self, _name: &str, _options: Option<UserDataProfileOptions>, _workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile> {
        not_a_function("createNamedProfile")
    }

    fn create_transient_profile(&self, _workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile> {
        not_a_function("createTransientProfile")
    }

    fn create_profile(&self, _id: &str, _name: &str, _options: Option<UserDataProfileOptions>, _workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile> {
        not_a_function("createProfile")
    }

    fn update_profile(&self, _profile: &UserDataProfile, _options: UserDataProfileUpdateOptions) -> Promise<UserDataProfile> {
        not_a_function("updateProfile")
    }

    fn remove_profile(&self, _profile: &UserDataProfile) -> Promise<()> {
        not_a_function("removeProfile")
    }

    fn set_profile_for_workspace(&self, _workspace_identifier: &AnyWorkspaceIdentifier, _profile: &UserDataProfile) -> Promise<()> {
        not_a_function("setProfileForWorkspace")
    }

    fn reset_workspaces(&self) -> Promise<()> {
        not_a_function("resetWorkspaces")
    }

    fn clean_up(&self) -> Promise<()> {
        not_a_function("cleanUp")
    }

    fn clean_up_transient_profiles(&self) -> Promise<()> {
        not_a_function("cleanUpTransientProfiles")
    }
}

/// `reportSettings(settings)`.
fn report_settings(settings: Settings) -> Vec<Event2> {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let log_service = NullLogService::shared();
    let workspace = Workspace::new("telemetry-test", Vec::new(), false, None, Rc::new(|_: &URI| false), None);
    let create_model = |values: Option<TestSettings>| -> ConfigurationModel {
        let mut parser = ConfigurationModelParser::new("telemetry-test", log_service.clone());
        parser.parse_raw(&Dynamic::object(values.unwrap_or_default()), None);
        parser.configuration_model()
    };
    let model = Configuration::new(
        create_model(Some(vec![
            (LayoutSettings::ModernUiFrostedGlass.as_str().to_string(), Dynamic::Bool(true)),
            (LayoutSettings::ModernUiFrostedGlassOpacity.as_str().to_string(), Dynamic::Number(80.0)),
        ])),
        create_model(None),
        create_model(settings.application),
        create_model(settings.user),
        create_model(None),
        create_model(settings.workspace),
        ResourceMap::new(),
        create_model(None),
        ResourceMap::new(),
        log_service.clone(),
    );
    let configuration: Rc<dyn IConfigurationService> = Rc::new(ModelConfigurationService { model, workspace });
    let events: Events = Rc::new(RefCell::new(Vec::new()));
    let telemetry: Rc<dyn ITelemetryService> = Rc::new(RecordingTelemetryService { events: Rc::clone(&events) });
    let profiles: Rc<dyn IUserDataProfilesService> = Rc::new(MockUserDataProfilesService);
    store.add_rc(ConfigurationTelemetryContribution::new(configuration, profiles, telemetry));
    let reported = events.borrow().clone();
    reported
}

/// `String(value)` de um número.
fn number_text(value: f64) -> String {
    json_stringify(&Dynamic::Number(value)).expect("número finito")
}

/// `{ [LayoutSettings.MODERN_UI_FROSTED_GLASS]: enabled, [..._OPACITY]: opacity, 'editor.fontSize': 14 }`.
fn explicit_glass_settings(enabled: bool, opacity: f64) -> TestSettings {
    vec![
        (LayoutSettings::ModernUiFrostedGlass.as_str().to_string(), Dynamic::Bool(enabled)),
        (LayoutSettings::ModernUiFrostedGlassOpacity.as_str().to_string(), Dynamic::Number(opacity)),
        ("editor.fontSize".to_string(), Dynamic::Number(14.0)),
    ]
}

/// `{ [scope]: settings }`.
fn settings_in(scope: &str, settings: TestSettings) -> Settings {
    match scope {
        "user" => Settings { user: Some(settings), ..Settings::default() },
        _ => Settings { application: Some(settings), ..Settings::default() },
    }
}

/// O caso de `reports explicit ${scope} glass ${enabled} and opacity ${opacity} using configuration telemetry`.
fn reports_explicit_glass(scope: &'static str, source: &'static str, enabled: bool, opacity: f64) {
    run_with_faked_timers(async move {
        assert_eq!(
            report_settings(settings_in(scope, explicit_glass_settings(enabled, opacity))),
            vec![
                event("workbench.modernUIFrostedGlass", Some(enabled.to_string()), source),
                event("workbench.modernUIFrostedGlassOpacity", Some(number_text(opacity)), source),
            ]
        );
    });
}

/// O caso de `does not include arbitrary text from invalid ${scope} glass settings`.
fn does_not_include_arbitrary_text(scope: &'static str, source: &'static str) {
    run_with_faked_timers(async move {
        let text = || Dynamic::string("arbitrary user text");
        let settings = vec![
            (LayoutSettings::ModernUiFrostedGlass.as_str().to_string(), text()),
            (LayoutSettings::ModernUiFrostedGlassOpacity.as_str().to_string(), text()),
        ];
        assert_eq!(
            report_settings(settings_in(scope, settings)),
            vec![event("workbench.modernUIFrostedGlass", None, source), event("workbench.modernUIFrostedGlassOpacity", None, source)]
        );
    });
}

#[test]
fn reports_explicit_user_glass_false_and_opacity_50_using_configuration_telemetry() {
    reports_explicit_glass("user", "USER_LOCAL", false, 50.0);
}

#[test]
fn reports_explicit_user_glass_true_and_opacity_75_5_using_configuration_telemetry() {
    reports_explicit_glass("user", "USER_LOCAL", true, 75.5);
}

#[test]
fn reports_explicit_user_glass_true_and_opacity_92_using_configuration_telemetry() {
    reports_explicit_glass("user", "USER_LOCAL", true, 92.0);
}

#[test]
fn reports_explicit_user_glass_true_and_opacity_100_using_configuration_telemetry() {
    reports_explicit_glass("user", "USER_LOCAL", true, 100.0);
}

#[test]
fn does_not_include_arbitrary_text_from_invalid_user_glass_settings() {
    does_not_include_arbitrary_text("user", "USER_LOCAL");
}

#[test]
fn reports_explicit_application_glass_false_and_opacity_50_using_configuration_telemetry() {
    reports_explicit_glass("application", "APPLICATION", false, 50.0);
}

#[test]
fn reports_explicit_application_glass_true_and_opacity_75_5_using_configuration_telemetry() {
    reports_explicit_glass("application", "APPLICATION", true, 75.5);
}

#[test]
fn reports_explicit_application_glass_true_and_opacity_92_using_configuration_telemetry() {
    reports_explicit_glass("application", "APPLICATION", true, 92.0);
}

#[test]
fn reports_explicit_application_glass_true_and_opacity_100_using_configuration_telemetry() {
    reports_explicit_glass("application", "APPLICATION", true, 100.0);
}

#[test]
fn does_not_include_arbitrary_text_from_invalid_application_glass_settings() {
    does_not_include_arbitrary_text("application", "APPLICATION");
}

#[test]
fn does_not_report_default_settings_as_explicit_choices() {
    run_with_faked_timers(async move {
        assert_eq!(report_settings(Settings::default()), Vec::<Event2>::new());
    });
}

#[test]
fn reports_explicit_values_from_each_configuration_scope_rather_than_the_merged_value() {
    run_with_faked_timers(async move {
        let opacity = |value: f64| vec![(LayoutSettings::ModernUiFrostedGlassOpacity.as_str().to_string(), Dynamic::Number(value))];
        assert_eq!(
            report_settings(Settings { application: Some(opacity(50.0)), user: Some(opacity(75.0)), workspace: Some(opacity(100.0)) }),
            vec![
                event("workbench.modernUIFrostedGlassOpacity", Some("50".to_string()), "APPLICATION"),
                event("workbench.modernUIFrostedGlassOpacity", Some("75".to_string()), "USER_LOCAL"),
                event("workbench.modernUIFrostedGlassOpacity", Some("100".to_string()), "WORKSPACE"),
            ]
        );
    });
}
