//! Porte parcial de `src/vs/platform/terminal/common/terminal.ts`: só o que o `terminalEnvironment.ts`, o
//! `terminalProfiles.ts` e o `requestStore.ts` usam (os tipos de shell, o ícone do terminal, o ambiente do
//! terminal, `ITerminalProfile`, os perfis de extensão e `IShellLaunchConfig`).
//!
//! Representação:
//! - Os `const enum` de string (`PosixShellType`, `WindowsShellType`, `GeneralShellType`) são enums com
//!   `as_str()` devolvendo o valor do original. `TerminalShellType` (a união dos três com `undefined`) é
//!   `Option<ShellType>`, e `ShellType` agrupa as três famílias.
//! - `TerminalIcon` (`ThemeIcon | URI | { light: URI; dark: URI }`) é o enum [`TerminalIcon`]. Onde o original
//!   guarda um `URI`, o porte guarda um [`UriLike`], que também aceita `UriComponents` (o objeto simples que
//!   chega revivido de um JSON): é o que o `isUriComponents` do `terminalProfiles.ts` distingue.
//! - `SingleOrMany<string>` é [`OneOrMany`]`<String>`. Os campos opcionais (`?`) são `Option`.
//! - `ITerminalEnvironment` (`{ [key]: string | null | undefined }`) é a lista de pares na ordem do objeto,
//!   com [`TerminalEnvironmentValue`] distinguindo o texto, o `null` e o `undefined`.
//! - `IExtensionTerminalProfile extends ITerminalProfileContribution` é a composição com `Deref`.
//! - `IShellLaunchConfig`: `waitOnExit` é [`WaitOnExitValue`] (o `(exitCode) => string` é um `Rc<dyn Fn>`) e a
//!   chave `[remoteResolverTerminal]` (um `Symbol`) é o campo `remote_resolver_terminal`.
//!
//! Divergências (o que ainda falta do `terminal.ts`, a portar quando um consumidor pedir): `LocalReconnectConstants`;
//! `IShellLaunchConfigDto`;
//! `ICreateContributedTerminalProfileOptions`; `ITerminalProfileObject`; os seletores de comando; o registro de backends
//! (`TerminalExtensions`, `ITerminalBackendRegistry`). Os tipos do pty host e do pty service (`TerminalIpcChannels`,
//! `HeartbeatConstants`, `IHeartbeatService`, os tipos de layout, `IPtyService`, `IPtyServiceContribution`,
//! `IPtyHostController`, `IPtyHostService`, `ILocalPtyService`, `ISerializedTerminalState`...) estão em
//! [`super::pty_service`]; `TerminalSettingPrefix` e `TerminalSettingId`, em [`super::terminal_settings`]. O
//! `ITerminalBackend` fica em `je_workbench::contrib::terminal::common::terminal_backend`,
//! porque herda o `ITerminalBackendWslPath` do workbench.
//!
//! Contratos do processo (porte de `ITerminalChildProcess` e companhia):
//! - `IProcessProperty<T>` (`{ type, value }` com o valor tipado por `IProcessPropertyMap[T]`) é o enum
//!   [`IProcessProperty`], uma variante por [`ProcessPropertyType`]; `refreshProperty` e `updateProperty` trocam esse enum.
//! - Os métodos e eventos opcionais de `ITerminalChildProcess` (`onProcessReplayComplete?`, `onRestoreCommands?`, `detach?`,
//!   `freePortKillProcess?`) devolvem `Option`: `None` é o membro ausente.
//! - `string | IProcessDataEvent` é [`ProcessData`]. O [`IProcessDataEvent`] é compartilhado por `Rc` e tem campos mutáveis
//!   (`data` e `writePromise`), porque o original muta o objeto que o evento carrega.
//! - `ITerminalLaunchError | ITerminalLaunchResult | undefined` é `Option<`[`ITerminalLaunchOutcome`]`>`.
//! - `customPtyImplementation` de `IShellLaunchConfig` é uma função que cria o [`ITerminalChildProcess`].
//! - `ITerminalLogService` é um [`LogService`] (o `ILogService` do porte) com identificador próprio.
//!
//! Outra divergência: `ThemeIcon.color` é comparado por valor (o id da cor), enquanto o `terminalIconsEqual`
//! do original compara a referência do objeto `ThemeColor` com `===`.

use std::any::Any;
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use je_base::JsString;
use je_base::arrays::OneOrMany;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::uri::{URI, UriComponents, is_uri_components};
use je_instantiation::service_id;

use super::capabilities::capabilities::ISerializedCommandDetectionCapability;
use super::environment_variable::ISerializableEnvironmentVariableCollections;
use crate::log::LogService;
use crate::workspace::WorkspaceFolder;

/// Declara um `const enum` de string do original: as variantes e o `as_str()` com o valor.
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$variant_meta:meta])* $variant:ident = $value:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }
    };
}

string_enum! {
    /// `PosixShellType`.
    PosixShellType {
        Bash = "bash",
        Fish = "fish",
        Sh = "sh",
        Csh = "csh",
        Ksh = "ksh",
        Zsh = "zsh",
    }
}

string_enum! {
    /// `WindowsShellType`.
    WindowsShellType {
        CommandPrompt = "cmd",
        Wsl = "wsl",
        GitBash = "gitbash",
    }
}

string_enum! {
    /// `GeneralShellType`.
    GeneralShellType {
        Claude = "claude",
        Codex = "codex",
        CommandCode = "commandcode",
        Copilot = "copilot",
        Gemini = "gemini",
        PowerShell = "pwsh",
        Python = "python",
        Julia = "julia",
        NuShell = "nu",
        Node = "node",
        Xonsh = "xonsh",
    }
}

/// `PosixShellType | WindowsShellType | GeneralShellType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShellType {
    Posix(PosixShellType),
    Windows(WindowsShellType),
    General(GeneralShellType),
}

impl ShellType {
    /// O valor do `const enum` de dentro.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Posix(shell) => shell.as_str(),
            Self::Windows(shell) => shell.as_str(),
            Self::General(shell) => shell.as_str(),
        }
    }
}

macro_rules! shell_type_from {
    ($($family:ident($ty:ty)),+ $(,)?) => {
        $(impl From<$ty> for ShellType {
            fn from(shell: $ty) -> Self {
                Self::$family(shell)
            }
        })+
    };
}

shell_type_from!(Posix(PosixShellType), Windows(WindowsShellType), General(GeneralShellType));

/// `TerminalShellType`: a união com `undefined` é o `Option`.
pub type TerminalShellType = Option<ShellType>;

/// `URI | UriComponents`: o que um campo `URI` do original guarda de fato em tempo de execução (uma
/// instância ou o objeto simples revivido de um JSON).
#[derive(Clone, Debug, PartialEq)]
pub enum UriLike {
    Uri(URI),
    Components(UriComponents),
}

impl UriLike {
    fn as_any(&self) -> &dyn Any {
        match self {
            Self::Uri(uri) => uri,
            Self::Components(components) => components,
        }
    }

    /// `URI.isUri(value)`.
    pub fn is_uri(&self) -> bool {
        URI::is_uri(self.as_any())
    }

    /// `isUriComponents(value)`.
    pub fn is_uri_components(&self) -> bool {
        is_uri_components(self.as_any())
    }

    /// `value.path` (`undefined` num objeto simples sem caminho).
    pub fn path(&self) -> Option<&JsString> {
        match self {
            Self::Uri(uri) => Some(uri.path()),
            Self::Components(components) => components.path.as_ref(),
        }
    }

    /// `value.scheme`.
    pub fn scheme(&self) -> Option<&JsString> {
        match self {
            Self::Uri(uri) => Some(uri.scheme()),
            Self::Components(components) => components.scheme.as_ref(),
        }
    }
}

impl From<URI> for UriLike {
    fn from(uri: URI) -> Self {
        Self::Uri(uri)
    }
}

/// `{ light: URI; dark: URI }`.
#[derive(Clone, Debug, PartialEq)]
pub struct LightDarkIcon {
    pub light: UriLike,
    pub dark: UriLike,
}

/// `TerminalIcon`: `ThemeIcon | URI | { light: URI; dark: URI }`.
#[derive(Clone, Debug, PartialEq)]
pub enum TerminalIcon {
    Theme(ThemeIcon),
    Uri(UriLike),
    LightDark(LightDarkIcon),
}

/// `IReconnectionProperties`.
#[derive(Clone, Debug)]
pub struct IReconnectionProperties {
    pub owner_id: String,
    pub data: Option<Dynamic>,
}

/// `TerminalType`: `'Task' | 'Local'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalType {
    Task,
    Local,
}

/// `TitleEventSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TitleEventSource {
    /// From the API or the rename command that overrides any other type
    Api,
    /// From the process name property
    Process,
    /// From the VT sequence
    Sequence,
    /// Config changed
    Config,
}

/// `IFixedTerminalDimensions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IFixedTerminalDimensions {
    /// The fixed columns of the terminal.
    pub cols: Option<u32>,
    /// The fixed rows of the terminal.
    pub rows: Option<u32>,
}

/// `ITerminalTabAction`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITerminalTabAction {
    pub id: String,
    pub label: String,
    pub icon: Option<ThemeIcon>,
}

/// `WaitOnExitValue`: `boolean | string | ((exitCode: number) => string)`.
#[derive(Clone)]
pub enum WaitOnExitValue {
    Bool(bool),
    Text(String),
    Callback(Rc<dyn Fn(i32) -> String>),
}

/// O valor de uma chave de `ITerminalEnvironment`: `string | null | undefined`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalEnvironmentValue {
    Value(String),
    Null,
    Undefined,
}

/// `ITerminalEnvironment`: `{ [key: string]: string | null | undefined }`, os pares na ordem do objeto.
pub type ITerminalEnvironment = Vec<(String, TerminalEnvironmentValue)>;

/// O `cwd?: string | URI` de `IShellLaunchConfig`.
#[derive(Clone, Debug)]
pub enum ShellCwd {
    Path(String),
    Uri(URI),
}

/// O `initialText?: string | { text: string; trailingNewLine: boolean }` de `IShellLaunchConfig`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitialText {
    Text(String),
    WithTrailingNewLine { text: String, trailing_new_line: bool },
}

/// O `attachPersistentProcess` de `IShellLaunchConfig`: um terminal que anexa a um processo em execução.
#[derive(Clone)]
pub struct IAttachPersistentProcess {
    pub id: u32,
    pub find_revived_id: Option<bool>,
    pub pid: u32,
    pub title: String,
    pub title_source: TitleEventSource,
    pub cwd: String,
    pub icon: Option<TerminalIcon>,
    pub color: Option<String>,
    pub has_child_processes: Option<bool>,
    pub fixed_dimensions: Option<IFixedTerminalDimensions>,
    pub environment_variable_collections: Option<ISerializableEnvironmentVariableCollections>,
    pub reconnection_properties: Option<IReconnectionProperties>,
    pub type_: Option<TerminalType>,
    pub wait_on_exit: Option<WaitOnExitValue>,
    pub hide_from_user: Option<bool>,
    pub is_feature_terminal: Option<bool>,
    pub shell_integration_nonce: String,
    pub tab_actions: Option<Vec<ITerminalTabAction>>,
}

/// `IShellLaunchConfig`.
#[derive(Clone, Default)]
pub struct IShellLaunchConfig {
    /// A custom implementation of the child process (`customPtyImplementation(terminalId, cols, rows)`): recebe o id da
    /// instância, as colunas e as linhas, e devolve o processo.
    pub custom_pty_implementation: Option<CustomPtyImplementation>,

    /// The name of the terminal, if this is not set the name of the process will be used.
    pub name: Option<String>,

    /// A string to follow the name of the terminal with, indicating the type of terminal
    pub type_: Option<TerminalType>,

    /// The shell executable (bash, cmd, etc.).
    pub executable: Option<String>,

    /// The CLI arguments to use with executable, a string[] is in argv format and will be escaped,
    /// a string is in "CommandLine" pre-escaped format and will be used as is. The string option is
    /// only supported on Windows and will throw an exception if used on macOS or Linux.
    pub args: Option<OneOrMany<String>>,

    /// The current working directory of the terminal, this overrides the `terminal.integrated.cwd`
    /// settings key.
    pub cwd: Option<ShellCwd>,

    /// A custom environment for the terminal, if this is not set the environment will be inherited
    /// from the VS Code process.
    pub env: Option<ITerminalEnvironment>,

    /// Whether to ignore a custom cwd from the `terminal.integrated.cwd` settings key (e.g. if the
    /// shell is being launched by an extension).
    pub ignore_configuration_cwd: Option<bool>,

    /// The reconnection properties for this terminal
    pub reconnection_properties: Option<IReconnectionProperties>,

    /// Whether to wait for a key press before closing the terminal.
    pub wait_on_exit: Option<WaitOnExitValue>,

    /// A string including ANSI escape sequences that will be written to the terminal emulator
    /// _before_ the terminal process has launched, when a string is specified, a trailing \n is
    /// added at the end. This allows for example the terminal instance to display a styled message
    /// as the first line of the terminal. Use \x1b over \033 or \e for the escape control character.
    pub initial_text: Option<InitialText>,

    /// A UUID generated by the extension host process for terminals created on the extension host process.
    pub ext_host_terminal_id: Option<String>,

    /// This is a terminal that attaches to an already running terminal.
    pub attach_persistent_process: Option<IAttachPersistentProcess>,

    /// Whether the terminal process environment should be exactly as provided in
    /// `TerminalOptions.env`. When this is false (default), the environment will be based on the
    /// window's environment and also apply configured platform settings like
    /// `terminal.integrated.env.windows` on top. When this is true, the complete environment must be
    /// provided as nothing will be inherited from the process or any configuration.
    pub strict_env: Option<bool>,

    /// Whether the terminal process environment will inherit VS Code's "shell environment" that may
    /// get sourced from running a login shell depnding on how the application was launched.
    /// Consumers that rely on development tools being present in the $PATH should set this to true.
    /// This will overwrite the value of the inheritEnv setting.
    pub use_shell_environment: Option<bool>,

    /// When enabled the terminal will run the process as normal but not be surfaced to the user
    /// until `Terminal.show` is called. The typical usage for this is when you need to run
    /// something that may need interactivity but only want to tell the user about it when
    /// interaction is needed. Note that the terminals will still be exposed to all extensions
    /// as normal. The hidden terminals will not be restored when the workspace is next opened.
    pub hide_from_user: Option<bool>,

    /// Whether to force the terminal to persist across sessions regardless of the other
    /// launch config, like `hideFromUser`.
    pub force_persist: Option<bool>,

    /// Whether this terminal is not a terminal that the user directly created and uses, but rather
    /// a terminal used to drive some VS Code feature.
    pub is_feature_terminal: Option<bool>,

    /// Whether this terminal was created by an extension.
    pub is_extension_owned_terminal: Option<bool>,

    /// Whether this terminal is used to bootstrap a remote authority resolver (`[remoteResolverTerminal]`).
    pub remote_resolver_terminal: bool,

    /// The icon for the terminal, used primarily in the terminal tab.
    pub icon: Option<TerminalIcon>,

    /// The color ID to use for this terminal. If not specified it will use the default fallback
    pub color: Option<String>,

    /// When a parent terminal is provided via API, the group needs
    /// to find the index in order to place the child
    /// directly to the right of its parent.
    pub parent_terminal_id: Option<u32>,

    /// The dimensions for the instance as set by the user
    /// or via Size to Content Width
    pub fixed_dimensions: Option<IFixedTerminalDimensions>,

    /// Opt-out of the default terminal persistence on restart and reload
    pub is_transient: Option<bool>,

    /// Attempt to force shell integration to be enabled by bypassing the `isFeatureTerminal`
    /// equals false requirement.
    pub force_shell_integration: Option<bool>,

    /// Create a terminal without shell integration even when it's enabled
    pub ignore_shell_integration: Option<bool>,

    /// Actions to include inline on hover of the terminal tab. E.g. the "Rerun task" action
    pub tab_actions: Option<Vec<ITerminalTabAction>>,

    /// Report terminal's shell environment variables to VS Code and extensions
    pub shell_integration_environment_reporting: Option<bool>,

    /// A custom nonce to use for shell integration when provided by an extension.
    /// This allows extensions to control shell integration for terminals they create.
    pub shell_integration_nonce: Option<String>,

    /// A title template string that supports the same variables as the
    /// `terminal.integrated.tabs.title` setting. When set, this overrides the config-based
    /// title template for this terminal instance.
    pub title_template: Option<String>,
}

/// O `shellIntegration` de `ITerminalProcessOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ITerminalProcessShellIntegrationOptions {
    pub enabled: bool,
    pub suggest_enabled: bool,
    pub nonce: String,
}

/// `ITerminalProcessOptions`.
#[derive(Clone, Debug, Default)]
pub struct ITerminalProcessOptions {
    pub shell_integration: ITerminalProcessShellIntegrationOptions,
    pub windows_use_conpty_dll: bool,
    pub environment_variable_collections: Option<ISerializableEnvironmentVariableCollections>,
    pub workspace_folder: Option<WorkspaceFolder>,
    pub is_screen_reader_optimized: bool,
}

string_enum! {
    /// `ShellIntegrationInjectionFailureReason`.
    ShellIntegrationInjectionFailureReason {
        /// The setting is disabled.
        InjectionSettingDisabled = "injectionSettingDisabled",
        /// There is no executable (so there's no way to determine how to inject).
        NoExecutable = "noExecutable",
        /// It's a feature terminal (tasks, debug), unless it's explicitly being forced.
        FeatureTerminal = "featureTerminal",
        /// The ignoreShellIntegration flag is passed (eg. relaunching without shell integration).
        IgnoreShellIntegrationFlag = "ignoreShellIntegrationFlag",
        /// Shell integration doesn't work on older Windows builds that don't support ConPTY.
        UnsupportedWindowsBuild = "unsupportedWindowsBuild",
        /// We're conservative whether we inject when we don't recognize the arguments used for the
        /// shell as we would prefer launching one without shell integration than breaking their profile.
        UnsupportedArgs = "unsupportedArgs",
        /// The shell doesn't have built-in shell integration. Note that this doesn't mean the shell
        /// won't have shell integration in the end.
        UnsupportedShell = "unsupportedShell",
        /// For zsh, we failed to set the sticky bit on the shell integration script folder.
        FailedToSetStickyBit = "failedToSetStickyBit",
        /// For zsh, we failed to create a temp directory for the shell integration script.
        FailedToCreateTmpDir = "failedToCreateTmpDir",
    }
}

/// `ITerminalProfile`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ITerminalProfile {
    pub profile_name: String,
    pub path: String,
    pub is_default: bool,
    /// Whether the terminal profile contains a potentially unsafe `path`. For example, the path
    /// `C:\Cygwin` is the default install for Cygwin on Windows, but it could be created by any
    /// user in a multi-user environment. As such, we don't want to blindly present it as a profile
    /// without a warning.
    pub is_unsafe_path: Option<bool>,
    /// An additional unsafe path that must exist, for example a script that appears in `args`.
    pub requires_unsafe_path: Option<String>,
    pub is_auto_detected: Option<bool>,
    /// Whether the profile path was found on the `$PATH` environment variable, if so it will be
    /// cleaner to display this profile in the UI using only `basename(path)`.
    pub is_from_path: Option<bool>,
    pub args: Option<OneOrMany<String>>,
    pub env: Option<ITerminalEnvironment>,
    pub override_name: Option<bool>,
    pub color: Option<String>,
    pub icon: Option<TerminalIcon>,
}

string_enum! {
    /// `ProfileSource`.
    ProfileSource {
        GitBash = "Git Bash",
        Pwsh = "PowerShell",
    }
}

/// `ITerminalUnsafePath`: um caminho que qualquer usuário de uma máquina compartilhada poderia ter criado.
/// O `isUnsafe: true` do original é o campo `is_unsafe`, que fica falso só num objeto vindo de configuração
/// que traga `isUnsafe` falso.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITerminalUnsafePath {
    pub path: String,
    pub is_unsafe: bool,
}

/// `string | ITerminalUnsafePath`: um elemento de `path` de `ITerminalExecutable` e o `requiresPath`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfilePath {
    Plain(String),
    Unsafe(ITerminalUnsafePath),
}

impl ProfilePath {
    /// `isString(e) ? e : e.path`.
    pub fn path(&self) -> &str {
        match self {
            Self::Plain(path) => path,
            Self::Unsafe(unsafe_path) => &unsafe_path.path,
        }
    }

    /// Um caminho inseguro (`{ path, isUnsafe: true }`).
    pub fn unsafe_path(path: impl Into<String>) -> Self {
        Self::Unsafe(ITerminalUnsafePath { path: path.into(), is_unsafe: true })
    }
}

/// `IBaseUnresolvedTerminalProfile`. O `icon?: string | ThemeIcon | URI | { light; dark }` é o
/// [`ContributedIcon`] (texto ou [`TerminalIcon`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IBaseUnresolvedTerminalProfile {
    pub args: Option<OneOrMany<String>>,
    pub is_auto_detected: Option<bool>,
    pub override_name: Option<bool>,
    pub icon: Option<ContributedIcon>,
    pub color: Option<String>,
    pub env: Option<ITerminalEnvironment>,
    pub requires_path: Option<ProfilePath>,
}

/// `ITerminalExecutable`. O `path: SingleOrMany<string | ITerminalUnsafePath>` fica sempre como lista (o original
/// o normaliza com `Array.isArray(path) ? path : [path]` antes de usar).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ITerminalExecutable {
    pub base: IBaseUnresolvedTerminalProfile,
    pub path: Vec<ProfilePath>,
}

/// `ITerminalProfileSource`. O `source` é o texto de `ProfileSource` que a configuração trouxe (um valor
/// desconhecido não casa com nenhuma fonte e o perfil é descartado, como no original).
#[derive(Clone, Debug, PartialEq)]
pub struct ITerminalProfileSource {
    pub base: IBaseUnresolvedTerminalProfile,
    pub source: String,
}

/// `ITerminalExecutable | ITerminalProfileSource` (o `null` do `IUnresolvedTerminalProfile` é o perfil ausente).
#[derive(Clone, Debug, PartialEq)]
pub enum IUnresolvedTerminalProfile {
    Executable(ITerminalExecutable),
    Source(ITerminalProfileSource),
}

impl IUnresolvedTerminalProfile {
    pub fn base(&self) -> &IBaseUnresolvedTerminalProfile {
        match self {
            Self::Executable(executable) => &executable.base,
            Self::Source(source) => &source.base,
        }
    }

    pub fn base_mut(&mut self) -> &mut IBaseUnresolvedTerminalProfile {
        match self {
            Self::Executable(executable) => &mut executable.base,
            Self::Source(source) => &mut source.base,
        }
    }
}

/// O `icon?: URI | { light: URI; dark: URI } | string` de `ITerminalProfileContribution`.
#[derive(Clone, Debug, PartialEq)]
pub enum ContributedIcon {
    Text(String),
    Icon(TerminalIcon),
}

/// `ITerminalProfileContribution`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ITerminalProfileContribution {
    pub title: String,
    pub id: String,
    pub icon: Option<ContributedIcon>,
    pub color: Option<String>,
    pub title_template: Option<String>,
}

/// `IExtensionTerminalProfile`: um `ITerminalProfileContribution` com o identificador da extensão.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IExtensionTerminalProfile {
    pub contribution: ITerminalProfileContribution,
    pub extension_identifier: String,
}

impl Deref for IExtensionTerminalProfile {
    type Target = ITerminalProfileContribution;

    fn deref(&self) -> &ITerminalProfileContribution {
        &self.contribution
    }
}

/// `TerminalLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalLocation {
    Panel = 1,
    Editor = 2,
}

/// `TerminalLocation | { viewColumn: number; preserveState?: boolean } | { splitActiveTerminal: boolean }`: o `location` de
/// `ICreateContributedTerminalProfileOptions`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TerminalProfileLocation {
    Location(TerminalLocation),
    ViewColumn { view_column: f64, preserve_state: Option<bool> },
    SplitActiveTerminal(bool),
}

/// `ICreateContributedTerminalProfileOptions`.
#[derive(Clone, Debug, Default)]
pub struct ICreateContributedTerminalProfileOptions {
    pub icon: Option<ContributedIcon>,
    pub color: Option<String>,
    pub location: Option<TerminalProfileLocation>,
    pub cwd: Option<ShellCwd>,
    pub title_template: Option<String>,
}

string_enum! {
    /// `TerminalLocationConfigValue`.
    TerminalLocationConfigValue {
        TerminalView = "view",
        Editor = "editor",
    }
}

/// `ITerminalLaunchError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITerminalLaunchError {
    pub message: String,
    pub code: Option<i32>,
}

/// `ShellIntegrationStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellIntegrationStatus {
    /// No shell integration sequences have been encountered.
    Off,
    /// Final term shell integration sequences have been encountered.
    FinalTerm,
    /// VS Code shell integration sequences have been encountered. Supercedes FinalTerm.
    VSCode,
}

/// `IShellIntegration`: o que o addon de integração com o shell oferece a quem o consome. `seenSequences` é um
/// retrato do conjunto (o `ReadonlySet<string>` do original, na ordem de inserção).
pub trait IShellIntegration {
    fn capabilities(&self) -> Rc<dyn crate::terminal::capabilities::capabilities::ITerminalCapabilityStore>;
    fn seen_sequences(&self) -> Vec<String>;
    fn status(&self) -> ShellIntegrationStatus;

    fn on_did_change_status(&self) -> je_base::event::Event<ShellIntegrationStatus>;
    fn on_did_change_seen_sequences(&self) -> je_base::event::Event<Vec<String>>;

    fn deserialize(&self, serialized: &crate::terminal::capabilities::capabilities::ISerializedCommandDetectionCapability);

    fn set_next_command_id(&self, command: &str, command_id: &str);
}

/// `RegExpMatchArray`: o texto casado e o casamento (o resultado de `String#match` sem a flag `g`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegExpMatchArray {
    pub input: JsString,
    pub matched: zyarr::Match,
}

/// `ITerminalOutputMatch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITerminalOutputMatch {
    pub regex_match: RegExpMatchArray,
    pub output_lines: Vec<String>,
}

/// `lineMatcher: string | RegExp`.
#[derive(Clone)]
pub enum LineMatcher {
    Text(String),
    RegExp(Rc<je_base::js_regexp::JsRegExp>),
}

/// `anchor: 'top' | 'bottom'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputMatcherAnchor {
    Top,
    Bottom,
}

/// `ITerminalOutputMatcher`: um casador que roda numa subseção da saída de um comando do terminal.
#[derive(Clone)]
pub struct ITerminalOutputMatcher {
    /// A string or regex to match against the unwrapped line. If this is a regex with the multiline
    /// flag, it will scan an amount of lines equal to `\n` instances in the regex + 1.
    pub line_matcher: LineMatcher,
    /// Which side of the output to anchor the `offset` and `length` against.
    pub anchor: OutputMatcherAnchor,
    /// The number of rows above or below the `anchor` to start matching against.
    pub offset: f64,
    /// The number of rows to match against, this should be as small as possible for performance
    /// reasons. This is capped at 40.
    pub length: f64,
    /// If multiple matches are expected - this will result in `outputLines` being returned when there's a
    /// `regexMatch` from `offset` to `length`.
    pub multiple_matches: Option<bool>,
}

/// `(terminalId: number, cols: number, rows: number) => ITerminalChildProcess`.
pub type CustomPtyImplementation = Rc<dyn Fn(u32, i32, i32) -> Rc<dyn ITerminalChildProcess>>;

string_enum! {
    /// `ProcessPropertyType`.
    ProcessPropertyType {
        Cwd = "cwd",
        InitialCwd = "initialCwd",
        FixedDimensions = "fixedDimensions",
        Title = "title",
        ShellType = "shellType",
        HasChildProcesses = "hasChildProcesses",
        ResolvedShellLaunchConfig = "resolvedShellLaunchConfig",
        OverrideDimensions = "overrideDimensions",
        FailedShellIntegrationActivation = "failedShellIntegrationActivation",
        UsedShellIntegrationInjection = "usedShellIntegrationInjection",
        ShellIntegrationInjectionFailureReason = "shellIntegrationInjectionFailureReason",
    }
}

/// `IProcessProperty` com o valor de `IProcessPropertyMap[type]`: uma variante por [`ProcessPropertyType`].
#[derive(Clone)]
pub enum IProcessProperty {
    Cwd(String),
    InitialCwd(String),
    FixedDimensions(IFixedTerminalDimensions),
    Title(String),
    ShellType(TerminalShellType),
    HasChildProcesses(bool),
    ResolvedShellLaunchConfig(IShellLaunchConfig),
    OverrideDimensions(Option<ITerminalDimensionsOverride>),
    FailedShellIntegrationActivation(Option<bool>),
    UsedShellIntegrationInjection(Option<bool>),
    ShellIntegrationInjectionFailureReason(Option<ShellIntegrationInjectionFailureReason>),
}

impl IProcessProperty {
    /// O `type` do original.
    pub fn property_type(&self) -> ProcessPropertyType {
        match self {
            Self::Cwd(_) => ProcessPropertyType::Cwd,
            Self::InitialCwd(_) => ProcessPropertyType::InitialCwd,
            Self::FixedDimensions(_) => ProcessPropertyType::FixedDimensions,
            Self::Title(_) => ProcessPropertyType::Title,
            Self::ShellType(_) => ProcessPropertyType::ShellType,
            Self::HasChildProcesses(_) => ProcessPropertyType::HasChildProcesses,
            Self::ResolvedShellLaunchConfig(_) => ProcessPropertyType::ResolvedShellLaunchConfig,
            Self::OverrideDimensions(_) => ProcessPropertyType::OverrideDimensions,
            Self::FailedShellIntegrationActivation(_) => ProcessPropertyType::FailedShellIntegrationActivation,
            Self::UsedShellIntegrationInjection(_) => ProcessPropertyType::UsedShellIntegrationInjection,
            Self::ShellIntegrationInjectionFailureReason(_) => ProcessPropertyType::ShellIntegrationInjectionFailureReason,
        }
    }
}

/// `ITerminalDimensions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ITerminalDimensions {
    /// The columns of the terminal.
    pub cols: i32,
    /// The rows of the terminal.
    pub rows: i32,
}

/// `ITerminalDimensionsOverride`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ITerminalDimensionsOverride {
    pub cols: i32,
    pub rows: i32,
    /// Indicate that xterm must receive these exact dimensions, even if they overflow the ui!
    pub force_exact_size: Option<bool>,
}

/// `ITerminalLaunchResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITerminalLaunchResult {
    pub injected_args: Vec<String>,
}

/// `ITerminalLaunchError | ITerminalLaunchResult` (o `undefined` do retorno de `start` é o `None` do `Option`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ITerminalLaunchOutcome {
    Error(ITerminalLaunchError),
    Result(ITerminalLaunchResult),
}

/// O `backend` de `IProcessReadyWindowsPty` (o original só tem `'conpty'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsPtyBackend {
    Conpty,
}

/// `IProcessReadyWindowsPty`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IProcessReadyWindowsPty {
    /// What pty emulation backend is being used.
    pub backend: WindowsPtyBackend,
    /// The Windows build version (eg. 19045).
    pub build_number: i32,
}

/// `IProcessReadyEvent`. O `pid` é `i32` porque o processo de um terminal de extensão anuncia `-1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IProcessReadyEvent {
    pub pid: i32,
    pub cwd: String,
    pub windows_pty: Option<IProcessReadyWindowsPty>,
}

/// O `'6' | '11'` de `setUnicodeVersion` e de `createProcess`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnicodeVersion {
    V6,
    V11,
}

impl UnicodeVersion {
    /// O texto do original (`'6'` ou `'11'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V6 => "6",
            Self::V11 => "11",
        }
    }

    /// `'11'` é o `V11`, qualquer outro valor é o `V6` (o padrão do `terminal.integrated.unicodeVersion`).
    pub fn from_config_text(text: &str) -> Self {
        if text == "11" { Self::V11 } else { Self::V6 }
    }
}

/// `FlowControlConstants`.
pub struct FlowControlConstants;

impl FlowControlConstants {
    /// The number of _unacknowledged_ chars to have been sent before the pty is paused in order for the client to catch up.
    pub const HIGH_WATERMARK_CHARS: usize = 100000;
    /// After flow control pauses the pty for the client the catch up, this is the number of _unacknowledged_ chars to have
    /// been caught up to on the client before resuming the pty again.
    pub const LOW_WATERMARK_CHARS: usize = 5000;
    /// The number characters that are accumulated on the client side before sending an ack event. This must be less than or
    /// equal to `LOW_WATERMARK_CHARS` or the terminal max never unpause.
    pub const CHAR_COUNT_ACK_SIZE: usize = 5000;
}

/// `IProcessDataEvent`: compartilhado por `Rc`, porque o consumidor muda o `data` e o produtor lê o `writePromise` depois do
/// disparo.
pub struct IProcessDataEvent {
    pub data: RefCell<String>,
    pub track_commit: bool,
    /// When `trackCommit` is set, this will be set to a promise that resolves when the data is parsed.
    pub write_promise: RefCell<Option<Promise<()>>>,
}

impl IProcessDataEvent {
    pub fn new(data: impl Into<String>, track_commit: bool) -> Rc<Self> {
        Rc::new(Self { data: RefCell::new(data.into()), track_commit, write_promise: RefCell::new(None) })
    }
}

/// `string | IProcessDataEvent`.
#[derive(Clone)]
pub enum ProcessData {
    Text(String),
    Event(Rc<IProcessDataEvent>),
}

impl ProcessData {
    /// `isString(e) ? e : e.data`.
    pub fn data(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Event(event) => event.data.borrow().clone(),
        }
    }
}

/// O resultado de `freePortKillProcess`: `{ port, processId }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IFreedPortProcess {
    pub port: String,
    pub process_id: String,
}

/// `TerminalExitReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalExitReason {
    Unknown = 0,
    Shutdown = 1,
    Process = 2,
    User = 3,
    Extension = 4,
}

/// An interface representing a raw terminal child process, this contains a subset of the `child_process.ChildProcess`
/// node.js interface.
pub trait ITerminalChildProcess {
    /// A unique identifier for the terminal process. Note that the uniqueness only applies to a given pty service
    /// connection, IDs will be duplicated for remote and local terminals for example. The ID will be 0 if it does not
    /// support reconnection.
    fn id(&self) -> u32;

    /// Whether the process should be persisted across reloads.
    fn should_persist(&self) -> bool;

    fn on_process_data(&self) -> Event<ProcessData>;
    fn on_process_ready(&self) -> Event<IProcessReadyEvent>;
    fn on_process_replay_complete(&self) -> Option<Event<()>> {
        None
    }
    fn on_did_change_property(&self) -> Event<IProcessProperty>;
    /// `onProcessExit: Event<number | undefined>`.
    fn on_process_exit(&self) -> Event<Option<i32>>;
    fn on_restore_commands(&self) -> Option<Event<ISerializedCommandDetectionCapability>> {
        None
    }

    /// Starts the process. Resolve `None` when the process was successfully started, otherwise an object containing
    /// information on what went wrong.
    fn start(&self) -> Promise<Option<ITerminalLaunchOutcome>>;

    /// Detach the process from the UI and await reconnect (`detach?`). `None` quando o processo não suporta o desanexar.
    fn detach(&self, _force_persist: Option<bool>) -> Option<Promise<()>> {
        None
    }

    /// Frees the port and kills the process (`freePortKillProcess?`).
    fn free_port_kill_process(&self, _port: &str) -> Option<Promise<IFreedPortProcess>> {
        None
    }

    /// Shutdown the terminal process. When `immediate` is true the process will be killed immediately, otherwise the
    /// process will be given some time to make sure no additional data comes through.
    fn shutdown(&self, immediate: bool);
    fn input(&self, data: &str);
    fn send_signal(&self, signal: &str);
    fn process_binary(&self, data: &str) -> Promise<()>;

    /// `resize`: o `Err` é a exceção do original (por exemplo `EPIPE`, o `code` do erro fica em `props`).
    fn resize(&self, cols: i32, rows: i32, pixel_width: Option<i32>, pixel_height: Option<i32>) -> Result<(), je_base::errors::Error>;
    fn clear_buffer(&self) -> Promise<()>;

    /// Acknowledge a data event has been parsed by the terminal, this is used to implement flow control to ensure remote
    /// processes to not get too far ahead of the client and flood the connection.
    fn acknowledge_data_event(&self, char_count: usize);

    /// Sets the unicode version for the process, this drives the size of some characters in the xterm-headless instance.
    fn set_unicode_version(&self, version: UnicodeVersion) -> Promise<()>;

    fn get_initial_cwd(&self) -> Promise<String>;
    fn get_cwd(&self) -> Promise<String>;
    fn refresh_property(&self, property: ProcessPropertyType) -> Promise<IProcessProperty>;
    fn update_property(&self, property: IProcessProperty) -> Promise<()>;
}

/// `ITerminalLogService`: um `ILogService` com marca própria (`_logBrand`), para o compilador distinguir os dois.
pub trait ITerminalLogService: LogService {}

service_id!(pub I_TERMINAL_LOG_SERVICE: dyn ITerminalLogService = "terminalLogService");

/// O `NullLogService` também serve de `ITerminalLogService`: descarta tudo.
impl ITerminalLogService for crate::log::NullLogService {}

