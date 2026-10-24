//! Porte de `src/vs/platform/encryption/common`: o contrato de cifra (`encryptionService.ts`). A implementação real
//! (`safeStorage` do Electron, chaveiro do sistema) não faz parte do porte: ver [`encryption_service`].

pub mod encryption_service;

pub use encryption_service::*;
