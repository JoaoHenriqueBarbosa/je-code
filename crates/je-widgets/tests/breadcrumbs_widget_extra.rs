//! Testes extras do `BreadcrumbsWidget` (o original só tem o caso de `reveal`, em `breadcrumbs_widget.rs`):
//! estrutura, reuso de nós, foco e seleção com eventos, clique, layout do flex, rolagem e cores por estado.

mod support;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Content, NodeId, PointerInput, PointerInputKind, Role, Tree, Ui};
use je_widgets::breadcrumbs_widget::{BreadcrumbsItem, BreadcrumbsItemEventType, BreadcrumbsItemRef, BreadcrumbsWidget, IBreadcrumbsItemEvent, IBreadcrumbsWidgetStyles};
use je_widgets::dom::create_el;
use je_widgets::text::{UI_FONT_SIZE, measure_text, natural_line_height, set_node_text, ui_font};
use support::ensure_no_disposables_are_leaked_in_test_suite;

struct TestItem {
    label: String,
    disposed: Rc<Cell<u32>>,
    fail: bool,
}

impl TestItem {
    fn item(label: &str) -> (BreadcrumbsItemRef, Rc<Cell<u32>>) {
        let disposed = Rc::new(Cell::new(0));
        (Rc::new(Self { label: label.to_owned(), disposed: disposed.clone(), fail: false }), disposed)
    }
}

impl BreadcrumbsItem for TestItem {
    fn dispose(&self) {
        self.disposed.set(self.disposed.get() + 1);
    }

    fn equals(&self, other: &dyn BreadcrumbsItem) -> bool {
        other.as_any().downcast_ref::<TestItem>().is_some_and(|other| other.label == self.label)
    }

    fn render(&self, ui: &Ui, container: NodeId, foreground: Rgba) {
        assert!(!self.fail, "falha de render");
        let mut tree = ui.tree_mut();
        let node = create_el(&mut tree, container, Role::Generic, "");
        let font = ui_font(UI_FONT_SIZE, 400);
        let height = natural_line_height(&font);
        let width = set_node_text(&mut tree, node, &self.label, &font, foreground, height);
        tree.set_bounds(node, Rect::new(0.0, 0.0, width, height));
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

const INHERITED: Rgba = Rgba::opaque(0xcc, 0xcc, 0xcc);
const FOREGROUND: Rgba = Rgba::opaque(0x10, 0x10, 0x10);
const HOVER: Rgba = Rgba::opaque(0x20, 0x20, 0x20);
const FOCUS: Rgba = Rgba::opaque(0x30, 0x30, 0x30);
const FOCUS_SELECTION: Rgba = Rgba::opaque(0x40, 0x40, 0x40);
const BACKGROUND: Rgba = Rgba::opaque(0x01, 0x02, 0x03);

fn styles() -> IBreadcrumbsWidgetStyles {
    IBreadcrumbsWidgetStyles {
        breadcrumbs_background: Some(BACKGROUND),
        breadcrumbs_foreground: Some(FOREGROUND),
        breadcrumbs_hover_foreground: Some(HOVER),
        breadcrumbs_focus_foreground: Some(FOCUS),
        breadcrumbs_focus_and_selection_foreground: Some(FOCUS_SELECTION),
    }
}

struct Fixture {
    ui: Ui,
    container: NodeId,
    widget: Rc<BreadcrumbsWidget>,
}

fn fixture(store: &support::LeakGuard, styles: IBreadcrumbsWidgetStyles) -> Fixture {
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 400.0, 100.0));
        let container = create_el(&mut tree, root, Role::Generic, "");
        tree.set_bounds(container, Rect::new(0.0, 0.0, 400.0, 30.0));
        container
    };
    let widget = store.add_rc(BreadcrumbsWidget::new(&ui, container, 3.0, None, ThemeIcon::from_id("chevron-right"), styles, None));
    Fixture { ui, container, widget }
}

fn items(labels: &[&str]) -> Vec<BreadcrumbsItemRef> {
    labels.iter().map(|l| TestItem::item(l).0).collect()
}

fn frames(ui: &Ui) {
    for _ in 0..4 {
        ui.run_animation_frame();
    }
}

/// A cor do primeiro texto do item.
fn item_color(tree: &Tree, node: NodeId) -> Rgba {
    tree.subtree(node)
        .into_iter()
        .find_map(|n| match &tree.style(n).content {
            Content::Text { color, .. } => Some(*color),
            _ => None,
        })
        .expect("texto no item")
}

fn separator_color(tree: &Tree, node: NodeId) -> Rgba {
    match &tree.style(*tree.children(node).last().expect("ícone")).content {
        Content::Codicon { color, .. } => *color,
        other => panic!("conteúdo inesperado: {other:?}"),
    }
}

fn pointer(ui: &Ui, kind: PointerInputKind, x: f64, y: f64) {
    ui.pointer_input(PointerInput::new(kind, x, y));
}

fn click_at(ui: &Ui, x: f64, y: f64) {
    pointer(ui, PointerInputKind::Move, x, y);
    pointer(ui, PointerInputKind::Down, x, y);
    pointer(ui, PointerInputKind::Up, x, y);
}

#[test]
fn builds_a_list_of_items_each_ending_with_the_separator_icon() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    f.widget.set_items(items(&["a", "b", "c"]));
    let tree = f.ui.tree();
    assert!(tree.has_class(f.widget.dom_node(), "monaco-breadcrumbs"));
    assert_eq!(tree.role(f.widget.dom_node()), Role::List);
    assert_eq!(tree.children(f.container), [f.widget.element()]);
    let nodes = f.widget.item_nodes();
    assert_eq!(nodes.len(), 3);
    for node in nodes {
        assert_eq!(tree.role(node), Role::ListItem);
        assert_eq!(tree.classes(node), ["monaco-breadcrumb-item"]);
        let icon = *tree.children(node).last().expect("ícone");
        assert_eq!(tree.classes(icon), ["codicon", "codicon-chevron-right"]);
        assert!(matches!(&tree.style(icon).content, Content::Codicon { id, .. } if id == "chevron-right"));
    }
}

#[test]
fn items_flow_left_to_right_and_the_widget_scrolls_nothing_when_it_fits() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    f.widget.set_items(items(&["first", "second"]));
    let tree = f.ui.tree();
    let font = ui_font(UI_FONT_SIZE, 400);
    let nodes = f.widget.item_nodes();
    let (a, b) = (tree.bounds(nodes[0]), tree.bounds(nodes[1]));
    assert_eq!(a.x0, 0.0);
    assert_eq!(a.width(), measure_text("first", &font) + 16.0, "o conteúdo e o ícone de 16 px");
    assert_eq!(b.x0, a.x1);
    // Sem `layout(dim)` a altura é a do conteúdo mais alto (o texto, ou o ícone de 16 px).
    assert_eq!(a.height(), natural_line_height(&font).max(16.0));
}

#[test]
fn layout_with_a_dimension_sizes_the_node_and_centers_the_content() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    f.widget.set_items(items(&["first"]));
    f.widget.layout(Some((300.0, 40.0)));
    assert!(f.ui.has_pending_animation_frame(), "o `modify` do original roda no quadro");
    frames(&f.ui);
    let tree = f.ui.tree();
    let r = tree.bounds(f.widget.dom_node());
    assert_eq!((r.width(), r.height()), (300.0, 40.0));
    let node = f.widget.item_nodes()[0];
    assert_eq!(tree.bounds(node).height(), 40.0, "`height: 100%`");
    let icon = tree.bounds(*tree.children(node).last().expect("ícone"));
    assert_eq!((icon.y0, icon.height()), (12.0, 16.0), "`align-items: center`");
    let text = tree.bounds(tree.children(node)[0]);
    assert_eq!(text.y0, (40.0 - text.height()) / 2.0);
    // O mesmo `dim` outra vez não refaz nada.
    drop(tree);
    f.widget.layout(Some((300.0, 40.0)));
    assert!(!f.ui.has_pending_animation_frame());
}

#[test]
fn set_items_keeps_the_nodes_and_items_of_the_common_prefix() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    let (a, a_disposed) = TestItem::item("a");
    let (b, b_disposed) = TestItem::item("b");
    f.widget.set_items(vec![a.clone(), b]);
    let nodes = f.widget.item_nodes();

    let (a_again, a_again_disposed) = TestItem::item("a");
    let (c, _) = TestItem::item("c");
    f.widget.set_items(vec![a_again, c]);
    assert_eq!(f.widget.item_nodes(), nodes, "mesmos nós");
    assert!(Rc::ptr_eq(&f.widget.get_items()[0], &a), "o item igual que já estava é mantido");
    assert_eq!((a_disposed.get(), b_disposed.get(), a_again_disposed.get()), (0, 1, 1), "o removido e o duplicado do prefixo são descartados");
    assert_eq!(f.widget.get_items().len(), 2);
}

#[test]
fn removed_nodes_go_to_the_free_list_and_come_back() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    f.widget.set_items(items(&["a", "b", "c"]));
    let nodes = f.widget.item_nodes();
    f.widget.set_items(items(&["a"]));
    assert_eq!(f.widget.item_nodes(), [nodes[0]]);
    assert_eq!(f.ui.tree().parent(nodes[1]), None, "o nó livre sai da árvore");
    f.widget.set_items(items(&["a", "x", "y"]));
    let again = f.widget.item_nodes();
    assert_eq!(again.len(), 3);
    assert!(again.contains(&nodes[1]) && again.contains(&nodes[2]), "os nós livres são reusados");
}

#[test]
fn a_render_that_fails_shows_a_marker_instead() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    let failing: BreadcrumbsItemRef = Rc::new(TestItem { label: "x".to_owned(), disposed: Rc::new(Cell::new(0)), fail: true });
    f.widget.set_items(vec![failing]);
    let tree = f.ui.tree();
    let node = f.widget.item_nodes()[0];
    let text = tree.subtree(node).into_iter().find_map(|n| match &tree.style(n).content {
        Content::Text { text, .. } => Some(text.clone()),
        _ => None,
    });
    assert_eq!(text.as_deref(), Some("<<RENDER ERROR>>"));
}

#[test]
fn focus_moves_through_the_items_and_fires_events() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    let list = items(&["a", "b", "c"]);
    f.widget.set_items(list.clone());
    let events: Rc<RefCell<Vec<IBreadcrumbsItemEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    store.add(f.widget.on_did_focus_item().listen(move |e| sink.borrow_mut().push(e.clone())));

    f.widget.focus_next(None);
    assert!(Rc::ptr_eq(&f.widget.get_focused().expect("foco"), &list[0]), "sem foco (índice -1) o `focusNext` vai ao primeiro");
    f.widget.set_focused(Some(&list[1]), None);
    assert!(Rc::ptr_eq(&f.widget.get_focused().expect("foco"), &list[1]));
    let nodes = f.widget.item_nodes();
    assert!(f.ui.tree().has_class(nodes[1], "focused"));
    assert_eq!(f.ui.focused(), Some(nodes[1]));

    f.widget.focus_next(None);
    assert!(Rc::ptr_eq(&f.widget.get_focused().expect("foco"), &list[2]));
    f.widget.focus_next(None);
    assert!(Rc::ptr_eq(&f.widget.get_focused().expect("foco"), &list[2]), "não passa do último");
    f.widget.focus_prev(None);
    f.widget.focus_prev(None);
    f.widget.focus_prev(None);
    assert!(Rc::ptr_eq(&f.widget.get_focused().expect("foco"), &list[0]), "não passa do primeiro");
    let tree = f.ui.tree();
    assert_eq!(nodes.iter().filter(|n| tree.has_class(**n, "focused")).count(), 1, "um item focado por vez");

    let last = events.borrow().last().cloned().expect("evento");
    assert_eq!(last.kind, BreadcrumbsItemEventType::Focus);
    assert_eq!(last.node, Some(nodes[0]));
    drop(tree);

    f.widget.set_focused(None, None);
    assert!(f.widget.get_focused().is_none());
    let cleared = events.borrow().last().cloned().expect("evento");
    assert!(cleared.item.is_none() && cleared.node.is_none(), "o índice -1 dá `undefined`");
}

#[test]
fn dom_focus_goes_to_the_last_item_or_the_previous_focus() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    let list = items(&["a", "b", "c"]);
    f.widget.set_items(list.clone());
    assert!(!f.widget.is_dom_focused());
    f.widget.dom_focus();
    assert!(Rc::ptr_eq(&f.widget.get_focused().expect("foco"), &list[2]));
    assert!(f.widget.is_dom_focused());

    let empty = fixture(&store, IBreadcrumbsWidgetStyles::default());
    empty.widget.dom_focus();
    assert_eq!(empty.ui.focused(), Some(empty.widget.dom_node()), "sem itens o foco vai ao próprio nó");
}

#[test]
fn on_did_change_focus_follows_the_focus_in_and_out_of_the_widget() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    f.widget.set_items(items(&["a"]));
    let changes = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    store.add(f.widget.on_did_change_focus().listen(move |focused| sink.borrow_mut().push(*focused)));
    // O `FocusTracker` adia a perda do foco para o próximo ciclo (`setTimeout(0)`), como o do original.
    je_base::runtime::with_faked_timers(|| {
        f.widget.dom_focus();
        f.ui.blur();
        je_base::runtime::advance_fake_time(0);
    });
    assert_eq!(*changes.borrow(), [true, false]);
}

#[test]
fn selection_marks_one_item_and_fires_the_select_event() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    let list = items(&["a", "b"]);
    f.widget.set_items(list.clone());
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    store.add(f.widget.on_did_select_item().listen(move |e: &IBreadcrumbsItemEvent| sink.borrow_mut().push((e.kind, e.item.is_some()))));
    f.widget.set_selection(Some(&list[1]), None);
    assert!(Rc::ptr_eq(&f.widget.get_selection().expect("seleção"), &list[1]));
    assert!(f.ui.tree().has_class(f.widget.item_nodes()[1], "selected"));
    f.widget.set_selection(Some(&list[0]), None);
    assert!(!f.ui.tree().has_class(f.widget.item_nodes()[1], "selected"));
    assert_eq!(*events.borrow(), [(BreadcrumbsItemEventType::Select, true), (BreadcrumbsItemEventType::Select, true)]);
}

#[test]
fn set_items_clears_the_focus() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    let list = items(&["a", "b"]);
    f.widget.set_items(list.clone());
    f.widget.set_focused(Some(&list[0]), None);
    f.widget.set_items(items(&["a", "b", "c"]));
    assert!(f.widget.get_focused().is_none());
    assert!(f.widget.item_nodes().iter().all(|n| !f.ui.tree().has_class(*n, "focused")));
}

#[test]
fn a_click_focuses_and_selects_the_item_with_the_pointer_as_payload() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    let list = items(&["alpha", "beta"]);
    f.widget.set_items(list.clone());
    frames(&f.ui);
    let (select, focus) = (Rc::new(RefCell::new(Vec::new())), Rc::new(RefCell::new(Vec::new())));
    let (s, fo) = (select.clone(), focus.clone());
    store.add(f.widget.on_did_select_item().listen(move |e| s.borrow_mut().push(e.payload.as_ref().is_some_and(|p| p.downcast_ref::<je_ui::PointerData>().is_some()))));
    store.add(f.widget.on_did_focus_item().listen(move |e| fo.borrow_mut().push(e.payload.is_some())));
    let second = f.widget.item_nodes()[1];
    let r = f.ui.tree().absolute_bounds(second);
    // O clique cai no texto do item, um descendente: o `_onClick` sobe até achar o nó do item.
    click_at(&f.ui, r.x0 + 4.0, r.y0 + 4.0);
    assert!(Rc::ptr_eq(&f.widget.get_focused().expect("foco"), &list[1]));
    assert!(Rc::ptr_eq(&f.widget.get_selection().expect("seleção"), &list[1]));
    assert_eq!((focus.borrow().clone(), select.borrow().clone()), (vec![true], vec![true]));
}

#[test]
fn a_click_is_ignored_while_disabled() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    f.widget.set_items(items(&["alpha"]));
    f.widget.set_enabled(false);
    let node = f.widget.item_nodes()[0];
    assert!(f.ui.tree().has_class(f.widget.dom_node(), "disabled"));
    assert_eq!(f.ui.tree().cursor(node), je_ui::Cursor::Default);
    let r = f.ui.tree().absolute_bounds(node);
    click_at(&f.ui, r.x0 + 4.0, r.y0 + 4.0);
    assert!(f.widget.get_selection().is_none());
    f.widget.set_enabled(true);
    assert_eq!(f.ui.tree().cursor(node), je_ui::Cursor::Pointer);
    click_at(&f.ui, r.x0 + 4.0, r.y0 + 4.0);
    assert!(f.widget.get_selection().is_some());
}

#[test]
fn reveal_scrolls_the_item_to_the_left_edge_on_the_next_frame() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    let list = items(&["aaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbb", "ccccccccccccccc", "ddddddddddddddd"]);
    f.widget.set_items(list.clone());
    f.widget.layout(Some((150.0, 22.0)));
    frames(&f.ui);

    let nodes = f.widget.item_nodes();
    assert_eq!(f.ui.tree().bounds(nodes[0]).x0, 0.0);
    f.widget.reveal(&list[2]);
    assert_eq!(f.ui.tree().bounds(nodes[0]).x0, 0.0, "sem `minimal` o `reveal` espera o quadro");
    f.ui.run_animation_frame();
    let tree = f.ui.tree();
    assert!(tree.bounds(nodes[0]).x0 < 0.0, "rolou para a esquerda");
    assert!(tree.bounds(nodes[2]).x0.abs() < 1.0, "o item revelado fica na borda esquerda: {}", tree.bounds(nodes[2]).x0);
    assert_eq!(tree.bounds(nodes[1]).x0 - tree.bounds(nodes[0]).x1, 0.0, "os itens continuam colados");
}

#[test]
fn focusing_an_item_out_of_view_scrolls_it_in_at_once() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    let list = items(&["aaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbb", "ccccccccccccccc", "ddddddddddddddd"]);
    f.widget.set_items(list.clone());
    f.widget.layout(Some((150.0, 22.0)));
    frames(&f.ui);
    f.widget.set_focused(Some(&list[3]), None);
    let tree = f.ui.tree();
    let nodes = f.widget.item_nodes();
    assert!(tree.bounds(nodes[0]).x0 < 0.0, "o foco revela de imediato (`minimal`)");
    assert!(tree.bounds(nodes[3]).x0 < 150.0, "o item focado ficou à vista");
}

#[test]
fn paints_the_background_and_colors_the_items_by_state() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, styles());
    let list = items(&["a", "b", "c"]);
    f.widget.set_items(list.clone());
    frames(&f.ui);
    let nodes = f.widget.item_nodes();
    assert_eq!(f.ui.tree().style(f.widget.dom_node()).background, Some(BACKGROUND));
    {
        let tree = f.ui.tree();
        assert_eq!(item_color(&tree, nodes[0]), FOREGROUND);
        assert_eq!(separator_color(&tree, nodes[0]), FOREGROUND, "`color: inherit` no separador");
    }

    f.widget.set_focused(Some(&list[0]), None);
    assert_eq!(item_color(&f.ui.tree(), nodes[0]), FOCUS);
    assert_eq!(separator_color(&f.ui.tree(), nodes[0]), FOCUS);
    f.widget.set_selection(Some(&list[0]), None);
    assert_eq!(item_color(&f.ui.tree(), nodes[0]), FOCUS_SELECTION);
    f.widget.set_selection(Some(&list[1]), None);
    assert_eq!(item_color(&f.ui.tree(), nodes[0]), FOCUS);
    assert_eq!(item_color(&f.ui.tree(), nodes[1]), FOREGROUND, "selecionado sem foco não muda de cor");
}

#[test]
fn hovering_an_unfocused_item_uses_the_hover_color_unless_disabled() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, styles());
    let list = items(&["alpha", "beta"]);
    f.widget.set_items(list.clone());
    frames(&f.ui);
    let nodes = f.widget.item_nodes();
    let r = f.ui.tree().absolute_bounds(nodes[1]);
    pointer(&f.ui, PointerInputKind::Move, r.x0 + 4.0, r.y0 + 4.0);
    assert_eq!(item_color(&f.ui.tree(), nodes[1]), HOVER);
    assert_eq!(item_color(&f.ui.tree(), nodes[0]), FOREGROUND);

    f.widget.set_focused(Some(&list[1]), None);
    assert_eq!(item_color(&f.ui.tree(), nodes[1]), FOCUS, "`:not(.focused)`");

    pointer(&f.ui, PointerInputKind::Move, 399.0, 90.0);
    f.widget.set_focused(None, None);
    assert_eq!(item_color(&f.ui.tree(), nodes[1]), FOREGROUND);

    f.widget.set_enabled(false);
    pointer(&f.ui, PointerInputKind::Move, r.x0 + 4.0, r.y0 + 4.0);
    f.widget.refresh_styles();
    assert_eq!(item_color(&f.ui.tree(), nodes[1]), FOREGROUND, "`:not(.disabled)`");
}

#[test]
fn without_styles_the_content_keeps_the_inherited_color() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&store, IBreadcrumbsWidgetStyles::default());
    f.widget.set_items(items(&["a"]));
    let node = f.widget.item_nodes()[0];
    assert_eq!(item_color(&f.ui.tree(), node), INHERITED);
    let other = Rgba::opaque(9, 9, 9);
    f.widget.set_inherited_foreground(other);
    assert_eq!(item_color(&f.ui.tree(), node), other);
    assert_eq!(f.ui.tree().style(f.widget.dom_node()).background, None);
}

#[test]
fn dispose_removes_the_nodes_and_disposes_the_items() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = {
        let ui = Ui::new();
        let container = {
            let mut tree = ui.tree_mut();
            let root = tree.root();
            create_el(&mut tree, root, Role::Generic, "")
        };
        let widget = BreadcrumbsWidget::new(&ui, container, 3.0, None, ThemeIcon::from_id("chevron-right"), IBreadcrumbsWidgetStyles::default(), None);
        Fixture { ui, container, widget }
    };
    let (item, disposed) = TestItem::item("a");
    f.widget.set_items(vec![item]);
    f.widget.dispose();
    assert_eq!(disposed.get(), 1);
    assert!(f.ui.tree().children(f.container).is_empty());
    assert!(f.widget.get_items().is_empty());
    f.widget.dispose();
    assert_eq!(disposed.get(), 1, "descartar de novo não repete");
}
