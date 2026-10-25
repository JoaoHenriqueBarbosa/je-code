//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellWidgets.ts`: o alvo de um clique na barra de status da
//! célula (`IClickTarget` e `ClickTargetType`).
//!
//! Divergências:
//! - `event: MouseEvent` é o [`PointerData`] do clique (coordenadas da janela, botão, modificadores e contagem de cliques).
//! - `const enum ClickTargetType` é o enum [`ClickTargetType`] com os mesmos valores numéricos.

use je_ui::PointerData;

/// `ClickTargetType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickTargetType {
    Container = 0,
    ContributedTextItem = 1,
    ContributedCommandItem = 2,
}

/// `IClickTarget`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IClickTarget {
    pub type_: ClickTargetType,
    pub event: PointerData,
}
