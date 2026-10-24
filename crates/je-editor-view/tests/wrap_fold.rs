//! A quebra de linha (`editor.wordWrap`) e a dobra de código pela `EditorView`: as linhas da view vêm do
//! `ViewModel`, o cursor anda nelas (End para no fim da linha visual, o clique na segunda linha visual cai depois
//! da quebra) e as regiões dobradas somem da view (o cursor pula a região, o chevron e os atalhos dobram e
//! desdobram). Com `JE_WRAP_PNG_DIR=<pasta>` grava as capturas para a comparação com o oráculo.

mod support;

use std::time::Instant;

use je_editor::position::Position;
use je_editor_view::EditorKey;
use je_editor_view::folding::WordWrap;
use je_render::{DisplayList, Rgba, render_rgba};
use je_ui::{Modifiers, PointerButton};
use support::{VIEW_HEIGHT, VIEW_WIDTH, plain_view};

const NONE: Modifiers = Modifiers { alt: false, ctrl: false, shift: false, meta: false };
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, meta: false };
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, meta: false };

fn render(view: &mut je_editor_view::EditorView, name: &str) -> Vec<u8> {
    let mut list = DisplayList::new();
    view.paint(&mut list, (0.0, 0.0), 1.0, Instant::now());
    let rgba = render_rgba(&list, Rgba::opaque(0, 0, 0), VIEW_WIDTH as u16, VIEW_HEIGHT as u16);
    if let Some(dir) = std::env::var_os("JE_WRAP_PNG_DIR") {
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        let file = std::fs::File::create(path).expect("png");
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), VIEW_WIDTH as u32, VIEW_HEIGHT as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().and_then(|mut w| w.write_image_data(&rgba)).expect("png");
    }
    rgba
}

fn position(view: &je_editor_view::EditorView) -> (i32, i32) {
    let p = view.cursor_position();
    (p.line_number, p.column)
}

/// Uma linha de 300 colunas (`a0 a1 a2 ...`) entre duas curtas.
fn long_text() -> String {
    let words: Vec<String> = (0..80).map(|i| format!("w{i:02}")).collect();
    format!("first\n{}\nlast", words.join(" "))
}

#[test]
fn word_wrap_splits_the_long_line_into_view_lines() {
    let mut v = plain_view(&long_text());
    render(&mut v, "wrap-off");
    assert_eq!(v.layout().line_count, 3, "sem quebra cada linha do modelo é uma linha da view");
    v.set_word_wrap(WordWrap::On);
    render(&mut v, "wrap-on");
    let view_lines = v.layout().line_count;
    assert!(view_lines > 3, "com quebra a linha longa ocupa várias linhas da view: {view_lines}");
    // End na linha longa para no fim da primeira linha visual; o segundo End vai ao fim da linha do modelo.
    v.set_cursor(Position::new(2, 1));
    v.key_down(EditorKey::End, NONE);
    let (line, column) = position(&v);
    assert_eq!(line, 2);
    assert!(column > 1 && column < 320, "End parou na quebra: coluna {column}");
    v.key_down(EditorKey::End, NONE);
    assert_eq!(position(&v), (2, 320), "o segundo End vai ao fim da linha do modelo");
    // Down na primeira linha visual vai para a segunda linha visual da mesma linha do modelo.
    v.set_cursor(Position::new(2, 1));
    v.key_down(EditorKey::Down, NONE);
    let (line, column) = position(&v);
    assert_eq!(line, 2, "a seta para baixo fica na mesma linha do modelo");
    assert!(column > 1, "e passa da quebra: coluna {column}");
    // O clique na segunda linha visual (terceira linha da view) cai depois da quebra, na mesma linha do modelo.
    let layout = v.layout().clone();
    let y = 2.0 * layout.line_height() + layout.line_height() / 2.0;
    v.mouse_down(layout.content_left() + 4.0 * layout.metrics.char_width, y, PointerButton::Primary, 1, NONE);
    v.mouse_up();
    let (line, column) = position(&v);
    assert_eq!(line, 2);
    assert!(column > 4, "o clique na linha visual quebrada vai para depois da quebra: coluna {column}");
    // Desligar volta às três linhas.
    v.toggle_word_wrap();
    render(&mut v, "wrap-off-again");
    assert_eq!(v.layout().line_count, 3);
    assert_eq!(v.word_wrap(), WordWrap::Off);
}

#[test]
fn wrapped_lines_have_one_line_number() {
    let mut v = plain_view(&long_text());
    v.set_word_wrap(WordWrap::On);
    let rgba = render(&mut v, "wrap-numbers");
    // A coluna dos números de linha (x de 19 a 44): a linha 2 tem tinta, as continuações não.
    let lh = v.layout().line_height() as usize;
    let ink = |row: usize| (row * lh + 3..row * lh + lh - 3).any(|y| (19..44).any(|x| rgba[(y * VIEW_WIDTH as usize + x) * 4] > 60));
    assert!(ink(0) && ink(1), "as linhas 1 e 2 têm número");
    assert!(!ink(2), "a continuação da linha 2 não tem número");
}

const FOLDABLE: &str = "function f() {\n    let a = 1;\n    let b = 2;\n}\nafter();\nfunction g() {\n    return 3;\n}\n";

#[test]
fn fold_and_unfold_hide_the_region_and_the_cursor_jumps_over_it() {
    let mut v = plain_view(FOLDABLE);
    render(&mut v, "fold-before");
    v.set_cursor(Position::new(1, 1));
    v.key_down(EditorKey::Char('['), CTRL_SHIFT);
    render(&mut v, "fold-folded");
    assert_eq!(v.layout().line_count, 7, "as duas linhas de dentro da função somem da view");
    v.key_down(EditorKey::Down, NONE);
    assert_eq!(position(&v).0, 4, "a seta para baixo pula a região dobrada");
    v.set_cursor(Position::new(1, 1));
    v.key_down(EditorKey::Char(']'), CTRL_SHIFT);
    render(&mut v, "fold-unfolded");
    assert_eq!(v.layout().line_count, 9);
    v.key_down(EditorKey::Down, NONE);
    assert_eq!(position(&v).0, 2);
}

#[test]
fn chevron_click_and_fold_all_chords() {
    let mut v = plain_view(FOLDABLE);
    render(&mut v, "chevron-before");
    let layout = v.layout().clone();
    let chevron_x = layout.line_numbers_left() + layout.line_numbers_width() + 15.0;
    // O chevron da linha 6 (`function g`).
    let y = 5.0 * layout.line_height() + layout.line_height() / 2.0;
    v.mouse_move(chevron_x, y);
    render(&mut v, "chevron-hover");
    v.mouse_down(chevron_x, y, PointerButton::Primary, 1, NONE);
    v.mouse_up();
    render(&mut v, "chevron-folded");
    assert_eq!(v.layout().line_count, 8, "o clique no chevron dobra a função g");
    v.mouse_down(chevron_x, y, PointerButton::Primary, 1, NONE);
    v.mouse_up();
    render(&mut v, "chevron-unfolded");
    assert_eq!(v.layout().line_count, 9, "o segundo clique desdobra");
    // Ctrl+K Ctrl+0 dobra tudo; Ctrl+K Ctrl+J desdobra tudo.
    v.key_down(EditorKey::Char('k'), CTRL);
    v.key_down(EditorKey::Char('0'), CTRL);
    render(&mut v, "fold-all");
    assert_eq!(v.layout().line_count, 6);
    v.key_down(EditorKey::Char('k'), CTRL);
    v.key_down(EditorKey::Char('j'), CTRL);
    render(&mut v, "unfold-all");
    assert_eq!(v.layout().line_count, 9);
}

#[test]
fn going_to_a_hidden_line_unfolds_it() {
    let mut v = plain_view(FOLDABLE);
    render(&mut v, "reveal-before");
    v.fold_all();
    render(&mut v, "reveal-folded");
    assert_eq!(v.layout().line_count, 6);
    // A busca e o ir para linha revelam o alvo escondido.
    v.select_range(je_editor::range::Range::new(3, 5, 3, 8));
    render(&mut v, "reveal-after");
    assert_eq!(position(&v), (3, 8));
    assert_eq!(v.layout().line_count, 8, "só a região de f desdobra (a de g segue dobrada)");
}

/// A região dobrada do arquivo de exemplo, com as medidas do oráculo (VS Code 1.143.1, tema 2026 Dark, editor de 921
/// px): o fundo `editor.foldBackground` (a seleção a 30%) na linha 4, o chevron à direita dos números e a reticência
/// depois do `{`.
#[test]
fn folded_region_matches_the_oracle() {
    let Some(env) = support::environment() else {
        eprintln!("extensões embutidas ausentes: dobra do exemplo não conferida");
        return;
    };
    let model = env.open("sample.ts", support::SAMPLE);
    {
        use je_editor::cursor_common::CursorSimpleModel;
        model.tokenization().force_tokenization(model.get_line_count());
    }
    let mut v = je_editor_view::EditorView::new(model, 921.0, 677.0, support::colors());
    v.set_token_colors(&env.color_map());
    render(&mut v, "probe-sample0");
    assert!(v.toggle_fold_at(4, false), "a interface Point começa uma região");
    v.set_cursor(Position::new(1, 1));
    let rgba = render(&mut v, "probe-sample-folded");
    let pixel = |x: usize, y: usize| {
        let i = (y * VIEW_WIDTH as usize + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2]]
    };
    // A linha da view 4 é a linha 4 do modelo (de 4 a 6, as linhas 5 e 6 somem).
    let y = (3.0 * v.layout().line_height() + 9.0) as usize;
    // O alfa vira byte (66 de 255) e a mistura sai até 1 nível acima da do Chromium.
    let near = pixel(700, y).iter().zip([23u8, 41, 48]).all(|(a, b)| a.abs_diff(b) <= 1);
    assert!(near, "o fundo da linha dobrada é o do oráculo: {:?}", pixel(700, y));
    let ink = |from: usize, to: usize| (from..to).any(|x| pixel(x, y).iter().map(|c| u32::from(*c)).sum::<u32>() > 250);
    assert!(ink(55, 64), "o chevron aparece mesmo sem o ponteiro na margem");
    assert!(ink(215, 225), "a reticência fica depois do `{{`, como no oráculo (x de 217 a 223)");
    assert_eq!(v.layout().line_count, 27);
}
