//! Porte de `src/vs/platform/externalTerminal/common/externalTerminal.ts`: o contrato do terminal externo
//! (`IExternalTerminalService`), as configurações de qual terminal abrir em cada sistema e o terminal padrão do macOS.
//!
//! Decisões de tradução:
//! - **`IExternalTerminalService` é o trait [`IExternalTerminalService`]**, com o identificador
//!   [`I_EXTERNAL_TERMINAL_SERVICE`]. As promessas são [`Promise`] (o `Promise<number | undefined>` de `runInTerminal` é
//!   `Promise<Option<i32>>`), os argumentos opcionais são `Option` e o `cwd: string | undefined` de `openTerminal` é
//!   `Option<&str>`.
//! - **`IExternalTerminalSettings` e `ITerminalForPlatform`** mantêm os nomes e os campos do original, em `snake_case`
//!   (`linuxExec` é `linux_exec`); um campo `?:` é `Option<String>`.
//! - **`IExternalTerminalConfiguration`** guarda `explorerKind` como o enum [`ExplorerKind`] (`'integrated' | 'external' |
//!   'both'`, com [`ExplorerKind::as_str`] para o texto das configurações).

use je_base::promise::Promise;
use je_instantiation::service_id;

use crate::terminal::terminal::ITerminalEnvironment;

/// `IExternalTerminalSettings`: o programa de terminal de cada sistema (`undefined` usa o padrão do sistema).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IExternalTerminalSettings {
    pub linux_exec: Option<String>,
    pub osx_exec: Option<String>,
    pub windows_exec: Option<String>,
}

/// `ITerminalForPlatform`: o terminal padrão de cada sistema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITerminalForPlatform {
    pub windows: String,
    pub linux: String,
    pub osx: String,
}

/// `IExternalTerminalService`.
pub trait IExternalTerminalService {
    /// `openTerminal(configuration, cwd)`: abre o terminal externo no diretório `cwd`, se dado.
    fn open_terminal(&self, configuration: &IExternalTerminalSettings, cwd: Option<&str>) -> Promise<()>;

    /// `runInTerminal(title, cwd, args, env, settings)`: roda `args` no terminal externo, em `cwd`, com as variáveis de
    /// ambiente `env`.
    fn run_in_terminal(&self, title: &str, cwd: &str, args: &[String], env: &ITerminalEnvironment, settings: &IExternalTerminalSettings) -> Promise<Option<i32>>;

    /// `getDefaultTerminalForPlatforms()`.
    fn get_default_terminal_for_platforms(&self) -> Promise<ITerminalForPlatform>;
}

service_id!(pub I_EXTERNAL_TERMINAL_SERVICE: dyn IExternalTerminalService = "externalTerminal");

/// O `explorerKind` de `IExternalTerminalConfiguration`: `'integrated' | 'external' | 'both'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplorerKind {
    Integrated,
    External,
    Both,
}

impl ExplorerKind {
    /// O valor textual das configurações (`'integrated'`, `'external'` ou `'both'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Integrated => "integrated",
            Self::External => "external",
            Self::Both => "both",
        }
    }
}

/// O objeto `terminal` de `IExternalTerminalConfiguration`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalTerminalConfigurationTerminal {
    pub explorer_kind: ExplorerKind,
    pub external: IExternalTerminalSettings,
}

/// `IExternalTerminalConfiguration`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IExternalTerminalConfiguration {
    pub terminal: ExternalTerminalConfigurationTerminal,
}

/// `DEFAULT_TERMINAL_OSX`.
pub const DEFAULT_TERMINAL_OSX: &str = "Terminal.app";
