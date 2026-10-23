//! Porte de `src/vs/base/browser/ui/scrollbar/abstractScrollbar.ts`, com as duas subclasses
//! (`horizontalScrollbar.ts` e `verticalScrollbar.ts`) fundidas num só tipo parametrizado por [`Axis`]
//! (a regra DRY do projeto: o que as subclasses mudam é só o eixo).
//!
//! Do DOM para o `je-ui`: o `FastDomNode` é um nó da árvore com `set_bounds`/`set_right`/`set_bottom`;
//! `pointerType` é sempre mouse (não há toque no `je-ui`); `offsetX/offsetY` saem da posição absoluta do
//! nó; o `GlobalPointerMoveMonitor` é [`GlobalPointerMoveMonitor`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::scrollable::{INewScrollPosition, ScrollEvent, Scrollable, ScrollbarVisibility};
use je_ui::{Event, EventKind, NodeId, PointerButton, PointerData, Role, StandardWheelEvent, Ui};

use crate::global_pointer_move_monitor::GlobalPointerMoveMonitor;
use crate::scrollbar::scrollable_element_options::ScrollableElementResolvedOptions;
use crate::scrollbar::scrollbar_arrow::{ARROW_IMG_SIZE, Edges, ScrollbarArrow, ScrollbarArrowOptions, place};
use crate::scrollbar::scrollbar_colors;
use crate::scrollbar::scrollbar_state::ScrollbarState;
use crate::scrollbar::visibility_controller::ScrollbarVisibilityController;

/// A distância ortogonal ao slider em que o arraste "reinicia" (o snap do Windows).
const POINTER_DRAG_RESET_DISTANCE: f64 = 140.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// O `ScrollbarHost`.
pub trait ScrollbarHost {
    fn on_mouse_wheel(&self, e: &StandardWheelEvent);
    fn on_drag_start(&self);
    fn on_drag_end(&self);
}

pub struct Scrollbar {
    base: Rc<Disposable>,
    weak_self: Weak<Scrollbar>,
    ui: Ui,
    axis: Axis,
    host: Rc<dyn ScrollbarHost>,
    scrollable: Rc<Scrollable>,
    scroll_by_page: Cell<bool>,
    lazy_render: bool,
    state: RefCell<ScrollbarState>,
    visibility_controller: Rc<ScrollbarVisibilityController>,
    pointer_move_monitor: Rc<GlobalPointerMoveMonitor>,
    /// O `domNode`.
    pub dom_node: NodeId,
    slider: Cell<Option<NodeId>>,
    should_render: Cell<bool>,
}

impl Scrollbar {
    pub fn new(ui: &Ui, axis: Axis, scrollable: Rc<Scrollable>, options: &ScrollableElementResolvedOptions, host: Rc<dyn ScrollbarHost>) -> Rc<Self> {
        let dims = scrollable.get_scroll_dimensions();
        let pos = scrollable.get_current_scroll_position();
        let hidden = |v: ScrollbarVisibility, size: f64| if v == ScrollbarVisibility::Hidden { 0.0 } else { size };
        let (state, visibility, extra) = match axis {
            Axis::Horizontal => (
                ScrollbarState::new(
                    if options.horizontal_has_arrows { options.arrow_size } else { 0.0 },
                    hidden(options.horizontal, options.horizontal_scrollbar_size),
                    hidden(options.vertical, options.vertical_scrollbar_size),
                    dims.width,
                    dims.scroll_width,
                    pos.scroll_left,
                ),
                options.horizontal,
                "horizontal",
            ),
            Axis::Vertical => (
                ScrollbarState::new(
                    if options.vertical_has_arrows { options.arrow_size } else { 0.0 },
                    hidden(options.vertical, options.vertical_scrollbar_size),
                    // dá prioridade à barra vertical e a deixa rolar até o fim
                    0.0,
                    dims.height,
                    dims.scroll_height,
                    pos.scroll_top,
                ),
                options.vertical,
                "vertical",
            ),
        };
        let base = Disposable::new();
        let controller = ScrollbarVisibilityController::new(ui, visibility, &format!("visible scrollbar {extra}"), &format!("invisible scrollbar {extra}"));
        base.register(controller.clone());
        controller.set_is_needed(state.is_needed());
        let monitor = GlobalPointerMoveMonitor::new(ui);
        base.register(monitor.clone());
        let dom_node = {
            let mut tree = ui.tree_mut();
            let node = tree.create_detached(Role::Generic);
            tree.style_mut(node).background = scrollbar_colors().background;
            node
        };
        controller.set_dom_node(dom_node);

        let this = Rc::new_cyclic(|weak| Self {
            base,
            weak_self: weak.clone(),
            ui: ui.clone(),
            axis,
            host,
            scrollable,
            scroll_by_page: Cell::new(options.scroll_by_page),
            lazy_render: options.lazy_render,
            state: RefCell::new(state),
            visibility_controller: controller,
            pointer_move_monitor: monitor,
            dom_node,
            slider: Cell::new(None),
            should_render: Cell::new(true),
        });

        let weak = this.weak_self.clone();
        this.base.register(ui.listen(dom_node, EventKind::PointerDown, false, move |e| {
            if let Some(this) = weak.upgrade() {
                this.dom_node_pointer_down(e);
            }
        }));

        match axis {
            Axis::Horizontal => this.build_horizontal(options),
            Axis::Vertical => this.build_vertical(options),
        }
        this
    }

    fn build_horizontal(&self, o: &ScrollableElementResolvedOptions) {
        if o.horizontal_has_arrows {
            let arrow_delta = (o.arrow_size - ARROW_IMG_SIZE) / 2.0;
            let scrollbar_delta = (o.horizontal_scrollbar_size - ARROW_IMG_SIZE) / 2.0;
            self.create_arrow("scrollbar-button-left", Edges { top: Some(scrollbar_delta), left: Some(arrow_delta), ..Default::default() }, (o.arrow_size, o.horizontal_scrollbar_size), (1.0, 0.0));
            self.create_arrow("scrollbar-button-right", Edges { top: Some(scrollbar_delta), right: Some(arrow_delta), ..Default::default() }, (o.arrow_size, o.horizontal_scrollbar_size), (-1.0, 0.0));
        }
        self.create_slider(((o.horizontal_scrollbar_size - o.horizontal_slider_size) / 2.0).floor(), 0.0, None, Some(o.horizontal_slider_size));
    }

    fn build_vertical(&self, o: &ScrollableElementResolvedOptions) {
        if o.vertical_has_arrows {
            let arrow_delta = (o.arrow_size - ARROW_IMG_SIZE) / 2.0;
            let scrollbar_delta = (o.vertical_scrollbar_size - ARROW_IMG_SIZE) / 2.0;
            self.create_arrow("scrollbar-button-up", Edges { top: Some(arrow_delta), left: Some(scrollbar_delta), ..Default::default() }, (o.vertical_scrollbar_size, o.arrow_size), (0.0, 1.0));
            self.create_arrow("scrollbar-button-down", Edges { bottom: Some(arrow_delta), left: Some(scrollbar_delta), ..Default::default() }, (o.vertical_scrollbar_size, o.arrow_size), (0.0, -1.0));
        }
        self.create_slider(0.0, ((o.vertical_scrollbar_size - o.vertical_slider_size) / 2.0).floor(), Some(o.vertical_slider_size), None);
    }

    /// `_createArrow`: cria a seta e a põe na barra (fundo e ícone). `bg` é `(bgWidth, bgHeight)` e `wheel` o
    /// `(deltaX, deltaY)` do `StandardWheelEvent` que a seta ativa.
    fn create_arrow(&self, icon: &'static str, edges: Edges, bg: (f64, f64), wheel: (f64, f64)) {
        let (bg_width, bg_height) = bg;
        let (wheel_x, wheel_y) = wheel;
        let Edges { top, left, bottom, right } = edges;
        let host = self.host.clone();
        let arrow = ScrollbarArrow::new(&self.ui, ScrollbarArrowOptions {
            on_activate: Rc::new(move || host.on_mouse_wheel(&StandardWheelEvent::synthetic(wheel_x, wheel_y))),
            class_name: "scra",
            icon,
            bg_width,
            bg_height,
            top,
            left,
            bottom,
            right,
        });
        {
            let mut tree = self.ui.tree_mut();
            tree.append_child(self.dom_node, arrow.bg_dom_node);
            tree.append_child(self.dom_node, arrow.dom_node);
        }
        self.base.register(arrow);
    }

    /// `_createSlider`: cria o slider, o põe na barra e liga os eventos.
    fn create_slider(&self, top: f64, left: f64, width: Option<f64>, height: Option<f64>) {
        let slider = {
            let mut tree = self.ui.tree_mut();
            let slider = tree.create(self.dom_node, Role::Generic);
            tree.add_class(slider, "slider");
            tree.style_mut(slider).background = Some(scrollbar_colors().slider_background);
            slider
        };
        place(&self.ui, slider, Edges { top: Some(top), left: Some(left), ..Default::default() }, width.unwrap_or(0.0), height.unwrap_or(0.0));
        self.slider.set(Some(slider));

        let weak = self.weak_self.clone();
        self.base.register(self.ui.listen(slider, EventKind::PointerDown, false, move |e| {
            if e.pointer.is_some_and(|p| p.button == PointerButton::Primary) {
                e.prevent_default();
                if let (Some(this), Some(p)) = (weak.upgrade(), e.pointer) {
                    this.slider_pointer_down(e.target, &p);
                }
            }
        }));
        // `onclick(slider, e => { if (e.leftButton) e.stopPropagation(); })`
        self.base.register(self.ui.listen(slider, EventKind::Click, false, |e| {
            if e.pointer.is_some_and(|p| p.button == PointerButton::Primary) {
                e.stop_propagation();
            }
        }));
        // `.slider:hover` e `.slider.active`
        for (kind, hover) in [(EventKind::PointerEnter, true), (EventKind::PointerLeave, false)] {
            let weak = self.weak_self.clone();
            self.base.register(self.ui.listen(slider, kind, false, move |_| {
                if let Some(this) = weak.upgrade() {
                    this.refresh_slider_style(Some(hover));
                }
            }));
        }
    }

    /// As três regras de CSS do slider: ativo, hover, normal (nessa precedência).
    fn refresh_slider_style(&self, hover: Option<bool>) {
        let Some(slider) = self.slider.get() else { return };
        let colors = scrollbar_colors();
        let mut tree = self.ui.tree_mut();
        let was_hover = tree.has_class(slider, "hover");
        let hover = hover.unwrap_or(was_hover);
        tree.toggle_class(slider, "hover", hover);
        let color = if tree.has_class(slider, "active") {
            colors.slider_active_background
        } else if hover {
            colors.slider_hover_background
        } else {
            colors.slider_background
        };
        tree.style_mut(slider).background = Some(color);
    }

    // ----------------- Update state

    fn after_state_change(&self, changed: bool) -> bool {
        if changed {
            self.visibility_controller.set_is_needed(self.state.borrow().is_needed());
            self.should_render.set(true);
            if !self.lazy_render {
                self.render();
            }
        }
        self.should_render.get()
    }

    pub(crate) fn on_element_size(&self, visible_size: f64) -> bool {
        let changed = self.state.borrow_mut().set_visible_size(visible_size);
        self.after_state_change(changed)
    }

    pub(crate) fn on_element_scroll_size(&self, scroll_size: f64) -> bool {
        let changed = self.state.borrow_mut().set_scroll_size(scroll_size);
        self.after_state_change(changed)
    }

    pub(crate) fn on_element_scroll_position(&self, scroll_position: f64) -> bool {
        let changed = self.state.borrow_mut().set_scroll_position(scroll_position);
        self.after_state_change(changed)
    }

    /// `onDidScroll` das duas subclasses.
    pub fn on_did_scroll(&self, e: &ScrollEvent) -> bool {
        let (scroll_size, position, size) = match self.axis {
            Axis::Horizontal => (e.scroll_width, e.scroll_left, e.width),
            Axis::Vertical => (e.scroll_height, e.scroll_top, e.height),
        };
        self.should_render.set(self.on_element_scroll_size(scroll_size) || self.should_render.get());
        self.should_render.set(self.on_element_scroll_position(position) || self.should_render.get());
        self.should_render.set(self.on_element_size(size) || self.should_render.get());
        self.should_render.get()
    }

    // ----------------- rendering

    pub fn begin_reveal(&self) {
        self.visibility_controller.set_should_be_visible(true);
    }

    pub fn begin_hide(&self) {
        self.visibility_controller.set_should_be_visible(false);
    }

    pub fn render(&self) {
        if !self.should_render.get() {
            return;
        }
        self.should_render.set(false);

        let (large, small, slider_size, slider_position) = {
            let s = self.state.borrow();
            (s.get_rectangle_large_size(), s.get_rectangle_small_size(), s.get_slider_size(), s.get_arrow_size() + s.get_slider_position())
        };
        let slider = self.slider.get().expect("o slider é criado no construtor");
        let mut tree = self.ui.tree_mut();
        match self.axis {
            Axis::Horizontal => {
                tree.set_bounds(self.dom_node, je_render::kurbo::Rect::new(0.0, 0.0, large, small));
                tree.set_bottom(self.dom_node, Some(0.0));
                let b = tree.bounds(slider);
                tree.set_bounds(slider, je_render::kurbo::Rect::new(slider_position, b.y0, slider_position + slider_size, b.y1));
            }
            Axis::Vertical => {
                tree.set_bounds(self.dom_node, je_render::kurbo::Rect::new(0.0, 0.0, small, large));
                tree.set_right(self.dom_node, Some(0.0));
                let b = tree.bounds(slider);
                tree.set_bounds(slider, je_render::kurbo::Rect::new(b.x0, slider_position, b.x1, slider_position + slider_size));
            }
        }
    }

    // ----------------- eventos

    fn dom_node_pointer_down(&self, e: &Event) {
        if e.target != self.dom_node {
            return;
        }
        if let Some(p) = e.pointer {
            self.on_pointer_down(&p);
        }
    }

    fn slider_pointer_position(&self, p: &PointerData) -> f64 {
        if self.axis == Axis::Horizontal { p.x } else { p.y }
    }

    fn slider_orthogonal_pointer_position(&self, p: &PointerData) -> f64 {
        if self.axis == Axis::Horizontal { p.y } else { p.x }
    }

    /// `delegatePointerDown`.
    pub fn delegate_pointer_down(&self, p: &PointerData, target: NodeId) {
        let dom_top = self.ui.tree().absolute_bounds(self.dom_node).y0;
        let (slider_position, slider_size) = {
            let s = self.state.borrow();
            (s.get_slider_position(), s.get_slider_size())
        };
        let slider_start = dom_top + slider_position;
        let slider_stop = dom_top + slider_position + slider_size;
        let pointer_pos = self.slider_pointer_position(p);
        if slider_start <= pointer_pos && pointer_pos <= slider_stop {
            // age como se fosse o pointer down do slider
            if p.button == PointerButton::Primary {
                self.slider_pointer_down(target, p);
            }
        } else {
            // age como se fosse o pointer down da barra
            self.on_pointer_down(p);
        }
    }

    fn on_pointer_down(&self, p: &PointerData) {
        let origin = self.ui.tree().absolute_bounds(self.dom_node);
        let (offset_x, offset_y) = (p.x - origin.x0, p.y - origin.y0);
        // `pointerType` é sempre mouse aqui
        let is_left_click = p.button == PointerButton::Primary;

        if is_left_click {
            let offset = if self.axis == Axis::Horizontal { offset_x } else { offset_y };
            let desired = {
                let s = self.state.borrow();
                if self.scroll_by_page.get() { s.get_desired_scroll_position_from_offset_paged(offset) } else { s.get_desired_scroll_position_from_offset(offset) }
            };
            self.set_desired_scroll_position_now(desired);
            self.slider_pointer_down(self.dom_node, p);
        }
    }

    fn slider_pointer_down(&self, target: NodeId, p: &PointerData) {
        let Some(slider) = self.slider.get() else { return };
        let initial_pointer_position = self.slider_pointer_position(p);
        let initial_orthogonal = self.slider_orthogonal_pointer_position(p);
        let initial_state = self.state.borrow().clone();
        self.ui.tree_mut().add_class(slider, "active");
        self.refresh_slider_style(None);

        let (weak_move, weak_stop) = (self.weak_self.clone(), self.weak_self.clone());
        self.pointer_move_monitor.start_monitoring(
            target,
            move |data| {
                let Some(this) = weak_move.upgrade() else { return };
                let orthogonal_delta = (this.slider_orthogonal_pointer_position(data) - initial_orthogonal).abs();
                if crate::common::is_windows() && orthogonal_delta > POINTER_DRAG_RESET_DISTANCE {
                    // O ponteiro se afastou da barra: reinicia o arraste.
                    this.set_desired_scroll_position_now(initial_state.get_scroll_position());
                    return;
                }
                let delta = this.slider_pointer_position(data) - initial_pointer_position;
                this.set_desired_scroll_position_now(initial_state.get_desired_scroll_position_from_delta(delta));
            },
            move || {
                if let Some(this) = weak_stop.upgrade() {
                    if let Some(slider) = this.slider.get() {
                        this.ui.tree_mut().remove_class(slider, "active");
                        this.refresh_slider_style(None);
                    }
                    this.host.on_drag_end();
                }
            },
        );
        self.host.on_drag_start();
    }

    fn set_desired_scroll_position_now(&self, desired: f64) {
        let mut position = INewScrollPosition::default();
        self.write_scroll_position(&mut position, desired);
        self.scrollable.set_scroll_position_now(&position);
    }

    pub fn update_scrollbar_size(&self, scrollbar_size: f64) {
        if let Some(slider) = self.slider.get() {
            let mut tree = self.ui.tree_mut();
            let b = tree.bounds(slider);
            match self.axis {
                Axis::Horizontal => tree.set_bounds(slider, je_render::kurbo::Rect::new(b.x0, b.y0, b.x1, b.y0 + scrollbar_size)),
                Axis::Vertical => tree.set_bounds(slider, je_render::kurbo::Rect::new(b.x0, b.y0, b.x0 + scrollbar_size, b.y1)),
            }
        }
        self.state.borrow_mut().set_scrollbar_size(scrollbar_size);
        self.should_render.set(true);
        if !self.lazy_render {
            self.render();
        }
    }

    pub fn is_needed(&self) -> bool {
        self.state.borrow().is_needed()
    }

    pub fn write_scroll_position(&self, target: &mut INewScrollPosition, scroll_position: f64) {
        match self.axis {
            Axis::Horizontal => target.scroll_left = Some(scroll_position),
            Axis::Vertical => target.scroll_top = Some(scroll_position),
        }
    }

    /// `updateOptions` das duas subclasses.
    pub fn update_options(&self, o: &ScrollableElementResolvedOptions) {
        let hidden = |v: ScrollbarVisibility, size: f64| if v == ScrollbarVisibility::Hidden { 0.0 } else { size };
        match self.axis {
            Axis::Horizontal => {
                self.update_scrollbar_size(hidden(o.horizontal, o.horizontal_scrollbar_size));
                self.state.borrow_mut().set_opposite_scrollbar_size(hidden(o.vertical, o.vertical_scrollbar_size));
                self.visibility_controller.set_visibility(o.horizontal);
            }
            Axis::Vertical => {
                self.update_scrollbar_size(hidden(o.vertical, o.vertical_scrollbar_size));
                // dá prioridade à barra vertical e a deixa rolar até o fim
                self.state.borrow_mut().set_opposite_scrollbar_size(0.0);
                self.visibility_controller.set_visibility(o.vertical);
            }
        }
        self.scroll_by_page.set(o.scroll_by_page);
    }
}

impl IDisposable for Scrollbar {
    fn dispose(&self) {
        self.base.dispose();
        self.ui.remove(self.dom_node);
    }

    fn constructor_name(&self) -> &'static str {
        "Scrollbar"
    }
}
