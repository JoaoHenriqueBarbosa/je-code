//! Porte de `src/vs/workbench/contrib/terminalContrib/telemetry/test/browser/terminalProfileConfigurationTelemetry.test.ts` (suíte `TerminalProfileConfigurationTelemetry`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo, o
//!   `TestConfigurationService`, o [`TestTelemetryService`] e o `TerminalProfileConfigurationTelemetry`, registrado no guarda.
//! - **`class TestTelemetryService extends NullTelemetryServiceShape`.** Sem herança em Rust, o [`TestTelemetryService`] delega
//!   tudo ao `NullTelemetryServiceShape` e sobrescreve o `public_log2`, que guarda `{ name, data }` quando o nome não é vazio.
//! - **`changeSetting(settingId, value, source)`.** É [`Fixture::change_setting`]: grava a configuração e dispara o
//!   `ConfigurationChangeEvent` real da chave com a origem dada (o literal do original afirma a chave e `overrides !== undefined`,
//!   que é o que o `affectsConfiguration(key, {})` do evento real faz para uma chave sem filhos).
//! - **`assert.deepStrictEqual(telemetryService.events, ...)`.** Compara o array dinâmico `{ name, data }` com o
//!   `assert_deep_strict_equal` de `je-base/tests/common`.
//! - **`as const` de `profileSettings`.** É a lista [`PROFILE_SETTINGS`] com o valor de cada configuração como [`Dynamic`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod configuration_event_support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_platform::configuration::ConfigurationTarget;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::telemetry::telemetry::{CommonPropertyValue, ITelemetryData, ITelemetryService, TelemetryLevel};
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_platform::terminal::terminal_settings::TerminalSettingId;
use je_workbench::contrib::terminal_contrib::chat_agent_tools::terminal_chat_agent_tools_configuration::{
    TERMINAL_PROFILE_LINUX_SETTING_ID, TERMINAL_PROFILE_MAC_OS_SETTING_ID, TERMINAL_PROFILE_WINDOWS_SETTING_ID,
};
use je_workbench::contrib::terminal_contrib::telemetry::terminal_profile_configuration_telemetry::TerminalProfileConfigurationTelemetry;

/// `TestTelemetryService`.
#[derive(Default)]
struct TestTelemetryService {
    null: NullTelemetryServiceShape,
    /// `readonly events: { name, data }[]`.
    events: RefCell<Vec<(String, Option<Dynamic>)>>,
}

impl TestTelemetryService {
    /// `telemetryService.events` como o array dinâmico `{ name, data }`.
    fn events(&self) -> Dynamic {
        Dynamic::array(self.events.borrow().iter().map(|(name, data)| Dynamic::object([("name", Dynamic::string(name)), ("data", data.clone().unwrap_or(Dynamic::Undefined))])))
    }
}

impl ITelemetryService for TestTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        self.null.telemetry_level()
    }

    fn session_id(&self) -> String {
        self.null.session_id()
    }

    fn machine_id(&self) -> String {
        self.null.machine_id()
    }

    fn sqm_id(&self) -> String {
        self.null.sqm_id()
    }

    fn dev_device_id(&self) -> String {
        self.null.dev_device_id()
    }

    fn first_session_date(&self) -> String {
        self.null.first_session_date()
    }

    fn send_error_telemetry(&self) -> bool {
        self.null.send_error_telemetry()
    }

    fn public_log(&self, event_name: &str, data: Option<ITelemetryData>) {
        self.null.public_log(event_name, data);
    }

    /// `override publicLog2(eventName?, data?)`.
    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        if !event_name.is_empty() {
            self.events.borrow_mut().push((event_name.to_owned(), data));
        }
    }

    fn public_log_error(&self, error_event_name: &str, data: Option<ITelemetryData>) {
        self.null.public_log_error(error_event_name, data);
    }

    fn public_log_error2(&self, event_name: &str, data: Option<Dynamic>) {
        self.null.public_log_error2(event_name, data);
    }

    fn set_experiment_property(&self, name: &str, value: &str) {
        self.null.set_experiment_property(name, value);
    }

    fn set_common_property(&self, name: &str, value: Option<CommonPropertyValue>) {
        self.null.set_common_property(name, value);
    }
}

/// O elemento de `profileSettings`.
struct ProfileSetting {
    setting_id: &'static str,
    profile_type: &'static str,
    os: &'static str,
    value: Dynamic,
}

/// `{ path }`.
fn path_value(path: &str) -> Dynamic {
    Dynamic::object([("path", Dynamic::string(path))])
}

/// `profileSettings`.
fn profile_settings() -> Vec<ProfileSetting> {
    vec![
        ProfileSetting { setting_id: TERMINAL_PROFILE_LINUX_SETTING_ID, profile_type: "chat", os: "linux", value: path_value("/bin/bash") },
        ProfileSetting { setting_id: TERMINAL_PROFILE_MAC_OS_SETTING_ID, profile_type: "chat", os: "osx", value: path_value("/bin/zsh") },
        ProfileSetting { setting_id: TERMINAL_PROFILE_WINDOWS_SETTING_ID, profile_type: "chat", os: "windows", value: path_value("pwsh.exe") },
        ProfileSetting { setting_id: TerminalSettingId::AutomationProfileLinux.as_str(), profile_type: "automation", os: "linux", value: path_value("/bin/bash") },
        ProfileSetting { setting_id: TerminalSettingId::AutomationProfileMacOs.as_str(), profile_type: "automation", os: "osx", value: path_value("/bin/zsh") },
        ProfileSetting { setting_id: TerminalSettingId::AutomationProfileWindows.as_str(), profile_type: "automation", os: "windows", value: path_value("pwsh.exe") },
        ProfileSetting { setting_id: TerminalSettingId::DefaultProfileLinux.as_str(), profile_type: "default", os: "linux", value: Dynamic::string("bash") },
        ProfileSetting { setting_id: TerminalSettingId::DefaultProfileMacOs.as_str(), profile_type: "default", os: "osx", value: Dynamic::string("zsh") },
        ProfileSetting { setting_id: TerminalSettingId::DefaultProfileWindows.as_str(), profile_type: "default", os: "windows", value: Dynamic::string("PowerShell") },
    ]
}

/// O `data` de um evento `terminal/profileSettingChanged`: `{ settingId, profileType, os, configured, changeType, source }`.
fn changed_data(setting_id: &str, profile_type: &str, os: &str, configured: bool, change_type: &str, source: &str) -> Dynamic {
    Dynamic::object([
        ("settingId", Dynamic::string(setting_id)),
        ("profileType", Dynamic::string(profile_type)),
        ("os", Dynamic::string(os)),
        ("configured", Dynamic::Bool(configured)),
        ("changeType", Dynamic::string(change_type)),
        ("source", Dynamic::string(source)),
    ])
}

/// `{ name, data }`.
fn event(name: &str, data: Dynamic) -> Dynamic {
    Dynamic::object([("name", Dynamic::string(name)), ("data", data)])
}

/// O `setup` da suíte.
struct Fixture {
    configuration_service: Rc<TestConfigurationService>,
    telemetry_service: Rc<TestTelemetryService>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    guard: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration_service = TestConfigurationService::new(None);
        let telemetry_service = Rc::new(TestTelemetryService::default());
        guard.add_rc(TerminalProfileConfigurationTelemetry::new(configuration_service.clone(), telemetry_service.clone()));
        Self { configuration_service, telemetry_service, guard }
    }

    /// `changeSetting(settingId, value, source = ConfigurationTarget.USER)`.
    async fn change_setting(&self, setting_id: &str, value: Dynamic, source: ConfigurationTarget) {
        self.configuration_service.set_user_configuration(setting_id, value, None).await.unwrap();
        configuration_event_support::fire_configuration_change_from(&self.configuration_service, &[setting_id], source);
    }
}

#[test]
fn reports_changes_for_each_terminal_profile_setting_without_profile_contents() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        for setting in profile_settings() {
            f.change_setting(setting.setting_id, setting.value.clone(), ConfigurationTarget::User).await;
        }

        let expected = Dynamic::array(
            profile_settings()
                .iter()
                .map(|setting| event("terminal/profileSettingChanged", changed_data(setting.setting_id, setting.profile_type, setting.os, true, "added", "USER"))),
        );
        assert_deep_strict_equal(&f.telemetry_service.events(), &expected);
        let _ = &f.guard;
    });
}

#[test]
fn reports_settings_that_already_have_configured_values() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let settings = profile_settings();
    let configured_settings = [&settings[0], &settings[7]];
    let configuration_service = TestConfigurationService::new(Some(Dynamic::object(configured_settings.iter().map(|setting| (setting.setting_id, setting.value.clone())))));
    let telemetry_service = Rc::new(TestTelemetryService::default());
    guard.add_rc(TerminalProfileConfigurationTelemetry::new(configuration_service, telemetry_service.clone()));

    let expected = Dynamic::array(configured_settings.iter().map(|setting| {
        event(
            "terminal/profileSettingState",
            Dynamic::object([
                ("settingId", Dynamic::string(setting.setting_id)),
                ("profileType", Dynamic::string(setting.profile_type)),
                ("os", Dynamic::string(setting.os)),
                ("configured", Dynamic::Bool(true)),
            ]),
        )
    }));
    assert_deep_strict_equal(&telemetry_service.events(), &expected);
}

#[test]
fn reports_added_changed_and_removed_settings_and_ignores_unrelated_settings() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let default_profile_linux = TerminalSettingId::DefaultProfileLinux.as_str();
        f.change_setting(default_profile_linux, Dynamic::string("bash"), ConfigurationTarget::User).await;
        f.change_setting(default_profile_linux, Dynamic::string("zsh"), ConfigurationTarget::Workspace).await;
        f.change_setting(default_profile_linux, Dynamic::Null, ConfigurationTarget::User).await;
        f.change_setting(TerminalSettingId::FontSize.as_str(), Dynamic::Number(16.0), ConfigurationTarget::User).await;

        let expected = Dynamic::array([
            event("terminal/profileSettingChanged", changed_data(default_profile_linux, "default", "linux", true, "added", "USER")),
            event("terminal/profileSettingChanged", changed_data(default_profile_linux, "default", "linux", true, "changed", "WORKSPACE")),
            event("terminal/profileSettingChanged", changed_data(default_profile_linux, "default", "linux", false, "removed", "USER")),
        ]);
        assert_deep_strict_equal(&f.telemetry_service.events(), &expected);
    });
}
