//! Porte de `src/vs/workbench/browser`: o shell do workbench. A UI é desenhada nativamente (sem DOM), então
//! aqui ficam os modelos de dados e a lógica pura das partes; o que no original é `HTMLElement` vira um
//! identificador opaco ou um trait implementado por quem desenha.

pub mod parts;
