//! Porte das estruturas de dados puras do pipeline de GPU do editor (`src/vs/editor/browser/gpu`): o
//! rastreador de regiões sujas, a coleção de objetos num buffer plano e os alocadores do atlas de
//! texturas (prateleiras e lajes).
//!
//! O je-code desenha por wgpu/vello, nunca por `GPUDevice`, `OffscreenCanvas` ou DOM, então ficam de fora o
//! que depende deles (observação de dimensões em pixels do dispositivo, rasterizador de glifos, o
//! `TextureAtlas` com as páginas de textura). O `OffscreenCanvas` que os alocadores recebem vira a
//! [`gpu_utils::PixelPage`]: uma página de pixels RGBA na CPU.

pub mod atlas;
pub mod buffer_dirty_tracker;
pub mod gpu_utils;
pub mod js_number_format;
pub mod object_collection_buffer;
pub mod raster;
pub mod texture_atlas_shelf_allocator;
pub mod texture_atlas_slab_allocator;
