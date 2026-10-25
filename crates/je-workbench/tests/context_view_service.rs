//! Porte de `src/vs/workbench/test/browser/contextViewService.test.ts` (suíte `ContextViewService`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - **DOM.** `$('.window-container')` e `$('.local-container')` são `ViewElement`s encadeados por `set_parent`, e
//!   `document.body.appendChild(windowContainer)` não tem o que anexar (a visão nativa não precisa de um documento).
//! - **`TestWindowLayoutService`.** `mainContainer`, `activeContainer`, `containers` e `getContainer()` devolvem o contêiner da
//!   janela (as sobrescritas do original), sobre o `ILayoutService` de `je-platform`; as medidas da janela e o evento de
//!   layout são os de um serviço de teste sem janelas auxiliares.
//! - **Raiz de sombra.** A visão nativa não tem raiz de sombra (`getRootNode()` é o próprio elemento da visão e
//!   `host.parentElement` é o pai dele), então `isShadowRoot(...)` vira a conferência de que o elemento da visão está montado
//!   (`parent_element()` existe), e `host.parentElement === windowContainer` é `parent_element()` ser o contêiner.
//! - **`showContextView(delegate, container, shadowRoot, useWindowContainerForShadowRoot)`.** É
//!   `ContextViewService::show_context_view_in`, que carrega o quarto argumento que o contrato `IContextViewService` do porte
//!   não tem.
//! - **`Disposable.None`.** É `none()`, o descarte vazio que o `render` devolve.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::event::{Emitter, Event};
use je_base::layout::IAnchor;
use je_base::lifecycle::{DisposableRef, none};
use je_instantiation::Injectable;
use je_platform::contextview::context_view::{AnchorTarget, IContextViewDelegate, IContextViewService};
use je_platform::contextview::context_view_service::ContextViewService;
use je_platform::contextview::view_element::ViewElement;
use je_platform::layout::layout_service::{ILayoutService, LayoutContainerEvent, Viewport};

/// `TestWindowLayoutService`: o contêiner da janela é o principal, o ativo e o da janela.
struct TestWindowLayoutService {
    window_container: Rc<ViewElement>,
    layout: Rc<Emitter<LayoutContainerEvent>>,
}

impl ILayoutService for TestWindowLayoutService {
    fn main_container(&self) -> Rc<ViewElement> {
        self.window_container.clone()
    }

    fn active_container(&self) -> Rc<ViewElement> {
        self.window_container.clone()
    }

    fn get_container(&self) -> Rc<ViewElement> {
        self.window_container.clone()
    }

    fn containers(&self) -> Vec<Rc<ViewElement>> {
        vec![self.window_container.clone()]
    }

    fn on_did_layout_container(&self) -> Event<LayoutContainerEvent> {
        self.layout.event()
    }

    fn window_viewport(&self) -> Viewport {
        Viewport { inner_width: 1280.0, inner_height: 800.0, page_x_offset: 0.0, page_y_offset: 0.0 }
    }
}

/// `{ getAnchor: () => ({ x: 0, y: 0 }), render: () => Disposable.None }`.
struct TestDelegate;

impl IContextViewDelegate for TestDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        AnchorTarget::Anchor(IAnchor { x: 0.0, y: 0.0, width: None, height: None })
    }

    fn render(&self, _container: &Rc<ViewElement>) -> Option<DisposableRef> {
        Some(none())
    }
}

#[test]
fn mounts_requested_shadow_roots_in_the_window_container() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let window_container = ViewElement::with_classes(&["window-container"]);
    let local_container = ViewElement::with_classes(&["local-container"]);
    local_container.set_parent(Some(window_container.clone()));

    let layout = Emitter::new();
    guard.add_rc(layout.clone());
    let service = guard.add_rc(ContextViewService::create((), (Rc::new(TestWindowLayoutService { window_container: window_container.clone(), layout }) as Rc<dyn ILayoutService>,)));
    let delegate: Rc<dyn IContextViewDelegate> = Rc::new(TestDelegate);

    service.show_context_view_in(delegate.clone(), Some(local_container.clone()), true, true);
    let window_mounted_host = service.get_context_view_element().parent_element();
    assert!(window_mounted_host.is_some());
    let window_mounted_at_root = window_mounted_host.is_some_and(|host| Rc::ptr_eq(&host, &window_container));

    service.hide_context_view(None);
    service.show_context_view(delegate, Some(local_container.clone()), true);
    let locally_mounted_host = service.get_context_view_element().parent_element();
    assert!(locally_mounted_host.is_some());

    assert_eq!(
        (window_mounted_at_root, locally_mounted_host.is_some_and(|host| Rc::ptr_eq(&host, &local_container))),
        (true, true)
    );
}
