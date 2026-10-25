//! Testes do modelo de eventos do `je-ui` (`src/events.rs`) e da caixa de `src/tree.rs`.
//!
//! Não há original no VS Code para este arquivo: o `je-ui` substitui o DOM, e o que ele promete é o que os
//! widgets do VS Code esperam do DOM (ordem captura, alvo, bolha; `mouseenter` que não borbulha; `click` no
//! ancestral comum; captura do ponteiro). Cada caso diz qual comportamento do DOM ele fixa.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_render::kurbo::Rect;
use je_ui::{Cursor, DataTransfer, EffectAllowed, Event, EventKind, KeyData, Modifiers, NodeId, Phase, PointerButton, PointerEvents, PointerInput, PointerInputKind, Role, Ui};

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

// ---- Arrastar e soltar (o modelo de processamento do HTML) ----

const DRAG_KINDS: [EventKind; 7] = [EventKind::DragStart, EventKind::Drag, EventKind::DragEnter, EventKind::DragOver, EventKind::DragLeave, EventKind::Drop, EventKind::DragEnd];

fn set_draggable(ui: &Ui, node: NodeId) {
    ui.tree_mut().set_attribute(node, "draggable", "true");
}

/// Registra na raiz (borbulha) os sete eventos de arrastar e soltar como `Tipo@rótulo` (o `Drop` leva o texto lido
/// e o `DragEnd` o `dropEffect` final). O `DragStart` escreve `text/plain` e `effectAllowed = copyMove`; o `DragOver`
/// aceita se `over_accepts`, o `Drop` se `drop_accepts`. No `DragOver` confere que o repositório está protegido.
fn drag_log(ui: &Ui, labels: &[(NodeId, &'static str)], over_accepts: bool, drop_accepts: bool) -> Log {
    let log: Log = Rc::default();
    let labels: Vec<(NodeId, &'static str)> = labels.to_vec();
    for kind in DRAG_KINDS {
        let (log, labels) = (log.clone(), labels.clone());
        std::mem::forget(ui.listen(ui.root(), kind, false, move |e: &mut Event| {
            let label = labels.iter().find(|(n, _)| *n == e.target).map_or("?", |(_, l)| *l);
            let dt = e.data_transfer.clone().expect("evento de arrastar sem dataTransfer");
            let entry = match kind {
                EventKind::DragStart => {
                    dt.set_data("text/plain", "hello");
                    dt.set_effect_allowed(EffectAllowed::CopyMove);
                    format!("DragStart@{label}")
                }
                EventKind::DragOver => {
                    assert_eq!(dt.get_data("text/plain"), "", "o dragover não pode ler a carga");
                    assert!(!dt.types().is_empty(), "os tipos aparecem em todos os eventos");
                    if over_accepts {
                        e.prevent_default();
                    }
                    format!("DragOver@{label}")
                }
                EventKind::Drop => {
                    if drop_accepts {
                        e.prevent_default();
                    }
                    format!("Drop@{label}:{}", dt.get_data("text/plain"))
                }
                EventKind::DragEnd => format!("DragEnd@{label}:{}", dt.drop_effect().as_str()),
                _ => format!("{kind:?}@{label}"),
            };
            log.borrow_mut().push(entry);
        }));
    }
    log
}

fn pointer_log(ui: &Ui, kinds: &[EventKind]) -> Log {
    let log: Log = Rc::default();
    for kind in kinds {
        let (log, kind) = (log.clone(), *kind);
        std::mem::forget(ui.listen(ui.root(), kind, false, move |_| log.borrow_mut().push(format!("{kind:?}"))));
    }
    log
}

#[test]
fn drag_starts_only_after_the_threshold_is_crossed() {
    let (ui, _root, _a, b, c) = nested();
    set_draggable(&ui, b);
    let drags = drag_log(&ui, &[(b, "b"), (c, "c")], true, true);
    let pointer = pointer_log(&ui, &[EventKind::PointerMove]);
    // O botão baixo sobre `c` (descendente de `b`, que é o `draggable` mais próximo).
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    assert!((ui.drag_threshold() - je_ui::DEFAULT_DRAG_THRESHOLD).abs() < f64::EPSILON);
    // Dois pixels: abaixo do limiar, é um movimento comum.
    ui.pointer_input(input(PointerInputKind::Move, 32.0, 30.0));
    assert!(drags.borrow().is_empty());
    assert_eq!(*pointer.borrow(), ["PointerMove"]);
    assert!(!ui.is_dragging());
    // Dez pixels: o arrasto começa na origem `b`, sem `PointerMove`.
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 30.0));
    assert!(ui.is_dragging());
    assert_eq!(drags.borrow()[0], "DragStart@b");
    assert_eq!(*pointer.borrow(), ["PointerMove"]);
    // Durante o arrasto o movimento é `Drag` na origem e `DragOver` no alvo, não `PointerMove`.
    ui.pointer_input(input(PointerInputKind::Move, 41.0, 30.0));
    assert_eq!(*pointer.borrow(), ["PointerMove"]);
    assert_eq!(drags.borrow().last().unwrap(), "DragOver@c");
}

#[test]
fn drag_threshold_is_a_parameter_of_the_ui() {
    let (ui, _root, _a, b, _c) = nested();
    set_draggable(&ui, b);
    ui.set_drag_threshold(20.0);
    let drags = drag_log(&ui, &[(b, "b")], true, true);
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 45.0, 30.0));
    assert!(drags.borrow().is_empty());
    ui.pointer_input(input(PointerInputKind::Move, 55.0, 30.0));
    assert_eq!(drags.borrow()[0], "DragStart@b");
}

#[test]
fn prevent_default_on_drag_start_cancels_the_drag() {
    let (ui, _root, _a, b, _c) = nested();
    set_draggable(&ui, b);
    std::mem::forget(ui.listen(b, EventKind::DragStart, false, |e| e.prevent_default()));
    let drags = drag_log(&ui, &[(b, "b")], true, true);
    let pointer = pointer_log(&ui, &[EventKind::PointerMove, EventKind::PointerUp, EventKind::Click]);
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 30.0));
    assert!(!ui.is_dragging());
    ui.pointer_input(input(PointerInputKind::Move, 45.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Up, 45.0, 30.0));
    // O único evento de arrasto é o `DragStart` cancelado; o ponteiro seguiu normal (movimento, soltar, clique).
    assert_eq!(*drags.borrow(), ["DragStart@b"]);
    assert_eq!(*pointer.borrow(), ["PointerMove", "PointerMove", "PointerUp", "Click"]);
}

#[test]
fn a_node_without_draggable_true_never_starts_a_drag() {
    let (ui, _root, _a, b, c) = nested();
    set_draggable(&ui, b);
    // `draggable="false"` no mais próximo vale mais que o antepassado.
    ui.tree_mut().set_attribute(c, "draggable", "false");
    let drags = drag_log(&ui, &[(b, "b"), (c, "c")], true, true);
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 60.0, 30.0));
    assert!(drags.borrow().is_empty());
    // Um botão que não é o principal também não arma o arrasto.
    let mut secondary = input(PointerInputKind::Down, 15.0, 15.0);
    secondary.button = PointerButton::Secondary;
    ui.pointer_input(input(PointerInputKind::Up, 30.0, 30.0));
    ui.pointer_input(secondary);
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 40.0));
    assert!(drags.borrow().is_empty());
}

#[test]
fn drag_runs_start_enter_over_leave_drop_end_in_order() {
    let (ui, _root, _a, b, c) = nested();
    set_draggable(&ui, b);
    let drags = drag_log(&ui, &[(b, "b"), (c, "c")], true, true);
    let pointer = pointer_log(&ui, &[EventKind::PointerUp, EventKind::Click]);
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 30.0));
    // Para `b` (fora de `c`): `DragEnter` no novo alvo vem antes do `DragLeave` no antigo.
    ui.pointer_input(input(PointerInputKind::Move, 15.0, 15.0));
    ui.pointer_input(input(PointerInputKind::Up, 15.0, 15.0));
    assert_eq!(
        *drags.borrow(),
        [
            "DragStart@b",
            "Drag@b",
            "DragEnter@c",
            "DragOver@c",
            "Drag@b",
            "DragEnter@b",
            "DragLeave@c",
            "DragOver@b",
            "Drag@b",
            "DragOver@b",
            // O repositório lê no `drop`; o `drop` cancelado mantém o efeito aceito (`copy`, de `copyMove`).
            "Drop@b:hello",
            "DragEnd@b:copy",
        ]
    );
    // Sem `PointerUp` nem `Click` ao soltar um arrasto.
    assert!(pointer.borrow().is_empty());
    assert!(!ui.is_dragging());
}

#[test]
fn drop_is_refused_when_dragover_is_not_prevented() {
    let (ui, _root, _a, b, c) = nested();
    set_draggable(&ui, b);
    let drags = drag_log(&ui, &[(b, "b"), (c, "c")], false, true);
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Up, 40.0, 30.0));
    let log = drags.borrow();
    assert!(!log.iter().any(|e| e.starts_with("Drop")), "sem preventDefault no dragover não há drop: {log:?}");
    assert_eq!(log[log.len() - 2], "DragLeave@c");
    assert_eq!(log[log.len() - 1], "DragEnd@b:none");
}

#[test]
fn drop_not_prevented_ends_with_drop_effect_none() {
    let (ui, _root, _a, b, c) = nested();
    set_draggable(&ui, b);
    let drags = drag_log(&ui, &[(b, "b"), (c, "c")], true, false);
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Up, 40.0, 30.0));
    let log = drags.borrow();
    assert_eq!(log[log.len() - 2], "Drop@c:hello");
    assert_eq!(log[log.len() - 1], "DragEnd@b:none");
}

#[test]
fn drop_effect_must_be_allowed_by_the_source() {
    let (ui, _root, _a, b, c) = nested();
    set_draggable(&ui, b);
    // A origem permite só `move`; o alvo pede `link`: a soltura é recusada.
    std::mem::forget(ui.listen(b, EventKind::DragStart, false, |e| e.data_transfer.as_ref().unwrap().set_effect_allowed(EffectAllowed::Move)));
    std::mem::forget(ui.listen(c, EventKind::DragOver, false, |e| {
        let dt = e.data_transfer.as_ref().unwrap();
        assert_eq!(dt.drop_effect().as_str(), "move", "o dropEffect inicial vem de effectAllowed");
        dt.set_drop_effect(je_ui::DropEffect::Link);
        e.prevent_default();
    }));
    let seen: Log = Rc::default();
    let seen_clone = seen.clone();
    std::mem::forget(ui.listen(b, EventKind::DragEnd, false, move |e| seen_clone.borrow_mut().push(e.data_transfer.as_ref().unwrap().drop_effect().as_str().to_owned())));
    let pointer = pointer_log(&ui, &[EventKind::Drop]);
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Up, 40.0, 30.0));
    assert!(pointer.borrow().is_empty());
    assert_eq!(*seen.borrow(), ["none"]);
}

#[test]
fn escape_cancels_the_drag_with_leave_and_end_none() {
    let (ui, _root, _a, b, c) = nested();
    set_draggable(&ui, b);
    let drags = drag_log(&ui, &[(b, "b"), (c, "c")], true, true);
    let pointer = pointer_log(&ui, &[EventKind::KeyDown, EventKind::PointerUp, EventKind::Click]);
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 30.0));
    let escape = KeyData { key_code: KeyCode::Escape, modifiers: Modifiers::default() };
    let proceed = ui.key_input(true, escape);
    assert!(!proceed, "o Escape que cancelou o arrasto é consumido");
    assert!(!ui.is_dragging());
    let log = drags.borrow();
    assert_eq!(log[log.len() - 2], "DragLeave@c");
    assert_eq!(log[log.len() - 1], "DragEnd@b:none");
    assert!(!log.iter().any(|e| e.starts_with("Drop")));
    drop(log);
    // Soltar o botão depois não solta nada nem dá clique; o `PointerUp` volta a ser comum.
    ui.pointer_input(input(PointerInputKind::Up, 40.0, 30.0));
    assert_eq!(*pointer.borrow(), ["PointerUp"]);
    // Sem arrasto, o Escape segue o caminho normal do teclado.
    assert!(ui.key_input(true, escape));
    assert_eq!(*pointer.borrow(), ["PointerUp", "KeyDown"]);
}

#[test]
fn external_file_drag_enters_overs_leaves_and_drops_without_a_source() {
    let (ui, _root, _a, b, c) = nested();
    let log: Log = Rc::default();
    let files_seen: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let labels = [(b, "b"), (c, "c")];
    for kind in DRAG_KINDS {
        let (log, files_seen) = (log.clone(), files_seen.clone());
        std::mem::forget(ui.listen(ui.root(), kind, false, move |e: &mut Event| {
            let label = labels.iter().find(|(n, _)| *n == e.target).map_or("?", |(_, l)| *l);
            let dt = e.data_transfer.clone().expect("dataTransfer");
            if matches!(kind, EventKind::DragOver | EventKind::Drop) {
                e.prevent_default();
            }
            if kind == EventKind::DragOver {
                assert_eq!(dt.types(), ["Files"]);
                assert!(dt.files().is_empty(), "os arquivos só são legíveis no drop");
            }
            if kind == EventKind::Drop {
                files_seen.borrow_mut().extend(dt.files());
            }
            log.borrow_mut().push(format!("{kind:?}@{label}"));
        }));
    }
    let pointer = pointer_log(&ui, &[EventKind::PointerMove]);
    // O winit manda um evento por arquivo: o segundo só acrescenta, sem novo `DragEnter`.
    ui.external_drag_enter(vec![PathBuf::from("/tmp/a.txt")], 30.0, 30.0);
    ui.external_drag_enter(vec![PathBuf::from("/tmp/b.txt")], 30.0, 30.0);
    assert!(ui.is_dragging());
    ui.external_drag_over(15.0, 15.0);
    assert!(ui.external_drop(15.0, 15.0));
    assert!(!ui.is_dragging());
    assert_eq!(
        *log.borrow(),
        ["DragEnter@c", "DragOver@c", "DragOver@c", "DragEnter@b", "DragLeave@c", "DragOver@b", "DragOver@b", "Drop@b"]
    );
    assert_eq!(*files_seen.borrow(), [PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b.txt")]);
    assert!(pointer.borrow().is_empty());
    // Fora de um arrasto externo as funções não fazem nada.
    assert!(!ui.external_drop(15.0, 15.0));
    ui.external_drag_over(15.0, 15.0);
    ui.external_drag_leave();
    assert_eq!(log.borrow().len(), 8);
}

#[test]
fn external_drag_leave_sends_dragleave_to_the_target_and_no_dragend() {
    let (ui, _root, _a, b, c) = nested();
    let log = drag_log(&ui, &[(b, "b"), (c, "c")], false, false);
    ui.external_drag_enter(vec![PathBuf::from("/tmp/a.txt")], 30.0, 30.0);
    ui.external_drag_leave();
    assert!(!ui.is_dragging());
    assert_eq!(log.borrow().last().unwrap(), "DragLeave@c");
    assert!(!log.borrow().iter().any(|e| e.starts_with("DragEnd") || e.starts_with("DragStart")));
}

#[test]
fn external_drop_is_refused_when_dragover_is_not_prevented() {
    let (ui, _root, _a, b, c) = nested();
    let log = drag_log(&ui, &[(b, "b"), (c, "c")], false, true);
    ui.external_drag_enter(vec![PathBuf::from("/tmp/a.txt")], 30.0, 30.0);
    assert!(!ui.external_drop(30.0, 30.0));
    assert!(!log.borrow().iter().any(|e| e.starts_with("Drop")));
    assert_eq!(log.borrow().last().unwrap(), "DragLeave@c");
}

#[test]
fn data_transfer_follows_the_html_data_store_rules() {
    let dt = DataTransfer::new();
    // Em modo de escrita (o início do arrasto): o tipo é normalizado e substituído no lugar.
    dt.set_data("Text", "um");
    dt.set_data("URL", "https://example.com");
    dt.set_data("text/plain", "dois");
    assert_eq!(dt.types(), ["text/plain", "text/uri-list"]);
    dt.clear_data(Some("url"));
    assert_eq!(dt.types(), ["text/plain"]);
    dt.set_effect_allowed(EffectAllowed::LinkMove);
    assert_eq!(dt.effect_allowed().as_str(), "linkMove");
    assert_eq!(EffectAllowed::parse("copyLink"), Some(EffectAllowed::CopyLink));
    assert_eq!(EffectAllowed::parse("nope"), None);
    assert!(EffectAllowed::LinkMove.permits(je_ui::DropEffect::Move));
    assert!(!EffectAllowed::LinkMove.permits(je_ui::DropEffect::Copy));
    assert_eq!(EffectAllowed::Uninitialized.initial_drop_effect(), je_ui::DropEffect::Copy);
    assert_eq!(EffectAllowed::None.initial_drop_effect(), je_ui::DropEffect::None);
    assert_eq!(je_ui::DropEffect::parse("link"), Some(je_ui::DropEffect::Link));
    // Só o `drop` lê (fora dele `getData` devolve vazio).
    assert_eq!(dt.get_data("text"), "");
    dt.clear_data(None);
    assert!(dt.types().is_empty());
}

#[test]
fn drag_image_is_kept_from_drag_start() {
    let (ui, _root, _a, b, c) = nested();
    set_draggable(&ui, b);
    std::mem::forget(ui.listen(b, EventKind::DragStart, false, move |e| e.data_transfer.as_ref().unwrap().set_drag_image(c, 3.0, 4.0)));
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 30.0));
    let image = ui.drag_data_transfer().expect("arrasto em andamento").drag_image().expect("imagem de arrasto");
    assert_eq!((image.node, image.x, image.y), (c, 3.0, 4.0));
}
