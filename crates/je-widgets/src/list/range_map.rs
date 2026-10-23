//! Porte de `src/vs/base/browser/ui/list/rangeMap.ts`: o mapa de intervalos que dá a posição de cada item
//! de uma lista de itens de tamanhos variados, em tempo proporcional ao número de grupos de tamanho
//! igual (uma lista de itens de mesma altura é um grupo só, qualquer que seja o tamanho).
//!
//! Os números são `f64` (o `number` do JavaScript); índices que podem ser `-1` (`index_at`, `position_at`)
//! são `i64`, e contagens e posições de inserção são `usize`.

use je_base::range::{IRange, IRangedGroup, intersect, is_empty};

/// `IItem`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IItem {
    pub size: f64,
}

/// `groupIntersect`: a interseção entre grupos e um intervalo; vazio se não há.
pub fn group_intersect(range: IRange, groups: &[IRangedGroup]) -> Vec<IRangedGroup> {
    let mut result = Vec::new();

    for r in groups {
        if range.start >= r.range.end {
            continue;
        }

        if range.end < r.range.start {
            break;
        }

        let intersection = intersect(range, r.range);

        if is_empty(intersection) {
            continue;
        }

        result.push(IRangedGroup { range: intersection, size: r.size });
    }

    result
}

/// `shift`: desloca um intervalo em `much`.
pub fn shift(IRange { start, end }: IRange, much: f64) -> IRange {
    IRange { start: start + much, end: end + much }
}

/// `consolidate`: junta os grupos consecutivos de mesmo `size`.
pub fn consolidate(groups: &[IRangedGroup]) -> Vec<IRangedGroup> {
    let mut result: Vec<IRangedGroup> = Vec::new();

    for group in groups {
        if let Some(previous) = result.last_mut()
            && group.size == previous.size
        {
            previous.range.end = group.range.end;
            continue;
        }
        result.push(IRangedGroup { range: IRange { start: group.range.start, end: group.range.end }, size: group.size });
    }

    result
}

/// `concat`: concatena coleções de grupos numa só, consolidada.
fn concat(groups: &[&[IRangedGroup]]) -> Vec<IRangedGroup> {
    let all: Vec<IRangedGroup> = groups.iter().flat_map(|g| g.iter().copied()).collect();
    consolidate(&all)
}

/// `IRangeMap`.
pub trait IRangeMap {
    fn size(&self) -> f64;
    fn count(&self) -> usize;
    fn padding_top(&self) -> f64;
    fn set_padding_top(&mut self, padding_top: f64);
    fn splice(&mut self, index: usize, delete_count: usize, items: &[IItem]);
    fn index_at(&self, position: f64) -> i64;
    fn index_after(&self, position: f64) -> i64;
    fn position_at(&self, index: i64) -> f64;
}

#[derive(Clone, Debug, Default)]
pub struct RangeMap {
    groups: Vec<IRangedGroup>,
    size: f64,
    padding_top: f64,
}

impl RangeMap {
    /// `new RangeMap(topPadding?)`.
    pub fn new(top_padding: Option<f64>) -> Self {
        let padding_top = top_padding.unwrap_or(0.0);
        Self { groups: Vec::new(), size: padding_top, padding_top }
    }

    /// Os grupos consolidados (a lista de itens de mesma altura é um só).
    pub fn groups(&self) -> &[IRangedGroup] {
        &self.groups
    }
}

impl IRangeMap for RangeMap {
    fn padding_top(&self) -> f64 {
        self.padding_top
    }

    fn set_padding_top(&mut self, padding_top: f64) {
        self.size = self.size + padding_top - self.padding_top;
        self.padding_top = padding_top;
    }

    fn splice(&mut self, index: usize, delete_count: usize, items: &[IItem]) {
        let diff = items.len() as f64 - delete_count as f64;
        let before = group_intersect(IRange { start: 0.0, end: index as f64 }, &self.groups);
        let after: Vec<IRangedGroup> = group_intersect(IRange { start: (index + delete_count) as f64, end: f64::INFINITY }, &self.groups)
            .into_iter()
            .map(|g| IRangedGroup { range: shift(g.range, diff), size: g.size })
            .collect();

        // O `middle` do original é um grupo por item e o `concat` o consolida; aqui o item que repete o
        // tamanho do anterior já nasce fundido (mesmo resultado, sem alocar um grupo por item).
        let mut middle: Vec<IRangedGroup> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let at = (index + i) as f64;
            match middle.last_mut() {
                Some(last) if last.size == item.size => last.range.end = at + 1.0,
                _ => middle.push(IRangedGroup { range: IRange { start: at, end: at + 1.0 }, size: item.size }),
            }
        }

        self.groups = concat(&[&before, &middle, &after]);
        self.size = self.padding_top + self.groups.iter().map(|g| g.size * (g.range.end - g.range.start)).sum::<f64>();
    }

    /// O número de itens do mapa.
    fn count(&self) -> usize {
        self.groups.last().map_or(0, |g| g.range.end as usize)
    }

    /// A soma dos tamanhos de todos os itens (mais o `padding_top`).
    fn size(&self) -> f64 {
        self.size
    }

    /// O índice do item na posição dada.
    fn index_at(&self, position: f64) -> i64 {
        if position < 0.0 {
            return -1;
        }

        if position < self.padding_top {
            return 0;
        }

        let mut index: i64 = 0;
        let mut size = self.padding_top;

        for group in &self.groups {
            let count = group.range.end - group.range.start;
            let new_size = size + count * group.size;

            if position < new_size {
                return index + ((position - size) / group.size).floor() as i64;
            }

            index += count as i64;
            size = new_size;
        }

        index
    }

    /// O índice do item logo depois do item na posição dada.
    fn index_after(&self, position: f64) -> i64 {
        (self.index_at(position) + 1).min(self.count() as i64)
    }

    /// A posição inicial do item de índice dado, ou `-1` se não existe.
    fn position_at(&self, index: i64) -> f64 {
        if index < 0 {
            return -1.0;
        }

        let mut position = 0.0;
        let mut count: i64 = 0;

        for group in &self.groups {
            let group_count = (group.range.end - group.range.start) as i64;
            let new_count = count + group_count;

            if index < new_count {
                return self.padding_top + position + (index - count) as f64 * group.size;
            }

            position += group_count as f64 * group.size;
            count = new_count;
        }

        -1.0
    }
}
