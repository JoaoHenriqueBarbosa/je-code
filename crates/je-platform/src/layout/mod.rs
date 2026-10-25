//! Porte parcial de `src/vs/platform/layout/browser/layoutService.ts`: o recorte do `ILayoutService` que a visão de contexto
//! e o `ActionList` leem (os contêineres, o evento de layout e as medidas da janela).
//!
//! Divergência: `getContainer(window)` e `window.innerWidth/innerHeight/pageXOffset/pageYOffset` do original são o
//! [`ILayoutService::get_container`] e o [`ILayoutService::window_viewport`], porque não há objeto `window` nativo.

pub mod layout_service;
