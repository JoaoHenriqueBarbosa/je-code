//! Porte de `src/vs/base/browser/ui/scrollbar/scrollbarVisibilityController.ts`: decide quando a barra
//! aparece e some, trocando a classe do nó (`visible scrollbar ...` / `invisible scrollbar ... fade`).
//!
//! A regra de CSS de `scrollbars.css` para as classes vira [`ScrollbarVisibilityController::apply`]:
//! `.visible` é opaco e fica na frente (`z-index: 11`); `.invisible` é transparente e não recebe ponteiro.
//! A transição de opacidade (100 ms ao aparecer, 800 ms ao sumir com `fade`) não é animada: o `je-ui` não
//! tem transições, então a troca é imediata. // TODO(integração): animar a opacidade quando o `je-ui` ganhar
//! um relógio de animação por nó.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::async_::TimeoutTimer;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::scrollable::ScrollbarVisibility;
use je_ui::{NodeId, PointerEvents, Ui};

pub struct ScrollbarVisibilityController {
    base: Rc<Disposable>,
    weak_self: Weak<ScrollbarVisibilityController>,
    ui: Ui,
    visibility: Cell<ScrollbarVisibility>,
    visible_class_name: String,
    invisible_class_name: String,
    dom_node: Cell<Option<NodeId>>,
    raw_should_be_visible: Cell<bool>,
    should_be_visible: Cell<bool>,
    is_needed: Cell<bool>,
    is_visible: Cell<bool>,
    reveal_timer: Rc<TimeoutTimer>,
}

impl ScrollbarVisibilityController {
    pub fn new(ui: &Ui, visibility: ScrollbarVisibility, visible_class_name: &str, invisible_class_name: &str) -> Rc<Self> {
        let base = Disposable::new();
        let reveal_timer = TimeoutTimer::new();
        base.register(reveal_timer.clone());
        Rc::new_cyclic(|weak| Self {
            base,
            weak_self: weak.clone(),
            ui: ui.clone(),
            visibility: Cell::new(visibility),
            visible_class_name: visible_class_name.to_owned(),
            invisible_class_name: invisible_class_name.to_owned(),
            dom_node: Cell::new(None),
            raw_should_be_visible: Cell::new(false),
            should_be_visible: Cell::new(false),
            is_needed: Cell::new(false),
            is_visible: Cell::new(false),
            reveal_timer,
        })
    }

    pub fn set_visibility(&self, visibility: ScrollbarVisibility) {
        if self.visibility.get() != visibility {
            self.visibility.set(visibility);
            self.update_should_be_visible();
        }
    }

    // ----------------- Hide / Reveal

    pub fn set_should_be_visible(&self, raw_should_be_visible: bool) {
        self.raw_should_be_visible.set(raw_should_be_visible);
        self.update_should_be_visible();
    }

    fn apply_visibility_setting(&self) -> bool {
        match self.visibility.get() {
            ScrollbarVisibility::Hidden => false,
            ScrollbarVisibility::Visible => true,
            ScrollbarVisibility::Auto => self.raw_should_be_visible.get(),
        }
    }

    fn update_should_be_visible(&self) {
        let should_be_visible = self.apply_visibility_setting();

        if self.should_be_visible.get() != should_be_visible {
            self.should_be_visible.set(should_be_visible);
            self.ensure_visibility();
        }
    }

    pub fn set_is_needed(&self, is_needed: bool) {
        if self.is_needed.get() != is_needed {
            self.is_needed.set(is_needed);
            self.ensure_visibility();
        }
    }

    pub fn set_dom_node(&self, dom_node: NodeId) {
        self.dom_node.set(Some(dom_node));
        self.set_class_name(&self.invisible_class_name);

        // Agora que as marcas e o nó estão consistentes, garante a configuração Hidden/Visible.
        self.set_should_be_visible(false);
    }

    pub fn ensure_visibility(&self) {
        if !self.is_needed.get() {
            // nada a desenhar
            self.hide(false);
            return;
        }

        if self.should_be_visible.get() {
            self.reveal();
        } else {
            self.hide(true);
        }
    }

    fn reveal(&self) {
        if self.is_visible.get() {
            return;
        }
        self.is_visible.set(true);

        // A animação do CSS não toca de outro jeito.
        let weak = self.weak_self.clone();
        self.reveal_timer.set_if_not_set(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.set_class_name(&this.visible_class_name);
                }
            },
            0,
        );
    }

    fn hide(&self, with_fade_away: bool) {
        self.reveal_timer.cancel();
        if !self.is_visible.get() {
            return;
        }
        self.is_visible.set(false);
        self.set_class_name(&format!("{}{}", self.invisible_class_name, if with_fade_away { " fade" } else { "" }));
    }

    /// `domNode.setClassName(name)` e a regra de CSS correspondente.
    fn set_class_name(&self, name: &str) {
        let Some(node) = self.dom_node.get() else { return };
        let mut tree = self.ui.tree_mut();
        tree.set_class_name(node, name);
        let visible = tree.has_class(node, "visible");
        let style = tree.style_mut(node);
        style.opacity = if visible { 1.0 } else { 0.0 };
        style.z_index = if visible { 11 } else { 0 };
        style.pointer_events = if visible { PointerEvents::Inherit } else { PointerEvents::None };
    }
}

impl IDisposable for ScrollbarVisibilityController {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ScrollbarVisibilityController"
    }
}
