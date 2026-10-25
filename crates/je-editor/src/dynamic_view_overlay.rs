//! Porte de `src/vs/editor/browser/view/dynamicViewOverlay.ts`: `DynamicViewOverlay`, a base das partes da visão
//! que pintam algo por linha a cada `prepareRender` e devolvem o resultado em `render`.
//!
//! Divergências:
//! - `abstract class DynamicViewOverlay extends ViewEventHandler` vira um trait com o [`ViewEventHandler`] como
//!   supertrait. É genérico no modelo de visão `M` porque o `RenderingContext` é (ver `rendering_context.rs`).
//! - `render` devolve `string` (o HTML da linha) no original. A UI é nativa, sem DOM: o resultado de cada parte é o
//!   tipo associado `Output`, as primitivas de pintura da linha (cada parte define o seu).

use crate::rendering_context::RenderingContext;
use crate::view_events::ViewEventHandler;
use crate::view_model_types::IViewModel;

/// `DynamicViewOverlay`.
pub trait DynamicViewOverlay<M: IViewModel>: ViewEventHandler {
    /// O que `render` devolve para uma linha (o `string` do original).
    type Output;

    fn prepare_render(&self, ctx: &RenderingContext<M>);

    fn render(&self, start_line_number: i32, line_number: i32) -> Self::Output;
}
