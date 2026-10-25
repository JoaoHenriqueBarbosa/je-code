//! Porte de `src/vs/editor/browser/widget/multiDiffEditor/compressedVirtualizedScrollView.ts`: a lista virtualizada
//! cujos itens de altura completa viram linhas limitadas pelo viewport, com a altura removida representada por
//! rolagem local do item.
//!
//! Decisões de tradução:
//! - **Sem DOM.** O original monta `div.scrollContent`, um `SmoothScrollableElement`, um
//!   `ObservableElementSizeObserver` e escreve `height` e `transform` no estilo. A renderização do porte é nativa
//!   (`CLAUDE.md`): a vista guarda o [`Scrollable`] (o que o `SmoothScrollableElement` repassa em
//!   `setScrollPosition`, `getScrollPosition` e `onScroll`) e recebe a dimensão por um observável, lida direto
//!   no `autorun` (o `ObservableElementSizeObserver.observe(dimension)` só repassa a dimensão dada). O `domNode`, o
//!   `elementToObserve`, o `contentDomNode` e o `overflowWidgetsDomNode` não existem; o contexto de criação dos itens
//!   traz só o `scrollLeft`. A quem pinta (o item) cabe usar o `renderedViewport` no lugar do `translateY`.
//! - Os números do layout são `i32` (ver [`crate::compressed_virtualized_scroll_layout`]); a rolagem do
//!   `Scrollable` é `f64` e é arredondada ao passar para o layout.
//! - O valor de um observável do original é um objeto comparado por identidade (`===`): o layout, a dimensão da
//!   rolagem e a edição de geometria são `Rc`, e a comparação é `Rc::ptr_eq`.
//! - O contexto do item ([`ICompressedVirtualizedScrollItemContext`]) guarda `Weak` da vista e do item (o item guarda
//!   o contexto, que ficaria num ciclo de `Rc`).
//! - `schedule_at_next_animation_frame` (o `scheduleAtNextAnimationFrame(getWindow(elementToObserve), ...)`) vem de
//!   quem cria a vista.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::arrays::{compare_by, number_comparator};
use je_base::arrays_find::find_first_max;
use je_base::async_::RunOnceScheduler;
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::deps::try_finally;
use je_base::observable::{
    IObservable, ITransaction, Observable, ObservableSignal, SettableExt, autorun, global_transaction, observable_from_event, observable_signal, observable_value,
};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::scrollable::{INewScrollDimensions, INewScrollPosition, IScrollPosition, IScrollableOptions, ScheduleAtNextAnimationFrame, Scrollable};

use crate::compressed_virtualized_scroll_layout::{
    CompressedVirtualizedItemVisibility, ICompressedVirtualizedScrollLayout, ICompressedVirtualizedScrollLayoutInput, ILogicalPosition, ISizeEdit, LayoutRevision,
    as_layout_revision, compute_compressed_virtualized_scroll_height, compute_compressed_virtualized_scroll_layout, compute_item_ranges,
    create_anchored_size_edit_batch, map_logical_position,
};
use crate::core_2d::dimension::Dimension;
use crate::offset_range::OffsetRange;

const SCROLL_DIRECTION_RETENTION_DURATION_MS: u64 = 100;

/// `{ readonly maxScroll: number }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaxScroll {
    pub max_scroll: f64,
}

/// `ICompressedVirtualizedScrollItem`.
pub trait ICompressedVirtualizedScrollItem {
    fn size(&self) -> IObservable<i32>;
    fn max_scroll(&self) -> IObservable<MaxScroll>;
    fn render(&self, rendered_range: OffsetRange, scroll_offset: i32, width: f64, rendered_viewport: OffsetRange, context: Rc<dyn ICompressedVirtualizedScrollItemContext>);
    fn hide(&self);
}

/// `ICompressedVirtualizedScrollViewContext` (sem os nós do DOM; ver a nota do módulo).
#[derive(Clone)]
pub struct ICompressedVirtualizedScrollViewContext {
    pub scroll_left: IObservable<f64>,
}

/// `ICompressedVirtualizedScrollItemContext`.
pub trait ICompressedVirtualizedScrollItemContext {
    fn run_with_scroll_anchor(&self, get_item_offset: Rc<dyn Fn() -> i32>, update: &dyn Fn(&dyn ITransaction));
}

/// O `anchorKind` de uma edição de geometria.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorKind {
    ViewportTop,
    ViewportBottom,
    Logical,
    Item,
}

/// `ICompressedVirtualizedGeometryEdit`.
#[derive(Clone, Debug, PartialEq)]
pub struct ICompressedVirtualizedGeometryEdit {
    pub from_revision: LayoutRevision,
    pub to_revision: LayoutRevision,
    pub edits: Vec<ISizeEdit>,
    pub anchor_kind: AnchorKind,
    pub anchor_offset: i32,
    pub mapped_anchor_offset: i32,
    pub anchor_viewport_offset: i32,
    pub desired_scroll_top: i32,
    pub applied_scroll_top: i32,
    pub leading_scroll_slack: i32,
    pub trailing_scroll_slack: i32,
}

/// `{ width, height, scrollWidth, scrollHeight }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollDimensions {
    pub width: f64,
    pub height: f64,
    pub scroll_width: f64,
    pub scroll_height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScrollDirection {
    Up,
    Down,
}

/// `_pendingAnchor`.
struct PendingAnchor<TItem> {
    position: ILogicalPosition,
    viewport_offset: i32,
    item: Option<Rc<TItem>>,
    get_item_offset: Option<Rc<dyn Fn() -> i32>>,
}

fn throw_bug(message: &str) -> ! {
    errors::throw(Error::bug_indicating(Some(message)))
}

fn to_pixels(value: f64) -> i32 {
    value.round() as i32
}

/// Virtualizes complete-height items into viewport-capped rows whose removed height is represented by item-local scrolling.
pub struct CompressedVirtualizedScrollView<TItem: ICompressedVirtualizedScrollItem + 'static> {
    base: Rc<Disposable>,
    me: Weak<Self>,
    scrollable: Rc<Scrollable>,
    items: IObservable<Rc<Vec<Rc<TItem>>>>,
    layout_value: Rc<ObservableValue<Rc<ICompressedVirtualizedScrollLayout>, ()>>,
    scroll_dimensions_value: Rc<ObservableValue<Rc<ScrollDimensions>, ()>>,
    last_geometry_edit_value: Rc<ObservableValue<Option<Rc<ICompressedVirtualizedGeometryEdit>>, ()>>,
    item_anchor_edit_signal: Rc<ObservableSignal<()>>,
    revision: Cell<LayoutRevision>,
    leading_scroll_slack: Cell<i32>,
    trailing_scroll_slack: Cell<i32>,
    previous_items: RefCell<Option<Vec<Rc<TItem>>>>,
    previous_item_heights: RefCell<Option<Vec<i32>>>,
    previous_item_gap: Cell<i32>,
    last_scroll_top: Cell<i32>,
    last_scroll_direction: Rc<Cell<Option<ScrollDirection>>>,
    clear_scroll_direction: Rc<RunOnceScheduler>,
    is_updating: Cell<bool>,
    pending_anchor: RefCell<Option<Rc<PendingAnchor<TItem>>>>,

    pub scroll_top: IObservable<f64>,
    pub scroll_left: IObservable<f64>,
    pub layout: IObservable<Rc<ICompressedVirtualizedScrollLayout>>,
    pub scroll_dimensions: IObservable<Rc<ScrollDimensions>>,
    pub last_geometry_edit: IObservable<Option<Rc<ICompressedVirtualizedGeometryEdit>>>,
}

struct ItemContext<TItem: ICompressedVirtualizedScrollItem + 'static> {
    view: Weak<CompressedVirtualizedScrollView<TItem>>,
    item: Weak<TItem>,
}

impl<TItem: ICompressedVirtualizedScrollItem + 'static> ICompressedVirtualizedScrollItemContext for ItemContext<TItem> {
    fn run_with_scroll_anchor(&self, get_item_offset: Rc<dyn Fn() -> i32>, update: &dyn Fn(&dyn ITransaction)) {
        if let (Some(view), Some(item)) = (self.view.upgrade(), self.item.upgrade()) {
            view.run_with_item_scroll_anchor(&item, get_item_offset, update);
        }
    }
}

impl<TItem: ICompressedVirtualizedScrollItem + 'static> CompressedVirtualizedScrollView<TItem> {
    pub fn new(
        dimension: IObservable<Option<Dimension>>,
        item_gap: IObservable<i32>,
        schedule_at_next_animation_frame: ScheduleAtNextAnimationFrame,
        create_items: impl FnOnce(ICompressedVirtualizedScrollViewContext) -> IObservable<Rc<Vec<Rc<TItem>>>>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let scrollable = Scrollable::new(IScrollableOptions { force_integer_values: false, smooth_scroll_duration: 100.0, schedule_at_next_animation_frame });
        base.register(scrollable.clone());
        let scroll_top = {
            let scrollable = scrollable.clone();
            observable_from_event(scrollable.on_scroll(), move |_| scrollable.get_current_scroll_position().scroll_top)
        };
        let scroll_left = {
            let scrollable = scrollable.clone();
            observable_from_event(scrollable.on_scroll(), move |_| scrollable.get_current_scroll_position().scroll_left)
        };
        let items = create_items(ICompressedVirtualizedScrollViewContext { scroll_left: scroll_left.clone() });
        let layout_value = observable_value::<Rc<ICompressedVirtualizedScrollLayout>, ()>(
            "layout",
            Rc::new(compute_compressed_virtualized_scroll_layout(&ICompressedVirtualizedScrollLayoutInput {
                revision: Some(as_layout_revision(0)),
                scroll_top: 0,
                viewport_height: 0,
                item_gap: 0,
                item_heights: &[],
                ..ICompressedVirtualizedScrollLayoutInput::default()
            })),
        );
        let scroll_dimensions_value = observable_value::<Rc<ScrollDimensions>, ()>("scrollDimensions", Rc::new(ScrollDimensions { width: 0.0, height: 0.0, scroll_width: 0.0, scroll_height: 0.0 }));
        let last_geometry_edit_value = observable_value::<Option<Rc<ICompressedVirtualizedGeometryEdit>>, ()>("lastGeometryEdit", None);
        let last_scroll_direction: Rc<Cell<Option<ScrollDirection>>> = Rc::new(Cell::new(None));
        let clear_scroll_direction = {
            let last_scroll_direction = last_scroll_direction.clone();
            RunOnceScheduler::new(move || last_scroll_direction.set(None), SCROLL_DIRECTION_RETENTION_DURATION_MS)
        };
        base.register(clear_scroll_direction.clone());

        let view = Rc::new_cyclic(|me| Self {
            base,
            me: me.clone(),
            scrollable,
            items,
            layout: layout_value.to_observable(),
            layout_value,
            scroll_dimensions: scroll_dimensions_value.to_observable(),
            scroll_dimensions_value,
            last_geometry_edit: last_geometry_edit_value.to_observable(),
            last_geometry_edit_value,
            item_anchor_edit_signal: observable_signal::<()>("itemAnchorEdit"),
            revision: Cell::new(as_layout_revision(0)),
            leading_scroll_slack: Cell::new(0),
            trailing_scroll_slack: Cell::new(0),
            previous_items: RefCell::new(None),
            previous_item_heights: RefCell::new(None),
            previous_item_gap: Cell::new(0),
            last_scroll_top: Cell::new(0),
            last_scroll_direction,
            clear_scroll_direction,
            is_updating: Cell::new(false),
            pending_anchor: RefCell::new(None),
            scroll_top,
            scroll_left,
        });

        let weak = Rc::downgrade(&view);
        view.base.register(autorun(move |reader| {
            let Some(this) = weak.upgrade() else { return };
            this.item_anchor_edit_signal.read(reader);
            let dimension = dimension.read(reader);
            let (width, height) = dimension.map_or((0.0, 0.0), |d| (d.width, d.height));
            let items = this.items.read(reader);
            let item_heights: Vec<i32> = items.iter().map(|item| item.size().read(reader)).collect();
            let max = find_first_max(items.as_slice(), compare_by(|item: &Rc<TItem>| item.max_scroll().read(reader).max_scroll, number_comparator));
            let max_scroll = max.map_or(0.0, |item| item.max_scroll().read(reader).max_scroll);
            let gap = item_gap.read(reader);
            let requested_scroll_top = to_pixels(this.scroll_top.read(reader));
            global_transaction(|tx| this.update(items.as_slice(), &item_heights, gap, width, height, width + max_scroll, requested_scroll_top, tx));
        }));
        view
    }

    /// `setScrollPosition(position, smooth)`.
    pub fn set_scroll_position(&self, position: INewScrollPosition, smooth: bool) {
        if smooth {
            self.scrollable.set_scroll_position_smooth(&position, true);
        } else {
            self.scrollable.set_scroll_position_now(&position);
        }
    }

    /// `setLogicalScrollPosition(scrollTop, smooth)`.
    pub fn set_logical_scroll_position(&self, scroll_top: i32, smooth: bool) {
        self.set_scroll_position(INewScrollPosition { scroll_left: None, scroll_top: Some(f64::from(self.leading_scroll_slack.get() + scroll_top)) }, smooth);
    }

    /// `getScrollPosition()`.
    pub fn get_scroll_position(&self) -> IScrollPosition {
        self.scrollable.get_current_scroll_position()
    }

    /// `runWithScrollAnchor(anchor, update)`.
    pub fn run_with_scroll_anchor(&self, anchor: ILogicalPosition, update: &dyn Fn(&dyn ITransaction)) {
        if anchor.revision != self.revision.get() {
            throw_bug(&format!("Cannot use layout revision {} as an anchor for revision {}", anchor.revision, self.revision.get()));
        }
        let logical_scroll_height = self.layout.get().logical_scroll_height;
        if anchor.offset < 0 || anchor.offset > logical_scroll_height {
            throw_bug(&format!("Logical anchor {} is outside layout revision {}", anchor.offset, self.revision.get()));
        }
        let logical_viewport_top = to_pixels(self.get_scroll_position().scroll_top) - self.leading_scroll_slack.get();
        self.run_with_pending_anchor(PendingAnchor { position: anchor, viewport_offset: anchor.offset - logical_viewport_top, item: None, get_item_offset: None }, update);
    }

    fn run_with_item_scroll_anchor(&self, item: &Rc<TItem>, get_item_offset: Rc<dyn Fn() -> i32>, update: &dyn Fn(&dyn ITransaction)) {
        let items = self.items.get();
        let Some(item_index) = items.iter().position(|candidate| Rc::ptr_eq(candidate, item)) else {
            throw_bug("Cannot anchor an item that is not in the compacted virtualized scroll view");
        };
        let item_range = self.layout.get().items[item_index].content_range;
        let item_offset = get_item_offset();
        if item_offset < 0 || item_offset > item_range.length() {
            throw_bug(&format!("Item anchor {item_offset} is outside item range {item_range}"));
        }
        let position = ILogicalPosition { revision: self.revision.get(), offset: item_range.start + item_offset };
        let logical_viewport_top = to_pixels(self.get_scroll_position().scroll_top) - self.leading_scroll_slack.get();
        let signal = self.item_anchor_edit_signal.clone();
        let offset_getter = get_item_offset.clone();
        self.run_with_pending_anchor(
            PendingAnchor { position, viewport_offset: position.offset - logical_viewport_top, item: Some(item.clone()), get_item_offset: Some(get_item_offset) },
            &move |tx| {
                update(tx);
                if offset_getter() != item_offset {
                    signal.trigger(Some(tx), ());
                }
            },
        );
    }

    fn run_with_pending_anchor(&self, pending_anchor: PendingAnchor<TItem>, update: &dyn Fn(&dyn ITransaction)) {
        if self.pending_anchor.borrow().is_some() {
            throw_bug("Cannot nest compacted virtualized scroll anchors");
        }
        let pending_anchor = Rc::new(pending_anchor);
        *self.pending_anchor.borrow_mut() = Some(pending_anchor.clone());
        try_finally(
            || global_transaction(|tx| update(tx)),
            || {
                let current_is_ours = self.pending_anchor.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, &pending_anchor));
                if current_is_ours {
                    *self.pending_anchor.borrow_mut() = None;
                }
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn update(&self, items: &[Rc<TItem>], item_heights: &[i32], item_gap: i32, width: f64, height: f64, scroll_width: f64, requested_scroll_top: i32, tx: &dyn ITransaction) {
        if self.is_updating.get() {
            return;
        }
        self.is_updating.set(true);
        try_finally(
            || self.update_inner(items, item_heights, item_gap, width, height, scroll_width, requested_scroll_top, tx),
            || self.is_updating.set(false),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn update_inner(&self, items: &[Rc<TItem>], item_heights: &[i32], item_gap: i32, width: f64, height: f64, scroll_width: f64, requested_scroll_top: i32, tx: &dyn ITransaction) {
        let viewport_height = to_pixels(height);
        let mut target_scroll_top = requested_scroll_top;
        let scroll_delta = requested_scroll_top - self.last_scroll_top.get();
        if scroll_delta != 0 {
            self.last_scroll_direction.set(Some(if scroll_delta < 0 { ScrollDirection::Up } else { ScrollDirection::Down }));
            self.clear_scroll_direction.schedule(None);
        }
        let previous_item_heights = self.previous_item_heights.borrow().clone();
        let geometry_changed = previous_item_heights.as_deref().is_some_and(|previous| item_gap != self.previous_item_gap.get() || previous != item_heights);
        let previous_items = self.previous_items.borrow().clone();
        let has_stable_items = previous_items.as_ref().is_some_and(|previous| previous.len() == items.len() && previous.iter().zip(items).all(|(a, b)| Rc::ptr_eq(a, b)));
        let items_changed = previous_items.is_some() && !has_stable_items;
        let mut did_apply_anchor = false;

        let pending_anchor = self.pending_anchor.borrow().clone();
        let applies_pending_anchor = pending_anchor.is_some() && has_stable_items;
        if let Some(previous_item_heights) = previous_item_heights.as_deref().filter(|_| (geometry_changed || applies_pending_anchor) && has_stable_items) {
            let previous_item_gap = self.previous_item_gap.get();
            let old_logical_scroll_height = compute_compressed_virtualized_scroll_height(previous_item_heights, previous_item_gap);
            let old_logical_viewport_top = requested_scroll_top - self.leading_scroll_slack.get();
            let is_scrolling_up = self.last_scroll_direction.get() == Some(ScrollDirection::Up);
            let default_anchor_viewport_offset = if is_scrolling_up { viewport_height } else { 0 };
            let anchor_kind = match &pending_anchor {
                Some(pending) if pending.item.is_some() => AnchorKind::Item,
                Some(_) => AnchorKind::Logical,
                None if is_scrolling_up => AnchorKind::ViewportBottom,
                None => AnchorKind::ViewportTop,
            };
            let anchor_offset = match &pending_anchor {
                Some(pending) => pending.position.offset,
                None => (old_logical_viewport_top + default_anchor_viewport_offset).min(old_logical_scroll_height).max(0),
            };
            let anchor_viewport_offset = match &pending_anchor {
                Some(pending) => pending.viewport_offset,
                None => anchor_offset - old_logical_viewport_top,
            };
            let from_revision = self.revision.get();
            let to_revision = if geometry_changed { as_layout_revision(from_revision.0 + 1) } else { from_revision };
            let edit = create_anchored_size_edit_batch(from_revision, to_revision, previous_item_heights, item_heights, previous_item_gap, item_gap, anchor_offset);
            let mut mapped_anchor = map_logical_position(edit.anchor, &edit);
            if let Some(pending) = &pending_anchor
                && let (Some(pending_item), Some(get_item_offset)) = (&pending.item, &pending.get_item_offset)
            {
                let Some(item_index) = items.iter().position(|item| Rc::ptr_eq(item, pending_item)) else {
                    throw_bug("Compacted virtualized scroll anchor item was removed during its edit");
                };
                let item_range = compute_item_ranges(item_heights, item_gap)[item_index];
                let item_offset = get_item_offset();
                if item_offset < 0 || item_offset > item_range.length() {
                    throw_bug(&format!("Mapped item anchor {item_offset} is outside item range {item_range}"));
                }
                mapped_anchor = ILogicalPosition { revision: to_revision, offset: item_range.start + item_offset };
            }
            let new_logical_scroll_height = compute_compressed_virtualized_scroll_height(item_heights, item_gap);
            let desired_logical_scroll_top = mapped_anchor.offset - anchor_viewport_offset;
            let natural_max_scroll_top = (new_logical_scroll_height - viewport_height).max(0);

            self.leading_scroll_slack.set((-desired_logical_scroll_top).max(0));
            self.trailing_scroll_slack.set((desired_logical_scroll_top - natural_max_scroll_top).max(0));
            target_scroll_top = self.leading_scroll_slack.get() + desired_logical_scroll_top;
            self.revision.set(to_revision);
            *self.pending_anchor.borrow_mut() = None;
            did_apply_anchor = true;
            self.last_geometry_edit_value.set(
                Some(Rc::new(ICompressedVirtualizedGeometryEdit {
                    from_revision,
                    to_revision,
                    edits: edit.edits,
                    anchor_kind,
                    anchor_offset,
                    mapped_anchor_offset: mapped_anchor.offset,
                    anchor_viewport_offset,
                    desired_scroll_top: target_scroll_top,
                    applied_scroll_top: target_scroll_top,
                    leading_scroll_slack: self.leading_scroll_slack.get(),
                    trailing_scroll_slack: self.trailing_scroll_slack.get(),
                })),
                Some(tx),
            );
        } else if geometry_changed || items_changed {
            if geometry_changed {
                self.revision.set(as_layout_revision(self.revision.get().0 + 1));
            }
            self.leading_scroll_slack.set(0);
            self.trailing_scroll_slack.set(0);
            self.last_geometry_edit_value.set(None, Some(tx));
        } else if scroll_delta > 0 && self.leading_scroll_slack.get() > 0 {
            let consumed_slack = scroll_delta.min(self.leading_scroll_slack.get());
            self.leading_scroll_slack.set(self.leading_scroll_slack.get() - consumed_slack);
            target_scroll_top -= consumed_slack;
        } else if scroll_delta < 0 && self.trailing_scroll_slack.get() > 0 {
            let logical_scroll_height = compute_compressed_virtualized_scroll_height(item_heights, item_gap);
            let natural_max_scroll_top = self.leading_scroll_slack.get() + (logical_scroll_height - viewport_height).max(0);
            self.trailing_scroll_slack.set((target_scroll_top - natural_max_scroll_top).max(0));
        }

        let compute_layout = |scroll_top: i32| {
            compute_compressed_virtualized_scroll_layout(&ICompressedVirtualizedScrollLayoutInput {
                revision: Some(self.revision.get()),
                scroll_top,
                viewport_height,
                item_gap,
                item_heights,
                leading_scroll_slack: Some(self.leading_scroll_slack.get()),
                trailing_scroll_slack: Some(self.trailing_scroll_slack.get()),
            })
        };
        let mut layout = compute_layout(target_scroll_top);
        let dimensions = ScrollDimensions { width, height, scroll_width, scroll_height: f64::from(layout.scroll_height) };
        self.scrollable.set_scroll_dimensions(
            &INewScrollDimensions { width: Some(dimensions.width), scroll_width: Some(dimensions.scroll_width), height: Some(dimensions.height), scroll_height: Some(dimensions.scroll_height) },
            true,
        );
        let needs_immediate_scroll_correction = geometry_changed || target_scroll_top != requested_scroll_top;
        if needs_immediate_scroll_correction {
            self.scrollable.set_scroll_position_now(&INewScrollPosition { scroll_left: None, scroll_top: Some(f64::from(layout.scroll_top)) });
        }
        let applied_scroll_top = to_pixels(self.get_scroll_position().scroll_top);
        if applied_scroll_top != layout.scroll_top {
            layout = compute_layout(applied_scroll_top);
        }
        let geometry_edit = self.last_geometry_edit_value.get();
        if did_apply_anchor && let Some(geometry_edit) = geometry_edit {
            self.last_geometry_edit_value.set(Some(Rc::new(ICompressedVirtualizedGeometryEdit { applied_scroll_top, ..(*geometry_edit).clone() })), Some(tx));
        }
        let layout = Rc::new(layout);
        self.layout_value.set(layout.clone(), Some(tx));
        self.scroll_dimensions_value.set(Rc::new(dimensions), Some(tx));
        self.render(items, &layout, width);
        *self.previous_items.borrow_mut() = Some(items.to_vec());
        *self.previous_item_heights.borrow_mut() = Some(item_heights.to_vec());
        self.previous_item_gap.set(item_gap);
        self.last_scroll_top.set(layout.scroll_top);
    }

    fn render(&self, items: &[Rc<TItem>], layout: &ICompressedVirtualizedScrollLayout, width: f64) {
        for (index, item) in items.iter().enumerate() {
            let item_layout = &layout.items[index];
            if item_layout.visibility != CompressedVirtualizedItemVisibility::Visible {
                item.hide();
            } else {
                let context: Rc<dyn ICompressedVirtualizedScrollItemContext> = Rc::new(ItemContext { view: self.me.clone(), item: Rc::downgrade(item) });
                item.render(item_layout.rendered_range, item_layout.scroll_offset, width, layout.rendered_viewport, context);
            }
        }
    }
}

impl<TItem: ICompressedVirtualizedScrollItem + 'static> IDisposable for CompressedVirtualizedScrollView<TItem> {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}
