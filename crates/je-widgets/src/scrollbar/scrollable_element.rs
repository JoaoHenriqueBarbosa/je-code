//! Porte de `src/vs/base/browser/ui/scrollbar/scrollableElement.ts`: o elemento que embrulha um conteúdo,
//! lhe põe as barras de rolagem e trata a roda do mouse.
//!
//! Do DOM para o `je-ui`: o elemento rolável é um nó solto (`dom_node`) com `overflow: hidden` (recorte)
//! que recebe o conteúdo e as barras como filhos; a roda é o [`EventKind::Wheel`]; `mouseover` e
//! `mouseleave` são `PointerEnter` e `PointerLeave` (o `mouseover` do DOM também dispara ao passar entre
//! filhos, o que aqui seria só repetir um `reveal` idempotente); `Date.now()` é o do runtime de `je-base`;
//! `scheduleAtNextAnimationFrame` é o do [`Ui`]. O isChrome/zoom do classificador não existe (sem
//! navegador): vale o ramo não-Chrome. `DomScrollableElement` (que lê `scrollTop` do HTMLElement) não tem
//! equivalente e fica de fora; `ScrollableElement` e `SmoothScrollableElement` estão aqui.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::TimeoutTimer;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::runtime;
use je_base::scrollable::{
    INewScrollDimensions, INewScrollPosition, IScrollDimensions, IScrollPosition, IScrollableOptions, ScrollEvent, Scrollable, ScrollbarVisibility,
};
use je_render::kurbo::Rect;
use je_ui::{EventKind, NodeId, Role, StandardWheelEvent, Ui, WheelData};

use crate::common::{GlobalSetting, registered_emitter, subscribe};
use crate::sash::is_macintosh;
use crate::scrollbar::abstract_scrollbar::{Axis, Scrollbar, ScrollbarHost};
use crate::scrollbar::scrollable_element_options::{ScrollableElementChangeOptions, ScrollableElementCreationOptions, ScrollableElementResolvedOptions};
use crate::scrollbar::scrollbar_colors;

const HIDE_TIMEOUT: u64 = 500;
const SCROLL_WHEEL_SENSITIVITY: f64 = 50.0;
const SCROLL_WHEEL_SMOOTH_SCROLL_ENABLED: bool = true;

/// O tamanho padrão (px) quando o elemento não passa um tamanho explícito.
pub const DEFAULT_SCROLLBAR_SIZE: f64 = 10.0;

thread_local! {
    static REVEAL_ON_SCROLL_DISABLED_COUNT: Cell<u32> = const { Cell::new(0) };
    static GLOBAL_DEFAULT_SCROLLBAR_SIZE: GlobalSetting<f64> = GlobalSetting::new(DEFAULT_SCROLLBAR_SIZE);
    static CLASSIFIER: RefCell<MouseWheelClassifier> = RefCell::new(MouseWheelClassifier::new());
}

/// Só para testes: impede que as barras automáticas apareçam ao rolar.
pub fn disable_scrollbar_reveal_on_scroll() -> DisposableRef {
    REVEAL_ON_SCROLL_DISABLED_COUNT.with(|c| c.set(c.get() + 1));
    je_base::lifecycle::to_disposable(|| REVEAL_ON_SCROLL_DISABLED_COUNT.with(|c| c.set(c.get() - 1)))
}

/// Muda o tamanho padrão das barras dos elementos criados sem tamanho explícito.
pub fn set_global_default_scrollbar_size(size: f64) {
    if size != GLOBAL_DEFAULT_SCROLLBAR_SIZE.with(|s| s.value.get()) {
        GLOBAL_DEFAULT_SCROLLBAR_SIZE.with(|s| s.set(size));
    }
}

fn global_default_scrollbar_size() -> f64 {
    GLOBAL_DEFAULT_SCROLLBAR_SIZE.with(|s| s.value.get())
}

struct MouseWheelClassifierItem {
    #[allow(dead_code)]
    timestamp: f64,
    delta_x: f64,
    delta_y: f64,
    score: f64,
}

/// `MouseWheelClassifier`: separa a roda física do touchpad pela regularidade dos deltas.
pub struct MouseWheelClassifier {
    capacity: usize,
    memory: Vec<MouseWheelClassifierItem>,
    front: Option<usize>,
    rear: Option<usize>,
}

impl Default for MouseWheelClassifier {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseWheelClassifier {
    pub fn new() -> Self {
        Self { capacity: 5, memory: Vec::new(), front: None, rear: None }
    }

    pub fn is_physical_mouse_wheel(&self) -> bool {
        let (Some(front), Some(rear)) = (self.front, self.rear) else {
            // sem elementos
            return false;
        };

        // 0.5 * último + 0.25 * penúltimo + 0.125 * antepenúltimo + ...
        let mut remaining_influence = 1.0;
        let mut score = 0.0;
        let mut iteration = 1;
        let mut index = rear;
        loop {
            let influence = if index == front { remaining_influence } else { 2f64.powi(-iteration) };
            remaining_influence -= influence;
            score += self.memory[index].score * influence;
            if index == front {
                break;
            }
            index = (self.capacity + index - 1) % self.capacity;
            iteration += 1;
        }
        score <= 0.5
    }

    pub fn accept_standard_wheel_event(&mut self, e: &StandardWheelEvent) {
        self.accept(runtime::date_now() as f64, e.delta_x, e.delta_y);
    }

    pub fn accept(&mut self, timestamp: f64, delta_x: f64, delta_y: f64) {
        let mut item = MouseWheelClassifierItem { timestamp, delta_x, delta_y, score: 0.0 };
        let previous;
        match (self.front, self.rear) {
            (None, None) => {
                previous = None;
                self.memory.push(MouseWheelClassifierItem { timestamp, delta_x, delta_y, score: 0.0 });
                self.front = Some(0);
                self.rear = Some(0);
            }
            (Some(front), Some(rear)) => {
                previous = Some((self.memory[rear].delta_x, self.memory[rear].delta_y));
                let new_rear = (rear + 1) % self.capacity;
                self.rear = Some(new_rear);
                if new_rear == front {
                    // descarta o mais antigo
                    self.front = Some((front + 1) % self.capacity);
                }
                if new_rear == self.memory.len() {
                    self.memory.push(MouseWheelClassifierItem { timestamp, delta_x, delta_y, score: 0.0 });
                } else {
                    self.memory[new_rear] = MouseWheelClassifierItem { timestamp, delta_x, delta_y, score: 0.0 };
                }
            }
            _ => unreachable!("front e rear andam juntos"),
        }
        item.score = Self::compute_score(&item, previous);
        let rear = self.rear.expect("rear acabou de ser definido");
        self.memory[rear].score = item.score;
    }

    /// Uma nota de 0 a 1 para `item`: perto de 0, a fonte parece uma roda física; perto de 1, um touchpad
    /// ou um magic mouse.
    fn compute_score(item: &MouseWheelClassifierItem, previous: Option<(f64, f64)>) -> f64 {
        if item.delta_x.abs() > 0.0 && item.delta_y.abs() > 0.0 {
            // os dois eixos exercitados: com certeza não é uma roda física
            return 1.0;
        }

        let mut score: f64 = 0.5;

        if !Self::is_almost_int(item.delta_x) || !Self::is_almost_int(item.delta_y) {
            // deltas fracionários indicam que não é uma roda física
            score += 0.25;
        }

        // Rolagem sem aceleração indica roda física: um delta é múltiplo do outro.
        if let Some((previous_x, previous_y)) = previous {
            let (abs_delta_x, abs_delta_y) = (item.delta_x.abs(), item.delta_y.abs());
            let (abs_previous_x, abs_previous_y) = (previous_x.abs(), previous_y.abs());

            // Mínimo 1 para não dividir por zero; módulo 1 continua dando 0.
            let min_delta_x = f64::max(f64::min(abs_delta_x, abs_previous_x), 1.0);
            let min_delta_y = f64::max(f64::min(abs_delta_y, abs_previous_y), 1.0);

            let max_delta_x = f64::max(abs_delta_x, abs_previous_x);
            let max_delta_y = f64::max(abs_delta_y, abs_previous_y);

            let is_same_modulo = max_delta_x % min_delta_x == 0.0 && max_delta_y % min_delta_y == 0.0;
            if is_same_modulo {
                score -= 0.5;
            }
        }

        score.clamp(0.0, 1.0)
    }

    fn is_almost_int(value: f64) -> bool {
        let epsilon = f64::EPSILON * 100.0; // tolerância para erro de ponto flutuante
        let delta = (crate::common::js_round(value) - value).abs();
        delta < 0.01 + epsilon
    }
}

fn resolve_options(opts: &ScrollableElementCreationOptions) -> ScrollableElementResolvedOptions {
    let horizontal_scrollbar_size = opts.horizontal_scrollbar_size.unwrap_or_else(global_default_scrollbar_size);
    let vertical_scrollbar_size = opts.vertical_scrollbar_size.unwrap_or_else(global_default_scrollbar_size);
    let mut class_name = opts.class_name.clone().unwrap_or_default();
    // Os padrões são diferentes nos Macs.
    if is_macintosh() {
        class_name.push_str(" mac");
    }
    ScrollableElementResolvedOptions {
        lazy_render: opts.lazy_render.unwrap_or(false),
        class_name,
        use_shadows: opts.use_shadows.unwrap_or(true),
        handle_mouse_wheel: opts.handle_mouse_wheel.unwrap_or(true),
        flip_axes: opts.flip_axes.unwrap_or(false),
        consume_mouse_wheel_if_scrollbar_is_needed: opts.consume_mouse_wheel_if_scrollbar_is_needed.unwrap_or(false),
        always_consume_mouse_wheel: opts.always_consume_mouse_wheel.unwrap_or(false),
        scroll_y_to_x: opts.scroll_y_to_x.unwrap_or(false),
        mouse_wheel_scroll_sensitivity: opts.mouse_wheel_scroll_sensitivity.unwrap_or(1.0),
        fast_scroll_sensitivity: opts.fast_scroll_sensitivity.unwrap_or(5.0),
        scroll_predominant_axis: opts.scroll_predominant_axis.unwrap_or(true),
        mouse_wheel_smooth_scroll: opts.mouse_wheel_smooth_scroll.unwrap_or(true),
        inertial_scroll: opts.inertial_scroll.unwrap_or(false),
        arrow_size: opts.arrow_size.unwrap_or(11.0),
        listen_on_dom_node: opts.listen_on_dom_node,
        horizontal: opts.horizontal.unwrap_or(ScrollbarVisibility::Auto),
        horizontal_scrollbar_size,
        horizontal_slider_size: opts.horizontal_slider_size.unwrap_or(horizontal_scrollbar_size),
        horizontal_has_arrows: opts.horizontal_has_arrows.unwrap_or(false),
        vertical: opts.vertical.unwrap_or(ScrollbarVisibility::Auto),
        vertical_scrollbar_size,
        vertical_slider_size: opts.vertical_slider_size.unwrap_or(vertical_scrollbar_size),
        vertical_has_arrows: opts.vertical_has_arrows.unwrap_or(false),
        scroll_by_page: opts.scroll_by_page.unwrap_or(false),
    }
}

struct Host(Weak<AbstractScrollableElement>);

impl ScrollbarHost for Host {
    fn on_mouse_wheel(&self, e: &StandardWheelEvent) {
        if let Some(this) = self.0.upgrade() {
            this.on_mouse_wheel(e);
        }
    }

    fn on_drag_start(&self) {
        if let Some(this) = self.0.upgrade() {
            this.is_dragging.set(true);
            this.reveal();
        }
    }

    fn on_drag_end(&self) {
        if let Some(this) = self.0.upgrade() {
            this.is_dragging.set(false);
            this.hide();
        }
    }
}

/// `AbstractScrollableElement`: as variantes (`ScrollableElement`, `SmoothScrollableElement`) são
/// construtores deste tipo com o `Scrollable` e as opções do original.
pub struct AbstractScrollableElement {
    base: Rc<Disposable>,
    weak_self: Weak<AbstractScrollableElement>,
    ui: Ui,
    options: RefCell<ScrollableElementResolvedOptions>,
    scrollable: Rc<Scrollable>,
    vertical_scrollbar: Rc<Scrollbar>,
    horizontal_scrollbar: Rc<Scrollbar>,
    /// O `_domNode`.
    dom_node: NodeId,
    left_shadow: Option<NodeId>,
    top_shadow: Option<NodeId>,
    top_left_shadow: Option<NodeId>,
    listen_on_dom_node: NodeId,
    mouse_wheel_to_dispose: RefCell<Option<DisposableRef>>,
    is_dragging: Cell<bool>,
    mouse_is_over: Cell<bool>,
    hide_timeout: Rc<TimeoutTimer>,
    should_render: Cell<bool>,
    reveal_on_scroll: Cell<bool>,
    inertial_timeout: RefCell<Option<Rc<TimeoutTimer>>>,
    inertial_speed: Cell<(f64, f64)>,
    on_scroll: Rc<Emitter<ScrollEvent>>,
    on_will_scroll: Rc<Emitter<ScrollEvent>>,
    disposed: Cell<bool>,
}

impl AbstractScrollableElement {
    fn new(ui: &Ui, element: NodeId, creation: &ScrollableElementCreationOptions, scrollable: Rc<Scrollable>) -> Rc<Self> {
        ui.tree_mut().style_mut(element).clip = true;
        let options = resolve_options(creation);
        let base = Disposable::new();
        let on_scroll = registered_emitter(&base);
        let on_will_scroll = registered_emitter(&base);

        let dom_node = {
            let mut tree = ui.tree_mut();
            let node = tree.create_detached(Role::Generic);
            tree.add_class(node, "monaco-scrollable-element");
            for class in options.class_name.split_whitespace() {
                tree.add_class(node, class);
            }
            tree.style_mut(node).clip = true;
            tree.append_child(node, element);
            node
        };

        let this = Rc::new_cyclic(|weak: &Weak<AbstractScrollableElement>| {
            let host: Rc<dyn ScrollbarHost> = Rc::new(Host(weak.clone()));
            let vertical_scrollbar = Scrollbar::new(ui, Axis::Vertical, scrollable.clone(), &options, host.clone());
            let horizontal_scrollbar = Scrollbar::new(ui, Axis::Horizontal, scrollable.clone(), &options, host);
            base.register(vertical_scrollbar.clone());
            base.register(horizontal_scrollbar.clone());

            let (left_shadow, top_shadow, top_left_shadow) = {
                let mut tree = ui.tree_mut();
                tree.append_child(dom_node, horizontal_scrollbar.dom_node);
                tree.append_child(dom_node, vertical_scrollbar.dom_node);
                if options.use_shadows {
                    let mut shadow = || {
                        let node = tree.create(dom_node, Role::Generic);
                        tree.add_class(node, "shadow");
                        node
                    };
                    (Some(shadow()), Some(shadow()), Some(shadow()))
                } else {
                    (None, None, None)
                }
            };
            let hide_timeout = TimeoutTimer::new();
            base.register(hide_timeout.clone());
            Self {
                base,
                weak_self: weak.clone(),
                ui: ui.clone(),
                listen_on_dom_node: options.listen_on_dom_node.unwrap_or(dom_node),
                options: RefCell::new(options.clone()),
                scrollable: scrollable.clone(),
                vertical_scrollbar,
                horizontal_scrollbar,
                dom_node,
                left_shadow,
                top_shadow,
                top_left_shadow,
                mouse_wheel_to_dispose: RefCell::new(None),
                is_dragging: Cell::new(false),
                mouse_is_over: Cell::new(false),
                hide_timeout,
                should_render: Cell::new(true),
                reveal_on_scroll: Cell::new(true),
                inertial_timeout: RefCell::new(None),
                inertial_speed: Cell::new((0.0, 0.0)),
                on_scroll,
                on_will_scroll,
                disposed: Cell::new(false),
            }
        });

        let weak = this.weak_self.clone();
        this.base.register(scrollable.on_scroll().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_will_scroll.fire(*e);
                this.on_did_scroll(e);
                this.on_scroll.fire(*e);
            }
        }));
        this.set_listening_to_mouse_wheel(options.handle_mouse_wheel);

        for (kind, over) in [(EventKind::PointerEnter, true), (EventKind::PointerLeave, false)] {
            let weak = this.weak_self.clone();
            this.base.register(ui.listen(this.listen_on_dom_node, kind, false, move |_| {
                if let Some(this) = weak.upgrade() {
                    this.mouse_is_over.set(over);
                    if over { this.reveal() } else { this.hide() }
                }
            }));
        }

        // Os tamanhos padrão globais só afetam os eixos sem tamanho explícito.
        let (h_explicit, v_explicit) = (creation.horizontal_scrollbar_size.is_some(), creation.vertical_scrollbar_size.is_some());
        if !h_explicit || !v_explicit {
            let weak = this.weak_self.clone();
            this.base.register(subscribe(&GLOBAL_DEFAULT_SCROLLBAR_SIZE, move |new_size| {
                if let Some(this) = weak.upgrade() {
                    this.update_options(&ScrollableElementChangeOptions {
                        horizontal_scrollbar_size: (!h_explicit).then_some(new_size),
                        vertical_scrollbar_size: (!v_explicit).then_some(new_size),
                        ..Default::default()
                    });
                }
            }));
        }
        this
    }

    /// `new ScrollableElement(element, options)`: rolagem sempre imediata, valores inteiros.
    pub fn scrollable_element(ui: &Ui, element: NodeId, mut options: ScrollableElementCreationOptions) -> Rc<Self> {
        options.mouse_wheel_smooth_scroll = Some(false);
        let scrollable = Self::make_scrollable(ui);
        let this = Self::new(ui, element, &options, scrollable.clone());
        this.base.register(scrollable);
        this
    }

    /// `new SmoothScrollableElement(element, options, scrollable)`: o `Scrollable` é do chamador.
    pub fn smooth_scrollable_element(ui: &Ui, element: NodeId, options: ScrollableElementCreationOptions, scrollable: Rc<Scrollable>) -> Rc<Self> {
        Self::new(ui, element, &options, scrollable)
    }

    /// O `Scrollable` do `ScrollableElement`: `forceIntegerValues: true`, sem rolagem suave.
    fn make_scrollable(ui: &Ui) -> Rc<Scrollable> {
        let ui = ui.clone();
        Scrollable::new(IScrollableOptions {
            force_integer_values: true,
            smooth_scroll_duration: 0.0,
            schedule_at_next_animation_frame: Rc::new(move |callback| ui.schedule_at_next_animation_frame(0, callback)),
        })
    }

    pub fn on_scroll(&self) -> BaseEvent<ScrollEvent> {
        self.on_scroll.event()
    }

    pub fn on_will_scroll(&self) -> BaseEvent<ScrollEvent> {
        self.on_will_scroll.event()
    }

    pub fn options(&self) -> ScrollableElementResolvedOptions {
        self.options.borrow().clone()
    }

    /// O nó rolável gerado.
    pub fn get_dom_node(&self) -> NodeId {
        self.dom_node
    }

    /// `getOverviewRulerLayoutInfo`: o pai e o nó antes do qual inserir.
    pub fn get_overview_ruler_layout_info(&self) -> (NodeId, NodeId) {
        (self.dom_node, self.vertical_scrollbar.dom_node)
    }

    /// `delegateVerticalScrollbarPointerDown`.
    pub fn delegate_vertical_scrollbar_pointer_down(&self, pointer: &je_ui::PointerData, target: NodeId) {
        self.vertical_scrollbar.delegate_pointer_down(pointer, target);
    }

    pub fn get_scroll_dimensions(&self) -> IScrollDimensions {
        self.scrollable.get_scroll_dimensions()
    }

    pub fn set_scroll_dimensions(&self, dimensions: &INewScrollDimensions) {
        self.scrollable.set_scroll_dimensions(dimensions, false);
    }

    /// `ScrollableElement.setScrollPosition`.
    pub fn set_scroll_position(&self, update: &INewScrollPosition) {
        self.scrollable.set_scroll_position_now(update);
    }

    /// `SmoothScrollableElement.setScrollPosition`: com `reuse_animation` a rolagem é suave.
    pub fn set_scroll_position_smooth(&self, update: &INewScrollPosition, reuse_animation: bool) {
        if reuse_animation {
            self.scrollable.set_scroll_position_smooth(update, reuse_animation);
        } else {
            self.scrollable.set_scroll_position_now(update);
        }
    }

    pub fn get_scroll_position(&self) -> IScrollPosition {
        self.scrollable.get_current_scroll_position()
    }

    /// `updateClassName`.
    pub fn update_class_name(&self, new_class_name: &str) {
        let mut class_name = new_class_name.to_owned();
        // Os padrões são diferentes nos Macs.
        if is_macintosh() {
            class_name.push_str(" mac");
        }
        let mut tree = self.ui.tree_mut();
        tree.set_class_name(self.dom_node, &format!("monaco-scrollable-element {class_name}"));
        drop(tree);
        self.options.borrow_mut().class_name = class_name;
    }

    /// `updateOptions`.
    pub fn update_options(&self, new: &ScrollableElementChangeOptions) {
        let resolved = {
            let mut o = self.options.borrow_mut();
            if let Some(v) = new.handle_mouse_wheel {
                o.handle_mouse_wheel = v;
            }
            if let Some(v) = new.mouse_wheel_scroll_sensitivity {
                o.mouse_wheel_scroll_sensitivity = v;
            }
            if let Some(v) = new.fast_scroll_sensitivity {
                o.fast_scroll_sensitivity = v;
            }
            if let Some(v) = new.scroll_predominant_axis {
                o.scroll_predominant_axis = v;
            }
            if let Some(v) = new.horizontal {
                o.horizontal = v;
            }
            if let Some(v) = new.vertical {
                o.vertical = v;
            }
            if let Some(v) = new.horizontal_scrollbar_size {
                o.horizontal_scrollbar_size = v;
            }
            if let Some(v) = new.vertical_scrollbar_size {
                o.vertical_scrollbar_size = v;
            }
            if let Some(v) = new.scroll_by_page {
                o.scroll_by_page = v;
            }
            o.clone()
        };
        if new.handle_mouse_wheel.is_some() {
            self.set_listening_to_mouse_wheel(resolved.handle_mouse_wheel);
        }
        self.horizontal_scrollbar.update_options(&resolved);
        self.vertical_scrollbar.update_options(&resolved);

        if !resolved.lazy_render {
            self.render();
        }
    }

    pub fn set_reveal_on_scroll(&self, value: bool) {
        self.reveal_on_scroll.set(value);
    }

    /// `delegateScrollFromMouseWheelEvent`.
    pub fn delegate_scroll_from_mouse_wheel_event(&self, wheel: &WheelData, target: Option<NodeId>) {
        self.on_mouse_wheel(&StandardWheelEvent::new(wheel, target, false));
    }

    fn periodic_sync(&self) {
        let mut schedule_again = false;
        let (mut speed_x, mut speed_y) = self.inertial_speed.get();

        if speed_x != 0.0 || speed_y != 0.0 {
            let current = self.scrollable.get_current_scroll_position();
            self.scrollable.set_scroll_position_now(&INewScrollPosition {
                scroll_top: Some(current.scroll_top - speed_y * 100.0),
                scroll_left: Some(current.scroll_left - speed_x * 100.0),
            });
            speed_x *= 0.9;
            speed_y *= 0.9;
            if speed_x.abs() < 0.01 {
                speed_x = 0.0;
            }
            if speed_y.abs() < 0.01 {
                speed_y = 0.0;
            }
            self.inertial_speed.set((speed_x, speed_y));
            schedule_again = speed_x != 0.0 || speed_y != 0.0;
        }

        if schedule_again {
            let timer = self.inertial_timeout.borrow_mut().get_or_insert_with(TimeoutTimer::new).clone();
            let weak = self.weak_self.clone();
            timer.cancel_and_set(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.periodic_sync();
                    }
                },
                1000 / 60,
            );
        } else if let Some(timer) = self.inertial_timeout.borrow_mut().take() {
            timer.dispose();
        }
    }

    // -------------------- rolagem pela roda do mouse --------------------

    fn set_listening_to_mouse_wheel(&self, should_listen: bool) {
        let is_listening = self.mouse_wheel_to_dispose.borrow().is_some();
        if is_listening == should_listen {
            // nada mudou
            return;
        }
        // Para de escutar, se for o caso.
        if let Some(listener) = self.mouse_wheel_to_dispose.borrow_mut().take() {
            listener.dispose();
        }
        // Começa a escutar, se for o caso.
        if should_listen {
            let weak = self.weak_self.clone();
            let listener = self.ui.listen(self.listen_on_dom_node, EventKind::Wheel, false, move |e| {
                let (Some(this), Some(wheel)) = (weak.upgrade(), e.wheel) else { return };
                let standard = StandardWheelEvent::new(&wheel, Some(e.target), e.default_prevented());
                if this.on_mouse_wheel(&standard) {
                    e.stop(true);
                }
            });
            *self.mouse_wheel_to_dispose.borrow_mut() = Some(listener);
        }
    }

    /// `_onMouseWheel`. Devolve se a roda foi consumida (o `preventDefault` e o `stopPropagation` do
    /// original); o ouvinte da roda os aplica ao evento.
    fn on_mouse_wheel(&self, e: &StandardWheelEvent) -> bool {
        if e.default_prevented {
            return false;
        }

        let options = self.options.borrow().clone();
        if SCROLL_WHEEL_SMOOTH_SCROLL_ENABLED {
            CLASSIFIER.with(|c| c.borrow_mut().accept_standard_wheel_event(e));
        }
        let is_physical = || CLASSIFIER.with(|c| c.borrow().is_physical_mouse_wheel());

        let mut did_scroll = false;

        if e.delta_y != 0.0 || e.delta_x != 0.0 {
            let mut delta_y = e.delta_y * options.mouse_wheel_scroll_sensitivity;
            let mut delta_x = e.delta_x * options.mouse_wheel_scroll_sensitivity;

            if options.scroll_predominant_axis {
                if options.scroll_y_to_x && delta_x + delta_y == 0.0 {
                    // Com Y para X e X e Y competindo com valores idênticos em sentidos opostos, não há
                    // eixo dominante: o delta é ignorado.
                    delta_x = 0.0;
                    delta_y = 0.0;
                } else if delta_y.abs() >= delta_x.abs() {
                    delta_x = 0.0;
                } else {
                    delta_y = 0.0;
                }
            }

            if options.flip_axes {
                std::mem::swap(&mut delta_x, &mut delta_y);
            }

            // Converte a rolagem vertical em horizontal com Shift (no Mac isso é tratado em nível mais alto).
            let shift_convert = !is_macintosh() && e.browser_event.is_some_and(|b| b.modifiers.shift);
            if (options.scroll_y_to_x || shift_convert) && delta_x == 0.0 {
                delta_x = delta_y;
                delta_y = 0.0;
            }

            if e.browser_event.is_some_and(|b| b.modifiers.alt) {
                // rolagem rápida
                delta_x *= options.fast_scroll_sensitivity;
                delta_y *= options.fast_scroll_sensitivity;
            }

            let future = self.scrollable.get_future_scroll_position();

            let mut desired = INewScrollPosition::default();
            if delta_y != 0.0 {
                let delta_scroll_top = SCROLL_WHEEL_SENSITIVITY * delta_y;
                // Converte valores como -0.3 em -1 e 0.3 em 1; sem isso a rolagem lenta nunca anda.
                let desired_scroll_top = future.scroll_top - if delta_scroll_top < 0.0 { delta_scroll_top.floor() } else { delta_scroll_top.ceil() };
                self.vertical_scrollbar.write_scroll_position(&mut desired, desired_scroll_top);
            }
            if delta_x != 0.0 {
                let delta_scroll_left = SCROLL_WHEEL_SENSITIVITY * delta_x;
                let desired_scroll_left = future.scroll_left - if delta_scroll_left < 0.0 { delta_scroll_left.floor() } else { delta_scroll_left.ceil() };
                self.horizontal_scrollbar.write_scroll_position(&mut desired, desired_scroll_left);
            }

            // Garante que a rolagem vai para um lugar válido.
            let validated = self.scrollable.validate_scroll_position(&desired);

            if options.inertial_scroll && (delta_x != 0.0 || delta_y != 0.0) && !is_physical() {
                let (speed_x, speed_y) = self.inertial_speed.get();
                // Só inicia o periódico se ele não estiver rodando.
                let start_periodic = speed_x == 0.0 && speed_y == 0.0;
                self.inertial_speed.set((
                    (if delta_x < 0.0 { -1.0 } else { 1.0 }) * delta_x.abs().powf(1.02),
                    (if delta_y < 0.0 { -1.0 } else { 1.0 }) * delta_y.abs().powf(1.02),
                ));
                if start_periodic {
                    self.periodic_sync();
                }
            }

            if future.scroll_left != validated.scroll_left || future.scroll_top != validated.scroll_top {
                let can_perform_smooth_scroll = SCROLL_WHEEL_SMOOTH_SCROLL_ENABLED && options.mouse_wheel_smooth_scroll && is_physical();
                let target = INewScrollPosition { scroll_left: Some(validated.scroll_left), scroll_top: Some(validated.scroll_top) };
                if can_perform_smooth_scroll {
                    self.scrollable.set_scroll_position_smooth(&target, false);
                } else {
                    self.scrollable.set_scroll_position_now(&target);
                }
                did_scroll = true;
            }
        }

        let mut consume = did_scroll;
        if !consume && options.always_consume_mouse_wheel {
            consume = true;
        }
        if !consume && options.consume_mouse_wheel_if_scrollbar_is_needed && (self.vertical_scrollbar.is_needed() || self.horizontal_scrollbar.is_needed()) {
            consume = true;
        }
        consume
    }

    fn on_did_scroll(&self, e: &ScrollEvent) {
        self.should_render.set(self.horizontal_scrollbar.on_did_scroll(e) || self.should_render.get());
        self.should_render.set(self.vertical_scrollbar.on_did_scroll(e) || self.should_render.get());

        let options = self.options.borrow();
        if options.use_shadows {
            self.should_render.set(true);
        }

        if self.reveal_on_scroll.get() && REVEAL_ON_SCROLL_DISABLED_COUNT.with(Cell::get) == 0 {
            self.reveal();
        }

        let lazy = options.lazy_render;
        drop(options);
        if !lazy {
            self.render();
        }
    }

    /// `renderNow`: desenha agora (use com `lazy_render`).
    pub fn render_now(&self) {
        if !self.options.borrow().lazy_render {
            crate::common::throw("Please use `lazyRender` together with `renderNow`!");
        }
        self.render();
    }

    fn render(&self) {
        if !self.should_render.get() {
            return;
        }
        self.should_render.set(false);

        self.horizontal_scrollbar.render();
        self.vertical_scrollbar.render();

        if self.options.borrow().use_shadows {
            let state = self.scrollable.get_current_scroll_position();
            let enable_top = state.scroll_top > 0.0;
            let enable_left = state.scroll_left > 0.0;

            let left_class = if enable_left { " left" } else { "" };
            let top_class = if enable_top { " top" } else { "" };
            let top_left_class = if enable_left || enable_top { " top-left-corner" } else { "" };
            self.set_shadow(self.left_shadow, &format!("shadow{left_class}"));
            self.set_shadow(self.top_shadow, &format!("shadow{top_class}"));
            self.set_shadow(self.top_left_shadow, &format!("shadow{top_left_class}{top_class}{left_class}"));
        }
    }

    /// `setClassName` do sombreamento mais a regra de CSS de `scrollbars.css`. O `box-shadow: inset` não
    /// existe no `je-render`: a sombra é uma faixa lisa de 3px com a cor `scrollbar.shadow` pela metade do
    /// alfa (aproximação). O canto não pinta.
    fn set_shadow(&self, node: Option<NodeId>, class_name: &str) {
        let Some(node) = node else { return };
        let mut tree = self.ui.tree_mut();
        tree.set_class_name(node, class_name);
        let size = tree.bounds(self.dom_node);
        let shadow = scrollbar_colors().shadow;
        let faded = je_render::Rgba { a: shadow.a / 2, ..shadow };
        let (rect, background) = if tree.has_class(node, "top-left-corner") {
            (Some(Rect::new(0.0, 0.0, 3.0, 3.0)), None)
        } else if tree.has_class(node, "top") {
            (Some(Rect::new(3.0, 0.0, 3.0 + size.width(), 3.0)), Some(faded))
        } else if tree.has_class(node, "left") {
            (Some(Rect::new(0.0, 3.0, 3.0, 3.0 + size.height())), Some(faded))
        } else {
            (None, None)
        };
        tree.style_mut(node).visible = rect.is_some();
        tree.style_mut(node).background = background;
        if let Some(rect) = rect {
            tree.set_bounds(node, rect);
        }
    }

    // -------------------- fade in e fade out --------------------

    fn reveal(&self) {
        self.vertical_scrollbar.begin_reveal();
        self.horizontal_scrollbar.begin_reveal();
        self.schedule_hide();
    }

    fn hide(&self) {
        if !self.mouse_is_over.get() && !self.is_dragging.get() {
            self.vertical_scrollbar.begin_hide();
            self.horizontal_scrollbar.begin_hide();
        }
    }

    fn schedule_hide(&self) {
        if !self.mouse_is_over.get() && !self.is_dragging.get() {
            let weak = self.weak_self.clone();
            self.hide_timeout.cancel_and_set(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.hide();
                    }
                },
                HIDE_TIMEOUT,
            );
        }
    }
}

impl IDisposable for AbstractScrollableElement {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        if let Some(listener) = self.mouse_wheel_to_dispose.borrow_mut().take() {
            listener.dispose();
        }
        if let Some(timer) = self.inertial_timeout.borrow_mut().take() {
            timer.dispose();
        }
        self.base.dispose();
        self.ui.remove(self.dom_node);
    }

    fn constructor_name(&self) -> &'static str {
        "AbstractScrollableElement"
    }
}
