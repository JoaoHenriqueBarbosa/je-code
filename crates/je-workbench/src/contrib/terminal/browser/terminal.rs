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
//! - **`ITerminalService`** tem só `getInstanceFromId` e `setNextCommandId`, que o gerenciador de processo e a informação de
//!   variáveis de ambiente leem. **`ITerminalInstance`** tem só o que esses consumidores leem (o id, o título, o relançamento,
//!   o estado de descarte e o `onDisposed`); a classe [`TerminalInstance`](super::terminal_instance::TerminalInstance) tem o
//!   resto como métodos próprios.
//! - **`ITerminalInstanceService.createInstance(launchConfig, target, editorOptions?)`** não leva o `editorOptions`
//!   (`TerminalEditorLocation`, do serviço de editores de terminal, que não existe no porte).
//! - **`convertProfileToShellLaunchConfig(IShellLaunchConfig | ITerminalProfile | undefined, cwd?)`** recebe o
//!   [`ShellLaunchConfigOrProfile`].
//! - **`getRegisteredBackends(): IterableIterator`** é um `Vec`.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::terminal::terminal::{IShellLaunchConfig, ITerminalProfile, ShellCwd, TerminalLocation};

use crate::contrib::terminal::common::terminal::{SharedShellLaunchConfig, TerminalFont};
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

/// `ITerminalInstance`, reduzido ao que o gerenciador de processo e a informação de variáveis de ambiente leem (ver
/// "Divergências").
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
}

/// `ITerminalService`, reduzido ao que o gerenciador de processo e a informação de variáveis de ambiente leem (ver
/// "Divergências").
pub trait ITerminalService {
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
