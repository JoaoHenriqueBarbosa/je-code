//! Porte de `src/vs/workbench/services/statusbar/browser/statusbar.ts`: os tipos de posição e prioridade das
//! entradas da barra de status, que o modelo de visão ([`StatusbarViewModel`]) usa para ordená-las.
//!
//! Divergências:
//! - **Só o que o modelo usa** é portado: [`StatusbarAlignment`], [`StatusbarEntryLocation`],
//!   [`StatusbarEntryPriority`] e [`StatusbarEntryKind`]. O restante do arquivo é especificação:
//!   - `IStatusbarService` e `IStatusbarEntryAccessor` (o serviço, com `getPart(container)` e
//!     `createAuxiliaryStatusbarPart(container)`) entram junto com a parte da barra de status (`statusbarPart.ts`).
//!   - `IStatusbarStyleOverride` depende de `ColorIdentifier`.
//!   - A entrada declarativa é [`StatusbarEntry`] (`IStatusbarEntry`), com `HTMLElement` como `ViewElement`, o
//!     `command` como [`StatusbarCommand`] (`ShowTooltipCommand` e `ToggleTooltipCommand` são variantes, comparadas
//!     por identidade como no original) e a dica como [`StatusbarTooltip`] (`isTooltipWithCommands` é
//!     [`is_tooltip_with_commands`]).
//! - `isStatusbarEntryLocation(thing: unknown)` e `isStatusbarEntryPriority(thing: unknown)` são guardas de tipo
//!   sobre um valor desconhecido. Em Rust o tipo já distingue os casos: um `primary` é um número ou uma
//!   localização ([`StatusbarEntryPrimaryPriority`]), e [`StatusbarEntryPrimaryPriority::location`] faz o papel de
//!   `isStatusbarEntryLocation` (devolve a localização quando é uma).
//! - `StatusbarAlignment` é um `const enum` numérico (`LEFT` = 0, `RIGHT` = 1); os valores são mantidos.
//! - `primary` e `secondary` são `number` no original, e aqui `f64`: o modelo ordena prioridades infinitas e
//!   `Number.MAX_VALUE`.
//!
//! [`StatusbarViewModel`]: crate::browser::parts::statusbar::statusbar_model::StatusbarViewModel

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::html_content::IMarkdownString;
use je_base::themables::ThemeColor;
use je_editor::code_action::languages::Command;
use je_platform::contextview::view_element::ViewElement;
use je_platform::hover::hover_service::ManagedHoverTooltip;

use crate::services::progress::progress_service::StatusbarShowProgress;

/// O `id` de `ShowTooltipCommand`.
pub const SHOW_TOOLTIP_COMMAND_ID: &str = "statusBar.entry.showTooltip";

/// O `id` de `ToggleTooltipCommand`.
pub const TOGGLE_TOOLTIP_COMMAND_ID: &str = "statusBar.entry.toggleTooltip";

/// O `command` de `IStatusbarEntry` (`string | Command | typeof ShowTooltipCommand | typeof ToggleTooltipCommand`).
/// `ShowTooltipCommand` e `ToggleTooltipCommand` são objetos únicos no original e comparados por identidade, então cada
/// um é uma variante; um `Command` qualquer é comparado pelo `Rc`, e um texto por valor, como o `!==` do JavaScript.
#[derive(Clone)]
pub enum StatusbarCommand {
    Id(String),
    Command(Rc<Command>),
    ShowTooltip,
    ToggleTooltip,
}

impl StatusbarCommand {
    /// `command.id`, ou o próprio texto.
    pub fn id(&self) -> String {
        match self {
            Self::Id(id) => id.clone(),
            Self::Command(command) => command.id.clone(),
            Self::ShowTooltip => SHOW_TOOLTIP_COMMAND_ID.to_owned(),
            Self::ToggleTooltip => TOGGLE_TOOLTIP_COMMAND_ID.to_owned(),
        }
    }

    /// `command.title` (vazio nos comandos de dica de ferramenta e no texto).
    pub fn title(&self) -> String {
        match self {
            Self::Command(command) => command.title.clone(),
            _ => String::new(),
        }
    }

    /// `command.arguments ?? []`.
    pub fn arguments(&self) -> Vec<Dynamic> {
        match self {
            Self::Command(command) => command.arguments.clone().unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// `a === b`.
    pub fn is_same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Id(a), Self::Id(b)) => a == b,
            (Self::Command(a), Self::Command(b)) => Rc::ptr_eq(a, b),
            (Self::ShowTooltip, Self::ShowTooltip) | (Self::ToggleTooltip, Self::ToggleTooltip) => true,
            _ => false,
        }
    }
}

/// `a === b` sobre dois `command?` opcionais.
pub fn is_same_command(a: Option<&StatusbarCommand>, b: Option<&StatusbarCommand>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.is_same(b),
        _ => false,
    }
}

/// `TooltipContent`: `string | IMarkdownString | HTMLElement | IManagedHoverTooltipMarkdownString | IManagedHoverTooltipHTMLElement`.
/// Os dois últimos são o [`ManagedHoverTooltip`], comparado pelo `Rc` (objeto no original).
#[derive(Clone)]
pub enum TooltipContent {
    Text(String),
    Markdown(IMarkdownString),
    Element(Rc<ViewElement>),
    Managed(Rc<ManagedHoverTooltip>),
}

/// `ITooltipWithCommands`.
pub struct TooltipWithCommands {
    pub content: TooltipContent,
    pub commands: Vec<StatusbarCommand>,
    /// Called only after the tooltip is displayed.
    pub on_did_show: Option<Rc<dyn Fn()>>,
}

/// O `tooltip` de `IStatusbarEntry` (`TooltipContent | ITooltipWithCommands`).
#[derive(Clone)]
pub enum StatusbarTooltip {
    Content(TooltipContent),
    WithCommands(Rc<TooltipWithCommands>),
}

/// `isTooltipWithCommands(thing)`: `!!candidate?.content && Array.isArray(candidate?.commands)`.
pub fn is_tooltip_with_commands(tooltip: &StatusbarTooltip) -> Option<&Rc<TooltipWithCommands>> {
    match tooltip {
        StatusbarTooltip::WithCommands(with_commands) if !matches!(&with_commands.content, TooltipContent::Text(text) if text.is_empty()) => Some(with_commands),
        _ => None,
    }
}

/// O `color` e o `backgroundColor` de `IStatusbarEntry` (`string | ThemeColor`). O `ThemeColor` é um objeto no original e
/// comparado por identidade, por isso o `Rc`.
#[derive(Clone)]
pub enum StatusbarColor {
    Css(String),
    Theme(Rc<ThemeColor>),
}

impl StatusbarColor {
    /// `!!color`.
    pub fn is_truthy(&self) -> bool {
        !matches!(self, Self::Css(css) if css.is_empty())
    }

    /// `a === b`.
    pub fn is_same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Css(a), Self::Css(b)) => a == b,
            (Self::Theme(a), Self::Theme(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// `a === b` sobre duas cores opcionais.
pub fn is_same_color(a: Option<&StatusbarColor>, b: Option<&StatusbarColor>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.is_same(b),
        _ => false,
    }
}

/// `IStatusbarEntry`: a descrição declarativa de uma entrada da barra de status.
#[derive(Clone)]
pub struct StatusbarEntry {
    /// The (short) name to show for the entry like 'Language Indicator', 'Git Status' etc.
    pub name: String,
    /// The text to show for the entry. Icons are embedded with `$(icon name)`.
    pub text: String,
    /// Text to be read out by the screen reader.
    pub aria_label: String,
    /// Role of the status bar entry (default `button`).
    pub role: Option<String>,
    pub tooltip: Option<StatusbarTooltip>,
    pub color: Option<StatusbarColor>,
    pub background_color: Option<StatusbarColor>,
    pub command: Option<StatusbarCommand>,
    pub show_beak: Option<bool>,
    pub show_progress: Option<StatusbarShowProgress>,
    pub kind: Option<StatusbarEntryKind>,
    pub show_in_all_windows: Option<bool>,
    pub extension_id: Option<String>,
    /// Allows to add content with custom rendering to the status bar entry (`HTMLElement`).
    pub content: Option<Rc<ViewElement>>,
}

impl StatusbarEntry {
    /// Uma entrada só com os campos obrigatórios.
    pub fn new(name: impl Into<String>, text: impl Into<String>, aria_label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
            aria_label: aria_label.into(),
            role: None,
            tooltip: None,
            color: None,
            background_color: None,
            command: None,
            show_beak: None,
            show_progress: None,
            kind: None,
            show_in_all_windows: None,
            extension_id: None,
            content: None,
        }
    }
}

/// `StatusbarAlignment`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusbarAlignment {
    Left = 0,
    Right = 1,
}

/// O `location` de [`StatusbarEntryLocation`]: `{ id: string; priority: number }`.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusbarLocation {
    /// The identifier of another status bar entry to position relative to. If the referenced entry does not
    /// exist, the priority will be used.
    pub id: String,
    /// A prioridade usada no lugar da entrada referenciada quando ela não existe.
    pub priority: f64,
}

/// `IStatusbarEntryLocation`.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusbarEntryLocation {
    /// The identifier and priority of another status bar entry to position relative to. If the referenced
    /// entry does not exist, the priority will be used.
    pub location: StatusbarLocation,

    /// The alignment of the status bar entry relative to the referenced entry.
    pub alignment: StatusbarAlignment,

    /// Whether to move the entry close to the location so that it appears as if both this entry and the
    /// location belong to each other.
    pub compact: Option<bool>,
}

/// O `number | IStatusbarEntryLocation` de `IStatusbarEntryPriority.primary`.
#[derive(Clone, Debug, PartialEq)]
pub enum StatusbarEntryPrimaryPriority {
    Number(f64),
    Location(StatusbarEntryLocation),
}

impl StatusbarEntryPrimaryPriority {
    /// `isStatusbarEntryLocation(primary)`: a localização, quando o `primary` é uma.
    pub fn location(&self) -> Option<&StatusbarEntryLocation> {
        match self {
            Self::Location(location) => Some(location),
            Self::Number(_) => None,
        }
    }
}

/// `IStatusbarEntryPriority`.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusbarEntryPriority {
    /// The main priority of the entry that defines the order of appearance: either a number or a reference to
    /// another status bar entry to position relative to.
    ///
    /// May not be unique across all entries.
    pub primary: StatusbarEntryPrimaryPriority,

    /// The secondary priority of the entry is used in case the main priority matches another one's priority.
    ///
    /// Should be unique across all entries.
    pub secondary: f64,
}

/// `StatusbarEntryKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusbarEntryKind {
    Standard,
    Warning,
    Error,
    Prominent,
    Remote,
    Offline,
}

/// `StatusbarEntryKinds`.
pub const STATUSBAR_ENTRY_KINDS: [StatusbarEntryKind; 6] = [
    StatusbarEntryKind::Standard,
    StatusbarEntryKind::Warning,
    StatusbarEntryKind::Error,
    StatusbarEntryKind::Prominent,
    StatusbarEntryKind::Remote,
    StatusbarEntryKind::Offline,
];

impl StatusbarEntryKind {
    /// O literal do tipo no original (`'standard' | 'warning' | ...`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Prominent => "prominent",
            Self::Remote => "remote",
            Self::Offline => "offline",
        }
    }
}

/// `IStatusbarStyleOverride`: o `background`, o `foreground` e a `border` (identificadores de cor do tema) que sobrescrevem
/// a aparência da barra de status. Quem o cria o guarda num `Rc`: a barra o identifica pela identidade do objeto, como o
/// `Set<IStatusbarStyleOverride>` do original.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusbarStyleOverride {
    /// A prioridade: vale o de menor número.
    pub priority: f64,
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub border: Option<String>,
}

/// `IStatusbarEntryAccessor`: atualiza ou remove (`dispose`) uma entrada adicionada à barra.
pub trait StatusbarEntryAccessor: je_base::lifecycle::IDisposable {
    /// `update(entry)`.
    fn update(&self, entry: StatusbarEntry);
}

/// `Partial<IStatusbarEntry>`: o que `overrideEntry` troca na entrada (`{ ...entry, ...override }`). A identidade do `Rc` é a do
/// objeto do original.
pub type StatusbarEntryOverride = Rc<dyn Fn(&mut StatusbarEntry)>;
