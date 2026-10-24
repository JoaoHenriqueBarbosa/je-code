//! Porte de `src/vs/workbench/services/views`: o modelo de contêiner de views (`common/viewContainerModel.ts`) em
//! [`view_container_model`] e o serviço de descritores de views (`browser/viewDescriptorService.ts`) em
//! [`view_descriptor_service`]. Os registros e os descritores (`common/views.ts`) estão em `crate::common::views`.

pub mod view_container_model;
pub mod view_descriptor_service;
