//! Testes extras do `HoverWidget` (`src/vs/base/browser/ui/hover/hoverWidget.ts` e
//! `src/vs/platform/hover/browser/hoverWidget.ts`): o VS Code 1.143.1 não tem teste dedicado da caixa, então
//! estes cobrem o comportamento portado (estrutura, ações, posicionamento em volta do alvo, ponteiro, altura
//! máxima, esconder e o atraso do delegado). Não fazem parte do porte caso a caso.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::html_content::IMarkdownString;
use je_base::key_codes::KeyCode;
use je_base::layout::AnchorAlignment;
use je_base::lifecycle::IDisposable;
use je_base::runtime;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Content, KeyData, Modifiers, NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::hover_api::{
    HoverContent, HoverDelay, HoverPlacement, HoverPosition, HoverStyle, HoverTarget, IHoverAction, IHoverAppearanceOptions, IHoverDelegate, IHoverDelegateOptions, IHoverOptions, IHoverPersistenceOptions, IHoverPositionOptions, IHoverTarget, IManagedHoverOptions, ManagedHoverContentOrFactory,
};
use je_widgets::hover_widget::{ClickAction, DefaultHoverDelegate, HoverAction, HoverStyles, HoverWidget, KeyDownAction, WorkbenchHoverWidget, get_hover_accessible_view_hint};
use support::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};

fn new_ui() -> Ui {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 1024.0, 768.0));
    ui
}

fn node_at(ui: &Ui, rect: Rect) -> NodeId {
    let root = ui.root();
    let mut tree = ui.tree_mut();
    let node = tree.create(root, Role::Generic);
    tree.set_bounds(node, rect);
    node
}

fn styles() -> HoverStyles {
    let color = |v: u8| Some(Rgba::new(v, v, v, 255));
    HoverStyles { background: color(30), border: color(60), foreground: color(200), status_bar_background: color(40), link_foreground: color(150), link_active_foreground: color(250), focus_border: color(100) }
}

fn text_options(target: NodeId, text: &str) -> IHoverOptions {
    IHoverOptions::new(HoverContent::Text(text.to_owned()), HoverTarget::Node(target))
}

fn hover(store: &LeakGuard, ui: &Ui, options: IHoverOptions) -> Rc<WorkbenchHoverWidget> {
    let hover = store.add_rc(WorkbenchHoverWidget::new(ui, options, styles(), None));
    // A visão de contexto põe a dica num contêiner; aqui o contêiner é a raiz e só `layout` interessa.
    hover.render(ui.root());
    hover
}

fn box_of(ui: &Ui, hover: &WorkbenchHoverWidget) -> Rect {
    ui.tree().bounds(hover.dom_node())
}

#[test]
fn the_base_widget_has_the_dom_structure_and_the_fade_in_class() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let widget = store.add_rc(HoverWidget::new(&ui, true));
    let tree = ui.tree();
    assert!(tree.has_class(widget.container_dom_node(), "monaco-hover") && tree.has_class(widget.container_dom_node(), "fade-in"));
    assert_eq!(tree.role(widget.container_dom_node()), Role::Tooltip);
    assert!(tree.has_class(widget.contents_dom_node(), "monaco-hover-content"));
    assert!(tree.is_ancestor(widget.container_dom_node(), widget.contents_dom_node()), "o conteúdo fica dentro do elemento rolável do contêiner");
    drop(tree);
    let plain = store.add_rc(HoverWidget::new(&ui, false));
    assert!(!ui.tree().has_class(plain.container_dom_node(), "fade-in"));
}

#[test]
fn the_accessible_view_hint_depends_on_the_flag_and_the_keybinding() {
    assert_eq!(get_hover_accessible_view_hint(false, Some("Alt+F2")), "");
    assert_eq!(get_hover_accessible_view_hint(true, Some("Alt+F2")), "Inspect this in the accessible view with Alt+F2.");
    assert!(get_hover_accessible_view_hint(true, None).contains("not triggerable via keybinding"));
}

#[test]
fn click_and_key_down_actions_run_and_stop_the_event() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let node = node_at(&ui, Rect::new(10.0, 10.0, 110.0, 40.0));
    let runs = Rc::new(Cell::new(0));
    let r = runs.clone();
    let run: Rc<dyn Fn(NodeId)> = Rc::new(move |_| r.set(r.get() + 1));
    store.add_rc(ClickAction::new(&ui, node, run.clone()));
    store.add_rc(KeyDownAction::new(&ui, node, run, vec![KeyCode::Enter, KeyCode::Space]));

    ui.pointer_input(PointerInput::new(PointerInputKind::Down, 50.0, 20.0));
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, 50.0, 20.0));
    assert_eq!(runs.get(), 1);
    ui.focus(node);
    assert!(!ui.key_input(true, KeyData { key_code: KeyCode::Enter, modifiers: Modifiers::default() }), "a tecla foi tratada");
    ui.key_input(true, KeyData { key_code: KeyCode::Space, modifiers: Modifiers::default() });
    assert!(ui.key_input(true, KeyData { key_code: KeyCode::KeyA, modifiers: Modifiers::default() }), "outra tecla passa");
    assert_eq!(runs.get(), 3);
}

#[test]
fn a_hover_action_renders_label_keybinding_and_disabled_state() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let parent = node_at(&ui, Rect::new(0.0, 0.0, 400.0, 22.0));
    let ran = Rc::new(Cell::new(0));
    let r = ran.clone();
    let options = IHoverAction { label: "Show".to_owned(), command_id: "cmd".to_owned(), icon_class: Some("codicon codicon-eye".to_owned()), run: Rc::new(move |_| r.set(r.get() + 1)) };
    let first = store.add_rc(HoverAction::render(&ui, parent, &options, Some("Ctrl+K"), &styles()));
    let second = store.add_rc(HoverAction::render(&ui, parent, &IHoverAction { icon_class: None, ..options.clone() }, None, &styles()));

    assert_eq!(first.action_rendered_label, "Show (Ctrl+K)");
    assert_eq!(second.action_rendered_label, "Show");
    assert_eq!(first.action_label, "Show");
    assert_eq!(first.action_keybinding_label.as_deref(), Some("Ctrl+K"));
    let tree = ui.tree();
    assert!(tree.has_class(first.action_container, "action-container"));
    let (a, b) = (tree.bounds(first.action_container), tree.bounds(second.action_container));
    assert_eq!(b.x0, a.x1 + 16.0, "as ações entram em fila com margin-right de 16px");
    drop(tree);

    second.set_enabled(false);
    assert!(ui.tree().has_class(second.action_container, "disabled"));
    assert_eq!(ui.tree().style(second.action_container).opacity, 0.4);
    let bounds = ui.tree().absolute_bounds(second.action_container);
    let (x, y) = ((bounds.x0 + bounds.x1) / 2.0, (bounds.y0 + bounds.y1) / 2.0);
    ui.pointer_input(PointerInput::new(PointerInputKind::Down, x, y));
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, x, y));
    assert_eq!(ran.get(), 0, "desabilitada: sem ponteiro");
    second.set_enabled(true);
    ui.pointer_input(PointerInput::new(PointerInputKind::Down, x, y));
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, x, y));
    assert_eq!(ran.get(), 1);
}

#[test]
fn by_default_the_hover_goes_above_the_target_aligned_with_its_left_edge() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 400.0, 380.0, 420.0));
    let hover = hover(&store, &ui, text_options(target, "Hello"));
    assert_eq!(hover.hover_position(), HoverPosition::Above);
    assert_eq!((hover.x(), hover.y()), (300.0, 400.0));
    assert_eq!(hover.anchor(), je_base::layout::AnchorPosition::Above);
    let tree = ui.tree();
    assert!(tree.has_class(hover.dom_node(), "monaco-hover") && tree.has_class(hover.dom_node(), "workbench-hover"));
    assert!(!tree.has_class(hover.dom_node(), "with-pointer"));
}

#[test]
fn the_box_measures_the_wrapped_text_with_padding_border_and_line_height() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 400.0, 380.0, 420.0));
    let short = hover(&store, &ui, text_options(target, "Hello"));
    let r = box_of(&ui, &short);
    assert_eq!(r.height(), 19.0 + 8.0 + 2.0, "uma linha de 19px, padding de 4px em cima e embaixo e borda de 1px");
    assert!(r.width() > 16.0 + 2.0 && r.width() < 80.0);

    let long_text = "palavra ".repeat(200);
    let long = hover(&store, &ui, text_options(target, &long_text));
    let r = box_of(&ui, &long);
    assert!(r.width() <= 500.0 + 16.0 + 2.0, "o conteúdo quebra em 500px (mais padding e borda)");
    assert!(r.width() > 400.0);
    // limitada à metade da janela, o resto rola
    assert!(r.height() <= 768.0 * 0.5 + 0.5, "a altura máxima é metade da janela, era {}", r.height());
}

#[test]
fn compact_and_markdown_content_change_the_padding_and_the_paragraph_gap() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 400.0, 380.0, 420.0));
    let mut compact = text_options(target, "Hello");
    compact.appearance = Some(IHoverAppearanceOptions { compact: Some(true), ..Default::default() });
    let compact = hover(&store, &ui, compact);
    assert!(ui.tree().has_class(compact.dom_node(), "compact"));
    assert_eq!(box_of(&ui, &compact).height(), 19.0 + 4.0 + 2.0, "padding de 2px no compact");

    let markdown = IHoverOptions::new(HoverContent::Markdown(IMarkdownString { value: "first\n\nsecond".into(), is_trusted: None, support_theme_icons: None, support_html: None, support_alert_syntax: None, base_uri: None, uris: None }), HoverTarget::Node(target));
    let markdown = hover(&store, &ui, markdown);
    assert_eq!(box_of(&ui, &markdown).height(), 2.0 * 19.0 + 8.0 + 8.0 + 2.0, "dois parágrafos separados por margin de 8px");
}

#[test]
fn it_flips_below_when_there_is_no_room_above() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 5.0, 380.0, 25.0));
    let hover = hover(&store, &ui, text_options(target, "Hello"));
    assert_eq!(hover.hover_position(), HoverPosition::Below);
    assert_eq!(hover.y(), 25.0 - 2.0, "abaixo: o fundo do alvo menos 2px");
    assert_eq!(hover.anchor(), je_base::layout::AnchorPosition::Below);
}

#[test]
fn it_flips_above_when_there_is_no_room_below() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 740.0, 380.0, 760.0));
    let mut options = text_options(target, "Hello");
    options.position = Some(IHoverPositionOptions { hover_position: Some(HoverPosition::Below), ..Default::default() });
    let hover = hover(&store, &ui, options);
    assert_eq!(hover.hover_position(), HoverPosition::Above);
}

#[test]
fn a_right_hover_flips_left_then_below_when_the_window_is_too_narrow() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let with_room_on_the_right = node_at(&ui, Rect::new(100.0, 300.0, 140.0, 320.0));
    let mut options = text_options(with_room_on_the_right, "Hello");
    options.position = Some(IHoverPositionOptions { hover_position: Some(HoverPosition::Right), ..Default::default() });
    let right = hover(&store, &ui, options.clone());
    assert_eq!((right.hover_position(), right.x()), (HoverPosition::Right, 140.0));
    drop(right);

    let at_right_edge = node_at(&ui, Rect::new(960.0, 300.0, 1000.0, 320.0));
    options.target = HoverTarget::Node(at_right_edge);
    let flipped = hover(&store, &ui, options.clone());
    assert_eq!(flipped.hover_position(), HoverPosition::Left);
    let width = box_of(&ui, &flipped).width();
    assert_eq!(flipped.x(), 960.0 - width, "à esquerda: a borda esquerda do alvo menos a largura da dica");

    // um alvo largo ocupando a janela: nem à direita nem à esquerda cabe
    let wide = node_at(&ui, Rect::new(10.0, 300.0, 1014.0, 320.0));
    options.target = HoverTarget::Node(wide);
    let below = hover(&store, &ui, options);
    assert_eq!(below.hover_position(), HoverPosition::Below);
}

#[test]
fn it_right_aligns_when_it_would_pass_the_right_edge_of_the_window() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(1000.0, 400.0, 1020.0, 420.0));
    let hover = hover(&store, &ui, text_options(target, "A reasonably long hover text"));
    let width = box_of(&ui, &hover).width();
    assert!(ui.tree().has_class(hover.dom_node(), "right-aligned"));
    assert_eq!(hover.x(), 1024.0 - width - 2.0);
}

#[test]
fn the_right_anchor_alignment_aligns_the_right_edges() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(600.0, 400.0, 700.0, 420.0));
    let mut options = text_options(target, "Hello");
    options.position = Some(IHoverPositionOptions { anchor_alignment: Some(AnchorAlignment::Right), ..Default::default() });
    let hover = hover(&store, &ui, options);
    assert_eq!(hover.x(), 700.0 - box_of(&ui, &hover).width());
}

#[test]
fn force_position_keeps_the_position_and_limits_the_size() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 5.0, 380.0, 25.0));
    let mut options = text_options(target, "Hello");
    options.position = Some(IHoverPositionOptions { force_position: Some(true), ..Default::default() });
    let hover = hover(&store, &ui, options);
    assert_eq!(hover.hover_position(), HoverPosition::Above, "forçada: não inverte");
}

#[test]
fn a_target_with_x_and_y_places_the_hover_there() {
    struct AtPoint(NodeId);
    impl IHoverTarget for AtPoint {
        fn target_elements(&self) -> Vec<NodeId> {
            vec![self.0]
        }
        fn x(&self) -> Option<f64> {
            Some(210.0)
        }
        fn y(&self) -> Option<f64> {
            Some(310.0)
        }
    }
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let node = node_at(&ui, Rect::new(500.0, 500.0, 520.0, 520.0));
    let options = IHoverOptions::new(HoverContent::Text("x".to_owned()), HoverTarget::Target(Rc::new(AtPoint(node))));
    let hover = hover(&store, &ui, options);
    assert_eq!((hover.x(), hover.y()), (210.0, 310.0));
}

#[test]
fn the_pointer_style_adds_the_pointer_and_offsets_the_hover_by_three_pixels() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 400.0, 380.0, 420.0));
    let mut options = text_options(target, "Hello");
    options.style = Some(HoverStyle::Pointer);
    let hover = hover(&store, &ui, options);
    let tree = ui.tree();
    assert!(tree.has_class(hover.dom_node(), "with-pointer") && tree.has_class(hover.dom_node(), "compact"));
    let pointer = tree.children_with_class(hover.hover_container(), "workbench-hover-pointer");
    assert_eq!(pointer.len(), 1);
    assert!(tree.has_class(pointer[0], "bottom"), "a dica está acima: o ponteiro fica embaixo");
    assert_eq!(hover.y(), 400.0 - 3.0, "o ponteiro desloca o alvo em 3px");
    let width = tree.bounds(hover.dom_node()).width();
    assert_eq!(hover.x(), 340.0 - (width - 2.0) / 2.0, "centralizada no alvo");
    assert_eq!(tree.style(hover.dom_node()).radius, 3.0, "com ponteiro o raio é 3px");
}

#[test]
fn escape_hides_the_hover_and_removes_its_nodes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 400.0, 380.0, 420.0));
    let hover = hover(&store, &ui, text_options(target, "Hello"));
    let disposed = Rc::new(Cell::new(0));
    let d = disposed.clone();
    store.add(hover.on_dispose().listen(move |_| d.set(d.get() + 1)));
    hover.focus();
    ui.key_input(true, KeyData { key_code: KeyCode::Escape, modifiers: Modifiers::default() });
    assert!(hover.is_disposed());
    assert_eq!(disposed.get(), 1);
    assert!(!ui.tree().is_alive(hover.dom_node()));
}

#[test]
fn leaving_the_target_hides_a_text_hover_after_the_debounce_but_not_while_the_mouse_is_in() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 400.0, 380.0, 420.0));
    let hover = hover(&store, &ui, text_options(target, "Hello"));
    let (inside, outside) = ((340.0, 410.0), (600.0, 100.0));
    let observed = runtime::with_faked_timers(|| {
        ui.pointer_input(PointerInput::new(PointerInputKind::Move, inside.0, inside.1));
        runtime::advance_fake_time(500);
        let still_there = !hover.is_disposed();
        ui.pointer_input(PointerInput::new(PointerInputKind::Move, outside.0, outside.1));
        runtime::advance_fake_time(199);
        let before_debounce = !hover.is_disposed();
        runtime::advance_fake_time(1);
        (still_there, before_debounce, hover.is_disposed())
    });
    assert_eq!(observed, (true, true, true));
}

#[test]
fn a_hover_with_actions_does_not_hide_when_the_mouse_moves_onto_it() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 500.0, 380.0, 520.0));
    let mut options = text_options(target, "Hello");
    options.actions = vec![IHoverAction { label: "Act".to_owned(), command_id: "c".to_owned(), icon_class: None, run: Rc::new(|_| {}) }];
    let hover = hover(&store, &ui, options);
    assert_eq!(hover.actions().len(), 1);
    let hover_box = ui.tree().absolute_bounds(hover.dom_node());
    let inside_hover = ((hover_box.x0 + hover_box.x1) / 2.0, hover_box.y0 + 5.0);
    let observed = runtime::with_faked_timers(|| {
        ui.pointer_input(PointerInput::new(PointerInputKind::Move, 340.0, 510.0));
        // do alvo para a dica, passando por fora, mas chegando antes do atraso
        ui.pointer_input(PointerInput::new(PointerInputKind::Move, 900.0, 700.0));
        runtime::advance_fake_time(100);
        ui.pointer_input(PointerInput::new(PointerInputKind::Move, inside_hover.0, inside_hover.1));
        runtime::advance_fake_time(1000);
        !hover.is_disposed()
    });
    assert!(observed, "com ações o mouse sobre a dica a mantém");
}

#[test]
fn the_actions_status_bar_is_laid_out_below_the_contents() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 500.0, 380.0, 520.0));
    let mut options = text_options(target, "Hello");
    options.actions = vec![IHoverAction { label: "Act".to_owned(), command_id: "c".to_owned(), icon_class: Some("codicon codicon-play".to_owned()), run: Rc::new(|_| {}) }];
    let hover = hover(&store, &ui, options);
    let tree = ui.tree();
    let rows = tree.children_with_class(hover.dom_node(), "status-bar");
    assert_eq!(rows.len(), 1);
    let row = tree.bounds(rows[0]);
    assert_eq!(row.height(), 23.0, "22px de linha mais 1px da borda de cima (a borda do tema está definida)");
    assert_eq!(tree.bounds(hover.dom_node()).height(), 19.0 + 8.0 + 23.0 + 2.0);
    let text_action = tree.children_with_class(tree.children_with_class(rows[0], "actions")[0], "action-container");
    assert_eq!(text_action.len(), 1);
    assert_eq!(tree.bounds(text_action[0]).x0, 8.0, "padding de 8px nas ações");
}

#[test]
fn a_long_content_gets_a_vertical_scroll_range() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 400.0, 380.0, 420.0));
    let long_text = "linha\n".repeat(60);
    let hover = hover(&store, &ui, text_options(target, &long_text));
    let tree = ui.tree();
    let content = tree.subtree(hover.dom_node()).into_iter().find(|n| tree.has_class(*n, "monaco-hover-content")).expect("conteúdo");
    let scrollable = tree.parent(content).expect("o elemento rolável");
    let (visible, full) = (tree.bounds(scrollable).height(), tree.bounds(content).height());
    assert!(full > visible, "o conteúdo ({full}) passa da área visível ({visible})");
    assert_eq!(tree.bounds(hover.dom_node()).height(), 768.0 * 0.5, "a dica tem metade da altura da janela");
}

#[test]
fn the_default_delegate_shows_the_hover_in_a_context_view_above_the_target() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let root = ui.root();
    let target = node_at(&ui, Rect::new(300.0, 400.0, 380.0, 420.0));
    let delegate = DefaultHoverDelegate::new(&ui, root, styles(), HoverDelay::Fixed(300.0), Some(HoverPlacement::Element));
    assert_eq!(delegate.delay().resolve(None), 300.0);
    assert_eq!(delegate.placement(), Some(HoverPlacement::Element));

    let widget = delegate
        .show_hover(
            IHoverDelegateOptions {
                managed: IManagedHoverOptions::default(),
                content: HoverContent::Text("Hello".to_owned()),
                target: HoverTarget::Node(target),
                container: None,
                hover_position: None,
                show_pointer: None,
                skip_fade_in_animation: None,
            },
            false,
        )
        .expect("a dica");
    let tree = ui.tree();
    let hover = tree.subtree(root).into_iter().find(|n| tree.has_class(*n, "monaco-hover")).expect("a caixa na árvore");
    let abs = tree.absolute_bounds(hover);
    assert_eq!(abs.x0, 300.0);
    assert_eq!(abs.y1, 400.0, "acima do alvo: a borda de baixo da dica está no topo do alvo");
    drop(tree);
    assert!(!widget.is_disposed());
    widget.dispose();
    delegate.dispose();
    assert!(ui.tree().subtree(root).into_iter().all(|n| !ui.tree().has_class(n, "monaco-hover")), "dispose tira a dica da tela");
    let _ = store;
}

#[test]
fn hover_delay_resolves_a_fixed_or_a_content_dependent_value() {
    assert_eq!(HoverDelay::Fixed(500.0).resolve(None), 500.0);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    let dynamic = HoverDelay::Dynamic(Rc::new(move |content| {
        s.borrow_mut().push(content.is_some());
        if content.is_some() { 100.0 } else { 700.0 }
    }));
    assert_eq!(dynamic.resolve(None), 700.0);
    assert_eq!(dynamic.resolve(Some(&ManagedHoverContentOrFactory::from("x"))), 100.0);
    assert_eq!(*seen.borrow(), [false, true]);
}

#[test]
fn text_content_is_one_node_per_wrapped_line_with_the_foreground_color() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 400.0, 380.0, 420.0));
    let hover = hover(&store, &ui, text_options(target, "one\ntwo"));
    let tree = ui.tree();
    let lines: Vec<String> = tree
        .subtree(hover.dom_node())
        .into_iter()
        .filter_map(|n| match &tree.style(n).content {
            Content::Text { text, color, .. } if tree.has_class(n, "hover-line") => {
                assert_eq!(*color, Rgba::new(200, 200, 200, 255));
                Some(text.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(lines, ["one", "two"], "as quebras de linha do texto são preservadas (pre-wrap)");
}

#[test]
fn a_persistence_option_overrides_whether_hovering_the_hover_keeps_it_open() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let target = node_at(&ui, Rect::new(300.0, 500.0, 380.0, 520.0));
    let mut options = text_options(target, "Hello");
    options.persistence = Some(IHoverPersistenceOptions { hide_on_hover: Some(false), ..Default::default() });
    let hover = hover(&store, &ui, options);
    let hover_box = ui.tree().absolute_bounds(hover.dom_node());
    let inside_hover = ((hover_box.x0 + hover_box.x1) / 2.0, (hover_box.y0 + hover_box.y1) / 2.0);
    let alive = runtime::with_faked_timers(|| {
        ui.pointer_input(PointerInput::new(PointerInputKind::Move, 340.0, 510.0));
        ui.pointer_input(PointerInput::new(PointerInputKind::Move, inside_hover.0, inside_hover.1));
        runtime::advance_fake_time(1000);
        !hover.is_disposed()
    });
    assert!(alive, "hide_on_hover falso deixa a dica aberta com o mouse sobre ela");
}
