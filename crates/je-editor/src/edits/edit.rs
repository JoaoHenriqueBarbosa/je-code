//! Porte de `src/vs/editor/common/core/edits/edit.ts`: `BaseEdit`, `BaseReplacement`, `Edit` e
//! `AnnotationReplacement`.
//!
//! As classes abstratas viram traits. `BaseEdit<T, TEdit>` vira [`BaseEdit`] com o tipo associado
//! [`BaseEdit::Replacement`] (o `T`) e `Self` no lugar de `TEdit`; `BaseReplacement<TSelf>` vira
//! [`BaseReplacement`] com `Self` no lugar de `TSelf`. O construtor do `BaseEdit`, que confere que as
//! substituições são disjuntas e ordenadas, é [`validate_replacements`] e cada edição concreta o chama no
//! seu `new`. `AnyEdit` e `AnyReplacement` são só apelidos de tipo recursivos do TypeScript e não têm
//! equivalente em Rust (não há tipo para nomear).

use std::collections::VecDeque;
use std::fmt;

use je_base::errors::{self, Error};

use crate::offset_range::OffsetRange;

/// `{ e1, e2 }`: o par que `decomposeSplit` e `trySwap` devolvem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditPair<E> {
    pub e1: E,
    pub e2: E,
}

/// O construtor de `BaseEdit`: as substituições precisam ser disjuntas e ordenadas.
pub fn validate_replacements<T: BaseReplacement>(replacements: &[T]) {
    let mut last_end_ex = -1;
    for replacement in replacements {
        if replacement.replace_range().start < last_end_ex {
            errors::throw(Error::bug_indicating(Some(&format!(
                "Edits must be disjoint and sorted. Found {} after {}",
                replacement.describe(),
                last_end_ex
            ))));
        }
        last_end_ex = replacement.replace_range().end_exclusive;
    }
}

/// `BaseEdit<T, TEdit>`: um conjunto de substituições aplicadas todas de uma vez.
pub trait BaseEdit: Sized + Clone {
    type Replacement: BaseReplacement;

    /// `replacements`.
    fn replacements(&self) -> &[Self::Replacement];

    /// `_createNew`.
    fn create_new(replacements: Vec<Self::Replacement>) -> Self;

    /// Verdadeiro se e só se esta edição e a dada são estruturalmente iguais. Isso não quer dizer que
    /// tenham o mesmo efeito numa entrada! Para isso veja `normalize()`.
    fn equals(&self, other: &Self) -> bool {
        if self.replacements().len() != other.replacements().len() {
            return false;
        }
        self.replacements().iter().zip(other.replacements()).all(|(a, b)| a.equals(b))
    }

    /// `toString`.
    fn describe(&self) -> String {
        let edits: Vec<String> = self.replacements().iter().map(BaseReplacement::describe).collect();
        format!("[{}]", edits.join(", "))
    }

    /// Normaliza a edição removendo as substituições vazias e juntando as que se tocam (se elas deixam).
    /// Duas edições têm a mesma edição normalizada se e só se têm o mesmo efeito em qualquer entrada.
    ///
    /// Invariante:
    /// ```text
    /// (forall base: TEdit.apply(base).equals(other.apply(base))) <-> this.normalize().equals(other.normalize())
    /// ```
    /// e
    /// ```text
    /// forall base: TEdit.apply(base).equals(this.normalize().apply(base))
    /// ```
    fn normalize(&self) -> Self {
        let mut new_replacements: Vec<Self::Replacement> = Vec::new();
        let mut last_replacement: Option<Self::Replacement> = None;
        for r in self.replacements() {
            if r.get_new_length() == 0 && r.replace_range().length() == 0 {
                continue;
            }
            if let Some(last) = &last_replacement
                && last.replace_range().end_exclusive == r.replace_range().start
                && let Some(joined) = last.try_join_touching(r)
            {
                last_replacement = Some(joined);
                continue;
            }

            if let Some(last) = last_replacement.take() {
                new_replacements.push(last);
            }
            last_replacement = Some(r.clone());
        }

        if let Some(last) = last_replacement {
            new_replacements.push(last);
        }
        Self::create_new(new_replacements)
    }

    /// Combina duas edições numa só com o mesmo efeito.
    ///
    /// Invariante:
    /// ```text
    /// other.apply(this.apply(s0)) = this.compose(other).apply(s0)
    /// ```
    fn compose(&self, other: &Self) -> Self {
        let edits1 = self.normalize();
        let edits2 = other.normalize();

        if edits1.is_empty() {
            return edits2;
        }
        if edits2.is_empty() {
            return edits1;
        }

        let mut edit1_queue: VecDeque<Self::Replacement> = edits1.replacements().iter().cloned().collect();
        let mut result: Vec<Self::Replacement> = Vec::new();

        let mut edit1_to_edit2 = 0;

        for r2 in edits2.replacements() {
            // Copia a edição 1 sem mudar até ela tocar a edição 2.
            loop {
                let Some(r1) = edit1_queue.front() else {
                    break;
                };
                if r1.replace_range().start + edit1_to_edit2 + r1.get_new_length() >= r2.replace_range().start {
                    break;
                }
                let r1 = edit1_queue.pop_front().expect("front() devolveu um elemento");

                edit1_to_edit2 += r1.get_new_length() - r1.replace_range().length();
                result.push(r1);
            }

            let first_edit1_to_edit2 = edit1_to_edit2;
            let mut first_intersecting: Option<Self::Replacement> = None; // ou tocando
            let mut last_intersecting: Option<Self::Replacement> = None; // ou tocando

            loop {
                let Some(r1) = edit1_queue.front() else {
                    break;
                };
                if r1.replace_range().start + edit1_to_edit2 > r2.replace_range().end_exclusive {
                    break;
                }
                // Senão se cruzam, porque o novo fim da edição 1 é depois ou igual ao nosso início.

                let r1 = edit1_queue.pop_front().expect("front() devolveu um elemento");
                if first_intersecting.is_none() {
                    first_intersecting = Some(r1.clone());
                }
                edit1_to_edit2 += r1.get_new_length() - r1.replace_range().length();
                last_intersecting = Some(r1);
            }

            let Some(first_intersecting) = first_intersecting else {
                result.push(r2.delta(-edit1_to_edit2));
                continue;
            };

            let new_replace_range_start =
                first_intersecting.replace_range().start.min(r2.replace_range().start - first_edit1_to_edit2);

            let prefix_length = r2.replace_range().start - (first_intersecting.replace_range().start + first_edit1_to_edit2);
            if prefix_length > 0 {
                let prefix = first_intersecting.slice(
                    OffsetRange::empty_at(new_replace_range_start),
                    Some(OffsetRange::new(0, prefix_length)),
                );
                result.push(prefix);
            }
            let Some(last_intersecting) = last_intersecting else {
                errors::throw(Error::bug_indicating(Some("Invariant violation: lastIntersecting is undefined")));
            };
            let suffix_length =
                (last_intersecting.replace_range().end_exclusive + edit1_to_edit2) - r2.replace_range().end_exclusive;
            if suffix_length > 0 {
                let e = last_intersecting.slice(
                    OffsetRange::of_start_and_length(last_intersecting.replace_range().end_exclusive, 0),
                    Some(OffsetRange::new(
                        last_intersecting.get_new_length() - suffix_length,
                        last_intersecting.get_new_length(),
                    )),
                );
                edit1_to_edit2 -= e.get_new_length() - e.replace_range().length();
                edit1_queue.push_front(e);
            }

            let new_replace_range =
                OffsetRange::new(new_replace_range_start, r2.replace_range().end_exclusive - edit1_to_edit2);
            let middle = r2.slice(new_replace_range, Some(OffsetRange::new(0, r2.get_new_length())));
            result.push(middle);
        }

        result.extend(edit1_queue);

        Self::create_new(result).normalize()
    }

    /// `decomposeSplit`.
    fn decompose_split(&self, should_be_in_e1: impl Fn(&Self::Replacement) -> bool) -> EditPair<Self> {
        let mut e1: Vec<Self::Replacement> = Vec::new();
        let mut e2: Vec<Self::Replacement> = Vec::new();

        let mut e2_delta = 0;
        for edit in self.replacements() {
            if should_be_in_e1(edit) {
                e1.push(edit.clone());
                e2_delta += edit.get_new_length() - edit.replace_range().length();
            } else {
                e2.push(edit.slice(edit.replace_range().delta(e2_delta), Some(OffsetRange::new(0, edit.get_new_length()))));
            }
        }
        EditPair { e1: Self::create_new(e1), e2: Self::create_new(e2) }
    }

    /// O intervalo de cada substituição no valor já aplicado.
    fn get_new_ranges(&self) -> Vec<OffsetRange> {
        let mut ranges = Vec::new();
        let mut offset = 0;
        for e in self.replacements() {
            ranges.push(OffsetRange::of_start_and_length(e.replace_range().start + offset, e.get_new_length()));
            offset += e.get_length_delta();
        }
        ranges
    }

    fn get_joined_replace_range(&self) -> Option<OffsetRange> {
        let first = self.replacements().first()?;
        let last = self.replacements().last()?;
        Some(first.replace_range().join(last.replace_range()))
    }

    fn is_empty(&self) -> bool {
        self.replacements().is_empty()
    }

    fn get_length_delta(&self) -> i32 {
        self.replacements().iter().map(BaseReplacement::get_length_delta).sum()
    }

    fn get_new_data_length(&self, data_length: i32) -> i32 {
        data_length + self.get_length_delta()
    }

    fn apply_to_offset(&self, original_offset: i32) -> i32 {
        let mut accumulated_delta = 0;
        for r in self.replacements() {
            if r.replace_range().start <= original_offset {
                if original_offset < r.replace_range().end_exclusive {
                    // o deslocamento está no intervalo substituído
                    return r.replace_range().start + accumulated_delta;
                }
                accumulated_delta += r.get_new_length() - r.replace_range().length();
            } else {
                break;
            }
        }
        original_offset + accumulated_delta
    }

    fn apply_to_offset_range(&self, original_range: OffsetRange) -> OffsetRange {
        OffsetRange::new(self.apply_to_offset(original_range.start), self.apply_to_offset(original_range.end_exclusive))
    }

    fn apply_inverse_to_offset(&self, post_edits_offset: i32) -> i32 {
        let mut accumulated_delta = 0;
        for edit in self.replacements() {
            let edit_length = edit.get_new_length();
            if edit.replace_range().start <= post_edits_offset - accumulated_delta {
                if post_edits_offset - accumulated_delta < edit.replace_range().start + edit_length {
                    // o deslocamento está no intervalo substituído
                    return edit.replace_range().start;
                }
                accumulated_delta += edit_length - edit.replace_range().length();
            } else {
                break;
            }
        }
        post_edits_offset - accumulated_delta
    }

    /// `None` se o `original_offset` está dentro de uma edição.
    fn apply_to_offset_or_undefined(&self, original_offset: i32) -> Option<i32> {
        let mut accumulated_delta = 0;
        for edit in self.replacements() {
            if edit.replace_range().start <= original_offset {
                if original_offset < edit.replace_range().end_exclusive {
                    // o deslocamento está no intervalo substituído
                    return None;
                }
                accumulated_delta += edit.get_new_length() - edit.replace_range().length();
            } else {
                break;
            }
        }
        Some(original_offset + accumulated_delta)
    }

    /// `None` se o `original_range` está dentro de uma edição.
    fn apply_to_offset_range_or_undefined(&self, original_range: OffsetRange) -> Option<OffsetRange> {
        let start = self.apply_to_offset_or_undefined(original_range.start)?;
        let end = self.apply_to_offset_or_undefined(original_range.end_exclusive)?;
        Some(OffsetRange::new(start, end))
    }
}

/// `BaseReplacement<TSelf>`: a troca de um intervalo por um novo conteúdo.
pub trait BaseReplacement: Clone + fmt::Debug {
    /// `replaceRange`: o intervalo a ser substituído.
    fn replace_range(&self) -> OffsetRange;

    fn get_new_length(&self) -> i32;

    /// Pré-condição: `this.range.endExclusive === other.range.start`.
    fn try_join_touching(&self, other: &Self) -> Option<Self>;

    fn slice(&self, new_replace_range: OffsetRange, range_in_replacement: Option<OffsetRange>) -> Self;

    fn equals(&self, other: &Self) -> bool;

    fn delta(&self, offset: i32) -> Self {
        self.slice(self.replace_range().delta(offset), Some(OffsetRange::new(0, self.get_new_length())))
    }

    fn get_length_delta(&self) -> i32 {
        self.get_new_length() - self.replace_range().length()
    }

    /// `toString`.
    fn describe(&self) -> String {
        format!("{{ {} -> {} }}", self.replace_range(), self.get_new_length())
    }

    /// O getter `isEmpty`.
    fn is_empty(&self) -> bool {
        self.get_new_length() == 0 && self.replace_range().length() == 0
    }

    fn get_range_after_replace(&self) -> OffsetRange {
        OffsetRange::new(self.replace_range().start, self.replace_range().start + self.get_new_length())
    }
}

/// `Edit<T>`: um conjunto de edições de uma string. Todas são aplicadas de uma vez.
#[derive(Clone, Debug)]
pub struct Edit<T: BaseReplacement> {
    replacements: Vec<T>,
}

impl<T: BaseReplacement> Edit<T> {
    pub fn new(replacements: Vec<T>) -> Self {
        validate_replacements(&replacements);
        Self { replacements }
    }

    /// `Edit.empty`.
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn create(replacements: Vec<T>) -> Self {
        Self::new(replacements)
    }

    pub fn single(replacement: T) -> Self {
        Self::new(vec![replacement])
    }
}

impl<T: BaseReplacement> BaseEdit for Edit<T> {
    type Replacement = T;

    fn replacements(&self) -> &[T] {
        &self.replacements
    }

    fn create_new(replacements: Vec<T>) -> Self {
        Self::new(replacements)
    }
}

/// `AnnotationReplacement<TAnnotation>`: só o intervalo, o novo comprimento e a anotação (comparada com
/// `PartialEq`, no lugar da identidade `===` do original).
#[derive(Clone, Debug)]
pub struct AnnotationReplacement<A: Clone + PartialEq + fmt::Debug> {
    pub replace_range: OffsetRange,
    pub new_length: i32,
    pub annotation: A,
}

impl<A: Clone + PartialEq + fmt::Debug> AnnotationReplacement<A> {
    pub fn new(range: OffsetRange, new_length: i32, annotation: A) -> Self {
        Self { replace_range: range, new_length, annotation }
    }
}

impl<A: Clone + PartialEq + fmt::Debug> BaseReplacement for AnnotationReplacement<A> {
    fn replace_range(&self) -> OffsetRange {
        self.replace_range
    }

    fn equals(&self, other: &Self) -> bool {
        self.replace_range.equals(other.replace_range)
            && self.new_length == other.new_length
            && self.annotation == other.annotation
    }

    fn get_new_length(&self) -> i32 {
        self.new_length
    }

    fn try_join_touching(&self, other: &Self) -> Option<Self> {
        if self.annotation != other.annotation {
            return None;
        }
        Some(Self::new(
            self.replace_range.join_right_touching(other.replace_range),
            self.new_length + other.new_length,
            self.annotation.clone(),
        ))
    }

    fn slice(&self, range: OffsetRange, range_in_replacement: Option<OffsetRange>) -> Self {
        Self::new(range, range_in_replacement.map_or(self.new_length, |r| r.length()), self.annotation.clone())
    }
}
