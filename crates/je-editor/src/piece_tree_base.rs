//! Porte de `src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeBase.ts`: o piece tree, a estrutura
//! de dados do buffer de texto do VS Code.
//!
//! O texto vive em buffers (o buffer 0 é o de mudanças, os demais são os pedaços do arquivo original, de
//! até 65535 unidades); a árvore rubro-negra guarda "peças" (`Piece`), cada uma um trecho de um buffer
//! com o comprimento e a quantidade de quebras de linha, e cada nó soma os dois valores da subárvore
//! esquerda. Isso dá `getOffsetAt`, `getPositionAt` e `getLineContent` em O(log n) e inserção e remoção
//! sem mover o texto.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - A árvore é uma arena com índices (ver `rb_tree_base`) e o sentinela é o nó 0.
//! - Textos são `&[u16]`/[`JsString`] (UTF-16, como o `string` do JavaScript); os buffers de linhas
//!   iniciais são `Vec<u32>` (o original escolhe `Uint16Array` ou `Uint32Array` por tamanho).
//! - Os buffers ficam num `Rc<RefCell<..>>` porque o snapshot os lê depois, na hora do `read()`, como o
//!   original lê `tree._buffers`; o cache de busca e a última linha visitada ficam em `RefCell`, para que
//!   `getLineContent` e afins sejam `&self` como leituras.
//! - `nodeAt`/`nodeAt2` que devolvem `null` e depois são desreferenciados (um `TypeError` no JS) viram
//!   `errors::throw(TypeError)`; a retirada de `resultLen` do `findMatches*` usa `result.len()`.
//! - Linhas e colunas são `i32` na API (como `Position`) e `isize` por dentro, onde o original deixa a
//!   conta passar de zero.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::char_code;
use je_base::errors::{self, Error};
use je_base::js_string;

use crate::model::{Eol, FindMatch, SearchData, TextSnapshot};
use crate::position::Position;
use crate::range::Range;
use crate::rb_tree_base::{DETACHED, NodeId, SENTINEL, Tree};
use crate::text_model_search::{Searcher, create_find_match, is_valid_match};

/// `AverageBufferSize`: o tamanho dos pedaços em que um texto grande é partido.
pub const AVERAGE_BUFFER_SIZE: usize = 65535;

/// Os resultados do cursor de uma linha e coluna dentro de um buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BufferCursor {
    /// Linha dentro do buffer.
    pub line: usize,
    /// Coluna dentro da linha do buffer.
    pub column: usize,
}

/// `Piece`: um trecho imutável de um buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Piece {
    pub buffer_index: usize,
    pub start: BufferCursor,
    pub end: BufferCursor,
    pub line_feed_cnt: usize,
    pub length: usize,
}

impl Piece {
    pub fn new(buffer_index: usize, start: BufferCursor, end: BufferCursor, line_feed_cnt: usize, length: usize) -> Self {
        Piece {
            buffer_index,
            start,
            end,
            line_feed_cnt,
            length,
        }
    }
}

/// `StringBuffer`: o texto de um buffer e os deslocamentos em que cada linha começa.
#[derive(Clone, Debug, Default)]
pub struct StringBuffer {
    pub buffer: Vec<u16>,
    pub line_starts: Vec<u32>,
}

impl StringBuffer {
    pub fn new(buffer: Vec<u16>, line_starts: Vec<u32>) -> Self {
        Self { buffer, line_starts }
    }

    /// Um buffer cujas linhas iniciais são calculadas agora (`createLineStartsFast`).
    pub fn from_text(buffer: Vec<u16>) -> Self {
        let line_starts = create_line_starts_fast(&buffer);
        Self { buffer, line_starts }
    }

    #[inline]
    fn line_start(&self, line: usize) -> usize {
        self.line_starts[line] as usize
    }
}

/// O resultado de [`create_line_starts`].
#[derive(Clone, Debug)]
pub struct LineStarts {
    pub line_starts: Vec<u32>,
    pub cr: usize,
    pub lf: usize,
    pub crlf: usize,
    pub is_basic_ascii: bool,
}

/// `createLineStartsFast(str)`: os deslocamentos em que cada linha começa (`\r\n`, `\r` e `\n` quebram).
pub fn create_line_starts_fast(str: &[u16]) -> Vec<u32> {
    let mut r: Vec<u32> = vec![0];
    let len = str.len();
    let mut i = 0;
    while i < len {
        let chr = str[i];
        if chr == char_code::CarriageReturn {
            if i + 1 < len && str[i + 1] == char_code::LineFeed {
                // caso \r\n
                r.push((i + 2) as u32);
                i += 1; // pula o \n
            } else {
                // caso \r
                r.push((i + 1) as u32);
            }
        } else if chr == char_code::LineFeed {
            r.push((i + 1) as u32);
        }
        i += 1;
    }
    r
}

/// `createLineStarts(r, str)`: como a rápida, e conta os tipos de quebra e se o texto é ASCII básico.
pub fn create_line_starts(str: &[u16]) -> LineStarts {
    let mut r: Vec<u32> = vec![0];
    let (mut cr, mut lf, mut crlf) = (0, 0, 0);
    let mut is_basic_ascii = true;
    let len = str.len();
    let mut i = 0;
    while i < len {
        let chr = str[i];
        if chr == char_code::CarriageReturn {
            if i + 1 < len && str[i + 1] == char_code::LineFeed {
                crlf += 1;
                r.push((i + 2) as u32);
                i += 1; // pula o \n
            } else {
                cr += 1;
                r.push((i + 1) as u32);
            }
        } else if chr == char_code::LineFeed {
            lf += 1;
            r.push((i + 1) as u32);
        } else if is_basic_ascii && chr != char_code::Tab && !(32..=126).contains(&chr) {
            is_basic_ascii = false;
        }
        i += 1;
    }
    LineStarts {
        line_starts: r,
        cr,
        lf,
        crlf,
        is_basic_ascii,
    }
}

/// `text.replace(/\r\n|\r|\n/g, eol)`.
pub fn replace_line_breaks(text: &[u16], eol: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let chr = text[i];
        if chr == char_code::CarriageReturn {
            if text.get(i + 1) == Some(&char_code::LineFeed) {
                i += 1;
            }
            out.extend_from_slice(eol);
        } else if chr == char_code::LineFeed {
            out.extend_from_slice(eol);
        } else {
            out.push(chr);
        }
        i += 1;
    }
    out
}

/// `text.replace(/(\r\n|\r|\n)$/, '')`: tira uma quebra de linha do fim.
fn strip_trailing_line_break(text: &[u16]) -> &[u16] {
    match text {
        [rest @ .., 13, 10] => rest,
        [rest @ .., 10] | [rest @ .., 13] => rest,
        _ => text,
    }
}

#[inline]
fn sub(buffer: &[u16], start: usize, end: usize) -> &[u16] {
    js_string::substring(buffer, start as i64, end as i64)
}

/// `NodePosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodePosition {
    /// O nó (a peça).
    pub node: NodeId,
    /// O resto dentro da peça.
    pub remainder: usize,
    /// O deslocamento do início do nó no documento.
    pub node_start_offset: usize,
}

fn type_error(message: &str) -> ! {
    errors::throw(Error::type_error(message))
}

/// O snapshot só de leitura: guarda as peças do momento da criação e lê o conteúdo delas dos buffers
/// vivos, como o original (que lê `tree._buffers` no `read()`).
pub struct PieceTreeSnapshot {
    pieces: Vec<Piece>,
    index: usize,
    buffers: Rc<RefCell<Vec<StringBuffer>>>,
    bom: JsString,
}

impl PieceTreeSnapshot {
    fn new(tree: &PieceTreeBase, bom: &[u16]) -> Self {
        let pieces = tree.nodes_in_order().map(|node| tree.tree.node(node).piece).collect();
        PieceTreeSnapshot {
            pieces,
            index: 0,
            buffers: tree.buffers.clone(),
            bom: JsString::from(bom),
        }
    }
}

impl TextSnapshot for PieceTreeSnapshot {
    fn read(&mut self) -> Option<JsString> {
        if self.pieces.is_empty() {
            if self.index == 0 {
                self.index += 1;
                return Some(self.bom.clone());
            }
            return None;
        }

        if self.index > self.pieces.len() - 1 {
            return None;
        }

        let content = piece_content(&self.buffers.borrow(), &self.pieces[self.index]);
        let first = self.index == 0;
        self.index += 1;
        if first {
            let mut with_bom = self.bom.clone();
            with_bom.push_units(&content);
            return Some(with_bom);
        }
        Some(JsString::from_units(content))
    }
}

fn piece_content(buffers: &[StringBuffer], piece: &Piece) -> Vec<u16> {
    let buffer = &buffers[piece.buffer_index];
    let start_offset = buffer.line_start(piece.start.line) + piece.start.column;
    let end_offset = buffer.line_start(piece.end.line) + piece.end.column;
    sub(&buffer.buffer, start_offset, end_offset).to_vec()
}

#[derive(Clone, Copy, Debug)]
struct CacheEntry {
    node: NodeId,
    node_start_offset: usize,
    node_start_line_number: Option<usize>,
}

/// `PieceTreeSearchCache`: os últimos nós achados por deslocamento e por linha.
#[derive(Debug)]
struct PieceTreeSearchCache {
    limit: usize,
    cache: Vec<CacheEntry>,
}

impl PieceTreeSearchCache {
    fn new(limit: usize) -> Self {
        Self { limit, cache: Vec::new() }
    }

    fn get(&self, tree: &Tree, offset: usize) -> Option<CacheEntry> {
        self.cache
            .iter()
            .rev()
            .find(|entry| entry.node_start_offset <= offset && entry.node_start_offset + tree.node(entry.node).piece.length >= offset)
            .copied()
    }

    fn get2(&self, tree: &Tree, line_number: usize) -> Option<(CacheEntry, usize)> {
        self.cache.iter().rev().find_map(|entry| match entry.node_start_line_number {
            Some(start_line)
                if start_line != 0 && start_line < line_number && start_line + tree.node(entry.node).piece.line_feed_cnt >= line_number =>
            {
                Some((*entry, start_line))
            }
            _ => None,
        })
    }

    fn set(&mut self, node_position: CacheEntry) {
        if self.cache.len() >= self.limit {
            self.cache.remove(0);
        }
        self.cache.push(node_position);
    }

    fn validate(&mut self, tree: &Tree, offset: usize) {
        self.cache
            .retain(|entry| tree.node(entry.node).parent != DETACHED && entry.node_start_offset < offset);
    }
}

/// O piece tree.
pub struct PieceTreeBase {
    tree: Tree,
    /// 0 é o buffer de mudanças; os demais são os buffers originais, só de leitura.
    buffers: Rc<RefCell<Vec<StringBuffer>>>,
    line_cnt: usize,
    length: usize,
    eol: Eol,
    eol_length: usize,
    eol_normalized: bool,
    last_change_buffer_pos: BufferCursor,
    search_cache: RefCell<PieceTreeSearchCache>,
    last_visited_line: RefCell<(usize, JsString)>,
}

impl PieceTreeBase {
    pub fn new(chunks: Vec<StringBuffer>, eol: Eol, eol_normalized: bool) -> Self {
        let mut this = PieceTreeBase {
            tree: Tree::new(),
            buffers: Rc::new(RefCell::new(Vec::new())),
            line_cnt: 1,
            length: 0,
            eol,
            eol_length: eol.len(),
            eol_normalized,
            last_change_buffer_pos: BufferCursor::default(),
            search_cache: RefCell::new(PieceTreeSearchCache::new(1)),
            last_visited_line: RefCell::new((0, JsString::new())),
        };
        this.create(chunks, eol, eol_normalized);
        this
    }

    /// `create`: recomeça a árvore sobre `chunks`.
    pub fn create(&mut self, chunks: Vec<StringBuffer>, eol: Eol, eol_normalized: bool) {
        let mut buffers = vec![StringBuffer::new(Vec::new(), vec![0])];
        self.last_change_buffer_pos = BufferCursor::default();
        self.tree.clear();
        self.line_cnt = 1;
        self.length = 0;
        self.eol = eol;
        self.eol_length = eol.len();
        self.eol_normalized = eol_normalized;

        let mut last_node: Option<NodeId> = None;
        for (i, chunk) in chunks.into_iter().enumerate() {
            if !chunk.buffer.is_empty() {
                let last_line = chunk.line_starts.len() - 1;
                let piece = Piece::new(
                    i + 1,
                    BufferCursor { line: 0, column: 0 },
                    BufferCursor {
                        line: last_line,
                        column: chunk.buffer.len() - chunk.line_starts[last_line] as usize,
                    },
                    last_line,
                    chunk.buffer.len(),
                );
                buffers.push(chunk);
                last_node = Some(self.rb_insert_right(last_node, piece));
            }
        }

        *self.buffers.borrow_mut() = buffers;
        self.search_cache = RefCell::new(PieceTreeSearchCache::new(1));
        self.last_visited_line = RefCell::new((0, JsString::new()));
        self.compute_buffer_metadata();
    }

    /// Os nós em ordem, do primeiro ao último (o `iterate` do original sem os sentinelas).
    fn nodes_in_order(&self) -> impl Iterator<Item = NodeId> + '_ {
        let first = if self.tree.root == SENTINEL {
            SENTINEL
        } else {
            self.tree.leftest(self.tree.root)
        };
        std::iter::successors((first != SENTINEL).then_some(first), move |&node| {
            let next = self.tree.next(node);
            (next != SENTINEL).then_some(next)
        })
    }

    pub fn normalize_eol(&mut self, eol: Eol) {
        let min = AVERAGE_BUFFER_SIZE - AVERAGE_BUFFER_SIZE / 3;
        let max = min * 2;

        let mut temp_chunk: Vec<u16> = Vec::new();
        let mut chunks: Vec<StringBuffer> = Vec::new();

        let flush = |temp_chunk: &[u16], chunks: &mut Vec<StringBuffer>| {
            chunks.push(StringBuffer::from_text(replace_line_breaks(temp_chunk, eol.as_units())));
        };

        for node in self.nodes_in_order().collect::<Vec<_>>() {
            let content = self.get_node_content(node);
            let len = content.len();
            if temp_chunk.len() <= min || temp_chunk.len() + len < max {
                temp_chunk.extend_from_slice(&content);
                continue;
            }

            // descarrega de qualquer jeito
            flush(&temp_chunk, &mut chunks);
            temp_chunk = content;
        }

        if !temp_chunk.is_empty() {
            flush(&temp_chunk, &mut chunks);
        }

        self.create(chunks, eol, true);
    }

    // #region Buffer API
    pub fn get_eol(&self) -> Eol {
        self.eol
    }

    pub fn set_eol(&mut self, new_eol: Eol) {
        self.eol = new_eol;
        self.eol_length = new_eol.len();
        self.normalize_eol(new_eol);
    }

    pub fn create_snapshot(&self, bom: &[u16]) -> PieceTreeSnapshot {
        PieceTreeSnapshot::new(self, bom)
    }

    /// A raiz da árvore (o sentinela se estiver vazia).
    pub fn root(&self) -> NodeId {
        self.tree.root
    }

    /// A árvore, para inspeção (os testes conferem as invariantes rubro-negras).
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn equal(&self, other: &PieceTreeBase) -> bool {
        if self.get_length() != other.get_length() {
            return false;
        }
        if self.get_line_count() != other.get_line_count() {
            return false;
        }

        let mut offset = 0;
        for node in self.nodes_in_order() {
            let str = self.get_node_content(node);
            let len = str.len();
            let start_position = other.node_at(offset);
            let end_position = other.node_at(offset + len);
            let val = other.get_value_in_range2(&start_position, &end_position);

            offset += len;
            if str != val.units() {
                return false;
            }
        }
        true
    }

    pub fn get_offset_at(&self, line_number: i32, column: i32) -> usize {
        let mut left_len = 0usize; // em ordem
        let mut line_number = line_number as isize;

        let mut x = self.tree.root;

        while x != SENTINEL {
            let n = *self.tree.node(x);
            if n.left != SENTINEL && n.lf_left as isize + 1 >= line_number {
                x = n.left;
            } else if (n.lf_left + n.piece.line_feed_cnt) as isize + 1 >= line_number {
                left_len += n.size_left;
                // lineNumber >= 2
                let accumulated_val_in_current_index = self.get_accumulated_value(x, line_number - n.lf_left as isize - 2);
                return (left_len as isize + accumulated_val_in_current_index as isize + column as isize - 1) as usize;
            } else {
                line_number -= (n.lf_left + n.piece.line_feed_cnt) as isize;
                left_len += n.size_left + n.piece.length;
                x = n.right;
            }
        }

        left_len
    }

    pub fn get_position_at(&self, offset: i64) -> Position {
        let mut offset = offset.max(0) as usize;

        let mut x = self.tree.root;
        let mut lf_cnt = 0usize;
        let original_offset = offset;

        while x != SENTINEL {
            let n = *self.tree.node(x);
            if n.size_left != 0 && n.size_left >= offset {
                x = n.left;
            } else if n.size_left + n.piece.length >= offset {
                let (index, remainder) = self.get_index_of(x, offset - n.size_left);

                lf_cnt += n.lf_left + index;

                if index == 0 {
                    let line_start_offset = self.get_offset_at((lf_cnt + 1) as i32, 1);
                    let column = original_offset - line_start_offset;
                    return Position::new((lf_cnt + 1) as i32, (column + 1) as i32);
                }

                return Position::new((lf_cnt + 1) as i32, (remainder + 1) as i32);
            } else {
                offset -= n.size_left + n.piece.length;
                lf_cnt += n.lf_left + n.piece.line_feed_cnt;

                if n.right == SENTINEL {
                    // último nó
                    let line_start_offset = self.get_offset_at((lf_cnt + 1) as i32, 1);
                    let column = original_offset - offset - line_start_offset;
                    return Position::new((lf_cnt + 1) as i32, (column + 1) as i32);
                }
                x = n.right;
            }
        }

        Position::new(1, 1)
    }

    pub fn get_value_in_range(&self, range: &Range, eol: Option<Eol>) -> JsString {
        if range.start_line_number == range.end_line_number && range.start_column == range.end_column {
            return JsString::new();
        }

        let start_position = self
            .node_at2(range.start_line_number, range.start_column)
            .unwrap_or_else(|| type_error("nodeAt2"));
        let end_position = self
            .node_at2(range.end_line_number, range.end_column)
            .unwrap_or_else(|| type_error("nodeAt2"));

        let value = self.get_value_in_range2(&start_position, &end_position);
        match eol {
            Some(eol) if eol != self.eol || !self.eol_normalized => {
                JsString::from_units(replace_line_breaks(value.units(), eol.as_units()))
            }
            _ => value,
        }
    }

    pub fn get_value_in_range2(&self, start_position: &NodePosition, end_position: &NodePosition) -> JsString {
        let buffers = self.buffers.borrow();
        let piece_of = |node: NodeId| self.tree.node(node).piece;

        if start_position.node == end_position.node {
            let piece = piece_of(start_position.node);
            let buffer = &buffers[piece.buffer_index];
            let start_offset = buffer.line_start(piece.start.line) + piece.start.column;
            return JsString::from(sub(
                &buffer.buffer,
                start_offset + start_position.remainder,
                start_offset + end_position.remainder,
            ));
        }

        let mut x = start_position.node;
        let piece = piece_of(x);
        let buffer = &buffers[piece.buffer_index];
        let start_offset = buffer.line_start(piece.start.line) + piece.start.column;
        let mut ret: Vec<u16> = sub(&buffer.buffer, start_offset + start_position.remainder, start_offset + piece.length).to_vec();

        x = self.tree.next(x);
        while x != SENTINEL {
            let piece = piece_of(x);
            let buffer = &buffers[piece.buffer_index];
            let start_offset = buffer.line_start(piece.start.line) + piece.start.column;

            if x == end_position.node {
                ret.extend_from_slice(sub(&buffer.buffer, start_offset, start_offset + end_position.remainder));
                break;
            }
            ret.extend_from_slice(sub(&buffer.buffer, start_offset, start_offset + piece.length));

            x = self.tree.next(x);
        }

        JsString::from_units(ret)
    }

    pub fn get_lines_content(&self) -> Vec<JsString> {
        let buffers = self.buffers.borrow();
        let mut lines: Vec<JsString> = Vec::new();
        let mut current_line: Vec<u16> = Vec::new();
        let mut dangling_cr = false;

        for node in self.nodes_in_order() {
            let piece = self.tree.node(node).piece;
            let mut piece_length = piece.length;
            if piece_length == 0 {
                continue;
            }

            let buffer_ref = &buffers[piece.buffer_index];
            let buffer = &buffer_ref.buffer;

            let piece_start_line = piece.start.line;
            let piece_end_line = piece.end.line;
            let mut piece_start_offset = buffer_ref.line_start(piece_start_line) + piece.start.column;

            if dangling_cr {
                if buffer.get(piece_start_offset) == Some(&char_code::LineFeed) {
                    // finge que o \n estava na peça anterior
                    piece_start_offset += 1;
                    piece_length -= 1;
                }
                lines.push(JsString::from_units(std::mem::take(&mut current_line)));
                dangling_cr = false;
                if piece_length == 0 {
                    continue;
                }
            }

            if piece_start_line == piece_end_line {
                // esta peça não tem quebra de linha
                if !self.eol_normalized && buffer.get(piece_start_offset + piece_length - 1) == Some(&char_code::CarriageReturn) {
                    dangling_cr = true;
                    current_line.extend_from_slice(sub(buffer, piece_start_offset, piece_start_offset + piece_length - 1));
                } else {
                    current_line.extend_from_slice(sub(buffer, piece_start_offset, piece_start_offset + piece_length));
                }
                continue;
            }

            // acrescenta o texto antes do primeiro início de linha desta peça
            let first_line_end = buffer_ref.line_start(piece_start_line + 1);
            if self.eol_normalized {
                let end = piece_start_offset.max(first_line_end - self.eol_length);
                current_line.extend_from_slice(sub(buffer, piece_start_offset, end));
            } else {
                current_line.extend_from_slice(strip_trailing_line_break(sub(buffer, piece_start_offset, first_line_end)));
            }
            lines.push(JsString::from_units(std::mem::take(&mut current_line)));

            for line in piece_start_line + 1..piece_end_line {
                let line_start = buffer_ref.line_start(line);
                let line_end = buffer_ref.line_start(line + 1);
                let text = if self.eol_normalized {
                    sub(buffer, line_start, line_end - self.eol_length)
                } else {
                    strip_trailing_line_break(sub(buffer, line_start, line_end))
                };
                lines.push(JsString::from(text));
            }

            let end_line_start = buffer_ref.line_start(piece_end_line);
            let last_is_cr = (end_line_start + piece.end.column)
                .checked_sub(1)
                .and_then(|index| buffer.get(index))
                == Some(&char_code::CarriageReturn);
            if !self.eol_normalized && last_is_cr {
                dangling_cr = true;
                if piece.end.column == 0 {
                    // A última linha terminava em \r: desfaz o push, a próxima iteração o faz.
                    current_line = lines.pop().map(|line| line.units().to_vec()).unwrap_or_default();
                } else {
                    current_line = sub(buffer, end_line_start, end_line_start + piece.end.column - 1).to_vec();
                }
            } else {
                current_line = sub(buffer, end_line_start, end_line_start + piece.end.column).to_vec();
            }
        }

        if dangling_cr {
            lines.push(JsString::from_units(std::mem::take(&mut current_line)));
        }

        lines.push(JsString::from_units(current_line));
        lines
    }

    pub fn get_length(&self) -> usize {
        self.length
    }

    pub fn get_line_count(&self) -> usize {
        self.line_cnt
    }

    pub fn get_line_content(&self, line_number: i32) -> JsString {
        let line_number_usize = line_number as usize;
        {
            let last = self.last_visited_line.borrow();
            if last.0 == line_number_usize {
                return last.1.clone();
            }
        }

        let value = if line_number_usize == self.line_cnt {
            self.get_line_raw_content(line_number, 0)
        } else if self.eol_normalized {
            self.get_line_raw_content(line_number, self.eol_length)
        } else {
            JsString::from(strip_trailing_line_break(self.get_line_raw_content(line_number, 0).units()))
        };

        *self.last_visited_line.borrow_mut() = (line_number_usize, value.clone());
        value
    }

    fn char_code_of_node_position(&self, node_pos: &NodePosition) -> Option<u16> {
        let buffers = self.buffers.borrow();
        let piece = self.tree.node(node_pos.node).piece;
        if node_pos.remainder == piece.length {
            // o caractere que queremos está no início do próximo nó
            let matching_node = self.tree.next(node_pos.node);
            if matching_node == SENTINEL {
                // No original o sentinela não tem `piece` e o acesso lança `TypeError`.
                type_error("Cannot read properties of null (reading 'bufferIndex')");
            }

            let matching_piece = self.tree.node(matching_node).piece;
            let buffer = &buffers[matching_piece.buffer_index];
            let start_offset = buffer.line_start(matching_piece.start.line) + matching_piece.start.column;
            buffer.buffer.get(start_offset).copied()
        } else {
            let buffer = &buffers[piece.buffer_index];
            let start_offset = buffer.line_start(piece.start.line) + piece.start.column;
            buffer.buffer.get(start_offset + node_pos.remainder).copied()
        }
    }

    /// `getLineCharCode`: `None` é o `NaN` do original (fora do texto).
    pub fn get_line_char_code(&self, line_number: i32, index: i32) -> Option<u16> {
        let node_pos = self.node_at2(line_number, index + 1).unwrap_or_else(|| type_error("nodeAt2"));
        self.char_code_of_node_position(&node_pos)
    }

    pub fn get_line_length(&self, line_number: i32) -> usize {
        if line_number as usize == self.get_line_count() {
            let start_offset = self.get_offset_at(line_number, 1);
            return self.get_length() - start_offset;
        }
        // Num buffer sem EOL normalizado, com quebras de comprimentos diferentes, o original pode dar um
        // comprimento negativo (linha vazia terminada em `\n` com EOL `\r\n`); aqui isso vira zero.
        let length =
            self.get_offset_at(line_number + 1, 1) as isize - self.get_offset_at(line_number, 1) as isize - self.eol_length as isize;
        length.max(0) as usize
    }

    /// `getCharCode`: `None` é o `NaN` do original.
    pub fn get_char_code(&self, offset: usize) -> Option<u16> {
        let node_pos = self.node_at(offset);
        self.char_code_of_node_position(&node_pos)
    }

    pub fn get_nearest_chunk(&self, offset: usize) -> JsString {
        let node_pos = self.node_at(offset);
        let buffers = self.buffers.borrow();
        let piece = self.tree.node(node_pos.node).piece;
        if node_pos.remainder == piece.length {
            // o deslocamento está no início do próximo nó
            let matching_node = self.tree.next(node_pos.node);
            if matching_node == SENTINEL {
                return JsString::new();
            }

            let matching_piece = self.tree.node(matching_node).piece;
            let buffer = &buffers[matching_piece.buffer_index];
            let start_offset = buffer.line_start(matching_piece.start.line) + matching_piece.start.column;
            JsString::from(sub(&buffer.buffer, start_offset, start_offset + matching_piece.length))
        } else {
            let buffer = &buffers[piece.buffer_index];
            let start_offset = buffer.line_start(piece.start.line) + piece.start.column;
            let target_offset = start_offset + node_pos.remainder;
            let target_end = start_offset + piece.length;
            JsString::from(sub(&buffer.buffer, target_offset, target_end))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn find_matches_in_node(
        &self,
        node: NodeId,
        searcher: &mut Searcher,
        start_line_number: i32,
        start_column: i32,
        start_cursor: BufferCursor,
        end_cursor: BufferCursor,
        capture_matches: bool,
        limit_result_count: usize,
        result: &mut Vec<FindMatch>,
    ) {
        let buffers = self.buffers.borrow();
        let piece = self.tree.node(node).piece;
        let buffer = &buffers[piece.buffer_index];
        let start_offset_in_buffer = buffer.line_start(piece.start.line) + piece.start.column;
        let start = buffer.line_start(start_cursor.line) + start_cursor.column;
        let end = buffer.line_start(end_cursor.line) + end_cursor.column;

        // Com separadores de palavra procura num recorte (e o deslocamento é relativo a ele).
        let (search_text, base): (&[u16], usize) = if searcher.word_separators.is_some() {
            searcher.reset(0);
            (sub(&buffer.buffer, start, end), start)
        } else {
            searcher.reset(start);
            (&buffer.buffer, 0)
        };

        while let Some(m) = searcher.next(search_text) {
            let m_index = m.start();
            let m_len = m.end() - m.start();
            if base + m_index >= end {
                return;
            }
            let ret = self.position_in_buffer_with(&buffers, node, base + m_index - start_offset_in_buffer);
            let line_feed_cnt = self.get_line_feed_cnt_with(&buffers, piece.buffer_index, start_cursor, ret);
            let ret_start_column = if ret.line == start_cursor.line {
                (ret.column + start_column as usize) - start_cursor.column
            } else {
                ret.column + 1
            };
            let ret_end_column = ret_start_column + m_len;
            let line = start_line_number + line_feed_cnt as i32;
            result.push(create_find_match(
                Range::new(line, ret_start_column as i32, line, ret_end_column as i32),
                search_text,
                &m,
                capture_matches,
            ));

            if base + m_index + m_len >= end {
                return;
            }
            if result.len() >= limit_result_count {
                return;
            }
        }
    }

    pub fn find_matches_line_by_line(
        &self,
        search_range: &Range,
        search_data: &SearchData,
        capture_matches: bool,
        limit_result_count: usize,
    ) -> Vec<FindMatch> {
        let mut result: Vec<FindMatch> = Vec::new();
        let word_separators = search_data.word_separators.as_deref();
        let mut searcher = Searcher::new(word_separators, &search_data.regex);

        let Some(mut start_position) = self.node_at2(search_range.start_line_number, search_range.start_column) else {
            return Vec::new();
        };
        let Some(end_position) = self.node_at2(search_range.end_line_number, search_range.end_column) else {
            return Vec::new();
        };
        let mut start = self.position_in_buffer(start_position.node, start_position.remainder);
        let end = self.position_in_buffer(end_position.node, end_position.remainder);

        if start_position.node == end_position.node {
            self.find_matches_in_node(
                start_position.node,
                &mut searcher,
                search_range.start_line_number,
                search_range.start_column,
                start,
                end,
                capture_matches,
                limit_result_count,
                &mut result,
            );
            return result;
        }

        let mut start_line_number = search_range.start_line_number;

        let mut current_node = start_position.node;
        while current_node != end_position.node {
            let current_piece = self.tree.node(current_node).piece;
            let line_break_cnt = self.get_line_feed_cnt(current_piece.buffer_index, start, current_piece.end);

            if line_break_cnt >= 1 {
                // posição da última quebra de linha
                let (next_line_start_offset, start_offset_in_buffer) = {
                    let buffers = self.buffers.borrow();
                    let buffer = &buffers[current_piece.buffer_index];
                    (
                        buffer.line_start(start.line + line_break_cnt),
                        buffer.line_start(current_piece.start.line) + current_piece.start.column,
                    )
                };
                let start_column = if start_line_number == search_range.start_line_number {
                    search_range.start_column
                } else {
                    1
                };
                let end_cursor = self.position_in_buffer(current_node, next_line_start_offset - start_offset_in_buffer);
                self.find_matches_in_node(
                    current_node,
                    &mut searcher,
                    start_line_number,
                    start_column,
                    start,
                    end_cursor,
                    capture_matches,
                    limit_result_count,
                    &mut result,
                );

                if result.len() >= limit_result_count {
                    return result;
                }

                start_line_number += line_break_cnt as i32;
            }

            let start_column = if start_line_number == search_range.start_line_number {
                (search_range.start_column - 1) as usize
            } else {
                0
            };
            // procura no que sobrou
            if start_line_number == search_range.end_line_number {
                let line = self.get_line_content(start_line_number);
                let text = sub(line.units(), start_column, (search_range.end_column - 1) as usize);
                self.find_matches_in_line(
                    search_data,
                    &mut searcher,
                    text,
                    search_range.end_line_number,
                    start_column,
                    &mut result,
                    capture_matches,
                    limit_result_count,
                );
                return result;
            }

            let line = self.get_line_content(start_line_number);
            let text = &line.units()[start_column.min(line.len())..];
            self.find_matches_in_line(
                search_data,
                &mut searcher,
                text,
                start_line_number,
                start_column,
                &mut result,
                capture_matches,
                limit_result_count,
            );

            if result.len() >= limit_result_count {
                return result;
            }

            start_line_number += 1;
            start_position = self.node_at2(start_line_number, 1).unwrap_or_else(|| type_error("nodeAt2"));
            current_node = start_position.node;
            start = self.position_in_buffer(start_position.node, start_position.remainder);
        }

        if start_line_number == search_range.end_line_number {
            let start_column = if start_line_number == search_range.start_line_number {
                (search_range.start_column - 1) as usize
            } else {
                0
            };
            let line = self.get_line_content(start_line_number);
            let text = sub(line.units(), start_column, (search_range.end_column - 1) as usize);
            self.find_matches_in_line(
                search_data,
                &mut searcher,
                text,
                search_range.end_line_number,
                start_column,
                &mut result,
                capture_matches,
                limit_result_count,
            );
            return result;
        }

        let start_column = if start_line_number == search_range.start_line_number {
            search_range.start_column
        } else {
            1
        };
        self.find_matches_in_node(
            end_position.node,
            &mut searcher,
            start_line_number,
            start_column,
            start,
            end,
            capture_matches,
            limit_result_count,
            &mut result,
        );
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn find_matches_in_line(
        &self,
        search_data: &SearchData,
        searcher: &mut Searcher,
        text: &[u16],
        line_number: i32,
        delta_offset: usize,
        result: &mut Vec<FindMatch>,
        capture_matches: bool,
        limit_result_count: usize,
    ) {
        let word_separators = search_data.word_separators.as_deref();
        if let (false, Some(search_string)) = (capture_matches, &search_data.simple_search) {
            let search_string = search_string.units();
            let search_string_len = search_string.len();

            let mut from = 0usize;
            while let Some(found) = js_string::index_of(text, search_string, from) {
                let last_match_index = found;
                from = found + search_string_len;
                if word_separators.is_none_or(|ws| is_valid_match(ws, text, last_match_index, search_string_len)) {
                    result.push(FindMatch {
                        range: Range::new(
                            line_number,
                            (last_match_index + 1 + delta_offset) as i32,
                            line_number,
                            (last_match_index + 1 + search_string_len + delta_offset) as i32,
                        ),
                        matches: None,
                    });
                    if result.len() >= limit_result_count {
                        return;
                    }
                }
                if from > text.len() {
                    break;
                }
            }
            return;
        }

        // Reinicia a regex para procurar desde o começo.
        searcher.reset(0);
        while let Some(m) = searcher.next(text) {
            result.push(create_find_match(
                Range::new(
                    line_number,
                    (m.start() + 1 + delta_offset) as i32,
                    line_number,
                    (m.end() + 1 + delta_offset) as i32,
                ),
                text,
                &m,
                capture_matches,
            ));
            if result.len() >= limit_result_count {
                return;
            }
        }
    }
    // #endregion

    // #region Piece Table
    pub fn insert(&mut self, offset: usize, value: &[u16], eol_normalized: bool) {
        self.eol_normalized = self.eol_normalized && eol_normalized;
        *self.last_visited_line.borrow_mut() = (0, JsString::new());

        let mut value: Vec<u16> = value.to_vec();

        if self.tree.root != SENTINEL {
            let NodePosition {
                node,
                remainder,
                node_start_offset,
            } = self.node_at(offset);
            let piece = self.tree.node(node).piece;
            let buffer_index = piece.buffer_index;
            let insert_pos_in_buffer = self.position_in_buffer(node, remainder);
            if piece.buffer_index == 0
                && piece.end.line == self.last_change_buffer_pos.line
                && piece.end.column == self.last_change_buffer_pos.column
                && node_start_offset + piece.length == offset
                && value.len() < AVERAGE_BUFFER_SIZE
            {
                // buffer de mudanças
                self.append_to_node(node, &value);
                self.compute_buffer_metadata();
                return;
            }

            if node_start_offset == offset {
                self.insert_content_to_node_left(&value, node);
                self.validate_search_cache(offset);
            } else if node_start_offset + piece.length > offset {
                // vamos inserir no meio de um nó
                let mut nodes_to_del: Vec<NodeId> = Vec::new();
                let mut new_right_piece = Piece::new(
                    piece.buffer_index,
                    insert_pos_in_buffer,
                    piece.end,
                    self.get_line_feed_cnt(piece.buffer_index, insert_pos_in_buffer, piece.end),
                    self.offset_in_buffer(buffer_index, piece.end) - self.offset_in_buffer(buffer_index, insert_pos_in_buffer),
                );

                if self.should_check_crlf() && Self::ends_with_cr_str(&value) {
                    let head_of_right = self.node_char_code_at(node, remainder as isize);

                    if head_of_right == 10 {
                        let new_start = BufferCursor {
                            line: new_right_piece.start.line + 1,
                            column: 0,
                        };
                        new_right_piece = Piece::new(
                            new_right_piece.buffer_index,
                            new_start,
                            new_right_piece.end,
                            self.get_line_feed_cnt(new_right_piece.buffer_index, new_start, new_right_piece.end),
                            new_right_piece.length - 1,
                        );

                        value.push(char_code::LineFeed);
                    }
                }

                // reaproveita o nó para o conteúdo antes do ponto de inserção
                if self.should_check_crlf() && Self::starts_with_lf_str(&value) {
                    let tail_of_left = self.node_char_code_at(node, remainder as isize - 1);
                    if tail_of_left == 13 {
                        let previous_pos = self.position_in_buffer(node, remainder - 1);
                        self.delete_node_tail(node, previous_pos);
                        value.insert(0, char_code::CarriageReturn);

                        if self.tree.node(node).piece.length == 0 {
                            nodes_to_del.push(node);
                        }
                    } else {
                        self.delete_node_tail(node, insert_pos_in_buffer);
                    }
                } else {
                    self.delete_node_tail(node, insert_pos_in_buffer);
                }

                let new_pieces = self.create_new_pieces(&value);
                if new_right_piece.length > 0 {
                    self.rb_insert_right(Some(node), new_right_piece);
                }

                let mut tmp_node = node;
                for piece in new_pieces {
                    tmp_node = self.rb_insert_right(Some(tmp_node), piece);
                }
                self.delete_nodes(&nodes_to_del);
            } else {
                self.insert_content_to_node_right(&value, node);
            }
        } else {
            // insere um nó novo
            let pieces = self.create_new_pieces(&value);
            let mut node = self.rb_insert_left(None, pieces[0]);

            for piece in &pieces[1..] {
                node = self.rb_insert_right(Some(node), *piece);
            }
        }

        // TODO do original: é brutal, a contagem total de quebras deveria ser atualizada como o lf_left.
        self.compute_buffer_metadata();
    }

    pub fn delete(&mut self, offset: usize, cnt: usize) {
        *self.last_visited_line.borrow_mut() = (0, JsString::new());

        if cnt == 0 || self.tree.root == SENTINEL {
            return;
        }

        let start_position = self.node_at(offset);
        let end_position = self.node_at(offset + cnt);
        let start_node = start_position.node;
        let end_node = end_position.node;

        if start_node == end_node {
            let start_split_pos_in_buffer = self.position_in_buffer(start_node, start_position.remainder);
            let end_split_pos_in_buffer = self.position_in_buffer(start_node, end_position.remainder);

            if start_position.node_start_offset == offset {
                if cnt == self.tree.node(start_node).piece.length {
                    // remove o nó
                    let next = self.tree.next(start_node);
                    self.tree.rb_delete(start_node);
                    self.validate_crlf_with_prev_node(next);
                    self.compute_buffer_metadata();
                    return;
                }
                self.delete_node_head(start_node, end_split_pos_in_buffer);
                self.validate_search_cache(offset);
                self.validate_crlf_with_prev_node(start_node);
                self.compute_buffer_metadata();
                return;
            }

            if start_position.node_start_offset + self.tree.node(start_node).piece.length == offset + cnt {
                self.delete_node_tail(start_node, start_split_pos_in_buffer);
                self.validate_crlf_with_next_node(start_node);
                self.compute_buffer_metadata();
                return;
            }

            // remove no meio, o nó será partido em dois
            self.shrink_node(start_node, start_split_pos_in_buffer, end_split_pos_in_buffer);
            self.compute_buffer_metadata();
            return;
        }

        let mut nodes_to_del: Vec<NodeId> = Vec::new();

        let start_split_pos_in_buffer = self.position_in_buffer(start_node, start_position.remainder);
        self.delete_node_tail(start_node, start_split_pos_in_buffer);
        self.validate_search_cache(offset);
        if self.tree.node(start_node).piece.length == 0 {
            nodes_to_del.push(start_node);
        }

        // atualiza o último nó tocado
        let end_split_pos_in_buffer = self.position_in_buffer(end_node, end_position.remainder);
        self.delete_node_head(end_node, end_split_pos_in_buffer);
        if self.tree.node(end_node).piece.length == 0 {
            nodes_to_del.push(end_node);
        }

        // remove os nós do meio
        let mut node = self.tree.next(start_node);
        while node != SENTINEL && node != end_node {
            nodes_to_del.push(node);
            node = self.tree.next(node);
        }

        let prev = if self.tree.node(start_node).piece.length == 0 {
            self.tree.prev(start_node)
        } else {
            start_node
        };
        self.delete_nodes(&nodes_to_del);
        self.validate_crlf_with_next_node(prev);
        self.compute_buffer_metadata();
    }

    fn insert_content_to_node_left(&mut self, value: &[u16], node: NodeId) {
        // vamos inserir conteúdo no começo do nó
        let mut value = value.to_vec();
        let mut nodes_to_del: Vec<NodeId> = Vec::new();
        if self.should_check_crlf() && Self::ends_with_cr_str(&value) && self.starts_with_lf_node(node) {
            // move o `\n` para um nó novo

            let piece = self.tree.node(node).piece;
            let new_start = BufferCursor {
                line: piece.start.line + 1,
                column: 0,
            };
            let n_piece = Piece::new(
                piece.buffer_index,
                new_start,
                piece.end,
                self.get_line_feed_cnt(piece.buffer_index, new_start, piece.end),
                piece.length - 1,
            );

            self.tree.node_mut(node).piece = n_piece;

            value.push(char_code::LineFeed);
            self.tree.update_tree_metadata(node, -1, -1);

            if self.tree.node(node).piece.length == 0 {
                nodes_to_del.push(node);
            }
        }

        let new_pieces = self.create_new_pieces(&value);
        let mut new_node = self.rb_insert_left(Some(node), new_pieces[new_pieces.len() - 1]);
        for k in (0..new_pieces.len() - 1).rev() {
            new_node = self.rb_insert_left(Some(new_node), new_pieces[k]);
        }
        self.validate_crlf_with_prev_node(new_node);
        self.delete_nodes(&nodes_to_del);
    }

    fn insert_content_to_node_right(&mut self, value: &[u16], node: NodeId) {
        // vamos inserir à direita deste nó
        let mut value = value.to_vec();
        if self.adjust_carriage_return_from_next(&value, node) {
            // move o \n para o nó novo
            value.push(char_code::LineFeed);
        }

        let new_pieces = self.create_new_pieces(&value);
        let new_node = self.rb_insert_right(Some(node), new_pieces[0]);
        let mut tmp_node = new_node;

        for piece in &new_pieces[1..] {
            tmp_node = self.rb_insert_right(Some(tmp_node), *piece);
        }

        self.validate_crlf_with_prev_node(new_node);
    }

    fn position_in_buffer_with(&self, buffers: &[StringBuffer], node: NodeId, remainder: usize) -> BufferCursor {
        let piece = self.tree.node(node).piece;
        let buffer = &buffers[piece.buffer_index];

        let start_offset = buffer.line_start(piece.start.line) + piece.start.column;

        let offset = start_offset + remainder;

        // busca binária do deslocamento entre startOffset e endOffset
        let mut low = piece.start.line as isize;
        let mut high = piece.end.line as isize;

        let mut mid: isize = 0;
        let mut mid_start: usize = 0;

        while low <= high {
            mid = low + (high - low) / 2;
            mid_start = buffer.line_start(mid as usize);

            if mid == high {
                break;
            }

            let mid_stop = buffer.line_start(mid as usize + 1);

            if offset < mid_start {
                high = mid - 1;
            } else if offset >= mid_stop {
                low = mid + 1;
            } else {
                break;
            }
        }

        BufferCursor {
            line: mid as usize,
            column: offset - mid_start,
        }
    }

    fn position_in_buffer(&self, node: NodeId, remainder: usize) -> BufferCursor {
        self.position_in_buffer_with(&self.buffers.borrow(), node, remainder)
    }

    fn get_line_feed_cnt_with(&self, buffers: &[StringBuffer], buffer_index: usize, start: BufferCursor, end: BufferCursor) -> usize {
        // Não precisamos nos preocupar com o início: abc\r|\n, abc|\r, abc|\n ou abc|\r\n não mudam o fato
        // de haver uma quebra depois do início. No fim: abc\r|\n, entre o \r e o \n, soma 1.
        if end.column == 0 {
            return end.line - start.line;
        }

        let buffer = &buffers[buffer_index];
        if end.line == buffer.line_starts.len() - 1 {
            // não há \n depois do fim, senão haveria mais um início de linha
            return end.line - start.line;
        }

        let next_line_start_offset = buffer.line_start(end.line + 1);
        let end_offset = buffer.line_start(end.line) + end.column;
        if next_line_start_offset > end_offset + 1 {
            // há mais de um caractere depois do fim, então não pode ser \n
            return end.line - start.line;
        }
        // endOffset + 1 == nextLineStartOffset: o caractere em endOffset é \n, então olha o anterior
        // (se fosse \r, end.column seria 0 e não chegaríamos aqui).
        let previous_char_offset = end_offset - 1; // end.column > 0
        if buffer.buffer.get(previous_char_offset) == Some(&13) {
            end.line - start.line + 1
        } else {
            end.line - start.line
        }
    }

    fn get_line_feed_cnt(&self, buffer_index: usize, start: BufferCursor, end: BufferCursor) -> usize {
        self.get_line_feed_cnt_with(&self.buffers.borrow(), buffer_index, start, end)
    }

    fn offset_in_buffer(&self, buffer_index: usize, cursor: BufferCursor) -> usize {
        self.buffers.borrow()[buffer_index].line_start(cursor.line) + cursor.column
    }

    fn delete_nodes(&mut self, nodes: &[NodeId]) {
        for &node in nodes {
            self.tree.rb_delete(node);
        }
    }

    fn create_new_pieces(&mut self, text: &[u16]) -> Vec<Piece> {
        let mut buffers = self.buffers.borrow_mut();
        if text.len() > AVERAGE_BUFFER_SIZE {
            // O conteúdo é grande: operações como substring e charCode ficam lentas, então partimos em
            // pedaços menores, como na normalização de CR/LF.
            let mut new_pieces: Vec<Piece> = Vec::new();
            let mut text = text;
            while text.len() > AVERAGE_BUFFER_SIZE {
                let last_char = text[AVERAGE_BUFFER_SIZE - 1];
                let split_len = if last_char == char_code::CarriageReturn || (0xD800..=0xDBFF).contains(&last_char) {
                    // o último caractere é \r ou um high surrogate: guarda-o para o próximo pedaço
                    AVERAGE_BUFFER_SIZE - 1
                } else {
                    AVERAGE_BUFFER_SIZE
                };
                let (split_text, rest) = text.split_at(split_len);
                text = rest;

                let line_starts = create_line_starts_fast(split_text);
                let last_line = line_starts.len() - 1;
                new_pieces.push(Piece::new(
                    buffers.len(),
                    BufferCursor { line: 0, column: 0 },
                    BufferCursor {
                        line: last_line,
                        column: split_text.len() - line_starts[last_line] as usize,
                    },
                    last_line,
                    split_text.len(),
                ));
                buffers.push(StringBuffer::new(split_text.to_vec(), line_starts));
            }

            let line_starts = create_line_starts_fast(text);
            let last_line = line_starts.len() - 1;
            new_pieces.push(Piece::new(
                buffers.len(),
                BufferCursor { line: 0, column: 0 },
                BufferCursor {
                    line: last_line,
                    column: text.len() - line_starts[last_line] as usize,
                },
                last_line,
                text.len(),
            ));
            buffers.push(StringBuffer::new(text.to_vec(), line_starts));

            return new_pieces;
        }

        let mut start_offset = buffers[0].buffer.len();
        let mut line_starts = create_line_starts_fast(text);

        let mut start = self.last_change_buffer_pos;
        let change_buffer = &mut buffers[0];
        if change_buffer.line_starts[change_buffer.line_starts.len() - 1] as usize == start_offset
            && start_offset != 0
            && Self::starts_with_lf_str(text)
            && Self::ends_with_cr_str(&change_buffer.buffer)
        {
            self.last_change_buffer_pos = BufferCursor {
                line: self.last_change_buffer_pos.line,
                column: self.last_change_buffer_pos.column + 1,
            };
            start = self.last_change_buffer_pos;

            for line_start in &mut line_starts {
                *line_start += start_offset as u32 + 1;
            }

            change_buffer.line_starts.extend_from_slice(&line_starts[1..]);
            change_buffer.buffer.push(u16::from(b'_'));
            change_buffer.buffer.extend_from_slice(text);
            start_offset += 1;
        } else {
            if start_offset != 0 {
                for line_start in &mut line_starts {
                    *line_start += start_offset as u32;
                }
            }
            change_buffer.line_starts.extend_from_slice(&line_starts[1..]);
            change_buffer.buffer.extend_from_slice(text);
        }

        let end_offset = change_buffer.buffer.len();
        let end_index = change_buffer.line_starts.len() - 1;
        let end_column = end_offset - change_buffer.line_starts[end_index] as usize;
        let end_pos = BufferCursor {
            line: end_index,
            column: end_column,
        };
        drop(buffers);
        let new_piece = Piece::new(
            0,
            start,
            end_pos,
            self.get_line_feed_cnt(0, start, end_pos),
            end_offset - start_offset,
        );
        self.last_change_buffer_pos = end_pos;
        vec![new_piece]
    }

    pub fn get_lines_raw_content(&self) -> JsString {
        self.get_content_of_sub_tree(self.tree.root)
    }

    pub fn get_line_raw_content(&self, line_number: i32, end_offset: usize) -> JsString {
        let buffers = self.buffers.borrow();
        let mut x = self.tree.root;

        let mut ret: Vec<u16> = Vec::new();
        let mut line_number = line_number as isize;
        let cache = self.search_cache.borrow().get2(&self.tree, line_number as usize);
        if let Some((entry, node_start_line_number)) = cache {
            x = entry.node;
            let piece = self.tree.node(x).piece;
            let prev_accumulated_value = self.get_accumulated_value(x, line_number - node_start_line_number as isize - 1);
            let buffer = &buffers[piece.buffer_index].buffer;
            let start_offset = self.offset_in_buffer_with(&buffers, piece.buffer_index, piece.start);
            if node_start_line_number + piece.line_feed_cnt == line_number as usize {
                ret.extend_from_slice(sub(buffer, start_offset + prev_accumulated_value, start_offset + piece.length));
            } else {
                let accumulated_value = self.get_accumulated_value(x, line_number - node_start_line_number as isize);
                return JsString::from(sub(
                    buffer,
                    start_offset + prev_accumulated_value,
                    start_offset + accumulated_value - end_offset,
                ));
            }
        } else {
            let mut node_start_offset = 0;
            let original_line_number = line_number;
            while x != SENTINEL {
                let n = *self.tree.node(x);
                if n.left != SENTINEL && n.lf_left as isize >= line_number - 1 {
                    x = n.left;
                } else if (n.lf_left + n.piece.line_feed_cnt) as isize > line_number - 1 {
                    let prev_accumulated_value = self.get_accumulated_value(x, line_number - n.lf_left as isize - 2);
                    let accumulated_value = self.get_accumulated_value(x, line_number - n.lf_left as isize - 1);
                    let buffer = &buffers[n.piece.buffer_index].buffer;
                    let start_offset = self.offset_in_buffer_with(&buffers, n.piece.buffer_index, n.piece.start);
                    node_start_offset += n.size_left;
                    self.search_cache.borrow_mut().set(CacheEntry {
                        node: x,
                        node_start_offset,
                        node_start_line_number: Some((original_line_number - (line_number - 1 - n.lf_left as isize)) as usize),
                    });

                    return JsString::from(sub(
                        buffer,
                        start_offset + prev_accumulated_value,
                        start_offset + accumulated_value - end_offset,
                    ));
                } else if (n.lf_left + n.piece.line_feed_cnt) as isize == line_number - 1 {
                    let prev_accumulated_value = self.get_accumulated_value(x, line_number - n.lf_left as isize - 2);
                    let buffer = &buffers[n.piece.buffer_index].buffer;
                    let start_offset = self.offset_in_buffer_with(&buffers, n.piece.buffer_index, n.piece.start);

                    ret.extend_from_slice(sub(buffer, start_offset + prev_accumulated_value, start_offset + n.piece.length));
                    break;
                } else {
                    line_number -= (n.lf_left + n.piece.line_feed_cnt) as isize;
                    node_start_offset += n.size_left + n.piece.length;
                    x = n.right;
                }
            }
        }

        // procura em ordem o nó que contém a coluna final
        x = self.tree.next(x);
        while x != SENTINEL {
            let piece = self.tree.node(x).piece;
            let buffer = &buffers[piece.buffer_index].buffer;

            if piece.line_feed_cnt > 0 {
                let accumulated_value = self.get_accumulated_value(x, 0);
                let start_offset = self.offset_in_buffer_with(&buffers, piece.buffer_index, piece.start);

                ret.extend_from_slice(sub(buffer, start_offset, start_offset + accumulated_value - end_offset));
                return JsString::from_units(ret);
            }
            let start_offset = self.offset_in_buffer_with(&buffers, piece.buffer_index, piece.start);
            ret.extend_from_slice(sub(buffer, start_offset, start_offset + piece.length));

            x = self.tree.next(x);
        }

        JsString::from_units(ret)
    }

    fn offset_in_buffer_with(&self, buffers: &[StringBuffer], buffer_index: usize, cursor: BufferCursor) -> usize {
        buffers[buffer_index].line_start(cursor.line) + cursor.column
    }

    fn compute_buffer_metadata(&mut self) {
        let mut x = self.tree.root;

        let mut lf_cnt = 1;
        let mut len = 0;

        while x != SENTINEL {
            let n = self.tree.node(x);
            lf_cnt += n.lf_left + n.piece.line_feed_cnt;
            len += n.size_left + n.piece.length;
            x = n.right;
        }

        self.line_cnt = lf_cnt;
        self.length = len;
        self.validate_search_cache(len);
        self.tree.release_detached();
    }

    fn validate_search_cache(&mut self, offset: usize) {
        self.search_cache.get_mut().validate(&self.tree, offset);
    }

    // #region node operations
    fn get_index_of(&self, node: NodeId, accumulated_value: usize) -> (usize, usize) {
        let piece = self.tree.node(node).piece;
        let pos = self.position_in_buffer(node, accumulated_value);
        let line_cnt = pos.line - piece.start.line;

        if self.offset_in_buffer(piece.buffer_index, piece.end) - self.offset_in_buffer(piece.buffer_index, piece.start)
            == accumulated_value
        {
            // estamos olhando o fim deste nó, então é preciso checar CRLF
            let real_line_cnt = self.get_line_feed_cnt(piece.buffer_index, piece.start, pos);
            if real_line_cnt != line_cnt {
                // ah sim, CRLF
                return (real_line_cnt, 0);
            }
        }

        (line_cnt, pos.column)
    }

    fn get_accumulated_value(&self, node: NodeId, index: isize) -> usize {
        if index < 0 {
            return 0;
        }
        let piece = self.tree.node(node).piece;
        let buffers = self.buffers.borrow();
        let buffer = &buffers[piece.buffer_index];
        let expected_line_start_index = piece.start.line + index as usize + 1;
        if expected_line_start_index > piece.end.line {
            buffer.line_start(piece.end.line) + piece.end.column - buffer.line_start(piece.start.line) - piece.start.column
        } else {
            buffer.line_start(expected_line_start_index) - buffer.line_start(piece.start.line) - piece.start.column
        }
    }

    fn delete_node_tail(&mut self, node: NodeId, pos: BufferCursor) {
        let piece = self.tree.node(node).piece;
        let original_lf_cnt = piece.line_feed_cnt;
        let original_end_offset = self.offset_in_buffer(piece.buffer_index, piece.end);

        let new_end = pos;
        let new_end_offset = self.offset_in_buffer(piece.buffer_index, new_end);
        let new_line_feed_cnt = self.get_line_feed_cnt(piece.buffer_index, piece.start, new_end);

        let lf_delta = new_line_feed_cnt as isize - original_lf_cnt as isize;
        let size_delta = new_end_offset as isize - original_end_offset as isize;
        let new_length = (piece.length as isize + size_delta) as usize;

        self.tree.node_mut(node).piece = Piece::new(piece.buffer_index, piece.start, new_end, new_line_feed_cnt, new_length);

        self.tree.update_tree_metadata(node, size_delta, lf_delta);
    }

    fn delete_node_head(&mut self, node: NodeId, pos: BufferCursor) {
        let piece = self.tree.node(node).piece;
        let original_lf_cnt = piece.line_feed_cnt;
        let original_start_offset = self.offset_in_buffer(piece.buffer_index, piece.start);

        let new_start = pos;
        let new_line_feed_cnt = self.get_line_feed_cnt(piece.buffer_index, new_start, piece.end);
        let new_start_offset = self.offset_in_buffer(piece.buffer_index, new_start);
        let lf_delta = new_line_feed_cnt as isize - original_lf_cnt as isize;
        let size_delta = original_start_offset as isize - new_start_offset as isize;
        let new_length = (piece.length as isize + size_delta) as usize;
        self.tree.node_mut(node).piece = Piece::new(piece.buffer_index, new_start, piece.end, new_line_feed_cnt, new_length);

        self.tree.update_tree_metadata(node, size_delta, lf_delta);
    }

    fn shrink_node(&mut self, node: NodeId, start: BufferCursor, end: BufferCursor) {
        let piece = self.tree.node(node).piece;
        let original_start_pos = piece.start;
        let original_end_pos = piece.end;

        // peça antiga, originalStartPos, start
        let old_length = piece.length;
        let old_lf_cnt = piece.line_feed_cnt;
        let new_end = start;
        let new_line_feed_cnt = self.get_line_feed_cnt(piece.buffer_index, piece.start, new_end);
        let new_length = self.offset_in_buffer(piece.buffer_index, start) - self.offset_in_buffer(piece.buffer_index, original_start_pos);

        self.tree.node_mut(node).piece = Piece::new(piece.buffer_index, piece.start, new_end, new_line_feed_cnt, new_length);

        self.tree.update_tree_metadata(
            node,
            new_length as isize - old_length as isize,
            new_line_feed_cnt as isize - old_lf_cnt as isize,
        );

        // peça nova à direita, end, originalEndPos
        let new_piece = Piece::new(
            piece.buffer_index,
            end,
            original_end_pos,
            self.get_line_feed_cnt(piece.buffer_index, end, original_end_pos),
            self.offset_in_buffer(piece.buffer_index, original_end_pos) - self.offset_in_buffer(piece.buffer_index, end),
        );

        let new_node = self.rb_insert_right(Some(node), new_piece);
        self.validate_crlf_with_prev_node(new_node);
    }

    fn append_to_node(&mut self, node: NodeId, value: &[u16]) {
        let mut value = value.to_vec();
        if self.adjust_carriage_return_from_next(&value, node) {
            value.push(char_code::LineFeed);
        }

        let hit_crlf = self.should_check_crlf() && Self::starts_with_lf_str(&value) && self.ends_with_cr_node(node);
        let mut buffers = self.buffers.borrow_mut();
        let change_buffer = &mut buffers[0];
        let start_offset = change_buffer.buffer.len();
        change_buffer.buffer.extend_from_slice(&value);
        let mut line_starts = create_line_starts_fast(&value);
        for line_start in &mut line_starts {
            *line_start += start_offset as u32;
        }
        if hit_crlf {
            let prev_start_offset = change_buffer.line_starts[change_buffer.line_starts.len() - 2] as usize;
            change_buffer.line_starts.pop();
            // _lastChangeBufferPos já está errado
            self.last_change_buffer_pos = BufferCursor {
                line: self.last_change_buffer_pos.line - 1,
                column: start_offset - prev_start_offset,
            };
        }

        change_buffer.line_starts.extend_from_slice(&line_starts[1..]);
        let end_index = change_buffer.line_starts.len() - 1;
        let end_column = change_buffer.buffer.len() - change_buffer.line_starts[end_index] as usize;
        let new_end = BufferCursor {
            line: end_index,
            column: end_column,
        };
        drop(buffers);

        let piece = self.tree.node(node).piece;
        let new_length = piece.length + value.len();
        let old_line_feed_cnt = piece.line_feed_cnt;
        let new_line_feed_cnt = self.get_line_feed_cnt(0, piece.start, new_end);
        let lf_delta = new_line_feed_cnt as isize - old_line_feed_cnt as isize;

        self.tree.node_mut(node).piece = Piece::new(piece.buffer_index, piece.start, new_end, new_line_feed_cnt, new_length);

        self.last_change_buffer_pos = new_end;
        self.tree.update_tree_metadata(node, value.len() as isize, lf_delta);
    }

    fn node_at_checked(&self, offset: usize) -> Option<NodePosition> {
        let cache = self.search_cache.borrow().get(&self.tree, offset);
        if let Some(cache) = cache {
            return Some(NodePosition {
                node: cache.node,
                node_start_offset: cache.node_start_offset,
                remainder: offset - cache.node_start_offset,
            });
        }

        let mut x = self.tree.root;
        let mut offset = offset;
        let mut node_start_offset = 0;

        while x != SENTINEL {
            let n = self.tree.node(x);
            if n.size_left > offset {
                x = n.left;
            } else if n.size_left + n.piece.length >= offset {
                node_start_offset += n.size_left;
                let ret = NodePosition {
                    node: x,
                    remainder: offset - n.size_left,
                    node_start_offset,
                };
                self.search_cache.borrow_mut().set(CacheEntry {
                    node: x,
                    node_start_offset,
                    node_start_line_number: None,
                });
                return Some(ret);
            } else {
                offset -= n.size_left + n.piece.length;
                node_start_offset += n.size_left + n.piece.length;
                x = n.right;
            }
        }

        None
    }

    /// `nodeAt(offset)`: o `null` do original, que lá é desreferenciado logo depois, vira `TypeError`.
    fn node_at(&self, offset: usize) -> NodePosition {
        self.node_at_checked(offset)
            .unwrap_or_else(|| type_error("Cannot destructure property of null (nodeAt)"))
    }

    fn node_at2(&self, line_number: i32, column: i32) -> Option<NodePosition> {
        let mut x = self.tree.root;
        let mut node_start_offset = 0;
        let mut line_number = line_number as isize;
        let mut column = column as isize;

        while x != SENTINEL {
            let n = *self.tree.node(x);
            if n.left != SENTINEL && n.lf_left as isize >= line_number - 1 {
                x = n.left;
            } else if (n.lf_left + n.piece.line_feed_cnt) as isize > line_number - 1 {
                let prev_accumulated_value = self.get_accumulated_value(x, line_number - n.lf_left as isize - 2);
                let accumulated_value = self.get_accumulated_value(x, line_number - n.lf_left as isize - 1);
                node_start_offset += n.size_left;

                return Some(NodePosition {
                    node: x,
                    remainder: ((prev_accumulated_value as isize + column - 1).min(accumulated_value as isize)) as usize,
                    node_start_offset,
                });
            } else if (n.lf_left + n.piece.line_feed_cnt) as isize == line_number - 1 {
                let prev_accumulated_value = self.get_accumulated_value(x, line_number - n.lf_left as isize - 2);
                if prev_accumulated_value as isize + column - 1 <= n.piece.length as isize {
                    return Some(NodePosition {
                        node: x,
                        remainder: (prev_accumulated_value as isize + column - 1) as usize,
                        node_start_offset,
                    });
                }
                column -= n.piece.length as isize - prev_accumulated_value as isize;
                break;
            } else {
                line_number -= (n.lf_left + n.piece.line_feed_cnt) as isize;
                node_start_offset += n.size_left + n.piece.length;
                x = n.right;
            }
        }

        // procura em ordem o nó que contém a coluna
        x = self.tree.next(x);
        while x != SENTINEL {
            let piece = self.tree.node(x).piece;
            if piece.line_feed_cnt > 0 {
                let accumulated_value = self.get_accumulated_value(x, 0);
                let node_start_offset = self.offset_of_node(x);
                return Some(NodePosition {
                    node: x,
                    remainder: ((column - 1).min(accumulated_value as isize)) as usize,
                    node_start_offset,
                });
            }
            if piece.length as isize >= column - 1 {
                let node_start_offset = self.offset_of_node(x);
                return Some(NodePosition {
                    node: x,
                    remainder: (column - 1) as usize,
                    node_start_offset,
                });
            }
            column -= piece.length as isize;

            x = self.tree.next(x);
        }

        None
    }

    /// `nodeCharCodeAt`: `-1` quando a peça não tem quebra de linha ou o índice cai fora (o `NaN`).
    fn node_char_code_at(&self, node: NodeId, offset: isize) -> i32 {
        let piece = self.tree.node(node).piece;
        if piece.line_feed_cnt < 1 {
            return -1;
        }
        let buffers = self.buffers.borrow();
        let new_offset = self.offset_in_buffer_with(&buffers, piece.buffer_index, piece.start) as isize + offset;
        usize::try_from(new_offset)
            .ok()
            .and_then(|index| buffers[piece.buffer_index].buffer.get(index))
            .map_or(-1, |&unit| i32::from(unit))
    }

    fn offset_of_node(&self, mut node: NodeId) -> usize {
        let mut pos = self.tree.node(node).size_left;
        while node != self.tree.root {
            let parent = self.tree.node(node).parent;
            if self.tree.node(parent).right == node {
                pos += self.tree.node(parent).size_left + self.tree.node(parent).piece.length;
            }

            node = parent;
        }

        pos
    }
    // #endregion

    // #region CRLF
    fn should_check_crlf(&self) -> bool {
        !(self.eol_normalized && self.eol == Eol::Lf)
    }

    fn starts_with_lf_str(val: &[u16]) -> bool {
        val.first() == Some(&10)
    }

    fn starts_with_lf_node(&self, val: NodeId) -> bool {
        if val == SENTINEL || self.tree.node(val).piece.line_feed_cnt == 0 {
            return false;
        }

        let piece = self.tree.node(val).piece;
        let buffers = self.buffers.borrow();
        let buffer = &buffers[piece.buffer_index];
        let line = piece.start.line;
        let start_offset = buffer.line_start(line) + piece.start.column;
        if line == buffer.line_starts.len() - 1 {
            // última linha, então não há quebra no fim dela
            return false;
        }
        let next_line_offset = buffer.line_start(line + 1);
        if next_line_offset > start_offset + 1 {
            return false;
        }
        buffer.buffer.get(start_offset) == Some(&10)
    }

    fn ends_with_cr_str(val: &[u16]) -> bool {
        val.last() == Some(&13)
    }

    fn ends_with_cr_node(&self, val: NodeId) -> bool {
        if val == SENTINEL || self.tree.node(val).piece.line_feed_cnt == 0 {
            return false;
        }

        self.node_char_code_at(val, self.tree.node(val).piece.length as isize - 1) == 13
    }

    fn validate_crlf_with_prev_node(&mut self, next_node: NodeId) {
        if self.should_check_crlf() && self.starts_with_lf_node(next_node) {
            let node = self.tree.prev(next_node);
            if self.ends_with_cr_node(node) {
                self.fix_crlf(node, next_node);
            }
        }
    }

    fn validate_crlf_with_next_node(&mut self, node: NodeId) {
        if self.should_check_crlf() && self.ends_with_cr_node(node) {
            let next_node = self.tree.next(node);
            if self.starts_with_lf_node(next_node) {
                self.fix_crlf(node, next_node);
            }
        }
    }

    fn fix_crlf(&mut self, prev: NodeId, next: NodeId) {
        let mut nodes_to_del: Vec<NodeId> = Vec::new();
        // atualiza o nó
        let prev_piece = self.tree.node(prev).piece;
        let new_end = {
            let buffers = self.buffers.borrow();
            let buffer = &buffers[prev_piece.buffer_index];
            if prev_piece.end.column == 0 {
                // a última linha termina em \r, não em \r\n
                BufferCursor {
                    line: prev_piece.end.line - 1,
                    column: buffer.line_start(prev_piece.end.line) - buffer.line_start(prev_piece.end.line - 1) - 1,
                }
            } else {
                // \r\n
                BufferCursor {
                    line: prev_piece.end.line,
                    column: prev_piece.end.column - 1,
                }
            }
        };

        let prev_new_length = prev_piece.length - 1;
        let prev_new_lf_cnt = prev_piece.line_feed_cnt - 1;
        self.tree.node_mut(prev).piece = Piece::new(prev_piece.buffer_index, prev_piece.start, new_end, prev_new_lf_cnt, prev_new_length);

        self.tree.update_tree_metadata(prev, -1, -1);
        if self.tree.node(prev).piece.length == 0 {
            nodes_to_del.push(prev);
        }

        // atualiza o nextNode
        let next_piece = self.tree.node(next).piece;
        let new_start = BufferCursor {
            line: next_piece.start.line + 1,
            column: 0,
        };
        let new_length = next_piece.length - 1;
        let new_line_feed_cnt = self.get_line_feed_cnt(next_piece.buffer_index, new_start, next_piece.end);
        self.tree.node_mut(next).piece = Piece::new(next_piece.buffer_index, new_start, next_piece.end, new_line_feed_cnt, new_length);

        self.tree.update_tree_metadata(next, -1, -1);
        if self.tree.node(next).piece.length == 0 {
            nodes_to_del.push(next);
        }

        // cria a peça nova que contém o \r\n
        let pieces = self.create_new_pieces(&[13, 10]);
        self.rb_insert_right(Some(prev), pieces[0]);
        // apaga os nós vazios

        for node in nodes_to_del {
            self.tree.rb_delete(node);
        }
    }

    fn adjust_carriage_return_from_next(&mut self, value: &[u16], node: NodeId) -> bool {
        if self.should_check_crlf() && Self::ends_with_cr_str(value) {
            let next_node = self.tree.next(node);
            if self.starts_with_lf_node(next_node) {
                // move o `\n` para a frente
                let next_piece = self.tree.node(next_node).piece;
                if next_piece.length == 1 {
                    self.tree.rb_delete(next_node);
                } else {
                    let new_start = BufferCursor {
                        line: next_piece.start.line + 1,
                        column: 0,
                    };
                    let new_length = next_piece.length - 1;
                    let new_line_feed_cnt = self.get_line_feed_cnt(next_piece.buffer_index, new_start, next_piece.end);
                    self.tree.node_mut(next_node).piece =
                        Piece::new(next_piece.buffer_index, new_start, next_piece.end, new_line_feed_cnt, new_length);

                    self.tree.update_tree_metadata(next_node, -1, -1);
                }
                return true;
            }
        }

        false
    }
    // #endregion

    // #endregion

    // #region Tree operations
    fn get_node_content(&self, node: NodeId) -> Vec<u16> {
        if node == SENTINEL {
            return Vec::new();
        }
        piece_content(&self.buffers.borrow(), &self.tree.node(node).piece)
    }

    /// `getPieceContent(piece)`.
    pub fn get_piece_content(&self, piece: &Piece) -> JsString {
        JsString::from_units(piece_content(&self.buffers.borrow(), piece))
    }

    /// Insere `p` logo depois de `node` em ordem (`node` é `None` só na árvore vazia).
    fn rb_insert_right(&mut self, node: Option<NodeId>, p: Piece) -> NodeId {
        let z = self.tree.alloc(p);

        if self.tree.root == SENTINEL {
            self.tree.root = z;
            self.tree.node_mut(z).color = crate::rb_tree_base::NodeColor::Black;
        } else {
            let node = node.expect("rbInsertRight sem nó numa árvore não vazia");
            if self.tree.node(node).right == SENTINEL {
                self.tree.node_mut(node).right = z;
                self.tree.node_mut(z).parent = node;
            } else {
                let next_node = self.tree.leftest(self.tree.node(node).right);
                self.tree.node_mut(next_node).left = z;
                self.tree.node_mut(z).parent = next_node;
            }
        }

        self.tree.fix_insert(z);
        z
    }

    /// Insere `p` logo antes de `node` em ordem (`node` é `None` só na árvore vazia).
    fn rb_insert_left(&mut self, node: Option<NodeId>, p: Piece) -> NodeId {
        let z = self.tree.alloc(p);

        if self.tree.root == SENTINEL {
            self.tree.root = z;
            self.tree.node_mut(z).color = crate::rb_tree_base::NodeColor::Black;
        } else {
            let node = node.expect("rbInsertLeft sem nó numa árvore não vazia");
            if self.tree.node(node).left == SENTINEL {
                self.tree.node_mut(node).left = z;
                self.tree.node_mut(z).parent = node;
            } else {
                let prev_node = self.tree.righttest(self.tree.node(node).left);
                self.tree.node_mut(prev_node).right = z;
                self.tree.node_mut(z).parent = prev_node;
            }
        }

        self.tree.fix_insert(z);
        z
    }

    fn get_content_of_sub_tree(&self, node: NodeId) -> JsString {
        let mut str: Vec<u16> = Vec::new();
        if node == SENTINEL {
            return JsString::new();
        }
        // Em ordem a partir de `node` (a subárvore): do mais à esquerda até o último dentro dela.
        let first = self.tree.leftest(node);
        let last = self.tree.righttest(node);
        let mut current = first;
        loop {
            str.extend_from_slice(&self.get_node_content(current));
            if current == last {
                break;
            }
            current = self.tree.next(current);
        }
        JsString::from_units(str)
    }
    // #endregion
}
