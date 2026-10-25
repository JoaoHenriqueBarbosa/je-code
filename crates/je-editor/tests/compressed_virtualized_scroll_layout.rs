//! Porte de `src/vs/editor/test/browser/widget/compressedVirtualizedScrollLayout.test.ts` (suíte
//! `CompressedVirtualizedScrollLayout`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O original cria um `container` no `document.body` para a vista. O porte é nativo e a vista não tem DOM (ver o
//!   cabeçalho de `compressed_virtualized_scroll_view.rs`): o `container`, o `container.remove()` e o
//!   `container.appendChild(view.domNode)` não existem; a dimensão `new Dimension(800, 480)` entra por um observável.
//! - Os resultados de `deepStrictEqual` sobre objetos são tuplas e structs locais com `PartialEq`.
//! - O valor `observableValue<readonly TestCompressedScrollItem[]>` e o `constObservable([...])` da lista de itens são
//!   `Rc<Vec<...>>` (um array do JavaScript é comparado por identidade).
//! - `scheduleAtNextAnimationFrame` é um `setTimeout` de 16 ms.
//! - O caso assíncrono roda com `run_with_real_timers`, como o `await timeout(...)` de relógio real do original.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/random.rs"]
mod random;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::observable::{IObservable, ITransaction, Observable, SettableExt, const_observable, observable_value, transaction};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::runtime;
use je_base::scrollable::{INewScrollPosition, ScheduleAtNextAnimationFrame};
use je_editor::compressed_virtualized_scroll_layout::{
    CompressedVirtualizedItemVisibility, ICompressedVirtualizedScrollLayoutInput, ILogicalPosition, as_layout_revision, compute_compressed_virtualized_scroll_layout,
    create_anchored_size_edit_batch, map_logical_position,
};
use je_editor::compressed_virtualized_scroll_view::{
    AnchorKind, CompressedVirtualizedScrollView, ICompressedVirtualizedScrollItem, ICompressedVirtualizedScrollItemContext, MaxScroll,
};
use je_editor::core_2d::dimension::Dimension;
use je_editor::offset_range::OffsetRange;
use random::Random;

type Items = Rc<Vec<Rc<TestCompressedScrollItem>>>;
type TestView = CompressedVirtualizedScrollView<TestCompressedScrollItem>;

fn visibility_name(visibility: CompressedVirtualizedItemVisibility) -> &'static str {
    match visibility {
        CompressedVirtualizedItemVisibility::Before => "before",
        CompressedVirtualizedItemVisibility::Visible => "visible",
        CompressedVirtualizedItemVisibility::After => "after",
    }
}

fn animation_frame_scheduler() -> ScheduleAtNextAnimationFrame {
    Rc::new(|callback: Box<dyn FnOnce()>| -> DisposableRef {
        let timer = runtime::set_timeout(callback, 16);
        to_disposable(move || runtime::clear_timeout(timer))
    })
}

fn create_view(width: f64, height: f64, item_gap: i32, items: IObservable<Items>) -> Rc<TestView> {
    CompressedVirtualizedScrollView::new(const_observable(Some(Dimension { width, height })), const_observable(item_gap), animation_frame_scheduler(), move |_| items)
}

fn scroll_to(scroll_top: f64) -> INewScrollPosition {
    INewScrollPosition { scroll_left: None, scroll_top: Some(scroll_top) }
}

fn const_items(items: &[&Rc<TestCompressedScrollItem>]) -> IObservable<Items> {
    const_observable(Rc::new(items.iter().map(|item| Rc::clone(item)).collect()))
}

#[test]
fn describes_mixed_height_content_in_complete_and_rendered_coordinates() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let layout = compute_compressed_virtualized_scroll_layout(&ICompressedVirtualizedScrollLayoutInput {
        scroll_top: 500,
        viewport_height: 400,
        item_gap: 10,
        item_heights: &[200, 900, 300],
        ..ICompressedVirtualizedScrollLayoutInput::default()
    });

    let items: Vec<(String, String, i32, i32, &str)> = layout
        .items
        .iter()
        .map(|item| (item.content_range.to_string(), item.rendered_range.to_string(), item.max_scroll_offset, item.scroll_offset, visibility_name(item.visibility)))
        .collect();
    assert_eq!(
        (layout.scroll_top, layout.scroll_height, layout.rendered_height, layout.rendered_viewport.start, layout.hidden_content_height_above_viewport, items),
        (
            500,
            1420,
            920,
            210,
            290,
            vec![
                ("[0, 200)".to_owned(), "[0, 200)".to_owned(), 0, 0, "before"),
                ("[210, 1110)".to_owned(), "[210, 610)".to_owned(), 500, 290, "visible"),
                ("[1120, 1420)".to_owned(), "[620, 920)".to_owned(), 0, 0, "after"),
            ],
        )
    );
}

#[test]
fn conserves_vertical_displacement() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut random = Random::create(873245);
    let mut failures: Vec<String> = Vec::new();

    for case_index in 0..2000 {
        let viewport_height = random.next_int_range(1, 1000) as i32;
        let item_gap = random.next_int_range(0, 41) as i32;
        let length = random.next_int_range(0, 31);
        let item_heights: Vec<i32> = (0..length).map(|_| random.next_int_range(0, 2501) as i32).collect();
        let initial_scroll_top = random.next_int_range(0, 50001) as i32;
        let initial = compute_compressed_virtualized_scroll_layout(&ICompressedVirtualizedScrollLayoutInput {
            scroll_top: initial_scroll_top,
            viewport_height,
            item_gap,
            item_heights: &item_heights,
            ..ICompressedVirtualizedScrollLayoutInput::default()
        });
        let next_scroll_top = initial.scroll_top + random.next_int_range(-2000, 2001) as i32;
        let next = compute_compressed_virtualized_scroll_layout(&ICompressedVirtualizedScrollLayoutInput {
            scroll_top: next_scroll_top,
            viewport_height,
            item_gap,
            item_heights: &item_heights,
            ..ICompressedVirtualizedScrollLayoutInput::default()
        });

        let scroll_delta = next.scroll_top - initial.scroll_top;
        let rendered_delta = next.rendered_viewport.start - initial.rendered_viewport.start;
        let hidden_delta = next.hidden_content_height_above_viewport - initial.hidden_content_height_above_viewport;
        let residual = scroll_delta - rendered_delta - hidden_delta;
        let invalid_item = next.items.iter().find(|item| {
            item.scroll_offset < 0 || item.scroll_offset > item.max_scroll_offset || item.max_scroll_offset != (item.content_range.length() - item.rendered_range.length()).max(0)
        });

        if f64::from(residual).abs() > 0.0001 || invalid_item.is_some() {
            failures.push(format!("case {case_index}: residual={residual}, invalidItem={}", invalid_item.is_some()));
            if failures.len() == 10 {
                break;
            }
        }
    }

    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn keeps_transient_scroll_slack_outside_logical_item_ranges() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let layout = compute_compressed_virtualized_scroll_layout(&ICompressedVirtualizedScrollLayoutInput {
        revision: Some(as_layout_revision(7)),
        scroll_top: 0,
        viewport_height: 400,
        item_gap: 10,
        item_heights: &[200, 300],
        leading_scroll_slack: Some(100),
        trailing_scroll_slack: Some(150),
    });

    let item_ranges: Vec<String> = layout.items.iter().map(|item| item.content_range.to_string()).collect();
    let rendered_ranges: Vec<String> = layout.items.iter().map(|item| item.rendered_range.to_string()).collect();
    assert_eq!(
        (layout.revision, layout.logical_scroll_height, layout.scroll_height, layout.rendered_height, layout.content_viewport.to_string(), item_ranges, rendered_ranges),
        (
            as_layout_revision(7),
            510,
            760,
            760,
            "[-100, 300)".to_owned(),
            vec!["[0, 200)".to_owned(), "[210, 510)".to_owned()],
            vec!["[100, 300)".to_owned(), "[310, 610)".to_owned()],
        )
    );
}

#[test]
fn maps_a_revisioned_viewport_anchor_through_simultaneous_size_edits() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let edit = create_anchored_size_edit_batch(as_layout_revision(3), as_layout_revision(4), &[200, 900, 300], &[300, 40, 500], 10, 10, 750);

    let edits: Vec<(String, String)> = edit.edits.iter().map(|e| (e.old_range.to_string(), e.new_range.to_string())).collect();
    assert_eq!(
        (
            edits,
            edit.anchor,
            map_logical_position(edit.anchor, &edit),
            map_logical_position(ILogicalPosition { revision: as_layout_revision(3), offset: 1420 }, &edit),
        ),
        (
            vec![
                ("[0, 200)".to_owned(), "[0, 300)".to_owned()),
                ("[210, 1110)".to_owned(), "[310, 350)".to_owned()),
                ("[1120, 1420)".to_owned(), "[360, 860)".to_owned()),
            ],
            ILogicalPosition { revision: as_layout_revision(3), offset: 750 },
            ILogicalPosition { revision: as_layout_revision(4), offset: 310 },
            ILogicalPosition { revision: as_layout_revision(4), offset: 860 },
        )
    );
}

#[test]
fn keeps_the_rendered_anchor_stable_when_content_grows_above_it() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let item_a = TestCompressedScrollItem::new(260);
    let item_b = TestCompressedScrollItem::new(1100);
    let view = ds.add_rc(create_view(800.0, 480.0, 12, const_items(&[&item_a, &item_b])));
    view.set_scroll_position(scroll_to(800.0), false);

    let get_state = || {
        let layout = view.layout.get();
        (view.get_scroll_position().scroll_top, layout.items[1].scroll_offset, layout.items[1].rendered_range.start - layout.rendered_viewport.start)
    };
    let before = get_state();

    item_a.set_size(360, None);
    let after_preceding_growth = get_state();
    item_b.set_size(1200, None);
    let after_anchor_growth = get_state();

    assert_eq!((before, after_preceding_growth, after_anchor_growth), ((800.0, 528, 0), (900.0, 528, 0), (900.0, 528, 0)));
}

#[test]
fn does_not_treat_item_local_scrolling_as_anchor_movement() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let item_a = TestCompressedScrollItem::new(260);
    let item_b = TestCompressedScrollItem::new(1100);
    let view = ds.add_rc(create_view(800.0, 480.0, 12, const_items(&[&item_a, &item_b])));

    let requested_scroll_tops = [400.0, 800.0, 700.0, 200.0, 850.0];
    let applied_scroll_tops: Vec<f64> = requested_scroll_tops
        .iter()
        .map(|&scroll_top| {
            view.set_scroll_position(scroll_to(scroll_top), false);
            view.get_scroll_position().scroll_top
        })
        .collect();

    assert_eq!(applied_scroll_tops, requested_scroll_tops.to_vec());
}

#[test]
fn preserves_geometry_compensation_during_outer_scrolling() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let item_a = TestCompressedScrollItem::new(260);
    let item_b = TestCompressedScrollItem::new(1100);
    let view = ds.add_rc(create_view(800.0, 480.0, 12, const_items(&[&item_a, &item_b])));
    view.set_scroll_position(scroll_to(800.0), false);

    transaction(|tx| {
        item_a.set_size(360, Some(tx));
        view.set_scroll_position(scroll_to(700.0), false);
    });

    assert_eq!((view.get_scroll_position().scroll_top, view.layout.get().items[1].scroll_offset), (800.0, 428));
}

#[test]
fn uses_trailing_slack_to_preserve_a_near_bottom_anchor_and_consumes_it_when_scrolling_up() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let item_a = TestCompressedScrollItem::new(500);
    let item_b = TestCompressedScrollItem::new(500);
    let view = ds.add_rc(create_view(800.0, 400.0, 12, const_items(&[&item_a, &item_b])));
    view.set_scroll_position(scroll_to(600.0), false);

    item_b.set_size(100, None);
    let after_shrink = view.layout.get();
    view.set_scroll_position(scroll_to(500.0), false);
    let after_scroll_up = view.layout.get();

    assert_eq!(
        (
            (after_shrink.scroll_top, after_shrink.logical_scroll_height, after_shrink.scroll_height, after_shrink.trailing_scroll_slack),
            (after_scroll_up.scroll_top, after_scroll_up.scroll_height, after_scroll_up.trailing_scroll_slack),
        ),
        ((600, 612, 1000, 388), (500, 900, 288))
    );
}

#[test]
fn uses_leading_slack_for_an_operation_anchor_and_consumes_it_when_scrolling_down() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let item_a = TestCompressedScrollItem::new(100);
    let item_b = TestCompressedScrollItem::new(500);
    let view = ds.add_rc(create_view(800.0, 400.0, 12, const_items(&[&item_a, &item_b])));
    let revision = view.layout.get().revision;

    view.run_with_scroll_anchor(ILogicalPosition { revision, offset: 100 }, &|tx| item_a.set_size(50, Some(tx)));
    let after_shrink = view.layout.get();
    view.set_scroll_position(scroll_to(100.0), false);
    let after_scroll_down = view.layout.get();

    assert_eq!(
        (
            (after_shrink.scroll_top, after_shrink.content_viewport.to_string(), after_shrink.leading_scroll_slack),
            (after_scroll_down.scroll_top, after_scroll_down.content_viewport.to_string(), after_scroll_down.leading_scroll_slack),
        ),
        ((0, "[-50, 350)".to_owned(), 50), (50, "[50, 450)".to_owned(), 0))
    );
}

#[test]
fn resets_transient_scroll_state_when_equal_height_items_are_replaced() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let item_a = TestCompressedScrollItem::new(100);
    let item_b = TestCompressedScrollItem::new(500);
    let items = observable_value::<Items, ()>("items", Rc::new(vec![Rc::clone(&item_a), Rc::clone(&item_b)]));
    let view = ds.add_rc(create_view(800.0, 400.0, 12, items.to_observable()));
    view.run_with_scroll_anchor(ILogicalPosition { revision: view.layout.get().revision, offset: 100 }, &|tx| item_a.set_size(50, Some(tx)));
    let before_replacement = view.layout.get();
    let before_replacement_edit = view.last_geometry_edit.get();

    transaction(|tx| items.set(Rc::new(vec![TestCompressedScrollItem::new(50), TestCompressedScrollItem::new(500)]), Some(tx)));
    let after_replacement = view.layout.get();
    let after_replacement_edit = view.last_geometry_edit.get();

    assert_eq!(
        (
            (before_replacement.revision, before_replacement.leading_scroll_slack, before_replacement_edit.map(|edit| edit.anchor_kind)),
            (
                after_replacement.revision,
                after_replacement.leading_scroll_slack,
                after_replacement.trailing_scroll_slack,
                after_replacement.content_viewport.to_string(),
                after_replacement_edit.is_none(),
            ),
        ),
        ((as_layout_revision(1), 50, Some(AnchorKind::Logical)), (as_layout_revision(1), 0, 0, "[0, 400)".to_owned(), true))
    );
}

#[test]
fn converts_logical_reveal_positions_after_leading_slack() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let item_a = TestCompressedScrollItem::new(100);
    let item_b = TestCompressedScrollItem::new(500);
    let view = ds.add_rc(create_view(800.0, 400.0, 12, const_items(&[&item_a, &item_b])));
    view.run_with_scroll_anchor(ILogicalPosition { revision: view.layout.get().revision, offset: 100 }, &|tx| item_a.set_size(50, Some(tx)));

    view.set_logical_scroll_position(112, false);
    let layout = view.layout.get();

    assert_eq!((layout.scroll_top, layout.content_viewport.to_string(), layout.leading_scroll_slack), (112, "[112, 512)".to_owned(), 0));
}

#[test]
fn keeps_an_item_local_semantic_anchor_stable_when_its_offset_and_item_size_change() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let item = TestCompressedScrollItem::new(1000);
    let item_anchor_offset = Rc::new(Cell::new(300));
    let next_item_anchor_offset = Cell::new(500);
    let next_item_size = 1200;
    let view = ds.add_rc(create_view(800.0, 400.0, 0, const_items(&[&item])));
    view.set_scroll_position(scroll_to(250.0), false);
    let run_with_item_scroll_anchor = || {
        let context = item.render_context.borrow().clone().expect("o item foi renderizado");
        let anchor_offset = item_anchor_offset.clone();
        context.run_with_scroll_anchor(Rc::new({
            let item_anchor_offset = item_anchor_offset.clone();
            move || item_anchor_offset.get()
        }), &|tx| {
            anchor_offset.set(next_item_anchor_offset.get());
            item.set_size(next_item_size, Some(tx));
        });
    };

    let before_viewport_offset = item_anchor_offset.get() - view.layout.get().content_viewport.start;
    run_with_item_scroll_anchor();
    let after_layout = view.layout.get();
    let after_viewport_offset = item_anchor_offset.get() - after_layout.content_viewport.start;
    next_item_anchor_offset.set(550);
    run_with_item_scroll_anchor();
    let after_internal_edit_layout = view.layout.get();
    let after_internal_edit = view.last_geometry_edit.get();
    let before_no_op_layout = view.layout.get();
    let before_no_op_edit = view.last_geometry_edit.get();
    run_with_item_scroll_anchor();

    let same_edit = match (view.last_geometry_edit.get(), &before_no_op_edit) {
        (Some(current), Some(previous)) => Rc::ptr_eq(&current, previous),
        (None, None) => true,
        _ => false,
    };
    assert_eq!(
        (
            before_viewport_offset,
            after_layout.revision,
            after_layout.scroll_top,
            after_viewport_offset,
            view.last_geometry_edit.get().map(|edit| edit.anchor_kind),
            after_internal_edit_layout.revision,
            after_internal_edit_layout.scroll_top,
            item_anchor_offset.get() - after_internal_edit_layout.content_viewport.start,
            after_internal_edit.as_ref().map(|edit| (edit.from_revision, edit.to_revision)),
            after_internal_edit.as_ref().map(|edit| edit.edits.len()),
            Rc::ptr_eq(&view.layout.get(), &before_no_op_layout),
            same_edit,
        ),
        (
            50,
            as_layout_revision(1),
            450,
            50,
            Some(AnchorKind::Item),
            as_layout_revision(1),
            500,
            50,
            Some((as_layout_revision(1), as_layout_revision(1))),
            Some(0),
            true,
            true,
        )
    );
}

#[test]
fn anchors_geometry_edits_based_on_recent_scroll_direction() {
    run_with_real_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let item_a = TestCompressedScrollItem::new(500);
        let item_b = TestCompressedScrollItem::new(1000);
        let view = ds.add_rc(create_view(800.0, 400.0, 0, const_items(&[&item_a, &item_b])));
        view.set_scroll_position(scroll_to(600.0), false);
        let item_a_weak = Rc::downgrade(&item_a);
        *item_a.on_next_render.borrow_mut() = Some(Rc::new(move || {
            if let Some(item_a) = item_a_weak.upgrade() {
                item_a.set_size(40, None);
            }
        }));
        view.set_scroll_position(scroll_to(499.0), false);

        let after_shrink_while_scrolling_up = view.layout.get();
        let after_shrink_edit = view.last_geometry_edit.get();
        item_a.set_size(500, None);
        let after_restore_while_scrolling_up = view.layout.get();
        let after_restore_edit = view.last_geometry_edit.get();
        timeout(60).await.expect("timeout");
        view.set_scroll_position(scroll_to(498.0), false);
        timeout(60).await.expect("timeout");
        item_a.set_size(600, None);
        let after_growth_within_retention_period = view.layout.get();
        let after_growth_within_retention_period_edit = view.last_geometry_edit.get();
        timeout(50).await.expect("timeout");
        item_b.set_size(900, None);
        let after_change_after_scrolling_ended = view.layout.get();
        let after_change_after_scrolling_ended_edit = view.last_geometry_edit.get();

        let bottom_offset = |layout: &je_editor::compressed_virtualized_scroll_layout::ICompressedVirtualizedScrollLayout| layout.content_viewport.end_exclusive - layout.items[1].content_range.start;
        assert_eq!(
            (
                (after_shrink_while_scrolling_up.scroll_top, bottom_offset(&after_shrink_while_scrolling_up), after_shrink_edit.map(|edit| edit.anchor_kind)),
                (after_restore_while_scrolling_up.scroll_top, bottom_offset(&after_restore_while_scrolling_up), after_restore_edit.map(|edit| edit.anchor_kind)),
                (
                    after_growth_within_retention_period.scroll_top,
                    bottom_offset(&after_growth_within_retention_period),
                    after_growth_within_retention_period_edit.map(|edit| edit.anchor_kind),
                ),
                (
                    after_change_after_scrolling_ended.scroll_top,
                    after_change_after_scrolling_ended.content_viewport.start - after_change_after_scrolling_ended.items[1].content_range.start,
                    after_change_after_scrolling_ended_edit.map(|edit| edit.anchor_kind),
                ),
            ),
            (
                (39, 399, Some(AnchorKind::ViewportBottom)),
                (499, 399, Some(AnchorKind::ViewportBottom)),
                (598, 398, Some(AnchorKind::ViewportBottom)),
                (598, -2, Some(AnchorKind::ViewportTop)),
            )
        );
    });
}

struct TestCompressedScrollItem {
    size: Rc<ObservableValue<i32, ()>>,
    max_scroll: IObservable<MaxScroll>,
    render_context: RefCell<Option<Rc<dyn ICompressedVirtualizedScrollItemContext>>>,
    on_next_render: RefCell<Option<Rc<dyn Fn()>>>,
}

impl TestCompressedScrollItem {
    fn new(content_height: i32) -> Rc<Self> {
        Rc::new(Self {
            size: observable_value::<i32, ()>("size", content_height),
            max_scroll: const_observable(MaxScroll { max_scroll: 0.0 }),
            render_context: RefCell::new(None),
            on_next_render: RefCell::new(None),
        })
    }

    fn set_size(&self, size: i32, tx: Option<&dyn ITransaction>) {
        match tx {
            Some(tx) => self.size.set(size, Some(tx)),
            None => transaction(|tx| self.size.set(size, Some(tx))),
        }
    }
}

impl ICompressedVirtualizedScrollItem for TestCompressedScrollItem {
    fn size(&self) -> IObservable<i32> {
        self.size.to_observable()
    }

    fn max_scroll(&self) -> IObservable<MaxScroll> {
        self.max_scroll.clone()
    }

    fn render(&self, _rendered_range: OffsetRange, _scroll_offset: i32, _width: f64, _rendered_viewport: OffsetRange, context: Rc<dyn ICompressedVirtualizedScrollItemContext>) {
        *self.render_context.borrow_mut() = Some(context);
        let on_next_render = self.on_next_render.borrow_mut().take();
        if let Some(on_next_render) = on_next_render {
            on_next_render();
        }
    }

    fn hide(&self) {}
}
