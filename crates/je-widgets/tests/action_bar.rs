//! Testes extras do `ActionBar` e do `ActionViewItem` (`src/vs/base/browser/ui/actionbar/`): o VS Code 1.143.1
//! não tem teste dedicado do action bar (os do toolbar o exercitam), então estes cobrem o comportamento
//! portado: teclado, foco, `push`/`pull`/`clear`, ações desabilitadas, runner, contexto, estilos de estado
//! e medidas. Não fazem parte do porte caso a caso.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::actions::{Action, ActionRef, ActionRunner, IAction, IActionRunner, Separator};
use je_base::dynamic::Dynamic;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_base::runtime;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Content, KeyData, Modifiers, NodeId, PointerInput, PointerInputKind, Ui};
use je_widgets::action_bar::{ActionBar, ActionTrigger, ActionsOrientation, IActionBarOptions, IActionOptions, prepare_actions};
use je_widgets::action_view_items::{ActionBarStyles, BaseActionViewItem, IActionViewItem, IActionViewItemOptions};
use support::{ensure_no_disposables_are_leaked_in_test_suite, LeakGuard};

fn setup(width: u32, height: u32) -> (Ui, NodeId) {
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 800.0, 600.0));
        let container = tree.create(root, je_ui::Role::Generic);
        tree.set_bounds(container, Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
        container
    };
    (ui, container)
}

fn bar(store: &LeakGuard, ui: &Ui, container: NodeId, options: IActionBarOptions) -> Rc<ActionBar> {
    store.add_rc(ActionBar::new(ui, container, options))
}

/// Uma ação que conta quantas vezes rodou e guarda o último contexto.
struct Probe {
    action: ActionRef,
    runs: Rc<Cell<u32>>,
    context: Rc<RefCell<Option<Dynamic>>>,
}

fn probe(store: &LeakGuard, id: &str, label: &str, class: &str, enabled: bool) -> Probe {
    let runs = Rc::new(Cell::new(0));
    let context = Rc::new(RefCell::new(None));
    let (r, c) = (runs.clone(), context.clone());
    let action = store.add_rc(Action::new(
        id,
        label,
        class,
        enabled,
        Some(Rc::new(move |ctx| {
            r.set(r.get() + 1);
            *c.borrow_mut() = Some(ctx);
            Dynamic::Undefined.into()
        })),
    ));
    Probe { action, runs, context }
}

fn press(ui: &Ui, key: KeyCode, down: bool) -> bool {
    ui.key_input(down, KeyData { key_code: key, modifiers: Modifiers::default() })
}

fn label_of(ui: &Ui, bar: &ActionBar, index: usize) -> NodeId {
    let tree = ui.tree();
    let li = tree.children(bar.actions_list())[index];
    tree.children_with_class(li, "action-label")[0]
}

fn li_of(ui: &Ui, bar: &ActionBar, index: usize) -> NodeId {
    ui.tree().children(bar.actions_list())[index]
}

fn labeled_actions(store: &LeakGuard, names: &[&str]) -> Vec<Probe> {
    names.iter().map(|n| probe(store, n, n, "", true)).collect()
}

#[test]
fn push_creates_one_item_per_action_with_the_dom_classes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    assert!(b.is_empty());
    let actions = labeled_actions(&store, &["a", "b", "c"]);
    b.push(&actions.iter().map(|p| p.action.clone()).collect::<Vec<_>>(), IActionOptions::default());

    assert_eq!(b.length(), 3);
    assert!(!b.is_empty());
    let tree = ui.tree();
    assert!(tree.has_class(b.dom_node(), "monaco-action-bar"));
    assert!(!tree.has_class(b.dom_node(), "vertical"));
    assert!(tree.has_class(b.actions_list(), "actions-container"));
    assert_eq!(tree.children(b.actions_list()).len(), 3);
    drop(tree);
    let tree = ui.tree();
    for i in 0..3 {
        assert!(tree.has_class(li_of(&ui, &b, i), "action-item"));
        assert!(tree.has_class(label_of(&ui, &b, i), "action-label"));
    }
    drop(tree);
    assert_eq!(b.get_action(1).unwrap().id(), "b");
    assert!(b.has_action(&actions[2].action));
    assert!(b.get_action(3).is_none());
}

#[test]
fn push_at_an_index_inserts_and_keeps_inserting_in_order() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let actions = labeled_actions(&store, &["a", "b", "x", "y"]);
    b.push(&[actions[0].action.clone(), actions[1].action.clone()], IActionOptions::default());
    b.push(&[actions[2].action.clone(), actions[3].action.clone()], IActionOptions { index: Some(1), ..Default::default() });

    let ids: Vec<String> = (0..b.length()).map(|i| b.get_action(i).unwrap().id()).collect();
    assert_eq!(ids, ["a", "x", "y", "b"]);
    // a ordem dos `li` na árvore é a dos itens
    let label_texts: Vec<String> = (0..4)
        .map(|i| {
            let tree = ui.tree();
            let label = label_of(&ui, &b, i);
            let text = tree.children(label)[0];
            match &tree.style(text).content {
                Content::Text { text, .. } => text.clone(),
                other => panic!("conteúdo inesperado {other:?}"),
            }
        })
        .collect();
    assert_eq!(label_texts, ["a", "x", "y", "b"]);
}

#[test]
fn pull_and_clear_dispose_the_items_and_their_nodes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let actions = labeled_actions(&store, &["a", "b", "c"]);
    b.push(&actions.iter().map(|p| p.action.clone()).collect::<Vec<_>>(), IActionOptions::default());

    b.pull(1);
    assert_eq!(b.length(), 2);
    assert_eq!(ui.tree().children(b.actions_list()).len(), 2);
    assert_eq!(b.get_action(1).unwrap().id(), "c");
    b.pull(5); // fora do intervalo: nada
    assert_eq!(b.length(), 2);

    b.clear();
    assert!(b.is_empty());
    assert!(ui.tree().children(b.actions_list()).is_empty());
    b.clear(); // de novo: nada
}

#[test]
fn horizontal_bar_navigates_with_left_and_right_and_loops() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let actions = labeled_actions(&store, &["a", "b", "c"]);
    b.push(&actions.iter().map(|p| p.action.clone()).collect::<Vec<_>>(), IActionOptions::default());

    b.focus(true);
    assert_eq!(b.focused_item(), Some(0));
    assert_eq!(ui.focused(), Some(label_of(&ui, &b, 0)));
    assert!(b.is_focused());
    assert!(b.is_item_focused(0));

    assert!(!press(&ui, KeyCode::RightArrow, true), "a seta foi tratada (preventDefault)");
    assert_eq!(b.focused_item(), Some(1));
    press(&ui, KeyCode::RightArrow, true);
    press(&ui, KeyCode::RightArrow, true);
    assert_eq!(b.focused_item(), Some(0), "dá a volta");
    press(&ui, KeyCode::LeftArrow, true);
    assert_eq!(b.focused_item(), Some(2), "volta para o último");
    assert_eq!(ui.focused(), Some(label_of(&ui, &b, 2)));
    press(&ui, KeyCode::UpArrow, true);
    assert_eq!(b.focused_item(), Some(2), "cima e baixo não navegam numa barra horizontal");
}

#[test]
fn vertical_bar_navigates_with_up_and_down() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(100, 100);
    let b = bar(&store, &ui, container, IActionBarOptions { orientation: Some(ActionsOrientation::Vertical), ..Default::default() });
    assert!(ui.tree().has_class(b.dom_node(), "vertical"));
    let actions = labeled_actions(&store, &["a", "b", "c"]);
    b.push(&actions.iter().map(|p| p.action.clone()).collect::<Vec<_>>(), IActionOptions::default());
    b.focus(true);
    press(&ui, KeyCode::DownArrow, true);
    assert_eq!(b.focused_item(), Some(1));
    press(&ui, KeyCode::UpArrow, true);
    press(&ui, KeyCode::UpArrow, true);
    assert_eq!(b.focused_item(), Some(2));
    press(&ui, KeyCode::RightArrow, true);
    assert_eq!(b.focused_item(), Some(2), "esquerda e direita não navegam numa barra vertical");
}

#[test]
fn home_and_end_focus_the_first_and_the_last_item() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let actions = labeled_actions(&store, &["a", "b", "c", "d"]);
    b.push(&actions.iter().map(|p| p.action.clone()).collect::<Vec<_>>(), IActionOptions::default());
    b.focus_index(1);
    press(&ui, KeyCode::End, true);
    assert_eq!(b.focused_item(), Some(3));
    press(&ui, KeyCode::Home, true);
    assert_eq!(b.focused_item(), Some(0));
}

#[test]
fn focus_only_enabled_items_skips_disabled_items_and_separators() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions { focus_only_enabled_items: true, ..Default::default() });
    let a = probe(&store, "a", "a", "", true);
    let off = probe(&store, "off", "off", "", false);
    let separator: ActionRef = Separator::new();
    let c = probe(&store, "c", "c", "", true);
    b.push(&[a.action.clone(), off.action.clone(), separator, c.action.clone()], IActionOptions::default());
    b.focus(true);
    assert_eq!(b.focused_item(), Some(0));
    press(&ui, KeyCode::RightArrow, true);
    assert_eq!(b.focused_item(), Some(3), "pulou o desabilitado e o separador");
    press(&ui, KeyCode::RightArrow, true);
    assert_eq!(b.focused_item(), Some(0));
    press(&ui, KeyCode::LeftArrow, true);
    assert_eq!(b.focused_item(), Some(3));
}

#[test]
fn separators_are_always_skipped_while_disabled_items_can_be_focused() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let a = probe(&store, "a", "a", "", true);
    let off = probe(&store, "off", "off", "", false);
    let separator: ActionRef = Separator::new();
    b.push(&[a.action.clone(), separator, off.action.clone()], IActionOptions::default());
    b.focus(true);
    press(&ui, KeyCode::RightArrow, true);
    assert_eq!(b.focused_item(), Some(2), "o separador é sempre pulado, o desabilitado não (sem focus_only_enabled_items)");
    assert_eq!(ui.focused(), Some(label_of(&ui, &b, 2)));
}

#[test]
fn prevent_loop_navigation_stops_at_the_ends() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions { prevent_loop_navigation: true, ..Default::default() });
    let actions = labeled_actions(&store, &["a", "b"]);
    b.push(&actions.iter().map(|p| p.action.clone()).collect::<Vec<_>>(), IActionOptions::default());
    b.focus(true);
    assert!(press(&ui, KeyCode::LeftArrow, true), "sem navegar o evento não é consumido");
    assert_eq!(b.focused_item(), Some(0));
    press(&ui, KeyCode::RightArrow, true);
    assert!(press(&ui, KeyCode::RightArrow, true));
    assert_eq!(b.focused_item(), Some(1));
}

#[test]
fn enter_and_space_run_the_focused_action_on_key_up_by_default() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let a = probe(&store, "a", "a", "", true);
    let c = probe(&store, "c", "c", "", true);
    b.push(&[a.action.clone(), c.action.clone()], IActionOptions::default());
    b.focus_index(1);

    press(&ui, KeyCode::Enter, true);
    assert_eq!(c.runs.get(), 0, "ainda não: o padrão roda no keyup");
    press(&ui, KeyCode::Enter, false);
    assert_eq!(c.runs.get(), 1);
    assert_eq!(a.runs.get(), 0);
    press(&ui, KeyCode::Space, true);
    press(&ui, KeyCode::Space, false);
    assert_eq!(c.runs.get(), 2);
    press(&ui, KeyCode::Enter, false);
    assert_eq!(c.runs.get(), 2, "keyup sem keydown antes não roda");
}

#[test]
fn trigger_keys_can_run_on_key_down_and_use_other_keys() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions { trigger_keys: Some(ActionTrigger { keys: Some(vec![KeyCode::KeyA]), key_down: true }), ..Default::default() });
    let a = probe(&store, "a", "a", "", true);
    b.push(std::slice::from_ref(&a.action), IActionOptions::default());
    b.focus(true);
    press(&ui, KeyCode::Enter, true);
    assert_eq!(a.runs.get(), 0, "Enter deixou de ser tecla de disparo");
    press(&ui, KeyCode::KeyA, true);
    assert_eq!(a.runs.get(), 1, "roda no keydown");
}

#[test]
fn a_disabled_action_does_not_run_and_the_runner_events_are_forwarded() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let enabled = probe(&store, "on", "on", "", true);
    let disabled = probe(&store, "off", "off", "", false);
    b.push(&[enabled.action.clone(), disabled.action.clone()], IActionOptions::default());
    let events = Rc::new(RefCell::new(Vec::<String>::new()));
    let (e1, e2) = (events.clone(), events.clone());
    store.add(b.on_will_run().listen(move |e| e1.borrow_mut().push(format!("will:{}", e.action.id()))));
    store.add(b.on_did_run().listen(move |e| e2.borrow_mut().push(format!("did:{}", e.action.id()))));

    let _ = b.run(&enabled.action, None);
    let _ = b.run(&disabled.action, None);
    runtime::run_microtasks();
    assert_eq!(enabled.runs.get(), 1);
    assert_eq!(disabled.runs.get(), 0);
    assert_eq!(*events.borrow(), ["will:on", "did:on"]);
}

#[test]
fn set_action_runner_forwards_events_from_the_new_runner_only() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let first = store.add_rc(ActionRunner::new());
    let second = store.add_rc(ActionRunner::new());
    let b = bar(&store, &ui, container, IActionBarOptions { action_runner: Some(first.clone() as Rc<dyn IActionRunner>), ..Default::default() });
    let a = probe(&store, "a", "a", "", true);
    b.push(std::slice::from_ref(&a.action), IActionOptions::default());
    let count = Rc::new(Cell::new(0));
    let c = count.clone();
    store.add(b.on_did_run().listen(move |_| c.set(c.get() + 1)));

    let _ = first.run(&a.action, None);
    runtime::run_microtasks();
    assert_eq!(count.get(), 1);
    b.set_action_runner(second.clone() as Rc<dyn IActionRunner>);
    let _ = first.run(&a.action, None);
    runtime::run_microtasks();
    assert_eq!(count.get(), 1, "o runner antigo não repassa mais");
    let _ = second.run(&a.action, None);
    runtime::run_microtasks();
    assert_eq!(count.get(), 2);
    // os itens passam a usar o runner novo
    let item = b.view_items()[0].clone();
    assert!(Rc::ptr_eq(&(item.action_runner()), &(second as Rc<dyn IActionRunner>)));
}

#[test]
fn a_click_runs_the_action_with_the_default_click_context() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let a = probe(&store, "a", "Hello", "", true);
    b.push(std::slice::from_ref(&a.action), IActionOptions::default());
    let label = label_of(&ui, &b, 0);
    let bounds = ui.tree().absolute_bounds(label);
    let (x, y) = ((bounds.x0 + bounds.x1) / 2.0, (bounds.y0 + bounds.y1) / 2.0);
    ui.pointer_input(PointerInput::new(PointerInputKind::Down, x, y));
    assert!(ui.tree().has_class(li_of(&ui, &b, 0), "active"), "o mouse baixo marca o item como ativo");
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, x, y));
    assert!(!ui.tree().has_class(li_of(&ui, &b, 0), "active"));

    assert_eq!(a.runs.get(), 1);
    let context = a.context.borrow().clone().expect("contexto");
    match context {
        Dynamic::Object(o) => assert!(matches!(o.get(&"preserveFocus".into()), Some(Dynamic::Bool(false)))),
        _ => panic!("o contexto padrão é um objeto"),
    }
}

#[test]
fn the_bar_context_reaches_the_items_and_the_action() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions { context: Some(Dynamic::Number(7.0)), ..Default::default() });
    let a = probe(&store, "a", "a", "", true);
    b.push(std::slice::from_ref(&a.action), IActionOptions::default());
    assert!(matches!(b.context(), Some(Dynamic::Number(n)) if n == 7.0));
    b.focus(true);
    press(&ui, KeyCode::Enter, true);
    press(&ui, KeyCode::Enter, false);
    assert!(matches!(a.context.borrow().clone(), Some(Dynamic::Number(n)) if n == 7.0));

    b.set_context(Some(Dynamic::Number(9.0)));
    press(&ui, KeyCode::Enter, true);
    press(&ui, KeyCode::Enter, false);
    assert!(matches!(a.context.borrow().clone(), Some(Dynamic::Number(n)) if n == 9.0));
}

#[test]
fn escape_fires_on_did_cancel_only_when_someone_listens() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let a = probe(&store, "a", "a", "", true);
    b.push(std::slice::from_ref(&a.action), IActionOptions::default());
    b.focus(true);
    assert!(press(&ui, KeyCode::Escape, true), "ninguém escuta: o Escape passa");
    let cancelled = Rc::new(Cell::new(0));
    let c = cancelled.clone();
    store.add(b.on_did_cancel().listen(move |_| c.set(c.get() + 1)));
    assert!(!press(&ui, KeyCode::Escape, true));
    assert_eq!(cancelled.get(), 1);
}

#[test]
fn on_did_blur_fires_only_when_the_focus_leaves_the_bar() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let root = ui.root();
    let outside = ui.tree_mut().create(root, je_ui::Role::Generic);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let actions = labeled_actions(&store, &["a", "b"]);
    b.push(&actions.iter().map(|p| p.action.clone()).collect::<Vec<_>>(), IActionOptions::default());
    let blurs = Rc::new(Cell::new(0));
    let c = blurs.clone();
    store.add(b.on_did_blur().listen(move |_| c.set(c.get() + 1)));

    // O `FocusTracker` adia a perda do foco para o próximo ciclo (`setTimeout(0)`), como o do original.
    let observed = runtime::with_faked_timers(|| {
        b.focus(true);
        press(&ui, KeyCode::RightArrow, true);
        runtime::advance_fake_time(0);
        let after_navigation = (blurs.get(), b.focused_item());
        ui.focus(outside);
        runtime::advance_fake_time(0);
        (after_navigation, (blurs.get(), b.focused_item(), b.is_focused()))
    });
    assert_eq!(observed.0, (0, Some(1)), "navegar dentro da barra não é perder o foco");
    assert_eq!(observed.1, (1, None, false));
}

#[test]
fn set_focusable_controls_which_item_is_a_tab_stop() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let a = probe(&store, "a", "a", "", true);
    let c = probe(&store, "c", "c", "", true);
    b.push(&[a.action.clone(), c.action.clone()], IActionOptions::default());
    // só o primeiro item habilitado entra na ordem de tabulação (#106441); o ActionViewItem guarda o do rótulo
    let label_tab = |i: usize| -> i32 { b.view_items()[i].is_focused() as i32 - 1 };
    assert_eq!((label_tab(0), label_tab(1)), (0, -1));
    b.set_focusable(false);
    assert_eq!((label_tab(0), label_tab(1)), (-1, -1));
    b.set_focusable(true);
    assert_eq!((label_tab(0), label_tab(1)), (0, -1));
}

#[test]
fn action_changes_update_the_item_classes_label_and_size() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let action = store.add_rc(Action::new("a", "Short", "", true, None));
    b.push(&[action.clone() as ActionRef], IActionOptions::default());
    let (li, label) = (li_of(&ui, &b, 0), label_of(&ui, &b, 0));
    let width_before = b.get_width(0);

    action.set_enabled(false);
    assert!(ui.tree().has_class(li, "disabled") && ui.tree().has_class(label, "disabled"));
    action.set_enabled(true);
    assert!(!ui.tree().has_class(li, "disabled") && !ui.tree().has_class(label, "disabled"));

    action.set_checked(Some(true));
    assert!(ui.tree().has_class(label, "checked"));
    action.set_checked(Some(false));
    assert!(!ui.tree().has_class(label, "checked"));

    action.set_label("A much longer label".to_owned());
    assert!(b.get_width(0) > width_before, "o item cresce com o rótulo e a barra reposiciona");
}

#[test]
fn icon_items_use_the_codicon_and_measure_22_by_22() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let action = probe(&store, "add", "Add", "codicon codicon-add", true);
    b.push(std::slice::from_ref(&action.action), IActionOptions { item: IActionViewItemOptions { icon: Some(true), label: Some(false), ..Default::default() }, ..Default::default() });
    let label = label_of(&ui, &b, 0);
    let tree = ui.tree();
    assert!(tree.has_class(label, "codicon") && tree.has_class(label, "codicon-add"));
    assert!(matches!(&tree.style(label).content, Content::Codicon { id, size, .. } if id == "add" && *size == 16.0));
    drop(tree);
    assert_eq!((b.get_width(0), b.get_height(0)), (22.0, 22.0));
    // centrado na vertical na barra de 30px
    assert_eq!(ui.tree().bounds(li_of(&ui, &b, 0)).y0, 4.0);
}

#[test]
fn separators_measure_9_by_26_horizontally_and_stack_vertically() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions::default());
    let separator: ActionRef = Separator::new();
    b.push(&[separator], IActionOptions::default());
    assert_eq!((b.get_width(0), b.get_height(0)), (9.0, 26.0));
    assert!(ui.tree().has_class(label_of(&ui, &b, 0), "separator"));

    let (ui, container) = setup(120, 200);
    let v = bar(&store, &ui, container, IActionBarOptions { orientation: Some(ActionsOrientation::Vertical), ..Default::default() });
    let a = probe(&store, "a", "a", "", true);
    let separator: ActionRef = Separator::new();
    let c = probe(&store, "c", "c", "", true);
    v.push(&[a.action.clone(), separator, c.action.clone()], IActionOptions::default());
    let tree = ui.tree();
    let rects: Vec<Rect> = (0..3).map(|i| tree.bounds(li_of(&ui, &v, i))).collect();
    assert_eq!(rects[0].y0, 0.0);
    assert_eq!(rects[1].y0, rects[0].y1);
    assert_eq!(rects[1].height(), 12.0);
    assert_eq!(rects[2].y0, rects[1].y1);
    assert!(rects.iter().all(|r| r.width() == 120.0), "os itens de uma barra vertical ocupam a largura toda");
}

#[test]
fn state_styles_follow_hover_active_focus_checked_and_disabled() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let color = |v: u8| Some(Rgba::new(v, 0, 0, 255));
    let styles = Rc::new(ActionBarStyles {
        foreground: color(1),
        disabled_foreground: color(2),
        toolbar_hover_background: color(3),
        toolbar_active_background: color(4),
        focus_border: color(5),
        toggled_background: color(6),
        ..Default::default()
    });
    let b = bar(&store, &ui, container, IActionBarOptions { styles: Some(styles), highlight_toggled_items: true, ..Default::default() });
    let action = store.add_rc(Action::new("a", "Hello", "", true, None));
    b.push(&[action.clone() as ActionRef], IActionOptions::default());
    let label = label_of(&ui, &b, 0);
    let background = || ui.tree().style(label).background;
    assert_eq!(background(), None);

    let bounds = ui.tree().absolute_bounds(label);
    let (x, y) = ((bounds.x0 + bounds.x1) / 2.0, (bounds.y0 + bounds.y1) / 2.0);
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, x, y));
    assert_eq!(background(), color(3), "hover");
    ui.pointer_input(PointerInput::new(PointerInputKind::Down, x, y));
    assert_eq!(background(), color(4), "ativo");
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, x, y));
    assert_eq!(background(), color(3));

    action.set_checked(Some(true));
    assert_eq!(background(), color(6), "alternado vence o hover (o !important do original)");
    action.set_checked(None);

    b.focus(true);
    assert_eq!(ui.tree().style(label).border.map(|b| b.color), color(5), "o foco desenha a borda de focusBorder");

    action.set_enabled(false);
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, 290.0, 29.0));
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, x, y));
    assert_eq!(background(), None, "desabilitado não tem hover");
    let text = ui.tree().children(label)[0];
    assert!(matches!(&ui.tree().style(text).content, Content::Text { color: c, .. } if Some(*c) == color(2)), "o texto desabilitado usa disabledForeground");
}

#[test]
fn aria_label_and_role_follow_the_options_and_the_number_of_items() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = bar(&store, &ui, container, IActionBarOptions { aria_label: Some("Ações".to_owned()), ..Default::default() });
    assert_eq!(ui.tree().label(b.actions_list()), Some("Ações"));
    assert_eq!(ui.tree().role(b.actions_list()), je_ui::Role::Toolbar);
    let a = probe(&store, "a", "a", "", true);
    b.push(std::slice::from_ref(&a.action), IActionOptions::default());
    assert_eq!(ui.tree().role(b.actions_list()), je_ui::Role::Toolbar);
    b.pull(0);
    assert_eq!(ui.tree().role(b.actions_list()), je_ui::Role::Presentation, "sem itens a lista é só apresentação");
    b.set_aria_label("");
    assert_eq!(ui.tree().label(b.actions_list()), None);
}

#[test]
fn a_provider_can_supply_the_view_item() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let created = Rc::new(Cell::new(0));
    let (c, ui2) = (created.clone(), ui.clone());
    let provider: je_widgets::action_bar::IActionViewItemProvider = Rc::new(move |action, options| {
        if action.id() == "custom" {
            c.set(c.get() + 1);
            Some(BaseActionViewItem::new(&ui2, None, action.clone(), options.clone()) as Rc<dyn IActionViewItem>)
        } else {
            None
        }
    });
    let b = bar(&store, &ui, container, IActionBarOptions { action_view_item_provider: Some(provider), ..Default::default() });
    let custom = probe(&store, "custom", "custom", "", true);
    let normal = probe(&store, "normal", "normal", "", true);
    b.push(&[custom.action.clone(), normal.action.clone()], IActionOptions::default());
    assert_eq!(created.get(), 1);
    assert!(ui.tree().children_with_class(li_of(&ui, &b, 0), "action-label").is_empty(), "o item base não cria rótulo");
    assert!(!ui.tree().children_with_class(li_of(&ui, &b, 1), "action-label").is_empty());
}

#[test]
fn prepare_actions_removes_leading_trailing_and_duplicate_separators() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let sep = || -> ActionRef { Separator::new() };
    let (a, c) = (probe(&store, "a", "a", "", true).action, probe(&store, "c", "c", "", true).action);
    let ids = |actions: Vec<ActionRef>| actions.iter().map(|a| a.id()).collect::<Vec<_>>();
    let separator = Separator::ID.to_owned();

    assert!(prepare_actions(Vec::new()).is_empty());
    assert!(prepare_actions(vec![sep(), sep()]).is_empty());
    assert_eq!(ids(prepare_actions(vec![sep(), a.clone(), sep(), sep(), c.clone(), sep()])), ["a", separator.as_str(), "c"]);
}

#[test]
fn disposing_the_bar_removes_its_nodes_and_items() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300, 30);
    let b = ActionBar::new(&ui, container, IActionBarOptions::default());
    let a = probe(&store, "a", "a", "", true);
    b.push(std::slice::from_ref(&a.action), IActionOptions::default());
    let dom_node = b.dom_node();
    b.dispose();
    assert!(!ui.tree().is_alive(dom_node));
    assert!(ui.tree().children(container).is_empty());
}
