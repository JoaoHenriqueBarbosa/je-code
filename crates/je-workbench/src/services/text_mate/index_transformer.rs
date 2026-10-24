//! Porte de `src/vs/workbench/services/textMate/browser/indexTransformer.ts`: transforma índices do valor antigo
//! no valor novo de uma ou várias edições.
//!
//! Divergências:
//! - `AnyEdit` (a união de todas as edições) vira o genérico [`BaseEdit`]. O transformador guarda só o que lê das
//!   substituições, o intervalo e a variação de comprimento (`getLengthDelta`), então o tipo da edição não aparece
//!   no tipo do transformador.
//! - `transform` leva `&mut self`, porque ele avança o estado interno (`idx` e `offset`).
//! - Os índices são `i32`, como os deslocamentos de [`OffsetRange`].

use je_editor::edits::edit::{BaseEdit, BaseReplacement};
use je_editor::offset_range::OffsetRange;

pub trait IIndexTransformer {
    fn transform(&mut self, index: i32) -> Option<i32>;
}

/// Só pode ser chamado com valores crescentes de `index`.
pub struct MonotonousIndexTransformer {
    idx: usize,
    offset: i32,
    /// Para cada substituição da edição: o intervalo substituído e a variação de comprimento.
    replacements: Vec<(OffsetRange, i32)>,
}

impl MonotonousIndexTransformer {
    pub fn from_many<E: BaseEdit>(transformations: &[E]) -> Box<dyn IIndexTransformer> {
        // O original anota que dá para ganhar desempenho combinando as transformações antes.
        let transformers = transformations
            .iter()
            .map(|t| Box::new(MonotonousIndexTransformer::new(t)) as Box<dyn IIndexTransformer>)
            .collect();
        Box::new(CombinedIndexTransformer::new(transformers))
    }

    pub fn new<E: BaseEdit>(transformation: &E) -> Self {
        Self {
            idx: 0,
            offset: 0,
            replacements: transformation.replacements().iter().map(|r| (r.replace_range(), r.get_length_delta())).collect(),
        }
    }
}

impl IIndexTransformer for MonotonousIndexTransformer {
    /// Pré-condição: `index >= previous-value-of(index)`.
    fn transform(&mut self, index: i32) -> Option<i32> {
        let mut next_change = self.replacements.get(self.idx);
        while let Some((range, length_delta)) = next_change
            && range.end_exclusive <= index
        {
            self.offset += length_delta;
            self.idx += 1;
            next_change = self.replacements.get(self.idx);
        }
        // assert nextChange === undefined || index < nextChange.offset + nextChange.length

        if let Some((range, _)) = next_change
            && range.start <= index
        {
            // Offset is touched by the change
            return None;
        }

        Some(index + self.offset)
    }
}

pub struct CombinedIndexTransformer {
    transformers: Vec<Box<dyn IIndexTransformer>>,
}

impl CombinedIndexTransformer {
    pub fn new(transformers: Vec<Box<dyn IIndexTransformer>>) -> Self {
        Self { transformers }
    }
}

impl IIndexTransformer for CombinedIndexTransformer {
    fn transform(&mut self, index: i32) -> Option<i32> {
        let mut index = index;
        for transformer in &mut self.transformers {
            index = transformer.transform(index)?;
        }
        Some(index)
    }
}
