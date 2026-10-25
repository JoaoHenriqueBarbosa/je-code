//! `ILayoutService` (recorte) e o `Viewport` da janela.

use std::rc::Rc;

use je_base::event::Event;
use je_base::layout::ISize;
use je_instantiation::service_id;

use crate::contextview::view_element::ViewElement;

/// `window.innerWidth`, `window.innerHeight`, `window.pageXOffset` e `window.pageYOffset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub inner_width: f64,
    pub inner_height: f64,
    pub page_x_offset: f64,
    pub page_y_offset: f64,
}

/// `{ container: HTMLElement; dimension: IDimension }` do `onDidLayoutContainer`.
#[derive(Clone)]
pub struct LayoutContainerEvent {
    pub container: Rc<ViewElement>,
    pub dimension: ISize,
}

/// `ILayoutService`.
pub trait ILayoutService {
    /// `mainContainer`.
    fn main_container(&self) -> Rc<ViewElement>;
    /// `activeContainer`.
    fn active_container(&self) -> Rc<ViewElement>;
    /// `getContainer(window)`.
    fn get_container(&self) -> Rc<ViewElement>;
    /// `onDidLayoutContainer`.
    fn on_did_layout_container(&self) -> Event<LayoutContainerEvent>;
    /// As medidas do `window` do contêiner.
    fn window_viewport(&self) -> Viewport;
}

service_id!(pub I_LAYOUT_SERVICE: dyn ILayoutService = "layoutService");
