//! Porte de `src/vs/base/browser/ui/splitview/splitview.ts`: o [`SplitView`], um layout flex
//! unidimensional para uma lista de [`SplitViewView`] com tamanho mínimo e máximo, prioridade e snap.
//!
//! A lógica é a do original linha a linha: o algoritmo de redistribuição (`resize`, com as listas de
//! prioridade baixa e alta e o snap), a distribuição do espaço vazio, as proporções, o arraste dos sashes
//! (inclusive com Alt), a visibilidade das views e os modos de [`Sizing`]. O que no original mexe em
//! `HTMLElement` vira nós do [`je_ui`], com a mesma estrutura de classes:
//!
//! ```text
//! .monaco-split-view2 (.vertical | .horizontal)
//!   .sash-container            os sashes, por cima das views (o z-index 35 do sash)
//!   .monaco-scrollable-element
//!     .split-view-container
//!       .split-view-view*      um por view, com `top/height` (ou `left/width`) do layout
//! ```
//!
//! Regras de CSS que viram código aqui: `width/height: 100%` dos contêineres ([`je_ui::Tree::set_fill`]),
//! `.split-view-view:not(.visible) { display: none }` (`NodeStyle::visible`), `pointer-events: none` do
//! `.sash-container` com `initial` nos sashes, e a linha de separação (`::before` de cada view menos a
//! primeira) como um nó filho por view.
//!
//! Divergências:
//!
//! - O `.monaco-scrollable-element` é só o nó com recorte (`overflow: hidden`): o `SmoothScrollableElement` e
//!   o `Scrollable` não estão portados, então a barra de rolagem, o `scrollbarVisibility` e o `onDidScroll`
//!   (um evento que nunca dispara) ficam para quando o `scrollableElement` for portado.
//! - O `layout` das views itera uma cópia da lista de views, não a lista viva do original; só difere se uma
//!   view mexer no próprio `SplitView` dentro do `layout`.
//! - `view.element` é lido uma vez, em `addView`, e guardado: o original relê a propriedade em `remove`.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use je_base::arrays::{push_to_end, push_to_start};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, combined_disposable};
use je_base::numbers::clamp;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{EventKind, NodeId, PointerEvents, Role, Ui};

pub use crate::sash::Orientation;
use crate::sash::{HorizontalSashLayoutProvider, Sash, SashEvent, SashOptions, SashState, VerticalSashLayoutProvider, registered_emitter};

/// O `ISplitViewStyles`: a cor da linha entre as views (`None` é `Color.transparent`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplitViewStyles {
    pub separator_border: Option<Rgba>,
}

/// O `LayoutPriority`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutPriority {
    Normal,
    Low,
    High,
}

/// O `IView<TLayoutContext>` de uma view do [`SplitView`]. `C` é o tipo do contexto de layout que o
/// [`SplitView::layout`] repassa a cada [`SplitViewView::layout`].
pub trait SplitViewView<C> {
    /// O nó desta view.
    fn element(&self) -> NodeId;

    /// O tamanho mínimo (`0` se não houver).
    fn minimum_size(&self) -> f64;

    /// O tamanho máximo (`f64::INFINITY` se não houver).
    fn maximum_size(&self) -> f64;

    /// A prioridade quando o algoritmo de layout roda (só vale com `proportional_layout` falso).
    fn priority(&self) -> Option<LayoutPriority> {
        None
    }

    /// Controle fino do layout proporcional por view (padrão `true`).
    fn proportional_layout(&self) -> Option<bool> {
        None
    }

    /// Se a view dá snap ao chegar no mínimo (padrão `false`).
    fn snap(&self) -> Option<bool> {
        None
    }

    /// Dispara quando qualquer restrição muda; opcionalmente com o tamanho preferido da view.
    fn on_did_change(&self) -> BaseEvent<Option<f64>>;

    /// Chamado pelo [`SplitView`] no layout. `size` é o tamanho da view, `offset` o deslocamento dela desde
    /// o início do split view.
    fn layout(&self, size: f64, offset: f64, context: Option<&C>);

    /// Chamado quando a view é mostrada ou escondida.
    fn set_visible(&self, _visible: bool) {}
}

/// A view como o [`SplitView`] a guarda.
pub type ViewRef<C> = Rc<dyn SplitViewView<C>>;

/// O `ISplitViewDescriptor`: uma descrição inicial, que cria todas as views no construtor.
pub struct SplitViewDescriptor<C> {
    pub size: f64,
    pub views: Vec<ViewDescriptor<C>>,
}

pub struct ViewDescriptor<C> {
    /// `visible` do original (`undefined` vale visível).
    pub visible: Option<bool>,
    pub size: f64,
    pub view: ViewRef<C>,
}

/// O `ISplitViewOptions`.
pub struct SplitViewOptions<C> {
    pub orientation: Option<Orientation>,
    pub styles: Option<SplitViewStyles>,
    /// Faz do Alt-arraste o arraste padrão.
    pub inverse_alt_behavior: bool,
    /// Redimensiona cada view proporcionalmente (padrão `true`).
    pub proportional_layout: Option<bool>,
    pub descriptor: Option<SplitViewDescriptor<C>>,
    /// Sobrescreve o tamanho ortogonal dos sashes.
    pub get_sash_orthogonal_size: Option<Rc<dyn Fn() -> f64>>,
}

impl<C> Default for SplitViewOptions<C> {
    fn default() -> Self {
        Self { orientation: None, styles: None, inverse_alt_behavior: false, proportional_layout: None, descriptor: None, get_sash_orthogonal_size: None }
    }
}

/// O que se passa a `addView` e `removeView` no lugar de um número: como redistribuir o espaço.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sizing {
    /// Distribui todo o espaço do split view uniformemente entre as views.
    Distribute,
    /// Abre espaço para a view nova reduzindo a de índice dado.
    Split(usize),
    /// `Distribute` se as views já estavam distribuídas por igual, senão `Split` com o índice.
    Auto(usize),
    /// A view entra (ou sai) invisível, lembrando o tamanho de quando for mostrada.
    Invisible(f64),
}

/// `number | Sizing`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewSize {
    Px(f64),
    Sizing(Sizing),
}

impl From<f64> for ViewSize {
    fn from(size: f64) -> Self {
        Self::Px(size)
    }
}

impl From<Sizing> for ViewSize {
    fn from(sizing: Sizing) -> Self {
        Self::Sizing(sizing)
    }
}

/// O evento de rolagem. Placeholder: o `Scrollable` não está portado, o evento nunca dispara.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollEvent;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Busy,
}

/// Marca o split view como ocupado, e volta a ocioso ao sair (o `try/finally` do original).
struct BusyGuard<'a>(&'a Cell<State>);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.set(State::Idle);
    }
}

fn throw(message: &str) -> ! {
    errors::throw(Error::new(message))
}

/// `Math.round`.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `range(from, to)` do `arrays.ts`: de `from` até `to` (exclusivo), subindo ou descendo.
fn range(from: i64, to: i64) -> Vec<usize> {
    je_base::arrays::range_from(from, to).into_iter().map(|i| usize::try_from(i).expect("índice negativo")).collect()
}

/// `console.error` dos `try/catch` do original.
fn report_failure(what: &str, result: Result<(), Box<dyn std::any::Any + Send>>) {
    if let Err(payload) = result {
        let message = payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()));
        eprintln!("Splitview: {what}");
        if let Some(message) = message {
            eprintln!("{message}");
        }
    }
}

struct ViewItem<C: 'static> {
    ui: Ui,
    container: NodeId,
    /// `view.element`, lido uma vez em `addView` e guardado (ver o topo do módulo).
    element: Cell<Option<NodeId>>,
    view: ViewRef<C>,
    orientation: Orientation,
    size: Cell<f64>,
    cached_visible_size: Cell<Option<f64>>,
    /// O `::before` da linha de separação, se a view tem uma.
    separator: Cell<Option<NodeId>>,
    disposable: RefCell<Option<DisposableRef>>,
}

impl<C: 'static> ViewItem<C> {
    fn new(ui: Ui, container: NodeId, view: ViewRef<C>, orientation: Orientation, size: ItemSize) -> Self {
        let (size, cached) = match size {
            ItemSize::Px(size) => {
                ui.tree_mut().add_class(container, "visible");
                (size, None)
            }
            ItemSize::Cached(cached) => (0.0, Some(cached)),
        };
        ui.tree_mut().style_mut(container).visible = cached.is_none();
        Self { ui, container, element: Cell::new(None), view, orientation, size: Cell::new(size), cached_visible_size: Cell::new(cached), separator: Cell::new(None), disposable: RefCell::new(None) }
    }

    fn size(&self) -> f64 {
        self.size.get()
    }

    fn visible(&self) -> bool {
        self.cached_visible_size.get().is_none()
    }

    fn set_visible(&self, visible: bool, size: Option<f64>) {
        if visible == self.visible() {
            return;
        }

        if visible {
            let cached = self.cached_visible_size.get().expect("view invisível sem tamanho guardado");
            self.size.set(clamp(cached, self.view_minimum_size(), self.view_maximum_size()));
            self.cached_visible_size.set(None);
        } else {
            self.cached_visible_size.set(Some(size.unwrap_or_else(|| self.size())));
            self.size.set(0.0);
        }

        {
            let mut tree = self.ui.tree_mut();
            tree.toggle_class(self.container, "visible", visible);
            tree.style_mut(self.container).visible = visible;
        }

        let view = self.view.clone();
        report_failure("Failed to set visible view", catch_unwind(AssertUnwindSafe(|| view.set_visible(visible))));
    }

    fn minimum_size(&self) -> f64 {
        if self.visible() { self.view.minimum_size() } else { 0.0 }
    }

    fn view_minimum_size(&self) -> f64 {
        self.view.minimum_size()
    }

    fn maximum_size(&self) -> f64 {
        if self.visible() { self.view.maximum_size() } else { 0.0 }
    }

    fn view_maximum_size(&self) -> f64 {
        self.view.maximum_size()
    }

    fn priority(&self) -> Option<LayoutPriority> {
        self.view.priority()
    }

    fn proportional_layout(&self) -> bool {
        self.view.proportional_layout().unwrap_or(true)
    }

    fn snap(&self) -> bool {
        self.view.snap().unwrap_or(false)
    }

    /// O `pointer-events` do contêiner: desligado durante o arraste de um sash.
    fn set_enabled(&self, enabled: bool) {
        self.ui.tree_mut().style_mut(self.container).pointer_events = if enabled { PointerEvents::Inherit } else { PointerEvents::None };
    }

    fn layout(&self, offset: f64, context: Option<&C>) {
        self.layout_container(offset);
        let (view, size) = (self.view.clone(), self.size());
        report_failure("Failed to layout view", catch_unwind(AssertUnwindSafe(|| view.layout(size, offset, context))));
    }

    /// `top/height` (vertical) ou `left/width` (horizontal), mais o `100%` do outro eixo do CSS.
    fn layout_container(&self, offset: f64) {
        let size = self.size();
        let mut tree = self.ui.tree_mut();
        match self.orientation {
            Orientation::Vertical => {
                tree.set_bounds(self.container, Rect::new(0.0, offset, 0.0, offset + size));
                tree.set_fill(self.container, true, false);
            }
            Orientation::Horizontal => {
                tree.set_bounds(self.container, Rect::new(offset, 0.0, offset + size, 0.0));
                tree.set_fill(self.container, false, true);
            }
        }
    }

    fn dispose(&self) {
        let disposable = self.disposable.borrow_mut().take();
        if let Some(disposable) = disposable {
            disposable.dispose();
        }
    }
}

enum ItemSize {
    Px(f64),
    Cached(f64),
}

struct SashItem {
    sash: Rc<Sash>,
    disposable: DisposableRef,
}

#[derive(Clone, Copy)]
struct SashDragSnapState {
    index: usize,
    limit_delta: f64,
    size: f64,
}

struct SashDragState {
    index: usize,
    start: f64,
    current: f64,
    sizes: Vec<f64>,
    min_delta: f64,
    max_delta: f64,
    alt: bool,
    snap_before: Option<SashDragSnapState>,
    snap_after: Option<SashDragSnapState>,
}

/// O fornecedor de layout dos sashes: a posição vem do split view, o comprimento da opção.
struct SashProvider<C: Clone + 'static> {
    split_view: Weak<SplitView<C>>,
    orthogonal_size: Option<Rc<dyn Fn() -> f64>>,
}

impl<C: Clone + 'static> SashProvider<C> {
    fn position(&self, sash: &Sash) -> f64 {
        self.split_view.upgrade().map_or(0.0, |split_view| split_view.get_sash_position(sash))
    }
}

impl<C: Clone + 'static> HorizontalSashLayoutProvider for SashProvider<C> {
    fn get_horizontal_sash_top(&self, sash: &Sash) -> f64 {
        self.position(sash)
    }

    fn get_horizontal_sash_width(&self, _sash: &Sash) -> Option<f64> {
        self.orthogonal_size.as_ref().map(|f| f())
    }
}

impl<C: Clone + 'static> VerticalSashLayoutProvider for SashProvider<C> {
    fn get_vertical_sash_left(&self, sash: &Sash) -> f64 {
        self.position(sash)
    }

    fn get_vertical_sash_height(&self, _sash: &Sash) -> Option<f64> {
        self.orthogonal_size.as_ref().map(|f| f())
    }
}

/// O `SplitView`.
pub struct SplitView<C: Clone + 'static = ()> {
    base: Rc<Disposable>,
    weak_self: Weak<SplitView<C>>,
    ui: Ui,
    orientation: Orientation,
    el: NodeId,
    sash_container: NodeId,
    scrollable_element: NodeId,
    view_container: NodeId,
    size: Cell<f64>,
    layout_context: RefCell<Option<C>>,
    content_size: Cell<f64>,
    proportions: RefCell<Option<Vec<Option<f64>>>>,
    view_items: RefCell<Vec<Rc<ViewItem<C>>>>,
    sash_items: RefCell<Vec<Rc<SashItem>>>,
    sash_drag_state: RefCell<Option<SashDragState>>,
    sash_drag_disposable: RefCell<Option<DisposableRef>>,
    state: Cell<State>,
    inverse_alt_behavior: bool,
    proportional_layout: bool,
    get_sash_orthogonal_size: Option<Rc<dyn Fn() -> f64>>,
    styles: Cell<SplitViewStyles>,
    on_did_sash_change: Rc<Emitter<usize>>,
    on_did_sash_reset: Rc<Emitter<usize>>,
    on_did_scroll: Rc<Emitter<ScrollEvent>>,
    orthogonal_start_sash: RefCell<Option<Rc<Sash>>>,
    orthogonal_end_sash: RefCell<Option<Rc<Sash>>>,
    start_snapping_enabled: Cell<bool>,
    end_snapping_enabled: Cell<bool>,
    disposed: Cell<bool>,
}

impl<C: Clone + 'static> SplitView<C> {
    /// Cria um split view dentro de `container`.
    pub fn new(ui: &Ui, container: NodeId, options: SplitViewOptions<C>) -> Rc<Self> {
        let orientation = options.orientation.unwrap_or(Orientation::Vertical);
        let base = Disposable::new();

        let (el, sash_container, scrollable_element, view_container) = {
            let mut tree = ui.tree_mut();
            let el = tree.create_detached(Role::Generic);
            tree.add_class(el, "monaco-split-view2");
            tree.add_class(el, if orientation == Orientation::Vertical { "vertical" } else { "horizontal" });
            tree.set_fill(el, true, true);
            tree.append_child(container, el);

            let sash_container = tree.create(el, Role::Generic);
            tree.add_class(sash_container, "sash-container");
            tree.set_fill(sash_container, true, true);
            // O sash tem `z-index: 35` e o `.sash-container` vem antes das views: o z-index é entre irmãos
            // na árvore do je-ui, então ele sobe no contêiner.
            let style = tree.style_mut(sash_container);
            style.pointer_events = PointerEvents::None;
            style.z_index = 35;

            let scrollable_element = tree.create(el, Role::Generic);
            tree.add_class(scrollable_element, "monaco-scrollable-element");
            tree.set_fill(scrollable_element, true, true);
            tree.style_mut(scrollable_element).clip = true;

            let view_container = tree.create(scrollable_element, Role::Generic);
            tree.add_class(view_container, "split-view-container");
            tree.set_fill(view_container, true, true);
            (el, sash_container, scrollable_element, view_container)
        };

        let this = Rc::new_cyclic(|weak| Self {
            weak_self: weak.clone(),
            ui: ui.clone(),
            orientation,
            el,
            sash_container,
            scrollable_element,
            view_container,
            size: Cell::new(0.0),
            layout_context: RefCell::new(None),
            content_size: Cell::new(0.0),
            proportions: RefCell::new(None),
            view_items: RefCell::new(Vec::new()),
            sash_items: RefCell::new(Vec::new()),
            sash_drag_state: RefCell::new(None),
            sash_drag_disposable: RefCell::new(None),
            state: Cell::new(State::Idle),
            inverse_alt_behavior: options.inverse_alt_behavior,
            proportional_layout: options.proportional_layout.unwrap_or(true),
            get_sash_orthogonal_size: options.get_sash_orthogonal_size,
            styles: Cell::new(SplitViewStyles::default()),
            on_did_sash_change: registered_emitter(&base),
            on_did_sash_reset: registered_emitter(&base),
            on_did_scroll: registered_emitter(&base),
            orthogonal_start_sash: RefCell::new(None),
            orthogonal_end_sash: RefCell::new(None),
            start_snapping_enabled: Cell::new(true),
            end_snapping_enabled: Cell::new(true),
            disposed: Cell::new(false),
            base,
        });

        this.style(options.styles.unwrap_or_default());

        // Já existe um conjunto de views: adiciona agora.
        if let Some(descriptor) = options.descriptor {
            this.size.set(descriptor.size);
            for (index, view_descriptor) in descriptor.views.into_iter().enumerate() {
                let sizing = if view_descriptor.visible.unwrap_or(true) { ViewSize::Px(view_descriptor.size) } else { ViewSize::Sizing(Sizing::Invisible(view_descriptor.size)) };
                this.do_add_view(view_descriptor.view, sizing, Some(index), true);
            }

            // Inicializa o tamanho do conteúdo e as proporções para o primeiro layout.
            this.content_size.set(this.items().iter().map(|i| i.size()).sum());
            this.save_proportions();
        }
        this
    }

    /// O nó do split view (o `.monaco-split-view2`).
    pub fn element(&self) -> NodeId {
        self.el
    }

    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    /// O contêiner dos sashes (usado por quem precisa achá-los na árvore).
    pub fn sash_container(&self) -> NodeId {
        self.sash_container
    }

    /// O contêiner das views (o `.split-view-container`).
    pub fn view_container(&self) -> NodeId {
        self.view_container
    }

    /// O `.monaco-scrollable-element` que envolve o contêiner das views.
    pub fn scrollable_element(&self) -> NodeId {
        self.scrollable_element
    }

    /// A soma dos tamanhos de todas as views.
    pub fn content_size(&self) -> f64 {
        self.content_size.get()
    }

    /// Dispara quando o usuário redimensiona um sash.
    pub fn on_did_sash_change(&self) -> BaseEvent<usize> {
        self.on_did_sash_change.event()
    }

    /// Dispara quando o usuário dá duplo clique num sash.
    pub fn on_did_sash_reset(&self) -> BaseEvent<usize> {
        self.on_did_sash_reset.event()
    }

    /// Dispara quando o split view rola. Nunca dispara por enquanto, ver o topo do módulo.
    pub fn on_did_scroll(&self) -> BaseEvent<ScrollEvent> {
        self.on_did_scroll.event()
    }

    /// O número de views.
    pub fn length(&self) -> usize {
        self.view_items.borrow().len()
    }

    /// O tamanho mínimo do split view.
    pub fn minimum_size(&self) -> f64 {
        self.items().iter().map(|i| i.minimum_size()).sum()
    }

    /// O tamanho máximo do split view.
    pub fn maximum_size(&self) -> f64 {
        if self.length() == 0 { f64::INFINITY } else { self.items().iter().map(|i| i.maximum_size()).sum() }
    }

    pub fn orthogonal_start_sash(&self) -> Option<Rc<Sash>> {
        self.orthogonal_start_sash.borrow().clone()
    }

    pub fn orthogonal_end_sash(&self) -> Option<Rc<Sash>> {
        self.orthogonal_end_sash.borrow().clone()
    }

    pub fn start_snapping_enabled(&self) -> bool {
        self.start_snapping_enabled.get()
    }

    pub fn end_snapping_enabled(&self) -> bool {
        self.end_snapping_enabled.get()
    }

    /// Um sash perpendicular a todos os deste split view, no lado esquerdo ou de cima.
    pub fn set_orthogonal_start_sash(&self, sash: Option<Rc<Sash>>) {
        for item in self.sash_items.borrow().iter() {
            item.sash.set_orthogonal_start_sash(sash.clone());
        }
        *self.orthogonal_start_sash.borrow_mut() = sash;
    }

    /// Um sash perpendicular a todos os deste split view, no lado direito ou de baixo.
    pub fn set_orthogonal_end_sash(&self, sash: Option<Rc<Sash>>) {
        for item in self.sash_items.borrow().iter() {
            item.sash.set_orthogonal_end_sash(sash.clone());
        }
        *self.orthogonal_end_sash.borrow_mut() = sash;
    }

    /// Os sashes internos.
    pub fn sashes(&self) -> Vec<Rc<Sash>> {
        self.sash_items.borrow().iter().map(|s| s.sash.clone()).collect()
    }

    /// Liga ou desliga o snap no início do split view.
    pub fn set_start_snapping_enabled(&self, enabled: bool) {
        if self.start_snapping_enabled.get() == enabled {
            return;
        }
        self.start_snapping_enabled.set(enabled);
        self.update_sash_enablement();
    }

    /// Liga ou desliga o snap no fim do split view.
    pub fn set_end_snapping_enabled(&self, enabled: bool) {
        if self.end_snapping_enabled.get() == enabled {
            return;
        }
        self.end_snapping_enabled.set(enabled);
        self.update_sash_enablement();
    }

    fn items(&self) -> Vec<Rc<ViewItem<C>>> {
        self.view_items.borrow().clone()
    }

    fn item(&self, index: usize) -> Rc<ViewItem<C>> {
        self.view_items.borrow()[index].clone()
    }

    fn begin(&self) -> BusyGuard<'_> {
        if self.state.get() != State::Idle {
            throw("Cant modify splitview");
        }
        self.state.set(State::Busy);
        BusyGuard(&self.state)
    }

    /// O `--separator-border` e a classe `separator-border`: a linha de 1px entre as views.
    pub fn style(&self, styles: SplitViewStyles) {
        let styles = SplitViewStyles { separator_border: styles.separator_border.filter(|c| c.a != 0) };
        self.styles.set(styles);
        self.ui.tree_mut().toggle_class(self.el, "separator-border", styles.separator_border.is_some());
        self.refresh_separators();
    }

    /// O `::before` de cada view menos a primeira, quando há cor de separação.
    fn refresh_separators(&self) {
        let color = self.styles.get().separator_border;
        for (index, item) in self.items().iter().enumerate() {
            let wanted = color.is_some() && index > 0;
            let mut tree = self.ui.tree_mut();
            match (item.separator.get(), wanted) {
                (Some(node), false) => {
                    drop(tree);
                    self.ui.remove(node);
                    item.separator.set(None);
                }
                (existing, true) => {
                    let node = existing.unwrap_or_else(|| {
                        let node = tree.create(item.container, Role::Separator);
                        let style = tree.style_mut(node);
                        style.pointer_events = PointerEvents::None;
                        style.z_index = 5;
                        node
                    });
                    item.separator.set(Some(node));
                    tree.style_mut(node).background = color;
                    match self.orientation {
                        Orientation::Vertical => {
                            tree.set_bounds(node, Rect::new(0.0, 0.0, 0.0, 1.0));
                            tree.set_fill(node, true, false);
                        }
                        Orientation::Horizontal => {
                            tree.set_bounds(node, Rect::new(0.0, 0.0, 1.0, 0.0));
                            tree.set_fill(node, false, true);
                        }
                    }
                }
                (None, false) => {}
            }
        }
    }

    /// Adiciona uma view. `index` ausente é o fim.
    pub fn add_view(self: &Rc<Self>, view: ViewRef<C>, size: impl Into<ViewSize>, index: Option<usize>, skip_layout: bool) {
        self.do_add_view(view, size.into(), index, skip_layout);
    }

    /// Remove a view de `index`. `sizing` diz se redistribui o espaço das outras.
    pub fn remove_view(&self, index: usize, sizing: Option<Sizing>) -> ViewRef<C> {
        if index >= self.length() {
            throw("Index out of bounds");
        }
        let _busy = self.begin();

        let sizing = match sizing {
            Some(Sizing::Auto(split_index)) => Some(if self.are_views_distributed() { Sizing::Distribute } else { Sizing::Split(split_index) }),
            other => other,
        };

        // Guarda a view de referência, no caso de `Split`.
        let reference = match sizing {
            Some(Sizing::Split(i)) => Some(self.item(i)),
            _ => None,
        };

        // Remove a view.
        let to_remove = self.view_items.borrow_mut().remove(index);

        // Redimensiona a view de referência, no caso de `Split`.
        if let Some(reference) = reference {
            reference.size.set(reference.size() + to_remove.size());
        }

        // Remove o sash.
        if self.length() >= 1 {
            let sash_index = index.saturating_sub(1);
            let sash_item = self.sash_items.borrow_mut().remove(sash_index);
            sash_item.disposable.dispose();
        }

        self.relayout(None, None);

        if sizing == Some(Sizing::Distribute) {
            self.distribute_view_sizes();
        }

        let view = to_remove.view.clone();
        self.dispose_item(&to_remove);
        self.refresh_separators();
        view
    }

    pub fn remove_all_views(&self) -> Vec<ViewRef<C>> {
        let _busy = self.begin();

        let items: Vec<_> = self.view_items.borrow_mut().drain(..).collect();
        for item in &items {
            self.dispose_item(item);
        }

        let sash_items: Vec<_> = self.sash_items.borrow_mut().drain(..).collect();
        for sash_item in sash_items {
            sash_item.disposable.dispose();
        }

        self.relayout(None, None);
        items.iter().map(|i| i.view.clone()).collect()
    }

    /// Solta o elemento da view do contêiner (a view pode voltar a outro) e descarta o resto do item.
    fn dispose_item(&self, item: &ViewItem<C>) {
        // `container.remove()` do original só tira o contêiner; o `view.element` sobrevive (ele pode ter sido
        // movido para outro contêiner, como no `flipNode` do grid). Aqui o elemento só sai se ainda estiver neste.
        if let Some(element) = item.element.get() {
            let mut tree = self.ui.tree_mut();
            if tree.parent(element) == Some(item.container) {
                tree.detach(element);
            }
        }
        item.dispose();
        let container = item.container;
        self.ui.remove(container);
    }

    /// Move uma view para outro índice.
    pub fn move_view(self: &Rc<Self>, from: usize, to: usize) {
        if self.state.get() != State::Idle {
            throw("Cant modify splitview");
        }
        let cached = self.get_view_cached_visible_size(from);
        let sizing: ViewSize = match cached {
            None => ViewSize::Px(self.get_view_size(from)),
            Some(cached) => ViewSize::Sizing(Sizing::Invisible(cached)),
        };
        let view = self.remove_view(from, None);
        self.add_view(view, sizing, Some(to), false);
    }

    /// Troca duas views.
    pub fn swap_views(self: &Rc<Self>, from: usize, to: usize) {
        if self.state.get() != State::Idle {
            throw("Cant modify splitview");
        }
        if from > to {
            return self.swap_views(to, from);
        }

        let from_size = self.get_view_size(from);
        let to_size = self.get_view_size(to);
        let to_view = self.remove_view(to, None);
        let from_view = self.remove_view(from, None);

        self.add_view(to_view, from_size, Some(from), false);
        self.add_view(from_view, to_size, Some(to), false);
    }

    /// Se a view está visível.
    pub fn is_view_visible(&self, index: usize) -> bool {
        if index >= self.length() {
            throw("Index out of bounds");
        }
        self.item(index).visible()
    }

    /// Define a visibilidade de uma view.
    pub fn set_view_visible(&self, index: usize, visible: bool) {
        if index >= self.length() {
            throw("Index out of bounds");
        }
        self.item(index).set_visible(visible, None);

        self.distribute_empty_space(Some(index));
        self.layout_views();
        self.save_proportions();
    }

    /// O tamanho que a view tinha antes de ser escondida.
    pub fn get_view_cached_visible_size(&self, index: usize) -> Option<f64> {
        if index >= self.length() {
            throw("Index out of bounds");
        }
        self.item(index).cached_visible_size.get()
    }

    /// Faz o layout do split view. `size` é o tamanho todo; `layout_context` vai para cada view.
    pub fn layout(&self, size: f64, layout_context: Option<C>) {
        let previous_size = self.size.get().max(self.content_size.get());
        self.size.set(size);
        *self.layout_context.borrow_mut() = layout_context;
        let mut size = size;

        let proportions = self.proportions.borrow().clone();
        let items = self.items();
        match proportions {
            None => {
                let low: Vec<usize> = (0..items.len()).filter(|i| items[*i].priority() == Some(LayoutPriority::Low)).collect();
                let high: Vec<usize> = (0..items.len()).filter(|i| items[*i].priority() == Some(LayoutPriority::High)).collect();
                self.resize(items.len() as i64 - 1, size - previous_size, None, Some(&low), Some(&high), f64::NEG_INFINITY, f64::INFINITY, None, None);
            }
            Some(proportions) => {
                let mut total = 0.0;
                for (item, proportion) in items.iter().zip(&proportions) {
                    match proportion {
                        Some(proportion) => total += proportion,
                        None => size -= item.size(),
                    }
                }
                for (item, proportion) in items.iter().zip(&proportions) {
                    if let Some(proportion) = proportion
                        && total > 0.0 {
                            item.size.set(clamp(js_round(proportion * size / total), item.minimum_size(), item.maximum_size()));
                        }
                }
            }
        }

        self.distribute_empty_space(None);
        self.layout_views();
    }

    fn save_proportions(&self) {
        if self.proportional_layout && self.content_size.get() > 0.0 {
            let content = self.content_size.get();
            *self.proportions.borrow_mut() = Some(self.items().iter().map(|v| (v.proportional_layout() && v.visible()).then(|| v.size() / content)).collect());
        }
    }

    fn on_sash_start(self: &Rc<Self>, sash: &Rc<Sash>, start: f64, alt: bool) {
        for item in self.items() {
            item.set_enabled(false);
        }

        let Some(index) = self.sash_index(sash) else { return };

        // Assim dá para apertar Alt enquanto redimensiona um sash, no estilo do macOS.
        let key = |down: bool| {
            let weak = self.weak_self.clone();
            move |event: &mut je_ui::Event| {
                let (Some(split_view), Some(key)) = (weak.upgrade(), event.key) else { return };
                let current = split_view.sash_drag_state.borrow().as_ref().map(|s| s.current);
                if let Some(current) = current {
                    split_view.reset_sash_drag_state(index, current, down && key.modifiers.alt);
                }
            }
        };
        let root = self.ui.root();
        let disposable = combined_disposable(vec![
            self.ui.listen(root, EventKind::KeyDown, false, key(true)),
            self.ui.listen(root, EventKind::KeyUp, false, key(false)),
        ]);
        *self.sash_drag_disposable.borrow_mut() = Some(disposable);

        self.reset_sash_drag_state(index, start, alt);
    }

    fn reset_sash_drag_state(&self, index: usize, start: f64, alt: bool) {
        let items = self.items();
        let sizes: Vec<f64> = items.iter().map(|i| i.size()).collect();
        let mut min_delta = f64::NEG_INFINITY;
        let mut max_delta = f64::INFINITY;

        let alt = if self.inverse_alt_behavior { !alt } else { alt };

        if alt {
            // Com Alt no último sash, redimensiona a view da esquerda/de cima, e não a da direita/de
            // baixo como de costume. Por isso o inverso do usual.
            let is_last_sash = index == self.sash_items.borrow().len() - 1;

            if is_last_sash {
                let item = &items[index];
                min_delta = (item.minimum_size() - item.size()) / 2.0;
                max_delta = (item.maximum_size() - item.size()) / 2.0;
            } else {
                let item = &items[index + 1];
                min_delta = (item.size() - item.maximum_size()) / 2.0;
                max_delta = (item.size() - item.minimum_size()) / 2.0;
            }
        }

        let mut snap_before = None;
        let mut snap_after = None;

        if !alt {
            let up = range(index as i64, -1);
            let down = range(index as i64 + 1, items.len() as i64);
            let min_delta_up: f64 = up.iter().map(|i| items[*i].minimum_size() - sizes[*i]).sum();
            let max_delta_up: f64 = up.iter().map(|i| items[*i].view_maximum_size() - sizes[*i]).sum();
            let max_delta_down = if down.is_empty() { f64::INFINITY } else { down.iter().map(|i| sizes[*i] - items[*i].minimum_size()).sum() };
            let min_delta_down = if down.is_empty() { f64::NEG_INFINITY } else { down.iter().map(|i| sizes[*i] - items[*i].view_maximum_size()).sum() };
            // No original estes dois sombreiam os `minDelta`/`maxDelta` de fora, que continuam
            // infinitos no estado do arraste (só o `limitDelta` do snap usa estes).
            let inner_min_delta = min_delta_up.max(min_delta_down);
            let inner_max_delta = max_delta_down.min(max_delta_up);
            let snap_before_index = self.find_first_snap_index(&up);
            let snap_after_index = self.find_first_snap_index(&down);

            if let Some(i) = snap_before_index {
                let item = &items[i];
                let half = (item.view_minimum_size() / 2.0).floor();
                snap_before = Some(SashDragSnapState { index: i, limit_delta: if item.visible() { inner_min_delta - half } else { inner_min_delta + half }, size: item.size() });
            }

            if let Some(i) = snap_after_index {
                let item = &items[i];
                let half = (item.view_minimum_size() / 2.0).floor();
                snap_after = Some(SashDragSnapState { index: i, limit_delta: if item.visible() { inner_max_delta + half } else { inner_max_delta - half }, size: item.size() });
            }
        }

        *self.sash_drag_state.borrow_mut() = Some(SashDragState { start, current: start, index, sizes, min_delta, max_delta, alt, snap_before, snap_after });
    }

    fn on_sash_change(&self, current: f64) {
        let (index, start, sizes, alt, min_delta, max_delta, snap_before, snap_after) = {
            let mut state = self.sash_drag_state.borrow_mut();
            let state = state.as_mut().expect("arraste sem estado");
            state.current = current;
            (state.index, state.start, state.sizes.clone(), state.alt, state.min_delta, state.max_delta, state.snap_before, state.snap_after)
        };

        let delta = current - start;
        let new_delta = self.resize(index as i64, delta, Some(sizes), None, None, min_delta, max_delta, snap_before, snap_after);

        if alt {
            let is_last_sash = index == self.sash_items.borrow().len() - 1;
            let new_sizes: Vec<f64> = self.items().iter().map(|i| i.size()).collect();
            let view_item_index = if is_last_sash { index } else { index + 1 };
            let item = self.item(view_item_index);
            let new_min_delta = item.size() - item.maximum_size();
            let new_max_delta = item.size() - item.minimum_size();
            let resize_index = if is_last_sash { index as i64 - 1 } else { index as i64 + 1 };

            self.resize(resize_index, -new_delta, Some(new_sizes), None, None, new_min_delta, new_max_delta, None, None);
        }

        self.distribute_empty_space(None);
        self.layout_views();
    }

    fn on_sash_end(&self, index: Option<usize>) {
        if let Some(index) = index {
            self.on_did_sash_change.fire(index);
        }
        let disposable = self.sash_drag_disposable.borrow_mut().take();
        if let Some(disposable) = disposable {
            disposable.dispose();
        }
        self.save_proportions();

        for item in self.items() {
            item.set_enabled(true);
        }
    }

    fn on_view_change(&self, item: &Rc<ViewItem<C>>, size: Option<f64>) {
        let items = self.items();
        let Some(index) = items.iter().position(|i| Rc::ptr_eq(i, item)) else { return };

        let size = clamp(size.unwrap_or_else(|| item.size()), item.minimum_size(), item.maximum_size());

        if self.inverse_alt_behavior && index > 0 {
            // Aqui a view cresce ou encolhe dos dois lados por igual; redimensiona o lado da esquerda pela
            // metade e deixa o `resize` fazer o clamp.
            self.resize(index as i64 - 1, ((item.size() - size) / 2.0).floor(), None, None, None, f64::NEG_INFINITY, f64::INFINITY, None, None);
            self.distribute_empty_space(None);
            self.layout_views();
        } else {
            item.size.set(size);

            // Absorve a mudança pela prioridade do layout, como o `resizeView`, para que um vizinho com
            // tamanho próprio não seja espremido (ver #334167).
            let others: Vec<usize> = (0..items.len()).filter(|i| *i != index).collect();
            let mut low: Vec<usize> = others.iter().copied().filter(|i| items[*i].priority() == Some(LayoutPriority::Low)).collect();
            low.push(index);
            let high: Vec<usize> = others.iter().copied().filter(|i| items[*i].priority() == Some(LayoutPriority::High)).collect();

            self.relayout(Some(&low), Some(&high));
        }
    }

    /// Redimensiona uma view dentro do split view.
    pub fn resize_view(&self, index: usize, size: f64) {
        if index >= self.length() {
            return;
        }
        let _busy = self.begin();

        let items = self.items();
        let others: Vec<usize> = (0..items.len()).filter(|i| *i != index).collect();
        let mut low: Vec<usize> = others.iter().copied().filter(|i| items[*i].priority() == Some(LayoutPriority::Low)).collect();
        low.push(index);
        let high: Vec<usize> = others.iter().copied().filter(|i| items[*i].priority() == Some(LayoutPriority::High)).collect();

        let item = &items[index];
        let size = clamp(js_round(size), item.minimum_size(), item.maximum_size().min(self.size.get()));

        item.size.set(size);
        self.relayout(Some(&low), Some(&high));
    }

    /// Se todas as outras views estão no tamanho mínimo.
    pub fn is_view_expanded(&self, index: usize) -> bool {
        let items = self.items();
        if index >= items.len() {
            return false;
        }
        items.iter().enumerate().all(|(i, item)| i == index || item.size() <= item.minimum_size())
    }

    /// Distribui o tamanho todo do split view entre as views.
    pub fn distribute_view_sizes(&self) {
        let items = self.items();
        let flexible: Vec<&Rc<ViewItem<C>>> = items.iter().filter(|i| i.maximum_size() - i.minimum_size() > 0.0).collect();
        let flexible_size: f64 = flexible.iter().map(|i| i.size()).sum();
        let size = (flexible_size / flexible.len() as f64).floor();

        for item in flexible {
            item.size.set(clamp(size, item.minimum_size(), item.maximum_size()));
        }

        let low: Vec<usize> = (0..items.len()).filter(|i| items[*i].priority() == Some(LayoutPriority::Low)).collect();
        let high: Vec<usize> = (0..items.len()).filter(|i| items[*i].priority() == Some(LayoutPriority::High)).collect();

        self.relayout(Some(&low), Some(&high));
    }

    /// O tamanho da view, ou `-1` se o índice não existe.
    pub fn get_view_size(&self, index: usize) -> f64 {
        self.view_items.borrow().get(index).map_or(-1.0, |i| i.size())
    }

    fn do_add_view(self: &Rc<Self>, view: ViewRef<C>, size: ViewSize, index: Option<usize>, skip_layout: bool) {
        let _busy = self.begin();
        let items_len = self.length();
        let index = index.unwrap_or(items_len);

        // Adiciona a view.
        let container = {
            let mut tree = self.ui.tree_mut();
            let container = tree.create_detached(Role::Generic);
            tree.add_class(container, "split-view-view");
            let reference = tree.children(self.view_container).get(index).copied().filter(|_| index != items_len);
            tree.insert_before(self.view_container, container, reference);
            container
        };

        let mut size = size;
        let view_size = match size {
            ViewSize::Px(px) => ItemSize::Px(px),
            ViewSize::Sizing(mut sizing) => {
                if let Sizing::Auto(split_index) = sizing {
                    sizing = if self.are_views_distributed() { Sizing::Distribute } else { Sizing::Split(split_index) };
                    size = ViewSize::Sizing(sizing);
                }
                match sizing {
                    Sizing::Split(i) => ItemSize::Px(self.get_view_size(i) / 2.0),
                    Sizing::Invisible(cached) => ItemSize::Cached(cached),
                    _ => ItemSize::Px(view.minimum_size()),
                }
            }
        };

        let item = Rc::new(ViewItem::new(self.ui.clone(), container, view.clone(), self.orientation, view_size));
        {
            let (weak, weak_item) = (self.weak_self.clone(), Rc::downgrade(&item));
            let on_change = view.on_did_change().listen(move |size| {
                if let (Some(split_view), Some(item)) = (weak.upgrade(), weak_item.upgrade()) {
                    split_view.on_view_change(&item, *size);
                }
            });
            // `container.remove()` do original: o contêiner sai da árvore junto com o item (ver `dispose_item`).
            *item.disposable.borrow_mut() = Some(on_change);
        }

        self.view_items.borrow_mut().insert(index, item.clone());

        // Adiciona o sash.
        if self.length() > 1 {
            self.add_sash(index);
        }

        let element = view.element();
        item.element.set(Some(element));
        self.ui.tree_mut().append_child(container, element);

        let high_priority_indexes = match size {
            ViewSize::Sizing(Sizing::Split(i)) => Some(vec![i]),
            _ => None,
        };

        if !skip_layout {
            self.relayout(Some(&[index]), high_priority_indexes.as_deref());
        }

        if !skip_layout && size == ViewSize::Sizing(Sizing::Distribute) {
            self.distribute_view_sizes();
        }
        self.refresh_separators();
    }

    /// Cria o sash entre a view `index - 1` e a `index` (ou, se `index` é 0, entre a 0 e a 1).
    fn add_sash(self: &Rc<Self>, index: usize) {
        let options = SashOptions { orthogonal_start_sash: self.orthogonal_start_sash(), orthogonal_end_sash: self.orthogonal_end_sash(), ..SashOptions::default() };
        let provider = Rc::new(SashProvider { split_view: self.weak_self.clone(), orthogonal_size: self.get_sash_orthogonal_size.clone() });

        let sash = match self.orientation {
            Orientation::Vertical => Sash::new_horizontal(&self.ui, self.sash_container, provider, options),
            Orientation::Horizontal => Sash::new_vertical(&self.ui, self.sash_container, provider, options),
        };
        let vertical = self.orientation == Orientation::Vertical;
        let position = move |e: &SashEvent| if vertical { (e.start_y, e.current_y) } else { (e.start_x, e.current_x) };

        let (weak, sash_weak) = (self.weak_self.clone(), Rc::downgrade(&sash));
        let on_start = sash.on_did_start().listen(move |e| {
            if let (Some(split_view), Some(sash)) = (weak.upgrade(), sash_weak.upgrade()) {
                split_view.on_sash_start(&sash, position(e).0, e.alt_key);
            }
        });
        let (weak, sash_weak) = (self.weak_self.clone(), Rc::downgrade(&sash));
        let on_change = sash.on_did_change().listen(move |e| {
            if let (Some(split_view), Some(_)) = (weak.upgrade(), sash_weak.upgrade()) {
                split_view.on_sash_change(position(e).1);
            }
        });
        let (weak, sash_weak) = (self.weak_self.clone(), Rc::downgrade(&sash));
        let on_end = sash.on_did_end().listen(move |_| {
            if let (Some(split_view), Some(sash)) = (weak.upgrade(), sash_weak.upgrade()) {
                let index = split_view.sash_index(&sash);
                split_view.on_sash_end(index);
            }
        });
        let (weak, sash_weak) = (self.weak_self.clone(), Rc::downgrade(&sash));
        let on_reset = sash.on_did_reset().listen(move |_| {
            let (Some(split_view), Some(sash)) = (weak.upgrade(), sash_weak.upgrade()) else { return };
            let Some(index) = split_view.sash_index(&sash) else { return };
            let up = range(index as i64, -1);
            let down = range(index as i64 + 1, split_view.length() as i64);
            let snap_before = split_view.find_first_snap_index(&up);
            let snap_after = split_view.find_first_snap_index(&down);

            if snap_before.is_some_and(|i| !split_view.item(i).visible()) {
                return;
            }
            if snap_after.is_some_and(|i| !split_view.item(i).visible()) {
                return;
            }
            split_view.on_did_sash_reset.fire(index);
        });

        let disposable = combined_disposable(vec![on_start, on_change, on_end, on_reset, sash.clone()]);
        // `this.sashItems.splice(index - 1, 0, sashItem)`: com `index` 0 o JavaScript conta do fim, mas o
        // sash só existe com mais de uma view, e o índice 0 insere antes de tudo (`splice(-1, ...)` entra
        // antes do último, que é o único).
        let position = if index == 0 { self.sash_items.borrow().len().saturating_sub(1) } else { index - 1 };
        self.sash_items.borrow_mut().insert(position, Rc::new(SashItem { sash, disposable }));
    }

    fn relayout(&self, low_priority_indexes: Option<&[usize]>, high_priority_indexes: Option<&[usize]>) {
        let content_size: f64 = self.items().iter().map(|i| i.size()).sum();

        self.resize(self.length() as i64 - 1, self.size.get() - content_size, None, low_priority_indexes, high_priority_indexes, f64::NEG_INFINITY, f64::INFINITY, None, None);
        self.distribute_empty_space(None);
        self.layout_views();
        self.save_proportions();
    }

    #[allow(clippy::too_many_arguments)]
    fn resize(
        &self,
        index: i64,
        delta: f64,
        sizes: Option<Vec<f64>>,
        low_priority_indexes: Option<&[usize]>,
        high_priority_indexes: Option<&[usize]>,
        overload_min_delta: f64,
        overload_max_delta: f64,
        snap_before: Option<SashDragSnapState>,
        snap_after: Option<SashDragSnapState>,
    ) -> f64 {
        let items = self.items();
        if index < 0 || index >= items.len() as i64 {
            return 0.0;
        }
        let sizes = sizes.unwrap_or_else(|| items.iter().map(|i| i.size()).collect());

        let mut up_indexes = range(index, -1);
        let mut down_indexes = range(index + 1, items.len() as i64);

        if let Some(high) = high_priority_indexes {
            for i in high {
                push_to_start(&mut up_indexes, i);
                push_to_start(&mut down_indexes, i);
            }
        }

        if let Some(low) = low_priority_indexes {
            for i in low {
                push_to_end(&mut up_indexes, i);
                push_to_end(&mut down_indexes, i);
            }
        }

        let up_items: Vec<&Rc<ViewItem<C>>> = up_indexes.iter().map(|i| &items[*i]).collect();
        let up_sizes: Vec<f64> = up_indexes.iter().map(|i| sizes[*i]).collect();

        let down_items: Vec<&Rc<ViewItem<C>>> = down_indexes.iter().map(|i| &items[*i]).collect();
        let down_sizes: Vec<f64> = down_indexes.iter().map(|i| sizes[*i]).collect();

        let min_delta_up: f64 = up_indexes.iter().map(|i| items[*i].minimum_size() - sizes[*i]).sum();
        let max_delta_up: f64 = up_indexes.iter().map(|i| items[*i].maximum_size() - sizes[*i]).sum();
        let max_delta_down = if down_indexes.is_empty() { f64::INFINITY } else { down_indexes.iter().map(|i| sizes[*i] - items[*i].minimum_size()).sum() };
        let min_delta_down = if down_indexes.is_empty() { f64::NEG_INFINITY } else { down_indexes.iter().map(|i| sizes[*i] - items[*i].maximum_size()).sum() };
        let min_delta = min_delta_up.max(min_delta_down).max(overload_min_delta);
        let max_delta = max_delta_down.min(max_delta_up).min(overload_max_delta);

        let mut snapped = false;

        if let Some(snap) = snap_before {
            let snap_view = &items[snap.index];
            let visible = delta >= snap.limit_delta;
            snapped = visible != snap_view.visible();
            snap_view.set_visible(visible, Some(snap.size));
        }

        if !snapped
            && let Some(snap) = snap_after {
                let snap_view = &items[snap.index];
                let visible = delta < snap.limit_delta;
                snapped = visible != snap_view.visible();
                snap_view.set_visible(visible, Some(snap.size));
            }

        if snapped {
            return self.resize(index, delta, Some(sizes), low_priority_indexes, high_priority_indexes, overload_min_delta, overload_max_delta, None, None);
        }

        let delta = clamp(delta, min_delta, max_delta);

        let mut delta_up = delta;
        for (item, up_size) in up_items.iter().zip(&up_sizes) {
            let size = clamp(up_size + delta_up, item.minimum_size(), item.maximum_size());
            let view_delta = size - up_size;

            delta_up -= view_delta;
            item.size.set(size);
        }

        let mut delta_down = delta;
        for (item, down_size) in down_items.iter().zip(&down_sizes) {
            let size = clamp(down_size - delta_down, item.minimum_size(), item.maximum_size());
            let view_delta = size - down_size;

            delta_down += view_delta;
            item.size.set(size);
        }

        delta
    }

    fn distribute_empty_space(&self, low_priority_index: Option<usize>) {
        let items = self.items();
        let content_size: f64 = items.iter().map(|i| i.size()).sum();
        let mut empty_delta = self.size.get() - content_size;

        let mut indexes = range(items.len() as i64 - 1, -1);
        let low: Vec<usize> = indexes.iter().copied().filter(|i| items[*i].priority() == Some(LayoutPriority::Low)).collect();
        let high: Vec<usize> = indexes.iter().copied().filter(|i| items[*i].priority() == Some(LayoutPriority::High)).collect();

        for index in &high {
            push_to_start(&mut indexes, index);
        }

        for index in &low {
            push_to_end(&mut indexes, index);
        }

        if let Some(index) = low_priority_index {
            push_to_end(&mut indexes, &index);
        }

        let mut i = 0;
        while empty_delta != 0.0 && i < indexes.len() {
            let item = &items[indexes[i]];
            let size = clamp(item.size() + empty_delta, item.minimum_size(), item.maximum_size());
            let view_delta = size - item.size();

            empty_delta -= view_delta;
            item.size.set(size);
            i += 1;
        }
    }

    fn layout_views(&self) {
        let items = self.items();
        // Guarda o novo tamanho do conteúdo.
        self.content_size.set(items.iter().map(|i| i.size()).sum());

        // Faz o layout das views.
        let mut offset = 0.0;
        let context = self.layout_context.borrow().clone();

        for item in &items {
            item.layout(offset, context.as_ref());
            offset += item.size();
        }

        // Faz o layout dos sashes.
        for sash in self.sashes() {
            sash.layout();
        }
        self.update_sash_enablement();
        // `updateScrollableElement`: o `Scrollable` não está portado (ver o topo do módulo).
    }

    fn update_sash_enablement(&self) {
        let items = self.items();
        let mut previous = false;
        let collapses_down: Vec<bool> = items.iter().map(|i| { previous = (i.size() - i.minimum_size() > 0.0) || previous; previous }).collect();

        previous = false;
        let expands_down: Vec<bool> = items.iter().map(|i| { previous = (i.maximum_size() - i.size() > 0.0) || previous; previous }).collect();

        previous = false;
        let mut collapses_up: Vec<bool> = items.iter().rev().map(|i| { previous = (i.size() - i.minimum_size() > 0.0) || previous; previous }).collect();
        collapses_up.reverse();

        previous = false;
        let mut expands_up: Vec<bool> = items.iter().rev().map(|i| { previous = (i.maximum_size() - i.size() > 0.0) || previous; previous }).collect();
        expands_up.reverse();

        let sash_items: Vec<Rc<SashItem>> = self.sash_items.borrow().clone();
        let at = |v: &[bool], i: usize| v.get(i).copied().unwrap_or(false);
        let mut position = 0.0;
        for (index, sash_item) in sash_items.iter().enumerate() {
            let sash = &sash_item.sash;
            position += items[index].size();

            let min = !(at(&collapses_down, index) && at(&expands_up, index + 1));
            let max = !(at(&expands_down, index) && at(&collapses_up, index + 1));

            if min && max {
                let up = range(index as i64, -1);
                let down = range(index as i64 + 1, items.len() as i64);
                let snap_before = self.find_first_snap_index(&up);
                let snap_after = self.find_first_snap_index(&down);

                let snapped_before = snap_before.is_some_and(|i| !items[i].visible());
                let snapped_after = snap_after.is_some_and(|i| !items[i].visible());

                if snapped_before && at(&collapses_up, index) && (position > 0.0 || self.start_snapping_enabled()) {
                    sash.set_state(SashState::AtMinimum);
                } else if snapped_after && at(&collapses_down, index) && (position < self.content_size.get() || self.end_snapping_enabled()) {
                    sash.set_state(SashState::AtMaximum);
                } else {
                    sash.set_state(SashState::Disabled);
                }
            } else if min && !max {
                sash.set_state(SashState::AtMinimum);
            } else if !min && max {
                sash.set_state(SashState::AtMaximum);
            } else {
                sash.set_state(SashState::Enabled);
            }
        }
    }

    fn sash_index(&self, sash: &Rc<Sash>) -> Option<usize> {
        self.sash_items.borrow().iter().position(|item| Rc::ptr_eq(&item.sash, sash))
    }

    /// A posição do sash: a soma dos tamanhos das views até ele (0 se não for daqui).
    fn get_sash_position(&self, sash: &Sash) -> f64 {
        let items = self.items();
        let sash_items = self.sash_items.borrow();
        let mut position = 0.0;

        for (i, sash_item) in sash_items.iter().enumerate() {
            position += items[i].size();

            if std::ptr::eq(&*sash_item.sash, sash) {
                return position;
            }
        }

        0.0
    }

    fn find_first_snap_index(&self, indexes: &[usize]) -> Option<usize> {
        let items = self.items();
        // Primeiro as views visíveis.
        for &index in indexes {
            let item = &items[index];

            if !item.visible() {
                continue;
            }

            if item.snap() {
                return Some(index);
            }
        }

        // Depois as escondidas.
        for &index in indexes {
            let item = &items[index];

            if item.visible() && item.maximum_size() - item.minimum_size() > 0.0 {
                return None;
            }

            if !item.visible() && item.snap() {
                return Some(index);
            }
        }

        None
    }

    fn are_views_distributed(&self) -> bool {
        let mut min: Option<f64> = None;
        let mut max: Option<f64> = None;

        for item in self.items() {
            min = Some(min.map_or(item.size(), |m| m.min(item.size())));
            max = Some(max.map_or(item.size(), |m| m.max(item.size())));

            if max.unwrap_or(0.0) - min.unwrap_or(0.0) > 2.0 {
                return false;
            }
        }

        true
    }
}

impl<C: Clone + 'static> IDisposable for SplitView<C> {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        let drag = self.sash_drag_disposable.borrow_mut().take();
        if let Some(drag) = drag {
            drag.dispose();
        }

        let items: Vec<_> = self.view_items.borrow_mut().drain(..).collect();
        for item in &items {
            self.dispose_item(item);
        }

        let sash_items: Vec<_> = self.sash_items.borrow_mut().drain(..).collect();
        for sash_item in sash_items {
            sash_item.disposable.dispose();
        }

        self.base.dispose();
        self.ui.remove(self.el);
    }

    fn constructor_name(&self) -> &'static str {
        "SplitView"
    }
}
