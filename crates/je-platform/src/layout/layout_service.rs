//! `ILayoutService` (recorte) e o `Viewport` da janela.

use std::rc::Rc;

use je_base::event::Event;
use je_base::layout::ISize;
use je_base::lifecycle::DisposableStore;
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

/// `{ container: HTMLElement; disposables: DisposableStore }` do `onDidAddContainer`.
#[derive(Clone)]
pub struct LayoutAddContainerEvent {
    pub container: Rc<ViewElement>,
    pub disposables: Rc<DisposableStore>,
}

/// `ILayoutOffsetInfo`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutOffsetInfo {
    /// Generic top offset.
    pub top: f64,
    /// Quick pick specific top offset.
    pub quick_pick_top: f64,
}

/// `ILayoutService`.
pub trait ILayoutService {
    /// `mainContainer`.
    fn main_container(&self) -> Rc<ViewElement>;
    /// `activeContainer`.
    fn active_container(&self) -> Rc<ViewElement>;
    /// `activeContainerDimension`: o tamanho do contêiner ativo.
    fn active_container_dimension(&self) -> ISize {
        self.active_container().size()
    }
    /// `activeContainerOffset`: sem barra de título própria nem painéis flutuantes, nenhum deslocamento.
    fn active_container_offset(&self) -> LayoutOffsetInfo {
        LayoutOffsetInfo { top: 0.0, quick_pick_top: 0.0 }
    }
    /// `getContainer(window)`.
    fn get_container(&self) -> Rc<ViewElement>;
    /// `onDidLayoutContainer`.
    fn on_did_layout_container(&self) -> Event<LayoutContainerEvent>;
    /// As medidas do `window` do contêiner.
    fn window_viewport(&self) -> Viewport;
    /// `containers`: todos os contêineres (o principal e os das janelas auxiliares). Sem janelas auxiliares, só o principal.
    fn containers(&self) -> Vec<Rc<ViewElement>> {
        vec![self.main_container()]
    }
    /// `onDidAddContainer`: sem janelas auxiliares, nunca dispara.
    fn on_did_add_container(&self) -> Event<LayoutAddContainerEvent> {
        Event::none()
    }
}

service_id!(pub I_LAYOUT_SERVICE: dyn ILayoutService = "layoutService");
