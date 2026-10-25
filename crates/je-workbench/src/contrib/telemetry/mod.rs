//! Porte de `src/vs/workbench/contrib/telemetry/browser/telemetry.contribution.ts`: o
//! `ConfigurationTelemetryContribution`, que reporta o valor explícito (por escopo de configuração) de um conjunto fechado de
//! configurações, e só de valores que não carregam texto livre do usuário.
//!
//! Recortes:
//! - O `TelemetryContribution` (evento `workspaceLoad`, telemetria de arquivos, `ErrorTelemetry` e o comando
//!   `workbench.action.showTelemetry`) fica para quando `ITextFileService`, `IOutputService`, `ILoggerService`,
//!   `IPaneCompositePartService` e `IKeybindingService` entrarem no porte do workbench; o oráculo não o cobre em
//!   `telemetry.contribution.test.ts`.
//! - O `registerWorkbenchContribution(..., LifecyclePhase.Eventually)` do fim do módulo fica com quem monta o workbench.
//!
//! Divergências:
//! - `publicLog2<UpdatedSettingEvent, ...>` é `public_log2` com um objeto [`Dynamic`] `{ settingValue, source }`; o
//!   `settingValue` ausente é `undefined`. As classificações de telemetria (tipos só de compilação) não têm destino.
//! - O `switch (key)` é a tabela [`REPORTED_SETTINGS`] (chave da configuração e nome do evento) mais o caso próprio de
//!   `window.newWindowProfile`.
//! - `TerminalContribSettingId.OutputLocation` é `chat.tools.terminal.outputLocation`, `SuggestEnabled` é
//!   `terminal.integrated.suggest.enabled` e `AgentSandboxEnabled` é `chat.agent.sandbox.enabled`, os valores do original.
//! - `value.toString()` de um número é o `JSON.stringify` dele, que para um número finito é o mesmo texto do JavaScript.

use std::rc::Rc;

use je_base::dynamic::{Dynamic, json_stringify};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::JsString;
use je_extensions::management::VERIFY_EXTENSION_SIGNATURE_CONFIG_KEY;
use je_platform::configuration::registry::{PropertySchema, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget, configuration_target_to_string};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::user_data_profile::user_data_profile::IUserDataProfilesService;

use crate::contrib::extensions::extensions::{AUTO_RESTART_CONFIGURATION_KEY, AUTO_UPDATE_CONFIGURATION_KEY};
use crate::services::layout::layout_service::LayoutSettings;

/// `TerminalContribSettingId.OutputLocation` (`TerminalChatAgentToolsSettingId.OutputLocation`).
const TERMINAL_OUTPUT_LOCATION_SETTING: &str = "chat.tools.terminal.outputLocation";

/// `TerminalContribSettingId.SuggestEnabled` (`TerminalSuggestSettingId.Enabled`).
const TERMINAL_SUGGEST_ENABLED_SETTING: &str = "terminal.integrated.suggest.enabled";

/// `TerminalContribSettingId.AgentSandboxEnabled` (`AgentSandboxSettingId.AgentSandboxEnabled`).
const AGENT_SANDBOX_ENABLED_SETTING: &str = "chat.agent.sandbox.enabled";

/// `window.newWindowProfile`, o único caso do `switch` que não reporta o valor da configuração como está.
const NEW_WINDOW_PROFILE_SETTING: &str = "window.newWindowProfile";

/// Os `case` do `switch (key)` que reportam o valor da configuração: a chave e o nome do evento.
fn reported_settings() -> [(&'static str, &'static str); 14] {
    [
        (LayoutSettings::ActivityBarLocation.as_str(), "workbench.activityBar.location"),
        (LayoutSettings::ModernUi.as_str(), "workbench.experimental.modernUI"),
        (LayoutSettings::ModernUiFrostedGlass.as_str(), "workbench.modernUIFrostedGlass"),
        (LayoutSettings::ModernUiFrostedGlassOpacity.as_str(), "workbench.modernUIFrostedGlassOpacity"),
        (LayoutSettings::ModernUiDensity.as_str(), "window.density.layout"),
        (AUTO_UPDATE_CONFIGURATION_KEY, "extensions.autoUpdate"),
        ("editor.stickyScroll.enabled", "editor.stickyScroll.enabled"),
        ("typescript.experimental.expandableHover", "typescript.experimental.expandableHover"),
        ("window.titleBarStyle", "window.titleBarStyle"),
        ("workbench.secondarySideBar.defaultVisibility", "workbench.secondarySideBar.defaultVisibility"),
        (VERIFY_EXTENSION_SIGNATURE_CONFIG_KEY, "extensions.verifySignature"),
        (AUTO_RESTART_CONFIGURATION_KEY, "extensions.autoRestart"),
        (TERMINAL_OUTPUT_LOCATION_SETTING, "terminal.integrated.chatAgentTools.outputLocation"),
        (TERMINAL_SUGGEST_ENABLED_SETTING, "terminal.integrated.suggest.enabled"),
    ]
}

/// `schema?.enum?.includes(value)`.
fn enum_includes(schema: Option<&PropertySchema>, value: &JsString) -> bool {
    match schema.map(|schema| schema.get("enum")) {
        Some(Dynamic::Array(allowed)) => allowed.to_vec().iter().any(|candidate| matches!(candidate, Dynamic::String(text) if text == value)),
        _ => false,
    }
}

/// `ConfigurationTelemetryContribution`.
pub struct ConfigurationTelemetryContribution {
    base: Rc<Disposable>,
    configuration_service: Rc<dyn IConfigurationService>,
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    telemetry_service: Rc<dyn ITelemetryService>,
}

impl ConfigurationTelemetryContribution {
    pub fn new(
        configuration_service: Rc<dyn IConfigurationService>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<Self> {
        let this = Rc::new(Self { base: Disposable::new(), configuration_service, user_data_profiles_service, telemetry_service });

        let keys = this.configuration_service.keys();
        let application_keys = this.configuration_service.get_configuration_data().map(|data| data.application.keys).unwrap_or_default();
        for setting in &application_keys {
            this.report_telemetry(&setting.to_string_lossy(), ConfigurationTarget::Application);
        }
        for setting in &keys.user {
            this.report_telemetry(&setting.to_string_lossy(), ConfigurationTarget::UserLocal);
        }
        for setting in &keys.workspace {
            this.report_telemetry(&setting.to_string_lossy(), ConfigurationTarget::Workspace);
        }
        this
    }

    /// Report value of a setting only if it is an enum, boolean, or number or an array of those.
    fn get_value_to_report(&self, key: &str, target: ConfigurationTarget) -> Option<String> {
        let inspect_data = self.configuration_service.inspect(key, &ConfigurationOverrides::default());
        let inspected = match target {
            ConfigurationTarget::Application => inspect_data.application(),
            ConfigurationTarget::UserLocal => inspect_data.user(),
            _ => inspect_data.workspace(),
        };
        let value = inspected.map_or(Dynamic::Undefined, |inspected| inspected.value());
        let scalar_text = |value: &Dynamic| match value {
            Dynamic::Number(_) | Dynamic::Bool(_) => json_stringify(value),
            _ => None,
        };
        if let Some(text) = scalar_text(&value) {
            return Some(text);
        }

        let properties = configuration_registry().get_configuration_properties();
        let schema = properties.get(key);
        match &value {
            Dynamic::String(text) => enum_includes(schema.as_ref(), text).then(|| text.to_string_lossy()),
            Dynamic::Array(items) => {
                let items = items.to_vec();
                let all_reportable = items.iter().all(|item| match item {
                    Dynamic::Number(_) | Dynamic::Bool(_) => true,
                    Dynamic::String(text) => enum_includes(schema.as_ref(), text),
                    _ => false,
                });
                if all_reportable { json_stringify(&value) } else { None }
            }
            _ => None,
        }
    }

    fn report_telemetry(&self, key: &str, target: ConfigurationTarget) {
        let source = configuration_target_to_string(target);

        if key == NEW_WINDOW_PROFILE_SETTING {
            let value_to_report = self.get_value_to_report(key, target);
            let setting_value = if value_to_report.as_deref() == Some(self.user_data_profiles_service.default_profile().name.as_str()) { "default" } else { "custom" };
            self.publish(key, Some(setting_value.to_string()), source);
            return;
        }

        if let Some((_, event_name)) = reported_settings().iter().find(|(setting, _)| *setting == key) {
            self.publish(event_name, self.get_value_to_report(key, target), source);
        } else if key == AGENT_SANDBOX_ENABLED_SETTING {
            self.publish("chat.agent.sandbox", self.get_value_to_report(key, target), source);
        }
    }

    /// `telemetryService.publicLog2(eventName, { settingValue, source })`.
    fn publish(&self, event_name: &str, setting_value: Option<String>, source: &str) {
        let setting_value = setting_value.map_or(Dynamic::Undefined, |value| Dynamic::string(&value));
        self.telemetry_service.public_log2(event_name, Some(Dynamic::object([("settingValue", setting_value), ("source", Dynamic::string(source))])));
    }
}

impl IDisposable for ConfigurationTelemetryContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}
