//! Porte de `src/vs/workbench/contrib/terminalContrib/zoom/common/terminal.zoom.ts`: os identificadores de comando e de
//! configuração do zoom do terminal e a configuração `terminal.integrated.mouseWheelZoom`.
//!
//! Divergências:
//! - **`const enum`** de string são enums com `as_str()` (a convenção de `terminal_settings.rs`).
//! - **`IStringDictionary<IConfigurationPropertySchema>`** é a lista de pares `(chave, propriedade)` na ordem do original,
//!   com cada propriedade como [`PropertySchema`].
//! - **`isMacintosh`** é o `cfg!(target_os = "macos")`.

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_platform::configuration::registry::PropertySchema;

/// `TerminalZoomCommandId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalZoomCommandId {
    FontZoomIn,
    FontZoomOut,
    FontZoomReset,
}

impl TerminalZoomCommandId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FontZoomIn => "workbench.action.terminal.fontZoomIn",
            Self::FontZoomOut => "workbench.action.terminal.fontZoomOut",
            Self::FontZoomReset => "workbench.action.terminal.fontZoomReset",
        }
    }
}

/// `TerminalZoomSettingId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalZoomSettingId {
    MouseWheelZoom,
}

impl TerminalZoomSettingId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MouseWheelZoom => "terminal.integrated.mouseWheelZoom",
        }
    }
}

/// `terminalZoomConfiguration`.
pub fn terminal_zoom_configuration() -> Vec<(&'static str, PropertySchema)> {
    let markdown_description = if cfg!(target_os = "macos") {
        localize("terminal.integrated.mouseWheelZoom.mac", "Zoom the font of the terminal when using mouse wheel and holding `Cmd`.", &[])
    } else {
        localize("terminal.integrated.mouseWheelZoom", "Zoom the font of the terminal when using mouse wheel and holding `Ctrl`.", &[])
    };
    vec![(
        TerminalZoomSettingId::MouseWheelZoom.as_str(),
        PropertySchema::from_entries([
            ("markdownDescription", Dynamic::string(&markdown_description)),
            ("type", Dynamic::string("boolean")),
            ("default", Dynamic::Bool(false)),
        ]),
    )]
}
