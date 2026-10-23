//! Porte de `src/vs/base/test/browser/ui/selectBox/selectBox.test.ts` (suíte `SelectBoxList`), caso a caso e na mesma ordem.
//!
//! O arquivo do original tem 13 casos (os `for` geram 7 e 4 testes); a suíte descrita na tarefa falava em 4.
//!
//! Divergências: o `je-ui` não tem `position`, `getComputedStyle` nem `getBoundingClientRect`. O `document.body` é a
//! raiz da árvore com 1024x768; `top: 50%` e `calc(100% - 100px)` são calculados sobre os 768px da janela, e o
//! contêiner de `position: fixed` é um nó com os limites explícitos (largura e altura do `<select>`, ou 150px de
//! largura onde o original fixa `width: 150px`). `select.click()` é um evento `Click` no nó do `<select>`
//! (`SelectBoxList::select_element`); `getAttribute('aria-expanded')` é `aria_expanded()` (`'true'`/`'false'`
//! como `Some(true)`/`Some(false)`); `select.selectedIndex` é `selected_index()`; `selectedOptions[0].text` é
//! `selected_option_text()`; `document.activeElement` é `ui.focused()`. `dispatchEvent(keydown)` no elemento
//! ativo é `ui.key_input`; no `list` é um `KeyDown` despachado no nó do `[role="listbox"]` (papel `ListBox`).
//! `clientHeight` é a altura do nó (a borda da caixa é 0 com `unthemedSelectBoxStyles`). `style.display` do
//! painel de detalhes é `'none'` quando `!style.visible` e `'block'` quando visível.

mod select_box_support;
mod support;

use je_base::key_codes::KeyCode;
use je_base::layout::AnchorPosition;
use je_ui::{Content, Event, EventKind};
use je_widgets::select_box::{ISelectBoxDelegate, ISelectBoxOptions, ISelectOptionItem};
use select_box_support::{TestContextViewProvider, WINDOW_HEIGHT, click, fixed_container, key, list_box, new_select_box, new_ui, numbered_options, set_top};
use support::{ensure_no_disposables_are_leaked_in_test_suite, query_all};

#[test]
fn defaults_to_below() {
    anchor_position_case(None, TopSpec::Percent50, 10, AnchorPosition::Below);
}

#[test]
fn honors_below_preference() {
    anchor_position_case(Some(AnchorPosition::Below), TopSpec::Percent50, 10, AnchorPosition::Below);
}

#[test]
fn honors_above_preference() {
    anchor_position_case(Some(AnchorPosition::Above), TopSpec::Percent50, 10, AnchorPosition::Above);
}

#[test]
fn flips_above_when_space_below_is_limited() {
    anchor_position_case(None, TopSpec::BottomMinus100, 10, AnchorPosition::Above);
}

#[test]
fn flips_below_when_space_above_is_limited() {
    anchor_position_case(Some(AnchorPosition::Above), TopSpec::Px30, 10, AnchorPosition::Below);
}

#[test]
fn stays_above_when_a_short_list_fits() {
    anchor_position_case(Some(AnchorPosition::Above), TopSpec::Px30, 1, AnchorPosition::Above);
}

#[test]
fn stays_below_when_a_short_list_fits() {
    anchor_position_case(None, TopSpec::BottomMinus100, 1, AnchorPosition::Below);
}

#[derive(Clone, Copy)]
enum TopSpec {
    Percent50,
    BottomMinus100,
    Px30,
}

impl TopSpec {
    fn px(self) -> f64 {
        match self {
            Self::Percent50 => WINDOW_HEIGHT * 0.5,
            Self::BottomMinus100 => WINDOW_HEIGHT - 100.0,
            Self::Px30 => 30.0,
        }
    }
}

/// O corpo do `for` dos sete primeiros casos.
fn anchor_position_case(anchor_position: Option<AnchorPosition>, top: TopSpec, count: usize, expected: AnchorPosition) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let provider = guard.add_rc(TestContextViewProvider::new(&ui));
    let select_box = new_select_box(&guard, &ui, &provider, numbered_options(count), ISelectBoxOptions { anchor_position, ..Default::default() });
    let container = fixed_container(&guard, &ui, &select_box, top.px(), None);
    select_box.render(container);
    let select = select_box.select_element();
    click(&ui, select);
    let initial_position = provider.anchor_position();
    let expanded = select_box.aria_expanded();
    ui.key_input(true, key(KeyCode::Escape));
    let closed = (select_box.aria_expanded(), ui.focused() == Some(select), select_box.selected_index());
    let (width, height) = (ui.tree().bounds(container).width(), ui.tree().bounds(container).height());
    set_top(&ui, container, WINDOW_HEIGHT * 0.5, width, height);
    click(&ui, select);
    assert_eq!(
        (initial_position, expanded, closed, provider.anchor_position()),
        (Some(expected), Some(true), (Some(false), true, 0), Some(anchor_position.unwrap_or(AnchorPosition::Below))),
    );
}

#[test]
fn hides_disabled_options_from_the_custom_dropdown_while_retaining_the_closed_value() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let options = vec![
        ISelectOptionItem { is_disabled: true, ..ISelectOptionItem::new("Pick an option") },
        ISelectOptionItem { description: Some("Do not show external sessions.".to_owned()), ..ISelectOptionItem::new("None") },
        ISelectOptionItem { description: Some("Show all external sessions.".to_owned()), ..ISelectOptionItem::new("All") },
    ];
    let provider = guard.add_rc(TestContextViewProvider::new(&ui));
    let select_box = new_select_box(&guard, &ui, &provider, options, ISelectBoxOptions { hide_disabled_options: true, show_option_description_hovers: true, ..Default::default() });
    let container = fixed_container(&guard, &ui, &select_box, 100.0, None);
    select_box.render(container);
    let select = select_box.select_element();
    let closed_text = select_box.selected_option_text();
    click(&ui, select);
    let open_text = select_box.selected_option_text();
    let option_texts: Vec<String> = {
        let tree = ui.tree();
        query_all(&tree, provider.container, &["option-text"]).into_iter().map(|n| (n, tree.style(n).content.clone())).map(|(_, c)| if let Content::Text { text, .. } = c { text } else { String::new() }).collect()
    };
    let details_display = {
        let tree = ui.tree();
        let pane = query_all(&tree, provider.container, &["select-box-details-pane"])[0];
        if tree.style(pane).visible { "block" } else { "none" }
    };
    click(&ui, select);

    assert_eq!(
        (closed_text, open_text, option_texts, details_display, select_box.selected_option_text()),
        (
            Some("Pick an option".to_owned()),
            Some("None".to_owned()),
            vec!["None".to_owned(), "All".to_owned()],
            "none",
            Some("Pick an option".to_owned()),
        ),
    );
}

#[test]
fn caps_upward_lists_at_six_options() {
    capped_list_case(TopSpec::Percent50, AnchorPosition::Above, 96, 132.0, AnchorPosition::Above);
}

#[test]
fn caps_downward_lists_at_six_options() {
    capped_list_case(TopSpec::Percent50, AnchorPosition::Below, 96, 132.0, AnchorPosition::Below);
}

#[test]
fn keeps_the_cap_when_falling_back_within_the_viewport() {
    capped_list_case(TopSpec::Px30, AnchorPosition::Above, 96, 132.0, AnchorPosition::Below);
}

#[test]
fn does_not_pad_short_lists_to_the_cap() {
    capped_list_case(TopSpec::Percent50, AnchorPosition::Above, 4, 88.0, AnchorPosition::Above);
}

/// O corpo do `for` dos quatro casos de altura limitada.
fn capped_list_case(top: TopSpec, anchor_position: AnchorPosition, count: usize, expected_height: f64, expected_position: AnchorPosition) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let provider = guard.add_rc(TestContextViewProvider::new(&ui));
    let select_box = new_select_box(&guard, &ui, &provider, numbered_options(count), ISelectBoxOptions { anchor_position: Some(anchor_position), max_visible_options: Some(6), ..Default::default() });
    let container = fixed_container(&guard, &ui, &select_box, top.px(), Some(150.0));
    select_box.render(container);
    let select = select_box.select_element();
    click(&ui, select);
    let position = provider.anchor_position();
    let list = list_box(&ui, &provider);
    let popup = query_all(&ui.tree(), provider.container, &["monaco-select-box-dropdown-container"])[0];
    let sizes = (ui.tree().bounds(list).height(), ui.tree().bounds(popup).height());
    ui.dispatch(&mut Event::new(EventKind::KeyDown, list).with_key(key(KeyCode::End)));
    let (row_bounds, list_bounds) = {
        let tree = ui.tree();
        let focused_row = tree.subtree(list).into_iter().find(|n| tree.has_class(*n, "monaco-list-row") && tree.has_class(*n, "focused")).expect("linha focada");
        (tree.absolute_bounds(focused_row), tree.absolute_bounds(list))
    };
    let last_option_visible = row_bounds.y0 >= list_bounds.y0 && row_bounds.y1 <= list_bounds.y1;
    ui.dispatch(&mut Event::new(EventKind::KeyDown, list).with_key(key(KeyCode::Enter)));
    assert_eq!(
        (position, sizes, last_option_visible, select_box.selected_index(), select_box.aria_expanded(), ui.focused() == Some(select)),
        (Some(expected_position), (expected_height, expected_height), true, count as i32 - 1, Some(false), true),
    );
}

#[test]
fn passes_the_requested_context_view_layer_to_the_dropdown() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let provider = guard.add_rc(TestContextViewProvider::new(&ui));
    let options = vec![ISelectOptionItem::new("One"), ISelectOptionItem::new("Two")];
    let select_box = new_select_box(&guard, &ui, &provider, options, ISelectBoxOptions { context_view_layer: Some(1), ..Default::default() });
    let container = fixed_container(&guard, &ui, &select_box, 0.0, None);
    select_box.render(container);
    click(&ui, select_box.select_element());

    assert_eq!(provider.active_layer(), Some(1));
}
