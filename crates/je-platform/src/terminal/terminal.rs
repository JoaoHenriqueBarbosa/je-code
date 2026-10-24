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
//! Divergências (o que ainda falta do `terminal.ts`, a portar quando um consumidor pedir): `TerminalSettingPrefix`
//! e `TerminalSettingId`; os tipos de layout (`IRawTerminalInstanceLayoutInfo`, `IPtyHostAttachTarget`...);
//! `TerminalIpcChannels`; `ProcessPropertyType`, `IProcessProperty` e `IProcessPropertyMap`; `IPtyService`,
//! `IPtyServiceContribution`, `IPtyHostController`, `IPtyHostService`, `ILocalPtyService`; as constantes
//! (`HeartbeatConstants`, `LocalReconnectConstants`, `FlowControlConstants`, `ShellIntegrationTimeoutOverride`)
//! e `IHeartbeatService`; os estados de serialização (`ISerializedTerminalState`...); `ITerminalChildProcess` e
//! os eventos de processo, e por isso o campo `customPtyImplementation` de `IShellLaunchConfig` não existe
//! aqui; `IShellLaunchConfigDto`; `ICreateContributedTerminalProfileOptions`; `ProfileSource`,
//! `IBaseUnresolvedTerminalProfile`, `ITerminalExecutable`, `ITerminalProfileSource`, `ITerminalProfileObject`;
//! `IShellIntegration` e `ShellIntegrationStatus`; `TerminalExitReason`; os seletores e casadores de saída; `ITerminalBackend`, o registro de backends
//! (`TerminalExtensions`, `ITerminalBackendRegistry`) e `ITerminalLogService`.
//!
//! Outra divergência: `ThemeIcon.color` é comparado por valor (o id da cor), enquanto o `terminalIconsEqual`
//! do original compara a referência do objeto `ThemeColor` com `===`.

use std::any::Any;
use std::ops::Deref;
use std::rc::Rc;

use je_base::JsString;
use je_base::arrays::OneOrMany;
use je_base::dynamic::Dynamic;
use je_base::themables::ThemeIcon;
use je_base::uri::{URI, UriComponents, is_uri_components};

use super::environment_variable::ISerializableEnvironmentVariableCollections;
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

/// `IShellLaunchConfig` (sem o `customPtyImplementation`, ver "Divergências").
#[derive(Clone, Default)]
pub struct IShellLaunchConfig {
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
