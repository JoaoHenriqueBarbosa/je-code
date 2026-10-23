//! Testes do modelo de eventos do `je-ui` (`src/events.rs`) e da caixa de `src/tree.rs`.
//!
//! Não há original no VS Code para este arquivo: o `je-ui` substitui o DOM, e o que ele promete é o que os
//! widgets do VS Code esperam do DOM (ordem captura, alvo, bolha; `mouseenter` que não borbulha; `click` no
//! ancestral comum; captura do ponteiro). Cada caso diz qual comportamento do DOM ele fixa.

use std::cell::RefCell;
use std::rc::Rc;

use je_render::kurbo::Rect;
use je_ui::{Cursor, Event, EventKind, Modifiers, NodeId, Phase, PointerButton, PointerEvents, PointerInput, PointerInputKind, Role, Ui};

type Log = Rc<RefCell<Vec<String>>>;

fn node(ui: &Ui, parent: NodeId, bounds: Rect) -> NodeId {
    let mut tree = ui.tree_mut();
    let id = tree.create(parent, Role::Generic);
    tree.set_bounds(id, bounds);
    id
}

/// Uma árvore raiz(100x100) > a(0,0,80,80) > b(10,10,70,70) > c(20,20,60,60), todas nas coordenadas do pai.
fn nested() -> (Ui, NodeId, NodeId, NodeId, NodeId) {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 100.0, 100.0));
    let a = node(&ui, root, Rect::new(0.0, 0.0, 80.0, 80.0));
    let b = node(&ui, a, Rect::new(10.0, 10.0, 70.0, 70.0));
    let c = node(&ui, b, Rect::new(10.0, 10.0, 50.0, 50.0));
    (ui, root, a, b, c)
}

fn record(ui: &Ui, log: &Log, node: NodeId, name: &str, kind: EventKind, capture: bool) {
    let (log, name) = (log.clone(), name.to_owned());
    std::mem::forget(ui.listen(node, kind, capture, move |e: &mut Event| log.borrow_mut().push(format!("{name}:{:?}", e.phase))));
}

fn input(kind: PointerInputKind, x: f64, y: f64) -> PointerInput {
    PointerInput::new(kind, x, y)
}

#[test]
fn dispatch_runs_capture_then_target_then_bubble() {
    let (ui, root, a, _b, c) = nested();
    let log: Log = Rc::default();
    for (n, name) in [(root, "root"), (a, "a"), (c, "c")] {
        record(&ui, &log, n, &format!("{name}-bubble"), EventKind::Click, false);
        record(&ui, &log, n, &format!("{name}-capture"), EventKind::Click, true);
    }
    ui.dispatch(&mut Event::new(EventKind::Click, c));
    assert_eq!(
        *log.borrow(),
        ["root-capture:Capture", "a-capture:Capture", "c-bubble:Target", "c-capture:Target", "a-bubble:Bubble", "root-bubble:Bubble"]
    );
}

#[test]
fn stop_propagation_in_capture_skips_target_and_bubble() {
    let (ui, root, a, _b, c) = nested();
    let log: Log = Rc::default();
    std::mem::forget(ui.listen(a, EventKind::Click, true, |e| e.stop_propagation()));
    record(&ui, &log, c, "c", EventKind::Click, false);
    record(&ui, &log, root, "root", EventKind::Click, false);
    ui.dispatch(&mut Event::new(EventKind::Click, c));
    assert!(log.borrow().is_empty());
}

#[test]
fn stop_propagation_in_bubble_stops_at_that_node_but_runs_its_other_listeners() {
    let (ui, root, a, _b, c) = nested();
    let log: Log = Rc::default();
    std::mem::forget(ui.listen(a, EventKind::Click, false, |e| e.stop_propagation()));
    record(&ui, &log, a, "a2", EventKind::Click, false);
    record(&ui, &log, root, "root", EventKind::Click, false);
    ui.dispatch(&mut Event::new(EventKind::Click, c));
    assert_eq!(*log.borrow(), ["a2:Bubble"]);
}

#[test]
fn event_helper_stop_prevents_default_and_optionally_cancels_bubble() {
    let (ui, root, _a, _b, c) = nested();
    let log: Log = Rc::default();
    std::mem::forget(ui.listen(c, EventKind::Click, false, |e| e.stop(false)));
    record(&ui, &log, root, "root", EventKind::Click, false);
    let mut event = Event::new(EventKind::Click, c);
    assert!(!ui.dispatch(&mut event), "dispatchEvent devolve falso com preventDefault");
    assert!(event.default_prevented());
    assert_eq!(log.borrow().len(), 1, "stop(false) não cancela a bolha");
}

#[test]
fn a_listener_removed_during_dispatch_is_not_called() {
    let (ui, _root, a, _b, c) = nested();
    let log: Log = Rc::default();
    let second = Rc::new(RefCell::new(None));
    let slot = second.clone();
    std::mem::forget(ui.listen(c, EventKind::Click, false, move |_| {
        if let Some(d) = slot.borrow_mut().take() {
            let d: je_base::lifecycle::DisposableRef = d;
            d.dispose();
        }
    }));
    let _ = a;
    let (l, ) = (log.clone(),);
    *second.borrow_mut() = Some(ui.listen(c, EventKind::Click, false, move |_| l.borrow_mut().push("second".into())));
    ui.dispatch(&mut Event::new(EventKind::Click, c));
    assert!(log.borrow().is_empty());
}

#[test]
fn removing_a_node_forgets_its_listeners_and_focus() {
    let (ui, _root, a, b, c) = nested();
    let log: Log = Rc::default();
    record(&ui, &log, c, "c", EventKind::Click, false);
    ui.focus(c);
    ui.remove(b);
    assert_eq!(ui.focused(), None);
    assert!(!ui.tree().is_alive(c));
    assert!(ui.tree().children(a).is_empty());
}

#[test]
fn pointer_down_targets_the_deepest_node_and_up_synthesizes_click_on_the_common_ancestor() {
    let (ui, root, a, b, c) = nested();
    let log: Log = Rc::default();
    for (n, name) in [(root, "root"), (a, "a"), (b, "b"), (c, "c")] {
        record(&ui, &log, n, name, EventKind::Click, false);
    }
    // c ocupa 40..80 na janela (10+10+... : a(0)+b(10)+c(10) = 20, até 60).
    assert_eq!(ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0)), c);
    assert_eq!(ui.pointer_input(input(PointerInputKind::Up, 30.0, 30.0)), c);
    assert_eq!(log.borrow().first().map(String::as_str), Some("c:Target"));

    log.borrow_mut().clear();
    // Botão baixo em c e alto em b (fora de c): o click vai ao ancestral comum, b.
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Up, 15.0, 15.0));
    assert_eq!(log.borrow().first().map(String::as_str), Some("b:Target"));
}

#[test]
fn double_click_comes_with_click_count_two_on_pointer_up() {
    let (ui, _root, _a, _b, c) = nested();
    let log: Log = Rc::default();
    record(&ui, &log, c, "dbl", EventKind::DoubleClick, false);
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    let mut up = input(PointerInputKind::Up, 30.0, 30.0);
    up.click_count = 2;
    ui.pointer_input(up);
    assert_eq!(log.borrow().len(), 1);
}

#[test]
fn secondary_button_does_not_click() {
    let (ui, _root, _a, _b, c) = nested();
    let log: Log = Rc::default();
    record(&ui, &log, c, "click", EventKind::Click, false);
    let mut down = input(PointerInputKind::Down, 30.0, 30.0);
    down.button = PointerButton::Secondary;
    ui.pointer_input(down);
    let mut up = input(PointerInputKind::Up, 30.0, 30.0);
    up.button = PointerButton::Secondary;
    ui.pointer_input(up);
    assert!(log.borrow().is_empty());
}

#[test]
fn enter_and_leave_do_not_bubble_and_follow_the_path_difference() {
    let (ui, root, a, b, c) = nested();
    let log: Log = Rc::default();
    for (n, name) in [(root, "root"), (a, "a"), (b, "b"), (c, "c")] {
        let (l, name) = (log.clone(), name.to_owned());
        std::mem::forget(ui.listen(n, EventKind::PointerEnter, false, {
            let (l, name) = (l.clone(), name.clone());
            move |_| l.borrow_mut().push(format!("enter {name}"))
        }));
        std::mem::forget(ui.listen(n, EventKind::PointerLeave, false, move |_| l.borrow_mut().push(format!("leave {name}"))));
    }
    ui.pointer_input(input(PointerInputKind::Move, 30.0, 30.0));
    assert_eq!(*log.borrow(), ["enter root", "enter a", "enter b", "enter c"]);
    log.borrow_mut().clear();
    ui.pointer_input(input(PointerInputKind::Move, 15.0, 15.0));
    assert_eq!(*log.borrow(), ["leave c"], "só o que saiu do caminho");
    log.borrow_mut().clear();
    ui.pointer_input(input(PointerInputKind::Move, 95.0, 95.0));
    assert_eq!(*log.borrow(), ["leave b", "leave a"]);
    log.borrow_mut().clear();
    ui.pointer_left_window();
    assert_eq!(*log.borrow(), ["leave root"]);
}

#[test]
fn pointer_capture_redirects_move_and_up_until_the_button_goes_up() {
    let (ui, root, a, _b, c) = nested();
    let log: Log = Rc::default();
    record(&ui, &log, a, "a-move", EventKind::PointerMove, false);
    record(&ui, &log, c, "c-move", EventKind::PointerMove, false);
    record(&ui, &log, a, "a-up", EventKind::PointerUp, false);
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.set_pointer_capture(a);
    // Fora de tudo (na raiz), mas capturado por `a`.
    assert_eq!(ui.pointer_input(input(PointerInputKind::Move, 95.0, 95.0)), a);
    assert_eq!(*log.borrow(), ["a-move:Target"]);
    ui.pointer_input(input(PointerInputKind::Up, 95.0, 95.0));
    assert_eq!(ui.pointer_captured_by(), None);
    assert_eq!(ui.pointer_input(input(PointerInputKind::Move, 30.0, 30.0)), c);
    let _ = root;
}

#[test]
fn window_level_listener_sees_every_move_like_the_sash_does() {
    let (ui, root, _a, _b, _c) = nested();
    let log: Log = Rc::default();
    record(&ui, &log, root, "window", EventKind::PointerMove, false);
    ui.pointer_input(input(PointerInputKind::Move, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 95.0, 95.0));
    assert_eq!(log.borrow().len(), 2);
}

#[test]
fn keys_go_to_the_focused_node_and_focus_blur_do_not_bubble() {
    let (ui, root, a, _b, c) = nested();
    let log: Log = Rc::default();
    record(&ui, &log, c, "key", EventKind::KeyDown, false);
    record(&ui, &log, root, "root-focus", EventKind::Focus, false);
    record(&ui, &log, a, "a-focus-capture", EventKind::Focus, true);
    record(&ui, &log, c, "c-blur", EventKind::Blur, false);
    ui.key_input(true, je_ui::KeyData { key_code: je_base::key_codes::KeyCode::Enter, modifiers: Modifiers::default() });
    assert!(log.borrow().is_empty(), "sem foco a tecla vai à raiz");
    ui.focus(c);
    assert_eq!(*log.borrow(), ["a-focus-capture:Capture"], "focus não borbulha, mas tem fase de captura");
    ui.key_input(true, je_ui::KeyData { key_code: je_base::key_codes::KeyCode::Enter, modifiers: Modifiers::default() });
    assert_eq!(log.borrow().last().map(String::as_str), Some("key:Target"));
    ui.blur();
    assert_eq!(log.borrow().last().map(String::as_str), Some("c-blur:Target"));
}

#[test]
fn pointer_events_none_is_inherited_and_overridden_by_auto() {
    let (ui, _root, a, b, c) = nested();
    ui.tree_mut().style_mut(a).pointer_events = PointerEvents::None;
    assert_eq!(ui.tree().hit_test(30.0, 30.0), None);
    ui.tree_mut().style_mut(c).pointer_events = PointerEvents::Auto;
    assert_eq!(ui.tree().hit_test(30.0, 30.0), Some(c), "initial no filho volta a receber o ponteiro");
    assert_eq!(ui.tree().hit_test(15.0, 15.0), None, "b herda o none de a");
    let _ = b;
}

#[test]
fn z_index_orders_siblings_for_paint_and_hit_test() {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 100.0, 100.0));
    let first = node(&ui, root, Rect::new(0.0, 0.0, 50.0, 50.0));
    let second = node(&ui, root, Rect::new(0.0, 0.0, 50.0, 50.0));
    assert_eq!(ui.tree().hit_test(10.0, 10.0), Some(second));
    ui.tree_mut().style_mut(first).z_index = 35;
    assert_eq!(ui.tree().hit_test(10.0, 10.0), Some(first));
}

#[test]
fn box_resolves_fill_right_and_bottom_against_the_parent() {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 200.0, 100.0));
    let full = node(&ui, root, Rect::new(5.0, 0.0, 5.0, 10.0));
    ui.tree_mut().set_fill(full, true, false);
    assert_eq!(ui.tree().bounds(full), Rect::new(5.0, 0.0, 205.0, 10.0), "width: 100% com left: 5 passa do pai, como no CSS");
    let anchored = node(&ui, root, Rect::new(0.0, 0.0, 8.0, 8.0));
    ui.tree_mut().set_right(anchored, Some(-4.0));
    ui.tree_mut().set_bottom(anchored, Some(2.0));
    assert_eq!(ui.tree().bounds(anchored), Rect::new(196.0, 90.0, 204.0, 98.0));
}

#[test]
fn nodes_can_be_detached_and_reattached_keeping_the_subtree() {
    let (ui, root, a, b, c) = nested();
    ui.tree_mut().detach(b);
    assert!(ui.tree().children(a).is_empty());
    assert_eq!(ui.tree().hit_test(30.0, 30.0), Some(a), "sem b, o ponto cai em a");
    ui.tree_mut().append_child(root, b);
    assert!(ui.tree().is_alive(c) && ui.tree().is_ancestor(b, c));
    assert_eq!(ui.tree().parent(b), Some(root));
}

#[test]
fn cursor_override_wins_over_the_node_cursor_and_the_last_wins() {
    let (ui, _root, _a, b, c) = nested();
    ui.tree_mut().style_mut(b).cursor = Some(Cursor::Pointer);
    assert_eq!(ui.cursor_at(30.0, 30.0), Cursor::Pointer, "o cursor do nó é herdado pelos filhos");
    let first = ui.add_cursor_override(Cursor::EwResize);
    let second = ui.add_cursor_override(Cursor::AllScroll);
    assert_eq!(ui.cursor_at(30.0, 30.0), Cursor::AllScroll);
    ui.remove_cursor_override(second);
    assert_eq!(ui.cursor_at(30.0, 30.0), Cursor::EwResize);
    ui.remove_cursor_override(first);
    assert_eq!(ui.cursor_at(30.0, 30.0), Cursor::Pointer);
    let _ = (c, Phase::Target);
}

#[test]
fn default_action_runs_after_the_listeners_unless_prevented() {
    let (ui, _root, a, _b, c) = nested();
    let log: Log = Rc::default();
    record(&ui, &log, a, "a-bubble", EventKind::KeyDown, false);
    {
        let log = log.clone();
        std::mem::forget(ui.listen_default(a, EventKind::KeyDown, move |_| log.borrow_mut().push("a-default".to_owned())));
    }
    ui.dispatch(&mut Event::new(EventKind::KeyDown, c));
    assert_eq!(*log.borrow(), ["a-bubble:Bubble", "a-default"]);

    // `preventDefault` num ouvinte cancela a ação; `stopPropagation` não.
    log.borrow_mut().clear();
    std::mem::forget(ui.listen(c, EventKind::KeyDown, false, |e: &mut Event| e.prevent_default()));
    ui.dispatch(&mut Event::new(EventKind::KeyDown, c));
    assert_eq!(*log.borrow(), ["a-bubble:Bubble"]);
}

#[test]
fn only_the_closest_default_action_runs() {
    let (ui, root, a, _b, c) = nested();
    let log: Log = Rc::default();
    for (node, name) in [(root, "root"), (a, "a")] {
        let log = log.clone();
        std::mem::forget(ui.listen_default(node, EventKind::Click, move |_| log.borrow_mut().push(name.to_owned())));
    }
    ui.dispatch(&mut Event::new(EventKind::Click, c));
    assert_eq!(*log.borrow(), ["a"]);
}
