//! Porte de `src/vs/platform/extensionManagement/common`: o manifesto da galeria de extensões
//! (`extensionGalleryManifest.ts`) e o serviço base que o monta a partir do produto
//! (`extensionGalleryManifestService.ts`).
//!
//! O resto da pasta (`extensionManagement.ts`, o serviço de galeria, os tipos de extensão) vive em `je-extensions`, que
//! depende de `je-platform` e por isso não pode ser importado daqui.

pub mod extension_gallery_manifest;
pub mod extension_gallery_manifest_service;
