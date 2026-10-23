//! Testes extras do `InputBox` e do `HistoryInputBox` (`src/input_box.rs`). O VS Code 1.143.1 não tem teste
//! dedicado do `inputBox.ts` (o `findInput` o exercita), então estes casos fixam o comportamento portado:
//! digitar, seleção, palavra, colar, desfazer, validação com mensagem, texto-guia, senha, histórico,
//! cursor por clique, UTF-16 com emoji, foco, estilo e piscar do cursor.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::Event as BaseEvent;
use je_base::history::HistorySet;
use je_base::key_codes::KeyCode;
use je_base::runtime;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Border, Content, Cursor, EventKind, KeyData, Modifiers, NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::input_box::{
    HistoryInputBox, HistoryInputBoxOptions, InlineContextViewProvider, InputBox, InputBoxOptions, InputBoxStyles, InputType, InputValidationOptions, Message, MessageType, Range, unthemed_input_box_styles,
};
use je_widgets::text::{UI_FONT_SIZE, measure_text, ui_font};
use je_widgets::text_field::MemoryClipboard;
use support::{ensure_no_disposables_are_leaked_in_test_suite, LeakGuard};

const FOCUS_BORDER: Rgba = Rgba::opaque(0x00, 0x7f, 0xd4);
const INPUT_BORDER: Rgba = Rgba::opaque(0x55, 0x55, 0x55);
const LINE_HEIGHT: f64 = 18.1875;
const SELECTION: Rgba = Rgba::opaque(0x26, 0x4f, 0x78);

fn styles() -> InputBoxStyles {
    InputBoxStyles {
        input_border: Some(INPUT_BORDER),
        input_placeholder_foreground: Some(Rgba::opaque(0xa0, 0xa0, 0xa0)),
        focus_border: Some(FOCUS_BORDER),
        selection_background: Some(SELECTION),
        input_validation_error_foreground: Some(Rgba::opaque(0xff, 0xff, 0xff)),
        ..unthemed_input_box_styles()
    }
}

fn options() -> InputBoxOptions {
    let mut options = InputBoxOptions::new(styles());
    options.caret_blink = false;
    options
}

/// A janela de 400x200 com um contêiner de `width` px.
fn setup(width: f64) -> (Ui, NodeId) {
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 400.0, 300.0));
        let container = tree.create(root, Role::Generic);
        tree.set_bounds(container, Rect::new(10.0, 10.0, 10.0 + width, 150.0));
        container
    };
    (ui, container)
}

fn new_box(store: &LeakGuard, ui: &Ui, container: NodeId, options: InputBoxOptions) -> Rc<InputBox> {
    store.add_rc(InputBox::new(ui, container, None, options))
}

/// O primeiro nó com a cadeia de classes `chain`, cada uma filha direta da anterior, abaixo de `from`.
fn find(ui: &Ui, from: NodeId, chain: &[&str]) -> NodeId {
    let tree = ui.tree();
    let mut node = from;
    for class in chain {
        node = *tree.children_with_class(node, class).first().unwrap_or_else(|| panic!("sem .{class}"));
    }
    node
}

fn editor(ui: &Ui, input_box: &InputBox, class: &str) -> NodeId {
    find(ui, input_box.input_element(), &["inner-editor", class])
}

/// O texto pintado no nó `text` do campo.
fn painted_text(ui: &Ui, input_box: &InputBox) -> Option<String> {
    match &ui.tree().style(editor(ui, input_box, "text")).content {
        Content::Text { text, .. } => Some(text.clone()),
        _ => None,
    }
}

fn placeholder_text(ui: &Ui, input_box: &InputBox) -> Option<String> {
    let node = editor(ui, input_box, "placeholder");
    let tree = ui.tree();
    match &tree.style(node).content {
        Content::Text { text, .. } if tree.style(node).visible => Some(text.clone()),
        _ => None,
    }
}

fn visible(ui: &Ui, input_box: &InputBox, class: &str) -> bool {
    ui.tree().style(editor(ui, input_box, class)).visible
}

fn element_border(ui: &Ui, input_box: &InputBox) -> Option<Border> {
    ui.tree().style(input_box.element()).border
}

fn key(ui: &Ui, code: KeyCode, modifiers: Modifiers) -> bool {
    ui.key_input(true, KeyData { key_code: code, modifiers })
}

fn press(ui: &Ui, code: KeyCode) -> bool {
    key(ui, code, Modifiers::default())
}

fn ctrl(ui: &Ui, code: KeyCode) -> bool {
    key(ui, code, Modifiers { ctrl: true, ..Modifiers::default() })
}

fn shift(ui: &Ui, code: KeyCode) -> bool {
    key(ui, code, Modifiers { shift: true, ..Modifiers::default() })
}

fn type_text(ui: &Ui, text: &str) {
    for c in text.chars() {
        ui.text_input(&c.to_string());
    }
}

fn collect<T: Clone + 'static>(guard: &LeakGuard, event: BaseEvent<T>) -> Rc<RefCell<Vec<T>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    guard.add(event.listen(move |v| sink.borrow_mut().push(v.clone())));
    log
}

/// O ponto da janela para o deslocamento `offset` do texto do campo (a meia altura da linha).
fn point_at(ui: &Ui, input_box: &InputBox, offset: usize) -> (f64, f64) {
    let inner = find(ui, input_box.input_element(), &["inner-editor"]);
    let bounds = ui.tree().absolute_bounds(inner);
    let x = input_box.text_field().x_at_offset(offset, &ui_font(UI_FONT_SIZE, 400));
    (bounds.x0 + x, bounds.y0 + bounds.height() / 2.0)
}

fn click(ui: &Ui, at: (f64, f64), count: u32) {
    ui.pointer_input(PointerInput { click_count: count, ..PointerInput::new(PointerInputKind::Down, at.0, at.1) });
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, at.0, at.1));
}

#[test]
fn builds_the_dom_structure_with_the_original_classes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());

    let tree = ui.tree();
    assert!(tree.has_class(input_box.element(), "monaco-inputbox"));
    assert!(tree.has_class(input_box.element(), "idle"));
    assert_eq!(tree.parent(input_box.element()), Some(container));
    let wrapper = tree.children_with_class(input_box.element(), "ibwrapper")[0];
    assert_eq!(tree.children_with_class(wrapper, "input"), [input_box.input_element()]);
    assert!(tree.has_class(input_box.input_element(), "empty"));
    assert_eq!(tree.role(input_box.input_element()), Role::TextInput);
    assert_eq!(tree.cursor(input_box.input_element()), Cursor::Text);
}

#[test]
fn height_is_two_borders_two_paddings_and_the_workbench_line_height() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());

    assert_eq!(input_box.height(), 2.0 + 8.0 + LINE_HEIGHT);
    assert_eq!(ui.tree().bounds(input_box.element()).width(), 200.0);
    // O `<input>` ocupa a largura do `ibwrapper`, que desconta as duas bordas.
    assert_eq!(input_box.width(), 198.0);
}

#[test]
fn paints_background_radius_and_a_transparent_border_when_none_is_set() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let mut options = options();
    options.input_box_styles.input_border = None;
    let input_box = new_box(&store, &ui, container, options);

    let tree = ui.tree();
    let style = tree.style(input_box.element());
    assert_eq!(style.background, Some(Rgba::opaque(0x3c, 0x3c, 0x3c)));
    assert_eq!(style.radius, 4.0);
    assert_eq!(style.border, Some(Border { width: 1.0, color: Rgba::TRANSPARENT }));
}

#[test]
fn typing_inserts_text_and_fires_change_and_input() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    let changes = collect(&store, input_box.on_did_change());
    let inputs = collect(&store, input_box.on_input());
    input_box.focus();

    type_text(&ui, "abc");

    assert_eq!(input_box.value(), "abc");
    assert_eq!(*changes.borrow(), ["a", "ab", "abc"]);
    assert_eq!(inputs.borrow().len(), 3);
    assert_eq!(painted_text(&ui, &input_box).as_deref(), Some("abc"));
    assert!(!ui.tree().has_class(input_box.input_element(), "empty"));
}

#[test]
fn set_value_fires_change_but_not_input_and_ignores_the_same_value() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    let changes = collect(&store, input_box.on_did_change());
    let inputs = collect(&store, input_box.on_input());

    input_box.set_value("x");
    input_box.set_value("x");
    input_box.set_value("");

    assert_eq!(*changes.borrow(), ["x", ""]);
    assert!(inputs.borrow().is_empty());
    assert!(ui.tree().has_class(input_box.input_element(), "empty"));
}

#[test]
fn text_input_is_ignored_without_focus_on_the_field() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());

    ui.text_input("lost");

    assert_eq!(input_box.value(), "");
}

#[test]
fn select_all_then_typing_replaces_everything() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("hello");
    input_box.focus();

    assert!(!ctrl(&ui, KeyCode::KeyA));
    ui.text_input("X");

    assert_eq!(input_box.value(), "X");
}

#[test]
fn shift_arrows_select_and_the_selection_is_painted_only_with_focus() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("hello world");
    input_box.focus();

    shift(&ui, KeyCode::LeftArrow);
    shift(&ui, KeyCode::LeftArrow);

    assert_eq!(input_box.get_selection(), Some(Range { start: 9, end: 11 }));
    let selection = editor(&ui, &input_box, "selection");
    {
        let tree = ui.tree();
        assert!(tree.style(selection).visible);
        assert_eq!(tree.style(selection).background, Some(SELECTION));
        assert!(!tree.style(editor(&ui, &input_box, "caret")).visible);
        let (x0, x1) = {
            let field = input_box.text_field();
            (field.x_at_offset(9, &ui_font(UI_FONT_SIZE, 400)), field.x_at_offset(11, &ui_font(UI_FONT_SIZE, 400)))
        };
        let bounds = tree.bounds(selection);
        assert_eq!((bounds.x0, bounds.x1, bounds.height()), (x0, x1, LINE_HEIGHT));
    }

    input_box.blur();
    assert!(!visible(&ui, &input_box, "selection"));
    // A seleção continua guardada: só não é pintada.
    assert_eq!(input_box.get_selection(), Some(Range { start: 9, end: 11 }));
}

#[test]
fn the_default_selection_color_is_the_chromium_blue() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let mut options = options();
    options.input_box_styles.selection_background = None;
    let input_box = new_box(&store, &ui, container, options);
    input_box.set_value("abc");
    input_box.focus();
    ctrl(&ui, KeyCode::KeyA);

    assert_eq!(ui.tree().style(editor(&ui, &input_box, "selection")).background, Some(Rgba::opaque(0x1e, 0x90, 0xff)));
}

#[test]
fn ctrl_backspace_deletes_a_word_and_backspace_a_character() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("foo bar baz");
    input_box.focus();

    ctrl(&ui, KeyCode::Backspace);
    assert_eq!(input_box.value(), "foo bar ");
    press(&ui, KeyCode::Backspace);
    assert_eq!(input_box.value(), "foo bar");
    press(&ui, KeyCode::Home);
    press(&ui, KeyCode::Delete);
    assert_eq!(input_box.value(), "oo bar");
}

#[test]
fn a_listener_that_prevents_default_stops_the_edit() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("abc");
    input_box.focus();
    store.add(ui.listen(input_box.input_element(), EventKind::KeyDown, false, |e| e.prevent_default()));

    press(&ui, KeyCode::Backspace);

    assert_eq!(input_box.value(), "abc");
}

#[test]
fn stop_propagation_alone_does_not_stop_the_edit() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("abc");
    input_box.focus();
    store.add(ui.listen(input_box.input_element(), EventKind::KeyDown, false, |e| e.stop_propagation()));

    press(&ui, KeyCode::Backspace);

    assert_eq!(input_box.value(), "ab");
}

#[test]
fn key_events_reach_the_public_events_with_the_key() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    let downs = collect(&store, input_box.on_key_down());
    let ups = collect(&store, input_box.on_key_up());
    input_box.focus();

    press(&ui, KeyCode::Enter);
    ui.key_input(false, KeyData { key_code: KeyCode::Enter, modifiers: Modifiers::default() });

    assert_eq!(downs.borrow().iter().map(|k| k.key_code).collect::<Vec<_>>(), [KeyCode::Enter]);
    assert_eq!(ups.borrow().len(), 1);
    // Enter não é do campo: continua livre para o consumidor.
    assert_eq!(input_box.value(), "");
}

#[test]
fn paste_comes_from_the_clipboard_and_cut_copy_go_to_it() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let clipboard = MemoryClipboard::with_text("pasted");
    let mut options = options();
    options.clipboard = Some(clipboard.clone());
    let input_box = new_box(&store, &ui, container, options);
    input_box.focus();

    ctrl(&ui, KeyCode::KeyV);
    assert_eq!(input_box.value(), "pasted");
    ctrl(&ui, KeyCode::KeyA);
    ctrl(&ui, KeyCode::KeyX);
    assert_eq!(input_box.value(), "");
    ctrl(&ui, KeyCode::KeyV);
    assert_eq!(input_box.value(), "pasted");
}

#[test]
fn undo_and_redo_with_ctrl_z_and_ctrl_y() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    let changes = collect(&store, input_box.on_did_change());
    input_box.focus();
    type_text(&ui, "hello");

    ctrl(&ui, KeyCode::KeyZ);
    assert_eq!(input_box.value(), "");
    ctrl(&ui, KeyCode::KeyY);
    assert_eq!(input_box.value(), "hello");
    assert_eq!(changes.borrow().last().map(String::as_str), Some("hello"));
}

#[test]
fn emoji_are_one_character_for_the_arrows_and_two_units_for_the_selection() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.focus();
    type_text(&ui, "a");
    ui.text_input("\u{1F44D}");

    assert_eq!(input_box.get_selection(), Some(Range { start: 3, end: 3 }));
    shift(&ui, KeyCode::LeftArrow);
    assert_eq!(input_box.get_selection(), Some(Range { start: 1, end: 3 }));
    press(&ui, KeyCode::Backspace);
    assert_eq!(input_box.value(), "a");
}

#[test]
fn clicking_puts_the_caret_at_the_nearest_boundary() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("hello world");

    let target = point_at(&ui, &input_box, 3);
    click(&ui, target, 1);

    assert!(input_box.has_focus());
    assert_eq!(input_box.get_selection(), Some(Range { start: 3, end: 3 }));
    ui.text_input("_");
    assert_eq!(input_box.value(), "hel_lo world");
}

#[test]
fn clicking_between_two_characters_picks_the_closer_one() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("abcdef");
    let (x2, y) = point_at(&ui, &input_box, 2);
    let (x3, _) = point_at(&ui, &input_box, 3);

    click(&ui, (x2 + (x3 - x2) * 0.3, y), 1);
    assert_eq!(input_box.get_selection(), Some(Range { start: 2, end: 2 }));
    click(&ui, (x2 + (x3 - x2) * 0.7, y), 1);
    assert_eq!(input_box.get_selection(), Some(Range { start: 3, end: 3 }));
}

#[test]
fn double_click_selects_a_word_and_triple_click_everything() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("foo bar baz");

    click(&ui, point_at(&ui, &input_box, 5), 2);
    assert_eq!(input_box.get_selection(), Some(Range { start: 4, end: 7 }));
    click(&ui, point_at(&ui, &input_box, 5), 3);
    assert_eq!(input_box.get_selection(), Some(Range { start: 0, end: 11 }));
}

#[test]
fn dragging_with_the_mouse_selects_even_outside_the_field() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("hello world");
    let start = point_at(&ui, &input_box, 2);
    let end = point_at(&ui, &input_box, 7);

    ui.pointer_input(PointerInput::new(PointerInputKind::Down, start.0, start.1));
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, end.0, end.1 + 80.0));
    assert_eq!(input_box.get_selection(), Some(Range { start: 2, end: 7 }));
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, end.0, end.1 + 80.0));
    // Depois de soltar, mover o mouse não muda mais a seleção.
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, start.0, start.1));
    assert_eq!(input_box.get_selection(), Some(Range { start: 2, end: 7 }));
}

#[test]
fn long_text_scrolls_to_keep_the_caret_visible() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(120.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.focus();

    type_text(&ui, "the quick brown fox jumps over the lazy dog");

    let text = editor(&ui, &input_box, "text");
    let inner = find(&ui, input_box.input_element(), &["inner-editor"]);
    let tree = ui.tree();
    let (text_bounds, inner_bounds) = (tree.bounds(text), tree.bounds(inner));
    assert!(text_bounds.x0 < 0.0);
    // O fim do texto cabe na janela de texto (a caixa de conteúdo, sem o padding).
    assert!((text_bounds.x1 - inner_bounds.width()).abs() < 1e-9, "{text_bounds:?} {inner_bounds:?}");
    assert_eq!(inner_bounds.width(), 120.0 - 2.0 - 12.0);
    assert!(tree.style(inner).clip);
}

#[test]
fn unfocused_long_text_gets_an_ellipsis_and_no_scroll() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(120.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("the quick brown fox jumps over the lazy dog");

    let shown = painted_text(&ui, &input_box).expect("texto");
    assert!(shown.ends_with('\u{2026}'), "{shown}");
    assert_eq!(ui.tree().bounds(editor(&ui, &input_box, "text")).x0, 0.0);
    assert_eq!(input_box.value(), "the quick brown fox jumps over the lazy dog");
}

#[test]
fn placeholder_shows_while_empty_and_hides_when_typing() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let mut options = options();
    options.placeholder = Some("Search".to_owned());
    let input_box = new_box(&store, &ui, container, options);

    assert_eq!(placeholder_text(&ui, &input_box).as_deref(), Some("Search"));
    let color = match &ui.tree().style(editor(&ui, &input_box, "placeholder")).content {
        Content::Text { color, .. } => *color,
        other => panic!("{other:?}"),
    };
    assert_eq!(color, Rgba::opaque(0xa0, 0xa0, 0xa0));
    input_box.focus();
    assert_eq!(placeholder_text(&ui, &input_box).as_deref(), Some("Search"));
    type_text(&ui, "x");
    assert_eq!(placeholder_text(&ui, &input_box), None);
    assert_eq!(input_box.tooltip(), "Search");
}

#[test]
fn show_placeholder_on_focus_only_shows_it_with_focus() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let mut options = options();
    options.placeholder = Some("Search".to_owned());
    options.show_placeholder_on_focus = true;
    let input_box = new_box(&store, &ui, container, options);

    assert_eq!(placeholder_text(&ui, &input_box), None);
    input_box.focus();
    assert_eq!(placeholder_text(&ui, &input_box).as_deref(), Some("Search"));
    input_box.blur();
    assert_eq!(placeholder_text(&ui, &input_box), None);
}

#[test]
fn set_place_holder_replaces_the_text_and_an_explicit_tooltip_wins() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let mut options = options();
    options.placeholder = Some("A".to_owned());
    options.tooltip = Some("Tip".to_owned());
    let input_box = new_box(&store, &ui, container, options);

    input_box.set_place_holder("B");

    assert_eq!((placeholder_text(&ui, &input_box).as_deref(), input_box.tooltip().as_str()), (Some("B"), "Tip"));
}

#[test]
fn password_masks_the_text_and_keeps_the_real_value() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let mut options = options();
    options.input_type = InputType::Password;
    let clipboard = MemoryClipboard::with_text("keep");
    options.clipboard = Some(clipboard.clone());
    let input_box = new_box(&store, &ui, container, options);
    input_box.focus();

    type_text(&ui, "s3cret");
    ctrl(&ui, KeyCode::KeyA);
    ctrl(&ui, KeyCode::KeyC);

    assert_eq!(input_box.value(), "s3cret");
    assert_eq!(painted_text(&ui, &input_box).as_deref(), Some("\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"));
    assert_eq!(je_widgets::text_field::ClipboardProvider::read_text(&*clipboard).as_deref(), Some("keep"));
}

#[test]
fn disabled_ignores_input_loses_focus_and_shows_the_default_cursor() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("abc");
    input_box.focus();

    input_box.set_enabled(false);
    assert!(!input_box.has_focus());
    ui.text_input("x");
    input_box.focus();
    assert!(!input_box.has_focus());
    click(&ui, point_at(&ui, &input_box, 1), 1);
    assert_eq!(input_box.value(), "abc");
    assert_eq!(ui.tree().cursor(input_box.input_element()), Cursor::Default);

    input_box.set_enabled(true);
    input_box.focus();
    assert!(input_box.has_focus());
    assert_eq!(ui.tree().cursor(input_box.input_element()), Cursor::Text);
}

#[test]
fn focus_adds_synthetic_focus_and_the_focus_border_and_blur_removes_them() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    let focus = collect(&store, input_box.on_did_focus());
    let blur = collect(&store, input_box.on_did_blur());

    assert_eq!(element_border(&ui, &input_box), Some(Border { width: 1.0, color: INPUT_BORDER }));
    input_box.focus();
    assert!(ui.tree().has_class(input_box.element(), "synthetic-focus"));
    assert_eq!(element_border(&ui, &input_box), Some(Border { width: 1.0, color: FOCUS_BORDER }));
    input_box.blur();
    assert!(!ui.tree().has_class(input_box.element(), "synthetic-focus"));
    assert_eq!(element_border(&ui, &input_box), Some(Border { width: 1.0, color: INPUT_BORDER }));
    assert_eq!((focus.borrow().len(), blur.borrow().len()), (1, 1));
}

#[test]
fn caret_is_a_one_pixel_bar_at_the_cursor_only_with_focus() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("abc");
    assert!(!visible(&ui, &input_box, "caret"));

    input_box.focus();
    let caret = editor(&ui, &input_box, "caret");
    let tree = ui.tree();
    assert!(tree.style(caret).visible);
    let bounds = tree.bounds(caret);
    assert_eq!(bounds.width(), 1.0);
    assert_eq!(bounds.x0, measure_text("abc", &ui_font(UI_FONT_SIZE, 400)));
    assert_eq!(tree.style(caret).background, Some(Rgba::opaque(0xcc, 0xcc, 0xcc)));
}

#[test]
fn caret_blinks_every_500_ms_and_stops_on_blur() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let mut options = options();
    options.caret_blink = true;
    let input_box = new_box(&store, &ui, container, options);

    runtime::with_faked_timers(|| {
        input_box.focus();
        assert!(visible(&ui, &input_box, "caret"));
        runtime::advance_fake_time(499);
        assert!(visible(&ui, &input_box, "caret"));
        runtime::advance_fake_time(1);
        assert!(!visible(&ui, &input_box, "caret"));
        runtime::advance_fake_time(500);
        assert!(visible(&ui, &input_box, "caret"));
        // Digitar mostra o cursor e recomeça a contagem.
        runtime::advance_fake_time(500);
        assert!(!visible(&ui, &input_box, "caret"));
        ui.text_input("x");
        assert!(visible(&ui, &input_box, "caret"));
        runtime::advance_fake_time(499);
        assert!(visible(&ui, &input_box, "caret"));
        input_box.blur();
    });
}

#[test]
fn select_with_a_range_sets_the_selection_and_reports_it() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("hello world");
    assert!(input_box.is_selection_at_end());

    input_box.select(None);
    assert_eq!(input_box.get_selection(), Some(Range { start: 0, end: 11 }));
    assert!(!input_box.is_selection_at_end());
    input_box.select(Some(Range { start: 2, end: 4 }));
    assert_eq!(input_box.get_selection(), Some(Range { start: 2, end: 4 }));
}

#[test]
fn insert_at_cursor_inserts_and_leaves_the_caret_one_past_the_start() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    input_box.set_value("hello");
    input_box.select(Some(Range { start: 1, end: 3 }));

    input_box.insert_at_cursor("XYZ");

    assert_eq!(input_box.value(), "hXYZlo");
    assert_eq!(input_box.get_selection(), Some(Range { start: 2, end: 2 }));
}

fn validating(content: &'static str, message_type: MessageType) -> InputBoxOptions {
    let mut options = options();
    options.validation_options = Some(InputValidationOptions {
        validation: Some(Rc::new(move |value: &str| (value == "bad").then(|| Message { content: Some(content.to_owned()), format_content: false, r#type: Some(message_type) }))),
    });
    options
}

#[test]
fn validation_marks_the_box_with_the_message_type_and_its_border() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, validating("Nope", MessageType::Error));

    assert!(input_box.is_input_valid());
    input_box.set_value("bad");

    let tree = ui.tree();
    assert!(tree.has_class(input_box.element(), "error"));
    assert!(!tree.has_class(input_box.element(), "idle"));
    assert_eq!(tree.style(input_box.element()).border, Some(Border { width: 1.0, color: Rgba::opaque(0xbe, 0x11, 0x00) }));
    drop(tree);
    assert!(input_box.is_aria_invalid());
    assert!(!input_box.is_input_valid());
    assert_eq!(input_box.validate(), Some(MessageType::Error));
}

#[test]
fn a_valid_value_clears_the_message_state() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, validating("Nope", MessageType::Warning));
    input_box.set_value("bad");
    assert!(ui.tree().has_class(input_box.element(), "warning"));

    input_box.set_value("good");

    let tree = ui.tree();
    assert!(tree.has_class(input_box.element(), "idle"));
    assert!(!tree.has_class(input_box.element(), "warning"));
    assert_eq!(tree.style(input_box.element()).border, Some(Border { width: 1.0, color: INPUT_BORDER }));
    drop(tree);
    assert!(!input_box.is_aria_invalid());
    assert_eq!(input_box.validate(), None);
}

#[test]
fn info_and_warning_use_their_own_styles() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());

    let info = input_box.styles_for_type(Some(MessageType::Info));
    let warning = input_box.styles_for_type(Some(MessageType::Warning));
    let error = input_box.styles_for_type(Some(MessageType::Error));
    assert_eq!(info.border, Some(Rgba::opaque(0x55, 0xaa, 0xff)));
    assert_eq!(warning.background, Some(Rgba::opaque(0x35, 0x2a, 0x05)));
    assert_eq!(error.foreground, Some(Rgba::opaque(0xff, 0xff, 0xff)));
    // Sem tipo vale o de erro, como o `default` do `switch` original.
    assert_eq!(input_box.styles_for_type(None), error);
}

#[test]
fn the_validation_border_wins_over_the_focus_ring() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, validating("Nope", MessageType::Error));
    input_box.set_value("bad");

    input_box.focus();

    assert_eq!(element_border(&ui, &input_box), Some(Border { width: 1.0, color: Rgba::opaque(0xbe, 0x11, 0x00) }));
}

#[test]
fn the_message_is_shown_by_the_context_view_provider_while_focused() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let provider = store.add_rc(Rc::new(NoDispose(InlineContextViewProvider::new(&ui)))).0.clone();
    let input_box = store.add_rc(InputBox::new(&ui, container, Some(provider.clone()), validating("Nope, try again", MessageType::Error)));
    input_box.set_value("bad");
    assert!(provider.container().is_none(), "sem foco não mostra a mensagem");

    input_box.focus();

    let host = provider.container().expect("visão aberta");
    let block = find(&ui, host, &["monaco-inputbox-container", "monaco-inputbox-message"]);
    let tree = ui.tree();
    assert!(tree.has_class(block, "error"));
    assert_eq!(tree.style(block).background, Some(Rgba::opaque(0x5a, 0x1d, 0x1d)));
    assert_eq!(tree.style(block).border, Some(Border { width: 1.0, color: Rgba::opaque(0xbe, 0x11, 0x00) }));
    // `width: 100%` do contêiner, que acompanha a largura do campo, e `margin-top: -1px`.
    assert_eq!(tree.bounds(block).width(), 200.0);
    assert_eq!(tree.bounds(block).y0, -1.0);
    // Uma linha: 17px de linha, 2 x 4.8px de padding, 2 x 1px de borda.
    assert!((tree.bounds(block).height() - (17.0 + 9.6 + 2.0)).abs() < 1e-9);
    let abs = tree.absolute_bounds(input_box.element());
    assert_eq!((tree.absolute_bounds(host).x0, tree.absolute_bounds(host).y0), (abs.x0, abs.y1));
    drop(tree);

    input_box.blur();
    assert!(provider.container().is_none());
}

/// Só para o `LeakGuard` aceitar o provedor (que não é um disposable).
struct NoDispose(Rc<InlineContextViewProvider>);

impl je_base::lifecycle::IDisposable for NoDispose {
    fn dispose(&self) {}
}

#[test]
fn a_long_message_wraps_and_the_block_grows() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(160.0);
    let provider = InlineContextViewProvider::new(&ui);
    let content = "This is a long validation message that cannot fit in one line of the box";
    let input_box = store.add_rc(InputBox::new(&ui, container, Some(provider.clone()), validating(content, MessageType::Info)));
    input_box.set_value("bad");
    input_box.focus();

    let host = provider.container().expect("visão aberta");
    let block = find(&ui, host, &["monaco-inputbox-container", "monaco-inputbox-message"]);
    let tree = ui.tree();
    let lines = tree.children(block).len();
    assert!(lines >= 3, "{lines}");
    let joined: String = tree.children(block).iter().map(|n| match &tree.style(*n).content {
        Content::Text { text, .. } => text.clone(),
        _ => String::new(),
    }).collect::<Vec<_>>().join(" ");
    assert_eq!(joined.split_whitespace().collect::<Vec<_>>().join(" "), content);
    assert!(tree.bounds(block).height() > 3.0 * 17.0);
    drop(tree);
    input_box.blur();
}

#[test]
fn the_message_follows_the_value_changes_and_hides_when_valid() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let provider = InlineContextViewProvider::new(&ui);
    let input_box = store.add_rc(InputBox::new(&ui, container, Some(provider.clone()), validating("Nope", MessageType::Error)));
    input_box.focus();

    type_text(&ui, "bad");
    assert!(provider.container().is_some());
    ui.text_input("!");
    assert!(provider.container().is_none(), "o valor deixou de ser inválido");
    assert!(ui.tree().has_class(input_box.element(), "idle"));
    input_box.blur();
}

#[test]
fn formatted_content_drops_the_markers_and_bolds_the_text() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300.0);
    let provider = InlineContextViewProvider::new(&ui);
    let mut options = options();
    options.validation_options = Some(InputValidationOptions {
        validation: Some(Rc::new(|_: &str| Some(Message { content: Some("a **bold** and `code` word".to_owned()), format_content: true, r#type: Some(MessageType::Info) }))),
    });
    let input_box = store.add_rc(InputBox::new(&ui, container, Some(provider.clone()), options));
    input_box.set_value("x");
    input_box.focus();

    let host = provider.container().expect("visão aberta");
    let block = find(&ui, host, &["monaco-inputbox-container", "monaco-inputbox-message"]);
    let tree = ui.tree();
    let pieces: Vec<(String, String)> = tree.children(block).iter().map(|n| {
        let text = match &tree.style(*n).content {
            Content::Text { text, .. } => text.clone(),
            _ => String::new(),
        };
        (tree.has_class(*n, "bold").then_some("bold").or(tree.has_class(*n, "code").then_some("code")).unwrap_or("text").to_owned(), text)
    }).collect();
    assert_eq!(
        pieces,
        [("text".to_owned(), "a ".to_owned()), ("bold".to_owned(), "bold".to_owned()), ("text".to_owned(), " and ".to_owned()), ("code".to_owned(), "code".to_owned()), ("text".to_owned(), " word".to_owned())]
    );
    drop(tree);
    input_box.blur();
}

#[test]
fn show_message_with_force_opens_the_view_without_focus_and_hide_message_closes_it() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let provider = InlineContextViewProvider::new(&ui);
    let input_box = store.add_rc(InputBox::new(&ui, container, Some(provider.clone()), options()));
    let message = Message { content: Some("Heads up".to_owned()), format_content: false, r#type: Some(MessageType::Warning) };

    input_box.show_message(message.clone(), false);
    assert!(provider.container().is_none());
    input_box.show_message(message, true);
    assert!(provider.container().is_some());
    input_box.hide_message();
    assert!(provider.container().is_none());
    assert!(ui.tree().has_class(input_box.element(), "idle"));
}

#[test]
fn layout_follows_the_container_width() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    let inner = find(&ui, input_box.input_element(), &["inner-editor"]);
    assert_eq!(ui.tree().bounds(inner).width(), 200.0 - 2.0 - 12.0);

    ui.tree_mut().set_bounds(container, Rect::new(10.0, 10.0, 110.0, 150.0));
    input_box.layout();

    assert_eq!(ui.tree().bounds(input_box.element()).width(), 100.0);
    assert_eq!(ui.tree().bounds(inner).width(), 100.0 - 2.0 - 12.0);
}

#[test]
fn width_and_padding_right_shrink_the_text_window() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = new_box(&store, &ui, container, options());
    let inner = find(&ui, input_box.input_element(), &["inner-editor"]);

    input_box.set_padding_right(30.0);
    assert_eq!(ui.tree().bounds(inner).width(), 198.0 - 12.0 - 30.0);
    input_box.set_width(100.0);
    assert_eq!((input_box.width(), ui.tree().bounds(inner).width()), (100.0, 100.0 - 12.0 - 30.0));
}

#[test]
fn aria_label_goes_on_the_field() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let mut options = options();
    options.aria_label = Some("Find".to_owned());
    let input_box = new_box(&store, &ui, container, options);
    assert_eq!(ui.tree().label(input_box.input_element()), Some("Find"));

    input_box.set_aria_label("");

    assert_eq!(ui.tree().label(input_box.input_element()), None);
    assert_eq!(input_box.aria_label(), "");
}

#[test]
fn hide_hover_on_value_change_does_not_break_a_null_hover_service() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let mut options = options();
    options.hide_hover_on_value_change = true;
    let input_box = new_box(&store, &ui, container, options);

    input_box.set_value("x");

    assert_eq!(input_box.value(), "x");
}

// ---- HistoryInputBox -------------------------------------------------------------------------------------

fn history_box(store: &LeakGuard, ui: &Ui, container: NodeId, seed: &[&str], hint: bool) -> Rc<HistoryInputBox> {
    let mut input = options();
    input.placeholder = Some("Find".to_owned());
    input.history = Some(Rc::new(HistorySet::new(seed.iter().map(|s| (*s).to_owned()))));
    let hint: Option<Rc<dyn Fn() -> bool>> = hint.then(|| Rc::new(|| true) as Rc<dyn Fn() -> bool>);
    store.add_rc(HistoryInputBox::new(ui, container, None, HistoryInputBoxOptions { input, show_history_hint: hint }))
}

#[test]
fn history_adds_values_and_skips_the_current_one() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = history_box(&store, &ui, container, &[], false);

    for value in ["a", "b", "b"] {
        input_box.set_value(value);
        input_box.add_to_history(false);
    }
    input_box.set_value("");
    input_box.add_to_history(false);

    assert_eq!(input_box.history(), ["a", "b"]);
}

#[test]
fn history_navigation_goes_back_and_forth_and_ends_empty() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = history_box(&store, &ui, container, &["one", "two", "three"], false);

    input_box.show_previous_value();
    assert_eq!(input_box.value(), "three");
    input_box.show_previous_value();
    assert_eq!(input_box.value(), "two");
    input_box.show_next_value();
    assert_eq!(input_box.value(), "three");
    input_box.show_next_value();
    assert_eq!(input_box.value(), "");
}

#[test]
fn a_typed_value_is_saved_before_navigating_away() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = history_box(&store, &ui, container, &["one"], false);
    input_box.set_value("draft");

    input_box.show_previous_value();

    assert_eq!(input_box.history(), ["one", "draft"]);
    assert_eq!(input_box.value(), "one");
}

#[test]
fn prepend_history_puts_restored_entries_first() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = history_box(&store, &ui, container, &["c"], false);

    input_box.prepend_history(&["a".to_owned(), "b".to_owned()]);

    assert_eq!(input_box.history(), ["a", "b", "c"]);
    input_box.clear_history();
    assert!(input_box.history().is_empty());
}

#[test]
fn history_position_queries() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = history_box(&store, &ui, container, &["one", "two"], false);

    assert!(input_box.is_nowhere_in_history());
    input_box.show_previous_value();
    input_box.show_previous_value();
    assert!(input_box.is_at_first_in_history());
    input_box.show_next_value();
    assert!(input_box.is_at_last_in_history());
}

#[test]
fn the_history_hint_is_appended_to_the_placeholder_on_focus_and_removed_on_blur() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = history_box(&store, &ui, container, &["one"], true);

    assert_eq!(placeholder_text(&ui, &input_box).as_deref(), Some("Find"));
    input_box.focus();
    assert_eq!(placeholder_text(&ui, &input_box).as_deref(), Some("Find (\u{21C5} for history)"));
    assert_eq!(input_box.tooltip(), "Find (\u{21C5} for history)");
    // Um segundo foco não duplica a dica.
    input_box.blur();
    assert_eq!(placeholder_text(&ui, &input_box).as_deref(), Some("Find"));
    input_box.focus();
    input_box.blur();
    assert_eq!(placeholder_text(&ui, &input_box).as_deref(), Some("Find"));
}

#[test]
fn the_history_hint_uses_the_no_parens_form_after_a_closing_parenthesis() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(300.0);
    let input_box = history_box(&store, &ui, container, &["one"], true);
    input_box.set_place_holder("Filter (e.g. text)");

    input_box.focus();

    assert_eq!(placeholder_text(&ui, &input_box).as_deref(), Some("Filter (e.g. text) or \u{21C5} for history"));
    input_box.blur();
    assert_eq!(placeholder_text(&ui, &input_box).as_deref(), Some("Filter (e.g. text)"));
}

#[test]
fn no_history_means_no_hint() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(200.0);
    let input_box = history_box(&store, &ui, container, &[], true);

    input_box.focus();

    assert_eq!(placeholder_text(&ui, &input_box).as_deref(), Some("Find"));
}
