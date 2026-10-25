//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/hintsWidget/`: o widget de dicas do inline completions
//! (a barra com anterior, contador e próximo, o aviso e as ações do menu) e o participante de hover que o mostra.
//!
//! O `inlineCompletionsHintsWidget.css` não é portado como arquivo: as medidas viram constantes do módulo do widget, que o
//! pintor nativo lê.

pub mod hover_participant;
pub mod inline_completions_hints_widget;
