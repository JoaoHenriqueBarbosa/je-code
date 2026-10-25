//! Porte de `src/vs/editor/standalone`: o editor standalone (o Monaco do VS Code).
//!
//! - [`standalone_theme`]: `standalone/common/standaloneTheme.ts`, os contratos do tema do editor standalone.
//! - [`standalone_services`]: `standalone/browser/standaloneServices.ts`, os serviços que o editor standalone traz no lugar
//!   dos do workbench (configuração, notificação, comandos e atalhos).
//! - [`standalone_languages`]: `standalone/browser/standaloneLanguages.ts`, a pontuação de seletores e os adaptadores que
//!   transformam um provedor de tokens escrito por fora num suporte de tokenização.

pub mod standalone_languages;
pub mod standalone_services;
pub mod standalone_theme;
