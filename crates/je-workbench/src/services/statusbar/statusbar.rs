//! Porte de `src/vs/workbench/services/statusbar/browser/statusbar.ts`: os tipos de posição e prioridade das
//! entradas da barra de status, que o modelo de visão ([`StatusbarViewModel`]) usa para ordená-las.
//!
//! Divergências:
//! - **Só o que o modelo usa** é portado: [`StatusbarAlignment`], [`StatusbarEntryLocation`],
//!   [`StatusbarEntryPriority`] e [`StatusbarEntryKind`]. O restante do arquivo é especificação:
//!   - `IStatusbarService`, `IStatusbarEntryAccessor` e `IStatusbarEntry` descrevem o serviço e a entrada
//!     declarativa que o resto do workbench consome, e dependem de `IInstantiationService`, `HTMLElement`
//!     (`content`, `getPart(container)`, `createAuxiliaryStatusbarPart(container)`), `Command`, `IMarkdownString`
//!     e dos tipos de dica de ferramenta (`IManagedHoverTooltipHTMLElement`...), todos DOM ou de camadas ainda
//!     não portadas. Entram junto com a parte da barra de status (`statusbarPart.ts`).
//!   - `ShowTooltipCommand` e `ToggleTooltipCommand` (`Command` com `id` e `title` vazio) e `IStatusbarStyleOverride`
//!     também dependem de `Command` e de `ColorIdentifier`.
//! - `isStatusbarEntryLocation(thing: unknown)` e `isStatusbarEntryPriority(thing: unknown)` são guardas de tipo
//!   sobre um valor desconhecido. Em Rust o tipo já distingue os casos: um `primary` é um número ou uma
//!   localização ([`StatusbarEntryPrimaryPriority`]), e [`StatusbarEntryPrimaryPriority::location`] faz o papel de
//!   `isStatusbarEntryLocation` (devolve a localização quando é uma).
//! - `StatusbarAlignment` é um `const enum` numérico (`LEFT` = 0, `RIGHT` = 1); os valores são mantidos.
//! - `primary` e `secondary` são `number` no original, e aqui `f64`: o modelo ordena prioridades infinitas e
//!   `Number.MAX_VALUE`.
//!
//! [`StatusbarViewModel`]: crate::browser::parts::statusbar::statusbar_model::StatusbarViewModel

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
