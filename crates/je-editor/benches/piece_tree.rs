//! Benchmark simples do piece tree num arquivo de 100 MB (meta do produto: abrir arquivo grande e rolar a
//! 60 fps). Roda com `cargo bench -p je-editor` (perfil release) e imprime os tempos por operação.
//!
//! O arquivo é ASCII, de linhas de 40 a 120 unidades (média perto de 80), com `\n`, entregue ao construtor em
//! pedaços de 64 KB como o VS Code lê um arquivo. A semente é fixa.

use std::fs;
use std::time::Instant;

use je_base::JsString;
use je_editor::model::{DefaultEndOfLine, ValidAnnotatedEditOperation};
use je_editor::piece_tree_text_buffer::PieceTreeTextBuffer;
use je_editor::piece_tree_text_buffer_builder::PieceTreeTextBufferBuilder;
use je_editor::range::Range;

const FILE_UNITS: usize = 100 * 1024 * 1024;
const CHUNK_UNITS: usize = 64 * 1024;

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

fn rss_mb() -> f64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with("VmRSS:"))
                .and_then(|line| line.split_whitespace().nth(1).and_then(|kb| kb.parse::<f64>().ok()))
        })
        .map_or(f64::NAN, |kb| kb / 1024.0)
}

fn report(name: &str, count: usize, start: Instant) {
    let elapsed = start.elapsed();
    let per_op_ns = elapsed.as_nanos() as f64 / count as f64;
    println!(
        "{name:<58} {count:>9} ops  {:>9.1} ms total  {per_op_ns:>10.0} ns/op",
        elapsed.as_secs_f64() * 1000.0
    );
}

fn make_file(rng: &mut Rng) -> Vec<u16> {
    let mut text: Vec<u16> = Vec::with_capacity(FILE_UNITS + 200);
    while text.len() < FILE_UNITS {
        let line_len = 40 + rng.below(81);
        for i in 0..line_len {
            text.push(u16::from(b'a') + ((i * 7 + text.len()) % 26) as u16);
        }
        text.push(10);
    }
    text.truncate(FILE_UNITS);
    text
}

fn touch_line(buffer: &PieceTreeTextBuffer, line_number: i32) -> usize {
    buffer.get_line_content(line_number).len()
}

fn main() {
    let mut rng = Rng(0xC0FFEE);
    let text = make_file(&mut rng);
    println!(
        "arquivo: {} unidades UTF-16 ({:.1} MB como texto ASCII, {:.1} MB na memória do buffer)",
        text.len(),
        text.len() as f64 / 1e6,
        text.len() as f64 * 2.0 / 1e6
    );

    let rss_before = rss_mb();
    let start = Instant::now();
    let mut builder = PieceTreeTextBufferBuilder::new();
    for chunk in text.chunks(CHUNK_UNITS) {
        builder.accept_chunk(chunk);
    }
    let mut buffer = builder.finish(true).create(DefaultEndOfLine::Lf);
    report("construir o buffer (accept_chunk x N, finish, create)", 1, start);
    drop(text);
    println!(
        "RSS: {:.0} MB com o texto de origem, {:.0} MB depois de liberá-lo (só o buffer)",
        rss_before,
        rss_mb()
    );

    let line_count = buffer.get_line_count();
    println!("linhas: {line_count}, peças na árvore: {}", buffer.get_piece_tree().tree().len());

    // getLineContent: acesso aleatório, a frio.
    let start = Instant::now();
    let mut sink = 0usize;
    let n = 200_000;
    for _ in 0..n {
        sink += touch_line(&buffer, 1 + rng.below(line_count) as i32);
    }
    report("getLineContent, linha aleatória (arquivo intacto)", n, start);

    // Rolagem: uma janela de 60 linhas consecutivas por quadro.
    let start = Instant::now();
    let frames = 20_000;
    for _ in 0..frames {
        let first = 1 + rng.below(line_count - 60);
        for line in first..first + 60 {
            sink += touch_line(&buffer, line as i32);
        }
    }
    report("rolagem: janela de 60 linhas (por quadro)", frames, start);

    // Varredura sequencial do arquivo inteiro.
    let start = Instant::now();
    for line in 1..=line_count {
        sink += touch_line(&buffer, line as i32);
    }
    report("getLineContent, varredura sequencial do arquivo", line_count, start);

    // getOffsetAt / getPositionAt.
    let start = Instant::now();
    let n = 200_000;
    for _ in 0..n {
        let line = 1 + rng.below(line_count) as i32;
        let offset = buffer.get_offset_at(line, 1);
        sink += buffer.get_position_at(offset as i64).line_number as usize;
    }
    report("getOffsetAt + getPositionAt, linha aleatória", n, start);

    // Inserção de um caractere em posições aleatórias, direto no piece tree.
    let length = buffer.get_length();
    let start = Instant::now();
    let n = 100_000;
    for _ in 0..n {
        let offset = rng.below(length);
        buffer.get_piece_tree_mut().insert(offset, &[u16::from(b'x')], true);
    }
    report("insert de 1 caractere, posição aleatória", n, start);

    // Digitação contínua numa posição só (cai no buffer de mudanças).
    let start = Instant::now();
    let n = 200_000;
    for offset in (length / 2..).take(n) {
        buffer.get_piece_tree_mut().insert(offset, &[u16::from(b'y')], true);
    }
    report("insert de 1 caractere, digitação contínua", n, start);

    // O caminho real de digitação: applyEdits com um intervalo vazio.
    let start = Instant::now();
    let n = 20_000;
    for _ in 0..n {
        let line = 1 + rng.below(buffer.get_line_count()) as i32;
        let range = Range::new(line, 1, line, 1);
        buffer.apply_edits(
            &[ValidAnnotatedEditOperation {
                identifier: None,
                range,
                text: Some(JsString::from("z")),
                force_move_markers: false,
                is_auto_whitespace_edit: false,
                is_tracked: false,
            }],
            false,
            false,
        );
    }
    report("applyEdits de 1 caractere (sem undo), linha aleatória", n, start);

    // Remoção de 5 caracteres em posições aleatórias.
    let start = Instant::now();
    let n = 50_000;
    for _ in 0..n {
        let offset = rng.below(buffer.get_length() - 10);
        buffer.get_piece_tree_mut().delete(offset, 5);
    }
    report("delete de 5 caracteres, posição aleatória", n, start);

    println!("peças na árvore depois das edições: {}", buffer.get_piece_tree().tree().len());

    // Depois das edições, a árvore tem muito mais peças.
    let line_count = buffer.get_line_count();
    let start = Instant::now();
    let n = 200_000;
    for _ in 0..n {
        sink += touch_line(&buffer, 1 + rng.below(line_count) as i32);
    }
    report("getLineContent, linha aleatória (depois das edições)", n, start);

    let start = Instant::now();
    for _ in 0..frames {
        let first = 1 + rng.below(line_count - 60);
        for line in first..first + 60 {
            sink += touch_line(&buffer, line as i32);
        }
    }
    report("rolagem: janela de 60 linhas (depois das edições)", frames, start);

    let start = Instant::now();
    let n = 200_000;
    for _ in 0..n {
        let line = 1 + rng.below(line_count) as i32;
        let offset = buffer.get_offset_at(line, 1);
        sink += buffer.get_position_at(offset as i64).line_number as usize;
    }
    report("getOffsetAt + getPositionAt (depois das edições)", n, start);

    println!("RSS ao fim: {:.0} MB (sink {sink})", rss_mb());
}
