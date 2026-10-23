//! Porte de `src/vs/base/common/range.ts`: intervalos `[start, end)` de uma dimensão.

/// `IRange`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IRange {
    pub start: f64,
    pub end: f64,
}

/// `IRangedGroup`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IRangedGroup {
    pub range: IRange,
    pub size: f64,
}

/// `Range.intersect`: a interseção como um intervalo; `{ start: 0, end: 0 }` se for vazia.
pub fn intersect(one: IRange, other: IRange) -> IRange {
    if one.start >= other.end || other.start >= one.end {
        return IRange { start: 0.0, end: 0.0 };
    }

    let start = one.start.max(other.start);
    let end = one.end.min(other.end);

    if end - start <= 0.0 {
        return IRange { start: 0.0, end: 0.0 };
    }

    IRange { start, end }
}

/// `Range.isEmpty`.
pub fn is_empty(range: IRange) -> bool {
    range.end - range.start <= 0.0
}

/// `Range.intersects`.
pub fn intersects(one: IRange, other: IRange) -> bool {
    !is_empty(intersect(one, other))
}

/// `Range.relativeComplement`.
pub fn relative_complement(one: IRange, other: IRange) -> Vec<IRange> {
    let mut result = Vec::new();
    let first = IRange { start: one.start, end: other.start.min(one.end) };
    let second = IRange { start: other.end.max(one.start), end: one.end };

    if !is_empty(first) {
        result.push(first);
    }

    if !is_empty(second) {
        result.push(second);
    }

    result
}
