//! Porte parcial de `src/vs/workbench/contrib/terminal/browser/terminal.ts`: `ITerminalConfigurationService` e `LinuxDistro`
//! (mais os tipos que o serviço recebe: o contêiner do painel e o núcleo do xterm).
//!
//! O resto do arquivo (`ITerminalService`, `ITerminalInstance`, `ITerminalGroupService`...) entra quando um consumidor pedir.
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

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_instantiation::service_id;
use je_platform::terminal::terminal::TerminalLocation;

use crate::contrib::terminal::common::terminal::TerminalFont;

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
