//! Porte de `src/vs/editor/common/core/edits/lengthEdit.ts`: `LengthEdit` e `LengthReplacement`, uma edição
//! que só guarda a informação de comprimento.
//!
//! Diferenças inevitáveis:
//! - `LengthEdit.compose(edits)` (estático) é [`LengthEdit::compose_all`], porque o nome `compose` é o do método
//!   de instância do trait [`BaseEdit`].
//! - `LengthEdit.fromEdit(edit: AnyEdit)` é genérico em qualquer [`BaseEdit`] (o `AnyEdit` do original é a união
//!   de todas as edições, sem equivalente em Rust).
//! - `applyArray` devolve um `Vec<T>` sempre preenchido. Num intervalo que passa do fim do array o original
//!   deixa buracos (`undefined`); aqui isso lança um pânico de índice fora do limite.

use je_base::errors::{self, Error};

use super::edit::{BaseEdit, BaseReplacement, validate_replacements};
use crate::offset_range::OffsetRange;

/// Como uma edição normal, mas só captura a informação de comprimento.
#[derive(Clone, Debug)]
pub struct LengthEdit {
    replacements: Vec<LengthReplacement>,
}

impl LengthEdit {
    pub fn new(replacements: Vec<LengthReplacement>) -> Self {
        validate_replacements(&replacements);
        Self { replacements }
    }

    /// `LengthEdit.empty`.
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn from_edit<E: BaseEdit>(edit: &E) -> Self {
        Self::new(
            edit.replacements()
                .iter()
                .map(|r| LengthReplacement::new(r.replace_range(), r.get_new_length()))
                .collect(),
        )
    }

    pub fn create(replacements: Vec<LengthReplacement>) -> Self {
        Self::new(replacements)
    }

    pub fn single(replacement: LengthReplacement) -> Self {
        Self::new(vec![replacement])
    }

    pub fn replace(range: OffsetRange, new_length: i32) -> Self {
        Self::new(vec![LengthReplacement::new(range, new_length)])
    }

    pub fn insert(offset: i32, new_length: i32) -> Self {
        Self::new(vec![LengthReplacement::new(OffsetRange::empty_at(offset), new_length)])
    }

    pub fn delete(range: OffsetRange) -> Self {
        Self::new(vec![LengthReplacement::new(range, 0)])
    }

    /// `LengthEdit.compose(edits)`.
    pub fn compose_all(edits: &[LengthEdit]) -> Self {
        let mut e = Self::empty();
        for edit in edits {
            e = e.compose(edit);
        }
        e
    }

    /// Cria uma edição que desfaz esta.
    pub fn inverse(&self) -> Self {
        let mut edits: Vec<LengthReplacement> = Vec::new();
        let mut offset = 0;
        for e in &self.replacements {
            edits.push(LengthReplacement::new(
                OffsetRange::of_start_and_length(e.replace_range.start + offset, e.new_length),
                e.replace_range.length(),
            ));
            offset += e.new_length - e.replace_range.length();
        }
        Self::new(edits)
    }

    pub fn apply_array<T: Clone>(&self, arr: &[T], fill_item: T) -> Vec<T> {
        let mut new_arr = Vec::with_capacity(self.get_new_data_length(arr.len() as i32).max(0) as usize);

        let mut src_pos = 0usize;

        for replacement in &self.replacements {
            // Copy items before the current replacement
            let start = replacement.replace_range.start as usize;
            new_arr.extend_from_slice(&arr[src_pos..start.max(src_pos)]);

            // Skip the replaced items in the source array
            src_pos = replacement.replace_range.end_exclusive as usize;

            // Fill with the provided fillItem for insertions
            new_arr.extend(std::iter::repeat_n(fill_item.clone(), replacement.new_length.max(0) as usize));
        }

        // Copy any remaining items from the original array
        if src_pos < arr.len() {
            new_arr.extend_from_slice(&arr[src_pos..]);
        }

        new_arr
    }
}

impl BaseEdit for LengthEdit {
    type Replacement = LengthReplacement;

    fn replacements(&self) -> &[LengthReplacement] {
        &self.replacements
    }

    fn create_new(replacements: Vec<LengthReplacement>) -> Self {
        Self::new(replacements)
    }
}

#[derive(Clone, Debug)]
pub struct LengthReplacement {
    pub replace_range: OffsetRange,
    pub new_length: i32,
}

impl LengthReplacement {
    pub fn new(range: OffsetRange, new_length: i32) -> Self {
        Self { replace_range: range, new_length }
    }

    pub fn create(start_offset: i32, end_offset_exclusive: i32, new_length: i32) -> Self {
        Self::new(OffsetRange::new(start_offset, end_offset_exclusive), new_length)
    }
}

impl BaseReplacement for LengthReplacement {
    fn replace_range(&self) -> OffsetRange {
        self.replace_range
    }

    fn equals(&self, other: &Self) -> bool {
        self.replace_range.equals(other.replace_range) && self.new_length == other.new_length
    }

    fn get_new_length(&self) -> i32 {
        self.new_length
    }

    fn try_join_touching(&self, other: &Self) -> Option<Self> {
        Some(Self::new(self.replace_range.join_right_touching(other.replace_range), self.new_length + other.new_length))
    }

    fn slice(&self, range: OffsetRange, range_in_replacement: Option<OffsetRange>) -> Self {
        // No original `rangeInReplacement` é obrigatório aqui: sem ele a leitura de `undefined` lança.
        let Some(range_in_replacement) = range_in_replacement else {
            errors::throw(Error::named("TypeError", "Cannot read properties of undefined (reading 'length')"));
        };
        Self::new(range, range_in_replacement.length())
    }

    fn describe(&self) -> String {
        format!("[{}, +{}) -> +{}}}", self.replace_range.start, self.replace_range.length(), self.new_length)
    }
}
