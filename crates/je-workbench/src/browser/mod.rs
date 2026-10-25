//! Porte de `src/vs/workbench/browser`: o shell do workbench. A UI é desenhada nativamente (sem DOM), então
//! aqui ficam os modelos de dados e a lógica pura das partes e das ações; o que no original é `HTMLElement`
//! vira um identificador opaco ou um trait implementado por quem desenha.

pub mod actions;
pub mod pane_composite;
pub mod labels;
pub mod parts;
pub mod quickaccess;
pub mod resource_navigator;
