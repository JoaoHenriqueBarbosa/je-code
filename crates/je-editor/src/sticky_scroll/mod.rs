//! Porte do que o sticky scroll do editor (`src/vs/editor/contrib/stickyScroll/browser`) usa para calcular as linhas
//! fixas no topo do editor: o modelo de escopos do documento e o provedor dos candidatos a linha fixa.
//!
//! Esta é a fatia 1. Os módulos filhos são `stickyScrollElement.ts` ([`sticky_scroll_element`]),
//! `stickyScrollModelProvider.ts` ([`sticky_scroll_model_provider`], que escolhe entre o outline, as dobras de sintaxe
//! e a indentação, reaproveitando [`crate::outline_model`], [`crate::folding::syntax_range_provider`] e
//! [`crate::folding::indent_range_provider`]) e `stickyScrollProvider.ts` ([`sticky_scroll_provider`]). As opções
//! (`editor.stickyScroll.*`) estão em [`crate::config::editor_options::sticky_scroll`].
//!
//! Fatia 2 (pendente):
//! - `stickyScrollWidget.ts` (o widget de overlay que desenha as linhas fixas e seus números de linha);
//! - `stickyScrollController.ts` (o `StickyScrollController`, contribuição do editor que escuta rolagem, layout,
//!   cursor, mouse e foco, calcula as linhas pela [`sticky_scroll_provider::IStickyLineCandidateProvider`] e as passa
//!   ao widget);
//! - as ações de `stickyScrollActions.ts` e o registro de `stickyScrollContribution.ts`;
//! - os quatro casos de `src/vs/editor/contrib/stickyScroll/test/browser/stickyScroll.test.ts`.

pub mod sticky_scroll_element;
pub mod sticky_scroll_model_provider;
pub mod sticky_scroll_provider;
