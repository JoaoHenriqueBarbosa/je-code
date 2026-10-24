//! Porte de `src/vs/workbench/contrib/notebook/common/notebookRange.ts`: o intervalo de células
//! (`ICellRange`, meio aberto `[start, end)`) e as funções que convertem, reduzem e comparam listas deles.
//!
//! Divergências:
//! - `start` e `end` são `number` no original; aqui são `i64` (índices inteiros de célula, sem NaN nem fração).
//! - `isICellRange(candidate: unknown)` recebe um [`Dynamic`], o valor JavaScript sem tipo do porte.
//! - `cellIndexesToRanges` e `reduceCellRanges` ordenam no lugar o array recebido e, no segundo caso, também
//!   mutam o `end` das instâncias de entrada que viram o acumulador. Aqui as funções recebem uma fatia só de
//!   leitura e trabalham numa cópia ordenada: o chamador não vê o efeito colateral, e o valor devolvido é o
//!   mesmo. `cellRangesEqual` herda a diferença (o original reatribui `a` e `b` com a versão reduzida, que
//!   ordenava os arrays do chamador).

use je_base::dynamic::Dynamic;

/// `[start, end]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ICellRange {
    /// zero based index
    pub start: i64,

    /// zero based index
    pub end: i64,
}

/// `isICellRange`: o candidato é um objeto com `start` e `end` numéricos.
pub fn is_icell_range(candidate: &Dynamic) -> bool {
    if candidate.is_nullish() || candidate.type_of() != "object" {
        return false;
    }
    matches!(candidate.get_str("start"), Dynamic::Number(_)) && matches!(candidate.get_str("end"), Dynamic::Number(_))
}

/// `cellIndexesToRanges`: agrupa os índices (em qualquer ordem) em intervalos contíguos, em ordem crescente.
pub fn cell_indexes_to_ranges(indexes: &[i64]) -> Vec<ICellRange> {
    let mut sorted = indexes.to_vec();
    sorted.sort();
    let mut remaining = sorted.into_iter();
    let Some(first) = remaining.next() else {
        return Vec::new();
    };

    // No original o acumulador cresce pela frente (`unshift`) e é invertido no fim; aqui cresce pelo fim e já sai
    // na ordem final. `ranges[0]` do original é o último elemento.
    let mut ranges: Vec<(i64, i64)> = vec![(first, first + 1)];
    for num in remaining {
        match ranges.last_mut() {
            Some(current) if num <= current.1 => current.1 = num + 1,
            _ => ranges.push((num, num + 1)),
        }
    }

    ranges.into_iter().map(|(start, end)| ICellRange { start, end }).collect()
}

/// `cellRangesToIndexes`: expande os intervalos em índices.
pub fn cell_ranges_to_indexes(ranges: &[ICellRange]) -> Vec<i64> {
    let mut indexes: Vec<i64> = Vec::new();
    for range in ranges {
        for i in range.start..range.end {
            indexes.push(i);
        }
    }

    indexes
}

/// `reduceCellRanges`: ordena e funde os intervalos que se tocam ou se sobrepõem.
pub fn reduce_cell_ranges(ranges: &[ICellRange]) -> Vec<ICellRange> {
    let mut sorted = ranges.to_vec();
    // `Array#sort` é estável, como o `sort_by_key` do Rust.
    sorted.sort_by_key(|range| range.start);
    let Some(first) = sorted.first().copied() else {
        return Vec::new();
    };

    // O `reduce` do original tem valor inicial, então visita todos os elementos, inclusive o primeiro.
    let mut reduced: Vec<ICellRange> = vec![first];
    for curr in sorted {
        match reduced.last_mut() {
            Some(last) if last.end >= curr.start => last.end = last.end.max(curr.end),
            _ => reduced.push(curr),
        }
    }

    if reduced.len() > 1 {
        // remove the (0, 0) range
        reduced.retain(|range| !(range.start == range.end && range.start == 0));
    }

    reduced
}

/// `cellRangesEqual`: igualdade depois de reduzir os dois lados.
pub fn cell_ranges_equal(a: &[ICellRange], b: &[ICellRange]) -> bool {
    reduce_cell_ranges(a) == reduce_cell_ranges(b)
}

/// todo@rebornix test and sort
///
/// `cellRangeContains`: `other` está inteiro dentro de `range`.
pub fn cell_range_contains(range: &ICellRange, other: &ICellRange) -> bool {
    other.start >= range.start && other.end <= range.end
}
