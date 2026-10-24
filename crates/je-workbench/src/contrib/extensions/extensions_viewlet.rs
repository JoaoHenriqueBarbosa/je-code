//! Porte parcial de `src/vs/workbench/contrib/extensions/browser/extensionsViewlet.ts` e de
//! `browser/media/extensionsViewlet.css`: o que o teste `extensionsViewlet.test.ts` exercita, o contorno de foco do botão de
//! ação da notificação do cabeçalho.
//!
//! Recortes:
//! - **Sem DOM.** O `ExtensionsViewPaneContainer` (cabeçalho, caixa de busca, lista, `notification-container`) é a
//!   renderização da Fase 6 e não tem equivalente aqui. O CSS do VS Code é especificação, nunca código executado: a regra
//!   `.extensions-viewlet .notification-container .message-action-button:focus-visible` (`outline:
//!   var(--vscode-strokeThickness) solid var(--vscode-focusBorder); outline-offset: 2px`) vira o [`FocusOutline`] que o
//!   widget nativo vai pintar, com a espessura e a cor vindas do tema.
//! - **Consultas e filtros** (`ExtensionsListView`, `DefaultViewsContext`, `SearchExtensionsContext`...) dependem do
//!   `ViewPaneContainer` e do serviço de visões; não foram portados nesta fatia.

/// `outline-offset` da regra de foco do botão de ação, em pixels.
pub const MESSAGE_ACTION_BUTTON_FOCUS_OUTLINE_OFFSET: f64 = 2.0;

/// O contorno que `:focus-visible` desenha no `.message-action-button`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusOutline {
    /// `outline-style`: sempre `solid`.
    pub style: &'static str,
    /// `outline-width`, em pixels (`--vscode-strokeThickness`).
    pub width: f64,
    /// `outline-color` como RGB (`--vscode-focusBorder`).
    pub color: (u8, u8, u8),
    /// `outline-offset`, em pixels.
    pub offset: f64,
}

/// A regra `.message-action-button:focus-visible`: `outline: var(--vscode-strokeThickness) solid var(--vscode-focusBorder)`
/// com `outline-offset: 2px`. A regra não depende do tema (`vs`, `vs-dark`, `hc-black`, `hc-light`): só os valores das
/// variáveis mudam.
pub fn message_action_button_focus_outline(stroke_thickness: f64, focus_border: (u8, u8, u8)) -> FocusOutline {
    FocusOutline { style: "solid", width: stroke_thickness, color: focus_border, offset: MESSAGE_ACTION_BUTTON_FOCUS_OUTLINE_OFFSET }
}
