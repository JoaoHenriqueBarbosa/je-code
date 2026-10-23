//! A entrada do editor: cursor, seleção, digitação, exclusão, indentação, desfazer e área de transferência.

mod support;

use je_editor::position::Position;
use je_editor_view::EditorKey;
use je_ui::{Modifiers, PointerButton};
use support::plain_view;

const NONE: Modifiers = Modifiers { alt: false, ctrl: false, shift: false, meta: false };
const SHIFT: Modifiers = Modifiers { alt: false, ctrl: false, shift: true, meta: false };
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, meta: false };
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, meta: false };

fn text(view: &je_editor_view::EditorView) -> String {
    view.model().get_value().to_string_lossy()
}

#[test]
fn typing_inserts_and_advances_cursor() {
    let mut v = plain_view("");
    v.type_text("a");
    v.type_text("b");
    assert_eq!(text(&v), "ab");
    assert_eq!(v.cursor_position(), Position::new(1, 3));
}

#[test]
fn arrows_home_end_and_pages() {
    let mut v = plain_view("    abc\nxy\n    def");
    v.set_cursor(Position::new(1, 8));
    v.key_down(EditorKey::Down, NONE);
    assert_eq!(v.cursor_position(), Position::new(2, 3), "a linha curta limita a coluna");
    v.key_down(EditorKey::Down, NONE);
    assert_eq!(v.cursor_position(), Position::new(3, 8), "a coluna desejada volta");
    v.key_down(EditorKey::Home, NONE);
    assert_eq!(v.cursor_position(), Position::new(3, 5), "Home vai ao texto");
    v.key_down(EditorKey::Home, NONE);
    assert_eq!(v.cursor_position(), Position::new(3, 1));
    v.key_down(EditorKey::End, NONE);
    assert_eq!(v.cursor_position(), Position::new(3, 8));
    v.key_down(EditorKey::Home, CTRL);
    assert_eq!(v.cursor_position(), Position::new(1, 1));
    v.key_down(EditorKey::End, CTRL);
    assert_eq!(v.cursor_position(), Position::new(3, 8));
    v.key_down(EditorKey::PageUp, NONE);
    assert_eq!(v.cursor_position().line_number, 1);
}

#[test]
fn ctrl_arrows_move_by_word_and_shift_selects() {
    let mut v = plain_view("foo.bar baz");
    v.key_down(EditorKey::Right, CTRL);
    assert_eq!(v.cursor_position(), Position::new(1, 4));
    v.key_down(EditorKey::Right, CTRL_SHIFT);
    assert_eq!(v.cursor_position(), Position::new(1, 5));
    assert_eq!(v.model().get_value_in_range(&v.cursor_selection_range()).to_string_lossy(), ".");
    v.key_down(EditorKey::Left, NONE);
    assert_eq!(v.cursor_position(), Position::new(1, 4), "a seta sem Shift colapsa na ponta esquerda");
}

#[test]
fn backspace_delete_and_line_join() {
    let mut v = plain_view("ab\ncd");
    v.set_cursor(Position::new(2, 1));
    v.key_down(EditorKey::Backspace, NONE);
    assert_eq!(text(&v), "abcd");
    assert_eq!(v.cursor_position(), Position::new(1, 3));
    v.key_down(EditorKey::Delete, NONE);
    assert_eq!(text(&v), "abd");
    v.key_down(EditorKey::Backspace, CTRL);
    assert_eq!(text(&v), "d", "Ctrl+Backspace apaga a palavra");
}

#[test]
fn backspace_in_indentation_goes_back_one_stop() {
    let mut v = plain_view("        x");
    v.set_cursor(Position::new(1, 9));
    v.key_down(EditorKey::Backspace, NONE);
    assert_eq!(text(&v), "    x", "de 8 espaços para 4, uma parada de tabulação");
}

#[test]
fn enter_keeps_line_indentation() {
    let mut v = plain_view("\t\tfoo");
    v.set_cursor(Position::new(1, 6));
    v.key_down(EditorKey::Enter, NONE);
    assert_eq!(text(&v), "\t\tfoo\n\t\t");
    assert_eq!(v.cursor_position(), Position::new(2, 3));
    // Enter de novo na linha só de indentação: a indentação automática da linha em branco é aparada.
    v.key_down(EditorKey::Enter, NONE);
    assert_eq!(text(&v), "\t\tfoo\n\n\t\t");
    assert_eq!(v.cursor_position(), Position::new(3, 3));
}

#[test]
fn tab_inserts_to_stop_and_shift_tab_removes() {
    let mut v = plain_view("ab");
    v.set_cursor(Position::new(1, 3));
    v.key_down(EditorKey::Tab, NONE);
    assert_eq!(text(&v), "ab  ", "de 2 colunas até a parada 4");
    v.key_down(EditorKey::Tab, SHIFT);
    assert_eq!(text(&v), "ab  ", "sem indentação à esquerda nada sai");
    v.set_cursor(Position::new(1, 1));
    v.key_down(EditorKey::Tab, NONE);
    assert_eq!(text(&v), "    ab  ");
    v.key_down(EditorKey::Tab, SHIFT);
    assert_eq!(text(&v), "ab  ");
}

#[test]
fn block_tab_indents_selected_lines() {
    let mut v = plain_view("a\nb\nc");
    v.select_all();
    v.key_down(EditorKey::Tab, NONE);
    assert_eq!(text(&v), "    a\n    b\n    c");
    v.key_down(EditorKey::Tab, SHIFT);
    assert_eq!(text(&v), "a\nb\nc");
}

#[test]
fn undo_and_redo_restore_text_and_selection() {
    let mut v = plain_view("hello");
    v.set_cursor(Position::new(1, 6));
    for c in [" ", "w", "o", "r", "l", "d"] {
        v.type_text(c);
    }
    assert_eq!(text(&v), "hello world");
    v.key_down(EditorKey::Char('z'), CTRL);
    assert_eq!(text(&v), "hello", "digitação consecutiva cai num grupo só");
    assert_eq!(v.cursor_position(), Position::new(1, 6));
    v.key_down(EditorKey::Char('z'), CTRL_SHIFT);
    assert_eq!(text(&v), "hello world");
    v.key_down(EditorKey::Char('z'), CTRL);
    v.key_down(EditorKey::Char('y'), CTRL);
    assert_eq!(text(&v), "hello world");
}

#[test]
fn select_all_copy_cut_paste() {
    let mut v = plain_view("one\ntwo");
    v.key_down(EditorKey::Char('a'), CTRL);
    v.key_down(EditorKey::Char('c'), CTRL);
    v.set_cursor(Position::new(2, 4));
    v.key_down(EditorKey::Char('v'), CTRL);
    assert_eq!(text(&v), "one\ntwoone\ntwo");
    v.undo();
    v.set_cursor(Position::new(1, 1));
    // Sem seleção, copiar e recortar levam a linha inteira.
    v.key_down(EditorKey::Char('x'), CTRL);
    assert_eq!(text(&v), "two");
    v.key_down(EditorKey::Char('v'), CTRL);
    assert_eq!(text(&v), "one\ntwo");
}

#[test]
fn click_drag_double_and_triple() {
    let mut v = plain_view("foo bar baz\nsecond line");
    let left = v.layout().content_left();
    let cw = v.layout().metrics.char_width;
    let y0 = 5.0;
    v.mouse_down(left + cw * 5.2, y0, PointerButton::Primary, 1, NONE);
    assert_eq!(v.cursor_position(), Position::new(1, 6));
    v.mouse_drag(left + cw * 9.1, y0);
    assert_eq!(v.cursor_selection_range().end_column, 10);
    v.mouse_up();
    // Duplo clique em "bar".
    v.mouse_down(left + cw * 5.0, y0, PointerButton::Primary, 2, NONE);
    assert_eq!(v.model().get_value_in_range(&v.cursor_selection_range()).to_string_lossy(), "bar");
    // Arrastar depois do duplo clique estende por palavras.
    v.mouse_drag(left + cw * 9.0, y0);
    assert_eq!(v.model().get_value_in_range(&v.cursor_selection_range()).to_string_lossy(), "bar baz");
    v.mouse_up();
    // Triplo clique na segunda linha.
    v.mouse_down(left + cw * 2.0, 19.0 + 5.0, PointerButton::Primary, 3, NONE);
    assert_eq!(v.model().get_value_in_range(&v.cursor_selection_range()).to_string_lossy(), "second line");
    // Shift+clique estende a partir da âncora.
    v.mouse_down(left + cw * 3.0, y0, PointerButton::Primary, 1, NONE);
    v.mouse_down(left + cw * 8.0, y0, PointerButton::Primary, 1, SHIFT);
    assert_eq!(v.model().get_value_in_range(&v.cursor_selection_range()).to_string_lossy(), " bar ");
}

#[test]
fn position_to_pixel_and_back_with_tabs() {
    let mut v = plain_view("\tx = 1;\nabc");
    let cw = v.layout().metrics.char_width;
    let left = v.layout().content_left();
    for column in 1..=8 {
        v.set_cursor(Position::new(1, column));
        let rect = v.cursor_rect();
        // A coluna volta do pixel (o meio do caractere cai na mesma coluna).
        let p = v.position_at(rect.x0 + 0.5, 4.0);
        assert_eq!(p, Position::new(1, column), "coluna {column}");
    }
    // A tabulação ocupa 4 células.
    v.set_cursor(Position::new(1, 2));
    assert!((v.cursor_rect().x0 - (left + 4.0 * cw)).abs() < 1e-9);
    // Clicar além do fim da linha vai para o fim.
    assert_eq!(v.position_at(left + 500.0, 4.0 + 19.0), Position::new(2, 4));
}

#[test]
fn status_shows_line_column_and_selection_size() {
    let mut v = plain_view("a\tb");
    v.set_cursor(Position::new(1, 3));
    assert_eq!(v.status_text(), "Ln 1, Col 5", "a coluna da barra de status conta a tabulação");
    v.select_all();
    assert_eq!(v.status_text(), "Ln 1, Col 6 (3 selected)");
}

#[test]
fn scroll_follows_cursor() {
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let mut v = plain_view(&text);
    v.key_down(EditorKey::End, CTRL);
    let last = v.cursor_position().line_number;
    let top = v.scroll_top();
    let lh = v.layout().line_height();
    let line_top = f64::from(last - 1) * lh - top;
    assert!(line_top >= 0.0 && line_top + lh <= v.layout().height + 0.001, "a última linha está visível: {line_top}");
    v.key_down(EditorKey::Home, CTRL);
    assert_eq!(v.scroll_top(), 0.0);
}
