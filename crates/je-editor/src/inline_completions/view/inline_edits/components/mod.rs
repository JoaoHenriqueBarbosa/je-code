//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/components/`: as peças de interface do
//! indicador da margem.
//!
//! Esta pasta traz `gutterIndicatorView` ([`gutter_indicator_view`]) e `gutterIndicatorMenu` ([`gutter_indicator_menu`]).
//! O menu se liga ao indicador pelo contrato [`gutter_indicator_view::IGutterIndicatorHost`], que o
//! [`gutter_indicator_menu::GutterIndicatorMenuHost`] implementa.

pub mod gutter_indicator_menu;
pub mod gutter_indicator_view;
