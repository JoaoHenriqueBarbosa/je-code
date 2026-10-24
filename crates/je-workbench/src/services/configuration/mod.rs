//! Porte de `src/vs/workbench/services/configuration`: a camada do workbench sobre o serviço de configuração da
//! plataforma (configuração por workspace e por pasta, edição, políticas e modo remoto).
//!
//! `common/` e `browser/` seguem a árvore do original; os testes de `configurationService.test.ts` ficam em
//! `tests/configuration_service.rs`.

pub mod browser;
pub mod common;
