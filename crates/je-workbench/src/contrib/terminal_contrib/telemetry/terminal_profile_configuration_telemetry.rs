//! Porte de `src/vs/workbench/contrib/terminalContrib/telemetry/browser/terminalProfileConfigurationTelemetry.ts`: a telemetria
//! das configurações de perfil do terminal (padrão, automação e agente), sem coletar o conteúdo dos perfis.
//!
//! Decisões de tradução:
//! - `terminalProfileSettings` é a função [`terminal_profile_settings`], e o `TerminalChatAgentToolsSettingId.TerminalProfile*`
//!   são as constantes de `chat_agent_tools::terminal_chat_agent_tools_configuration`.
//! - O `Map<settingId, boolean>` (`_configuredSettings`) é um `HashMap` compartilhado com o ouvinte da configuração.
//! - `telemetryService.publicLog2<...>(name, { ...setting, ... })` é o `public_log2` com o objeto dinâmico dos mesmos campos.
//!   As tipagens de evento e de classificação são só do compilador do TypeScript e não existem aqui.
//! - `ConfigurationTargetToString(event.source) ?? 'UNKNOWN'` é [`configuration_target_to_string`] com o `UNKNOWN` quando
//!   o evento não tem origem.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, configuration_target_to_string};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::terminal::terminal_settings::TerminalSettingId;

use crate::contrib::terminal_contrib::chat_agent_tools::terminal_chat_agent_tools_configuration::{
    TERMINAL_PROFILE_LINUX_SETTING_ID, TERMINAL_PROFILE_MAC_OS_SETTING_ID, TERMINAL_PROFILE_WINDOWS_SETTING_ID,
};

/// O elemento de `terminalProfileSettings`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalProfileSetting {
    pub setting_id: &'static str,
    pub profile_type: &'static str,
    pub os: &'static str,
}

/// `terminalProfileSettings` (uma função porque o `as_str` dos identificadores não é `const`).
pub fn terminal_profile_settings() -> [TerminalProfileSetting; 9] {
    [
        TerminalProfileSetting { setting_id: TERMINAL_PROFILE_LINUX_SETTING_ID, profile_type: "chat", os: "linux" },
        TerminalProfileSetting { setting_id: TERMINAL_PROFILE_MAC_OS_SETTING_ID, profile_type: "chat", os: "osx" },
        TerminalProfileSetting { setting_id: TERMINAL_PROFILE_WINDOWS_SETTING_ID, profile_type: "chat", os: "windows" },
        TerminalProfileSetting { setting_id: TerminalSettingId::AutomationProfileLinux.as_str(), profile_type: "automation", os: "linux" },
        TerminalProfileSetting { setting_id: TerminalSettingId::AutomationProfileMacOs.as_str(), profile_type: "automation", os: "osx" },
        TerminalProfileSetting { setting_id: TerminalSettingId::AutomationProfileWindows.as_str(), profile_type: "automation", os: "windows" },
        TerminalProfileSetting { setting_id: TerminalSettingId::DefaultProfileLinux.as_str(), profile_type: "default", os: "linux" },
        TerminalProfileSetting { setting_id: TerminalSettingId::DefaultProfileMacOs.as_str(), profile_type: "default", os: "osx" },
        TerminalProfileSetting { setting_id: TerminalSettingId::DefaultProfileWindows.as_str(), profile_type: "default", os: "windows" },
    ]
}

/// `{ ...setting, ...extra }`: o objeto do evento, com os campos do `setting` primeiro.
fn event_data(setting: &TerminalProfileSetting, extra: Vec<(&str, Dynamic)>) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> =
        vec![("settingId", Dynamic::string(setting.setting_id)), ("profileType", Dynamic::string(setting.profile_type)), ("os", Dynamic::string(setting.os))];
    entries.extend(extra);
    Dynamic::object(entries)
}

/// `_isConfigured(configurationService, settingId)`.
fn is_configured(configuration_service: &dyn IConfigurationService, setting_id: &str) -> bool {
    !configuration_service.get_value(Some(setting_id), None).is_nullish()
}

/// `TerminalProfileConfigurationTelemetry`.
pub struct TerminalProfileConfigurationTelemetry {
    base: Rc<Disposable>,
}

impl TerminalProfileConfigurationTelemetry {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>, telemetry_service: Rc<dyn ITelemetryService>) -> Rc<Self> {
        let base = Disposable::new();
        let configured_settings: Rc<RefCell<HashMap<&'static str, bool>>> = Rc::default();

        for setting in &terminal_profile_settings() {
            let configured = is_configured(configuration_service.as_ref(), setting.setting_id);
            configured_settings.borrow_mut().insert(setting.setting_id, configured);
            if configured {
                Self::report_setting_state(telemetry_service.as_ref(), setting);
            }
        }

        let listener_configuration_service = configuration_service.clone();
        base.register(configuration_service.on_did_change_configuration().listen(move |event| {
            for setting in &terminal_profile_settings() {
                if !event.affects_configuration(setting.setting_id, Some(&ConfigurationOverrides::default())) {
                    continue;
                }

                let source = event.source.get().map_or("UNKNOWN", configuration_target_to_string);
                Self::report_setting_changed(listener_configuration_service.as_ref(), telemetry_service.as_ref(), &configured_settings, setting, source);
            }
        }));
        Rc::new(Self { base })
    }

    fn report_setting_state(telemetry_service: &dyn ITelemetryService, setting: &TerminalProfileSetting) {
        telemetry_service.public_log2("terminal/profileSettingState", Some(event_data(setting, vec![("configured", Dynamic::Bool(true))])));
    }

    fn report_setting_changed(
        configuration_service: &dyn IConfigurationService,
        telemetry_service: &dyn ITelemetryService,
        configured_settings: &RefCell<HashMap<&'static str, bool>>,
        setting: &TerminalProfileSetting,
        source: &str,
    ) {
        let configured = is_configured(configuration_service, setting.setting_id);
        let was_configured = configured_settings.borrow().get(setting.setting_id).copied().unwrap_or(false);
        let change_type = match (configured, was_configured) {
            (true, true) => "changed",
            (true, false) => "added",
            (false, true) => "removed",
            (false, false) => "changed",
        };
        configured_settings.borrow_mut().insert(setting.setting_id, configured);

        telemetry_service.public_log2(
            "terminal/profileSettingChanged",
            Some(event_data(
                setting,
                vec![("configured", Dynamic::Bool(configured)), ("changeType", Dynamic::string(change_type)), ("source", Dynamic::string(source))],
            )),
        );
    }
}

impl IDisposable for TerminalProfileConfigurationTelemetry {
    fn dispose(&self) {
        self.base.dispose();
    }
}
