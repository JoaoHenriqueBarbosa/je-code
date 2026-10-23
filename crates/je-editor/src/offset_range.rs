//! Porte de `src/vs/editor/common/core/ranges/offsetRange.ts`: `OffsetRange` e `OffsetRangeSet`.
//!
//! `IOffsetRange` e a `OffsetRange.equals` estática não têm tipo próprio aqui: só existe o `OffsetRange`, e a
//! igualdade é a derivada (`PartialEq`) ou a `equals` de instância. Os métodos que recebem outro intervalo o
//! recebem por valor, porque `OffsetRange` é `Copy`.

use je_base::errors::{self, Error};
use je_base::js_string;

/// `OffsetRange`: intervalo semiaberto `[start, end_exclusive)` de inteiros.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OffsetRange {
    pub start: i32,
    pub end_exclusive: i32,
}

/// `Array.prototype.slice(start, end)`: índices negativos contam do fim e os dois são limitados ao tamanho.
/// `None` em `end` é o `undefined` do original (até o fim).
pub fn js_array_slice<T: Clone>(arr: &[T], start: i32, end: Option<i32>) -> Vec<T> {
    let len = arr.len() as i64;
    let relative = |index: i32| {
        let index = i64::from(index);
        if index < 0 { (len + index).max(0) } else { index.min(len) }
    };
    let from = relative(start);
    let to = end.map_or(len, relative);
    if from >= to {
        return Vec::new();
    }
    arr[from as usize..to as usize].to_vec()
}

impl OffsetRange {
    pub fn from_to(start: i32, end_exclusive: i32) -> Self {
        Self::new(start, end_exclusive)
    }

    /// `OffsetRange.addRange(range, sortedRanges)`: insere juntando os que tocam.
    pub fn add_range(range: OffsetRange, sorted_ranges: &mut Vec<OffsetRange>) {
        let mut i = 0;
        while i < sorted_ranges.len() && sorted_ranges[i].end_exclusive < range.start {
            i += 1;
        }
        let mut j = i;
        while j < sorted_ranges.len() && sorted_ranges[j].start <= range.end_exclusive {
            j += 1;
        }
        if i == j {
            sorted_ranges.insert(i, range);
        } else {
            let start = range.start.min(sorted_ranges[i].start);
            let end = range.end_exclusive.max(sorted_ranges[j - 1].end_exclusive);
            sorted_ranges.splice(i..j, [OffsetRange::new(start, end)]);
        }
    }

    pub fn try_create(start: i32, end_exclusive: i32) -> Option<OffsetRange> {
        if start > end_exclusive {
            return None;
        }
        Some(OffsetRange::new(start, end_exclusive))
    }

    pub fn of_length(length: i32) -> OffsetRange {
        OffsetRange::new(0, length)
    }

    pub fn of_start_and_length(start: i32, length: i32) -> OffsetRange {
        OffsetRange::new(start, start + length)
    }

    pub fn empty_at(offset: i32) -> OffsetRange {
        OffsetRange::new(offset, offset)
    }

    /// O construtor: `start` depois de `end_exclusive` é erro de lógica.
    pub fn new(start: i32, end_exclusive: i32) -> Self {
        if start > end_exclusive {
            errors::throw(Error::bug_indicating(Some(&format!("Invalid range: [{start}, {end_exclusive})"))));
        }
        Self { start, end_exclusive }
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end_exclusive
    }

    pub fn delta(&self, offset: i32) -> OffsetRange {
        OffsetRange::new(self.start + offset, self.end_exclusive + offset)
    }

    pub fn delta_start(&self, offset: i32) -> OffsetRange {
        OffsetRange::new(self.start + offset, self.end_exclusive)
    }

    pub fn delta_end(&self, offset: i32) -> OffsetRange {
        OffsetRange::new(self.start, self.end_exclusive + offset)
    }

    pub fn length(&self) -> i32 {
        self.end_exclusive - self.start
    }

    pub fn equals(&self, other: OffsetRange) -> bool {
        self.start == other.start && self.end_exclusive == other.end_exclusive
    }

    pub fn contains_range(&self, other: OffsetRange) -> bool {
        self.start <= other.start && other.end_exclusive <= self.end_exclusive
    }

    pub fn contains(&self, offset: i32) -> bool {
        self.start <= offset && offset < self.end_exclusive
    }

    /// `for all numbers n: range1.contains(n) or range2.contains(n) => range1.join(range2).contains(n)`.
    /// O intervalo unido é o menor que contém os dois.
    pub fn join(&self, other: OffsetRange) -> OffsetRange {
        OffsetRange::new(self.start.min(other.start), self.end_exclusive.max(other.end_exclusive))
    }

    /// O resultado é vazio se os intervalos só se tocam, e `None` se nem se tocam.
    pub fn intersect(&self, other: OffsetRange) -> Option<OffsetRange> {
        let start = self.start.max(other.start);
        let end = self.end_exclusive.min(other.end_exclusive);
        if start <= end {
            return Some(OffsetRange::new(start, end));
        }
        None
    }

    pub fn intersection_length(&self, range: &OffsetRange) -> i32 {
        let start = self.start.max(range.start);
        let end = self.end_exclusive.min(range.end_exclusive);
        (end - start).max(0)
    }

    /// `a.intersects(b)` se existe `n` com `a.contains(n)` e `b.contains(n)`. Se um dos intervalos é
    /// vazio, devolve sempre `false`.
    pub fn intersects(&self, other: OffsetRange) -> bool {
        let start = self.start.max(other.start);
        let end = self.end_exclusive.min(other.end_exclusive);
        start < end
    }

    pub fn intersects_or_touches(&self, other: OffsetRange) -> bool {
        let start = self.start.max(other.start);
        let end = self.end_exclusive.min(other.end_exclusive);
        start <= end
    }

    pub fn is_before(&self, other: OffsetRange) -> bool {
        self.end_exclusive <= other.start
    }

    pub fn is_after(&self, other: OffsetRange) -> bool {
        self.start >= other.end_exclusive
    }

    /// `arr.slice(this.start, this.endExclusive)`.
    pub fn slice<T: Clone>(&self, arr: &[T]) -> Vec<T> {
        js_array_slice(arr, self.start, Some(self.end_exclusive))
    }

    /// `str.substring(this.start, this.endExclusive)`.
    pub fn substring<'a>(&self, str: &'a [u16]) -> &'a [u16] {
        js_string::substring(str, i64::from(self.start), i64::from(self.end_exclusive))
    }

    /// O valor dado se está contido, senão o valor contido mais próximo. O intervalo não pode ser vazio.
    pub fn clip(&self, value: i32) -> i32 {
        if self.is_empty() {
            errors::throw(Error::bug_indicating(Some(&format!("Invalid clipping range: {self}"))));
        }
        self.start.max((self.end_exclusive - 1).min(value))
    }

    /// `r := value + k * length` tal que `r` está contido no intervalo. O intervalo não pode ser vazio.
    ///
    /// Por exemplo `[5, 10).clipCyclic(10) === 5`, `[5, 10).clipCyclic(11) === 6` e
    /// `[5, 10).clipCyclic(4) === 9`.
    pub fn clip_cyclic(&self, value: i32) -> i32 {
        if self.is_empty() {
            errors::throw(Error::bug_indicating(Some(&format!("Invalid clipping range: {self}"))));
        }
        if value < self.start {
            return self.end_exclusive - ((self.start - value) % self.length());
        }
        if value >= self.end_exclusive {
            return self.start + ((value - self.start) % self.length());
        }
        value
    }

    pub fn map<T>(&self, mut f: impl FnMut(i32) -> T) -> Vec<T> {
        let mut result = Vec::new();
        for i in self.start..self.end_exclusive {
            result.push(f(i));
        }
        result
    }

    pub fn for_each(&self, mut f: impl FnMut(i32)) {
        for i in self.start..self.end_exclusive {
            f(i);
        }
    }

    /// `this: [5, 10), range: [10, 15) => [5, 15)`. Lança se os intervalos não se tocam.
    pub fn join_right_touching(&self, range: OffsetRange) -> OffsetRange {
        if self.end_exclusive != range.start {
            errors::throw(Error::bug_indicating(Some(&format!("Invalid join: {self} and {range}"))));
        }
        OffsetRange::new(self.start, range.end_exclusive)
    }

    /// `withMargin(margin)`: a mesma margem nos dois lados.
    pub fn with_margin(&self, margin: i32) -> OffsetRange {
        self.with_margin_start_end(margin, margin)
    }

    /// `withMargin(marginStart, marginEnd)`.
    pub fn with_margin_start_end(&self, margin_start: i32, margin_end: i32) -> OffsetRange {
        OffsetRange::new(self.start - margin_start, self.end_exclusive + margin_end)
    }
}

impl std::fmt::Display for OffsetRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}, {})", self.start, self.end_exclusive)
    }
}

/// `OffsetRangeSet`: um conjunto de intervalos ordenados, sem sobreposição nem contato.
#[derive(Clone, Debug, Default)]
pub struct OffsetRangeSet {
    sorted_ranges: Vec<OffsetRange>,
}

impl OffsetRangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ranges(&self) -> Vec<OffsetRange> {
        self.sorted_ranges.clone()
    }

    pub fn add_range(&mut self, range: OffsetRange) {
        OffsetRange::add_range(range, &mut self.sorted_ranges);
    }

    /// Se há um valor contido neste conjunto e no intervalo dado.
    pub fn intersects_strict(&self, other: OffsetRange) -> bool {
        // TODO do original: usar busca binária.
        let mut i = 0;
        while i < self.sorted_ranges.len() && self.sorted_ranges[i].end_exclusive <= other.start {
            i += 1;
        }
        i < self.sorted_ranges.len() && self.sorted_ranges[i].start < other.end_exclusive
    }

    pub fn intersect_with_range(&self, other: OffsetRange) -> OffsetRangeSet {
        // TODO do original: usar busca binária mais fatia.
        let mut result = OffsetRangeSet::new();
        for range in &self.sorted_ranges {
            if let Some(intersection) = range.intersect(other) {
                result.add_range(intersection);
            }
        }
        result
    }

    pub fn intersect_with_range_length(&self, other: OffsetRange) -> i32 {
        self.intersect_with_range(other).length()
    }

    pub fn length(&self) -> i32 {
        self.sorted_ranges.iter().map(OffsetRange::length).sum()
    }
}

impl std::fmt::Display for OffsetRangeSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ranges: Vec<String> = self.sorted_ranges.iter().map(ToString::to_string).collect();
        f.write_str(&ranges.join(", "))
    }
}
