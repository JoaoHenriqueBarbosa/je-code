//! Os comandos do núcleo do editor pela `EditorView`: vários cursores, ações de linha, comentário, `Ctrl+D` e a
//! área de transferência com seleção vazia. Cada um passa pelo atalho do VS Code (o `KeybindingsRegistry` do
//! `je-editor`) e confere o texto e as seleções que o `CodeEditorWidget` deixa.

mod support;

use je_editor::position::Position;
use je_editor_view::EditorKey;
use je_ui::{Modifiers, PointerButton};
use support::{environment, plain_view, sample_view};

const NONE: Modifiers = Modifiers { alt: false, ctrl: false, shift: false, meta: false };
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, meta: false };
const ALT: Modifiers = Modifiers { alt: true, ctrl: false, shift: false, meta: false };
const CTRL_ALT: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, meta: false };
const SHIFT_ALT: Modifiers = Modifiers { alt: true, ctrl: false, shift: true, meta: false };
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, meta: false };

fn line(view: &je_editor_view::EditorView, n: i32) -> String {
    use je_editor::cursor_common::CursorSimpleModel;
    view.model().get_line_content(n).to_string_lossy()
}

fn text(view: &je_editor_view::EditorView) -> String {
    view.model().get_value().to_string_lossy()
}

/// O ponto da view sobre a coluna `column` da linha `line` (a view sem rolagem, fonte de 19 px de linha).
fn point_at(view: &mut je_editor_view::EditorView, line: i32, column: i32) -> (f64, f64) {
    let layout = view.layout();
    let y = f64::from(line - 1) * layout.line_height() + layout.line_height() / 2.0;
    let x = layout.content_left() + f64::from(column - 1) * layout.metrics.char_width + 1.0;
    (x, y)
}

#[test]
fn alt_click_adds_a_cursor_and_typing_goes_to_both() {
    let mut v = plain_view("one\ntwo\nthree");
    v.set_cursor(Position::new(1, 1));
    let (x, y) = point_at(&mut v, 2, 1);
    v.mouse_down(x, y, PointerButton::Primary, 1, ALT);
    v.mouse_up();
    assert_eq!(v.selections().len(), 2, "Alt+clique cria o segundo cursor");
    v.type_text("X");
    assert_eq!(text(&v), "Xone\nXtwo\nthree");
    v.undo();
    assert_eq!(text(&v), "one\ntwo\nthree", "um desfazer reverte a digitação dos dois cursores");
    // Alt+clique sobre um cursor existente o remove.
    let (x, y) = point_at(&mut v, 2, 1);
    v.mouse_down(x, y, PointerButton::Primary, 1, ALT);
    v.mouse_up();
    assert_eq!(v.selections().len(), 1);
}

#[test]
fn insert_cursor_below_and_escape_back_to_one() {
    let mut v = plain_view("ab\ncd\nef");
    v.set_cursor(Position::new(1, 2));
    // `insertCursorBelow`: `Ctrl+Alt+Seta` no Windows e no macOS, `Shift+Alt+Seta` no Linux.
    let mods = if cfg!(target_os = "linux") { SHIFT_ALT } else { CTRL_ALT };
    v.key_down(EditorKey::Down, mods);
    v.key_down(EditorKey::Down, mods);
    assert_eq!(v.selections().len(), 3);
    v.type_text("-");
    assert_eq!(text(&v), "a-b\nc-d\ne-f");
    v.key_down(EditorKey::Escape, NONE);
    assert_eq!(v.selections().len(), 1, "Escape fica só com o cursor primário");
}

#[test]
fn move_lines_with_alt_arrows() {
    let mut v = plain_view("a\nb\nc");
    v.set_cursor(Position::new(1, 1));
    v.key_down(EditorKey::Down, ALT);
    assert_eq!(text(&v), "b\na\nc");
    assert_eq!(v.cursor_position().line_number, 2, "o cursor acompanha a linha");
    v.key_down(EditorKey::Up, ALT);
    assert_eq!(text(&v), "a\nb\nc");
}

#[test]
fn copy_line_down_and_delete_line() {
    let mut v = plain_view("a\nb");
    v.set_cursor(Position::new(1, 1));
    // O Linux troca `Shift+Alt+Seta` por `Ctrl+Shift+Alt+Seta` (o primeiro é do gerenciador de janelas).
    let mods = if cfg!(target_os = "linux") { Modifiers { alt: true, ctrl: true, shift: true, meta: false } } else { Modifiers { alt: true, ctrl: false, shift: true, meta: false } };
    v.key_down(EditorKey::Down, mods);
    assert_eq!(text(&v), "a\na\nb");
    v.key_down(EditorKey::Char('k'), CTRL_SHIFT);
    assert_eq!(text(&v), "a\nb", "Ctrl+Shift+K apaga a linha");
}

#[test]
fn ctrl_d_selects_word_then_next_occurrence() {
    let mut v = plain_view("foo bar foo");
    v.set_cursor(Position::new(1, 2));
    v.key_down(EditorKey::Char('d'), CTRL);
    assert_eq!(v.selections().len(), 1);
    assert_eq!(v.model().get_value_in_range(&v.cursor_selection_range()).to_string_lossy(), "foo", "o primeiro Ctrl+D seleciona a palavra");
    v.key_down(EditorKey::Char('d'), CTRL);
    assert_eq!(v.selections().len(), 2, "o segundo acrescenta a próxima ocorrência");
    v.type_text("x");
    assert_eq!(text(&v), "x bar x");
}

#[test]
fn line_comment_toggle_and_chords() {
    let Some(env) = environment() else {
        eprintln!("extensões embutidas ausentes: comentário de linha não conferido");
        return;
    };
    let mut v = sample_view(&env);
    v.set_cursor(Position::new(2, 1));
    let before = line(&v, 2);
    v.key_down(EditorKey::Char('/'), CTRL);
    assert!(line(&v, 2).starts_with("// "), "Ctrl+/ comenta a linha: {}", line(&v, 2));
    v.key_down(EditorKey::Char('/'), CTRL);
    assert_eq!(line(&v, 2), before, "e descomenta de novo");
    // O acorde Ctrl+K Ctrl+C adiciona o comentário e Ctrl+K Ctrl+U o remove.
    v.key_down(EditorKey::Char('k'), CTRL);
    v.key_down(EditorKey::Char('c'), CTRL);
    assert!(line(&v, 2).starts_with("// "));
    v.key_down(EditorKey::Char('k'), CTRL);
    v.key_down(EditorKey::Char('u'), CTRL);
    assert_eq!(line(&v, 2), before);
}

#[test]
fn copying_an_empty_selection_pastes_a_whole_line_above() {
    let mut v = plain_view("one\ntwo");
    v.set_cursor(Position::new(1, 2));
    v.key_down(EditorKey::Char('c'), CTRL);
    v.set_cursor(Position::new(2, 2));
    v.key_down(EditorKey::Char('v'), CTRL);
    assert_eq!(text(&v), "one\none\ntwo", "colar o que veio de seleção vazia entra na linha de cima");
}

#[test]
fn status_counts_cursors() {
    let mut v = plain_view("ab\ncd");
    v.set_cursor(Position::new(1, 1));
    v.key_down(EditorKey::Down, if cfg!(target_os = "linux") { SHIFT_ALT } else { CTRL_ALT });
    assert_eq!(v.status_text(), "2 selections");
}
