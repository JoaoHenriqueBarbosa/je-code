//! A captura: o `render_rgba` pinta a view de um arquivo TypeScript e o teste confere pixels conhecidos
//! (fundo, número de linha, palavra-chave pelo tema, realce da linha, cursor, guia de indentação), sem
//! comparar o PNG inteiro. Os valores esperados foram lidos do oráculo (VS Code 1.143.1, tema 2026 Dark).
//!
//! Precisa das extensões embutidas (`ref/vscode/extensions`, fora do git): sem elas o teste avisa e passa.
//! Com `JE_CAPTURE_PNG=<arquivo>` grava também o PNG da view, para o `oracle-harness/imgdiff.py` (H26).

mod support;

use std::time::Instant;

use je_render::{DisplayList, Rgba, render_rgba};
use support::{SAMPLE, VIEW_HEIGHT, VIEW_WIDTH, colors, environment, sample_view};

fn pixel(rgba: &[u8], x: usize, y: usize) -> [u8; 3] {
    let i = (y * VIEW_WIDTH as usize + x) * 4;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

fn near(a: [u8; 3], b: [u8; 3], tolerance: i32) -> bool {
    a.iter().zip(&b).all(|(x, y)| (i32::from(*x) - i32::from(*y)).abs() <= tolerance)
}

/// Algum pixel do retângulo tem (quase) a cor dada.
fn has_color(rgba: &[u8], rect: (usize, usize, usize, usize), color: [u8; 3], tolerance: i32) -> bool {
    (rect.1..rect.3).any(|y| (rect.0..rect.2).any(|x| near(pixel(rgba, x, y), color, tolerance)))
}

fn render(view: &mut je_editor_view::EditorView, now: Instant) -> Vec<u8> {
    let mut list = DisplayList::new();
    view.paint(&mut list, (0.0, 0.0), 1.0, now);
    render_rgba(&list, Rgba::opaque(0, 0, 0), VIEW_WIDTH as u16, VIEW_HEIGHT as u16)
}

#[test]
fn typescript_capture_matches_theme() {
    let Some(env) = environment() else {
        eprintln!("extensões embutidas ausentes (JE_BUILTIN_EXTENSIONS ou ref/vscode/extensions): captura não conferida");
        return;
    };
    let mut view = sample_view(&env);
    assert_eq!(SAMPLE.lines().count(), 28);
    view.reset_blink();
    let t0 = Instant::now();
    let rgba = render(&mut view, t0);
    if let Some(path) = std::env::var_os("JE_CAPTURE_PNG") {
        write_png(&path, &rgba);
    }
    let c = colors();
    let rgb = |c: Rgba| [c.r, c.g, c.b];

    // Fundo do editor numa região vazia (abaixo do texto, à direita).
    assert_eq!(pixel(&rgba, 700, 640), rgb(c.background));
    assert_eq!(pixel(&rgba, 700, 640), [0x12, 0x13, 0x14]);

    // Realce da linha do cursor (linha 1, y de 0 a 19): só no conteúdo; a margem e a linha 2 ficam no fundo.
    assert_eq!(pixel(&rgba, 600, 5), [0x24, 0x25, 0x26]);
    assert_eq!(pixel(&rgba, 5, 5), [0x12, 0x13, 0x14]);
    assert_eq!(pixel(&rgba, 600, 25), [0x12, 0x13, 0x14], "a linha 2 não é realçada");

    // Cursor: barra de 2 px, na cor do tema, no início do conteúdo (x 70).
    assert_eq!(pixel(&rgba, 70, 5), [0xBB, 0xBE, 0xBF]);
    assert_eq!(pixel(&rgba, 71, 5), [0xBB, 0xBE, 0xBF]);
    assert_eq!(pixel(&rgba, 72, 5), [0x24, 0x25, 0x26]);

    // Número de linha: ativo na linha 1 (#BBBEBF), apagado na linha 2 (#858889).
    assert!(has_color(&rgba, (19, 0, 44, 19), [0xBB, 0xBE, 0xBF], 6), "número da linha ativa");
    assert!(has_color(&rgba, (19, 19, 44, 38), [0x85, 0x88, 0x89], 6), "número da linha 2");
    assert!(!has_color(&rgba, (19, 19, 44, 38), [0xBB, 0xBE, 0xBF], 6), "a linha 2 não é ativa");

    // Palavras-chave pelo tema: `import` (linha 2) e `export` (linha 9) em keyword.control (#C586C0).
    assert!(has_color(&rgba, (70, 19, 120, 38), [0xC5, 0x86, 0xC0], 4), "import");
    assert!(has_color(&rgba, (70, 152, 125, 171), [0xC5, 0x86, 0xC0], 4), "export");
    // O comentário da linha 1 não tem a cor do texto comum.
    assert!(!has_color(&rgba, (80, 0, 400, 19), [0xBB, 0xBE, 0xBF], 0), "comentário");

    // Guia de indentação (#8384854D sobre o fundo) na linha 5 (y de 76 a 95), na borda do conteúdo.
    // (o oráculo dá 52, 53, 53; a mistura do vello_cpu arredonda um nível acima).
    assert!(near(pixel(&rgba, 70, 80), [52, 53, 53], 1), "guia: {:?}", pixel(&rgba, 70, 80));

    // Quando o cursor está na fase escura do piscar, a barra some e sobra o realce.
    let hidden = render(&mut view, t0 + std::time::Duration::from_millis(600));
    assert_eq!(pixel(&hidden, 70, 5), [0x24, 0x25, 0x26]);
}

fn write_png(path: &std::ffi::OsStr, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), VIEW_WIDTH as u32, VIEW_HEIGHT as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().expect("cabeçalho").write_image_data(rgba).expect("dados");
}

/// Seleção de tudo (Ctrl+A): a cor da seleção sobre o fundo, a quebra de linha como uma célula a mais, a linha
/// vazia com o seu bloco, e os cantos externos arredondados. `JE_CAPTURE_PNG_SELECTION` grava o PNG.
#[test]
fn select_all_capture() {
    let Some(env) = environment() else {
        eprintln!("extensões embutidas ausentes: captura não conferida");
        return;
    };
    let mut view = sample_view(&env);
    view.select_all();
    view.reset_blink();
    let rgba = render(&mut view, Instant::now());
    if let Some(path) = std::env::var_os("JE_CAPTURE_PNG_SELECTION") {
        write_png(&path, &rgba);
    }
    // #276782dd sobre #121314: o oráculo mede (36, 92, 115) no meio da seleção. O ponto (377, 9) é a célula da
    // quebra da linha 1, depois do texto (36 colunas de 8,43 px a partir de x 70), sem glifo por cima.
    let inside = pixel(&rgba, 377, 9);
    assert!(near(inside, [36, 92, 115], 1), "célula da quebra da linha 1: {inside:?}");
    assert_eq!(pixel(&rgba, 70 + 40 * 8, 9), [0x12, 0x13, 0x14], "a seleção termina uma célula depois do texto");
    // A linha 3 está vazia: só a célula da quebra (8 px) fica selecionada, e o resto da linha não.
    assert!(near(pixel(&rgba, 74, 2 * 19 + 9), [36, 92, 115], 1), "célula da quebra na linha vazia");
    assert_eq!(pixel(&rgba, 200, 2 * 19 + 9), [0x12, 0x13, 0x14], "o resto da linha vazia fica de fora");
    // O canto superior esquerdo da primeira linha é arredondado: o oráculo mede (19, 25, 29) no pixel do canto.
    assert!(pixel(&rgba, 70, 0)[1] < 40, "canto arredondado: {:?}", pixel(&rgba, 70, 0));
}

/// A tokenização em fatias do loop do runtime avança e a view pede um quadro novo a cada chegada de tokens: com o
/// arquivo repetido 60 vezes (1 680 linhas, longe do alcance da tokenização barata) as últimas linhas só ficam
/// com tokens exatos depois das fatias, e o `import` da linha 2 continua com a cor de `keyword.control`.
#[test]
fn sliced_tokens_request_redraw() {
    let Some(env) = environment() else {
        eprintln!("extensões embutidas ausentes: não conferido");
        return;
    };
    let mut view = support::lazy_sample_view(&env, 60);
    view.reset_blink();
    let model = view.model().clone();
    let last = model.get_line_count();
    render(&mut view, Instant::now());
    assert!(!model.tokenization().has_accurate_tokens_for_line(last), "no começo só o trecho visível tem tokens exatos");
    view.take_dirty();
    je_base::runtime::run_until_idle();
    assert!(model.tokenization().has_accurate_tokens_for_line(last), "as fatias terminaram o arquivo todo");
    assert!(view.take_dirty(), "a chegada dos tokens pede um quadro novo");
    let rgba = render(&mut view, Instant::now());
    assert!(has_color(&rgba, (70, 19, 120, 38), [0xC5, 0x86, 0xC0], 4), "import colorido");
}

/// Em tela com escala 2 as medidas dobram: o conteúdo começa em x 140, o cursor tem 4 px e a linha 38.
#[test]
fn scale_two_capture() {
    let Some(env) = environment() else {
        eprintln!("extensões embutidas ausentes: não conferido");
        return;
    };
    let mut view = sample_view(&env);
    view.resize(300.0, 200.0);
    view.reset_blink();
    let mut list = DisplayList::new();
    view.paint(&mut list, (0.0, 0.0), 2.0, Instant::now());
    let rgba = render_rgba(&list, Rgba::opaque(0, 0, 0), 600, 400);
    let px = |x: usize, y: usize| [rgba[(y * 600 + x) * 4], rgba[(y * 600 + x) * 4 + 1], rgba[(y * 600 + x) * 4 + 2]];
    assert_eq!(px(140, 10), [0xBB, 0xBE, 0xBF], "cursor de 4 px de largura");
    assert_eq!(px(143, 10), [0xBB, 0xBE, 0xBF]);
    assert_eq!(px(144, 10), [0x24, 0x25, 0x26], "realce da linha 1, que tem 38 px de altura");
    assert_eq!(px(300, 37), [0x24, 0x25, 0x26]);
    assert_eq!(px(300, 39), [0x12, 0x13, 0x14]);
    // O texto está pintado em tamanho dobrado: a palavra-chave `import` ocupa de x 140 a 140 + 6 * 16,86 px.
    assert!((140..250).any(|x| (40..76).any(|y| near(px(x, y), [0xC5, 0x86, 0xC0], 4))), "import na escala 2");
}
