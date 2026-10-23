//! Porte de `src/vs/editor/common/cursorEvents.ts`: os eventos que descrevem a mudança do cursor.
//!
//! Decisões de tradução:
//!
//! - `CursorChangeReason` já tem porte em `view_model_types.rs` (que o traz junto de `VerticalRevealType`, os
//!   dois enums de que o `viewModel.ts` precisa); é reexportado aqui para o caminho do original continuar valendo.
//! - As interfaces `ICursorPositionChangedEvent` e `ICursorSelectionChangedEvent` são structs com os mesmos
//!   campos. `string` de `source` é `String`, `number` do `modelVersionId` é `i64` (como no `CursorStateChangedEvent`
//!   de `view_model_event_dispatcher.rs`) e `Selection[] | null` de `oldSelections` é `Option<Vec<Selection>>`.

pub use crate::view_model_types::CursorChangeReason;

use crate::position::Position;
use crate::selection::Selection;

/// Um evento que descreve que a posição do cursor mudou.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICursorPositionChangedEvent {
    /// A posição do cursor primário.
    pub position: Position,
    /// As posições dos cursores secundários.
    pub secondary_positions: Vec<Position>,
    /// O motivo.
    pub reason: CursorChangeReason,
    /// A origem da chamada que causou o evento. As origens possíveis incluem `"mouse"`, `"keyboard"` e `"api"`.
    pub source: String,
}

/// Um evento que descreve que a seleção do cursor mudou.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICursorSelectionChangedEvent {
    /// A seleção primária.
    pub selection: Selection,
    /// As seleções secundárias.
    pub secondary_selections: Vec<Selection>,
    /// O id da versão do modelo.
    pub model_version_id: i64,
    /// As seleções antigas.
    pub old_selections: Option<Vec<Selection>>,
    /// O id da versão do modelo a que `old_selections` se refere.
    pub old_model_version_id: i64,
    /// A origem da chamada que causou o evento. As origens possíveis incluem `"mouse"`, `"keyboard"` e `"api"`.
    pub source: String,
    /// O motivo.
    pub reason: CursorChangeReason,
}
