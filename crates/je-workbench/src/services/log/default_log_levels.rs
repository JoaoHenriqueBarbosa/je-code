//! Porte (parcial) de `src/vs/workbench/services/log/common/defaultLogLevels.ts`: o contrato `IDefaultLogLevelsService` e o
//! `DefaultLogLevels`, que o `OutputService` lê.
//!
//! Divergências: a classe `DefaultLogLevelsService` (que lê e grava os níveis no `argv.json` pelo `IJSONEditingService`) não entrou,
//! porque nenhum consumidor portado a instancia; quem monta o workbench fornece o serviço. `ParsedArgvLogLevels` também não entrou.
//! `extensions?: [string, LogLevel][]` é `Vec<(String, LogLevel)>`.

use je_base::event::Event;
use je_base::promise::Promise;
use je_platform::log::LogLevel;

/// `DefaultLogLevels` (`Required<Readonly<ParsedArgvLogLevels>>`).
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultLogLevels {
    pub default: LogLevel,
    pub extensions: Vec<(String, LogLevel)>,
}

/// `IDefaultLogLevelsService`.
pub trait IDefaultLogLevelsService {
    /// `defaultLogLevels`.
    fn default_log_levels(&self) -> DefaultLogLevels;

    /// `onDidChangeDefaultLogLevels`.
    fn on_did_change_default_log_levels(&self) -> Event<DefaultLogLevels>;

    /// `getDefaultLogLevel(extensionId?)`.
    fn get_default_log_level(&self, extension_id: Option<&str>) -> LogLevel;

    /// `setDefaultLogLevel(logLevel, extensionId?)`.
    fn set_default_log_level(&self, log_level: LogLevel, extension_id: Option<&str>) -> Promise<()>;
}

je_instantiation::service_id!(pub I_DEFAULT_LOG_LEVELS_SERVICE: dyn IDefaultLogLevelsService = "IDefaultLogLevelsService");
