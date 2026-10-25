//! Auxiliares compartilhados pelos testes do `ActionList` (`actionList.test.ts`) e do `TabbedActionListWidget`
//! (`tabbedActionListWidget.test.ts`): o item de teste (`ITestActionItem`), `action(id)`, o serviço de layout com a
//! altura da janela ajustável e a altura da janela dos casos de altura.

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_platform::action_widget::action_list::{ActionListItemData, IActionListItem};
use je_platform::contextview::view_element::ViewElement;
use je_platform::layout::layout_service::{ILayoutService, LayoutContainerEvent, Viewport};

/// `ITestActionItem`: `{ id, checked? }`.
#[derive(Clone)]
pub struct Item {
    pub id: String,
    pub checked: bool,
}

impl ActionListItemData for Item {
    fn list_item_id(&self) -> Option<String> {
        Some(self.id.clone())
    }

    fn list_item_checked(&self) -> bool {
        self.checked
    }
}

pub type TestItem = IActionListItem<Item>;

/// `action(id)`.
pub fn action(id: &str) -> TestItem {
    IActionListItem::action(id, Item { id: id.to_owned(), checked: false })
}

pub fn rc(item: TestItem) -> Rc<TestItem> {
    Rc::new(item)
}

pub fn rcs(items: Vec<TestItem>) -> Vec<Rc<TestItem>> {
    items.into_iter().map(Rc::new).collect()
}

thread_local! {
    /// `window.innerHeight` da janela dos casos de altura (o `Viewport` do serviço de layout).
    static WINDOW_INNER_HEIGHT: Cell<f64> = const { Cell::new(800.0) };
}

/// `window.innerHeight` atual.
pub fn window_inner_height() -> f64 {
    WINDOW_INNER_HEIGHT.with(Cell::get)
}

/// `withWindowInnerHeight(height, callback)`: o valor anterior volta ao fim, mesmo se o `callback` falhar.
pub fn with_window_inner_height<R>(height: f64, callback: impl FnOnce() -> R) -> R {
    struct Restore(f64);
    impl Drop for Restore {
        fn drop(&mut self) {
            WINDOW_INNER_HEIGHT.with(|inner_height| inner_height.set(self.0));
        }
    }
    let _restore = Restore(WINDOW_INNER_HEIGHT.with(|inner_height| inner_height.replace(height)));
    callback()
}

/// `instantiationService.stub(ILayoutService, { getContainer, mainContainer, activeContainer, ... })`.
pub struct StubLayoutService {
    pub container: Rc<ViewElement>,
    pub layout: Rc<Emitter<LayoutContainerEvent>>,
}

impl ILayoutService for StubLayoutService {
    fn main_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }

    fn active_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }

    fn get_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }

    fn on_did_layout_container(&self) -> Event<LayoutContainerEvent> {
        self.layout.event()
    }

    fn window_viewport(&self) -> Viewport {
        Viewport { inner_width: 1280.0, inner_height: window_inner_height(), page_x_offset: 0.0, page_y_offset: 0.0 }
    }
}
