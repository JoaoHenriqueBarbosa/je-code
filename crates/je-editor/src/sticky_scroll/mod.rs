//! Porte do que o sticky scroll do editor (`src/vs/editor/contrib/stickyScroll/browser`) faz: o modelo de escopos do
//! documento, o provedor dos candidatos a linha fixa, o widget de overlay que desenha as linhas fixas, o controlador que as
//! calcula e as ações.
//!
//! Os módulos filhos são `stickyScrollElement.ts` ([`sticky_scroll_element`]), `stickyScrollModelProvider.ts`
//! ([`sticky_scroll_model_provider`], que escolhe entre o outline, as dobras de sintaxe e a indentação, reaproveitando
//! [`crate::outline_model`], [`crate::folding::syntax_range_provider`] e [`crate::folding::indent_range_provider`]),
//! `stickyScrollProvider.ts` ([`sticky_scroll_provider`]), `stickyScrollWidget.ts` ([`sticky_scroll_widget`], o widget de
//! overlay com as linhas fixas e os números de linha, em que o DOM virou dado que o pintor lê),
//! `stickyScrollController.ts` ([`sticky_scroll_controller`], o `StickyScrollController`: contribuição do editor que escuta
//! rolagem, layout, tokens, cursor, mouse e foco, calcula as linhas pela
//! [`sticky_scroll_provider::IStickyLineCandidateProvider`] e as passa ao widget), `stickyScrollActions.ts`
//! ([`sticky_scroll_actions`]) e `stickyScrollContribution.ts` ([`sticky_scroll_contribution`]). As opções
//! (`editor.stickyScroll.*`) estão em [`crate::config::editor_options::sticky_scroll`]. Os quatro casos de
//! `src/vs/editor/contrib/stickyScroll/test/browser/stickyScroll.test.ts` estão em `tests/sticky_scroll.rs`.

pub mod sticky_scroll_actions;
pub mod sticky_scroll_contribution;
pub mod sticky_scroll_controller;
pub mod sticky_scroll_element;
pub mod sticky_scroll_model_provider;
pub mod sticky_scroll_provider;
pub mod sticky_scroll_widget;
