//! Benchmark das decorações do `TextModel` num arquivo de ~100 MB. Roda com
//! `cargo bench -p je-editor --bench text_model_decorations` (perfil release) e imprime mediana e mínimo de
//! várias execuções de cada cenário.
//!
//! Cenários:
//! - (a) `deltaDecorations` criando 10 000 decorações espalhadas pelo arquivo, e depois trocando as 10 000
//!   por outras 10 000 (remove e recria na mesma chamada).
//! - (b) `getLinesDecorations` e `getLineDecorations` linha a linha numa janela de 60 linhas no meio do
//!   arquivo, com as 10 000 decorações presentes.
//! - (c) o mesmo (b) logo depois de um `applyEdits` no começo do arquivo, o que obriga a árvore de intervalos
//!   a reaplicar o deslocamento pendente (`acceptReplace` + `resolveNode`) nos nós visitados.
//!
//! O arquivo é ASCII, de linhas de 80 a 120 unidades, com `\n`, entregue ao construtor em pedaços de 64 KB.
//! A semente é fixa.

use std::hint::black_box;
use std::rc::Rc;
use std::time::{Duration, Instant};

use je_editor::edit_operation::EditOperation;
use je_editor::model::{ModelDecorationOptionsInit, ModelDeltaDecoration};
use je_editor::piece_tree_text_buffer_builder::PieceTreeTextBufferBuilder;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::{ModelDecorationOptions, TextModel, TextSource};
use je_editor::undo_redo_service::UndoRedoService;

const FILE_UNITS: usize = 100 * 1024 * 1024;
const CHUNK_UNITS: usize = 64 * 1024;
const DECORATIONS: usize = 10_000;
const WINDOW_LINES: i32 = 60;
const RUNS: usize = 9;

struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

fn make_file(rng: &mut Rng) -> Vec<u16> {
    let mut text: Vec<u16> = Vec::with_capacity(FILE_UNITS + 200);
    while text.len() < FILE_UNITS {
        let line_len = 80 + rng.below(41);
        for i in 0..line_len {
            text.push(u16::from(b'a') + ((i * 7 + text.len()) % 26) as u16);
        }
        text.push(10);
    }
    text
}

/// Mediana e mínimo das amostras, em microssegundos, por operação (`ops` operações por amostra).
fn report(name: &str, samples: &mut [Duration], ops: usize) {
    samples.sort();
    let per_op_us = |d: Duration| d.as_secs_f64() * 1e6 / ops as f64;
    println!(
        "{name:<72} mediana {:>10.2} us  mínimo {:>10.2} us  ({} amostras de {ops} op)",
        per_op_us(samples[samples.len() / 2]),
        per_op_us(samples[0]),
        samples.len(),
    );
}

/// 10 000 decorações espalhadas: começo numa linha aleatória, às vezes atravessando algumas linhas.
fn random_decorations(rng: &mut Rng, model: &TextModel, options: &Rc<ModelDecorationOptions>) -> Vec<ModelDeltaDecoration> {
    let line_count = model.get_line_count() as usize;
    (0..DECORATIONS)
        .map(|_| {
            let start_line = 1 + rng.below(line_count) as i32;
            let start_column = 1 + rng.below(80) as i32;
            let end_line = (start_line + if rng.below(4) == 0 { rng.below(5) as i32 } else { 0 }).min(line_count as i32);
            let end_column = if end_line == start_line { start_column + rng.below(20) as i32 } else { 1 + rng.below(80) as i32 };
            ModelDeltaDecoration {
                range: Range::new(start_line, start_column, end_line, end_column),
                options: options.clone(),
            }
        })
        .collect()
}

fn main() {
    let mut rng = Rng(0xDEC0);

    let start = Instant::now();
    let text = make_file(&mut rng);
    println!("gerar o texto: {:.1} ms ({} unidades UTF-16)", start.elapsed().as_secs_f64() * 1e3, text.len());

    let start = Instant::now();
    let mut builder = PieceTreeTextBufferBuilder::new();
    for chunk in text.chunks(CHUNK_UNITS) {
        builder.accept_chunk(chunk);
    }
    let factory = builder.finish(true);
    drop(text);
    let model = TextModel::new(
        TextSource::Factory(factory),
        "plaintext",
        TextModel::default_creation_options(),
        None,
        UndoRedoService::for_tests(),
    );
    println!(
        "criar o modelo (builder + TextModel::new): {:.1} ms, {} linhas",
        start.elapsed().as_secs_f64() * 1e3,
        model.get_line_count()
    );

    let options = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: "bench".to_owned(),
        class_name: Some("bench".to_owned()),
        ..Default::default()
    });

    // (a) criar 10 000 e trocar as 10 000.
    let mut create = Vec::new();
    let mut replace = Vec::new();
    let mut remove = Vec::new();
    for _ in 0..RUNS {
        let first = random_decorations(&mut rng, &model, &options);
        let second = random_decorations(&mut rng, &model, &options);

        let start = Instant::now();
        let ids = model.delta_decorations(&[], &first, 0);
        create.push(start.elapsed());

        let start = Instant::now();
        let ids = model.delta_decorations(&ids, &second, 0);
        replace.push(start.elapsed());

        let start = Instant::now();
        model.delta_decorations(&ids, &[], 0);
        remove.push(start.elapsed());
    }
    report("(a) deltaDecorations([], 10 000 novas)", &mut create, 1);
    report("(a) deltaDecorations(10 000 antigas, 10 000 novas)", &mut replace, 1);
    report("(a) deltaDecorations(10 000 antigas, [])", &mut remove, 1);

    // (b) janela de 60 linhas no meio do arquivo, com as 10 000 presentes.
    let decorations = random_decorations(&mut rng, &model, &options);
    let ids = model.delta_decorations(&[], &decorations, 0);
    assert_eq!(model.get_all_decorations(0, false, false).len(), DECORATIONS);
    let middle = model.get_line_count() / 2;
    let window_end = middle + WINDOW_LINES - 1;
    let found = model.get_lines_decorations(middle, window_end, 0, false, false, false).len();
    println!("decorações na janela de {WINDOW_LINES} linhas a partir da linha {middle}: {found}");

    let calls = 20_000;
    let mut samples = Vec::new();
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..calls {
            black_box(model.get_lines_decorations(middle, window_end, 0, false, false, false));
        }
        samples.push(start.elapsed());
    }
    report("(b) getLinesDecorations, janela de 60 linhas", &mut samples, calls);

    let frames = 2_000;
    let mut samples = Vec::new();
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..frames {
            for line in middle..=window_end {
                black_box(model.get_line_decorations(line, 0, false, false));
            }
        }
        samples.push(start.elapsed());
    }
    report("(b) getLineDecorations x 60 linhas (por janela)", &mut samples, frames);

    // (c) depois de uma edição no começo do arquivo.
    let edits = 2_000;
    let mut edit_only = Vec::new();
    let mut first_query = Vec::new();
    let mut warm_query = Vec::new();
    for _ in 0..RUNS {
        let mut edit_time = Duration::ZERO;
        let mut first_time = Duration::ZERO;
        let mut warm_time = Duration::ZERO;
        for _ in 0..edits {
            let start = Instant::now();
            model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "x")]);
            edit_time += start.elapsed();

            let start = Instant::now();
            black_box(model.get_lines_decorations(middle, window_end, 0, false, false, false));
            first_time += start.elapsed();

            let start = Instant::now();
            black_box(model.get_lines_decorations(middle, window_end, 0, false, false, false));
            warm_time += start.elapsed();
        }
        edit_only.push(edit_time);
        first_query.push(first_time);
        warm_query.push(warm_time);
    }
    report("(c) applyEdits de 1 caractere em (1,1), 10 000 decorações", &mut edit_only, edits);
    report("(c) getLinesDecorations logo depois da edição (resolve os nós)", &mut first_query, edits);
    report("(c) getLinesDecorations de novo, sem edição no meio", &mut warm_query, edits);

    // As decorações seguem no lugar: a janela tem as mesmas, e o total não mudou.
    assert_eq!(model.get_lines_decorations(middle, window_end, 0, false, false, false).len(), found);
    model.delta_decorations(&ids, &[], 0);
    model.dispose();
}
