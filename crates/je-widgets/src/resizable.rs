//! Porte de `src/vs/base/browser/ui/resizable/resizable.ts`: o [`ResizableHtmlElement`] (`ResizableHTMLElement`), um nó
//! com quatro [`Sash`]es nas bordas que o usuário arrasta para mudar o tamanho dele, respeitando um tamanho mínimo e
//! um máximo, com um tamanho preferido para onde o duplo clique volta.
//!
//! Como o DOM vira [`je_ui`]:
//!
//! - O `domNode` é um nó solto da árvore ([`ResizableHtmlElement::dom_node`]); `style.height` e `style.width` são os
//!   limites do nó, reescritos por [`ResizableHtmlElement::layout`] sem mexer na origem, que é do dono do nó (o
//!   `position: absolute` com `left` e `top` do original).
//! - O `Dimension` de `dom.ts` é o [`Dimension`] deste módulo (largura e altura em `f64`, o `number` do JavaScript).
//!   `Dimension.lift` não existe: o argumento já é um [`Dimension`].
//! - Os provedores de layout dos sashes (`{ getVerticalSashLeft: () => this._size.width }`) são fechamentos que leem o
//!   tamanho por uma referência fraca, para o nó não formar ciclo com os sashes.
//! - `IResizeEvent.north | east | south | west` (opcionais, `undefined` é falso) são `bool`.
//! - `Emitter` e `DisposableStore` são os de `je-base`; o `Event.any` é [`je_base::event::any`].

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent, any};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_render::kurbo::Rect;
use je_ui::{NodeId, Role, Ui};

use crate::sash::{HorizontalSashLayoutProvider, OrthogonalEdge, Sash, SashEvent, SashOptions, SashState, VerticalSashLayoutProvider};

/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// `Dimension` de `dom.ts`: largura e altura.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dimension {
    pub width: f64,
    pub height: f64,
}

impl Dimension {
    /// `new Dimension(width, height)`.
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// `IResizeEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IResizeEvent {
    pub dimension: Dimension,
    pub done: bool,
    pub north: bool,
    pub east: bool,
    pub south: bool,
    pub west: bool,
}

impl IResizeEvent {
    /// O fim do redimensionamento (`{ dimension, done: true }`).
    fn finished(dimension: Dimension) -> Self {
        Self { dimension, done: true, north: false, east: false, south: false, west: false }
    }

    /// Um passo do arraste da borda `edge` (`{ dimension, done: false, <edge>: true }`).
    fn dragging(dimension: Dimension, edge: ResizeEdge) -> Self {
        Self {
            dimension,
            done: false,
            north: edge == ResizeEdge::North,
            east: edge == ResizeEdge::East,
            south: edge == ResizeEdge::South,
            west: edge == ResizeEdge::West,
        }
    }
}

/// A borda que o usuário arrasta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResizeEdge {
    North,
    East,
    South,
    West,
}

/// O estado de um arraste, que o construtor do original guarda em variáveis locais (`currentSize`, `deltaY`, `deltaX`).
#[derive(Default)]
struct DragState {
    current_size: Cell<Option<Dimension>>,
    delta_y: Cell<f64>,
    delta_x: Cell<f64>,
}

impl DragState {
    fn reset_deltas(&self) {
        self.delta_y.set(0.0);
        self.delta_x.set(0.0);
    }
}

/// `ResizableHTMLElement`.
pub struct ResizableHtmlElement {
    ui: Ui,
    dom_node: NodeId,
    on_did_will_resize: Rc<Emitter<()>>,
    on_did_resize: Rc<Emitter<IResizeEvent>>,
    north_sash: Rc<Sash>,
    east_sash: Rc<Sash>,
    south_sash: Rc<Sash>,
    west_sash: Rc<Sash>,
    sash_listener: Rc<DisposableStore>,
    size: Cell<Dimension>,
    min_size: Cell<Dimension>,
    max_size: Cell<Dimension>,
    preferred_size: Cell<Option<Dimension>>,
}

impl ResizableHtmlElement {
    /// `new ResizableHTMLElement()`.
    pub fn new(ui: &Ui) -> Rc<Self> {
        let dom_node = ui.tree_mut().create_detached(Role::Generic);
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let east_provider: Rc<dyn VerticalSashLayoutProvider> = {
                let weak = weak.clone();
                Rc::new(move |_: &Sash| weak.upgrade().map_or(0.0, |this| this.size.get().width))
            };
            let west_provider: Rc<dyn VerticalSashLayoutProvider> = Rc::new(|_: &Sash| 0.0);
            let north_provider: Rc<dyn HorizontalSashLayoutProvider> = Rc::new(|_: &Sash| 0.0);
            let south_provider: Rc<dyn HorizontalSashLayoutProvider> = {
                let weak = weak.clone();
                Rc::new(move |_: &Sash| weak.upgrade().map_or(0.0, |this| this.size.get().height))
            };

            let east_sash = Sash::new_vertical(ui, dom_node, east_provider, SashOptions::default());
            let west_sash = Sash::new_vertical(ui, dom_node, west_provider, SashOptions::default());
            let north_sash =
                Sash::new_horizontal(ui, dom_node, north_provider, SashOptions { orthogonal_edge: Some(OrthogonalEdge::North), ..SashOptions::default() });
            let south_sash =
                Sash::new_horizontal(ui, dom_node, south_provider, SashOptions { orthogonal_edge: Some(OrthogonalEdge::South), ..SashOptions::default() });

            north_sash.set_orthogonal_start_sash(Some(Rc::clone(&west_sash)));
            north_sash.set_orthogonal_end_sash(Some(Rc::clone(&east_sash)));
            south_sash.set_orthogonal_start_sash(Some(Rc::clone(&west_sash)));
            south_sash.set_orthogonal_end_sash(Some(Rc::clone(&east_sash)));

            let on_did_will_resize = Emitter::new();
            let on_did_resize = Emitter::new();
            let sash_listener = DisposableStore::new();
            let drag = Rc::new(DragState::default());

            // Início do arraste de qualquer borda.
            let starts = any(vec![north_sash.on_did_start(), east_sash.on_did_start(), south_sash.on_did_start(), west_sash.on_did_start()]);
            sash_listener.add(starts.listen({
                let (weak, drag) = (weak.clone(), Rc::clone(&drag));
                move |_| {
                    let Some(this) = weak.upgrade() else {
                        return;
                    };
                    if drag.current_size.get().is_none() {
                        this.on_did_will_resize.fire(());
                        drag.current_size.set(Some(this.size.get()));
                        drag.reset_deltas();
                    }
                }
            }));

            // Fim do arraste de qualquer borda.
            let ends = any(vec![north_sash.on_did_end(), east_sash.on_did_end(), south_sash.on_did_end(), west_sash.on_did_end()]);
            sash_listener.add(ends.listen({
                let (weak, drag) = (weak.clone(), Rc::clone(&drag));
                move |_| {
                    let Some(this) = weak.upgrade() else {
                        return;
                    };
                    if drag.current_size.get().is_some() {
                        drag.current_size.set(None);
                        drag.reset_deltas();
                        this.on_did_resize.fire(IResizeEvent::finished(this.size.get()));
                    }
                }
            }));

            for (sash, edge) in [(&east_sash, ResizeEdge::East), (&west_sash, ResizeEdge::West), (&north_sash, ResizeEdge::North), (&south_sash, ResizeEdge::South)] {
                Self::listen_edge_change(weak, &drag, &sash_listener, sash, edge);
            }

            // Duplo clique: volta ao tamanho preferido (na largura pelas bordas leste e oeste, na altura pelas outras).
            let horizontal_resets = any(vec![east_sash.on_did_reset(), west_sash.on_did_reset()]);
            sash_listener.add(horizontal_resets.listen({
                let weak = weak.clone();
                move |_| {
                    let Some(this) = weak.upgrade() else {
                        return;
                    };
                    if let Some(preferred) = this.preferred_size.get() {
                        this.layout(Some(this.size.get().height), Some(preferred.width));
                        this.on_did_resize.fire(IResizeEvent::finished(this.size.get()));
                    }
                }
            }));
            let vertical_resets = any(vec![north_sash.on_did_reset(), south_sash.on_did_reset()]);
            sash_listener.add(vertical_resets.listen({
                let weak = weak.clone();
                move |_| {
                    let Some(this) = weak.upgrade() else {
                        return;
                    };
                    if let Some(preferred) = this.preferred_size.get() {
                        this.layout(Some(preferred.height), Some(this.size.get().width));
                        this.on_did_resize.fire(IResizeEvent::finished(this.size.get()));
                    }
                }
            }));

            Self {
                ui: ui.clone(),
                dom_node,
                on_did_will_resize,
                on_did_resize,
                north_sash,
                east_sash,
                south_sash,
                west_sash,
                sash_listener,
                size: Cell::new(Dimension::new(0.0, 0.0)),
                min_size: Cell::new(Dimension::new(0.0, 0.0)),
                max_size: Cell::new(Dimension::new(MAX_SAFE_INTEGER, MAX_SAFE_INTEGER)),
                preferred_size: Cell::new(None),
            }
        })
    }

    /// O `onDidChange` de uma borda: soma o deslocamento do arraste ao tamanho de quando o arraste começou. A borda leste
    /// e a sul crescem com o ponteiro; a oeste e a norte, ao contrário.
    fn listen_edge_change(weak: &Weak<Self>, drag: &Rc<DragState>, store: &DisposableStore, sash: &Rc<Sash>, edge: ResizeEdge) {
        let (weak, drag) = (weak.clone(), Rc::clone(drag));
        store.add(sash.on_did_change().listen(move |event: &SashEvent| {
            let Some(current_size) = drag.current_size.get() else {
                return;
            };
            let Some(this) = weak.upgrade() else {
                return;
            };
            match edge {
                ResizeEdge::East => drag.delta_x.set(event.current_x - event.start_x),
                ResizeEdge::West => drag.delta_x.set(-(event.current_x - event.start_x)),
                ResizeEdge::North => drag.delta_y.set(-(event.current_y - event.start_y)),
                ResizeEdge::South => drag.delta_y.set(event.current_y - event.start_y),
            }
            this.layout(Some(current_size.height + drag.delta_y.get()), Some(current_size.width + drag.delta_x.get()));
            this.on_did_resize.fire(IResizeEvent::dragging(this.size.get(), edge));
        }));
    }

    /// `domNode`.
    pub fn dom_node(&self) -> NodeId {
        self.dom_node
    }

    /// `onDidWillResize`: o usuário começou a arrastar uma borda.
    pub fn on_did_will_resize(&self) -> BaseEvent<()> {
        self.on_did_will_resize.event()
    }

    /// `onDidResize`.
    pub fn on_did_resize(&self) -> BaseEvent<IResizeEvent> {
        self.on_did_resize.event()
    }

    /// `enableSashes(north, east, south, west)`.
    pub fn enable_sashes(&self, north: bool, east: bool, south: bool, west: bool) {
        let state = |enabled: bool| if enabled { SashState::Enabled } else { SashState::Disabled };
        self.north_sash.set_state(state(north));
        self.east_sash.set_state(state(east));
        self.south_sash.set_state(state(south));
        self.west_sash.set_state(state(west));
    }

    /// `layout(height = this.size.height, width = this.size.width)`: `None` é o argumento ausente, que vale o tamanho atual.
    /// O tamanho final respeita o mínimo e o máximo.
    pub fn layout(&self, height: Option<f64>, width: Option<f64>) {
        let size = self.size.get();
        let (min, max) = (self.min_size.get(), self.max_size.get());

        let height = min.height.max(max.height.min(height.unwrap_or(size.height)));
        let width = min.width.max(max.width.min(width.unwrap_or(size.width)));

        let new_size = Dimension::new(width, height);
        if new_size != size {
            {
                let mut tree = self.ui.tree_mut();
                let bounds = tree.bounds(self.dom_node);
                tree.set_bounds(self.dom_node, Rect::new(bounds.x0, bounds.y0, bounds.x0 + width, bounds.y0 + height));
            }
            self.size.set(new_size);
            self.north_sash.layout();
            self.east_sash.layout();
            self.south_sash.layout();
            self.west_sash.layout();
        }
    }

    /// `clearSashHoverState()`.
    pub fn clear_sash_hover_state(&self) {
        self.east_sash.clear_sash_hover_state();
        self.west_sash.clear_sash_hover_state();
        self.north_sash.clear_sash_hover_state();
        self.south_sash.clear_sash_hover_state();
    }

    /// `size`.
    pub fn size(&self) -> Dimension {
        self.size.get()
    }

    /// `set maxSize`.
    pub fn set_max_size(&self, value: Dimension) {
        self.max_size.set(value);
    }

    /// `get maxSize`.
    pub fn max_size(&self) -> Dimension {
        self.max_size.get()
    }

    /// `set minSize`.
    pub fn set_min_size(&self, value: Dimension) {
        self.min_size.set(value);
    }

    /// `get minSize`.
    pub fn min_size(&self) -> Dimension {
        self.min_size.get()
    }

    /// `set preferredSize`.
    pub fn set_preferred_size(&self, value: Option<Dimension>) {
        self.preferred_size.set(value);
    }

    /// `get preferredSize`.
    pub fn preferred_size(&self) -> Option<Dimension> {
        self.preferred_size.get()
    }
}

impl IDisposable for ResizableHtmlElement {
    fn dispose(&self) {
        self.north_sash.dispose();
        self.south_sash.dispose();
        self.east_sash.dispose();
        self.west_sash.dispose();
        self.sash_listener.dispose();
        self.on_did_resize.dispose();
        self.on_did_will_resize.dispose();
        self.ui.remove(self.dom_node);
    }

    fn constructor_name(&self) -> &'static str {
        "ResizableHTMLElement"
    }
}
