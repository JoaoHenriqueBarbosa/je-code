//! Testes do `Scrollable` e do `ScrollState`, que o `scrollable.test.ts` do VS Code não cobre (ele só testa a
//! `SmoothScrollingOperation`). Validam a validação de posição, os eventos e o ciclo da animação, com um
//! agendador de quadros manual e o relógio virtual.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::lifecycle::{IDisposable, plain_disposable};
use je_base::runtime;
use je_base::scrollable::{INewScrollDimensions, INewScrollPosition, IScrollableOptions, ScrollEvent, ScrollState, Scrollable};

type Frames = Rc<RefCell<Vec<Option<Box<dyn FnOnce()>>>>>;

/// Um `scheduleAtNextAnimationFrame` manual: `run_frame` roda os callbacks pendentes que não foram cancelados.
struct Harness {
    scrollable: Rc<Scrollable>,
    frames: Frames,
    events: Rc<RefCell<Vec<ScrollEvent>>>,
}

/// O `Scrollable` e o listener ficam registrados no `ds`, que os descarta ao fim do teste.
fn harness(ds: &LeakGuard, smooth_scroll_duration: f64, force_integer_values: bool) -> Harness {
    let frames: Frames = Rc::default();
    let queue = frames.clone();
    let scrollable = Scrollable::new(IScrollableOptions {
        force_integer_values,
        smooth_scroll_duration,
        schedule_at_next_animation_frame: Rc::new(move |callback| {
            let index = queue.borrow().len();
            queue.borrow_mut().push(Some(callback));
            let queue = queue.clone();
            plain_disposable(move || queue.borrow_mut()[index] = None)
        }),
    });
    let events: Rc<RefCell<Vec<ScrollEvent>>> = Rc::default();
    let sink = events.clone();
    ds.add(scrollable.on_scroll().listen(move |event| sink.borrow_mut().push(*event)));
    ds.add_rc(scrollable.clone());
    Harness { scrollable, frames, events }
}

impl Harness {
    fn run_frame(&self) {
        let pending: Vec<_> = self.frames.borrow_mut().iter_mut().filter_map(Option::take).collect();
        for callback in pending {
            callback();
        }
    }

    fn dimensions(&self, width: f64, scroll_width: f64, height: f64, scroll_height: f64) {
        self.scrollable.set_scroll_dimensions(
            &INewScrollDimensions { width: Some(width), scroll_width: Some(scroll_width), height: Some(height), scroll_height: Some(scroll_height) },
            false,
        );
    }
}

fn top(scroll_top: f64) -> INewScrollPosition {
    INewScrollPosition { scroll_left: None, scroll_top: Some(scroll_top) }
}

#[test]
fn scroll_state_clamps_the_position_and_keeps_the_raw_one() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let state = ScrollState::new(false, 100.0, 300.0, 500.0, 50.0, 80.0, -10.0);

    assert_eq!((state.scroll_left, state.raw_scroll_left), (200.0, 500.0));
    assert_eq!((state.scroll_top, state.raw_scroll_top), (0.0, -10.0));
    assert_eq!(ScrollState::new(false, -5.0, 0.0, 0.0, -5.0, 0.0, 0.0).width, 0.0);
}

#[test]
fn scroll_state_force_integer_values_truncates_like_or_zero() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let state = ScrollState::new(true, 10.9, 100.7, 3.9, 10.2, f64::NAN, -2.5);

    assert_eq!((state.width, state.scroll_width, state.scroll_left, state.height, state.scroll_height, state.raw_scroll_top), (10.0, 100.0, 3.0, 10.0, 0.0, -2.0));
    assert_eq!(ScrollState::new(true, 4_294_967_297.0, 5_000_000_000.0, 0.0, 0.0, 0.0, 0.0).width, 1.0);
}

#[test]
fn set_scroll_dimensions_fires_one_event_with_the_changes() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let h = harness(&ds, 0.0, false);

    h.dimensions(100.0, 100.0, 50.0, 400.0);
    h.dimensions(100.0, 100.0, 50.0, 400.0);

    let events = h.events.borrow();
    assert_eq!(events.len(), 1);
    assert!(events[0].height_changed && events[0].scroll_height_changed && events[0].width_changed);
    assert!(!events[0].scroll_top_changed && !events[0].in_smooth_scrolling);
}

#[test]
fn set_scroll_position_now_validates_and_cancels_smooth_scrolling() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let h = harness(&ds, 100.0, false);
    h.dimensions(100.0, 100.0, 50.0, 400.0);

    h.scrollable.set_scroll_position_smooth(&top(300.0), false);
    assert!(h.scrollable.has_pending_scroll_animation());
    assert_eq!(h.scrollable.get_future_scroll_position().scroll_top, 300.0);
    assert_eq!(h.scrollable.get_current_scroll_position().scroll_top, 0.0);

    h.scrollable.set_scroll_position_now(&top(1000.0));
    assert!(!h.scrollable.has_pending_scroll_animation());
    assert_eq!(h.scrollable.get_current_scroll_position().scroll_top, 350.0);
    assert_eq!(h.scrollable.validate_scroll_position(&top(-4.0)).scroll_top, 0.0);

    // o quadro agendado foi cancelado: rodar o quadro não muda nada
    let before = h.events.borrow().len();
    h.run_frame();
    assert_eq!(h.events.borrow().len(), before);
}

#[test]
fn smooth_scrolling_advances_frame_by_frame_until_done() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let h = harness(&ds, 100.0, false);
        h.dimensions(100.0, 100.0, 50.0, 1000.0);
        h.events.borrow_mut().clear();

        h.scrollable.set_scroll_position_smooth(&top(500.0), false);
        h.scrollable.set_scroll_position_smooth(&top(500.0), false);
        assert_eq!(h.frames.borrow().len(), 1, "o mesmo destino não reinicia a animação");

        runtime::advance_fake_time(50);
        h.run_frame();
        let middle = h.scrollable.get_current_scroll_position().scroll_top;
        assert!(middle > 0.0 && middle < 500.0, "{middle}");
        assert!(h.events.borrow().last().unwrap().in_smooth_scrolling);
        assert!(h.scrollable.has_pending_scroll_animation());

        runtime::advance_fake_time(100);
        h.run_frame();
        assert_eq!(h.scrollable.get_current_scroll_position().scroll_top, 500.0);
        assert!(!h.scrollable.has_pending_scroll_animation());
    });
}

#[test]
fn smooth_scrolling_retargets_when_the_dimensions_shrink() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let h = harness(&ds, 100.0, false);
    h.dimensions(100.0, 100.0, 50.0, 1000.0);

    h.scrollable.set_scroll_position_smooth(&top(900.0), false);
    h.dimensions(100.0, 100.0, 50.0, 300.0);

    assert_eq!(h.scrollable.get_future_scroll_position().scroll_top, 250.0);
}

#[test]
fn smooth_scrolling_without_duration_scrolls_now() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let h = harness(&ds, 0.0, false);
    h.dimensions(100.0, 100.0, 50.0, 400.0);

    h.scrollable.set_scroll_position_smooth(&top(120.0), false);

    assert!(!h.scrollable.has_pending_scroll_animation());
    assert_eq!(h.scrollable.get_current_scroll_position().scroll_top, 120.0);
    assert!(h.frames.borrow().is_empty());
}

#[test]
fn dispose_cancels_the_pending_frame() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let h = harness(&ds, 100.0, false);
    h.dimensions(100.0, 100.0, 50.0, 400.0);
    h.scrollable.set_scroll_position_smooth(&top(200.0), false);

    h.scrollable.dispose();
    let before = h.events.borrow().len();
    h.run_frame();

    assert_eq!(h.events.borrow().len(), before);
}
