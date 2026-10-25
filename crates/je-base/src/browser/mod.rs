//! Porte de `src/vs/base/browser`: a parte que não depende do DOM do navegador.

pub mod animation_sync;
#[allow(clippy::module_inception)] // `base/browser/browser.ts`
pub mod browser;
pub mod css_value;
pub mod dom;
pub mod dom_sanitize;
pub mod formatted_text_renderer;
pub mod html_entities;
pub mod html_parser;
pub mod html_to_markdown;
pub mod keyboard_event;
