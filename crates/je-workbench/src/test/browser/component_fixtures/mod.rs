//! Porte de `src/vs/workbench/test/browser/componentFixtures`: a infraestrutura que monta componentes isolados com tema e
//! foco (CSS do tema, foco simulado, realce de sintaxe), usada pelos fixtures e pelos testes dela.
//!
//! O DOM e o `getComputedStyle` do original são a árvore nativa da `Ui` e o resolvedor de estilo de `fixture_css` (ver as
//! divergências lá): o CSS dos fixtures é especificação interpretada só nos testes, nunca no caminho de pintura.

pub mod fixture_css;
pub mod fixture_editor_styles;
pub mod fixture_focus;
pub mod fixture_resource_loader;
pub mod fixture_syntax_highlighting;
pub mod fixture_utils_css;
