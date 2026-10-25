//! Porte de `src/vs/workbench/browser/parts`: as partes do workbench (barra de título, barra de status...).

pub mod statusbar;

// O banner (`banner/bannerPart.ts`).
pub mod banner;

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

// A base das partes de composites (`compositePart.ts`).
pub mod composite_part;

// O rastreador de contêineres de visões visíveis (`visibleViewContainersTracker.ts`).
pub mod visible_view_containers_tracker;

// A base das partes de painéis (`paneCompositePart.ts`) e a vista nativa dela e da `CompositePart`.
pub mod pane_composite_part;
pub mod native_composite_part_view;

// A barra lateral, o painel e a barra auxiliar.
pub mod sidebar;
pub mod panel;
pub mod auxiliarybar;
