//! Porte de `src/vs/platform/contextview/browser/contextViewService.ts` sobre uma `ContextView` sem DOM
//! (`src/vs/base/browser/ui/contextview/contextview.ts`): o `ContextViewService`, que guarda o delegate ativo, renderiza o
//! conteúdo no elemento da visão, posiciona com `layout2d` e esconde com ou sem a animação de fechamento.
//!
//! Divergências:
//! - **`ContextViewDOMPosition` e raiz de sombra.** A visão nativa não tem posição de DOM (`absolute`, `fixed`, `fixed-shadow`),
//!   então o `container` só dá o pai da visão (e as classes que a animação exige dos ancestrais); o `shadowRoot` não tem
//!   efeito, e `useWindowContainerForShadowRoot` escolhe o contêiner da janela como esse pai
//!   ([`ContextViewService::show_context_view_in`]).
//! - **`prepareMenuCloseAnimation`.** Copia `opacity` e `transform` calculados do `.monaco-scrollable-element` para variáveis
//!   de CSS; o pintor nativo anima o menu sem passar por variáveis, então o passo não existe.
//! - **Medida da visão.** `DOM.getTotalWidth/Height(view)` é o [`ViewElement::size`] da visão, que o conteúdo renderizado
//!   define com [`ViewElement::set_measure`].
//! - **Eventos de DOM do contêiner** (`onDOMEvent`, clique de captura que esconde a visão): ver [`super::context_view`].

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::disposable_timeout;
use je_base::layout::{AnchorAlignment, AnchorPosition, ILayout2DOptions, IRect, layout2d};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_instantiation::{Injectable, service_impl};

use super::context_view::{
    CONTEXT_VIEW_CLOSE_ANIMATION_DURATION_VARIABLE, HideData, IContextViewDelegate, IContextViewService, get_anchor_rect, has_required_ancestor_classes,
};
use super::view_element::ViewElement;
use crate::layout::layout_service::ILayoutService;

/// A posição que o `layout2d` resolveu para a visão.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextViewPlacement {
    pub top: f64,
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub anchor_position: AnchorPosition,
    pub anchor_alignment: AnchorAlignment,
}

/// `hidingContextView`.
struct HidingContextView {
    timer: DisposableRef,
    to_dispose: Option<DisposableRef>,
    class_name: String,
}

#[derive(Default)]
struct ContextViewState {
    delegate: Option<Rc<dyn IContextViewDelegate>>,
    to_dispose_on_clean: Option<DisposableRef>,
    hiding: Option<HidingContextView>,
    placement: Option<ContextViewPlacement>,
}

/// `ContextViewService`.
pub struct ContextViewService {
    this: Weak<ContextViewService>,
    layout_service: Rc<dyn ILayoutService>,
    view: Rc<ViewElement>,
    state: RefCell<ContextViewState>,
    disposables: Rc<DisposableStore>,
}

impl Injectable for ContextViewService {
    type Args = ();
    type Deps = (Rc<dyn ILayoutService>,);

    fn create(_: (), (layout_service,): Self::Deps) -> Rc<Self> {
        let view = ViewElement::with_classes(&["context-view", "monaco-component"]);
        view.set_parent(Some(layout_service.main_container()));
        view.hide();
        let disposables = DisposableStore::new();
        let service = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            layout_service: layout_service.clone(),
            view,
            state: RefCell::new(ContextViewState::default()),
            disposables: disposables.clone(),
        });
        service.layout();
        let weak = Rc::downgrade(&service);
        disposables.add(layout_service.on_did_layout_container().listen(move |_| {
            if let Some(service) = weak.upgrade() {
                service.layout();
            }
        }));
        service
    }
}

service_impl!(ContextViewService => dyn IContextViewService, disposable);

impl IDisposable for ContextViewService {
    fn dispose(&self) {
        self.hide(None, true);
        self.complete_hide_animation();
        self.disposables.dispose();
    }
}

impl ContextViewService {
    /// `showContextView(delegate, container, shadowRoot, useWindowContainerForShadowRoot)`: com um `container` e
    /// `useWindowContainerForShadowRoot`, a visão monta no contêiner da janela (`getContainer(getWindow(container))`) e não no
    /// `container`; sem `container`, monta no contêiner ativo. O `shadowRoot` só escolhe a posição de DOM (`fixed-shadow` ou
    /// `fixed`), que a visão nativa não tem.
    pub fn show_context_view_in(&self, delegate: Rc<dyn IContextViewDelegate>, container: Option<Rc<ViewElement>>, _shadow_root: bool, use_window_container_for_shadow_root: bool) {
        let context_view_container = match container {
            Some(_) if use_window_container_for_shadow_root => self.layout_service.get_container(),
            Some(container) => container,
            None => self.layout_service.active_container(),
        };
        self.view.set_parent(Some(context_view_container));

        self.show(delegate);
    }

    /// A posição resolvida na última vez que a visão foi posicionada.
    pub fn last_placement(&self) -> Option<ContextViewPlacement> {
        self.state.borrow().placement
    }

    fn visible_delegate(&self) -> Option<Rc<dyn IContextViewDelegate>> {
        self.state.borrow().delegate.clone()
    }

    fn show(&self, delegate: Rc<dyn IContextViewDelegate>) {
        self.complete_hide_animation();

        if self.visible_delegate().is_some() {
            self.hide(None, true);
        }

        // Show static box
        self.view.set_class_name("context-view monaco-component");
        self.view.style_set_property("top", "0px");
        self.view.style_set_property("left", "0px");
        self.view.style_set_property("z-index", &format!("{}", 2575 + delegate.layer().unwrap_or(0)));
        self.view.style_set_property("position", "absolute");
        self.view.show();

        // Render content
        let to_dispose = delegate.render(&self.view);

        // Set active delegate
        {
            let mut state = self.state.borrow_mut();
            state.to_dispose_on_clean = to_dispose;
            state.delegate = Some(delegate.clone());
        }

        self.do_layout();

        delegate.focus();
    }

    fn do_layout(&self) {
        // Check that we still have a delegate - the delegate's layout may have hidden
        let Some(delegate) = self.visible_delegate() else {
            return;
        };

        // Get anchor
        let anchor = get_anchor_rect(&delegate.get_anchor());
        let window = self.layout_service.window_viewport();
        let viewport = IRect { top: window.page_y_offset, left: window.page_x_offset, width: window.inner_width, height: window.inner_height };
        self.view.style_set_property("top", "0px");
        self.view.style_set_property("left", "0px");
        let view = self.view.size();
        let layout_result = layout2d(
            viewport,
            view,
            anchor,
            Some(ILayout2DOptions {
                anchor_alignment: delegate.anchor_alignment(),
                anchor_position: delegate.anchor_position(),
                anchor_axis_alignment: delegate.anchor_axis_alignment(),
            }),
        );

        for class_name in ["top", "bottom", "left", "right"] {
            self.view.class_list_remove(class_name);
        }
        self.view.class_list_add(if layout_result.anchor_position == AnchorPosition::Below { "bottom" } else { "top" });
        self.view.class_list_add(if layout_result.anchor_alignment == AnchorAlignment::Left { "left" } else { "right" });

        self.view.style_set_property("top", &format!("{}px", layout_result.top));
        self.view.style_set_property("left", &format!("{}px", layout_result.left));
        self.view.style_set_property("width", "initial");
        self.view.set_position(layout_result.top, layout_result.left);
        self.state.borrow_mut().placement = Some(ContextViewPlacement {
            top: layout_result.top,
            left: layout_result.left,
            bottom: layout_result.bottom,
            right: layout_result.right,
            anchor_position: layout_result.anchor_position,
            anchor_alignment: layout_result.anchor_alignment,
        });
    }

    fn hide(&self, data: Option<HideData>, skip_animation: bool) {
        if self.state.borrow().hiding.is_some() {
            if skip_animation {
                self.complete_hide_animation();
            }
            return;
        }

        let (delegate, to_dispose) = {
            let mut state = self.state.borrow_mut();
            (state.delegate.take(), state.to_dispose_on_clean.take())
        };
        let Some(delegate) = delegate else {
            return;
        };

        delegate.on_hide(data);

        if let Some(close_animation) = delegate.close_animation() {
            if !skip_animation && close_animation.duration > 0.0 && has_required_ancestor_classes(&self.view, &close_animation.required_ancestor_classes) {
                self.view.style_set_property(CONTEXT_VIEW_CLOSE_ANIMATION_DURATION_VARIABLE, &format!("{}ms", close_animation.duration));
                self.view.class_list_add(&close_animation.class_name);
                let weak = self.this.clone();
                let timer = disposable_timeout(
                    move || {
                        if let Some(service) = weak.upgrade() {
                            service.complete_hide_animation();
                        }
                    },
                    close_animation.duration as u64,
                    None,
                );
                self.state.borrow_mut().hiding = Some(HidingContextView { timer, to_dispose, class_name: close_animation.class_name });
                return;
            }
        }

        if let Some(to_dispose) = to_dispose {
            to_dispose.dispose();
        }
        self.view.hide();
    }

    fn complete_hide_animation(&self) {
        let hiding = self.state.borrow_mut().hiding.take();
        let Some(hiding) = hiding else {
            return;
        };

        hiding.timer.dispose();
        self.view.class_list_remove(&hiding.class_name);
        self.view.style_remove_property(CONTEXT_VIEW_CLOSE_ANIMATION_DURATION_VARIABLE);
        if let Some(to_dispose) = hiding.to_dispose {
            to_dispose.dispose();
        }
        self.view.hide();
    }
}

impl IContextViewService for ContextViewService {
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, container: Option<Rc<ViewElement>>, shadow_root: bool) {
        self.show_context_view_in(delegate, container, shadow_root, false);
    }

    fn hide_context_view(&self, data: Option<HideData>) {
        self.hide(data, false);
    }

    fn layout(&self) {
        let Some(delegate) = self.visible_delegate() else {
            return;
        };

        if !delegate.can_relayout() {
            self.hide(None, false);
            return;
        }

        delegate.layout();

        self.do_layout();
    }

    fn get_context_view_element(&self) -> Rc<ViewElement> {
        self.view.clone()
    }
}
