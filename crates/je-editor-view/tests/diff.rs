//! O editor de diff lado a lado: a geometria do oráculo (VS Code 1.143.1, janela de 1142 px de editor), o
//! alinhamento das linhas pelos espaços hachurados, as decorações, a rolagem sincronizada, a entrada por lado e o
//! hachurado pixel a pixel. As posições dos espaços saem da captura do oráculo com o mesmo par de arquivos.

mod support;

use std::time::Instant;

use je_editor::cursor_common::CursorSimpleModel;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::undo_redo_service::UndoRedoService;
use je_editor_view::clipboard::Clipboard;
use je_editor_view::diff::{Alignment, compute_alignments, decorations_of, zones_of};
use je_editor_view::{DiffColors, DiffEditor, DiffLayout, EditorKey, EditorView, Side};
use je_render::{DisplayList, Rgba, render_rgba};
use je_theme::ColorTheme;
use je_ui::{Modifiers, PointerButton};

const NONE: Modifiers = Modifiers { alt: false, ctrl: false, shift: false, meta: false };

const ORIGINAL: &str = "import { readFile } from \"fs\";\n\nfunction add(a: number, b: number): number {\n    return a + b;\n}\n\nconst items = [1, 2, 3];\nconsole.log(items.map(x => x * 2));\n\nfunction removed() {\n    return \"gone\";\n}\n\nexport class Counter {\n    private count = 0;\n    inc() { this.count++; }\n}\n";
const MODIFIED: &str = "import { readFile } from \"fs\";\nimport { join } from \"path\";\n\nfunction add(a: number, b: number): number {\n    const sum = a + b + 1;\n    return sum;\n}\n\nconst items = [1, 2, 3, 4];\nconsole.log(items.map(x => x * 2));\n\nexport class Counter {\n    private count = 0;\n    inc() { this.count += 1; }\n    dec() { this.count -= 1; }\n}\n";

fn model(text: &str) -> std::rc::Rc<TextModel> {
    TextModel::new(TextSource::from(text), "plaintext", TextModel::default_creation_options(), None, UndoRedoService::for_tests())
}

fn theme() -> ColorTheme {
    ColorTheme::load(&support::theme_path()).expect("tema")
}

/// O editor de diff de `original` contra `modified` na área de 1142 x 677 do oráculo.
fn diff_of(original: &str, modified: &str) -> (DiffEditor, EditorView) {
    let mut view = EditorView::new(model(modified), 556.0, 677.0, support::colors());
    view.set_clipboard(Clipboard::internal_only());
    let mut diff = DiffEditor::new(model(original), &mut view, 1142.0, 677.0, support::colors(), DiffColors::from_theme(&theme()));
    diff.original_mut().set_clipboard(Clipboard::internal_only());
    (diff, view)
}

fn paint(diff: &mut DiffEditor, view: &mut EditorView) -> DisplayList {
    let mut list = DisplayList::new();
    diff.paint(view, &mut list, (0.0, 0.0), 1.0, Instant::now());
    list
}

#[test]
fn layout_matches_the_oracle_dom() {
    // O DOM do oráculo: original 521 px, calha de 35 px, modificado a partir de 556 (1 px de borda) e a régua de 30 px.
    let layout = DiffLayout::new(1142.0, 677.0);
    assert_eq!((layout.original.x0, layout.original.width()), (0.0, 521.0));
    assert_eq!((layout.gutter.x0, layout.gutter.width()), (521.0, 35.0));
    assert_eq!((layout.border.x0, layout.border.width()), (556.0, 1.0));
    assert_eq!((layout.modified.x0, layout.modified.width()), (557.0, 556.0));
    assert_eq!((layout.overview.x0, layout.overview.width()), (1112.0, 30.0));
}

#[test]
fn alignment_gives_the_oracle_zones() {
    let (mut diff, mut view) = diff_of(ORIGINAL, MODIFIED);
    paint(&mut diff, &mut view);
    assert_eq!(diff.mappings().len(), 5);
    // No oráculo o original tem hachurado de uma linha depois das linhas 1, 3 e 16 e o modificado de quatro linhas
    // depois da linha 10 (as quatro linhas da função removida).
    let original: Vec<(i32, f64)> = diff.original().layout().zones().map(|z| (z.after_line, z.height)).collect();
    let modified: Vec<(i32, f64)> = view.layout().zones().map(|z| (z.after_line, z.height)).collect();
    assert_eq!(original, vec![(1, 19.0), (3, 19.0), (16, 19.0)]);
    assert_eq!(modified, vec![(10, 76.0)]);
    // As alturas totais do texto se igualam: o alinhamento deixa o fim dos dois lados na mesma altura.
    let (o, m) = (diff.original().layout(), view.layout());
    assert_eq!(f64::from(o.line_count) * 19.0 + o.zones_height(), f64::from(m.line_count) * 19.0 + m.zones_height());
}

#[test]
fn pure_insert_and_delete_align_without_inner_changes() {
    let original = model("a\nb\nc\n");
    let modified = model("a\nx\ny\nb\nc\n");
    let diff = je_editor::diff::default_lines_diff_computer::default_lines_diff_computer::DefaultLinesDiffComputer::new();
    use je_editor::diff::lines_diff_computer::{ILinesDiffComputer, LinesDiffComputerOptions};
    let options = LinesDiffComputerOptions { ignore_trim_whitespace: true, max_computation_time_ms: 1000.0, compute_moves: false, extend_to_subwords: None };
    let result = diff.compute_diff(&original.get_lines_content(), &modified.get_lines_content(), &options);
    let alignments = compute_alignments(&result.changes, original.get_line_count(), &|l| original.get_line_max_column(l));
    assert_eq!(alignments, vec![Alignment { original: (2, 2), modified: (2, 4) }]);
    let (original_zones, modified_zones) = zones_of(&alignments, 19.0);
    assert_eq!(original_zones.iter().map(|z| (z.after_line, z.height)).collect::<Vec<_>>(), vec![(1, 38.0)]);
    assert!(modified_zones.is_empty());
    let ((original_lines, _), (modified_lines, _)) = decorations_of(&result.changes, &DiffColors::from_theme(&theme()));
    assert!(original_lines.is_empty(), "nada removido");
    // Um fundo da linha e outro, mais forte, da linha inteira inserida.
    assert_eq!(modified_lines.len(), 2);
    assert_eq!((modified_lines[0].start_line, modified_lines[0].end_line), (2, 3));
}

#[test]
fn both_sides_scroll_together() {
    let long = |change: &str| (1..=120).map(|n| if n == 100 { change.to_owned() } else { format!("line {n}") }).collect::<Vec<_>>().join("\n");
    let (mut diff, mut view) = diff_of(&long("old"), &long("new\nnew 2"));
    paint(&mut diff, &mut view);
    // Abrir revela a primeira mudança (linha 100), com os dois lados no mesmo ponto.
    assert_eq!(view.scroll_top(), diff.original().scroll_top());
    view.set_scroll_top(0.0);
    paint(&mut diff, &mut view);
    assert_eq!(diff.original().scroll_top(), 0.0);
    // A roda sobre o modificado leva o original junto no quadro seguinte.
    diff.scroll_by(&mut view, 800.0, 0.0, 400.0);
    paint(&mut diff, &mut view);
    assert_eq!(view.scroll_top(), 400.0);
    assert_eq!(diff.original().scroll_top(), 400.0);
    // E sobre o original, o modificado acompanha.
    diff.scroll_by(&mut view, 100.0, 0.0, 190.0);
    paint(&mut diff, &mut view);
    assert_eq!(diff.original().scroll_top(), 590.0);
    assert_eq!(view.scroll_top(), 590.0);
}

#[test]
fn the_original_is_read_only_and_focus_follows_the_click() {
    let (mut diff, mut view) = diff_of("um\ndois\n", "um\ndois\ntres\n");
    paint(&mut diff, &mut view);
    // O cursor do modificado já foi para a primeira mudança (linha 3).
    assert_eq!(view.cursor_position().line_number, 3);
    assert_eq!(diff.focused_side(), Side::Modified);
    // Clicar no original o foca; digitar não edita o texto dele.
    diff.mouse_down(&mut view, 200.0, 30.0, PointerButton::Primary, 1, NONE);
    diff.mouse_up(&mut view);
    assert_eq!(diff.focused_side(), Side::Original);
    diff.type_text(&mut view, "X");
    diff.key_down(&mut view, EditorKey::Backspace, NONE);
    assert_eq!(diff.original().model().get_value().to_string_lossy(), "um\ndois\n");
    // Clicar no modificado devolve o foco e digitar edita.
    diff.mouse_down(&mut view, 700.0, 30.0, PointerButton::Primary, 1, NONE);
    diff.mouse_up(&mut view);
    assert_eq!(diff.focused_side(), Side::Modified);
    diff.type_text(&mut view, "Y");
    assert!(view.model().get_value().to_string_lossy().contains('Y'));
    // O diff se refaz com o texto novo: o clique caiu na linha 2 (y = 30), que agora difere.
    paint(&mut diff, &mut view);
    assert_eq!(diff.mappings()[0].modified.start_line_number, 2);
}

/// O hachurado do oráculo: o pixel é da listra quando `(x - início do conteúdo) + (y - topo do espaço)` módulo 8 é 6 ou 7.
#[test]
fn hatch_pixels_follow_the_oracle_rule() {
    let (mut diff, mut view) = diff_of("a\n", "a\nb\n");
    let colors = DiffColors::from_theme(&theme());
    let list = paint(&mut diff, &mut view);
    let background = support::colors().background;
    let rgba = render_rgba(&list, background, 1142, 677);
    let layout = DiffLayout::new(1142.0, 677.0);
    // O original tem um espaço de 19 px depois da linha 1 (topo em 19) que começa no conteúdo (13 + ...).
    let content_left = diff.original().layout().content_left();
    let pixel = |x: u32, y: u32| {
        let i = ((y * 1142 + x) * 4) as usize;
        Rgba::new(rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3])
    };
    let fill = pixel(0, 0);
    assert_eq!(fill, background);
    let mut stripe = 0;
    let mut blank = 0;
    for v in 0..19u32 {
        for u in 0..64u32 {
            let p = pixel(content_left as u32 + u, 19 + v);
            let expected = (u + v) % 8 >= 6;
            if expected {
                stripe += 1;
                assert_ne!(p, background, "u={u} v={v} devia ser da listra");
            } else {
                blank += 1;
                assert_eq!(p, background, "u={u} v={v} devia ser o fundo");
            }
        }
    }
    assert!(stripe > 0 && blank > stripe);
    let _ = (layout, colors);
}

/// Com `JE_DIFF_PNG_DIR=<pasta>` grava a captura do diff com os tokens coloridos (1142 x 677), para a comparação com
/// a captura do oráculo (`oracle-harness/run-oracle.sh 600 --diff a.ts b.ts`).
#[test]
fn colored_diff_capture() {
    let Some(env) = support::environment() else { return };
    let open = |text: &str| {
        let model = env.open("a.ts", text);
        model.tokenization().force_tokenization(model.get_line_count());
        model
    };
    let mut view = EditorView::new(open(MODIFIED), 556.0, 677.0, support::colors());
    view.set_token_colors(&env.color_map());
    view.set_clipboard(Clipboard::internal_only());
    let mut diff = DiffEditor::new(open(ORIGINAL), &mut view, 1142.0, 677.0, support::colors(), DiffColors::from_theme(&theme()));
    diff.set_token_colors(&env.color_map());
    let list = paint(&mut diff, &mut view);
    let rgba = render_rgba(&list, support::colors().background, 1142, 677);
    assert_eq!(rgba.len(), 1142 * 677 * 4);
    if let Some(dir) = std::env::var_os("JE_DIFF_PNG_DIR") {
        let file = std::fs::File::create(std::path::Path::new(&dir).join("diff-colored.png")).expect("png");
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 1142, 677);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().and_then(|mut w| w.write_image_data(&rgba)).expect("png");
    }
}
