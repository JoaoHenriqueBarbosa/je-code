//! Porte de `src/vs/workbench/browser/parts`: as partes do workbench (barra de título, barra de status...).

pub mod statusbar;

// A janela alvo, o tema e a decisão de inatividade que as partes compartilham.
pub mod part_support;

pub mod editor;

pub mod notifications;

pub mod views;

pub mod pane_composite_bar;

// As ações da barra de composites (`compositeBarActions.ts`).
pub mod composite_bar_actions;

// A barra global de atividades (`globalCompositeBar.ts`).
pub mod global_composite_bar;

pub mod activitybar;

// A barra de título e o menu compacto (`titlebar`).
pub mod titlebar;
