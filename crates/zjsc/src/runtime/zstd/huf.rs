//! Compressor Huffman dos literais: porte de `lib/compress/huf_compress.c` (libzstd 1.5.7).
//!
//! Conteúdo: `HUF_optimalTableLog`, `HUF_buildCTable` (`HUF_sort`, `HUF_buildTree`, `HUF_setMaxHeight`),
//! `HUF_writeCTable` (pesos por FSE ou crus em 4 bits), `HUF_compress1X_usingCTable`,
//! `HUF_compress4X_usingCTable` (tabela de saltos de 6 bytes) e `HUF_compress_internal` com o
//! `HUF_repeat`, a heurística `preferRepeat`, a amostragem `suspectUncompressible` e o corte por ganho.
//!
//! O fluxo de bits do C (dois contêineres, desenrolamento por `tableLog`, escrita rápida ou lenta)
//! só muda a velocidade: os bytes são os de um acumulador LSB primeiro que recebe os símbolos de trás
//! para frente e fecha com a marca de fim. A checagem de estouro (`ptr >= endPtr` no fechamento) vira a
//! comparação dos bytes completos com `dst_size - 8`.

use std::ops::{Index, IndexMut};

use super::fse::{self, FseError};
use super::literals::{HufFlags, HufOutput, HufRequest, HUF_SYMBOLVALUE_MAX, LIT_HUF_LOG};
use super::params::highbit32;

/// `HUF_TABLELOG_MAX`.
pub const HUF_TABLELOG_MAX: u32 = 12;
/// `HUF_TABLELOG_DEFAULT`.
pub const HUF_TABLELOG_DEFAULT: u32 = 11;
/// `HUF_BLOCKSIZE_MAX`.
pub const HUF_BLOCKSIZE_MAX: usize = 128 * 1024;

const MAX_FSE_TABLELOG_FOR_HUFF_HEADER: u32 = 6;
const SUSPECT_INCOMPRESSIBLE_SAMPLE_SIZE: usize = 4096;
const SUSPECT_INCOMPRESSIBLE_SAMPLE_RATIO: usize = 10;
const RANK_POSITION_TABLE_SIZE: usize = 192;
const RANK_POSITION_MAX_COUNT_LOG: u32 = 32;
const RANK_POSITION_LOG_BUCKETS_BEGIN: u32 = (RANK_POSITION_TABLE_SIZE as u32 - 1) - RANK_POSITION_MAX_COUNT_LOG - 1;
const STARTNODE: i32 = HUF_SYMBOLVALUE_MAX as i32 + 1;
const NO_SYMBOL: u32 = 0xF0F0_F0F0;
/// Espaço que `HUF_optimalTableLog` dá a `HUF_writeCTable` (`sizeof(wksps) - sizeof(HUF_WriteCTableWksp)`
/// = 4864 - 752): sobra de longe para o cabeçalho de no máximo 129 bytes.
const OPTIMAL_DEPTH_HEADER_CAPACITY: usize = 4112;
/// Teto prático do buffer dos pesos: eles ocupam no máximo algumas centenas de bytes, então limitar a
/// capacidade aqui não muda nenhuma decisão de estouro do C.
const WEIGHTS_BUFFER_CAP: usize = 512;

/// Erros do libzstd que as funções desta fatia produzem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HufError {
    Generic,
    DstSizeTooSmall,
    TableLogTooLarge,
    MaxSymbolValueTooLarge,
    SrcSizeWrong,
}

impl From<FseError> for HufError {
    fn from(e: FseError) -> Self {
        match e {
            FseError::DstSizeTooSmall => HufError::DstSizeTooSmall,
            FseError::TableLogTooLarge => HufError::TableLogTooLarge,
            FseError::Generic | FseError::MaxSymbolValueTooSmall => HufError::Generic,
        }
    }
}

pub type HufResult<T> = Result<T, HufError>;

/// `HUF_repeat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HufRepeat {
    None,
    Check,
    Valid,
}

/// `HUF_CElt` desempacotado: `nb_bits` e o código `value` (de `nb_bits` bits, bit menos significativo
/// emitido primeiro).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HufElt {
    pub nb_bits: u8,
    pub value: u16,
}

/// `HUF_CElt[]` com o cabeçalho `HUF_CTableHeader`. As entradas além de `max_symbol_value` são zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HufCTable {
    pub table_log: u32,
    pub max_symbol_value: u32,
    pub elts: [HufElt; 256],
}

impl HufCTable {
    pub fn new() -> Self {
        Self { table_log: 0, max_symbol_value: 0, elts: [HufElt::default(); 256] }
    }
}

impl Default for HufCTable {
    fn default() -> Self {
        Self::new()
    }
}

/// A tabela anterior (`oldHufTable`) mais o `HUF_repeat`, o par que `HUF_compress*_repeat` atualiza.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HufState {
    pub table: HufCTable,
    pub repeat: HufRepeat,
}

impl HufState {
    pub fn new() -> Self {
        Self { table: HufCTable::new(), repeat: HufRepeat::None }
    }
}

impl Default for HufState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------------------------
// Árvore de Huffman
// ---------------------------------------------------------------------------------------------

/// `nodeElt`.
#[derive(Debug, Clone, Copy, Default)]
struct Node {
    count: u32,
    parent: u16,
    byte: u8,
    nb_bits: u8,
}

/// `huffNodeTable` com o nó falso `huffNode0[0]` do C: o índice -1 é a barreira do `HUF_buildTree`.
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    fn new() -> Self {
        Self { nodes: vec![Node::default(); 2 * (HUF_SYMBOLVALUE_MAX as usize + 1) + 1] }
    }

    /// `huffNode` (sem o nó falso), para o `HUF_sort`.
    fn real_nodes(&mut self) -> &mut [Node] {
        &mut self.nodes[1..]
    }
}

impl Index<i32> for Tree {
    type Output = Node;
    fn index(&self, i: i32) -> &Node {
        &self.nodes[(i + 1) as usize]
    }
}

impl IndexMut<i32> for Tree {
    fn index_mut(&mut self, i: i32) -> &mut Node {
        &mut self.nodes[(i + 1) as usize]
    }
}

#[derive(Clone, Copy, Default)]
struct RankPos {
    base: u16,
    curr: u16,
}

/// `RANK_POSITION_DISTINCT_COUNT_CUTOFF` (158 + highbit32(158) = 165).
fn distinct_count_cutoff() -> u32 {
    RANK_POSITION_LOG_BUCKETS_BEGIN + highbit32(RANK_POSITION_LOG_BUCKETS_BEGIN)
}

/// `HUF_getIndex`.
fn rank_index(count: u32) -> u32 {
    if count < distinct_count_cutoff() {
        count
    } else {
        highbit32(count) + RANK_POSITION_LOG_BUCKETS_BEGIN
    }
}

/// `HUF_insertionSort` (ordem decrescente) sobre `arr` inteiro.
fn insertion_sort(arr: &mut [Node]) {
    for i in 1..arr.len() {
        let key = arr[i];
        let mut j = i as isize - 1;
        while j >= 0 && arr[j as usize].count < key.count {
            arr[(j + 1) as usize] = arr[j as usize];
            j -= 1;
        }
        arr[(j + 1) as usize] = key;
    }
}

/// `HUF_quickSortPartition`: pivô é o último elemento.
fn quick_sort_partition(arr: &mut [Node], low: i32, high: i32) -> i32 {
    let pivot = arr[high as usize].count;
    let mut i = low - 1;
    for j in low..high {
        if arr[j as usize].count > pivot {
            i += 1;
            arr.swap(i as usize, j as usize);
        }
    }
    arr.swap((i + 1) as usize, high as usize);
    i + 1
}

/// `HUF_simpleQuickSort`. O C chama a inserção também com `high < low` (tamanho não positivo, sem efeito).
fn simple_quick_sort(arr: &mut [Node], mut low: i32, mut high: i32) {
    const INSERTION_SORT_THRESHOLD: i32 = 8;
    if high - low < INSERTION_SORT_THRESHOLD {
        if high > low {
            insertion_sort(&mut arr[low as usize..=high as usize]);
        }
        return;
    }
    while low < high {
        let idx = quick_sort_partition(arr, low, high);
        if idx - low < high - idx {
            simple_quick_sort(arr, low, idx - 1);
            low = idx + 1;
        } else {
            simple_quick_sort(arr, idx + 1, high);
            high = idx - 1;
        }
    }
}

/// `HUF_sort`: ordena os símbolos `[0, max_symbol_value]` por contagem decrescente.
fn sort(nodes: &mut [Node], count: &[u32], max_symbol_value: u32) {
    let m1 = max_symbol_value as usize + 1;
    let mut rank = [RankPos::default(); RANK_POSITION_TABLE_SIZE];
    for &c in &count[..m1] {
        rank[rank_index(c) as usize].base += 1;
    }
    for n in (1..RANK_POSITION_TABLE_SIZE).rev() {
        rank[n - 1].base += rank[n].base;
        rank[n - 1].curr = rank[n - 1].base;
    }
    for (n, &c) in count[..m1].iter().enumerate() {
        let r = rank_index(c) as usize + 1;
        let pos = rank[r].curr as usize;
        rank[r].curr += 1;
        nodes[pos].count = c;
        nodes[pos].byte = n as u8;
    }
    for n in distinct_count_cutoff() as usize..RANK_POSITION_TABLE_SIZE - 1 {
        let bucket_size = i32::from(rank[n].curr - rank[n].base);
        let start = rank[n].base as usize;
        if bucket_size > 1 {
            simple_quick_sort(&mut nodes[start..], 0, bucket_size - 1);
        }
    }
}

/// `HUF_buildTree`: árvore sem limite de altura sobre os nós ordenados. Devolve `nonNullRank`.
fn build_tree(h: &mut Tree, max_symbol_value: u32) -> i32 {
    let mut non_null_rank = max_symbol_value as i32;
    while h[non_null_rank].count == 0 {
        non_null_rank -= 1;
    }
    let mut low_s = non_null_rank;
    let mut node_nb = STARTNODE;
    let node_root = node_nb + low_s - 1;
    let mut low_n = node_nb;
    h[node_nb].count = h[low_s].count + h[low_s - 1].count;
    h[low_s].parent = node_nb as u16;
    h[low_s - 1].parent = node_nb as u16;
    node_nb += 1;
    low_s -= 2;
    for n in node_nb..=node_root {
        h[n].count = 1 << 30;
    }
    h[-1].count = 1 << 31; // entrada falsa, barreira forte

    while node_nb <= node_root {
        let n1 = if h[low_s].count < h[low_n].count {
            low_s -= 1;
            low_s + 1
        } else {
            low_n += 1;
            low_n - 1
        };
        let n2 = if h[low_s].count < h[low_n].count {
            low_s -= 1;
            low_s + 1
        } else {
            low_n += 1;
            low_n - 1
        };
        h[node_nb].count = h[n1].count + h[n2].count;
        h[n1].parent = node_nb as u16;
        h[n2].parent = node_nb as u16;
        node_nb += 1;
    }

    h[node_root].nb_bits = 0;
    for n in (STARTNODE..node_root).rev() {
        let parent = i32::from(h[n].parent);
        h[n].nb_bits = h[parent].nb_bits + 1;
    }
    for n in 0..=non_null_rank {
        let parent = i32::from(h[n].parent);
        h[n].nb_bits = h[parent].nb_bits + 1;
    }
    non_null_rank
}

/// `HUF_setMaxHeight`: força a altura da árvore a `target_nb_bits` mantendo-a canônica válida.
fn set_max_height(h: &mut Tree, last_non_null: u32, target_nb_bits: u32) -> HufResult<u32> {
    let largest_bits = u32::from(h[last_non_null as i32].nb_bits);
    if largest_bits <= target_nb_bits {
        return Ok(largest_bits);
    }
    let base_cost: i32 = 1 << (largest_bits - target_nb_bits);
    let mut total_cost: i32 = 0;
    let mut n = last_non_null as i32;
    while u32::from(h[n].nb_bits) > target_nb_bits {
        total_cost += base_cost - (1 << (largest_bits - u32::from(h[n].nb_bits)));
        h[n].nb_bits = target_nb_bits as u8;
        n -= 1;
    }
    while u32::from(h[n].nb_bits) == target_nb_bits {
        n -= 1;
    }
    total_cost >>= largest_bits - target_nb_bits;

    let mut rank_last = [NO_SYMBOL; HUF_TABLELOG_MAX as usize + 2];
    let mut current_nb_bits = target_nb_bits;
    let mut pos = n;
    while pos >= 0 {
        let nb = u32::from(h[pos].nb_bits);
        if nb < current_nb_bits {
            current_nb_bits = nb;
            rank_last[(target_nb_bits - current_nb_bits) as usize] = pos as u32;
        }
        pos -= 1;
    }

    while total_cost > 0 {
        let mut n_bits_to_decrease = highbit32(total_cost as u32) + 1;
        while n_bits_to_decrease > 1 {
            let high_pos = rank_last[n_bits_to_decrease as usize];
            let low_pos = rank_last[n_bits_to_decrease as usize - 1];
            if high_pos != NO_SYMBOL {
                if low_pos == NO_SYMBOL {
                    break;
                }
                let high_total = h[high_pos as i32].count;
                let low_total = 2 * h[low_pos as i32].count;
                if high_total <= low_total {
                    break;
                }
            }
            n_bits_to_decrease -= 1;
        }
        while n_bits_to_decrease <= HUF_TABLELOG_MAX && rank_last[n_bits_to_decrease as usize] == NO_SYMBOL {
            n_bits_to_decrease += 1;
        }
        let k = n_bits_to_decrease as usize;
        if rank_last[k] == NO_SYMBOL {
            return Err(HufError::Generic); // o C afirma que existe sempre um símbolo aqui
        }
        total_cost -= 1 << (n_bits_to_decrease - 1);
        let sym = rank_last[k] as i32;
        h[sym].nb_bits = h[sym].nb_bits.wrapping_add(1);
        if rank_last[k - 1] == NO_SYMBOL {
            rank_last[k - 1] = rank_last[k];
        }
        if rank_last[k] == 0 {
            rank_last[k] = NO_SYMBOL;
        } else {
            rank_last[k] -= 1;
            if i64::from(h[rank_last[k] as i32].nb_bits) != i64::from(target_nb_bits) - k as i64 {
                rank_last[k] = NO_SYMBOL;
            }
        }
    }

    while total_cost < 0 {
        if rank_last[1] == NO_SYMBOL {
            while u32::from(h[n].nb_bits) == target_nb_bits {
                n -= 1;
            }
            h[n + 1].nb_bits = h[n + 1].nb_bits.wrapping_sub(1);
            rank_last[1] = (n + 1) as u32;
            total_cost += 1;
            continue;
        }
        let idx = rank_last[1] as i32 + 1;
        h[idx].nb_bits = h[idx].nb_bits.wrapping_sub(1);
        rank_last[1] += 1;
        total_cost += 1;
    }
    Ok(target_nb_bits)
}

/// `HUF_buildCTableFromTree`.
fn ctable_from_tree(h: &Tree, non_null_rank: i32, max_symbol_value: u32, max_nb_bits: u32) -> HufCTable {
    let mut ct = HufCTable::new();
    let mut nb_per_rank = [0u16; HUF_TABLELOG_MAX as usize + 1];
    let mut val_per_rank = [0u16; HUF_TABLELOG_MAX as usize + 1];
    let alphabet_size = max_symbol_value as i32 + 1;
    for n in 0..=non_null_rank {
        nb_per_rank[h[n].nb_bits as usize] += 1;
    }
    let mut min: u16 = 0;
    for n in (1..=max_nb_bits as usize).rev() {
        val_per_rank[n] = min;
        min += nb_per_rank[n];
        min >>= 1;
    }
    for n in 0..alphabet_size {
        ct.elts[h[n].byte as usize].nb_bits = h[n].nb_bits;
    }
    for n in 0..alphabet_size as usize {
        let rank = ct.elts[n].nb_bits as usize;
        ct.elts[n].value = if rank > 0 { val_per_rank[rank] } else { 0 };
        val_per_rank[rank] += 1;
    }
    ct.table_log = max_nb_bits;
    ct.max_symbol_value = max_symbol_value;
    ct
}

/// `HUF_buildCTable_wksp`. `count` tem pelo menos `max_symbol_value + 1` entradas e ao menos dois
/// símbolos presentes (o caso de um símbolo só é RLE e nunca chega aqui).
pub fn build_ctable(count: &[u32], max_symbol_value: u32, max_nb_bits: u32) -> HufResult<HufCTable> {
    let max_nb_bits = if max_nb_bits == 0 { HUF_TABLELOG_DEFAULT } else { max_nb_bits };
    if max_symbol_value > HUF_SYMBOLVALUE_MAX {
        return Err(HufError::MaxSymbolValueTooLarge);
    }
    if count.len() <= max_symbol_value as usize || max_symbol_value == 0 {
        return Err(HufError::Generic);
    }
    if count[..=max_symbol_value as usize].iter().filter(|&&c| c != 0).count() < 2 {
        return Err(HufError::Generic); // um símbolo só é RLE e o C nunca chega aqui
    }
    let mut tree = Tree::new();
    sort(tree.real_nodes(), count, max_symbol_value);
    let non_null_rank = build_tree(&mut tree, max_symbol_value);
    let max_nb_bits = set_max_height(&mut tree, non_null_rank as u32, max_nb_bits)?;
    if max_nb_bits > HUF_TABLELOG_MAX {
        return Err(HufError::Generic);
    }
    Ok(ctable_from_tree(&tree, non_null_rank, max_symbol_value, max_nb_bits))
}

/// `HUF_estimateCompressedSize`.
pub fn estimate_compressed_size(ct: &HufCTable, count: &[u32], max_symbol_value: u32) -> usize {
    let nb_bits: usize = (0..=max_symbol_value as usize).map(|s| ct.elts[s].nb_bits as usize * count[s] as usize).sum();
    nb_bits >> 3
}

/// `HUF_validateCTable`.
pub fn validate_ctable(ct: &HufCTable, count: &[u32], max_symbol_value: u32) -> bool {
    ct.max_symbol_value >= max_symbol_value
        && (0..=max_symbol_value as usize).all(|s| count[s] == 0 || ct.elts[s].nb_bits != 0)
}

// ---------------------------------------------------------------------------------------------
// Cabeçalho da tabela
// ---------------------------------------------------------------------------------------------

/// Resultado de `HUF_compressWeights`: o C devolve 0 (não comprime), 1 (um símbolo só, RLE) ou o tamanho.
enum Weights {
    NotCompressible,
    Rle,
    Fse(Vec<u8>),
}

/// `HUF_compressWeights`.
fn compress_weights(dst_size: usize, weights: &[u8]) -> HufResult<Weights> {
    if weights.len() <= 1 {
        return Ok(Weights::NotCompressible);
    }
    let mut count = [0u32; HUF_TABLELOG_MAX as usize + 1];
    let (max_count, max_symbol_value) = fse::hist_count(&mut count, HUF_TABLELOG_MAX, weights)?;
    if max_count as usize == weights.len() {
        return Ok(Weights::Rle);
    }
    if max_count == 1 {
        return Ok(Weights::NotCompressible);
    }
    let table_log = fse::optimal_table_log(MAX_FSE_TABLELOG_FOR_HUFF_HEADER, weights.len(), max_symbol_value)?;
    let mut norm = [0i16; HUF_TABLELOG_MAX as usize + 1];
    fse::normalize_count(&mut norm, table_log, &count, weights.len(), max_symbol_value, false)?;
    let mut header = vec![0u8; dst_size.min(fse::FSE_NCOUNTBOUND)];
    let h_size = fse::write_ncount(&mut header, &norm, max_symbol_value, table_log)?;
    header.truncate(h_size);
    let ct = fse::build_ctable(&norm, max_symbol_value, table_log)?;
    match fse::compress_using_ctable((dst_size - h_size).min(WEIGHTS_BUFFER_CAP), weights, &ct)? {
        Some(payload) => {
            header.extend_from_slice(&payload);
            Ok(Weights::Fse(header))
        }
        None => Ok(Weights::NotCompressible),
    }
}

/// `HUF_writeCTable_wksp`: cabeçalho da tabela (pesos por FSE ou crus em 4 bits).
pub fn write_ctable(dst_size: usize, ct: &HufCTable, max_symbol_value: u32, huff_log: u32) -> HufResult<Vec<u8>> {
    if max_symbol_value > HUF_SYMBOLVALUE_MAX {
        return Err(HufError::MaxSymbolValueTooLarge);
    }
    if huff_log > HUF_TABLELOG_MAX || max_symbol_value == 0 {
        return Err(HufError::Generic);
    }
    let max = max_symbol_value as usize;
    let mut bits_to_weight = [0u8; HUF_TABLELOG_MAX as usize + 1];
    for n in 1..=huff_log as usize {
        bits_to_weight[n] = (huff_log as usize + 1 - n) as u8;
    }
    let mut weights = [0u8; HUF_SYMBOLVALUE_MAX as usize + 1];
    for n in 0..max {
        weights[n] = *bits_to_weight.get(ct.elts[n].nb_bits as usize).ok_or(HufError::Generic)?;
    }

    if dst_size < 1 {
        return Err(HufError::DstSizeTooSmall);
    }
    if let Weights::Fse(compressed) = compress_weights(dst_size - 1, &weights[..max])? {
        let h_size = compressed.len();
        if h_size > 1 && h_size < max / 2 {
            let mut out = Vec::with_capacity(h_size + 1);
            out.push(h_size as u8);
            out.extend_from_slice(&compressed);
            return Ok(out);
        }
    }

    // pesos crus em 4 bits (máximo 15)
    if max > 256 - 128 {
        return Err(HufError::Generic);
    }
    if (max + 1) / 2 + 1 > dst_size {
        return Err(HufError::DstSizeTooSmall);
    }
    let mut out = Vec::with_capacity((max + 1) / 2 + 1);
    out.push((128 + (max - 1)) as u8);
    weights[max] = 0;
    for n in (0..max).step_by(2) {
        out.push((weights[n] << 4) + weights[n + 1]);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Escolha do tableLog
// ---------------------------------------------------------------------------------------------

/// `HUF_optimalTableLog`. `count` já tem o histograma até `max_symbol_value`.
pub fn optimal_table_log(max_table_log: u32, src_size: usize, max_symbol_value: u32, count: &[u32], optimal_depth: bool) -> HufResult<u32> {
    if !optimal_depth {
        return Ok(fse::optimal_table_log_internal(max_table_log, src_size, max_symbol_value, 1)?);
    }
    let cardinality = count[..=max_symbol_value as usize].iter().filter(|&&c| c != 0).count() as u32;
    let min_table_log = highbit32(cardinality) + 1;
    let mut opt_size = usize::MAX - 1;
    let mut opt_log = max_table_log;
    for guess in min_table_log..=max_table_log {
        let Ok(table) = build_ctable(count, max_symbol_value, guess) else {
            continue;
        };
        let max_bits = table.table_log;
        if max_bits < guess && guess > min_table_log {
            break;
        }
        let Ok(header) = write_ctable(OPTIMAL_DEPTH_HEADER_CAPACITY, &table, max_symbol_value, max_bits) else {
            continue;
        };
        let new_size = estimate_compressed_size(&table, count, max_symbol_value) + header.len();
        if new_size > opt_size + 1 {
            break;
        }
        if new_size < opt_size {
            opt_size = new_size;
            opt_log = guess;
        }
    }
    Ok(opt_log)
}

// ---------------------------------------------------------------------------------------------
// Compressão com a tabela pronta
// ---------------------------------------------------------------------------------------------

/// `HUF_compress1X_usingCTable`: um fluxo. `None` é o retorno 0 do C (sem espaço em `dst_size`).
pub fn compress_1x_using_ctable(dst_size: usize, src: &[u8], ct: &HufCTable) -> Option<Vec<u8>> {
    // `dstSize < 8` sai direto e `HUF_initCStream` rejeita `dstCapacity <= 8`.
    if dst_size <= 8 {
        return None;
    }
    let mut out = Vec::with_capacity(src.len() * ct.table_log as usize / 8 + 9);
    let mut acc: u64 = 0;
    let mut nb_acc: u32 = 0;
    let mut add = |out: &mut Vec<u8>, value: u64, nb_bits: u32| {
        acc |= value << nb_acc;
        nb_acc += nb_bits;
        while nb_acc >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            nb_acc -= 8;
        }
    };
    for &b in src.iter().rev() {
        let e = ct.elts[b as usize];
        add(&mut out, u64::from(e.value), u32::from(e.nb_bits));
    }
    add(&mut out, 1, 1); // marca de fim
    if out.len() >= dst_size - 8 {
        return None; // estouro detectado no fechamento
    }
    if nb_acc > 0 {
        out.push(acc as u8);
    }
    Some(out)
}

/// `HUF_compress4X_usingCTable`: quatro fluxos e a tabela de saltos (três tamanhos de 16 bits).
pub fn compress_4x_using_ctable(dst_size: usize, src: &[u8], ct: &HufCTable) -> Option<Vec<u8>> {
    if dst_size < 6 + 1 + 1 + 1 + 8 || src.len() < 12 {
        return None;
    }
    let segment = (src.len() + 3) / 4;
    let mut out = vec![0u8; 6];
    let mut available = dst_size - 6;
    for i in 0..4 {
        let part = if i < 3 { &src[i * segment..(i + 1) * segment] } else { &src[3 * segment..] };
        let stream = compress_1x_using_ctable(available, part, ct)?;
        if stream.len() > 65535 {
            return None;
        }
        if i < 3 {
            out[2 * i..2 * i + 2].copy_from_slice(&(stream.len() as u16).to_le_bytes());
        }
        available -= stream.len();
        out.extend_from_slice(&stream);
    }
    Some(out)
}

/// `HUF_compressCTable_internal`: `prefix` é o cabeçalho já escrito (vazio ao reaproveitar a tabela).
/// Vetor vazio é o retorno 0 do C (incompressível).
fn compress_ctable_internal(prefix: &[u8], dst_size: usize, src: &[u8], four_streams: bool, ct: &HufCTable) -> HufResult<Vec<u8>> {
    let available = dst_size.checked_sub(prefix.len()).ok_or(HufError::DstSizeTooSmall)?;
    let body = if four_streams { compress_4x_using_ctable(available, src, ct) } else { compress_1x_using_ctable(available, src, ct) };
    let Some(body) = body else {
        return Ok(Vec::new());
    };
    if prefix.len() + body.len() >= src.len() - 1 {
        return Ok(Vec::new());
    }
    let mut out = Vec::with_capacity(prefix.len() + body.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(&body);
    Ok(out)
}

/// `HUF_compress_internal`. Vetor vazio é o retorno 0 do C; um byte só é o RLE (retorno 1).
/// `state` faz o papel de `oldHufTable` e `*repeat`: ao construir tabela nova ela é gravada em
/// `state.table` e o `repeat` vira `None`.
pub fn compress_internal(
    src: &[u8],
    max_symbol_value: u32,
    huff_log: u32,
    four_streams: bool,
    dst_size: usize,
    state: &mut HufState,
    flags: HufFlags,
) -> HufResult<Vec<u8>> {
    if src.is_empty() || dst_size == 0 {
        return Ok(Vec::new());
    }
    if src.len() > HUF_BLOCKSIZE_MAX {
        return Err(HufError::SrcSizeWrong);
    }
    if huff_log > HUF_TABLELOG_MAX {
        return Err(HufError::TableLogTooLarge);
    }
    if max_symbol_value > HUF_SYMBOLVALUE_MAX {
        return Err(HufError::MaxSymbolValueTooLarge);
    }
    let max_symbol_value = if max_symbol_value == 0 { HUF_SYMBOLVALUE_MAX } else { max_symbol_value };
    let huff_log = if huff_log == 0 { HUF_TABLELOG_DEFAULT } else { huff_log };
    let src_size = src.len();

    // Heurística: tabela anterior válida serve para entradas pequenas.
    if flags.prefer_repeat && state.repeat == HufRepeat::Valid {
        return compress_ctable_internal(&[], dst_size, src, four_streams, &state.table);
    }

    // Suspeita de incompressível: amostra pequena primeiro.
    let sample = SUSPECT_INCOMPRESSIBLE_SAMPLE_SIZE;
    if flags.suspect_uncompressible && src_size >= sample * SUSPECT_INCOMPRESSIBLE_SAMPLE_RATIO {
        let mut sample_count = [0u32; HUF_SYMBOLVALUE_MAX as usize + 1];
        let (largest_begin, _) = fse::hist_count(&mut sample_count, max_symbol_value, &src[..sample])?;
        let (largest_end, _) = fse::hist_count(&mut sample_count, max_symbol_value, &src[src_size - sample..])?;
        if (largest_begin + largest_end) as usize <= ((2 * sample) >> 7) + 4 {
            return Ok(Vec::new());
        }
    }

    // Histograma.
    let mut count = [0u32; HUF_SYMBOLVALUE_MAX as usize + 1];
    let (largest, max_symbol_value) = fse::hist_count(&mut count, max_symbol_value, src)?;
    if largest as usize == src_size {
        return Ok(vec![src[0]]); // símbolo único, RLE
    }
    if largest as usize <= (src_size >> 7) + 4 {
        return Ok(Vec::new()); // provavelmente não comprime o bastante
    }

    // Validade da tabela anterior.
    if state.repeat == HufRepeat::Check && !validate_ctable(&state.table, &count, max_symbol_value) {
        state.repeat = HufRepeat::None;
    }
    if flags.prefer_repeat && state.repeat != HufRepeat::None {
        return compress_ctable_internal(&[], dst_size, src, four_streams, &state.table);
    }

    // Árvore nova.
    let huff_log = optimal_table_log(huff_log, src_size, max_symbol_value, &count, flags.optimal_depth)?;
    let table = build_ctable(&count, max_symbol_value, huff_log)?;
    let huff_log = table.table_log;

    // Cabeçalho da tabela.
    let header = write_ctable(dst_size, &table, max_symbol_value, huff_log)?;
    let h_size = header.len();
    if state.repeat != HufRepeat::None {
        let old_size = estimate_compressed_size(&state.table, &count, max_symbol_value);
        let new_size = estimate_compressed_size(&table, &count, max_symbol_value);
        if old_size <= h_size + new_size || h_size + 12 >= src_size {
            return compress_ctable_internal(&[], dst_size, src, four_streams, &state.table);
        }
    }

    // Usa a tabela nova.
    if h_size + 12 >= src_size {
        return Ok(Vec::new());
    }
    state.repeat = HufRepeat::None;
    state.table = table.clone();
    compress_ctable_internal(&header, dst_size, src, four_streams, &table)
}

/// `HUF_compress1X_repeat` ou `HUF_compress4X_repeat` conforme o pedido, com os parâmetros que
/// `ZSTD_compressLiterals` usa (`HUF_SYMBOLVALUE_MAX`, `LitHufLog`). `None` equivale a `cLitSize == 0` ou erro.
///
/// `state` é a cópia de trabalho (`nextHuf`, copiada de `prevHuf` antes da chamada): quem chama só a
/// promove quando a seção volta como `TableState::NewlyBuilt` (e então com `repeat = Check`); nos
/// demais casos descarta a cópia.
pub fn compress_literals_huf(src: &[u8], request: HufRequest, dst_capacity: usize, state: &mut HufState) -> Option<HufOutput> {
    let bytes = compress_internal(src, HUF_SYMBOLVALUE_MAX, LIT_HUF_LOG, request.four_streams, dst_capacity, state, request.flags).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(HufOutput { bytes, reused_table: state.repeat != HufRepeat::None })
}

/// O closure `encode` de `literals::compress_literals`, preso a `src`, à capacidade de saída e ao estado.
pub fn literals_encoder<'a>(
    src: &'a [u8],
    dst_capacity: usize,
    state: &'a mut HufState,
) -> impl FnOnce(HufRequest) -> Option<HufOutput> + 'a {
    move |request| compress_literals_huf(src, request, dst_capacity, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_FLAGS: HufFlags = HufFlags { prefer_repeat: false, optimal_depth: false, suspect_uncompressible: false };

    fn tree_from(counts: &[u32], bits: &[u8]) -> Tree {
        let mut t = Tree::new();
        for (i, (&c, &b)) in counts.iter().zip(bits).enumerate() {
            t[i as i32].count = c;
            t[i as i32].nb_bits = b;
        }
        t
    }

    #[test]
    fn rank_index_buckets() {
        // 158 + highbit32(158) = 158 + 7 = 165 é o corte
        assert_eq!(distinct_count_cutoff(), 165);
        assert_eq!(rank_index(1), 1);
        assert_eq!(rank_index(164), 164);
        assert_eq!(rank_index(165), 7 + 158);
        assert_eq!(rank_index(1000), 9 + 158);
    }

    #[test]
    fn build_ctable_four_symbols() {
        // contagens 4,2,1,1: alturas 1,2,3,3 e códigos 1, 01, 000, 001 (traçado à mão do HUF_buildTree)
        let ct = build_ctable(&[4, 2, 1, 1], 3, 0).unwrap();
        assert_eq!(ct.table_log, 3);
        assert_eq!(ct.max_symbol_value, 3);
        let got: Vec<(u8, u16)> = ct.elts[..4].iter().map(|e| (e.nb_bits, e.value)).collect();
        assert_eq!(got, vec![(1, 1), (2, 1), (3, 0), (3, 1)]);
    }

    #[test]
    fn set_max_height_repays_cost() {
        // contagens 8,4,2,1,1 com alturas 1,2,3,4,4 e alvo 3: o símbolo 1 sobe para 3 bits
        let mut t = tree_from(&[8, 4, 2, 1, 1], &[1, 2, 3, 4, 4]);
        assert_eq!(set_max_height(&mut t, 4, 3), Ok(3));
        let bits: Vec<u8> = (0..5).map(|i| t[i].nb_bits).collect();
        assert_eq!(bits, vec![1, 3, 3, 3, 3]);
    }

    #[test]
    fn build_ctable_enforces_max_height() {
        let ct = build_ctable(&[8, 4, 2, 1, 1], 4, 3).unwrap();
        assert_eq!(ct.table_log, 3);
        let got: Vec<(u8, u16)> = ct.elts[..5].iter().map(|e| (e.nb_bits, e.value)).collect();
        assert_eq!(got, vec![(1, 1), (3, 0), (3, 1), (3, 2), (3, 3)]);
    }

    #[test]
    fn write_ctable_raw_weights() {
        // alturas 1,2,3,3: pesos 3,2,1 (não comprimem por FSE, cada um aparece uma vez);
        // 128 + (3 - 1) = 0x82, depois (3 << 4) + 2 e (1 << 4) + 0
        let ct = build_ctable(&[4, 2, 1, 1], 3, 0).unwrap();
        assert_eq!(write_ctable(64, &ct, 3, 3).unwrap(), vec![0x82, 0x32, 0x10]);
        assert_eq!(write_ctable(0, &ct, 3, 3), Err(HufError::DstSizeTooSmall));
    }

    #[test]
    fn compress_1x_hand_traced_bits() {
        // src 0,1,2,3,0,0 lido de trás para frente: 0,0,3,2,1,0 com códigos 1, 1, 001, 000, 01, 1 e a marca
        // de fim: bits 0..11 = 1,1,1,0,0,0,0,0,1,0,1,1 => 0x07 e 0x0D
        let ct = build_ctable(&[4, 2, 1, 1], 3, 0).unwrap();
        assert_eq!(compress_1x_using_ctable(16, &[0, 1, 2, 3, 0, 0], &ct), Some(vec![0x07, 0x0D]));
        // dst_size <= 8 nunca cabe; e 1 byte completo (F = 1) já estoura com dst_size = 9
        assert_eq!(compress_1x_using_ctable(8, &[0, 1], &ct), None);
        assert_eq!(compress_1x_using_ctable(9, &[0, 1, 2, 3, 0, 0], &ct), None);
    }

    #[test]
    fn compress_4x_jump_table() {
        // 12 símbolos 0: segmentos de 3, cada um com 1,1,1 e a marca de fim = 0x0F, um byte
        let ct = build_ctable(&[4, 2, 1, 1], 3, 0).unwrap();
        let out = compress_4x_using_ctable(64, &[0; 12], &ct).unwrap();
        assert_eq!(out, vec![1, 0, 1, 0, 1, 0, 0x0F, 0x0F, 0x0F, 0x0F]);
        assert_eq!(compress_4x_using_ctable(16, &[0; 12], &ct), None);
        assert_eq!(compress_4x_using_ctable(64, &[0; 11], &ct), None);
    }

    fn sample_128() -> Vec<u8> {
        let mut v = vec![0u8; 64];
        v.extend(std::iter::repeat(1u8).take(32));
        v.extend(std::iter::repeat(2u8).take(16));
        v.extend(std::iter::repeat(3u8).take(16));
        v
    }

    #[test]
    fn compress_internal_new_table_single_stream() {
        // contagens 64,32,16,16: tableLog = clamp(min(max(5,3)...)) = 5, alturas 1,2,3,3 => cabeçalho cru de 3
        // bytes; 224 bits + marca = 225 bits = 28 bytes e 1 bit => 29 bytes de fluxo.
        // De trás para frente: 16 vezes '001' (49 92 24 49 92 24), 16 vezes '000' (6 zeros),
        // 32 vezes '01' (8 vezes 0x55), 64 uns + marca (8 vezes 0xFF e 0x01).
        let src = sample_128();
        let mut state = HufState::new();
        let out = compress_internal(&src, 255, 11, false, 256, &mut state, NO_FLAGS).unwrap();
        let mut want = vec![0x82, 0x32, 0x10, 0x49, 0x92, 0x24, 0x49, 0x92, 0x24];
        want.extend([0u8; 6]);
        want.extend([0x55u8; 8]);
        want.extend([0xFFu8; 8]);
        want.push(0x01);
        assert_eq!(out, want);
        assert_eq!(state.repeat, HufRepeat::None);
        assert_eq!(state.table.table_log, 3);
    }

    #[test]
    fn compress_internal_rle_and_heuristics() {
        let mut state = HufState::new();
        assert_eq!(compress_internal(&[7; 50], 255, 11, false, 256, &mut state, NO_FLAGS), Ok(vec![7]));
        assert_eq!(compress_internal(&[], 255, 11, false, 256, &mut state, NO_FLAGS), Ok(vec![]));
        assert_eq!(compress_internal(&[1, 2, 3], 255, 11, false, 0, &mut state, NO_FLAGS), Ok(vec![]));
        // largest <= (srcSize >> 7) + 4 => 0
        let flat: Vec<u8> = (0..=255u8).collect();
        assert_eq!(compress_internal(&flat, 255, 11, false, 512, &mut state, NO_FLAGS), Ok(vec![]));
        assert_eq!(compress_internal(&flat, 255, 13, false, 512, &mut state, NO_FLAGS), Err(HufError::TableLogTooLarge));
    }

    #[test]
    fn compress_internal_reuses_valid_table() {
        let src = sample_128();
        let mut state = HufState::new();
        let first = compress_internal(&src, 255, 11, false, 256, &mut state, NO_FLAGS).unwrap();
        // com a tabela anterior válida e preferRepeat, não há cabeçalho: só o fluxo (29 bytes)
        state.repeat = HufRepeat::Valid;
        let flags = HufFlags { prefer_repeat: true, ..NO_FLAGS };
        let again = compress_internal(&src, 255, 11, false, 256, &mut state, flags).unwrap();
        assert_eq!(again, first[3..].to_vec());
        let enc = compress_literals_huf(&src, HufRequest { four_streams: false, flags }, 256, &mut state).unwrap();
        assert!(enc.reused_table);
        assert_eq!(enc.bytes, again);
    }

    #[test]
    fn suspect_uncompressible_samples_ends() {
        // 40960 bytes planos: soma dos maiores das duas amostras de 4096 = 32 <= 68 => 0
        let src: Vec<u8> = (0..40960u32).map(|i| (i % 256) as u8).collect();
        let mut state = HufState::new();
        let flags = HufFlags { suspect_uncompressible: true, ..NO_FLAGS };
        assert_eq!(compress_internal(&src, 255, 11, false, 50000, &mut state, flags), Ok(vec![]));
    }

    #[test]
    fn optimal_table_log_cheap_path() {
        // FSE_optimalTableLog_internal(11, 128, 3, 1): maxBitsSrc = 6 - 1 = 5, minBits = 3 => 5
        assert_eq!(optimal_table_log(11, 128, 3, &[64, 32, 16, 16], false), Ok(5));
    }
}
