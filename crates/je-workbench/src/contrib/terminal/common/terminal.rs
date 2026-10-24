//! Porte parcial de `src/vs/workbench/contrib/terminal/common/terminal.ts`: as constantes de fonte, `FontWeight`,
//! `ITerminalFont`, os ids de comando (`TerminalCommandId`), `DEFAULT_COMMANDS_TO_SKIP_SHELL`, `ProcessState`, o estado do
//! processo (`ITerminalProcessInfo`, `ITerminalProcessManager`, `IBeforeProcessDataEvent`), o `ITerminalProfileResolverService`
//! (com o `IShellLaunchConfigResolveOptions`) e o `ITerminalStatus`.
//!
//! O resto do arquivo (`ITerminalConfiguration` tipada, `ITerminalEditorService`...) entra quando
//! um consumidor pedir. O `ITerminalProfileService` tem só `onDidChangeAvailableProfiles` e `registerTerminalProfileProvider`, que o
//! `MainThreadTerminalService` lê, e o `ITerminalProfileProvider` é o que ele registra; o `ITerminalProcessExtHostProxy` e o
//! `IStartExtensionTerminalRequest` são os do terminal de extensão (`onStart` do proxy, que nada dispara, fica de fora).
//!
//! Divergências:
//! - **`const enum` de string.** `TerminalCommandId` é um enum com `as_str()` devolvendo o valor do original, como os de
//!   `je_platform::terminal::terminal`.
//! - **`DEFAULT_COMMANDS_TO_SKIP_SHELL`** é um `Vec<&'static str>` montado por [`default_commands_to_skip_shell`], porque
//!   o `DEFAULT_LINE_HEIGHT` e a lista dependem da plataforma detectada em tempo de execução, não de uma constante. Os
//!   itens vindos de `defaultTerminalContribCommandsToSkipShell` (acessibilidade, busca, histórico e sugestão) estão
//!   embutidos na ordem do original, e os do histórico vêm do enum já portado.
//! - **`ITerminalFont`** tem `charWidth` e `charHeight` opcionais, que são `Option<f64>`.
//! - **`isLinux`** é a plataforma detectada ([`Platform::detect`]).
//! - **`IShellLaunchConfig` compartilhado.** O original passa o mesmo objeto da instância ao gerenciador de processo e ao
//!   resolvedor de perfil, que o alteram no lugar (`executable`, `args`, `env`, `attachPersistentProcess`...). Aqui o objeto é
//!   um [`SharedShellLaunchConfig`] (`Rc<RefCell<...>>`), e quem o lê ou escreve empresta só por um instante, nunca por um
//!   `await`.
//! - **`ITerminalProcessManager.setDimensions`** tem três sobrecargas no original; aqui são [`ITerminalProcessManager::set_dimensions`]
//!   (a assíncrona, que espera o `ptyProcessReady`) e [`ITerminalProcessManager::set_dimensions_sync`] (a de `sync: true`).
//! - **`ITerminalProcessManager.dispose(immediate?)`** é [`ITerminalProcessManager::dispose_immediate`], e o `dispose()` do
//!   `IDisposable` é o `dispose_immediate(false)`.
//! - **`ptyProcessReady`.** Depois de descartado o gerenciador o campo vira `undefined` no original (que o `await` aceita);
//!   aqui devolve uma promessa já resolvida.
//! - **`ITerminalStatus.hoverActions[].run`** é um `Rc<dyn Fn()>`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::themables::ThemeIcon;
use je_instantiation::service_id;
use je_platform::terminal::capabilities::capabilities::{ISerializedCommandDetectionCapability, ITerminalCapabilityStore};
use je_platform::terminal::environment_variable::IMergedEnvironmentVariableCollection;
use je_platform::terminal::terminal::{
    ICreateContributedTerminalProfileOptions, IProcessDataEvent, IProcessProperty, IProcessReadyEvent, IProcessReadyWindowsPty, IShellLaunchConfig,
    ITerminalDimensions, ITerminalLaunchError, ITerminalLaunchOutcome, ITerminalProfile, ProcessPropertyType, UnicodeVersion,
};
use je_base::arrays::OneOrMany;

use super::environment_variable::IEnvironmentVariableInfo;
use super::terminal_backend::ITerminalBackend;
use crate::contrib::terminal_contrib::history::terminal_history::DEFAULT_TERMINAL_HISTORY_COMMANDS_TO_SKIP_SHELL;

/// `TERMINAL_CONFIG_SECTION`.
pub const TERMINAL_CONFIG_SECTION: &str = "terminal.integrated";

/// `DEFAULT_LETTER_SPACING`.
pub const DEFAULT_LETTER_SPACING: f64 = 0.0;
/// `MINIMUM_LETTER_SPACING`.
pub const MINIMUM_LETTER_SPACING: f64 = -5.0;

/// `DEFAULT_LINE_HEIGHT`: no Linux é comum as fontes trazerem um sublinhado abaixo da célula, que o renderizador cortaria
/// (https://github.com/microsoft/vscode/issues/211933), então a altura padrão é 1.1.
pub fn default_line_height() -> f64 {
    if Platform::detect().is_linux { 1.1 } else { 1.0 }
}

/// `MINIMUM_FONT_WEIGHT`.
pub const MINIMUM_FONT_WEIGHT: f64 = 1.0;
/// `MAXIMUM_FONT_WEIGHT`.
pub const MAXIMUM_FONT_WEIGHT: f64 = 1000.0;

/// `FontWeight` (`'normal' | 'bold' | number`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FontWeight {
    Normal,
    Bold,
    Number(f64),
}

/// `DEFAULT_FONT_WEIGHT`.
pub const DEFAULT_FONT_WEIGHT: FontWeight = FontWeight::Normal;
/// `DEFAULT_BOLD_FONT_WEIGHT`.
pub const DEFAULT_BOLD_FONT_WEIGHT: FontWeight = FontWeight::Bold;

/// `ITerminalFont`.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalFont {
    pub font_family: String,
    pub font_size: f64,
    pub letter_spacing: f64,
    pub line_height: f64,
    pub char_width: Option<f64>,
    pub char_height: Option<f64>,
}

/// `ProcessState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    /// The process has not been initialized yet.
    Uninitialized = 1,
    /// The process is currently launching, the process is marked as launching for a short duration after being created and
    /// is helpful to indicate whether the process died as a result of bad shell and args.
    Launching = 2,
    /// The process is running normally.
    Running = 3,
    /// The process was killed during launch, likely as a result of bad shell and args.
    KilledDuringLaunch = 4,
    /// The process was killed by the user (the event originated from VS Code).
    KilledByUser = 5,
    /// The process was killed by itself, for example the shell crashed or `exit` was run.
    KilledByProcess = 6,
}

/// Declara o `const enum` de string `TerminalCommandId`: as variantes e o `as_str()` com o valor do original.
macro_rules! terminal_command_ids {
    ($($variant:ident = $value:literal),+ $(,)?) => {
        /// `TerminalCommandId`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum TerminalCommandId {
            $($variant),+
        }

        impl TerminalCommandId {
            /// O valor do `const enum` do original.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }
    };
}

terminal_command_ids! {
    Toggle = "workbench.action.terminal.toggleTerminal",
    Kill = "workbench.action.terminal.kill",
    KillViewOrEditor = "workbench.action.terminal.killViewOrEditor",
    KillEditor = "workbench.action.terminal.killEditor",
    KillActiveTab = "workbench.action.terminal.killActiveTab",
    KillAll = "workbench.action.terminal.killAll",
    QuickKill = "workbench.action.terminal.quickKill",
    ConfigureTerminalSettings = "workbench.action.terminal.openSettings",
    ShellIntegrationLearnMore = "workbench.action.terminal.learnMore",
    CopyLastCommand = "workbench.action.terminal.copyLastCommand",
    CopyLastCommandOutput = "workbench.action.terminal.copyLastCommandOutput",
    CopyLastCommandAndLastCommandOutput = "workbench.action.terminal.copyLastCommandAndLastCommandOutput",
    CopyAndClearSelection = "workbench.action.terminal.copyAndClearSelection",
    CopySelection = "workbench.action.terminal.copySelection",
    CopySelectionAsHtml = "workbench.action.terminal.copySelectionAsHtml",
    SelectAll = "workbench.action.terminal.selectAll",
    DeleteWordLeft = "workbench.action.terminal.deleteWordLeft",
    DeleteWordRight = "workbench.action.terminal.deleteWordRight",
    DeleteToLineStart = "workbench.action.terminal.deleteToLineStart",
    MoveToLineStart = "workbench.action.terminal.moveToLineStart",
    MoveToLineEnd = "workbench.action.terminal.moveToLineEnd",
    New = "workbench.action.terminal.new",
    NewWithCwd = "workbench.action.terminal.newWithCwd",
    NewLocal = "workbench.action.terminal.newLocal",
    NewInActiveWorkspace = "workbench.action.terminal.newInActiveWorkspace",
    NewWithProfile = "workbench.action.terminal.newWithProfile",
    Split = "workbench.action.terminal.split",
    SplitActiveTab = "workbench.action.terminal.splitActiveTab",
    SplitInActiveWorkspace = "workbench.action.terminal.splitInActiveWorkspace",
    Unsplit = "workbench.action.terminal.unsplit",
    JoinActiveTab = "workbench.action.terminal.joinActiveTab",
    Join = "workbench.action.terminal.join",
    Relaunch = "workbench.action.terminal.relaunch",
    FocusPreviousPane = "workbench.action.terminal.focusPreviousPane",
    CreateTerminalEditor = "workbench.action.createTerminalEditor",
    CreateTerminalEditorSameGroup = "workbench.action.createTerminalEditorSameGroup",
    CreateTerminalEditorSide = "workbench.action.createTerminalEditorSide",
    FocusTabs = "workbench.action.terminal.focusTabs",
    FocusNextPane = "workbench.action.terminal.focusNextPane",
    ResizePaneLeft = "workbench.action.terminal.resizePaneLeft",
    ResizePaneRight = "workbench.action.terminal.resizePaneRight",
    ResizePaneUp = "workbench.action.terminal.resizePaneUp",
    SizeToContentWidth = "workbench.action.terminal.sizeToContentWidth",
    SizeToContentWidthActiveTab = "workbench.action.terminal.sizeToContentWidthActiveTab",
    ResizePaneDown = "workbench.action.terminal.resizePaneDown",
    Focus = "workbench.action.terminal.focus",
    FocusInstance = "workbench.action.terminal.focusInstance",
    FocusNext = "workbench.action.terminal.focusNext",
    FocusPrevious = "workbench.action.terminal.focusPrevious",
    Paste = "workbench.action.terminal.paste",
    PastePwsh = "workbench.action.terminal.pastePwsh",
    PasteSelection = "workbench.action.terminal.pasteSelection",
    SelectDefaultProfile = "workbench.action.terminal.selectDefaultShell",
    RunSelectedText = "workbench.action.terminal.runSelectedText",
    RunActiveFile = "workbench.action.terminal.runActiveFile",
    SwitchTerminal = "workbench.action.terminal.switchTerminal",
    ScrollDownLine = "workbench.action.terminal.scrollDown",
    ScrollDownPage = "workbench.action.terminal.scrollDownPage",
    ScrollToBottom = "workbench.action.terminal.scrollToBottom",
    ScrollUpLine = "workbench.action.terminal.scrollUp",
    ScrollUpPage = "workbench.action.terminal.scrollUpPage",
    ScrollToTop = "workbench.action.terminal.scrollToTop",
    Clear = "workbench.action.terminal.clear",
    ClearSelection = "workbench.action.terminal.clearSelection",
    ChangeIcon = "workbench.action.terminal.changeIcon",
    ChangeIconActiveTab = "workbench.action.terminal.changeIconActiveTab",
    ChangeColor = "workbench.action.terminal.changeColor",
    ChangeColorActiveTab = "workbench.action.terminal.changeColorActiveTab",
    Rename = "workbench.action.terminal.rename",
    RenameActiveTab = "workbench.action.terminal.renameActiveTab",
    RenameWithArgs = "workbench.action.terminal.renameWithArg",
    ScrollToPreviousCommand = "workbench.action.terminal.scrollToPreviousCommand",
    ScrollToNextCommand = "workbench.action.terminal.scrollToNextCommand",
    SelectToPreviousCommand = "workbench.action.terminal.selectToPreviousCommand",
    SelectToNextCommand = "workbench.action.terminal.selectToNextCommand",
    SelectToPreviousLine = "workbench.action.terminal.selectToPreviousLine",
    SelectToNextLine = "workbench.action.terminal.selectToNextLine",
    SendSequence = "workbench.action.terminal.sendSequence",
    SendSignal = "workbench.action.terminal.sendSignal",
    AttachToSession = "workbench.action.terminal.attachToSession",
    DetachSession = "workbench.action.terminal.detachSession",
    MoveToEditor = "workbench.action.terminal.moveToEditor",
    MoveToTerminalPanel = "workbench.action.terminal.moveToTerminalPanel",
    MoveIntoNewWindow = "workbench.action.terminal.moveIntoNewWindow",
    NewInNewWindow = "workbench.action.terminal.newInNewWindow",
    SetDimensions = "workbench.action.terminal.setDimensions",
    FocusHover = "workbench.action.terminal.focusHover",
    ShowEnvironmentContributions = "workbench.action.terminal.showEnvironmentContributions",
    StartVoice = "workbench.action.terminal.startVoice",
    StopVoice = "workbench.action.terminal.stopVoice",
    RevealCommand = "workbench.action.terminal.revealCommand",
}

/// `AccessibilityCommandId.OpenAccessibilityHelp` (`workbench/contrib/accessibility/common/accessibilityCommands.ts`).
const OPEN_ACCESSIBILITY_HELP: &str = "editor.action.accessibilityHelp";

/// `defaultTerminalAccessibilityCommandsToSkipShell` (`TerminalAccessibilityCommandId.FocusAccessibleBuffer`).
const DEFAULT_TERMINAL_ACCESSIBILITY_COMMANDS_TO_SKIP_SHELL: [&str; 1] = ["workbench.action.terminal.focusAccessibleBuffer"];

/// `defaultTerminalFindCommandToSkipShell`.
const DEFAULT_TERMINAL_FIND_COMMAND_TO_SKIP_SHELL: [&str; 8] = [
    "workbench.action.terminal.focusFind",
    "workbench.action.terminal.hideFind",
    "workbench.action.terminal.findNext",
    "workbench.action.terminal.findPrevious",
    "workbench.action.terminal.toggleFindRegex",
    "workbench.action.terminal.toggleFindWholeWord",
    "workbench.action.terminal.toggleFindCaseSensitive",
    "workbench.action.terminal.searchWorkspace",
];

/// `defaultTerminalSuggestCommandsToSkipShell`.
const DEFAULT_TERMINAL_SUGGEST_COMMANDS_TO_SKIP_SHELL: [&str; 10] = [
    "workbench.action.terminal.selectPrevSuggestion",
    "workbench.action.terminal.selectPrevPageSuggestion",
    "workbench.action.terminal.selectNextSuggestion",
    "workbench.action.terminal.selectNextPageSuggestion",
    "workbench.action.terminal.acceptSelectedSuggestion",
    "workbench.action.terminal.acceptSelectedSuggestionEnter",
    "workbench.action.terminal.hideSuggestWidget",
    "workbench.action.terminal.triggerSuggest",
    "workbench.action.terminal.suggestToggleDetails",
    "workbench.action.terminal.suggestToggleDetailsFocus",
];

/// Os ids de comando literais de `DEFAULT_COMMANDS_TO_SKIP_SHELL`, os que vêm depois de `TerminalCommandId.SendSignal`
/// e antes do `...defaultTerminalContribCommandsToSkipShell` (ver [`default_commands_to_skip_shell`]).
const LITERAL_COMMANDS_TO_SKIP_SHELL: &[&str] = &[
    "workbench.action.tasks.rerunForActiveTerminal",
    "editor.action.toggleTabFocusMode",
    "notifications.hideList",
    "notifications.hideToasts",
    "workbench.action.closeQuickOpen",
    "workbench.action.quickOpen",
    "workbench.action.quickOpenPreviousEditor",
    "workbench.action.showCommands",
    "workbench.action.tasks.build",
    "workbench.action.tasks.restartTask",
    "workbench.action.tasks.runTask",
    "workbench.action.tasks.reRunTask",
    "workbench.action.tasks.showLog",
    "workbench.action.tasks.showTasks",
    "workbench.action.tasks.terminate",
    "workbench.action.tasks.test",
    "workbench.action.toggleFullScreen",
    "workbench.action.terminal.focusAtIndex1",
    "workbench.action.terminal.focusAtIndex2",
    "workbench.action.terminal.focusAtIndex3",
    "workbench.action.terminal.focusAtIndex4",
    "workbench.action.terminal.focusAtIndex5",
    "workbench.action.terminal.focusAtIndex6",
    "workbench.action.terminal.focusAtIndex7",
    "workbench.action.terminal.focusAtIndex8",
    "workbench.action.terminal.focusAtIndex9",
    "workbench.action.focusSecondEditorGroup",
    "workbench.action.focusThirdEditorGroup",
    "workbench.action.focusFourthEditorGroup",
    "workbench.action.focusFifthEditorGroup",
    "workbench.action.focusSixthEditorGroup",
    "workbench.action.focusSeventhEditorGroup",
    "workbench.action.focusEighthEditorGroup",
    "workbench.action.focusNextPart",
    "workbench.action.focusPreviousPart",
    "workbench.action.nextPanelView",
    "workbench.action.previousPanelView",
    "workbench.action.nextSideBarView",
    "workbench.action.previousSideBarView",
    "workbench.action.debug.disconnect",
    "workbench.action.debug.start",
    "workbench.action.debug.stop",
    "workbench.action.debug.run",
    "workbench.action.debug.restart",
    "workbench.action.debug.continue",
    "workbench.action.debug.pause",
    "workbench.action.debug.stepInto",
    "workbench.action.debug.stepOut",
    "workbench.action.debug.stepOver",
    "sessions.goBack",
    "sessions.goForward",
    "sessions.focusActiveSession",
    "sessions.focusSessionInGrid1",
    "sessions.focusSessionInGrid2",
    "sessions.focusSessionInGrid3",
    "sessions.focusSessionInGrid4",
    "sessions.focusSessionInGrid5",
    "sessions.focusSessionInGrid6",
    "sessions.focusSessionInGrid7",
    "sessions.focusSessionInGrid8",
    "sessions.focusSessionInGrid9",
    "sessionsViewPane.navigatePreviousSession",
    "sessionsViewPane.navigateNextSession",
    "workbench.action.nextEditor",
    "workbench.action.previousEditor",
    "workbench.action.nextEditorInGroup",
    "workbench.action.previousEditorInGroup",
    "workbench.action.openNextRecentlyUsedEditor",
    "workbench.action.openPreviousRecentlyUsedEditor",
    "workbench.action.openNextRecentlyUsedEditorInGroup",
    "workbench.action.openPreviousRecentlyUsedEditorInGroup",
    "workbench.action.quickOpenPreviousRecentlyUsedEditor",
    "workbench.action.quickOpenLeastRecentlyUsedEditor",
    "workbench.action.quickOpenPreviousRecentlyUsedEditorInGroup",
    "workbench.action.quickOpenLeastRecentlyUsedEditorInGroup",
    "workbench.action.focusActiveEditorGroup",
    "workbench.action.focusFirstEditorGroup",
    "workbench.action.focusLastEditorGroup",
    "workbench.action.firstEditorInGroup",
    "workbench.action.lastEditorInGroup",
    "workbench.action.navigateUp",
    "workbench.action.navigateDown",
    "workbench.action.navigateRight",
    "workbench.action.navigateLeft",
    "workbench.action.togglePanel",
    "workbench.action.quickOpenView",
    "workbench.action.toggleMaximizedPanel",
    "workbench.action.zoomIn",
    "workbench.action.zoomOut",
    "workbench.action.zoomReset",
    "notification.acceptPrimaryAction",
    "runCommands",
    "workbench.action.terminal.chat.start",
    "workbench.action.terminal.chat.close",
    "workbench.action.terminal.chat.discard",
    "workbench.action.terminal.chat.makeRequest",
    "workbench.action.terminal.chat.cancel",
    "workbench.action.terminal.chat.feedbackHelpful",
    "workbench.action.terminal.chat.feedbackUnhelpful",
    "workbench.action.terminal.chat.feedbackReportIssue",
    "workbench.action.terminal.chat.runCommand",
    "workbench.action.terminal.chat.insertCommand",
    "workbench.action.terminal.chat.viewInChat",
];

/// `DEFAULT_COMMANDS_TO_SKIP_SHELL`: os comandos que, por padrão, pulam o shell e são tratados como atalhos comuns.
pub fn default_commands_to_skip_shell() -> Vec<&'static str> {
    use TerminalCommandId as Id;
    let mut commands: Vec<&'static str> = [
        Id::ClearSelection,
        Id::Clear,
        Id::CopyAndClearSelection,
        Id::CopySelection,
        Id::CopySelectionAsHtml,
        Id::CopyLastCommand,
        Id::CopyLastCommandOutput,
        Id::CopyLastCommandAndLastCommandOutput,
        Id::DeleteToLineStart,
        Id::DeleteWordLeft,
        Id::DeleteWordRight,
        Id::FocusNextPane,
        Id::FocusNext,
        Id::FocusPreviousPane,
        Id::FocusPrevious,
        Id::Focus,
        Id::SizeToContentWidth,
        Id::Kill,
        Id::KillEditor,
        Id::MoveToEditor,
        Id::MoveToLineEnd,
        Id::MoveToLineStart,
        Id::MoveToTerminalPanel,
        Id::NewInActiveWorkspace,
        Id::New,
        Id::NewInNewWindow,
        Id::Paste,
        Id::PastePwsh,
        Id::PasteSelection,
        Id::ResizePaneDown,
        Id::ResizePaneLeft,
        Id::ResizePaneRight,
        Id::ResizePaneUp,
        Id::RunActiveFile,
        Id::RunSelectedText,
        Id::ScrollDownLine,
        Id::ScrollDownPage,
        Id::ScrollToBottom,
        Id::ScrollToNextCommand,
        Id::ScrollToPreviousCommand,
        Id::ScrollToTop,
        Id::ScrollUpLine,
        Id::ScrollUpPage,
        Id::SendSequence,
        Id::SelectAll,
        Id::SelectToNextCommand,
        Id::SelectToNextLine,
        Id::SelectToPreviousCommand,
        Id::SelectToPreviousLine,
        Id::SplitInActiveWorkspace,
        Id::Split,
        Id::Toggle,
        Id::FocusHover,
    ]
    .map(TerminalCommandId::as_str)
    .to_vec();
    commands.push(OPEN_ACCESSIBILITY_HELP);
    commands.push(Id::StopVoice.as_str());
    commands.push(Id::SendSignal.as_str());
    commands.extend(LITERAL_COMMANDS_TO_SKIP_SHELL);
    commands.extend(DEFAULT_TERMINAL_ACCESSIBILITY_COMMANDS_TO_SKIP_SHELL);
    commands.extend(DEFAULT_TERMINAL_FIND_COMMAND_TO_SKIP_SHELL);
    commands.extend(DEFAULT_TERMINAL_HISTORY_COMMANDS_TO_SKIP_SHELL.iter().map(|command| command.as_str()));
    commands.extend(DEFAULT_TERMINAL_SUGGEST_COMMANDS_TO_SKIP_SHELL);
    commands
}

/// O `IShellLaunchConfig` compartilhado entre a instância, o gerenciador de processo e o resolvedor de perfil (ver
/// "Divergências").
pub type SharedShellLaunchConfig = Rc<RefCell<IShellLaunchConfig>>;

/// `IShellLaunchConfigResolveOptions`.
#[derive(Clone, Copy, Debug)]
pub struct IShellLaunchConfigResolveOptions<'a> {
    pub remote_authority: Option<&'a str>,
    pub os: OperatingSystem,
    pub allow_automation_shell: Option<bool>,
    pub allow_agent_host_shell: Option<bool>,
}

/// `ITerminalProfileResolverService`.
pub trait ITerminalProfileResolverService {
    fn default_profile_name(&self) -> Option<String>;

    /// Resolves the icon of a shell launch config if this will use the default profile.
    fn resolve_icon(&self, shell_launch_config: &mut IShellLaunchConfig, os: OperatingSystem);
    fn resolve_shell_launch_config(&self, shell_launch_config: SharedShellLaunchConfig, options: IShellLaunchConfigResolveOptions<'_>) -> Promise<()>;
    fn get_default_profile(&self, options: IShellLaunchConfigResolveOptions<'_>) -> Promise<ITerminalProfile>;
    fn get_default_shell(&self, options: IShellLaunchConfigResolveOptions<'_>) -> Promise<String>;
    fn get_default_shell_args(&self, options: IShellLaunchConfigResolveOptions<'_>) -> Promise<OneOrMany<String>>;
    /// `getDefaultIcon(): TerminalIcon & ThemeIcon`.
    fn get_default_icon(&self) -> ThemeIcon;
    fn get_environment(&self, remote_authority: Option<&str>) -> Promise<je_base::process::ProcessEnvironment>;
}

service_id!(pub I_TERMINAL_PROFILE_RESOLVER_SERVICE: dyn ITerminalProfileResolverService = "terminalProfileResolverService");

/// `ITerminalStatusHoverAction`.
#[derive(Clone)]
pub struct ITerminalStatusHoverAction {
    pub label: String,
    pub command_id: String,
    pub run: Rc<dyn Fn()>,
}

/// `ITerminalStatus`.
#[derive(Clone)]
pub struct ITerminalStatus {
    /// An internal string ID used to identify the status.
    pub id: String,
    /// The severity of the status, this defines both the color and how likely the status is to be the "primary status".
    pub severity: Severity,
    /// An icon representing the status, if this is not specified it will not show up on the terminal tab and will use the
    /// generic `info` icon when hovering.
    pub icon: Option<ThemeIcon>,
    /// What to show for this status in the terminal's hover.
    pub tooltip: Option<String>,
    /// What to show for this status in the terminal's hover when details are toggled.
    pub detailed_tooltip: Option<String>,
    /// Actions to expose on hover.
    pub hover_actions: Option<Vec<ITerminalStatusHoverAction>>,
}

/// `ITerminalProfileProvider`: o provedor de perfil que uma extensão registra.
pub trait ITerminalProfileProvider {
    fn create_contributed_terminal_profile(&self, options: ICreateContributedTerminalProfileOptions) -> Promise<()>;
}

/// `ITerminalProfileService`, reduzido ao que o `MainThreadTerminalService` lê (ver as divergências no topo do módulo).
pub trait ITerminalProfileService {
    /// `onDidChangeAvailableProfiles`.
    fn on_did_change_available_profiles(&self) -> Event<Vec<ITerminalProfile>>;
    /// `registerTerminalProfileProvider(extensionIdentifier, id, profileProvider)`.
    fn register_terminal_profile_provider(&self, extension_identifier: &str, id: &str, profile_provider: Rc<dyn ITerminalProfileProvider>) -> DisposableRef;
}

service_id!(pub I_TERMINAL_PROFILE_SERVICE: dyn ITerminalProfileService = "terminalProfileService");

/// `ITerminalProcessExtHostProxy`: o processo de um terminal de extensão, visto da thread principal.
pub trait ITerminalProcessExtHostProxy: IDisposable {
    fn instance_id(&self) -> u32;

    fn emit_data(&self, data: &str);
    fn emit_process_property(&self, property: IProcessProperty);
    fn emit_ready(&self, pid: i32, cwd: &str, windows_pty: Option<IProcessReadyWindowsPty>);
    fn emit_exit(&self, exit_code: Option<i32>);

    fn on_input(&self) -> Event<String>;
    fn on_binary(&self) -> Event<String>;
    fn on_resize(&self) -> Event<ITerminalDimensions>;
    fn on_acknowledge_data_event(&self) -> Event<usize>;
    fn on_shutdown(&self) -> Event<bool>;
    fn on_request_initial_cwd(&self) -> Event<()>;
    fn on_request_cwd(&self) -> Event<()>;
}

/// `IStartExtensionTerminalRequest`.
#[derive(Clone)]
pub struct IStartExtensionTerminalRequest {
    pub proxy: Rc<dyn ITerminalProcessExtHostProxy>,
    pub cols: i32,
    pub rows: i32,
    pub callback: Rc<dyn Fn(Option<ITerminalLaunchError>)>,
}

/// `IBeforeProcessDataEvent`.
pub struct IBeforeProcessDataEvent {
    /// The data of the event, this can be modified by the event listener to change what gets sent to the terminal.
    pub data: RefCell<String>,
}

/// `ITerminalProcessInfo`: read-only process information that can apply to detached terminals.
pub trait ITerminalProcessInfo {
    fn process_state(&self) -> ProcessState;
    fn pty_process_ready(&self) -> Promise<()>;
    fn shell_process_id(&self) -> Option<i32>;
    fn remote_authority(&self) -> Option<String>;
    fn os(&self) -> Option<OperatingSystem>;
    fn user_home(&self) -> Option<String>;
    fn initial_cwd(&self) -> String;
    fn environment_variable_info(&self) -> Option<Rc<dyn IEnvironmentVariableInfo>>;
    fn persistent_process_id(&self) -> Option<u32>;
    fn should_persist(&self) -> bool;
    fn has_written_data(&self) -> bool;
    fn has_child_processes(&self) -> bool;
    fn backend(&self) -> Option<Rc<dyn ITerminalBackend>>;
    fn capabilities(&self) -> Rc<dyn ITerminalCapabilityStore>;
    fn shell_integration_nonce(&self) -> String;
    fn ext_environment_variable_collection(&self) -> Option<Rc<dyn IMergedEnvironmentVariableCollection>>;
}

/// `ITerminalProcessManager`.
pub trait ITerminalProcessManager: je_base::lifecycle::IDisposable + ITerminalProcessInfo {
    fn process_traits(&self) -> Option<IProcessReadyEvent>;
    fn process_ready_timestamp(&self) -> f64;

    fn on_pty_disconnect(&self) -> Event<()>;
    fn on_pty_reconnect(&self) -> Event<()>;

    fn on_process_ready(&self) -> Event<IProcessReadyEvent>;
    fn on_before_process_data(&self) -> Event<Rc<IBeforeProcessDataEvent>>;
    fn on_process_data(&self) -> Event<Rc<IProcessDataEvent>>;
    fn on_process_replay_complete(&self) -> Event<()>;
    fn on_environment_variable_info_changed(&self) -> Event<Rc<dyn IEnvironmentVariableInfo>>;
    fn on_did_change_property(&self) -> Event<IProcessProperty>;
    fn on_process_exit(&self) -> Event<Option<i32>>;
    fn on_restore_commands(&self) -> Event<ISerializedCommandDetectionCapability>;

    /// `dispose(immediate?)`.
    fn dispose_immediate(&self, immediate: bool);
    fn detach_from_process(&self, force_persist: Option<bool>) -> Promise<()>;
    fn create_process(&self, shell_launch_config: SharedShellLaunchConfig, cols: i32, rows: i32) -> Promise<Option<ITerminalLaunchOutcome>>;
    fn relaunch(&self, shell_launch_config: SharedShellLaunchConfig, cols: i32, rows: i32, reset: bool) -> Promise<Option<ITerminalLaunchOutcome>>;
    fn write(&self, data: &str) -> Promise<()>;
    fn send_signal(&self, signal: &str) -> Promise<()>;
    /// `setDimensions(cols, rows, sync?: undefined | false, pixelWidth?, pixelHeight?)`.
    fn set_dimensions(&self, cols: i32, rows: i32, pixel_width: Option<i32>, pixel_height: Option<i32>) -> Promise<()>;
    /// `setDimensions(cols, rows, true, pixelWidth?, pixelHeight?)`.
    fn set_dimensions_sync(&self, cols: i32, rows: i32, pixel_width: Option<i32>, pixel_height: Option<i32>);
    fn clear_buffer(&self) -> Promise<()>;
    fn set_unicode_version(&self, version: UnicodeVersion) -> Promise<()>;
    fn set_next_command_id(&self, command_line: &str, command_id: &str) -> Promise<()>;
    fn acknowledge_data_event(&self, char_count: usize);
    fn process_binary(&self, data: &str) -> Promise<()>;

    fn refresh_property(&self, property_type: ProcessPropertyType) -> Promise<IProcessProperty>;
    fn update_property(&self, property: IProcessProperty) -> Promise<()>;
    fn get_backend_os(&self) -> Promise<OperatingSystem>;
    fn free_port_kill_process(&self, port: &str) -> Promise<()>;
}

