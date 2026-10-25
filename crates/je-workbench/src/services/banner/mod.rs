//! Porte de `src/vs/workbench/services/banner`: o contrato do serviço de banner da janela (`browser/bannerService.ts`).

#[allow(clippy::module_inception)] // `banner_service.rs` é o porte de `bannerService.ts`, como no original
pub mod banner_service;
