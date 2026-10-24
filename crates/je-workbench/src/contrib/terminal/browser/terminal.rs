//! Porte parcial de `src/vs/workbench/contrib/terminal/browser/terminal.ts`: `ITerminalConfigurationService`, `LinuxDistro`
//! (mais os tipos que o serviço recebe: o contêiner do painel e o núcleo do xterm), o `ITerminalInstanceService`, e as
//! versões reduzidas de `ITerminalService` e `ITerminalInstance` que o resto do porte já lê.
//!
//! O resto do arquivo (`ITerminalGroupService`, `ITerminalEditorService`, `IXtermTerminal`...) entra quando um consumidor
//! pedir.
//!
//! Divergências:
//! - **`config`.** `Readonly<ITerminalConfiguration>` é um [`Dynamic`] (objeto): a configuração tem cerca de 120
//!   propriedades e o serviço só precisa delas como o `getValue` as devolve. Cada leitura devolve o mesmo objeto
//!   (a identidade muda quando o serviço recria a configuração, o que `notStrictEqual` observa, ver [`Dynamic::identity`]).
//! - **`setPanelContainer(HTMLElement)`.** Não há DOM: o contêiner é um [`ITerminalPanelContainer`], que sabe medir a caixa
//!   de um texto numa fonte (o que o elemento `div` de medição do original faz com `getBoundingClientRect`).
//! - **`getFont(w: Window, ...)`.** A janela só serve para o `devicePixelRatio`, que é um número aqui.
//! - **`xtermCore?: IXtermCore`.** O núcleo privado do xterm vira [`IXtermCore`], que devolve as dimensões em CSS de uma
//!   célula quando o renderizador existe (`_renderService._renderer.value` definido).
//! - **`ITerminalService`** tem o que o gerenciador de processo, a informação de variáveis de ambiente e as pontes
//!   `MainThreadTerminalService` e `MainThreadTerminalShellIntegration` leem: `instances`, `activeInstance`, os eventos
//!   `onDidCreateInstance`, `onDidDisposeInstance`, `onAnyInstanceProcessIdReady`, `onDidChangeInstanceDimensions`,
//!   `onAnyInstanceMaximumDimensionsChange`, `onDidRequestStartExtensionTerminal`, `onDidChangeActiveInstance`,
//!   `onAnyInstanceTitleChange`, `onAnyInstanceDataInput` e `onAnyInstanceShellTypeChanged`, `registerProcessSupport`,
//!   `setActiveInstance`, `createTerminal`, `requestStartExtensionTerminal`, `getInstanceFromId` e `setNextCommandId`.
//!   `createOnInstanceEvent<T>` e `createOnInstanceCapabilityEvent<T, K>` (genéricos, fora de um trait de objeto) são o
//!   [`ITerminalServiceEvents`](super::terminal_events::ITerminalServiceEvents), um trait de extensão implementado para todo
//!   serviço, que monta o multiplexador com `instances`, `onDidCreateInstance` e `onDidDisposeInstance`, como o
//!   `TerminalService` do original. `onAnyInstanceSelectionChange` e `ITerminalInstance#selection` ficam de fora: o contrato do
//!   emulador do porte não tem seleção (ver `terminal_instance.rs`).
//!   **`ITerminalInstance`** tem o que esses consumidores leem (o id, o título, o relançamento, o estado de descarte, os
//!   eventos de descarte, as capacidades, o tipo de shell, o local, o id e a prontidão do processo, as dimensões, o código e o
//!   motivo de saída, os dados iniciais, `onData`, `runCommand`, `sendText`, `rename` e `dispose(reason)`); a classe
//!   [`TerminalInstance`](super::terminal_instance::TerminalInstance) tem o resto como métodos próprios.
//! - **`ICreateTerminalOptions`** tem `config` (`IShellLaunchConfig | ITerminalProfile`, sem o `IExtensionTerminalProfile`),
//!   `cwd`, `resource`, `location` e `skipContributedProfileCheck`. O `location` é o [`TerminalCreateLocation`].
//! - **`ITerminalGroupService`** e **`ITerminalEditorService`** têm só `showPanel`/`hidePanel` e `revealActiveEditor`.
//! - **`ITerminalLink`** e **`ITerminalExternalLinkProvider`** são os do `MainThreadTerminalService` (o `activate` é um `Rc<dyn Fn()>`).
//! - **`ITerminalInstanceService.createInstance(launchConfig, target, editorOptions?)`** não leva o `editorOptions`
//!   (`TerminalEditorLocation`, do serviço de editores de terminal, que não existe no porte).
//! - **`convertProfileToShellLaunchConfig(IShellLaunchConfig | ITerminalProfile | undefined, cwd?)`** recebe o
//!   [`ShellLaunchConfigOrProfile`].
//! - **`getRegisteredBackends(): IterableIterator`** é um `Vec`.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::terminal::capabilities::capabilities::ITerminalCapabilityStore;
use je_platform::terminal::terminal::{
    IShellLaunchConfig, ITerminalLaunchError, ITerminalProfile, ShellCwd, TerminalExitReason, TerminalLocation, TerminalShellType,
};

use crate::contrib::terminal::common::terminal::{IStartExtensionTerminalRequest, ITerminalProcessExtHostProxy, SharedShellLaunchConfig, TerminalFont};
use crate::contrib::terminal::common::terminal_backend::ITerminalBackend;

/// `LinuxDistro`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxDistro {
    Unknown = 1,
    Fedora = 2,
    Ubuntu = 3,
}

/// A caixa de um texto medido (`DOMRect`), em pixels CSS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClientRect {
    pub width: f64,
    pub height: f64,
}

/// O elemento do painel (`HTMLElement`) onde o serviço cria o elemento de medição de caractere.
pub trait ITerminalPanelContainer {
    /// `_getBoundingRectFor`: mede `text` na família e no tamanho dados (o elemento `inline-block` com `lineHeight: normal`).
    /// O elemento de medição é criado se ainda não existe ou se o pai o descartou.
    fn measure_text(&self, text: &str, font_family: &str, font_size: f64) -> ClientRect;

    /// `this._charMeasureElement?.remove()`: o serviço descarta o elemento de medição ao ser descartado.
    fn remove_char_measure_element(&self);
}

/// O que o serviço lê do `IXtermCore` privado.
pub trait IXtermCore {
    /// `_renderService.dimensions.css.cell` quando `_renderService._renderer.value` existe, e `None` sem renderizador.
    fn css_cell_dimensions(&self) -> Option<ClientRect>;
}

/// `ITerminalConfigurationService`: acesso à configuração do terminal e aos valores derivados dela.
pub trait ITerminalConfigurationService {
    /// A representação tipada e parcialmente validada da configuração do terminal.
    fn config(&self) -> Dynamic;

    /// O local padrão dos terminais.
    fn default_location(&self) -> TerminalLocation;

    /// Dispara quando algo na configuração do terminal muda.
    fn on_config_changed(&self) -> Event<()>;

    fn set_panel_container(&self, panel_container: std::rc::Rc<dyn ITerminalPanelContainer>);
    fn config_font_is_monospace(&self) -> bool;
    fn get_font(&self, device_pixel_ratio: f64, xterm_core: Option<&dyn IXtermCore>, exclude_dimensions: bool) -> TerminalFont;

    /// Se um comando deve pular o shell e ser tratado como um atalho comum.
    fn should_command_skip_shell(&self, command_id: &str) -> bool;
}

service_id!(pub I_TERMINAL_CONFIGURATION_SERVICE: dyn ITerminalConfigurationService = "terminalConfigurationService");

/// `ITerminalInstance`, reduzido ao que o gerenciador de processo, a informação de variáveis de ambiente e as pontes
/// `MainThreadTerminalService` e `MainThreadTerminalShellIntegration` leem (ver "Divergências").
pub trait ITerminalInstance {
    fn instance_id(&self) -> u32;
    fn title(&self) -> String;
    fn is_disposed(&self) -> bool;
    fn shell_launch_config(&self) -> SharedShellLaunchConfig;

    /// Fired when the terminal instance is about to be disposed (the terminal is still alive for the listeners).
    fn on_will_dispose(&self) -> Event<Rc<dyn ITerminalInstance>>;
    /// Fired after the terminal instance (and its xterm) has been disposed.
    fn on_disposed(&self) -> Event<Rc<dyn ITerminalInstance>>;

    /// Relaunches the process with the same launch config (debounced by one second, as in the original).
    fn relaunch(&self);

    /// `capabilities`.
    fn capabilities(&self) -> Rc<dyn ITerminalCapabilityStore>;
    /// `shellType`.
    fn shell_type(&self) -> TerminalShellType;
    /// `target`.
    fn target(&self) -> Option<TerminalLocation>;
    /// `processId`: o `-1` de um terminal de extensão cabe aqui.
    fn process_id(&self) -> Option<i32>;
    /// `processReady`.
    fn process_ready(&self) -> Promise<()>;
    /// `cols`, `rows`, `maxCols` e `maxRows`.
    fn cols(&self) -> i32;
    fn rows(&self) -> i32;
    fn max_cols(&self) -> i32;
    fn max_rows(&self) -> i32;
    /// `exitCode` e `exitReason`.
    fn exit_code(&self) -> Option<i32>;
    fn exit_reason(&self) -> Option<TerminalExitReason>;
    /// `initialDataEvents`.
    fn initial_data_events(&self) -> Option<Vec<String>>;
    /// `onData`.
    fn on_data(&self) -> Event<String>;
    /// `runCommand(commandLine, shouldExecute)` (sem os argumentos opcionais).
    fn run_command(&self, command_line: &str, should_execute: bool) -> Promise<()>;
    /// `sendText(text, shouldExecute)` (sem o `forceBracketedPasteMode`).
    fn send_text(&self, text: &str, should_execute: bool) -> Promise<()>;
    /// `rename(title?)`.
    fn rename(&self, title: Option<&str>);
    /// `dispose(reason?)`.
    fn dispose_with_reason(&self, reason: Option<TerminalExitReason>);
}

/// `ITerminalLink`.
#[derive(Clone)]
pub struct ITerminalLink {
    pub id: u32,
    pub start_index: f64,
    pub length: f64,
    pub label: Option<String>,
    pub activate: Rc<dyn Fn()>,
}

/// `ITerminalExternalLinkProvider`.
pub trait ITerminalExternalLinkProvider {
    fn provide_links(&self, instance: Rc<dyn ITerminalInstance>, line: &str) -> Promise<Option<Vec<ITerminalLink>>>;
}

/// `ITerminalGroupService`, reduzido ao que o `MainThreadTerminalService` lê.
pub trait ITerminalGroupService {
    /// `showPanel(focus?)`.
    fn show_panel(&self, focus: bool) -> Promise<()>;
    /// `hidePanel()`.
    fn hide_panel(&self);
}

service_id!(pub I_TERMINAL_GROUP_SERVICE: dyn ITerminalGroupService = "terminalGroupService");

/// `ITerminalEditorService`, reduzido ao que o `MainThreadTerminalService` lê.
pub trait ITerminalEditorService {
    /// `revealActiveEditor(preserveFocus?)`.
    fn reveal_active_editor(&self, preserve_focus: bool) -> Promise<()>;
}

service_id!(pub I_TERMINAL_EDITOR_SERVICE: dyn ITerminalEditorService = "terminalEditorService");

/// O `location` de `ICreateTerminalOptions`: `TerminalLocation | TerminalEditorLocationOptions | { parentTerminal: ITerminalInstance } |
/// { splitActiveTerminal: boolean }`.
#[derive(Clone)]
pub enum TerminalCreateLocation {
    Location(TerminalLocation),
    /// `TerminalEditorLocationOptions`: `{ viewColumn, preserveFocus? }`.
    Editor { view_column: f64, preserve_focus: Option<bool> },
    ParentTerminal(Rc<dyn ITerminalInstance>),
    SplitActiveTerminal(bool),
}

/// `ICreateTerminalOptions`.
#[derive(Clone, Default)]
pub struct ICreateTerminalOptions {
    /// The shell launch config or profile to launch with, when not specified the default terminal profile will be used.
    pub config: Option<ShellLaunchConfigOrProfile>,
    /// The current working directory to launch the shell with, this will override the `cwd` property of `config`.
    pub cwd: Option<ShellCwd>,
    /// The terminal's resource, passed when the terminal has moved windows.
    pub resource: Option<URI>,
    /// The terminal's location (editor or panel).
    pub location: Option<TerminalCreateLocation>,
    /// This terminal will not wait for contributed profiles to resolve which means it will proceed to launch with the default
    /// profile.
    pub skip_contributed_profile_check: Option<bool>,
}

/// `ITerminalService`, reduzido ao que o gerenciador de processo, a informação de variáveis de ambiente e as pontes
/// `MainThreadTerminalService` e `MainThreadTerminalShellIntegration` leem (ver "Divergências").
pub trait ITerminalService {
    fn instances(&self) -> Vec<Rc<dyn ITerminalInstance>>;
    fn active_instance(&self) -> Option<Rc<dyn ITerminalInstance>>;

    fn on_did_create_instance(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_did_dispose_instance(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_any_instance_process_id_ready(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_did_change_instance_dimensions(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_any_instance_maximum_dimensions_change(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_did_request_start_extension_terminal(&self) -> Event<IStartExtensionTerminalRequest>;
    fn on_did_change_active_instance(&self) -> Event<Option<Rc<dyn ITerminalInstance>>>;
    fn on_any_instance_title_change(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_any_instance_data_input(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_any_instance_shell_type_changed(&self) -> Event<Rc<dyn ITerminalInstance>>;

    /// `registerProcessSupport(isSupported)`.
    fn register_process_support(&self, is_supported: bool);
    /// `setActiveInstance(instance)`.
    fn set_active_instance(&self, instance: Rc<dyn ITerminalInstance>);
    /// `createTerminal(options?)`.
    fn create_terminal(&self, options: Option<ICreateTerminalOptions>) -> Promise<Rc<dyn ITerminalInstance>>;
    /// `requestStartExtensionTerminal(proxy, cols, rows)`.
    fn request_start_extension_terminal(&self, proxy: Rc<dyn ITerminalProcessExtHostProxy>, cols: i32, rows: i32) -> Promise<Option<ITerminalLaunchError>>;

    fn get_instance_from_id(&self, terminal_id: u32) -> Option<Rc<dyn ITerminalInstance>>;
    fn set_next_command_id(&self, id: u32, command_line: &str, command_id: &str) -> Promise<()>;
}

service_id!(pub I_TERMINAL_SERVICE: dyn ITerminalService = "terminalService");

/// `IShellLaunchConfig | ITerminalProfile`: o argumento de `convertProfileToShellLaunchConfig`.
#[derive(Clone)]
pub enum ShellLaunchConfigOrProfile {
    LaunchConfig(IShellLaunchConfig),
    Profile(ITerminalProfile),
}

/// `ITerminalInstanceService`.
pub trait ITerminalInstanceService {
    /// An event that's fired when a terminal instance is created.
    fn on_did_create_instance(&self) -> Event<Rc<dyn ITerminalInstance>>;

    /// An event that's fired when a new backend is registered.
    fn on_did_register_backend(&self) -> Event<Rc<dyn ITerminalBackend>>;

    /// Helper function to convert a shell launch config, a profile or undefined into its equivalent shell launch config.
    /// `cwd` is a cwd to override.
    fn convert_profile_to_shell_launch_config(&self, shell_launch_config_or_profile: Option<ShellLaunchConfigOrProfile>, cwd: Option<ShellCwd>) -> IShellLaunchConfig;

    /// Create a new terminal instance.
    fn create_instance(&self, launch_config: IShellLaunchConfig, target: TerminalLocation) -> Rc<dyn ITerminalInstance>;

    /// Gets the registered backend for a remote authority (`None` = local). This is a convenience method to avoid using the
    /// more verbose fetching from the registry.
    fn get_backend(&self, remote_authority: Option<&str>) -> Promise<Option<Rc<dyn ITerminalBackend>>>;

    fn get_registered_backends(&self) -> Vec<Rc<dyn ITerminalBackend>>;
    fn did_register_backend(&self, backend: Rc<dyn ITerminalBackend>);
}

service_id!(pub I_TERMINAL_INSTANCE_SERVICE: dyn ITerminalInstanceService = "terminalInstanceService");
