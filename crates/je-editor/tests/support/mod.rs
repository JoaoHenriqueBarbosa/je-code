//! Auxiliares dos testes do buffer de texto: gerador com semente fixa, espelho em `Vec<u16>`, construção do
//! buffer, conferência das invariantes rubro-negras da árvore e das linhas.
#![allow(dead_code)]

pub mod editable_text_model_test_utils;
pub mod test_text_model;

use std::rc::Rc;

use je_base::JsString;
use je_base::strings;
use je_editor::js_reg_exp::JsRegExp;
use je_editor::model::{DefaultEndOfLine, SearchData, TextSnapshot, ValidAnnotatedEditOperation};
use je_editor::piece_tree_base::PieceTreeBase;
use je_editor::piece_tree_text_buffer::PieceTreeTextBuffer;
use je_editor::piece_tree_text_buffer_builder::PieceTreeTextBufferBuilder;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::rb_tree_base::{NodeColor, NodeId, SENTINEL, Tree};
use je_editor::word_character_classifier::WordCharacterClassifier;

use crate::common::LeakGuard;

pub const ALPHABET: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\r\n";

/// SplitMix64: o gerador com semente fixa no lugar do `Math.random`.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `Math.random()`: um número em `[0, 1)`.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `randomInt(bound)`.
    pub fn random_int(&mut self, bound: usize) -> usize {
        (self.next_f64() * bound as f64) as usize
    }
}

pub fn random_char(rng: &mut Rng) -> char {
    let alphabet: Vec<char> = ALPHABET.chars().collect();
    alphabet[rng.random_int(alphabet.len())]
}

/// `randomStr(len)`: o laço do original vai de 1 a `len - 1`, então são `len - 1` caracteres.
pub fn random_str(rng: &mut Rng, len: usize) -> String {
    (1..len).map(|_| random_char(rng)).collect()
}

pub fn u(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `str = str.substring(0, pos) + text + str.substring(pos)`.
pub fn ins(s: &mut Vec<u16>, pos: usize, text: &str) {
    s.splice(pos..pos, text.encode_utf16());
}

/// `str = str.substring(0, pos) + str.substring(pos + len)`.
pub fn del(s: &mut Vec<u16>, pos: usize, len: usize) {
    let end = (pos + len).min(s.len());
    s.drain(pos.min(end)..end);
}

pub fn assert_text(actual: &JsString, expected: &[u16]) {
    assert!(
        actual.units() == expected,
        "esperado {:?}, obtido {:?}",
        String::from_utf16_lossy(expected),
        actual.to_string_lossy()
    );
}

pub fn trim_line_feed(text: &[u16]) -> &[u16] {
    match text {
        [] => text,
        [10] | [13] => &[],
        [rest @ .., 13, 10] => rest,
        [rest @ .., 10] | [rest @ .., 13] => rest,
        _ => text,
    }
}

// #region Assertion

pub fn test_lines_content(str: &[u16], piece_table: &PieceTreeBase) {
    let lines = strings::split_lines(str);
    assert_eq!(piece_table.get_line_count(), lines.len());
    assert_text(&piece_table.get_lines_raw_content(), str);
    for (i, line) in lines.iter().enumerate() {
        let n = i as i32 + 1;
        assert_text(&piece_table.get_line_content(n), line);
        let end_column = line.len() as i32 + if i == lines.len() - 1 { 1 } else { 2 };
        let value = piece_table.get_value_in_range(&Range::new(n, 1, n, end_column), None);
        assert_eq!(trim_line_feed(value.units()), *line);
    }
}

pub fn test_line_starts(str: &[u16], piece_table: &PieceTreeBase) {
    // Os inícios de linha por uma varredura independente de `\r\n|\r|\n` (o laço do `RegExp` do original).
    let mut line_starts: Vec<usize> = vec![0];
    let mut i = 0;
    while i < str.len() {
        match str[i] {
            13 if str.get(i + 1) == Some(&10) => {
                i += 2;
                line_starts.push(i);
            }
            13 | 10 => {
                i += 1;
                line_starts.push(i);
            }
            _ => i += 1,
        }
    }

    for (i, &line_start) in line_starts.iter().enumerate() {
        assert_eq!(piece_table.get_position_at(line_start as i64), Position::new(i as i32 + 1, 1));
        assert_eq!(piece_table.get_offset_at(i as i32 + 1, 1), line_start);
    }

    for &line_start in &line_starts[1..] {
        let pos = piece_table.get_position_at(line_start as i64 - 1);
        assert_eq!(piece_table.get_offset_at(pos.line_number, pos.column), line_start - 1);
    }
}

pub fn create_text_buffer(ds: &LeakGuard, val: &[&str], normalize_eol: bool) -> PieceTreeTextBuffer {
    let mut buffer_builder = PieceTreeTextBufferBuilder::new();
    for chunk in val {
        buffer_builder.accept_chunk(&u(chunk));
    }
    let factory = buffer_builder.finish(normalize_eol);
    let buffer = factory.create(DefaultEndOfLine::Lf);
    ds.add(buffer.disposable());
    buffer
}

pub fn assert_tree_invariants(t: &PieceTreeBase) {
    let tree = t.tree();
    let sentinel = tree.node(SENTINEL);
    assert!(sentinel.color == NodeColor::Black);
    assert!(sentinel.parent == SENTINEL);
    assert!(sentinel.left == SENTINEL);
    assert!(sentinel.right == SENTINEL);
    assert!(sentinel.size_left == 0);
    assert!(sentinel.lf_left == 0);
    assert_valid_tree(t);
}

pub fn depth(tree: &Tree, n: NodeId) -> usize {
    if n == SENTINEL {
        // As folhas são pretas
        return 1;
    }
    let node = tree.node(n);
    assert!(depth(tree, node.left) == depth(tree, node.right));
    usize::from(node.color == NodeColor::Black) + depth(tree, node.left)
}

pub fn assert_valid_node(tree: &Tree, n: NodeId) -> (usize, usize) {
    if n == SENTINEL {
        return (0, 0);
    }

    let node = tree.node(n);
    let (l, r) = (node.left, node.right);

    if node.color == NodeColor::Red {
        assert!(tree.node(l).color == NodeColor::Black);
        assert!(tree.node(r).color == NodeColor::Black);
    }

    let actual_left = assert_valid_node(tree, l);
    assert!(actual_left.1 == node.lf_left);
    assert!(actual_left.0 == node.size_left);
    let actual_right = assert_valid_node(tree, r);

    (
        node.size_left + node.piece.length + actual_right.0,
        node.lf_left + node.piece.line_feed_cnt + actual_right.1,
    )
}

pub fn assert_valid_tree(t: &PieceTreeBase) {
    let tree = t.tree();
    if tree.root == SENTINEL {
        return;
    }
    let root = tree.node(tree.root);
    assert!(root.color == NodeColor::Black);
    assert!(depth(tree, root.left) == depth(tree, root.right));
    assert_valid_node(tree, tree.root);
}

// #endregion

pub fn search_data(pattern: &str, flags: &str, word_separators: Option<&str>, simple_search: Option<&str>) -> SearchData {
    SearchData::new(
        JsRegExp::from_str(pattern, flags).expect("regex válida"),
        word_separators.map(|separators| Rc::new(WordCharacterClassifier::new(&u(separators), Vec::new()))),
        simple_search.map(JsString::from),
    )
}

pub fn get_value_in_snapshot(snapshot: &mut dyn TextSnapshot) -> JsString {
    let mut ret = JsString::new();
    while let Some(tmp) = snapshot.read() {
        ret.push_units(tmp.units());
    }
    ret
}

/// `createTextModel(text)`: o buffer que o modelo usa, sem registrar o descarte (o caso o faz no fim).
pub fn create_model(_ds: &LeakGuard, text: &str) -> PieceTreeTextBuffer {
    let mut buffer_builder = PieceTreeTextBufferBuilder::new();
    buffer_builder.accept_chunk(&u(text));
    buffer_builder.finish(true).create(DefaultEndOfLine::Lf)
}

/// `model.applyEdits([{ range, text }])`.
pub fn apply_edit(model: &mut PieceTreeTextBuffer, range: Range, text: &str) {
    model.apply_edits(
        &[ValidAnnotatedEditOperation {
            identifier: None,
            range,
            text: Some(JsString::from(text)),
            force_move_markers: false,
            is_auto_whitespace_edit: false,
            is_tracked: false,
        }],
        false,
        false,
    );
}

/// `model.getLinesContent().join('\n')`.
pub fn lines_joined(model: &PieceTreeTextBuffer) -> JsString {
    let mut out = JsString::new();
    for (i, line) in model.get_lines_content().iter().enumerate() {
        if i > 0 {
            out.push_unit(10);
        }
        out.push_units(line.units());
    }
    out
}
