//! Testes de `src/dnd.rs`: o `DragEvent` montado a partir dos eventos de arrastar e soltar do `je-ui`.
//!
//! Não há original no VS Code para este arquivo: no DOM o `dragstart`, o `dragover` e o `drop` chegam direto ao
//! ouvinte, e aqui `listen_drag_event` faz esse papel. Cada caso diz qual comportamento do DOM ele fixa.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_render::kurbo::Rect;
use je_ui::{DropEffect, EffectAllowed, EventKind, NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::dnd::{DataTransfers, DragEvent, DragHandler, listen_drag_event};

type Log = Rc<RefCell<Vec<String>>>;

/// Uma árvore raiz(100x100) > source(10,10,90,90) > child(10,10,50,50) (coordenadas do pai).
fn scene() -> (Ui, NodeId, NodeId) {
    let ui = Ui::new();
    let root = ui.root();
    let (source, child) = {
        let mut tree = ui.tree_mut();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 100.0, 100.0));
        let source = tree.create(root, Role::Generic);
        tree.set_bounds(source, Rect::new(10.0, 10.0, 90.0, 90.0));
        tree.set_attribute(source, "draggable", "true");
        let child = tree.create(source, Role::Generic);
        tree.set_bounds(child, Rect::new(10.0, 10.0, 50.0, 50.0));
        (source, child)
    };
    (ui, source, child)
}

fn input(kind: PointerInputKind, x: f64, y: f64) -> PointerInput {
    PointerInput::new(kind, x, y)
}

fn listen(ui: &Ui, node: NodeId, kind: EventKind, guards: &mut Vec<DisposableRef>, handler: impl Fn(&DragEvent) -> bool + 'static) {
    let handler: DragHandler = Rc::new(handler);
    guards.push(listen_drag_event(ui, node, kind, handler));
}

#[test]
fn a_full_drag_reaches_the_handlers_with_the_data_transfer_the_target_and_the_offset() {
    let (ui, source, child) = scene();
    let log: Log = Rc::default();
    let mut guards = Vec::new();

    let start_log = log.clone();
    listen(&ui, source, EventKind::DragStart, &mut guards, move |event| {
        event.data_transfer.set_effect_allowed(EffectAllowed::CopyMove);
        event.data_transfer.set_data(DataTransfers::TEXT, "hello");
        start_log.borrow_mut().push(format!("dragstart@{}", event.target == source));
        false
    });
    let over_log = log.clone();
    listen(&ui, source, EventKind::DragOver, &mut guards, move |event| {
        event.data_transfer.set_drop_effect(DropEffect::Move);
        over_log.borrow_mut().push(format!("dragover@{} offset_y={}", event.target == child, event.offset_y));
        true
    });
    let drop_log = log.clone();
    listen(&ui, source, EventKind::Drop, &mut guards, move |event| {
        drop_log.borrow_mut().push(format!("drop:{}", event.data_transfer.get_data(DataTransfers::TEXT)));
        true
    });
    let end_log = log.clone();
    listen(&ui, source, EventKind::DragEnd, &mut guards, move |event| {
        end_log.borrow_mut().push(format!("dragend:{}", event.data_transfer.drop_effect().as_str()));
        false
    });

    // O botão baixo sobre `child` (dentro do `draggable` `source`), e o ponteiro passa do limiar sobre `child`.
    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 40.0));
    assert!(ui.is_dragging());
    // `child` começa em y = 20 (10 de `source` + 10 dele): o ponteiro em y = 45 está 25 abaixo do topo.
    ui.pointer_input(input(PointerInputKind::Move, 41.0, 45.0));
    ui.pointer_input(input(PointerInputKind::Up, 41.0, 45.0));

    let log = log.borrow();
    assert_eq!(log[0], "dragstart@true");
    assert_eq!(log.last().map(String::as_str), Some("dragend:move"), "o drop foi tratado, o dropEffect final é o escolhido");
    assert!(log.contains(&"dragover@true offset_y=25".to_owned()), "o offsetY é do alvo (o filho): {log:?}");
    assert!(log.contains(&"drop:hello".to_owned()), "o getData funciona no drop: {log:?}");
    assert!(!ui.is_dragging());
}

#[test]
fn a_handler_that_returns_true_cancels_the_drag_start() {
    let (ui, source, _child) = scene();
    let mut guards = Vec::new();
    listen(&ui, source, EventKind::DragStart, &mut guards, |_| true);

    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 40.0));

    assert!(!ui.is_dragging(), "o preventDefault do dragstart cancela o arrasto");
}

#[test]
fn a_dragover_handler_that_returns_false_refuses_the_drop() {
    let (ui, source, _child) = scene();
    let log: Log = Rc::default();
    let mut guards = Vec::new();
    listen(&ui, source, EventKind::DragStart, &mut guards, |event| {
        event.data_transfer.set_data(DataTransfers::TEXT, "hello");
        false
    });
    listen(&ui, source, EventKind::DragOver, &mut guards, |_| false);
    let drop_log = log.clone();
    listen(&ui, source, EventKind::Drop, &mut guards, move |_| {
        drop_log.borrow_mut().push("drop".to_owned());
        true
    });
    let end_log = log.clone();
    listen(&ui, source, EventKind::DragEnd, &mut guards, move |event| {
        end_log.borrow_mut().push(format!("dragend:{}", event.data_transfer.drop_effect().as_str()));
        false
    });

    ui.pointer_input(input(PointerInputKind::Down, 30.0, 30.0));
    ui.pointer_input(input(PointerInputKind::Move, 40.0, 40.0));
    ui.pointer_input(input(PointerInputKind::Up, 40.0, 40.0));

    assert_eq!(*log.borrow(), ["dragend:none"], "sem preventDefault no dragover não há drop");
}
