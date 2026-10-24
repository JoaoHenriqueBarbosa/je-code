//! O editor obedecendo ao `settings.json` e ao `keybindings.json` do usuário pela `EditorView`: indentação, fonte,
//! margem, quebra, opções que saem do arquivo voltando ao padrão, os participantes do salvamento e os atalhos do
//! usuário (do editor e os que ficam para o shell). Com `JE_SETTINGS_PNG_DIR=<pasta>` grava a captura de
//! `fontSize 18` com números relativos e régua na coluna 40, para a comparação com o oráculo.

mod support;

use std::time::Instant;

use je_editor::cursor_common::CursorSimpleModel;
use je_editor::position::Position;
use je_editor_view::folding::WordWrap;
use je_editor_view::{EditorKey, set_user_keybindings};
use je_render::{DisplayList, Rgba, render_rgba};
use je_ui::Modifiers;
use serde_json::{Map, Value, json};
use support::{VIEW_HEIGHT, VIEW_WIDTH, plain_view};

const NONE: Modifiers = Modifiers { alt: false, ctrl: false, shift: false, meta: false };
const CTRL_ALT: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, meta: false };

fn settings(value: Value) -> Map<String, Value> {
    value.as_object().expect("objeto").clone()
}

fn text(view: &je_editor_view::EditorView) -> String {
    view.model().get_value().to_string_lossy()
}

#[test]
fn indentation_comes_from_the_general_and_the_language_settings() {
    let mut v = plain_view("a\n");
    v.apply_configuration(&settings(json!({ "editor.detectIndentation": false, "editor.tabSize": 2 })));
    v.set_cursor(Position::new(2, 1));
    v.key_down(EditorKey::Tab, NONE);
    assert_eq!(text(&v), "a\n  ", "Tab insere o tabSize geral");
    let mut v = plain_view("a\n");
    v.apply_configuration(&settings(json!({ "editor.detectIndentation": false, "editor.tabSize": 2, "[plaintext]": { "editor.insertSpaces": false } })));
    v.set_cursor(Position::new(2, 1));
    v.key_down(EditorKey::Tab, NONE);
    assert_eq!(text(&v), "a\n\t", "o bloco da linguagem vence o geral");
}

#[test]
fn font_margin_and_minimap_follow_the_settings() {
    let mut v = plain_view("x\n");
    let default_left = v.layout().content_left();
    v.apply_configuration(&settings(json!({ "editor.fontSize": 18, "editor.minimap.enabled": false })));
    assert_eq!(v.layout().line_height(), 24.0, "round(18 * 1.35)");
    assert!(v.layout().content_left() > default_left, "a margem cresce com a fonte");
    assert_eq!(v.layout().text_right(), VIEW_WIDTH - 14.0, "sem minimapa o texto vai até a barra de rolagem");
    v.apply_configuration(&settings(json!({ "editor.lineNumbers": "off" })));
    assert_eq!(v.layout().line_numbers_width(), 0.0);
    assert_eq!(v.layout().line_height(), 19.0, "o fontSize que saiu do arquivo volta ao padrão");
}

#[test]
fn word_wrap_from_settings_and_back() {
    let mut v = plain_view(&"palavra ".repeat(300));
    v.apply_configuration(&settings(json!({ "editor.wordWrap": "on" })));
    assert_eq!(v.word_wrap(), WordWrap::On);
    let mut list = DisplayList::new();
    v.paint(&mut list, (0.0, 0.0), 1.0, Instant::now());
    assert!(v.layout().line_count > 1, "a linha longa quebrou");
    v.apply_configuration(&Map::new());
    assert_eq!(v.word_wrap(), WordWrap::Off);
}

#[test]
fn save_participants_trim_and_add_the_final_newline() {
    let mut v = plain_view("a  \nb\t\n\n\n");
    v.apply_configuration(&settings(json!({ "files.trimTrailingWhitespace": true, "files.trimFinalNewlines": true })));
    v.run_save_participants(false);
    assert_eq!(text(&v), "a\nb\n");
    let mut v = plain_view("a");
    v.apply_configuration(&settings(json!({ "files.insertFinalNewline": true })));
    v.run_save_participants(false);
    assert_eq!(text(&v), "a\n");
    v.undo();
    assert_eq!(text(&v), "a", "cada participante é uma edição que o desfazer volta");
    let mut v = plain_view("a  ");
    v.run_save_participants(false);
    assert_eq!(text(&v), "a  ", "sem a configuração nada muda");
}

#[test]
fn user_keybindings_reach_editor_commands_and_hand_the_rest_to_the_shell() {
    let mut v = plain_view("um\ndois\n");
    set_user_keybindings(
        r#"[
            { "key": "ctrl+alt+d", "command": "editor.action.deleteLines" },
            { "key": "ctrl+alt+p", "command": "workbench.action.quickOpen", "args": "x" },
        ]"#,
    )
    .expect("lê");
    assert!(v.key_down(EditorKey::Char('d'), CTRL_ALT));
    assert_eq!(text(&v), "dois\n", "o comando do editor rodou");
    assert!(v.key_down(EditorKey::Char('p'), CTRL_ALT));
    let (id, args) = v.take_workbench_command().expect("o comando do workbench ficou para o shell");
    assert_eq!(id, "workbench.action.quickOpen");
    assert!(matches!(&args, je_base::dynamic::Dynamic::String(s) if s.to_string_lossy() == "x"), "com os argumentos do atalho");
    assert!(v.take_workbench_command().is_none());
    set_user_keybindings("[]").expect("lê");
}

#[test]
fn relative_line_numbers_rulers_and_large_font_capture() {
    let Some(env) = support::environment() else { return };
    let mut v = support::sample_view(&env);
    v.apply_configuration(&settings(json!({ "editor.fontSize": 18, "editor.lineNumbers": "relative", "editor.rulers": [40] })));
    v.set_cursor(Position::new(9, 1));
    v.set_focus(false);
    let mut list = DisplayList::new();
    v.paint(&mut list, (0.0, 0.0), 1.0, Instant::now());
    let rgba = render_rgba(&list, Rgba::opaque(0, 0, 0), VIEW_WIDTH as u16, VIEW_HEIGHT as u16);
    if let Some(dir) = std::env::var_os("JE_SETTINGS_PNG_DIR") {
        let file = std::fs::File::create(std::path::Path::new(&dir).join("settings-relative.png")).expect("png");
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), VIEW_WIDTH as u32, VIEW_HEIGHT as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().and_then(|mut w| w.write_image_data(&rgba)).expect("png");
    }
    // A régua: uma coluna de pixels com a cor de `editorRuler.foreground` na altura inteira.
    let x = (v.layout().content_left() + 40.0 * v.layout().metrics.char_width).round() as usize;
    let at = |y: usize| &rgba[(y * VIEW_WIDTH as usize + x) * 4..(y * VIEW_WIDTH as usize + x) * 4 + 3];
    assert_eq!(at(VIEW_HEIGHT as usize - 2), at(5), "a régua vai do topo ao fim");
    assert_ne!(at(VIEW_HEIGHT as usize - 2), &rgba[(600 * VIEW_WIDTH as usize + 900) * 4..(600 * VIEW_WIDTH as usize + 900) * 4 + 3]);
}
