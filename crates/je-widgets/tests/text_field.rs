//! Testes extras do `TextField` (`src/text_field.rs`), o editor de linha única que o `<input>` do Chromium dá
//! ao `InputBox`. Não há original no VS Code: o comportamento fixado aqui é o do `<input>` (seleção em
//! UTF-16 como o `selectionStart`/`selectionEnd`, cursor por grafema, palavra, desfazer agrupado,
//! atalhos por sistema, rolagem horizontal).

use je_base::key_codes::KeyCode;
use je_widgets::dom::StandardKeyboardEvent;
use je_widgets::text::{ui_font, UI_FONT_SIZE};
use je_widgets::text_field::{DeleteUnit, MemoryClipboard, Movement, Platform, SelectionDirection, TextField, PASSWORD_MASK};

fn field(platform: Platform, value: &str) -> TextField {
    let mut field = TextField::new();
    field.set_platform(platform);
    field.set_value(value);
    field
}

fn linux(value: &str) -> TextField {
    field(Platform::Linux, value)
}

fn key(code: KeyCode) -> StandardKeyboardEvent {
    StandardKeyboardEvent { key_code: code, ctrl_key: false, shift_key: false, alt_key: false, meta_key: false }
}

fn ctrl(code: KeyCode) -> StandardKeyboardEvent {
    StandardKeyboardEvent { ctrl_key: true, ..key(code) }
}

fn shift(code: KeyCode) -> StandardKeyboardEvent {
    StandardKeyboardEvent { shift_key: true, ..key(code) }
}

fn meta(code: KeyCode) -> StandardKeyboardEvent {
    StandardKeyboardEvent { meta_key: true, ..key(code) }
}

fn alt(code: KeyCode) -> StandardKeyboardEvent {
    StandardKeyboardEvent { alt_key: true, ..key(code) }
}

const THUMBS_UP: &str = "\u{1F44D}";
const FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";

#[test]
fn set_value_puts_the_caret_at_the_end_and_strips_line_breaks() {
    let field = linux("ab\r\ncd\n");
    assert_eq!(field.value(), "abcd");
    assert_eq!(field.selection(), (4, 4));
    assert!(!field.has_selection());
}

#[test]
fn set_value_with_the_same_value_changes_nothing() {
    let mut field = linux("abc");
    field.set_selection_range(0, 1, SelectionDirection::Forward);
    assert!(!field.set_value("abc"));
    assert_eq!(field.selection(), (0, 1));
}

#[test]
fn insert_text_replaces_the_selection_and_leaves_the_caret_after_it() {
    let mut field = linux("hello world");
    field.set_selection_range(6, 11, SelectionDirection::Forward);
    assert!(field.insert_text("there"));
    assert_eq!((field.value(), field.selection()), ("hello there", (11, 11)));
}

#[test]
fn insert_text_turns_line_breaks_into_spaces() {
    let mut field = linux("");
    field.insert_text("a\r\nb\nc");
    assert_eq!(field.value(), "a b c");
}

#[test]
fn offsets_are_utf16_units() {
    let mut field = linux("a");
    field.insert_text(THUMBS_UP);
    assert_eq!((field.len(), field.selection()), (3, (3, 3)));
}

#[test]
fn a_selection_range_never_stops_inside_a_surrogate_pair() {
    let mut field = linux(&format!("a{THUMBS_UP}b"));
    field.set_selection_range(2, 2, SelectionDirection::Forward);
    assert_eq!(field.selection(), (1, 1));
    field.set_selection_range(2, 4, SelectionDirection::Forward);
    // O começo desce para antes do par e o fim sobe para depois dele.
    assert_eq!(field.selection(), (1, 4));
    field.set_selection_range(1, 2, SelectionDirection::Forward);
    assert_eq!(field.selection(), (1, 3));
}

#[test]
fn selection_range_is_clamped_to_the_text() {
    let mut field = linux("abc");
    field.set_selection_range(1, 99, SelectionDirection::Backward);
    assert_eq!(field.selection(), (1, 3));
    assert_eq!(field.selection_direction(), SelectionDirection::Backward);
    assert_eq!(field.caret(), 1);
}

#[test]
fn backspace_and_delete_remove_one_grapheme() {
    let mut field = linux(&format!("a{THUMBS_UP}"));
    field.delete(DeleteUnit::Grapheme, false);
    assert_eq!(field.value(), "a");
    let mut field = linux(&format!("{FAMILY}x"));
    field.set_caret(0);
    field.delete(DeleteUnit::Grapheme, true);
    assert_eq!(field.value(), "x");
}

#[test]
fn backspace_removes_a_combining_sequence_as_one() {
    let mut field = linux("e\u{0301}");
    field.delete(DeleteUnit::Grapheme, false);
    assert_eq!(field.value(), "");
}

#[test]
fn arrows_walk_by_grapheme_not_by_code_unit() {
    let mut field = linux(&format!("{FAMILY}b"));
    field.set_caret(0);
    field.move_caret(Movement::Right, false);
    assert_eq!(field.caret(), FAMILY.encode_utf16().count());
    field.move_caret(Movement::Left, false);
    assert_eq!(field.caret(), 0);
}

#[test]
fn left_and_right_collapse_a_selection_to_its_edges() {
    let mut field = linux("abcdef");
    field.set_selection_range(2, 4, SelectionDirection::Forward);
    field.move_caret(Movement::Left, false);
    assert_eq!(field.selection(), (2, 2));
    field.set_selection_range(2, 4, SelectionDirection::Forward);
    field.move_caret(Movement::Right, false);
    assert_eq!(field.selection(), (4, 4));
}

#[test]
fn shift_arrows_extend_from_the_anchor_in_both_directions() {
    let mut field = linux("abcdef");
    field.set_caret(3);
    field.move_caret(Movement::Right, true);
    field.move_caret(Movement::Right, true);
    assert_eq!((field.selection(), field.selection_direction()), ((3, 5), SelectionDirection::Forward));
    for _ in 0..4 {
        field.move_caret(Movement::Left, true);
    }
    assert_eq!((field.selection(), field.selection_direction()), ((1, 3), SelectionDirection::Backward));
}

#[test]
fn home_and_end_and_their_shift_forms() {
    let mut field = linux("abc");
    field.set_caret(1);
    assert!(field.handle_key(&key(KeyCode::Home), &MemoryClipboard::default()).handled);
    assert_eq!(field.selection(), (0, 0));
    field.handle_key(&shift(KeyCode::End), &MemoryClipboard::default());
    assert_eq!(field.selection(), (0, 3));
}

#[test]
fn up_and_down_go_to_the_start_and_the_end() {
    let mut field = linux("abc");
    field.set_caret(1);
    field.handle_key(&key(KeyCode::DownArrow), &MemoryClipboard::default());
    assert_eq!(field.caret(), 3);
    field.handle_key(&key(KeyCode::UpArrow), &MemoryClipboard::default());
    assert_eq!(field.caret(), 0);
}

#[test]
fn word_movement_on_linux_goes_to_the_word_end_going_right() {
    let mut field = linux("foo, bar_baz  qux");
    field.set_caret(0);
    let mut stops = Vec::new();
    for _ in 0..3 {
        field.move_caret(Movement::WordRight, false);
        stops.push(field.caret());
    }
    assert_eq!(stops, [3, 12, 17]);
    let mut back = Vec::new();
    for _ in 0..3 {
        field.move_caret(Movement::WordLeft, false);
        back.push(field.caret());
    }
    assert_eq!(back, [14, 5, 0]);
}

#[test]
fn word_movement_on_windows_goes_to_the_next_word_start_going_right() {
    let mut field = field(Platform::Windows, "foo bar  baz");
    field.set_caret(0);
    field.move_caret(Movement::WordRight, false);
    assert_eq!(field.caret(), 4);
    field.move_caret(Movement::WordRight, false);
    assert_eq!(field.caret(), 9);
    field.move_caret(Movement::WordLeft, false);
    assert_eq!(field.caret(), 4);
}

#[test]
fn ctrl_backspace_and_ctrl_delete_remove_a_word() {
    let mut field = linux("one two three");
    field.set_caret(7);
    let clipboard = MemoryClipboard::default();
    assert!(field.handle_key(&ctrl(KeyCode::Backspace), &clipboard).changed);
    assert_eq!((field.value(), field.caret()), ("one  three", 4));
    field.handle_key(&ctrl(KeyCode::Delete), &clipboard);
    assert_eq!(field.value(), "one ");
}

#[test]
fn word_delete_with_a_selection_deletes_only_the_selection() {
    let mut field = linux("one two");
    field.set_selection_range(0, 2, SelectionDirection::Forward);
    field.delete(DeleteUnit::Word, false);
    assert_eq!(field.value(), "e two");
}

#[test]
fn cmd_backspace_on_mac_deletes_to_the_line_start() {
    let mut field = field(Platform::Mac, "hello world");
    field.set_caret(5);
    field.handle_key(&meta(KeyCode::Backspace), &MemoryClipboard::default());
    assert_eq!(field.value(), " world");
}

#[test]
fn option_arrows_move_by_word_on_mac_and_cmd_arrows_to_the_edges() {
    let mut field = field(Platform::Mac, "foo bar");
    let clipboard = MemoryClipboard::default();
    field.set_caret(0);
    field.handle_key(&alt(KeyCode::RightArrow), &clipboard);
    assert_eq!(field.caret(), 3);
    field.handle_key(&meta(KeyCode::RightArrow), &clipboard);
    assert_eq!(field.caret(), 7);
    field.handle_key(&meta(KeyCode::LeftArrow), &clipboard);
    assert_eq!(field.caret(), 0);
}

#[test]
fn ctrl_arrows_on_mac_are_not_the_fields() {
    let mut field = field(Platform::Mac, "foo bar");
    field.set_caret(0);
    let outcome = field.handle_key(&ctrl(KeyCode::RightArrow), &MemoryClipboard::default());
    assert!(!outcome.handled);
    assert_eq!(field.caret(), 0);
}

#[test]
fn double_click_selects_the_word_and_drag_grows_by_words() {
    let mut field = linux("foo bar baz");
    field.pointer_down(5, 2, false);
    assert_eq!(field.selection(), (4, 7));
    field.pointer_drag(9);
    assert_eq!(field.selection(), (4, 11));
    field.pointer_drag(1);
    assert_eq!((field.selection(), field.selection_direction()), ((0, 7), SelectionDirection::Backward));
    field.pointer_up();
    assert!(!field.is_dragging());
}

#[test]
fn double_click_on_windows_includes_the_trailing_spaces() {
    let mut field = field(Platform::Windows, "foo  bar");
    field.pointer_down(1, 2, false);
    assert_eq!(field.selection(), (0, 5));
}

#[test]
fn double_click_on_punctuation_selects_one_character_and_on_spaces_the_run() {
    let mut field = linux("a, , b");
    assert_eq!(field.word_range_at(1), (1, 2));
    field.pointer_down(2, 2, false);
    assert_eq!(field.selection(), (2, 3));
    assert_eq!(field.word_range_at(5), (5, 6));
}

#[test]
fn double_click_past_the_end_selects_the_last_word() {
    let field = linux("foo bar");
    assert_eq!(field.word_range_at(99), (4, 7));
}

#[test]
fn triple_click_selects_everything() {
    let mut field = linux("foo bar");
    field.pointer_down(2, 3, false);
    assert_eq!(field.selection(), (0, 7));
    field.pointer_drag(1);
    assert_eq!(field.selection(), (0, 7));
}

#[test]
fn click_places_the_caret_and_shift_click_extends() {
    let mut field = linux("foo bar");
    field.pointer_down(2, 1, false);
    assert_eq!(field.selection(), (2, 2));
    field.pointer_up();
    field.pointer_down(6, 1, true);
    assert_eq!(field.selection(), (2, 6));
}

#[test]
fn dragging_with_the_mouse_selects_both_ways() {
    let mut field = linux("abcdefgh");
    field.pointer_down(3, 1, false);
    field.pointer_drag(6);
    assert_eq!(field.selection(), (3, 6));
    field.pointer_drag(1);
    assert_eq!((field.selection(), field.selection_direction()), ((1, 3), SelectionDirection::Backward));
}

#[test]
fn typing_is_one_undo_step_until_the_caret_moves() {
    let mut field = linux("");
    for c in ["a", "b", "c"] {
        field.insert_text(c);
    }
    field.move_caret(Movement::Left, false);
    field.move_caret(Movement::Right, false);
    field.insert_text("d");
    assert_eq!(field.value(), "abcd");
    assert!(field.undo());
    assert_eq!(field.value(), "abc");
    assert!(field.undo());
    assert_eq!((field.value(), field.selection()), ("", (0, 0)));
    assert!(!field.undo());
}

#[test]
fn undo_restores_the_replaced_selection_and_redo_replays_it() {
    let mut field = linux("hello");
    field.set_selection_range(1, 4, SelectionDirection::Forward);
    field.insert_text("X");
    assert_eq!(field.value(), "hXo");
    field.undo();
    assert_eq!((field.value(), field.selection()), ("hello", (1, 4)));
    assert!(field.redo());
    assert_eq!((field.value(), field.selection()), ("hXo", (2, 2)));
    assert!(!field.redo());
}

#[test]
fn a_new_edit_clears_the_redo_stack() {
    let mut field = linux("");
    field.insert_text("a");
    field.undo();
    field.insert_text("b");
    assert!(!field.redo());
}

#[test]
fn consecutive_backspaces_undo_together_and_so_do_deletes() {
    let mut field = linux("abcdef");
    for _ in 0..3 {
        field.delete(DeleteUnit::Grapheme, false);
    }
    assert_eq!(field.value(), "abc");
    field.undo();
    assert_eq!(field.value(), "abcdef");
    field.set_caret(0);
    for _ in 0..2 {
        field.delete(DeleteUnit::Grapheme, true);
    }
    assert_eq!(field.value(), "cdef");
    field.undo();
    assert_eq!(field.value(), "abcdef");
}

#[test]
fn paste_and_word_deletes_are_their_own_undo_steps() {
    let mut field = linux("");
    let clipboard = MemoryClipboard::with_text("xyz");
    field.insert_text("a");
    field.paste(&clipboard);
    field.insert_text("b");
    assert_eq!(field.value(), "axyzb");
    field.undo();
    assert_eq!(field.value(), "axyz");
    field.undo();
    assert_eq!(field.value(), "a");
}

#[test]
fn set_value_clears_the_undo_history() {
    let mut field = linux("");
    field.insert_text("a");
    field.set_value("b");
    assert!(!field.undo());
}

#[test]
fn undo_and_redo_work_with_surrogate_pairs() {
    let mut field = linux("");
    field.insert_text(THUMBS_UP);
    field.insert_text("x");
    field.undo();
    assert_eq!(field.value(), "");
    field.redo();
    assert_eq!(field.value(), format!("{THUMBS_UP}x"));
}

#[test]
fn cut_copy_and_paste_go_through_the_clipboard() {
    let clipboard = MemoryClipboard::new();
    let mut field = linux("hello world");
    field.set_selection_range(0, 5, SelectionDirection::Forward);
    assert!(field.copy(&*clipboard));
    assert_eq!(clipboard_text(&clipboard), "hello");
    assert!(field.cut(&*clipboard));
    assert_eq!(field.value(), " world");
    field.set_caret(6);
    assert!(field.paste(&*clipboard));
    assert_eq!(field.value(), " worldhello");
}

fn clipboard_text(clipboard: &MemoryClipboard) -> String {
    use je_widgets::text_field::ClipboardProvider;
    clipboard.read_text().unwrap_or_default()
}

#[test]
fn copy_with_no_selection_leaves_the_clipboard_alone() {
    let clipboard = MemoryClipboard::with_text("keep");
    let field = linux("abc");
    assert!(!field.copy(&*clipboard));
    assert_eq!(clipboard_text(&clipboard), "keep");
}

#[test]
fn paste_with_an_empty_clipboard_does_nothing() {
    let mut field = linux("abc");
    assert!(!field.paste(&*MemoryClipboard::new()));
    assert_eq!(field.value(), "abc");
}

#[test]
fn pasted_line_breaks_become_spaces() {
    let mut field = linux("");
    field.paste(&*MemoryClipboard::with_text("one\ntwo\r\nthree"));
    assert_eq!(field.value(), "one two three");
}

#[test]
fn a_password_field_does_not_copy_or_cut_and_masks_its_text() {
    let clipboard = MemoryClipboard::with_text("keep");
    let mut field = linux(&format!("s3{THUMBS_UP}"));
    field.set_password(true);
    field.select_all();
    assert!(!field.copy(&*clipboard));
    assert!(!field.cut(&*clipboard));
    assert_eq!(field.value(), format!("s3{THUMBS_UP}"));
    assert_eq!(clipboard_text(&clipboard), "keep");
    let mask = PASSWORD_MASK.to_string();
    assert_eq!(field.display_text(), mask.repeat(3));
}

#[test]
fn keyboard_shortcuts_use_ctrl_on_linux_and_windows() {
    for platform in [Platform::Linux, Platform::Windows] {
        let clipboard = MemoryClipboard::new();
        let mut field = field(platform, "abc");
        assert!(field.handle_key(&ctrl(KeyCode::KeyA), &clipboard).handled);
        assert_eq!(field.selection(), (0, 3));
        field.handle_key(&ctrl(KeyCode::KeyX), &clipboard);
        assert_eq!(field.value(), "");
        field.handle_key(&ctrl(KeyCode::KeyV), &clipboard);
        assert_eq!(field.value(), "abc");
        field.handle_key(&ctrl(KeyCode::KeyZ), &clipboard);
        assert_eq!(field.value(), "");
        field.handle_key(&ctrl(KeyCode::KeyY), &clipboard);
        assert_eq!(field.value(), "abc");
        field.handle_key(&ctrl(KeyCode::KeyZ), &clipboard);
        field.handle_key(&StandardKeyboardEvent { ctrl_key: true, shift_key: true, ..key(KeyCode::KeyZ) }, &clipboard);
        assert_eq!(field.value(), "abc");
    }
}

#[test]
fn keyboard_shortcuts_use_cmd_on_mac() {
    let clipboard = MemoryClipboard::new();
    let mut field = field(Platform::Mac, "abc");
    assert!(!field.handle_key(&ctrl(KeyCode::KeyA), &clipboard).changed);
    assert!(field.handle_key(&meta(KeyCode::KeyA), &clipboard).handled);
    assert_eq!(field.selection(), (0, 3));
    field.handle_key(&meta(KeyCode::KeyC), &clipboard);
    assert_eq!(clipboard_text(&clipboard), "abc");
    field.handle_key(&meta(KeyCode::KeyV), &clipboard);
    assert_eq!(field.value(), "abc");
    field.handle_key(&meta(KeyCode::KeyZ), &clipboard);
    assert_eq!(field.value(), "abc");
    field.set_caret(3);
    field.insert_text("d");
    field.handle_key(&meta(KeyCode::KeyZ), &clipboard);
    field.handle_key(&StandardKeyboardEvent { meta_key: true, shift_key: true, ..key(KeyCode::KeyZ) }, &clipboard);
    assert_eq!(field.value(), "abcd");
}

#[test]
fn cmd_y_is_not_redo_on_mac_and_emacs_keys_work() {
    let clipboard = MemoryClipboard::new();
    let mut field = field(Platform::Mac, "abc");
    assert!(!field.handle_key(&meta(KeyCode::KeyY), &clipboard).handled);
    field.handle_key(&ctrl(KeyCode::KeyA), &clipboard);
    assert_eq!(field.caret(), 0);
    field.handle_key(&ctrl(KeyCode::KeyE), &clipboard);
    assert_eq!(field.caret(), 3);
    field.handle_key(&ctrl(KeyCode::KeyH), &clipboard);
    assert_eq!(field.value(), "ab");
    field.handle_key(&ctrl(KeyCode::KeyB), &clipboard);
    field.handle_key(&ctrl(KeyCode::KeyK), &clipboard);
    assert_eq!(field.value(), "a");
}

#[test]
fn legacy_clipboard_keys_on_linux() {
    let clipboard = MemoryClipboard::with_text("Z");
    let mut field = linux("abc");
    field.select_all();
    field.handle_key(&ctrl(KeyCode::Insert), &clipboard);
    assert_eq!(clipboard_text(&clipboard), "abc");
    field.handle_key(&shift(KeyCode::Delete), &clipboard);
    assert_eq!(field.value(), "");
    field.handle_key(&shift(KeyCode::Insert), &clipboard);
    assert_eq!(field.value(), "abc");
}

#[test]
fn keys_that_only_produce_text_or_belong_to_others_are_not_handled() {
    let clipboard = MemoryClipboard::default();
    let mut field = linux("abc");
    for code in [KeyCode::KeyA, KeyCode::Enter, KeyCode::Escape, KeyCode::Tab, KeyCode::Space, KeyCode::F5, KeyCode::PageUp] {
        assert!(!field.handle_key(&key(code), &clipboard).handled, "{code:?}");
    }
    assert!(!field.handle_key(&alt(KeyCode::LeftArrow), &clipboard).handled);
    assert_eq!(field.value(), "abc");
}

#[test]
fn backspace_at_the_start_and_delete_at_the_end_change_nothing() {
    let clipboard = MemoryClipboard::default();
    let mut field = linux("ab");
    field.set_caret(0);
    assert!(!field.handle_key(&key(KeyCode::Backspace), &clipboard).changed);
    field.set_caret(2);
    assert!(field.handle_key(&key(KeyCode::Delete), &clipboard).handled);
    assert_eq!(field.value(), "ab");
}

fn style() -> je_text::TextStyle {
    ui_font(UI_FONT_SIZE, 400)
}

#[test]
fn x_grows_with_the_offset_and_offset_at_x_inverts_it() {
    let field = linux("hello world");
    let style = style();
    let xs: Vec<f64> = (0..=field.len()).map(|o| field.x_at_offset(o, &style)).collect();
    assert_eq!(xs[0], 0.0);
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
    assert_eq!(xs[field.len()], field.content_width(&style));
    for (offset, x) in xs.iter().enumerate() {
        assert_eq!(field.offset_at_x(*x, &style), offset);
    }
}

#[test]
fn offset_at_x_picks_the_nearest_boundary_and_clamps() {
    let field = linux("abcd");
    let style = style();
    let (x1, x2) = (field.x_at_offset(1, &style), field.x_at_offset(2, &style));
    assert_eq!(field.offset_at_x(x1 + (x2 - x1) * 0.4, &style), 1);
    assert_eq!(field.offset_at_x(x1 + (x2 - x1) * 0.6, &style), 2);
    assert_eq!(field.offset_at_x(-20.0, &style), 0);
    assert_eq!(field.offset_at_x(10_000.0, &style), 4);
}

#[test]
fn offset_at_x_never_lands_inside_an_emoji() {
    let field = linux(&format!("a{FAMILY}b"));
    let style = style();
    let family_len = FAMILY.encode_utf16().count();
    let allowed = [0, 1, 1 + family_len, 2 + family_len];
    for step in 0..60 {
        let offset = field.offset_at_x(f64::from(step) * 1.5, &style);
        assert!(allowed.contains(&offset), "{offset}");
    }
}

#[test]
fn the_password_mask_measures_one_bullet_per_character() {
    let mut field = linux("secret");
    field.set_password(true);
    let style = style();
    let bullet = je_widgets::text::measure_text("\u{2022}", &style);
    assert!((field.x_at_offset(3, &style) - 3.0 * bullet).abs() < 0.05);
    assert_eq!(field.offset_at_x(2.0 * bullet + 0.1, &style), 2);
}

#[test]
fn the_selection_range_in_pixels_follows_the_offsets() {
    let mut field = linux("hello world");
    let style = style();
    assert_eq!(field.selection_x_range(&style), None);
    field.set_selection_range(2, 5, SelectionDirection::Forward);
    let (x0, x1) = field.selection_x_range(&style).expect("seleção");
    assert_eq!((x0, x1), (field.x_at_offset(2, &style), field.x_at_offset(5, &style)));
}

#[test]
fn reveal_caret_scrolls_just_enough_and_never_below_zero() {
    let mut field = linux("the quick brown fox jumps over the lazy dog");
    let style = style();
    let viewport = 80.0;
    field.reveal_caret(viewport, &style);
    let end_x = field.x_at_offset(field.len(), &style);
    assert!((field.scroll_left() - (end_x - viewport)).abs() < 1e-9);
    field.set_caret(0);
    field.reveal_caret(viewport, &style);
    assert_eq!(field.scroll_left(), 0.0);
    // O cursor já visível não rola.
    field.set_caret(3);
    field.reveal_caret(viewport, &style);
    assert_eq!(field.scroll_left(), 0.0);
}

#[test]
fn text_that_fits_never_scrolls() {
    let mut field = linux("hi");
    let style = style();
    field.reveal_caret(200.0, &style);
    field.set_scroll_left(50.0, 200.0, &style);
    assert_eq!(field.scroll_left(), 0.0);
}

#[test]
fn scroll_left_is_limited_to_the_hidden_width() {
    let mut field = linux("the quick brown fox jumps over the lazy dog");
    let style = style();
    field.set_scroll_left(1.0e6, 80.0, &style);
    assert!((field.scroll_left() - (field.content_width(&style) - 80.0)).abs() < 1e-9);
}
