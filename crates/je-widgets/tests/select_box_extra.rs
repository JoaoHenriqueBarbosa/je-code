//! Testes extras da caixa de seleção (não vêm do VS Code): o comportamento portado de `selectBoxCustom.ts` que o
//! `selectBox.test.ts` não cobre (teclado da lista aberta, opções desabilitadas e separadores, mouse, painel de
//! detalhes, `<select>` fechado, perda de foco, `SelectActionViewItem`). Usam a `ContextView` de verdade como
//! provedor, para a lista ficar posicionada sob a âncora.

mod select_box_support;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::actions::{Action, ActionRef};
use je_base::dynamic::Dynamic;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_render::kurbo::Rect;
use je_ui::{Content, KeyData, Modifiers, NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::action_view_items::{IActionViewItem, SelectActionViewItem};
use je_widgets::context_view::ContextView;
use je_widgets::context_view_api::{ContextViewDOMPosition, IContextViewProvider};
use je_widgets::dom::create_el;
use je_widgets::select_box::{ISelectBoxDelegate, ISelectBoxOptions, ISelectData, ISelectOptionItem, separator_select_option, unthemed_select_box_styles};
use je_widgets::select_box_custom::SelectBoxList;
use select_box_support::{click, key, new_ui};
use support::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};

struct Fixture {
    ui: Ui,
    select_box: Rc<SelectBoxList>,
    selected: Rc<RefCell<Vec<ISelectData>>>,
    container: NodeId,
}

fn fixture(guard: &LeakGuard, options: Vec<ISelectOptionItem>, selected: usize, select_box_options: ISelectBoxOptions) -> Fixture {
    let ui = new_ui();
    let root = ui.root();
    let context_view = guard.add_rc(ContextView::new(&ui, root, ContextViewDOMPosition::Absolute));
    let provider: Rc<dyn IContextViewProvider> = context_view;
    let select_box = guard.add_rc(SelectBoxList::new(&ui, options, selected, provider, unthemed_select_box_styles(), Some(select_box_options)));
    let container = create_el(&mut ui.tree_mut(), root, Role::Generic, "host");
    let height = select_box.natural_size().1;
    ui.tree_mut().set_bounds(container, Rect::new(100.0, 100.0, 300.0, 100.0 + height));
    select_box.render(container);
    let selected_events = Rc::new(RefCell::new(Vec::new()));
    let sink = selected_events.clone();
    guard.add(select_box.on_did_select().listen(move |e| sink.borrow_mut().push(e.clone())));
    Fixture { ui, select_box, selected: selected_events, container }
}

fn fruits() -> Vec<ISelectOptionItem> {
    ["Apple", "Banana", "Cherry"].iter().map(|t| ISelectOptionItem::new(t)).collect()
}

fn press(f: &Fixture, code: KeyCode) {
    f.ui.key_input(true, key(code));
}

fn pointer(f: &Fixture, kind: PointerInputKind, x: f64, y: f64) {
    f.ui.pointer_input(PointerInput::new(kind, x, y));
}

fn row_center(f: &Fixture, index: usize) -> (f64, f64) {
    let tree = f.ui.tree();
    let row = tree.subtree(f.select_box.drop_down_node()).into_iter().filter(|n| tree.has_class(*n, "monaco-list-row")).find(|n| tree.children_with_class(*n, "option-text").first().is_some_and(|t| matches!(&tree.style(*t).content, Content::Text { text, .. } if *text == ["Apple", "Banana", "Cherry", "Date"][index]))).expect("linha");
    let r = tree.absolute_bounds(row);
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

fn focused_row_text(f: &Fixture) -> Option<String> {
    let tree = f.ui.tree();
    let row = tree.subtree(f.select_box.drop_down_node()).into_iter().find(|n| tree.has_class(*n, "monaco-list-row") && tree.has_class(*n, "focused"))?;
    tree.children_with_class(row, "option-text").first().and_then(|t| if let Content::Text { text, .. } = &tree.style(*t).content { Some(text.clone()) } else { None })
}

#[test]
fn renders_the_selected_text_and_the_chevron_in_the_select_element() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&guard, fruits(), 1, ISelectBoxOptions::default());
    let tree = f.ui.tree();
    let select = f.select_box.select_element();
    assert!(tree.has_class(select, "monaco-select-box") && tree.has_class(f.container, "select-container"));
    let text = tree.children_with_class(select, "select-box-text")[0];
    assert!(matches!(&tree.style(text).content, Content::Text { text, .. } if text == "Banana"));
    let chevron = tree.children_with_class(select, "select-box-chevron")[0];
    assert!(matches!(&tree.style(chevron).content, Content::Codicon { id, .. } if id == "chevron-down"));
    assert_eq!(f.select_box.selected_index(), 1);
}

#[test]
fn enter_after_arrow_navigation_fires_on_did_select_once_and_closes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&guard, fruits(), 0, ISelectBoxOptions::default());
    click(&f.ui, f.select_box.select_element());
    assert_eq!(focused_row_text(&f).as_deref(), Some("Apple"));
    press(&f, KeyCode::DownArrow);
    press(&f, KeyCode::DownArrow);
    assert_eq!(focused_row_text(&f).as_deref(), Some("Cherry"));
    assert!(f.selected.borrow().is_empty(), "só dispara ao fechar");
    press(&f, KeyCode::Enter);
    assert_eq!(*f.selected.borrow(), vec![ISelectData { selected: "Cherry".to_owned(), index: 2 }]);
    assert_eq!(f.select_box.aria_expanded(), Some(false));
    assert_eq!(f.ui.focused(), Some(f.select_box.select_element()));
}

#[test]
fn escape_restores_the_previous_selection_without_firing() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&guard, fruits(), 0, ISelectBoxOptions::default());
    click(&f.ui, f.select_box.select_element());
    press(&f, KeyCode::DownArrow);
    press(&f, KeyCode::Escape);
    assert!(f.selected.borrow().is_empty());
    assert_eq!(f.select_box.selected_option_text().as_deref(), Some("Apple"));
}

#[test]
fn arrows_skip_disabled_options_and_separators_and_home_end_land_on_enabled_ones() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = vec![
        ISelectOptionItem { is_disabled: true, ..ISelectOptionItem::new("Header") },
        ISelectOptionItem::new("Apple"),
        separator_select_option(),
        ISelectOptionItem { is_disabled: true, ..ISelectOptionItem::new("Banana") },
        ISelectOptionItem::new("Cherry"),
        ISelectOptionItem { is_disabled: true, ..ISelectOptionItem::new("Footer") },
    ];
    let f = fixture(&guard, options, 1, ISelectBoxOptions::default());
    click(&f.ui, f.select_box.select_element());
    press(&f, KeyCode::DownArrow);
    assert_eq!(focused_row_text(&f).as_deref(), Some("Cherry"), "pula separador e desabilitada");
    press(&f, KeyCode::DownArrow);
    assert_eq!(focused_row_text(&f).as_deref(), Some("Cherry"), "só sobra o desabilitado do fim");
    press(&f, KeyCode::UpArrow);
    assert_eq!(focused_row_text(&f).as_deref(), Some("Apple"));
    press(&f, KeyCode::End);
    assert_eq!(focused_row_text(&f).as_deref(), Some("Cherry"));
    press(&f, KeyCode::Home);
    assert_eq!(focused_row_text(&f).as_deref(), Some("Apple"));
}

#[test]
fn typing_a_letter_focuses_the_next_option_that_starts_with_it() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&guard, fruits(), 0, ISelectBoxOptions::default());
    click(&f.ui, f.select_box.select_element());
    press(&f, KeyCode::KeyC);
    assert_eq!(focused_row_text(&f).as_deref(), Some("Cherry"));
    press(&f, KeyCode::KeyZ);
    assert_eq!(focused_row_text(&f).as_deref(), Some("Cherry"), "sem correspondência nada muda");
}

#[test]
fn hovering_focuses_an_enabled_row_and_a_pointer_up_selects_it() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut options = fruits();
    options.push(ISelectOptionItem::new("Date"));
    let f = fixture(&guard, options, 0, ISelectBoxOptions::default());
    click(&f.ui, f.select_box.select_element());
    let (x, y) = row_center(&f, 2);
    pointer(&f, PointerInputKind::Move, x, y);
    assert_eq!(focused_row_text(&f).as_deref(), Some("Cherry"));
    pointer(&f, PointerInputKind::Down, x, y);
    pointer(&f, PointerInputKind::Up, x, y);
    assert_eq!(*f.selected.borrow(), vec![ISelectData { selected: "Cherry".to_owned(), index: 2 }]);
    assert_eq!(f.select_box.aria_expanded(), Some(false));
    assert_eq!(f.select_box.selected_option_text().as_deref(), Some("Cherry"));
}

#[test]
fn a_pointer_up_on_a_disabled_row_is_ignored_and_hover_does_not_focus_it() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut options = fruits();
    options[1].is_disabled = true;
    options.push(ISelectOptionItem::new("Date"));
    let f = fixture(&guard, options, 0, ISelectBoxOptions::default());
    click(&f.ui, f.select_box.select_element());
    let (x, y) = row_center(&f, 1);
    pointer(&f, PointerInputKind::Move, x, y);
    assert_eq!(focused_row_text(&f).as_deref(), Some("Apple"), "a linha desabilitada não toma o foco");
    pointer(&f, PointerInputKind::Down, x, y);
    pointer(&f, PointerInputKind::Up, x, y);
    assert!(f.selected.borrow().is_empty());
    assert_eq!(f.select_box.aria_expanded(), Some(true), "continua aberta");
}

#[test]
fn losing_focus_closes_the_list_and_restores_the_selection() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&guard, fruits(), 0, ISelectBoxOptions::default());
    click(&f.ui, f.select_box.select_element());
    press(&f, KeyCode::DownArrow);
    let root = f.ui.root();
    let other = create_el(&mut f.ui.tree_mut(), root, Role::Generic, "other");
    f.ui.focus(other);
    assert_eq!(f.select_box.aria_expanded(), Some(false));
    assert!(f.selected.borrow().is_empty());
    assert_eq!(f.select_box.selected_option_text().as_deref(), Some("Apple"));
}

#[test]
fn a_disabled_select_ignores_clicks_and_keys() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&guard, fruits(), 0, ISelectBoxOptions::default());
    f.select_box.set_enabled(false);
    click(&f.ui, f.select_box.select_element());
    assert_eq!(f.select_box.aria_expanded(), None);
    f.select_box.focus();
    assert_ne!(f.ui.focused(), Some(f.select_box.select_element()));
    f.select_box.set_enabled(true);
    f.select_box.focus();
    assert_eq!(f.ui.focused(), Some(f.select_box.select_element()));
    press(&f, KeyCode::Enter);
    assert_eq!(f.select_box.aria_expanded(), Some(true), "Enter abre a lista");
}

#[test]
fn arrows_on_the_closed_select_change_the_selection_skipping_disabled_and_alt_down_opens() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut options = fruits();
    options[1].is_disabled = true;
    let f = fixture(&guard, options, 0, ISelectBoxOptions::default());
    f.select_box.focus();
    press(&f, KeyCode::DownArrow);
    assert_eq!(*f.selected.borrow(), vec![ISelectData { selected: "Cherry".to_owned(), index: 2 }]);
    assert_eq!(f.select_box.selected_index(), 2);
    press(&f, KeyCode::UpArrow);
    assert_eq!(f.select_box.selected_option_text().as_deref(), Some("Apple"));
    assert_eq!(f.select_box.aria_expanded(), None, "setas sem Alt não abrem");
    f.ui.key_input(true, KeyData { key_code: KeyCode::DownArrow, modifiers: Modifiers { alt: true, ..Modifiers::default() } });
    assert_eq!(f.select_box.aria_expanded(), Some(true));
}

#[test]
fn set_options_replaces_the_list_and_select_clamps_to_the_last_option() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = fixture(&guard, fruits(), 0, ISelectBoxOptions::default());
    f.select_box.select(99);
    assert_eq!(f.select_box.selected_index(), 2);
    f.select_box.set_options(vec![ISelectOptionItem::new("One"), ISelectOptionItem::new("Two")], Some(1));
    assert_eq!(f.select_box.selected_option_text().as_deref(), Some("Two"));
    f.select_box.set_options(Vec::new(), None);
    assert_eq!(f.select_box.selected_index(), -1);
    assert_eq!(f.select_box.selected_option_text(), None);
    assert!(f.selected.borrow().is_empty(), "set_options e select não disparam onDidSelect");
}

#[test]
fn detail_and_decorator_right_are_laid_out_after_the_text_and_at_the_right_edge() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = vec![ISelectOptionItem { detail: Some("detalhe".to_owned()), decorator_right: Some("Ctrl+K".to_owned()), ..ISelectOptionItem::new("Apple") }, ISelectOptionItem::new("Banana")];
    let f = fixture(&guard, options, 0, ISelectBoxOptions::default());
    click(&f.ui, f.select_box.select_element());
    let tree = f.ui.tree();
    let rows: Vec<NodeId> = tree.subtree(f.select_box.drop_down_node()).into_iter().filter(|n| tree.has_class(*n, "monaco-list-row")).collect();
    let row = rows[0];
    let (text, detail, decorator) = (tree.children_with_class(row, "option-text")[0], tree.children_with_class(row, "option-detail")[0], tree.children_with_class(row, "option-decorator-right")[0]);
    assert_eq!(tree.bounds(text).x0, 8.0, "2px da linha mais 6px do trecho");
    assert_eq!(tree.bounds(detail).x0, tree.bounds(text).x1 + 6.0);
    assert_eq!(tree.bounds(row).x1 - tree.bounds(decorator).x1, 10.0, "padding-right de 10px");
    let plain = rows[1];
    assert!(!tree.style(tree.children_with_class(plain, "option-detail")[0]).visible);
    assert!(!tree.style(tree.children_with_class(plain, "option-decorator-right")[0]).visible);
}

#[test]
fn a_separator_row_hides_its_text_and_draws_a_line_in_the_middle() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = vec![ISelectOptionItem::new("Apple"), separator_select_option(), ISelectOptionItem::new("Banana")];
    let f = fixture(&guard, options, 0, ISelectBoxOptions::default());
    click(&f.ui, f.select_box.select_element());
    let tree = f.ui.tree();
    let row = tree.subtree(f.select_box.drop_down_node()).into_iter().find(|n| tree.has_class(*n, "option-separator")).expect("separador");
    assert!(tree.has_class(row, "option-disabled"));
    assert!(!tree.style(tree.children_with_class(row, "option-text")[0]).visible);
    let line = tree.children_with_class(row, "option-separator-line")[0];
    assert!(tree.style(line).visible);
    let (line_bounds, row_bounds) = (tree.bounds(line), tree.bounds(row));
    assert_eq!((line_bounds.y0, line_bounds.height(), line_bounds.width()), (11.0, 1.0, row_bounds.width()));
}

#[test]
fn the_details_pane_follows_the_focused_option_description() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = vec![
        ISelectOptionItem { description: Some("Primeira descrição.".to_owned()), ..ISelectOptionItem::new("One") },
        ISelectOptionItem { description: Some("Segunda descrição bem maior que a primeira, para quebrar em mais de uma linha dentro do painel de detalhes.".to_owned()), ..ISelectOptionItem::new("Two") },
        ISelectOptionItem::new("Three"),
    ];
    let f = fixture(&guard, options, 0, ISelectBoxOptions::default());
    click(&f.ui, f.select_box.select_element());
    let pane_lines = |f: &Fixture| {
        let tree = f.ui.tree();
        let pane = tree.subtree(f.select_box.drop_down_node()).into_iter().find(|n| tree.has_class(*n, "select-box-details-pane")).expect("painel");
        (tree.style(pane).visible, tree.children_with_class(pane, "select-box-details-line").len(), tree.bounds(pane).height())
    };
    let (visible, lines, height) = pane_lines(&f);
    assert!(visible && lines == 1);
    press(&f, KeyCode::DownArrow);
    let (visible, lines_two, height_two) = pane_lines(&f);
    assert!(visible && lines_two > 1 && height_two > height, "a segunda descrição quebra em mais linhas");
    press(&f, KeyCode::DownArrow);
    assert!(!pane_lines(&f).0, "sem descrição o painel some");
    // O painel reserva a altura da maior descrição: a caixa não muda de tamanho ao navegar.
    let popup_height = f.ui.tree().bounds(f.select_box.drop_down_node()).height();
    press(&f, KeyCode::UpArrow);
    assert!(f.ui.tree().bounds(f.select_box.drop_down_node()).height() >= popup_height);
}

#[test]
fn page_down_lands_on_an_enabled_option_after_the_scroll_settles() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options: Vec<ISelectOptionItem> = (0..30).map(|i| ISelectOptionItem::new(&format!("Option {i}"))).collect();
    let f = fixture(&guard, options, 0, ISelectBoxOptions { max_visible_options: Some(6), ..Default::default() });
    click(&f.ui, f.select_box.select_element());
    support::run_with_faked_timers(async move {
        press(&f, KeyCode::PageDown);
        // `onPageDown` espera a rolagem assentar (`setTimeout(..., 1)`).
        let _ = support::timeout(2).await;
        assert!(f.select_box.selected_index() > 0, "a seleção acompanhou a página");
        assert_eq!(focused_row_text(&f).as_deref(), f.select_box.selected_option_text().as_deref());
    });
}

#[test]
fn select_action_view_item_runs_the_action_with_the_chosen_option_text() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let root = ui.root();
    let context_view = guard.add_rc(ContextView::new(&ui, root, ContextViewDOMPosition::Absolute));
    let provider: Rc<dyn IContextViewProvider> = context_view;
    let contexts = Rc::new(RefCell::new(Vec::<String>::new()));
    let sink = contexts.clone();
    let action = guard.add_rc(Action::new(
        "pick",
        "Escolher",
        "",
        true,
        Some(Rc::new(move |ctx| {
            if let Dynamic::String(text) = ctx {
                sink.borrow_mut().push(text.to_string_lossy());
            }
            Dynamic::Undefined.into()
        })),
    ));
    let item = guard.add_rc(SelectActionViewItem::new(&ui, None, action.clone() as ActionRef, fruits(), 0, provider, unthemed_select_box_styles(), None));
    let li = create_el(&mut ui.tree_mut(), root, Role::Generic, "action-item");
    item.render(li);
    let (width, height) = item.size(false);
    ui.tree_mut().set_bounds(li, Rect::new(10.0, 10.0, 10.0 + width, 10.0 + height));
    item.layout(false, width, height);
    assert!(width >= 100.0, "min-width: 100px");
    assert_eq!(item.select_box().tab_index(), -1, "setFocusable(false) no construtor");
    click(&ui, item.select_box().select_element());
    ui.key_input(true, key(KeyCode::DownArrow));
    ui.key_input(true, key(KeyCode::Enter));
    assert_eq!(*contexts.borrow(), vec!["Banana".to_owned()]);
    item.select(0);
    assert_eq!(item.select_box().selected_option_text().as_deref(), Some("Apple"));
    item.dispose();
}
