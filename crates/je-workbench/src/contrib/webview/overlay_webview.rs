//! Porte de `src/vs/workbench/contrib/webview/browser/overlayWebview.ts`: a sobreposição que flutua sobre o elemento
//! âncora e, a partir da âncora, mantém as classes `webview-overlay-modal` e `webview-overlay-outer-*` do conteúdo.
//!
//! Recorte declarado (sem DOM, a árvore de widgets é nativa):
//! - Os elementos do DOM de que `setAnchorElement` depende viram modelos geométricos: [`OverlayPart`] (um elemento com
//!   `className`, caixa de borda e `clientLeft`/`clientTop`/`clientWidth`/`clientHeight`, mais o `MutationObserver` de
//!   `class`), [`OverlayAnchor`] (a âncora, com a cadeia de ancestrais que o `closest('.part')` percorre) e
//!   [`OverlayRoot`] (o contêiner do workbench, com as classes e as variáveis CSS que decidem o raio dos cantos).
//! - `OverlayLayoutElement` é o [`OverlayContent`]: as classes do conteúdo, a visibilidade e a âncora corrente. O
//!   `getComputedStyle(...).borderBottom*Radius` do conteúdo é [`OverlayWebview::computed_bottom_corner_radii`], que aplica as
//!   regras de `floatingPanels.css` sobre as classes do contêiner e do conteúdo (a especificação, não CSS executado).
//! - O `MutationObserver` entrega os registros numa microtarefa; no modelo a entrega é [`OverlayPart::deliver_mutation_records`],
//!   o ponto em que o original espera (`await timeout(0)`).
//! - `claim`, `release`, `_show`, o elemento interno de webview (`IWebviewElement`), as chaves de contexto da busca e o
//!   encaminhamento de eventos dependem de `IWebviewService.createWebviewElement`, que é recorte da Fase 6 (ver
//!   `browser_webview.rs`). Ficam o ciclo de vida, o `container`, a âncora e as classes de borda.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable, to_disposable};
use je_base::uuid::generate_uuid;

use super::browser_webview::{WebviewContentOptions, WebviewExtensionDescription, WebviewInitInfo, WebviewOptions};

/// Os quatro lados de um `DOMRect`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// Um dos quatro lados, na ordem em que `_syncOuterEdgeClasses` os percorre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    pub const ALL: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];

    pub fn name(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Right => "right",
            Edge::Top => "top",
            Edge::Bottom => "bottom",
        }
    }

    fn of(self, rect: &Rect) -> f64 {
        match self {
            Edge::Left => rect.left,
            Edge::Right => rect.right,
            Edge::Top => rect.top,
            Edge::Bottom => rect.bottom,
        }
    }
}

/// A caixa de um elemento: a caixa de borda e a espessura da borda (que dá `clientLeft`/`clientTop`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct BoxGeometry {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    border: f64,
}

/// Um elemento do DOM com `className`, caixa e o observador de mutação de `class`.
pub struct OverlayPart {
    class_name: RefCell<String>,
    geometry: Cell<BoxGeometry>,
    observers: RefCell<Vec<(u64, Rc<dyn Fn()>)>>,
    next_observer_id: Cell<u64>,
    mutation_pending: Cell<bool>,
}

impl OverlayPart {
    /// Um elemento fora do documento: todas as medidas são zero, como no navegador.
    pub fn new(class_name: &str) -> Rc<Self> {
        Rc::new(Self {
            class_name: RefCell::new(class_name.to_string()),
            geometry: Cell::new(BoxGeometry::default()),
            observers: RefCell::new(Vec::new()),
            next_observer_id: Cell::new(0),
            mutation_pending: Cell::new(false),
        })
    }

    /// Posiciona o elemento no documento (`getBoundingClientRect` e `clientLeft`/`clientTop`/`clientWidth`/`clientHeight`).
    pub fn set_geometry(&self, left: f64, top: f64, width: f64, height: f64, border: f64) {
        self.geometry.set(BoxGeometry { left, top, width, height, border });
    }

    /// `className` (getter).
    pub fn class_name(&self) -> String {
        self.class_name.borrow().clone()
    }

    /// `className` (setter): enfileira o registro de mutação de quem observa `class`.
    pub fn set_class_name(&self, class_name: &str) {
        *self.class_name.borrow_mut() = class_name.to_string();
        if !self.observers.borrow().is_empty() {
            self.mutation_pending.set(true);
        }
    }

    /// `classList.contains(name)`.
    pub fn contains_class(&self, name: &str) -> bool {
        self.class_name.borrow().split_whitespace().any(|class| class == name)
    }

    /// `getBoundingClientRect()`.
    pub fn bounding_client_rect(&self) -> Rect {
        let g = self.geometry.get();
        Rect { left: g.left, top: g.top, right: g.left + g.width, bottom: g.top + g.height }
    }

    fn client_left(&self) -> f64 {
        self.geometry.get().border
    }

    fn client_top(&self) -> f64 {
        self.geometry.get().border
    }

    fn client_width(&self) -> f64 {
        let g = self.geometry.get();
        (g.width - 2.0 * g.border).max(0.0)
    }

    fn client_height(&self) -> f64 {
        let g = self.geometry.get();
        (g.height - 2.0 * g.border).max(0.0)
    }

    /// `new MutationObserver(callback)` + `observe(part, { attributes: true, attributeFilter: ['class'] })`; o disposable é o
    /// `disconnect()`.
    pub fn observe_class_changes(self: &Rc<Self>, callback: Rc<dyn Fn()>) -> DisposableRef {
        let id = self.next_observer_id.get();
        self.next_observer_id.set(id + 1);
        self.observers.borrow_mut().push((id, callback));
        let part = Rc::downgrade(self);
        to_disposable(move || {
            if let Some(part) = part.upgrade() {
                part.observers.borrow_mut().retain(|(observer_id, _)| *observer_id != id);
            }
        })
    }

    /// A entrega dos registros de mutação pendentes (a microtarefa do `MutationObserver`).
    pub fn deliver_mutation_records(&self) {
        if !self.mutation_pending.replace(false) {
            return;
        }
        let callbacks: Vec<Rc<dyn Fn()>> = self.observers.borrow().iter().map(|(_, callback)| callback.clone()).collect();
        for callback in callbacks {
            callback();
        }
    }
}

/// Posição `absolute` de uma âncora, em porcentagem da caixa de preenchimento do ancestral posicionado.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

/// O elemento âncora. `ancestors` é a cadeia que `closest` percorre, do pai para cima; o primeiro é o ancestral posicionado
/// de que as porcentagens dependem.
pub struct OverlayAnchor {
    ancestors: Vec<Rc<OverlayPart>>,
    bounds: Cell<Rect>,
    placement: Cell<Option<Placement>>,
}

impl OverlayAnchor {
    /// Uma âncora com medidas zero (elemento fora do documento).
    pub fn new(ancestors: Vec<Rc<OverlayPart>>) -> Rc<Self> {
        Rc::new(Self { ancestors, bounds: Cell::new(Rect::default()), placement: Cell::new(None) })
    }

    /// `style.cssText = 'position: absolute; left: L%; top: T%; width: W%; height: H%;'`.
    pub fn place_absolute(&self, left: f64, top: f64, width: f64, height: f64) {
        self.placement.set(Some(Placement { left, top, width, height }));
    }

    /// `style.top = 'T%'`.
    pub fn set_top_percent(&self, top: f64) {
        if let Some(placement) = self.placement.get() {
            self.placement.set(Some(Placement { top, ..placement }));
        }
    }

    /// `getBoundingClientRect()`.
    pub fn bounding_client_rect(&self) -> Rect {
        let (Some(placement), Some(parent)) = (self.placement.get(), self.ancestors.first()) else {
            return self.bounds.get();
        };
        let origin_left = parent.bounding_client_rect().left + parent.client_left();
        let origin_top = parent.bounding_client_rect().top + parent.client_top();
        let left = origin_left + parent.client_width() * placement.left / 100.0;
        let top = origin_top + parent.client_height() * placement.top / 100.0;
        Rect {
            left,
            top,
            right: left + parent.client_width() * placement.width / 100.0,
            bottom: top + parent.client_height() * placement.height / 100.0,
        }
    }

    /// `closest('.part')`.
    pub fn closest_part(&self) -> Option<Rc<OverlayPart>> {
        self.ancestors.iter().find(|ancestor| ancestor.contains_class("part")).cloned()
    }
}

/// O contêiner do workbench (`getContainer`): as classes e as variáveis CSS do `style` inline.
pub struct OverlayRoot {
    class_name: RefCell<String>,
    properties: RefCell<Vec<(String, String)>>,
}

impl OverlayRoot {
    pub fn new(class_name: &str) -> Rc<Self> {
        Rc::new(Self { class_name: RefCell::new(class_name.to_string()), properties: RefCell::new(Vec::new()) })
    }

    pub fn has_class(&self, name: &str) -> bool {
        self.class_name.borrow().split_whitespace().any(|class| class == name)
    }

    /// `style.cssText = '--a: 1px; --b: 2;'`: troca todas as declarações.
    pub fn set_css_text(&self, css_text: &str) {
        let declarations = css_text
            .split(';')
            .filter_map(|declaration| declaration.split_once(':'))
            .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
            .collect();
        *self.properties.borrow_mut() = declarations;
    }

    /// `style.setProperty(name, value)`.
    pub fn set_property(&self, name: &str, value: &str) {
        let mut properties = self.properties.borrow_mut();
        match properties.iter_mut().find(|(existing, _)| existing == name) {
            Some(entry) => entry.1 = value.to_string(),
            None => properties.push((name.to_string(), value.to_string())),
        }
    }

    /// O valor numérico de uma variável, com o `px` opcional.
    fn number(&self, name: &str) -> Option<f64> {
        let properties = self.properties.borrow();
        let (_, value) = properties.iter().find(|(existing, _)| existing == name)?;
        value.trim_end_matches("px").trim().parse().ok()
    }

    /// `var(--vscode-cornerRadius-large, 8px)`.
    fn corner_radius_large(&self) -> f64 {
        self.number("--vscode-cornerRadius-large").unwrap_or(8.0)
    }

    /// `--window-corner-radius` de `style.css`: o quadro nativo não muda com o zoom (`10px` no mac, `16px` no Tahoe).
    fn window_corner_radius(&self) -> Option<f64> {
        let zoom = self.number("--window-zoom-factor").unwrap_or(1.0);
        if self.has_class("mac") && self.has_class("macos-tahoe") {
            Some(16.0 / zoom)
        } else if self.has_class("mac") {
            Some(10.0 / zoom)
        } else {
            None
        }
    }

    /// `var(--window-border-width, 0px)`: o `.border:not(.fullscreen)` de `style.css` define 1px.
    fn window_border_width(&self) -> f64 {
        self.number("--window-border-width").unwrap_or(if self.has_class("border") && !self.has_class("fullscreen") { 1.0 } else { 0.0 })
    }
}

/// O conteúdo da sobreposição (`OverlayLayoutElement.content`): as classes, a visibilidade e a âncora corrente.
pub struct OverlayContent {
    classes: RefCell<Vec<String>>,
    visibility: RefCell<String>,
    anchor: RefCell<Option<(Rc<OverlayAnchor>, Option<Rc<OverlayPart>>)>>,
}

impl OverlayContent {
    fn new() -> Rc<Self> {
        Rc::new(Self { classes: RefCell::new(Vec::new()), visibility: RefCell::new(String::new()), anchor: RefCell::new(None) })
    }

    /// `classList.contains(name)`.
    pub fn contains_class(&self, name: &str) -> bool {
        self.classes.borrow().iter().any(|class| class == name)
    }

    fn add_class(&self, name: &str) {
        if !self.contains_class(name) {
            self.classes.borrow_mut().push(name.to_string());
        }
    }

    /// `classList.toggle(name, force)`.
    fn toggle_class(&self, name: &str, force: bool) {
        if force {
            self.add_class(name);
        } else {
            self.classes.borrow_mut().retain(|class| class != name);
        }
    }

    /// `style.visibility`.
    pub fn visibility(&self) -> String {
        self.visibility.borrow().clone()
    }

    fn set_anchor_element(&self, anchor: Rc<OverlayAnchor>, clipping_container: Option<Rc<OverlayPart>>) {
        *self.anchor.borrow_mut() = Some((anchor, clipping_container));
    }
}

/// `_anchorState` (o `clippingContainer` que o original guarda também só é relido por `claim`, fora do recorte).
struct AnchorState {
    anchor_element: Rc<OverlayAnchor>,
}

/// Webview that is absolutely positioned over another element (ver o recorte no cabeçalho do módulo).
pub struct OverlayWebview {
    base: Rc<Disposable>,
    weak_self: Weak<OverlayWebview>,
    layout_root: Rc<OverlayRoot>,

    provided_view_type: Option<String>,
    origin: String,
    title: RefCell<Option<String>>,
    extension: RefCell<Option<WebviewExtensionDescription>>,
    content_options: RefCell<WebviewContentOptions>,
    options: RefCell<WebviewOptions>,

    overlay_layout: RefCell<Option<Rc<OverlayContent>>>,
    anchor_state: RefCell<Option<AnchorState>>,
    outer_edge_part: RefCell<Option<Rc<OverlayPart>>>,
    outer_edge_observer: Rc<MutableDisposable<dyn IDisposable>>,
    is_disposed: Cell<bool>,
}

impl OverlayWebview {
    pub fn new(init_info: WebviewInitInfo, layout_root: Rc<OverlayRoot>) -> Rc<Self> {
        let base = Disposable::new();
        let outer_edge_observer = MutableDisposable::<dyn IDisposable>::new();
        base.register(outer_edge_observer.clone());
        Rc::new_cyclic(|weak_self| Self {
            base,
            weak_self: weak_self.clone(),
            layout_root,
            provided_view_type: init_info.provided_view_type,
            origin: init_info.origin.unwrap_or_else(generate_uuid),
            title: RefCell::new(init_info.title),
            extension: RefCell::new(init_info.extension),
            content_options: RefCell::new(init_info.content_options),
            options: RefCell::new(init_info.options),
            overlay_layout: RefCell::new(None),
            anchor_state: RefCell::new(None),
            outer_edge_part: RefCell::new(None),
            outer_edge_observer,
            is_disposed: Cell::new(false),
        })
    }

    pub fn provided_view_type(&self) -> Option<String> {
        self.provided_view_type.clone()
    }

    pub fn origin(&self) -> String {
        self.origin.clone()
    }

    pub fn title(&self) -> Option<String> {
        self.title.borrow().clone()
    }

    pub fn extension(&self) -> Option<WebviewExtensionDescription> {
        self.extension.borrow().clone()
    }

    pub fn content_options(&self) -> WebviewContentOptions {
        self.content_options.borrow().clone()
    }

    pub fn options(&self) -> WebviewOptions {
        self.options.borrow().clone()
    }

    /// `get container()`.
    pub fn container(&self) -> Rc<OverlayContent> {
        if self.is_disposed.get() {
            errors::throw(Error::new("OverlayWebview has been disposed"));
        }
        self.overlay_layout()
    }

    /// `get overlayLayout()`: cria o conteúdo na primeira leitura.
    fn overlay_layout(&self) -> Rc<OverlayContent> {
        if let Some(layout) = self.overlay_layout.borrow().clone() {
            return layout;
        }
        let layout = OverlayContent::new();
        *layout.visibility.borrow_mut() = "hidden".to_string();
        // Marker used by floating panels (Modern UI) CSS to round the overlay content.
        layout.add_class("webview-overlay-content");
        *self.overlay_layout.borrow_mut() = Some(layout.clone());
        layout
    }

    /// `setAnchorElement(anchorElement, clippingContainer?)`.
    pub fn set_anchor_element(&self, anchor_element: &Rc<OverlayAnchor>, clipping_container: Option<Rc<OverlayPart>>) {
        *self.anchor_state.borrow_mut() = Some(AnchorState { anchor_element: anchor_element.clone() });
        // Force the overlay layout to be created if it doesn't exist
        self.overlay_layout().set_anchor_element(anchor_element.clone(), clipping_container);

        let part = anchor_element.closest_part();
        self.sync_outer_edge_classes(part.as_ref());

        let same_part = match (&part, &*self.outer_edge_part.borrow()) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same_part {
            self.outer_edge_observer.clear();
            *self.outer_edge_part.borrow_mut() = part.clone();

            if let Some(part) = part {
                let weak_self = self.weak_self.clone();
                let observed = part.clone();
                let observer = part.observe_class_changes(Rc::new(move || {
                    if let Some(this) = weak_self.upgrade() {
                        this.sync_outer_edge_classes(Some(&observed));
                    }
                }));
                self.outer_edge_observer.set(Some(observer));
            }
        }
    }

    /// `_syncOuterEdgeClasses(part)`.
    fn sync_outer_edge_classes(&self, part: Option<&Rc<OverlayPart>>) {
        let content = self.overlay_layout();
        content.toggle_class("webview-overlay-modal", part.is_some_and(|part| part.contains_class("modal-editor-part")));
        let anchor_bounds = self.anchor_state.borrow().as_ref().map(|state| state.anchor_element.bounding_client_rect());
        let content_bounds = part.map(|part| {
            let bounds = part.bounding_client_rect();
            Rect {
                left: bounds.left + part.client_left(),
                top: bounds.top + part.client_top(),
                right: bounds.left + part.client_left() + part.client_width(),
                bottom: bounds.top + part.client_top() + part.client_height(),
            }
        });

        for edge in Edge::ALL {
            let is_outer_edge = match (&anchor_bounds, &content_bounds, part) {
                (Some(anchor), Some(content), Some(part)) => {
                    (edge.of(anchor) - edge.of(content)).abs() <= 1.0
                        && (part.contains_class(&format!("floating-part-outer-{}", edge.name()))
                            || part.contains_class(&format!("floating-editor-outer-{}", edge.name())))
                }
                _ => false,
            };
            content.toggle_class(&format!("webview-overlay-outer-{}", edge.name()), is_outer_edge);
        }
    }

    /// `getComputedStyle(container).borderBottomLeftRadius` e `borderBottomRightRadius`, pelas regras de `floatingPanels.css`.
    pub fn computed_bottom_corner_radii(&self) -> (String, String) {
        let content = self.container();
        let root = &self.layout_root;
        let px = |value: f64| format!("{value}px");
        if !(root.has_class("monaco-workbench") && root.has_class("floating-panels")) {
            return (px(0.0), px(0.0));
        }

        let modal = content.contains_class("webview-overlay-modal");
        let compact = root.has_class("modern-ui-compact");
        let base = if compact && !modal { 0.0 } else { root.corner_radius_large() };
        let (mut bottom_left, mut bottom_right) = (base, base);

        // Window-facing curves are concentric with the native frame, not the internal card corners.
        let window_facing = root.has_class("mac")
            && root.has_class("nostatusbar")
            && !root.has_class("web")
            && !root.has_class("fullscreen")
            && !compact;
        if window_facing && !modal {
            let stroke = root.number("--vscode-strokeThickness");
            let outer_margin = root.number("--vscode-spacing-size40");
            let window_radius = root.window_corner_radius().zip(outer_margin).map(|(radius, margin)| (radius - root.window_border_width() - margin).max(0.0));
            // Uma variável sem valor invalida a declaração no cálculo, e o raio cai no valor inicial (zero).
            let inner = window_radius.zip(stroke).map_or(0.0, |(radius, stroke)| (radius - stroke).max(0.0));
            if content.contains_class("webview-overlay-outer-left") && content.contains_class("webview-overlay-outer-bottom") {
                bottom_left = inner;
            }
            if content.contains_class("webview-overlay-outer-right") && content.contains_class("webview-overlay-outer-bottom") {
                bottom_right = inner;
            }
        }
        (px(bottom_left), px(bottom_right))
    }
}

impl IDisposable for OverlayWebview {
    fn dispose(&self) {
        if self.is_disposed.replace(true) {
            return;
        }
        *self.overlay_layout.borrow_mut() = None;
        self.base.dispose();
    }
}
