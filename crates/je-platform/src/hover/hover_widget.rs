//! Porte de `src/vs/platform/hover/browser/hoverWidget.ts`: o `HoverWidget` posicionado em relação ao alvo (as quatro direções,
//! a troca de lado quando falta espaço, o ajuste ao viewport, o ponteiro, o modo fixo `forcePosition`, o limite de altura e a
//! trava por Alt) e o `CompositeMouseTracker`.
//!
//! Divergências de tradução:
//! - **Medidas do DOM.** O que o original lê do navegador (`window.innerHeight`, `documentElement.clientWidth` e `clientLeft`,
//!   `getDomNodeZoomLevel`, o refluxo do conteúdo e a largura da barra de rolagem) entra pela interface [`IHoverWidgetHost`].
//!   O resto da geometria (`getBoundingClientRect` do alvo, `clientWidth`, `offsetHeight`, `maxWidth`, `maxHeight`) é calculado
//!   aqui a partir de [`HoverContentMetrics`] e dos limites que o `layout` impõe, na mesma ordem do original.
//! - **Árvore de elementos.** O widget monta `ViewElement`s com as mesmas classes do original (`workbench-hover-container`,
//!   `monaco-hover`, `workbench-hover`, `compact`, `with-pointer`, `fade-in`, `right-aligned`, `locked`, `top`, `left`, `right`,
//!   `bottom`) e grava nelas os estilos que o original grava (`max-width`, `max-height`, `padding-*`, `margin-*`, `top`,
//!   `left`, `padding-right`), porque a pintura nativa os lê.
//! - **Conteúdo.** A renderização do markdown, das ações (`HoverAction.render`, com o rótulo do atalho) e da dica de
//!   "Hold Alt" é do pintor nativo, que informa o tamanho em [`HoverContentMetrics`]. O `ResizeObserver` do conteúdo HTML e o
//!   `asyncRenderCallback` do markdown viram [`HoverWidget::on_content_resized`] e [`HoverWidget::on_async_markdown_render`].
//! - **Eventos.** `mousedown.stopPropagation` não existe (não há propagação); `Escape` no contêiner e o `blur` da janela
//!   chegam pelo [`IHoverDom`].
//! - **Armadilha de foco.** O percurso por `tabIndex` é do host ([`IHoverWidgetHost::last_focusable_child`]); o widget cria os
//!   dois elementos sentinela, ouve o `focus` deles e devolve o foco, como no original.
//! - **Serviços.** `IKeybindingService`, `IConfigurationService`, `IAccessibilityService` e `IMarkdownRendererService` só
//!   serviam à renderização e à dica de acessibilidade; a dica chega pronta de [`IHoverWidgetHost::accessible_view_hint`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::TimeoutTimer;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::layout::{AnchorAlignment, AnchorPosition, ISize};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};

use crate::contextview::view_element::ViewElement;
use crate::hover::hover_service::{
    HoverContent, HoverDomEvent, HoverDomEventKind, HoverDomTarget, HoverPosition, HoverPositionOption, HoverStyle, HoverTarget, IHoverDom, IHoverOptions,
    IHoverWidget, IHoverWidgetFactory, IHoverWidgetHandle,
};

const POINTER_SIZE: f64 = 3.0;
const HOVER_BORDER_WIDTH: f64 = 2.0;
const HOVER_WINDOW_EDGE_MARGIN: f64 = 2.0;
const DEFAULT_MOUSE_OUT_DEBOUNCE_MS: u64 = 200;

/// O que o pintor nativo mede do conteúdo de um hover.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverContentMetrics {
    /// `containerDomNode.clientWidth` com o `max-width` pedido já aplicado (o texto quebra para caber).
    pub width: f64,
    /// A altura natural do conteúdo (`contentsDomNode.scrollHeight`), sem o limite de `max-height`.
    pub contents_height: f64,
    /// `containerDomNode.offsetHeight - contentsDomNode.offsetHeight` sem limites: bordas e a barra de status.
    pub outside_height: f64,
    /// `scrollbar.options.verticalScrollbarSize`.
    pub vertical_scrollbar_size: f64,
}

/// O pedido de medição: o conteúdo e o limite de largura (a caixa com borda) em vigor.
pub struct HoverMeasureRequest<'a> {
    pub content: &'a HoverContent,
    pub max_width: Option<f64>,
    pub has_status_bar: bool,
    pub compact: bool,
    pub with_pointer: bool,
}

/// As medidas do navegador e do pintor que o `HoverWidget` lê.
pub trait IHoverWidgetHost {
    /// `targetWindow.innerHeight`.
    fn window_inner_height(&self) -> f64;
    /// `targetDocumentElement.clientWidth`.
    fn document_client_width(&self) -> f64;
    /// `targetDocumentElement.clientLeft`.
    fn document_client_left(&self) -> f64;
    /// `getDomNodeZoomLevel(element)`.
    fn zoom_level(&self, element: &Rc<ViewElement>) -> f64;
    /// O refluxo do conteúdo para o limite de largura dado.
    fn measure(&self, request: &HoverMeasureRequest<'_>) -> HoverContentMetrics;
    /// `findLastFocusableChild(container)`.
    fn last_focusable_child(&self, container: &Rc<ViewElement>) -> Option<Rc<ViewElement>>;
    /// `getHoverAccessibleViewHint(...)` já resolvido com a configuração e o leitor de tela; `None` quando não há dica.
    fn accessible_view_hint(&self) -> Option<String>;
    /// `status(message)` do `aria`.
    fn announce(&self, message: &str);
    /// `this._hover.onContentsChanged()` (o `scanDomNode` da barra de rolagem).
    fn on_contents_changed(&self);
}

/// O alvo do hover: os elementos e as coordenadas explícitas.
struct WidgetTarget {
    elements: Vec<Rc<ViewElement>>,
    x: Option<f64>,
    y: Option<f64>,
}

impl WidgetTarget {
    fn from_options(target: &HoverTarget) -> Self {
        match target {
            HoverTarget::Element(element) => Self { elements: vec![element.clone()], x: None, y: None },
            HoverTarget::Structured(target) => Self { elements: target.target_elements.clone(), x: target.x, y: target.y },
        }
    }
}

/// `TargetRect`.
#[derive(Clone, Copy)]
struct TargetRect {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    width: f64,
    height: f64,
    center_x: f64,
    center_y: f64,
}

/// `HoverWidget`.
pub struct HoverWidget {
    this: Weak<HoverWidget>,
    dom: Rc<dyn IHoverDom>,
    host: Rc<dyn IHoverWidgetHost>,
    target: WidgetTarget,
    clipping_element: Option<Rc<ViewElement>>,
    anchor_alignment: AnchorAlignment,
    link_handler: Option<Rc<dyn Fn(&str)>>,
    content: HoverContent,
    has_status_bar: bool,
    compact: bool,

    hover_pointer: Option<Rc<ViewElement>>,
    hover_container: Rc<ViewElement>,
    container: Rc<ViewElement>,
    contents: Rc<ViewElement>,

    is_disposed: Cell<bool>,
    hover_position: Cell<HoverPosition>,
    force_position: bool,
    x: Cell<f64>,
    y: Cell<f64>,
    is_locked: Cell<bool>,
    enable_focus_traps: bool,
    added_focus_trap: Cell<bool>,
    max_height_ratio_relative_to_window: f64,
    container_max_width: Cell<Option<f64>>,
    container_max_height: Cell<Option<f64>>,
    contents_max_height: Cell<Option<f64>>,
    contents_padding_right: Cell<Option<f64>>,
    metrics_cache: Cell<Option<(Option<f64>, HoverContentMetrics)>>,

    mouse_tracker: Rc<CompositeMouseTracker>,
    lock_mouse_tracker: Rc<CompositeMouseTracker>,
    store: Rc<DisposableStore>,
    on_dispose: Rc<Emitter<()>>,
    on_request_layout: Rc<Emitter<()>>,
}

impl HoverWidget {
    pub fn new(options: &mut IHoverOptions, dom: Rc<dyn IHoverDom>, host: Rc<dyn IHoverWidgetHost>) -> Rc<Self> {
        let target = WidgetTarget::from_options(&options.target);
        let clipping_element = options.position.as_ref().and_then(|position| position.clipping_element.clone());
        let anchor_alignment = options.position.as_ref().and_then(|position| position.anchor_alignment).unwrap_or(AnchorAlignment::Left);

        match options.style {
            Some(HoverStyle::Pointer) => {
                let mut appearance = options.appearance.unwrap_or_default();
                appearance.compact.get_or_insert(true);
                appearance.show_pointer.get_or_insert(true);
                options.appearance = Some(appearance);
            }
            Some(HoverStyle::Mouse) => {
                let mut appearance = options.appearance.unwrap_or_default();
                appearance.compact.get_or_insert(true);
                options.appearance = Some(appearance);
            }
            None => {}
        }
        let appearance = options.appearance.unwrap_or_default();

        let hover_pointer = appearance.show_pointer.unwrap_or(false).then(|| ViewElement::with_classes(&["workbench-hover-pointer"]));
        let container = ViewElement::with_classes(&["monaco-hover"]);
        if !appearance.skip_fade_in_animation.unwrap_or(false) {
            container.class_list_add("fade-in");
        }
        container.class_list_add("workbench-hover");
        let compact = appearance.compact.unwrap_or(false);
        if compact {
            container.class_list_add("compact");
        }
        if hover_pointer.is_some() {
            container.class_list_add("with-pointer");
        }
        for class_name in &options.additional_classes {
            container.class_list_add(class_name);
        }
        let force_position = options.position.as_ref().and_then(|position| position.force_position).unwrap_or(false);
        let enable_focus_traps = options.trap_focus;

        let max_height_ratio_relative_to_window = match appearance.max_height_ratio {
            Some(ratio) if ratio > 0.0 && ratio <= 1.0 => ratio,
            _ => 0.5,
        };

        // Default to position above when the position is unspecified or a mouse event
        let hover_position = match options.position.as_ref().and_then(|position| position.hover_position) {
            None => HoverPosition::Above,
            Some(HoverPositionOption::Position(position)) => position,
            Some(HoverPositionOption::Mouse { .. }) => HoverPosition::Below,
        };

        let contents = ViewElement::with_classes(&["monaco-hover-content"]);
        match &options.content {
            HoverContent::Text(_) => contents.style_set_property("white-space", "pre-wrap"),
            HoverContent::Element(element) => {
                element.set_parent(Some(contents.clone()));
                contents.class_list_add("html-hover-contents");
            }
            HoverContent::Markdown(_) => {}
        }
        contents.set_parent(Some(container.clone()));

        let hover_container = ViewElement::with_classes(&["workbench-hover-container"]);
        if let Some(pointer) = &hover_pointer {
            pointer.set_parent(Some(hover_container.clone()));
        }
        container.set_parent(Some(hover_container.clone()));

        // Determine whether to hide on hover
        let has_actions = !options.actions.is_empty();
        let hide_on_hover = if has_actions {
            // If there are actions, require hover so they can be accessed
            false
        } else {
            match options.persistence.and_then(|persistence| persistence.hide_on_hover) {
                // When unset, will default to true when it's a string or when it's markdown that appears to have a link
                // using a naive check for '](' and '</a>'
                None => match &options.content {
                    HoverContent::Text(_) => true,
                    HoverContent::Markdown(markdown) => {
                        let value = markdown.value.to_string();
                        !value.contains("](") && !value.contains("</a>")
                    }
                    HoverContent::Element(_) => false,
                },
                // It's set explicitly
                Some(explicit) => explicit,
            }
        };
        let has_status_bar = has_actions || appearance.show_hover_hint.unwrap_or(false);

        let mut tracker_targets = target.elements.clone();
        if !hide_on_hover {
            tracker_targets.push(hover_container.clone());
        }
        let mouse_tracker = CompositeMouseTracker::new(dom.clone(), tracker_targets, DEFAULT_MOUSE_OUT_DEBOUNCE_MS);
        // Setup another mouse tracker when hideOnHover is set in order to track the hover as well when it is locked. This
        // ensures the hover will hide on mouseout after alt has been released to unlock the element.
        let lock_mouse_tracker = if hide_on_hover {
            let mut lock_targets = target.elements.clone();
            lock_targets.push(hover_container.clone());
            CompositeMouseTracker::new(dom.clone(), lock_targets, DEFAULT_MOUSE_OUT_DEBOUNCE_MS)
        } else {
            mouse_tracker.clone()
        };

        let widget = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            dom,
            host,
            target,
            clipping_element,
            anchor_alignment,
            link_handler: options.link_handler.clone(),
            content: options.content.clone(),
            has_status_bar,
            compact,
            hover_pointer,
            hover_container,
            container,
            contents,
            is_disposed: Cell::new(false),
            hover_position: Cell::new(hover_position),
            force_position,
            x: Cell::new(0.0),
            y: Cell::new(0.0),
            is_locked: Cell::new(false),
            enable_focus_traps,
            added_focus_trap: Cell::new(false),
            max_height_ratio_relative_to_window,
            container_max_width: Cell::new(None),
            container_max_height: Cell::new(None),
            contents_max_height: Cell::new(None),
            contents_padding_right: Cell::new(None),
            metrics_cache: Cell::new(None),
            mouse_tracker,
            lock_mouse_tracker,
            store: DisposableStore::new(),
            on_dispose: Emitter::new(),
            on_request_layout: Emitter::new(),
        });
        widget.wire();
        widget
    }

    /// Liga os ouvintes que o construtor do original registra.
    fn wire(&self) {
        let weak = self.this.clone();
        let container = self.container.clone();
        // Hide hover on escape
        self.store.add(self.dom.add_listener(
            HoverDomTarget::Element(container),
            HoverDomEventKind::KeyDown,
            false,
            Rc::new({
                let weak = weak.clone();
                move |e: &HoverDomEvent| {
                    let plain = !e.ctrl_key && !e.shift_key && !e.alt_key && !e.meta_key;
                    if plain && e.key_code == KeyCode::Escape {
                        if let Some(widget) = weak.upgrade() {
                            widget.dispose();
                        }
                    }
                }
            }),
        ));

        // Hide when the window loses focus
        self.store.add(self.dom.add_listener(
            HoverDomTarget::Window,
            HoverDomEventKind::Blur,
            false,
            Rc::new({
                let weak = weak.clone();
                move |_: &HoverDomEvent| {
                    if let Some(widget) = weak.upgrade() {
                        widget.dispose();
                    }
                }
            }),
        ));

        let trackers = if Rc::ptr_eq(&self.mouse_tracker, &self.lock_mouse_tracker) {
            vec![self.mouse_tracker.clone()]
        } else {
            vec![self.mouse_tracker.clone(), self.lock_mouse_tracker.clone()]
        };
        for tracker in trackers {
            self.store.add(tracker.on_mouse_out().listen({
                let weak = weak.clone();
                move |_| {
                    if let Some(widget) = weak.upgrade() {
                        if !widget.is_locked.get() {
                            widget.dispose();
                        }
                    }
                }
            }));
        }
    }

    pub fn is_disposed(&self) -> bool {
        self.is_disposed.get()
    }

    pub fn is_mouse_in(&self) -> bool {
        self.lock_mouse_tracker.is_mouse_in()
    }

    /// `domNode`: o `containerDomNode` do widget base.
    pub fn dom_node(&self) -> Rc<ViewElement> {
        self.container.clone()
    }

    /// O contêiner externo (`workbench-hover-container`), que leva o ponteiro e as margens.
    pub fn hover_container(&self) -> Rc<ViewElement> {
        self.hover_container.clone()
    }

    /// O elemento de conteúdo (`monaco-hover-content`).
    pub fn contents_dom_node(&self) -> Rc<ViewElement> {
        self.contents.clone()
    }

    /// O ponteiro, quando `showPointer`.
    pub fn hover_pointer(&self) -> Option<Rc<ViewElement>> {
        self.hover_pointer.clone()
    }

    pub fn link_handler(&self) -> Option<Rc<dyn Fn(&str)>> {
        self.link_handler.clone()
    }

    /// A direção atual (muda quando falta espaço).
    pub fn hover_position(&self) -> HoverPosition {
        self.hover_position.get()
    }

    pub fn on_dispose(&self) -> Event<()> {
        self.on_dispose.event()
    }

    pub fn on_request_layout(&self) -> Event<()> {
        self.on_request_layout.event()
    }

    pub fn anchor(&self) -> AnchorPosition {
        if self.hover_position.get() == HoverPosition::Below { AnchorPosition::Below } else { AnchorPosition::Above }
    }

    pub fn x(&self) -> f64 {
        self.x.get()
    }

    pub fn y(&self) -> f64 {
        self.y.get()
    }

    /// Whether the hover is "locked" by holding the alt/option key. When locked, the hover will not hide and can be hovered
    /// regardless of whether the `hideOnHover` hover option is set.
    pub fn is_locked(&self) -> bool {
        self.is_locked.get()
    }

    pub fn set_is_locked(&self, value: bool) {
        if self.is_locked.get() == value {
            return;
        }
        self.is_locked.set(value);
        if value {
            self.hover_container.class_list_add("locked");
        } else {
            self.hover_container.class_list_remove("locked");
        }
    }

    /// Adds an element to be tracked by this hover's mouse tracker (hovers aninhados).
    pub fn add_mouse_tracking_element(&self, element: &Rc<ViewElement>) -> DisposableRef {
        self.lock_mouse_tracker.add_element(element)
    }

    /// O `ResizeObserver` do conteúdo HTML: o tamanho mudou.
    pub fn on_content_resized(&self) {
        self.layout();
        self.on_request_layout.fire(());
    }

    /// O `asyncRenderCallback` do markdown: o conteúdo mudou de dimensões depois de renderizado.
    pub fn on_async_markdown_render(&self) {
        self.contents.class_list_add("code-hover-contents");
        self.layout();
        // This changes the dimensions of the hover so trigger a layout
        self.on_request_layout.fire(());
    }

    fn add_focus_trap(&self) {
        if !self.enable_focus_traps || self.added_focus_trap.get() {
            return;
        }
        self.added_focus_trap.set(true);

        // Add a hover tab loop if the hover has at least one element with a valid tabIndex
        let first_container_focus_element = self.container.clone();
        if let Some(last_container_focus_element) = self.host.last_focusable_child(&self.container) {
            let before = ViewElement::new();
            let after = ViewElement::new();
            before.set_parent(Some(self.hover_container.clone()));
            after.set_parent(Some(self.hover_container.clone()));
            let dom = self.dom.clone();
            self.store.add(self.dom.add_listener(
                HoverDomTarget::Element(after),
                HoverDomEventKind::Focus,
                false,
                Rc::new(move |_: &HoverDomEvent| dom.focus(&first_container_focus_element)),
            ));
            let dom = self.dom.clone();
            self.store.add(self.dom.add_listener(
                HoverDomTarget::Element(before),
                HoverDomEventKind::Focus,
                false,
                Rc::new(move |_: &HoverDomEvent| dom.focus(&last_container_focus_element)),
            ));
        }
    }

    pub fn render(&self, container: &Rc<ViewElement>) {
        self.hover_container.set_parent(Some(container.clone()));
        let hover_focused = self.dom.is_ancestor_of_active_element(&self.hover_container);
        if hover_focused {
            if let Some(hint) = self.host.accessible_view_hint() {
                self.host.announce(&hint);
            }
        }
        self.layout();
        self.add_focus_trap();
    }

    // region Geometria do contêiner (o que o DOM devolve)

    fn metrics(&self) -> HoverContentMetrics {
        let max_width = self.container_max_width.get();
        if let Some((cached_width, metrics)) = self.metrics_cache.get() {
            if cached_width == max_width {
                return metrics;
            }
        }
        let metrics = self.host.measure(&HoverMeasureRequest {
            content: &self.content,
            max_width,
            has_status_bar: self.has_status_bar,
            compact: self.compact,
            with_pointer: self.hover_pointer.is_some(),
        });
        self.metrics_cache.set(Some((max_width, metrics)));
        metrics
    }

    /// `containerDomNode.clientWidth`.
    fn client_width(&self) -> f64 {
        self.metrics().width
    }

    /// `contentsDomNode.offsetHeight`.
    fn contents_offset_height(&self) -> f64 {
        let natural = self.metrics().contents_height;
        self.contents_max_height.get().map_or(natural, |max| natural.min(max))
    }

    /// `containerDomNode.offsetHeight`.
    fn offset_height(&self) -> f64 {
        let natural = self.contents_offset_height() + self.metrics().outside_height;
        self.container_max_height.get().map_or(natural, |max| natural.min(max))
    }

    /// `containerDomNode.clientHeight`.
    fn client_height(&self) -> f64 {
        (self.offset_height() - HOVER_BORDER_WIDTH).max(0.0)
    }

    fn set_container_max_width(&self, value: Option<f64>) {
        self.container_max_width.set(value);
        match value {
            Some(value) => self.container.style_set_property("max-width", &format!("{value}px")),
            None => self.container.style_remove_property("max-width"),
        }
    }

    fn set_container_max_height(&self, value: Option<f64>) {
        self.container_max_height.set(value);
        match value {
            Some(value) => self.container.style_set_property("max-height", &format!("{value}px")),
            None => self.container.style_remove_property("max-height"),
        }
    }

    fn set_contents_max_height(&self, value: Option<f64>) {
        self.contents_max_height.set(value);
        match value {
            Some(value) => self.contents.style_set_property("max-height", &format!("{value}px")),
            None => self.contents.style_remove_property("max-height"),
        }
    }

    fn zoom_accounted_rect(&self, element: &Rc<ViewElement>) -> (f64, f64, f64, f64) {
        let zoom = self.host.zoom_level(element);
        let rect = element.rect();
        (rect.top * zoom, rect.left * zoom + rect.width * zoom, rect.top * zoom + rect.height * zoom, rect.left * zoom)
    }

    // endregion

    pub fn layout(&self) {
        // Cancel any pending mouseout timers since the hover is being repositioned (e.g. due to content resize from
        // collapsible sections). The mouse may end up back inside the hover after the layout.
        self.mouse_tracker.suppress_pending_mouse_out();
        if !Rc::ptr_eq(&self.lock_mouse_tracker, &self.mouse_tracker) {
            self.lock_mouse_tracker.suppress_pending_mouse_out();
        }

        self.container.class_list_remove("right-aligned");
        self.set_contents_max_height(None);
        self.set_container_max_width(None);
        self.metrics_cache.set(None);

        let first = self.target.elements.first().expect("o hover tem ao menos um elemento alvo");
        let (mut top, mut right, mut bottom, mut left) = {
            let (top, right, bottom, left) = self.zoom_accounted_rect(first);
            (top, right, bottom, left)
        };
        if let Some(clipping) = &self.clipping_element {
            let (clip_top, clip_right, clip_bottom, clip_left) = self.zoom_accounted_rect(clipping);
            let visible_top = top.max(clip_top);
            let visible_right = right.min(clip_right);
            let visible_bottom = bottom.min(clip_bottom);
            let visible_left = left.max(clip_left);
            // Keep the whole target when none of it is visible
            if visible_left <= visible_right && visible_top <= visible_bottom {
                top = visible_top;
                right = visible_right;
                bottom = visible_bottom;
                left = visible_left;
            }
        }
        let width = right - left;
        let height = bottom - top;

        let mut target_rect = TargetRect { top, right, bottom, left, width, height, center_x: left + (width / 2.0), center_y: top + (height / 2.0) };

        // These calls adjust the position depending on spacing.
        self.adjust_horizontal_hover_position(&target_rect);
        self.adjust_vertical_hover_position(&target_rect);
        // This call limits the maximum height of the hover.
        self.adjust_hover_max_height(&target_rect);

        // Offset the hover position if there is a pointer so it aligns with the target element
        for property in ["padding", "margin", "padding-left", "padding-right", "padding-top", "padding-bottom", "margin-left", "margin-right", "margin-top", "margin-bottom"] {
            self.hover_container.style_remove_property(property);
        }
        if self.hover_pointer.is_some() {
            let pointer = format!("{POINTER_SIZE}px");
            let negative = format!("{}px", -POINTER_SIZE);
            match self.hover_position.get() {
                HoverPosition::Right => {
                    target_rect.left += POINTER_SIZE;
                    target_rect.right += POINTER_SIZE;
                    self.hover_container.style_set_property("padding-left", &pointer);
                    self.hover_container.style_set_property("margin-left", &negative);
                }
                HoverPosition::Left => {
                    target_rect.left -= POINTER_SIZE;
                    target_rect.right -= POINTER_SIZE;
                    self.hover_container.style_set_property("padding-right", &pointer);
                    self.hover_container.style_set_property("margin-right", &negative);
                }
                HoverPosition::Below => {
                    target_rect.top += POINTER_SIZE;
                    target_rect.bottom += POINTER_SIZE;
                    self.hover_container.style_set_property("padding-top", &pointer);
                    self.hover_container.style_set_property("margin-top", &negative);
                }
                HoverPosition::Above => {
                    target_rect.top -= POINTER_SIZE;
                    target_rect.bottom -= POINTER_SIZE;
                    self.hover_container.style_set_property("padding-bottom", &pointer);
                    self.hover_container.style_set_property("margin-bottom", &negative);
                }
            }

            target_rect.center_x = target_rect.left + (width / 2.0);
            target_rect.center_y = target_rect.top + (height / 2.0);
        }

        self.compute_x_coordinate(&target_rect);
        self.compute_y_coordinate(&target_rect);

        if let Some(pointer) = &self.hover_pointer {
            // reset
            for class_name in ["top", "left", "right", "bottom"] {
                pointer.class_list_remove(class_name);
            }
            self.set_hover_pointer_position(pointer, &target_rect);
        }
        self.host.on_contents_changed();
    }

    fn compute_x_coordinate(&self, target: &TargetRect) {
        let hover_width = self.client_width() + HOVER_BORDER_WIDTH;
        let document_element_client_left = self.host.document_client_left();
        let position = self.hover_position.get();

        if let Some(x) = self.target.x {
            self.x.set(x);
        } else if position == HoverPosition::Right {
            self.x.set(target.right);
        } else if position == HoverPosition::Left {
            self.x.set(target.left - hover_width);
        } else {
            if self.anchor_alignment == AnchorAlignment::Right {
                self.x.set(target.right - hover_width);
            } else if self.hover_pointer.is_some() {
                self.x.set(target.center_x - (self.client_width() / 2.0));
            } else {
                self.x.set(target.left);
            }

            // Hover is going beyond window towards right end
            let document_element_client_width = self.host.document_client_width();
            if self.x.get() + hover_width >= document_element_client_width {
                self.container.class_list_add("right-aligned");
                self.x.set((document_element_client_width - hover_width - HOVER_WINDOW_EDGE_MARGIN).max(document_element_client_left));
            }
        }

        // Hover is going beyond window towards left end
        if self.x.get() < document_element_client_left {
            self.x.set(target.left + HOVER_WINDOW_EDGE_MARGIN);
        }
    }

    fn compute_y_coordinate(&self, target: &TargetRect) {
        let position = self.hover_position.get();
        if let Some(y) = self.target.y {
            self.y.set(y);
        } else if position == HoverPosition::Above {
            self.y.set(target.top);
        } else if position == HoverPosition::Below {
            self.y.set(target.bottom - 2.0);
        } else if self.hover_pointer.is_some() {
            self.y.set(target.center_y + (self.client_height() / 2.0));
        } else {
            self.y.set(target.bottom);
        }

        // Hover on bottom is going beyond window
        if self.y.get() > self.host.window_inner_height() {
            self.y.set(target.bottom);
        }
    }

    fn adjust_horizontal_hover_position(&self, target: &TargetRect) {
        // Do not adjust horizontal hover position if x cordiante is provided
        if self.target.x.is_some() {
            return;
        }
        let position = self.hover_position.get();
        let document_client_left = self.host.document_client_left();
        let document_client_width = self.host.document_client_width();

        if self.anchor_alignment == AnchorAlignment::Right && (position == HoverPosition::Above || position == HoverPosition::Below) {
            let available_width = target.right - document_client_left - HOVER_WINDOW_EDGE_MARGIN - HOVER_BORDER_WIDTH;
            if self.client_width() > available_width {
                self.set_container_max_width(Some(available_width.max(0.0)));
            }
            return;
        }

        let hover_pointer_offset = if self.hover_pointer.is_some() { POINTER_SIZE } else { 0.0 };

        // When force position is enabled, restrict max width
        if self.force_position {
            let padding = hover_pointer_offset + HOVER_BORDER_WIDTH;
            if position == HoverPosition::Right {
                self.set_container_max_width(Some(document_client_width - target.right - padding));
            } else if position == HoverPosition::Left {
                self.set_container_max_width(Some(target.left - padding));
            }
            return;
        }

        // Position hover on right to target
        if position == HoverPosition::Right {
            let room_on_right = document_client_width - target.right;
            // Hover on the right is going beyond window.
            if room_on_right < self.client_width() + hover_pointer_offset {
                let room_on_left = target.left;
                // There's enough room on the left, flip the hover position
                if room_on_left >= self.client_width() + hover_pointer_offset {
                    self.hover_position.set(HoverPosition::Left);
                }
                // Hover on the left would go beyond window too
                else {
                    self.hover_position.set(HoverPosition::Below);
                }
            }
        }
        // Position hover on left to target
        else if position == HoverPosition::Left {
            let room_on_left = target.left;
            // Hover on the left is going beyond window.
            if room_on_left < self.client_width() + hover_pointer_offset {
                let room_on_right = document_client_width - target.right;
                // There's enough room on the right, flip the hover position
                if room_on_right >= self.client_width() + hover_pointer_offset {
                    self.hover_position.set(HoverPosition::Right);
                }
                // Hover on the right would go beyond window too
                else {
                    self.hover_position.set(HoverPosition::Below);
                }
            }
            // Hover on the left is going beyond window.
            if target.left - self.client_width() - hover_pointer_offset <= document_client_left {
                self.hover_position.set(HoverPosition::Right);
            }
        }
    }

    fn adjust_vertical_hover_position(&self, target: &TargetRect) {
        // Do not adjust vertical hover position if the y coordinate is provided or the position is forced
        if self.target.y.is_some() || self.force_position {
            return;
        }

        let hover_pointer_offset = if self.hover_pointer.is_some() { POINTER_SIZE } else { 0.0 };

        match self.hover_position.get() {
            // Position hover on top of the target
            HoverPosition::Above => {
                // Hover on top is going beyond window
                if target.top - self.client_height() - hover_pointer_offset < 0.0 {
                    self.hover_position.set(HoverPosition::Below);
                }
            }
            // Position hover below the target
            HoverPosition::Below => {
                // Hover on bottom is going beyond window
                if target.bottom + self.offset_height() + hover_pointer_offset > self.host.window_inner_height() {
                    self.hover_position.set(HoverPosition::Above);
                }
            }
            HoverPosition::Left | HoverPosition::Right => {}
        }
    }

    fn adjust_hover_max_height(&self, target: &TargetRect) {
        let window_inner_height = self.host.window_inner_height();
        let mut max_height = window_inner_height * self.max_height_ratio_relative_to_window;

        // When force position is enabled, restrict max height
        if self.force_position {
            let padding = (if self.hover_pointer.is_some() { POINTER_SIZE } else { 0.0 }) + HOVER_BORDER_WIDTH;
            match self.hover_position.get() {
                HoverPosition::Above => max_height = max_height.min(target.top - padding),
                HoverPosition::Below => max_height = max_height.min(window_inner_height - target.bottom - padding),
                HoverPosition::Left | HoverPosition::Right => {}
            }
        }

        self.set_container_max_height(None);
        let height_outside_contents = self.offset_height() - self.contents_offset_height();
        let contents_max_height = (max_height - height_outside_contents).max(0.0);

        self.set_container_max_height(Some(max_height));
        self.set_contents_max_height(Some(contents_max_height));
        if self.contents_offset_height() < self.metrics().contents_height {
            // Add padding for a vertical scrollbar
            let extra_right_padding = self.metrics().vertical_scrollbar_size;
            if self.contents_padding_right.get() != Some(extra_right_padding) {
                self.contents_padding_right.set(Some(extra_right_padding));
                self.contents.style_set_property("padding-right", &format!("{extra_right_padding}px"));
            }
        }
    }

    fn set_hover_pointer_position(&self, pointer: &Rc<ViewElement>, target: &TargetRect) {
        match self.hover_position.get() {
            position @ (HoverPosition::Left | HoverPosition::Right) => {
                pointer.class_list_add(if position == HoverPosition::Left { "right" } else { "left" });
                let hover_height = self.client_height();

                // If hover is taller than target, then show the pointer at the center of target
                if hover_height > target.height {
                    pointer.style_set_property("top", &format!("{}px", target.center_y - (self.y.get() - hover_height) - POINTER_SIZE));
                }
                // Otherwise show the pointer at the center of hover, unrounded so it stays centred
                else {
                    pointer.style_set_property("top", &format!("{}px", (hover_height / 2.0) - POINTER_SIZE));
                }
            }
            position @ (HoverPosition::Above | HoverPosition::Below) => {
                pointer.class_list_add(if position == HoverPosition::Above { "bottom" } else { "top" });
                let hover_width = self.client_width();

                // Position pointer at the center of the hover, unrounded so it stays centred
                let mut pointer_left_position = (hover_width / 2.0) - POINTER_SIZE;

                // If pointer goes beyond target then position it at the center of the target
                let pointer_x = self.x.get() + pointer_left_position;
                if pointer_x < target.left || pointer_x > target.right {
                    pointer_left_position = target.center_x - self.x.get() - POINTER_SIZE;
                }

                pointer.style_set_property("left", &format!("{pointer_left_position}px"));
            }
        }
    }

    pub fn focus(&self) {
        self.dom.focus(&self.container);
    }

    pub fn hide(&self) {
        self.dispose();
    }
}

impl IDisposable for HoverWidget {
    fn dispose(&self) {
        if !self.is_disposed.get() {
            self.on_dispose.fire(());
            self.hover_container.set_parent(None);
            self.mouse_tracker.dispose();
            self.lock_mouse_tracker.dispose();
            self.store.dispose();
            self.on_dispose.dispose();
            self.on_request_layout.dispose();
        }
        self.is_disposed.set(true);
    }
}

impl IHoverWidget for HoverWidget {
    fn is_disposed(&self) -> bool {
        HoverWidget::is_disposed(self)
    }
}

impl IHoverWidgetHandle for HoverWidget {
    fn dom_node(&self) -> Rc<ViewElement> {
        HoverWidget::dom_node(self)
    }

    fn is_locked(&self) -> bool {
        HoverWidget::is_locked(self)
    }

    fn set_is_locked(&self, locked: bool) {
        HoverWidget::set_is_locked(self, locked);
    }

    fn is_mouse_in(&self) -> bool {
        HoverWidget::is_mouse_in(self)
    }

    fn on_dispose(&self) -> Event<()> {
        HoverWidget::on_dispose(self)
    }

    fn on_request_layout(&self) -> Event<()> {
        HoverWidget::on_request_layout(self)
    }

    fn add_mouse_tracking_element(&self, element: &Rc<ViewElement>) -> DisposableRef {
        HoverWidget::add_mouse_tracking_element(self, element)
    }

    fn render(&self, container: &Rc<ViewElement>) {
        HoverWidget::render(self, container);
    }

    fn focus(&self) {
        HoverWidget::focus(self);
    }

    fn layout(&self) {
        HoverWidget::layout(self);
    }

    fn anchor(&self) -> AnchorPosition {
        HoverWidget::anchor(self)
    }

    fn x(&self) -> f64 {
        HoverWidget::x(self)
    }

    fn y(&self) -> f64 {
        HoverWidget::y(self)
    }
}

/// `createInstance(HoverWidget, options)`.
pub struct HoverWidgetFactory {
    dom: Rc<dyn IHoverDom>,
    host: Rc<dyn IHoverWidgetHost>,
}

impl HoverWidgetFactory {
    pub fn new(dom: Rc<dyn IHoverDom>, host: Rc<dyn IHoverWidgetHost>) -> Self {
        Self { dom, host }
    }
}

impl HoverWidgetFactory {
    /// O `createInstance(HoverWidget, options)` com o tipo concreto (o ponteiro e o conteúdo, que o handle não expõe).
    pub fn create_widget(&self, options: &mut IHoverOptions) -> Rc<HoverWidget> {
        let widget = HoverWidget::new(options, self.dom.clone(), self.host.clone());
        // O `offsetWidth` e o `offsetHeight` do `domNode` são os do contêiner medido.
        let weak = Rc::downgrade(&widget);
        widget.container.set_measure(Rc::new(move || match weak.upgrade() {
            Some(widget) => ISize { width: widget.client_width() + HOVER_BORDER_WIDTH, height: widget.offset_height() },
            None => ISize { width: 0.0, height: 0.0 },
        }));
        widget
    }
}

impl IHoverWidgetFactory for HoverWidgetFactory {
    fn create(&self, options: &mut IHoverOptions) -> Rc<dyn IHoverWidgetHandle> {
        self.create_widget(options)
    }
}

/// `CompositeMouseTracker`: junta o entra e sai do mouse de vários elementos, com um atraso antes de avisar a saída.
pub struct CompositeMouseTracker {
    this: Weak<CompositeMouseTracker>,
    dom: Rc<dyn IHoverDom>,
    elements: RefCell<Vec<Rc<ViewElement>>>,
    event_debounce_delay: u64,
    is_mouse_in: Cell<bool>,
    suppress_next_mouse_out: Cell<bool>,
    mouse_timer: RefCell<Option<Rc<TimeoutTimer>>>,
    on_mouse_out: Rc<Emitter<()>>,
    store: Rc<DisposableStore>,
}

impl CompositeMouseTracker {
    /// `elements`: os elementos a rastrear. `event_debounce_delay`: o atraso em ms antes de disparar a saída, para dar tempo
    /// ao mouse de entrar no hover ou num alvo próximo (passar pela barra de rolagem não esconde o hover na hora).
    pub fn new(dom: Rc<dyn IHoverDom>, elements: Vec<Rc<ViewElement>>, event_debounce_delay: u64) -> Rc<Self> {
        let tracker = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            dom,
            elements: RefCell::new(elements.clone()),
            event_debounce_delay,
            is_mouse_in: Cell::new(true),
            suppress_next_mouse_out: Cell::new(false),
            mouse_timer: RefCell::new(None),
            on_mouse_out: Emitter::new(),
            store: DisposableStore::new(),
        });
        for element in &elements {
            tracker.store.add(tracker.listen_to(element));
        }
        tracker
    }

    fn listen_to(&self, element: &Rc<ViewElement>) -> DisposableRef {
        let store = DisposableStore::new();
        let weak = self.this.clone();
        store.add(self.dom.add_listener(
            HoverDomTarget::Element(element.clone()),
            HoverDomEventKind::MouseOver,
            false,
            Rc::new(move |_: &HoverDomEvent| {
                if let Some(tracker) = weak.upgrade() {
                    tracker.on_target_mouse_over();
                }
            }),
        ));
        let weak = self.this.clone();
        store.add(self.dom.add_listener(
            HoverDomTarget::Element(element.clone()),
            HoverDomEventKind::MouseLeave,
            false,
            Rc::new(move |_: &HoverDomEvent| {
                if let Some(tracker) = weak.upgrade() {
                    tracker.on_target_mouse_leave();
                }
            }),
        ));
        store
    }

    pub fn on_mouse_out(&self) -> Event<()> {
        self.on_mouse_out.event()
    }

    pub fn is_mouse_in(&self) -> bool {
        self.is_mouse_in.get()
    }

    fn clear_timer(&self) {
        let timer = self.mouse_timer.borrow_mut().take();
        if let Some(timer) = timer {
            timer.dispose();
        }
    }

    fn on_target_mouse_over(&self) {
        self.is_mouse_in.set(true);
        self.suppress_next_mouse_out.set(false);
        self.clear_timer();
    }

    fn on_target_mouse_leave(&self) {
        self.is_mouse_in.set(false);
        // Evaluate whether the mouse is still outside asynchronously such that other mouse targets have the opportunity to
        // first their mouse in event.
        let weak = self.this.clone();
        let timer = TimeoutTimer::with_runner(
            move || {
                if let Some(tracker) = weak.upgrade() {
                    tracker.fire_if_mouse_outside();
                }
            },
            self.event_debounce_delay,
        );
        let previous = self.mouse_timer.borrow_mut().replace(timer);
        if let Some(previous) = previous {
            previous.dispose();
        }
    }

    fn fire_if_mouse_outside(&self) {
        if !self.is_mouse_in.get() && !self.suppress_next_mouse_out.get() {
            self.on_mouse_out.fire(());
        }
    }

    /// Suppresses the next pending mouseout dismissal. Call this when tracked elements are being resized or repositioned to
    /// avoid spurious dismissals caused by the element shrinking away from the cursor. The suppression is cleared when the
    /// mouse next enters a tracked element.
    pub fn suppress_pending_mouse_out(&self) {
        if !self.is_mouse_in.get() {
            self.suppress_next_mouse_out.set(true);
        }
    }

    /// Adds an element to be tracked by this mouse tracker. Mouse events on this element will be considered as being "inside"
    /// the tracked area.
    pub fn add_element(&self, element: &Rc<ViewElement>) -> DisposableRef {
        if self.elements.borrow().iter().any(|existing| Rc::ptr_eq(existing, element)) {
            return to_disposable(|| {});
        }
        self.elements.borrow_mut().push(element.clone());
        let store = DisposableStore::new();
        store.add(self.listen_to(element));
        let weak = self.this.clone();
        let element = element.clone();
        store.add(to_disposable(move || {
            if let Some(tracker) = weak.upgrade() {
                tracker.elements.borrow_mut().retain(|existing| !Rc::ptr_eq(existing, &element));
            }
        }));
        store
    }
}

impl IDisposable for CompositeMouseTracker {
    fn dispose(&self) {
        self.store.dispose();
        self.clear_timer();
        self.on_mouse_out.dispose();
    }
}
