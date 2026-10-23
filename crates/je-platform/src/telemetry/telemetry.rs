//! Porte de `src/vs/platform/telemetry/common/telemetry.ts`: o contrato do serviço de telemetria.
//!
//! Divergências: `publicLog2<E, T>` (a classificação GDPR é só tipo do TypeScript) é `public_log2` com o
//! dado como [`Dynamic`]; `ITelemetryData` é o de [`je_base::actions`], que é o mesmo tipo do original.

use je_base::dynamic::Dynamic;
pub use je_base::actions::ITelemetryData;
use je_instantiation::service_id;

/// `TelemetryLevel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TelemetryLevel {
    None = 0,
    Crash = 1,
    Error = 2,
    Usage = 3,
}

/// `TelemetryConfiguration`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelemetryConfiguration {
    Off,
    Crash,
    Error,
    On,
}

impl TelemetryConfiguration {
    /// O valor de texto da configuração.
    pub fn as_str(self) -> &'static str {
        match self {
            TelemetryConfiguration::Off => "off",
            TelemetryConfiguration::Crash => "crash",
            TelemetryConfiguration::Error => "error",
            TelemetryConfiguration::On => "all",
        }
    }
}

/// `ITelemetryService`.
pub trait ITelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel;
    fn session_id(&self) -> String;
    fn machine_id(&self) -> String;
    fn sqm_id(&self) -> String;
    fn dev_device_id(&self) -> String;
    fn first_session_date(&self) -> String;
    fn msft_internal(&self) -> Option<bool> {
        None
    }
    /// Se a telemetria de erro será enviada. Falso: `public_log_error` não faz nada.
    fn send_error_telemetry(&self) -> bool;

    /// Obsoleto: prefira `public_log2`.
    fn public_log(&self, event_name: &str, data: Option<ITelemetryData>);
    /// Envia um evento aprovado quanto à privacidade. Só chame com aprovação.
    fn public_log2(&self, event_name: &str, data: Option<Dynamic>);
    /// Obsoleto: prefira `public_log_error2`.
    fn public_log_error(&self, error_event_name: &str, data: Option<ITelemetryData>);
    fn public_log_error2(&self, event_name: &str, data: Option<Dynamic>);

    fn set_experiment_property(&self, name: &str, value: &str);
    /// Define uma propriedade comum anexada a todos os eventos (ou a remove, com `None`). As propriedades
    /// comuns entram depois da limpeza de dados pessoais e os dados do evento não as sobrescrevem.
    fn set_common_property(&self, name: &str, value: Option<CommonPropertyValue>);
}

/// `string | boolean | undefined` de `setCommonProperty` e de `ICommonProperties`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommonPropertyValue {
    Text(String),
    Bool(bool),
}

service_id!(pub I_TELEMETRY_SERVICE: dyn ITelemetryService = "telemetryService");

/// `telemetryLevelEnabled`.
pub fn telemetry_level_enabled(service: &dyn ITelemetryService, level: TelemetryLevel) -> bool {
    service.telemetry_level() >= level
}

/// Troca `/` e `\` por `|` nos identificadores de modelo, para o pipeline de telemetria não os apagar como caminhos.
pub fn escape_model_id_for_telemetry(model_id: Option<&str>) -> Option<String> {
    model_id.map(|id| id.replace(['/', '\\'], "|"))
}

/// `ITelemetryEndpoint`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelemetryEndpoint {
    pub id: String,
    pub ai_key: String,
    pub send_error_telemetry: bool,
}

/// `ICustomEndpointTelemetryService`.
pub trait ICustomEndpointTelemetryService {
    fn public_log(&self, endpoint: &TelemetryEndpoint, event_name: &str, data: Option<ITelemetryData>);
    fn public_log_error(&self, endpoint: &TelemetryEndpoint, error_event_name: &str, data: Option<ITelemetryData>);
}

service_id!(pub I_CUSTOM_ENDPOINT_TELEMETRY_SERVICE: dyn ICustomEndpointTelemetryService = "customEndpointTelemetryService");

// Chaves de armazenamento.
pub const CURRENT_SESSION_DATE_STORAGE_KEY: &str = "telemetry.currentSessionDate";
pub const FIRST_SESSION_DATE_STORAGE_KEY: &str = "telemetry.firstSessionDate";
pub const LAST_SESSION_DATE_STORAGE_KEY: &str = "telemetry.lastSessionDate";
pub const MACHINE_ID_KEY: &str = "telemetry.machineId";
pub const SQM_ID_KEY: &str = "telemetry.sqmId";
pub const DEV_DEVICE_ID_KEY: &str = "telemetry.devDeviceId";

// Chaves de configuração.
pub const TELEMETRY_SECTION_ID: &str = "telemetry";
pub const TELEMETRY_SETTING_ID: &str = "telemetry.telemetryLevel";
pub const TELEMETRY_CRASH_REPORTER_SETTING_ID: &str = "telemetry.enableCrashReporter";
pub const TELEMETRY_OLD_SETTING_ID: &str = "telemetry.enableTelemetry";
