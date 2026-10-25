//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/components/`: as peças de interface do
//! indicador da margem.
//!
//! Esta fatia (4c-3, parte 1) traz `gutterIndicatorView` ([`gutter_indicator_view`]). O `gutterIndicatorMenu` (o conteúdo
//! do menu que o indicador abre) é a fatia seguinte e se liga ao contrato [`gutter_indicator_view::IGutterIndicatorHost`].

pub mod gutter_indicator_view;
