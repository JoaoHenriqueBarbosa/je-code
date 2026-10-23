//! Porte de `src/vs/editor/contrib/folding/browser/foldingRanges.ts`: as regiões de dobra
//! (`FoldingRegions`, `FoldingRegion`), `FoldRange`, `FoldSource` e a mescla `sanitizeAndMerge`.
//!
//! Decisões de tradução:
//! - `Uint32Array` é `Vec<u32>`, e o empacotamento do original é mantido bit a bit: o índice do pai de cada
//!   região vive nos 8 bits altos de `_startIndexes[i]` (bits 0 a 7 do pai) e de `_endIndexes[i]` (bits 8 a
//!   15 do pai), por isso os números de linha não passam de [`MAX_LINE_NUMBER`].
//! - O estado interno (`_startIndexes`, `_endIndexes`, os `BitField`) é mutável por referência
//!   compartilhada (`RefCell`), como o objeto do original: `ensureParentIndices` muda os vetores dentro de
//!   um `getParentIndex` de leitura, e `setCollapsed` é chamado com a região emprestada por
//!   [`FoldingRegion`].
//! - A leitura fora dos limites de um `Uint32Array` dá `undefined` (e `undefined & x` é 0) e a escrita é
//!   ignorada; os acessos daqui seguem isso em vez de entrar em pânico.
//! - O construtor lança `Error` no original; aqui é `panic!` com a mesma mensagem.
//! - `FoldingRegions | FoldRange[]` de `sanitizeAndMerge` é [`FoldingRangesInput`]. O original muda os
//!   objetos `FoldRange` das listas de entrada (`isCollapsed`, `source`) e devolve esses mesmos objetos;
//!   aqui as entradas são copiadas e o resultado é de valores novos, então as listas do chamador não
//!   são alteradas. O resultado é o mesmo.
//! - `type` é `r#type: Option<String>` (`string | undefined`); número de linha é `u32`.

use std::cell::{Cell, RefCell};

use super::SelectedLines;

/// `ILineRange`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ILineRange {
    pub start_line_number: u32,
    pub end_line_number: u32,
}

/// `FoldSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldSource {
    Provider = 0,
    UserDefined = 1,
    Recovered = 2,
}

/// `foldSourceAbbr[source]`.
pub fn fold_source_abbr(source: FoldSource) -> char {
    match source {
        FoldSource::Provider => ' ',
        FoldSource::UserDefined => 'u',
        FoldSource::Recovered => 'r',
    }
}

/// `FoldRange`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldRange {
    pub start_line_number: u32,
    pub end_line_number: u32,
    pub r#type: Option<String>,
    pub is_collapsed: bool,
    pub source: FoldSource,
}

pub const MAX_FOLDING_REGIONS: usize = 0xFFFF;
pub const MAX_LINE_NUMBER: u32 = 0xFF_FFFF;

const MASK_INDENT: u32 = 0xFF00_0000;

/// `BitField`.
struct BitField {
    states: RefCell<Vec<u32>>,
}

impl BitField {
    fn new(size: usize) -> Self {
        let num_words = size.div_ceil(32);
        Self {
            states: RefCell::new(vec![0; num_words]),
        }
    }

    fn get(&self, index: usize) -> bool {
        let array_index = index / 32;
        let bit = index % 32;
        self.states
            .borrow()
            .get(array_index)
            .is_some_and(|word| word & (1u32 << bit) != 0)
    }

    fn set(&self, index: usize, new_state: bool) {
        let array_index = index / 32;
        let bit = index % 32;
        if let Some(word) = self.states.borrow_mut().get_mut(array_index) {
            if new_state {
                *word |= 1u32 << bit;
            } else {
                *word &= !(1u32 << bit);
            }
        }
    }
}

/// `FoldingRegions`.
pub struct FoldingRegions {
    start_indexes: RefCell<Vec<u32>>,
    end_indexes: RefCell<Vec<u32>>,
    collapse_states: BitField,
    user_defined_states: BitField,
    recovered_states: BitField,
    parents_computed: Cell<bool>,
    types: Option<Vec<Option<String>>>,
}

/// `FoldingRegions | FoldRange[]`, a entrada de [`FoldingRegions::sanitize_and_merge`].
#[derive(Clone, Copy)]
pub enum FoldingRangesInput<'a> {
    Regions(&'a FoldingRegions),
    Ranges(&'a [FoldRange]),
}

impl<'a> From<&'a FoldingRegions> for FoldingRangesInput<'a> {
    fn from(regions: &'a FoldingRegions) -> Self {
        Self::Regions(regions)
    }
}

impl<'a> From<&'a [FoldRange]> for FoldingRangesInput<'a> {
    fn from(ranges: &'a [FoldRange]) -> Self {
        Self::Ranges(ranges)
    }
}

impl<'a> From<&'a Vec<FoldRange>> for FoldingRangesInput<'a> {
    fn from(ranges: &'a Vec<FoldRange>) -> Self {
        Self::Ranges(ranges.as_slice())
    }
}

impl FoldingRangesInput<'_> {
    fn length(&self) -> usize {
        match self {
            Self::Regions(regions) => regions.length(),
            Self::Ranges(ranges) => ranges.len(),
        }
    }

    /// O `getIndexedFunction` do original: `undefined` depois do último.
    fn get(&self, index: usize) -> Option<FoldRange> {
        if index >= self.length() {
            return None;
        }
        match self {
            Self::Regions(regions) => Some(regions.to_fold_range(index)),
            Self::Ranges(ranges) => Some(ranges[index].clone()),
        }
    }
}

impl FoldingRegions {
    /// `new FoldingRegions(startIndexes, endIndexes, types)`. Entra em pânico se os tamanhos diferem ou
    /// passam de [`MAX_FOLDING_REGIONS`] (o `Error` do original).
    pub fn new(
        start_indexes: Vec<u32>,
        end_indexes: Vec<u32>,
        types: Option<Vec<Option<String>>>,
    ) -> Self {
        if start_indexes.len() != end_indexes.len() || start_indexes.len() > MAX_FOLDING_REGIONS {
            panic!("invalid startIndexes or endIndexes size");
        }
        let size = start_indexes.len();
        Self {
            start_indexes: RefCell::new(start_indexes),
            end_indexes: RefCell::new(end_indexes),
            collapse_states: BitField::new(size),
            user_defined_states: BitField::new(size),
            recovered_states: BitField::new(size),
            types,
            parents_computed: Cell::new(false),
        }
    }

    fn ensure_parent_indices(&self) {
        if self.parents_computed.get() {
            return;
        }
        self.parents_computed.set(true);
        let mut parent_indexes: Vec<usize> = Vec::new();
        let len = self.start_indexes.borrow().len();
        for i in 0..len {
            let start_line_number = self.start_indexes.borrow()[i];
            let end_line_number = self.end_indexes.borrow()[i];
            if start_line_number > MAX_LINE_NUMBER || end_line_number > MAX_LINE_NUMBER {
                panic!(
                    "startLineNumber or endLineNumber must not exceed {}",
                    MAX_LINE_NUMBER
                );
            }
            // isInsideLast
            while let Some(&index) = parent_indexes.last()
                && !(self.get_start_line_number(index) <= start_line_number
                    && self.get_end_line_number(index) >= end_line_number)
            {
                parent_indexes.pop();
            }
            // -1 quando não há pai: `(-1 & 0xFF) << 24` do original é 0xFF000000 em 32 bits.
            let parent_index: u32 = parent_indexes.last().map_or(u32::MAX, |&p| p as u32);
            parent_indexes.push(i);
            self.start_indexes.borrow_mut()[i] =
                start_line_number.wrapping_add((parent_index & 0xFF) << 24);
            self.end_indexes.borrow_mut()[i] =
                end_line_number.wrapping_add((parent_index & 0xFF00) << 16);
        }
    }

    pub fn length(&self) -> usize {
        self.start_indexes.borrow().len()
    }

    pub fn get_start_line_number(&self, index: usize) -> u32 {
        self.start_indexes.borrow().get(index).copied().unwrap_or(0) & MAX_LINE_NUMBER
    }

    pub fn get_end_line_number(&self, index: usize) -> u32 {
        self.end_indexes.borrow().get(index).copied().unwrap_or(0) & MAX_LINE_NUMBER
    }

    pub fn get_type(&self, index: usize) -> Option<&str> {
        self.types
            .as_ref()
            .and_then(|types| types.get(index))
            .and_then(|t| t.as_deref())
    }

    pub fn has_types(&self) -> bool {
        self.types.is_some()
    }

    pub fn is_collapsed(&self, index: usize) -> bool {
        self.collapse_states.get(index)
    }

    pub fn set_collapsed(&self, index: usize, new_state: bool) {
        self.collapse_states.set(index, new_state);
    }

    fn is_user_defined(&self, index: usize) -> bool {
        self.user_defined_states.get(index)
    }

    fn set_user_defined(&self, index: usize, new_state: bool) {
        self.user_defined_states.set(index, new_state);
    }

    fn is_recovered(&self, index: usize) -> bool {
        self.recovered_states.get(index)
    }

    fn set_recovered(&self, index: usize, new_state: bool) {
        self.recovered_states.set(index, new_state);
    }

    pub fn get_source(&self, index: usize) -> FoldSource {
        if self.is_user_defined(index) {
            FoldSource::UserDefined
        } else if self.is_recovered(index) {
            FoldSource::Recovered
        } else {
            FoldSource::Provider
        }
    }

    pub fn set_source(&self, index: usize, source: FoldSource) {
        match source {
            FoldSource::UserDefined => {
                self.set_user_defined(index, true);
                self.set_recovered(index, false);
            }
            FoldSource::Recovered => {
                self.set_user_defined(index, false);
                self.set_recovered(index, true);
            }
            FoldSource::Provider => {
                self.set_user_defined(index, false);
                self.set_recovered(index, false);
            }
        }
    }

    pub fn set_collapsed_all_of_type(&self, r#type: &str, new_state: bool) -> bool {
        let mut has_changed = false;
        if let Some(types) = &self.types {
            for (i, t) in types.iter().enumerate() {
                if t.as_deref() == Some(r#type) {
                    self.set_collapsed(i, new_state);
                    has_changed = true;
                }
            }
        }
        has_changed
    }

    pub fn to_region(&self, index: usize) -> FoldingRegion<'_> {
        FoldingRegion::new(self, index)
    }

    /// O índice do pai, ou `-1` se a região não tem pai.
    pub fn get_parent_index(&self, index: usize) -> i32 {
        self.ensure_parent_indices();
        let start = self.start_indexes.borrow().get(index).copied().unwrap_or(0);
        let end = self.end_indexes.borrow().get(index).copied().unwrap_or(0);
        let parent = ((start & MASK_INDENT) >> 24) + ((end & MASK_INDENT) >> 16);
        if parent as usize == MAX_FOLDING_REGIONS {
            return -1;
        }
        parent as i32
    }

    pub fn contains(&self, index: usize, line: u32) -> bool {
        self.get_start_line_number(index) <= line && self.get_end_line_number(index) >= line
    }

    fn find_index(&self, line: u32) -> i64 {
        let mut low = 0usize;
        let mut high = self.start_indexes.borrow().len();
        if high == 0 {
            return -1; // no children
        }
        while low < high {
            let mid = (low + high) / 2;
            if line < self.get_start_line_number(mid) {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        low as i64 - 1
    }

    /// O índice da região mais interna que contém `line`, ou `-1`.
    pub fn find_range(&self, line: u32) -> i32 {
        let index = self.find_index(line);
        if index >= 0 {
            let mut index = index as usize;
            let end_line_number = self.get_end_line_number(index);
            if end_line_number >= line {
                return index as i32;
            }
            let mut parent = self.get_parent_index(index);
            while parent != -1 {
                index = parent as usize;
                if self.contains(index, line) {
                    return index as i32;
                }
                parent = self.get_parent_index(index);
            }
        }
        -1
    }

    pub fn to_fold_range(&self, index: usize) -> FoldRange {
        FoldRange {
            start_line_number: self.get_start_line_number(index),
            end_line_number: self.get_end_line_number(index),
            r#type: self.types.as_ref().and_then(|types| types.get(index)).cloned().flatten(),
            is_collapsed: self.is_collapsed(index),
            source: self.get_source(index),
        }
    }

    pub fn from_fold_ranges(ranges: &[FoldRange]) -> FoldingRegions {
        let ranges_length = ranges.len();
        let mut start_indexes = Vec::with_capacity(ranges_length);
        let mut end_indexes = Vec::with_capacity(ranges_length);
        let mut types: Vec<Option<String>> = Vec::with_capacity(ranges_length);
        let mut got_types = false;
        for range in ranges {
            start_indexes.push(range.start_line_number);
            end_indexes.push(range.end_line_number);
            types.push(range.r#type.clone());
            // `if (range.type)`: a string vazia é falsa.
            if range.r#type.as_deref().is_some_and(|t| !t.is_empty()) {
                got_types = true;
            }
        }
        let regions = FoldingRegions::new(start_indexes, end_indexes, got_types.then_some(types));
        for (i, range) in ranges.iter().enumerate() {
            if range.is_collapsed {
                regions.set_collapsed(i, true);
            }
            regions.set_source(i, range.source);
        }
        regions
    }

    /// Duas entradas, cada uma um `FoldingRegions` ou uma lista de `FoldRange`, são mescladas.
    /// Cada entrada deve estar ordenada por `startLineNumber`.
    /// A primeira lista deve incluir todas as regiões atualmente definidas pelos provedores.
    /// A segunda só contém as regiões antes recolhidas e todas as manuais.
    /// Se a linha inicial coincide, vale o intervalo novo, e ele deixa de ser manual.
    /// Quando uma entrada de uma lista se sobrepõe a uma da outra, a da segunda lista "vence" e as
    /// entradas sobrepostas da primeira são descartadas.
    /// Entradas inválidas são descartadas. Uma entrada é inválida se:
    /// - as linhas inicial e final não são um intervalo válido de linhas,
    /// - está fora de ordem ou tem a mesma linha inicial de uma entrada anterior,
    /// - se sobrepõe a uma entrada anterior e não está inteira dentro dela.
    pub fn sanitize_and_merge(
        ranges_a: FoldingRangesInput<'_>,
        ranges_b: FoldingRangesInput<'_>,
        max_line_number: Option<u32>,
        selection: Option<&dyn SelectedLines>,
    ) -> Vec<FoldRange> {
        let max_line_number = max_line_number.unwrap_or(u32::MAX);

        let mut index_a = 0usize;
        let mut index_b = 0usize;
        let mut next_a = ranges_a.get(0);
        let mut next_b = ranges_b.get(0);

        // Só o `endLineNumber` do topo e da pilha é lido, então a pilha guarda ele.
        let mut stacked_ranges: Vec<u32> = Vec::new();
        let mut top_stacked_range: Option<u32> = None;
        let mut prev_line_number = 0u32;
        let mut result_ranges: Vec<FoldRange> = Vec::new();

        while next_a.is_some() || next_b.is_some() {
            let mut use_range: Option<FoldRange> = None;
            let b_is_next = match (&next_a, &next_b) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(a), Some(b)) => a.start_line_number >= b.start_line_number,
            };
            if b_is_next {
                let mut b = next_b.take().expect("nextB existe");
                if let Some(mut a) =
                    next_a.take_if(|a| a.start_line_number == b.start_line_number)
                {
                    if b.source == FoldSource::UserDefined {
                        // a user defined range (possibly unfolded)
                        use_range = Some(b);
                    } else {
                        // a previously folded range or a (possibly unfolded) recovered range
                        // stays collapsed if the range still has the same number of lines or the
                        // selection is not in the range or after it
                        a.is_collapsed = b.is_collapsed
                            && (a.end_line_number == b.end_line_number
                                || selection.is_none_or(|s| {
                                    !s.starts_inside(a.start_line_number + 1, a.end_line_number + 1)
                                }));
                        a.source = FoldSource::Provider;
                        use_range = Some(a);
                    }
                    index_a += 1;
                    next_a = ranges_a.get(index_a); // not necessary, just for speed
                } else {
                    if b.is_collapsed && b.source == FoldSource::Provider {
                        // a previously collapsed range
                        b.source = FoldSource::Recovered;
                    }
                    use_range = Some(b);
                }
                index_b += 1;
                next_b = ranges_b.get(index_b);
            } else {
                // nextA is next. The user folded B set takes precedence and we sometimes need to look
                // ahead in it to check for an upcoming conflict.
                let a = next_a.take().expect("nextA existe");
                let a_end = a.end_line_number;
                let mut conflict = false;
                let mut scan_index = index_b;
                let mut prescan_b = next_b.clone();
                while let Some(p) = &prescan_b {
                    if p.start_line_number > a_end {
                        break; // no conflict, use this nextA
                    }
                    if p.source == FoldSource::UserDefined && p.end_line_number > a_end {
                        // we found a user folded range, it wins
                        conflict = true; // this nextA gets skipped
                        break;
                    }
                    scan_index += 1;
                    prescan_b = ranges_b.get(scan_index);
                }
                if !conflict {
                    use_range = Some(a);
                }
                index_a += 1;
                next_a = ranges_a.get(index_a);
            }

            if let Some(use_range) = use_range {
                while let Some(top) = top_stacked_range
                    && top < use_range.start_line_number
                {
                    top_stacked_range = stacked_ranges.pop();
                }
                if use_range.end_line_number > use_range.start_line_number
                    && use_range.start_line_number > prev_line_number
                    && use_range.end_line_number <= max_line_number
                    && top_stacked_range.is_none_or(|top| top >= use_range.end_line_number)
                {
                    prev_line_number = use_range.start_line_number;
                    if let Some(top) = top_stacked_range {
                        stacked_ranges.push(top);
                    }
                    top_stacked_range = Some(use_range.end_line_number);
                    result_ranges.push(use_range);
                }
            }
        }
        result_ranges
    }
}

impl std::fmt::Display for FoldingRegions {
    /// `toString()`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let res: Vec<String> = (0..self.length())
            .map(|i| {
                format!(
                    "[{}{}] {}/{}",
                    fold_source_abbr(self.get_source(i)),
                    if self.is_collapsed(i) { '+' } else { '-' },
                    self.get_start_line_number(i),
                    self.get_end_line_number(i)
                )
            })
            .collect();
        f.write_str(&res.join(", "))
    }
}

/// `FoldingRegion`: uma região de `FoldingRegions`, por índice.
#[derive(Clone, Copy)]
pub struct FoldingRegion<'a> {
    ranges: &'a FoldingRegions,
    index: usize,
}

impl<'a> FoldingRegion<'a> {
    pub fn new(ranges: &'a FoldingRegions, index: usize) -> Self {
        Self { ranges, index }
    }

    pub fn start_line_number(&self) -> u32 {
        self.ranges.get_start_line_number(self.index)
    }

    pub fn end_line_number(&self) -> u32 {
        self.ranges.get_end_line_number(self.index)
    }

    pub fn region_index(&self) -> usize {
        self.index
    }

    pub fn parent_index(&self) -> i32 {
        self.ranges.get_parent_index(self.index)
    }

    pub fn is_collapsed(&self) -> bool {
        self.ranges.is_collapsed(self.index)
    }

    pub fn contained_by(&self, range: ILineRange) -> bool {
        range.start_line_number <= self.start_line_number()
            && range.end_line_number >= self.end_line_number()
    }

    pub fn contains_line(&self, line_number: u32) -> bool {
        self.start_line_number() <= line_number && line_number <= self.end_line_number()
    }

    pub fn hides_line(&self, line_number: u32) -> bool {
        self.start_line_number() < line_number && line_number <= self.end_line_number()
    }
}
