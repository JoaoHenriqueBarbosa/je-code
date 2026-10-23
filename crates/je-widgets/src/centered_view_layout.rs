//! Porte de `src/vs/base/browser/ui/centered/centeredViewLayout.ts`: o [`CenteredViewLayout`], que centraliza
//! uma view entre duas margens redimensionáveis (o "layout centralizado" do editor), com a largura
//! proporcional ao espaço ou fixa.
//!
//! Do DOM para o `je-ui`:
//!
//! - O contêiner ganha `overflow: hidden` (recorte). A view entra no contêiner enquanto o layout está inativo
//!   e no [`SplitView`] horizontal (margem, view, margem) enquanto ativo, como no original; as margens são
//!   `.centered-layout-margin` com `height: 100%` e a cor de fundo do estilo.
//! - O `IView` do `grid.ts` é [`GridViewView`]; o contexto de layout do split view é o `{ top, left }` da
//!   posição do último layout ([`LayoutOffset`]).
//! - A cor de fundo do original é um `Color`; aqui é [`Rgba`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_render::Rgba;
use je_ui::{NodeId, Role, Ui};

use crate::dom::create_detached_el;
use crate::gridview::{GridViewRef, ViewDimensions};
use crate::sash::{BoundarySashes, Orientation};
use crate::splitview::{LayoutPriority, Sizing, SplitView, SplitViewOptions, SplitViewStyles, SplitViewView};

/// O `CenteredViewState`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CenteredViewState {
    pub target_width: f64,
    pub left_margin_ratio: f64,
    pub right_margin_ratio: f64,
}

/// O `defaultState`.
pub const DEFAULT_STATE: CenteredViewState = CenteredViewState { target_width: 900.0, left_margin_ratio: 0.1909, right_margin_ratio: 0.1909 };

/// O `{ top, left }` do contexto de layout do split view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutOffset {
    pub top: f64,
    pub left: f64,
}

/// O `ICenteredViewStyles` (estende `ISplitViewStyles`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CenteredViewStyles {
    pub split_view: SplitViewStyles,
    pub background: Rgba,
}

/// O `IDomNodePagePosition` do último layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct LayoutPosition {
    width: f64,
    height: f64,
    left: f64,
    top: f64,
}

/// `createEmptyView(background)`: a margem.
struct EmptyView {
    element: NodeId,
    on_did_change: Rc<Emitter<Option<f64>>>,
}

impl EmptyView {
    fn new(ui: &Ui, background: Option<Rgba>) -> Rc<Self> {
        let element = {
            let mut tree = ui.tree_mut();
            let element = create_detached_el(&mut tree, Role::Generic, "centered-layout-margin");
            tree.set_fill(element, false, true);
            tree.style_mut(element).background = background;
            element
        };
        Rc::new(Self { element, on_did_change: Emitter::new() })
    }
}

impl SplitViewView<LayoutOffset> for EmptyView {
    fn element(&self) -> NodeId {
        self.element
    }

    fn minimum_size(&self) -> f64 {
        60.0
    }

    fn maximum_size(&self) -> f64 {
        f64::INFINITY
    }

    fn on_did_change(&self) -> BaseEvent<Option<f64>> {
        self.on_did_change.event()
    }

    fn layout(&self, _size: f64, _offset: f64, _context: Option<&LayoutOffset>) {}
}

impl IDisposable for EmptyView {
    fn dispose(&self) {
        self.on_did_change.dispose();
    }
}

/// `toSplitViewView(view, getHeight)`: a view do grid como view do split view horizontal.
struct ViewAdapter {
    view: GridViewRef,
    get_height: Rc<dyn Fn() -> f64>,
}

impl SplitViewView<LayoutOffset> for ViewAdapter {
    fn element(&self) -> NodeId {
        self.view.element()
    }

    fn minimum_size(&self) -> f64 {
        self.view.minimum_width()
    }

    fn maximum_size(&self) -> f64 {
        self.view.maximum_width()
    }

    fn priority(&self) -> Option<LayoutPriority> {
        self.view.priority()
    }

    fn proportional_layout(&self) -> Option<bool> {
        self.view.proportional_layout()
    }

    fn snap(&self) -> Option<bool> {
        self.view.snap()
    }

    fn on_did_change(&self) -> BaseEvent<Option<f64>> {
        self.view.on_did_change().map(|e: &Option<ViewDimensions>| e.map(|d| d.width), None)
    }

    fn layout(&self, size: f64, offset: f64, context: Option<&LayoutOffset>) {
        let context = context.copied().unwrap_or_default();
        self.view.layout(size, (self.get_height)(), context.top, context.left + offset);
    }
}

pub struct CenteredViewLayout {
    ui: Ui,
    weak_self: Weak<CenteredViewLayout>,
    container: NodeId,
    view: GridViewRef,
    split_view: RefCell<Option<Rc<SplitView<LayoutOffset>>>>,
    last_layout_position: Cell<LayoutPosition>,
    style: Cell<Option<CenteredViewStyles>>,
    did_layout: Cell<bool>,
    empty_views: RefCell<Option<[Rc<EmptyView>; 2]>>,
    split_view_disposables: Rc<DisposableStore>,
    centered_layout_fixed_width: Cell<bool>,
    state: Cell<CenteredViewState>,
    boundary_sashes: RefCell<BoundarySashes>,
}

impl CenteredViewLayout {
    /// `new CenteredViewLayout(container, view, state, centeredLayoutFixedWidth)`.
    pub fn new(ui: &Ui, container: NodeId, view: GridViewRef, state: Option<CenteredViewState>, centered_layout_fixed_width: bool) -> Rc<Self> {
        {
            let mut tree = ui.tree_mut();
            tree.append_child(container, view.element());
            tree.style_mut(container).clip = true;
        }
        Rc::new_cyclic(|weak| Self {
            ui: ui.clone(),
            weak_self: weak.clone(),
            container,
            view,
            split_view: RefCell::new(None),
            last_layout_position: Cell::new(LayoutPosition::default()),
            style: Cell::new(None),
            did_layout: Cell::new(false),
            empty_views: RefCell::new(None),
            split_view_disposables: DisposableStore::new(),
            centered_layout_fixed_width: Cell::new(centered_layout_fixed_width),
            state: Cell::new(state.unwrap_or(DEFAULT_STATE)),
            boundary_sashes: RefCell::new(BoundarySashes::default()),
        })
    }

    /// O `state` (campo público do original).
    pub fn state(&self) -> CenteredViewState {
        self.state.get()
    }

    pub fn set_state(&self, state: CenteredViewState) {
        self.state.set(state);
    }

    pub fn minimum_width(&self) -> f64 {
        self.split_view.borrow().as_ref().map_or_else(|| self.view.minimum_width(), |s| s.minimum_size())
    }

    pub fn maximum_width(&self) -> f64 {
        self.split_view.borrow().as_ref().map_or_else(|| self.view.maximum_width(), |s| s.maximum_size())
    }

    pub fn minimum_height(&self) -> f64 {
        self.view.minimum_height()
    }

    pub fn maximum_height(&self) -> f64 {
        self.view.maximum_height()
    }

    pub fn on_did_change(&self) -> BaseEvent<Option<ViewDimensions>> {
        self.view.on_did_change()
    }

    pub fn boundary_sashes(&self) -> BoundarySashes {
        self.boundary_sashes.borrow().clone()
    }

    pub fn set_boundary_sashes(&self, boundary_sashes: BoundarySashes) {
        *self.boundary_sashes.borrow_mut() = boundary_sashes.clone();
        if let Some(split_view) = &*self.split_view.borrow() {
            split_view.set_orthogonal_start_sash(boundary_sashes.top);
            split_view.set_orthogonal_end_sash(boundary_sashes.bottom);
        }
    }

    /// O `SplitView` enquanto ativo.
    pub fn split_view(&self) -> Option<Rc<SplitView<LayoutOffset>>> {
        self.split_view.borrow().clone()
    }

    pub fn layout(&self, width: f64, height: f64, top: f64, left: f64) {
        let position = LayoutPosition { width, height, top, left };
        self.last_layout_position.set(position);
        let split_view = self.split_view.borrow().clone();
        if let Some(split_view) = split_view {
            split_view.layout(width, Some(LayoutOffset { top, left }));
            if !self.did_layout.get() || self.centered_layout_fixed_width.get() {
                self.resize_split_views();
            }
        } else {
            self.view.layout(width, height, top, left);
        }

        self.did_layout.set(true);
    }

    fn resize_split_views(&self) {
        let Some(split_view) = self.split_view.borrow().clone() else { return };
        let (width, state) = (self.last_layout_position.get().width, self.state.get());
        if self.centered_layout_fixed_width.get() {
            let center_view_width = width.min(state.target_width);
            let margin_width_float = (width - center_view_width) / 2.0;
            split_view.resize_view(0, margin_width_float.floor());
            split_view.resize_view(1, center_view_width);
            split_view.resize_view(2, margin_width_float.ceil());
        } else {
            let left_margin = state.left_margin_ratio * width;
            let right_margin = state.right_margin_ratio * width;
            let center = width - left_margin - right_margin;
            split_view.resize_view(0, left_margin);
            split_view.resize_view(1, center);
            split_view.resize_view(2, right_margin);
        }
    }

    pub fn set_fixed_width(&self, option: bool) {
        self.centered_layout_fixed_width.set(option);
        if self.split_view.borrow().is_some() {
            self.update_state();
            self.resize_split_views();
        }
    }

    fn update_state(&self) {
        let Some(split_view) = self.split_view.borrow().clone() else { return };
        let width = self.last_layout_position.get().width;
        self.state.set(CenteredViewState {
            target_width: split_view.get_view_size(1),
            left_margin_ratio: split_view.get_view_size(0) / width,
            right_margin_ratio: split_view.get_view_size(2) / width,
        });
    }

    pub fn is_active(&self) -> bool {
        self.split_view.borrow().is_some()
    }

    pub fn styles(&self, style: CenteredViewStyles) {
        self.style.set(Some(style));
        if let (Some(split_view), Some(empty_views)) = (&*self.split_view.borrow(), &*self.empty_views.borrow()) {
            split_view.style(style.split_view);
            let mut tree = self.ui.tree_mut();
            for view in empty_views {
                tree.style_mut(view.element).background = Some(style.background);
            }
        }
    }

    pub fn activate(&self, active: bool) {
        if active == self.is_active() {
            return;
        }

        if active {
            self.ui.tree_mut().detach(self.view.element());
            let boundary_sashes = self.boundary_sashes();
            let split_view = SplitView::<LayoutOffset>::new(
                &self.ui,
                self.container,
                SplitViewOptions { inverse_alt_behavior: true, orientation: Some(Orientation::Horizontal), styles: self.style.get().map(|s| s.split_view), ..Default::default() },
            );
            split_view.set_orthogonal_start_sash(boundary_sashes.top);
            split_view.set_orthogonal_end_sash(boundary_sashes.bottom);

            let weak = self.weak_self.clone();
            self.split_view_disposables.add(split_view.on_did_sash_change().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_state();
                }
            }));
            let weak = self.weak_self.clone();
            self.split_view_disposables.add(split_view.on_did_sash_reset().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.state.set(DEFAULT_STATE);
                    this.resize_split_views();
                }
            }));

            let position = self.last_layout_position.get();
            split_view.layout(position.width, Some(LayoutOffset { top: position.top, left: position.left }));
            let background = self.style.get().map(|s| s.background);
            let empty_views = [EmptyView::new(&self.ui, background), EmptyView::new(&self.ui, background)];

            *self.split_view.borrow_mut() = Some(split_view.clone());
            split_view.add_view(empty_views[0].clone(), Sizing::Distribute, Some(0), false);
            let weak = self.weak_self.clone();
            let get_height: Rc<dyn Fn() -> f64> = Rc::new(move || weak.upgrade().map_or(0.0, |this| this.last_layout_position.get().height));
            split_view.add_view(Rc::new(ViewAdapter { view: self.view.clone(), get_height }), Sizing::Distribute, Some(1), false);
            split_view.add_view(empty_views[1].clone(), Sizing::Distribute, Some(2), false);
            *self.empty_views.borrow_mut() = Some(empty_views);

            self.resize_split_views();
        } else {
            let split_view = self.split_view.borrow_mut().take();
            if let Some(split_view) = &split_view {
                self.ui.tree_mut().detach(split_view.element());
            }
            self.split_view_disposables.clear();
            if let Some(split_view) = split_view {
                split_view.dispose();
            }
            *self.empty_views.borrow_mut() = None;
            let position = self.last_layout_position.get();
            self.ui.tree_mut().append_child(self.container, self.view.element());
            self.view.layout(position.width, position.height, position.top, position.left);
        }
    }

    pub fn is_default(&self, state: &CenteredViewState) -> bool {
        if self.centered_layout_fixed_width.get() {
            state.target_width == DEFAULT_STATE.target_width
        } else {
            state.left_margin_ratio == DEFAULT_STATE.left_margin_ratio && state.right_margin_ratio == DEFAULT_STATE.right_margin_ratio
        }
    }
}

impl IDisposable for CenteredViewLayout {
    fn dispose(&self) {
        self.split_view_disposables.dispose();

        let split_view = self.split_view.borrow_mut().take();
        if let Some(split_view) = split_view {
            split_view.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "CenteredViewLayout"
    }
}
