//! `SeqStore_t`: literais e sequências produzidos pelo buscador de correspondências.

/// Offset base das repetições: `REPCODE1_TO_OFFBASE`.
pub const REPCODE1_OFFBASE: u32 = 1;

/// `OFFSET_TO_OFFBASE`: offsets reais ficam acima dos três códigos de repetição.
pub const fn offset_to_offbase(offset: u32) -> u32 {
    offset + 3
}

/// Uma sequência (`SeqDef`), com o tamanho de correspondência inteiro (o libzstd guarda `mlBase`, ou seja, menos 3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sequence {
    pub lit_length: usize,
    pub off_base: u32,
    pub match_length: usize,
}

#[derive(Default)]
pub struct SeqStore {
    pub literals: Vec<u8>,
    pub sequences: Vec<Sequence>,
}

impl SeqStore {
    pub fn clear(&mut self) {
        self.literals.clear();
        self.sequences.clear();
    }

    /// `ZSTD_storeSeq`: `literals` são os `lit_length` bytes que precedem a correspondência.
    pub fn store(&mut self, literals: &[u8], off_base: u32, match_length: usize) {
        self.literals.extend_from_slice(literals);
        self.sequences.push(Sequence { lit_length: literals.len(), off_base, match_length });
    }
}
