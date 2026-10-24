//! Porte de `src/vs/workbench/services/extensionManagement`: um módulo por arquivo da árvore do original.
//!
//! `global_extension_enablement_service` é o `platform/extensionManagement/common/extensionEnablementService.ts` (o
//! `GlobalExtensionEnablementService` e o `StorageManager` que o gerenciador de habilitação usa) e `service_contracts` os
//! recortes dos contratos de serviço de que ele depende e que ainda não foram portados.

pub mod extension_bisect;
pub mod extension_enablement_service;
pub mod extension_features;
pub mod extension_gallery_account;
pub mod extension_gallery_account_service;
pub mod extension_gallery_contracts;
pub mod extension_gallery_manifest_service;
pub mod extension_gallery_manifest_service_browser;
pub mod extension_gallery_service;
pub mod extension_install_service;
pub mod extension_install_service_impl;
pub mod extension_management;
pub mod global_extension_enablement_service;
pub mod service_contracts;
