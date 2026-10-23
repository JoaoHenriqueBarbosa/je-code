//! Porte de `src/vs/platform/webContentExtractor` do VS Code: a parte pura (dados e cache), sem Electron,
//! sem webview e sem rede.
//!
//! Portado até agora: os tipos de `common/webContentExtractor.ts` que o resto usa
//! ([`web_content_extractor`]), o cache de resultados ([`web_content_cache`]) e a conversão da árvore de
//! acessibilidade do CDP em Markdown ([`cdp_accessibility_domain`]). `webPageLoader.ts` e os serviços
//! dependem do Electron e ficam para outra fatia.

pub mod cdp_accessibility_domain;
pub mod web_content_cache;
#[allow(clippy::module_inception)] // `web_content_extractor.rs` é o porte de `webContentExtractor.ts`, como no original
pub mod web_content_extractor;
