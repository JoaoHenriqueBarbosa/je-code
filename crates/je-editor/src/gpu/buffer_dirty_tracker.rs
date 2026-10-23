//! Porte de `src/vs/editor/browser/gpu/bufferDirtyTracker.ts`: um rastreador simples das regiões sujas de
//! um buffer.
//!
//! Divergências:
//! - Os índices são `usize` (no original, `number`), e `undefined` vira `None`.
//! - O estado fica em `Cell`, então `flag` e `clear` recebem `&self`. O original compartilha o mesmo objeto
//!   entre o `ObjectCollectionBuffer` (que marca) e o leitor externo (que limpa); com `&self` o
//!   `Rc<BufferDirtyTracker>` faz o mesmo papel sem `RefCell`.
//! - `flag(index, length = 1)` vira duas funções, `flag(index)` e `flag_range(index, length)`, porque o Rust
//!   não tem argumento padrão.

use std::cell::Cell;

/// `IBufferDirtyTrackerReader`.
pub trait IBufferDirtyTrackerReader {
    /// O índice do primeiro índice sujo.
    fn data_offset(&self) -> Option<usize>;
    /// A quantidade de índices entre o primeiro e o último sujo (o último é inclusivo).
    fn dirty_size(&self) -> Option<usize>;
    /// Se o buffer está sujo.
    fn is_dirty(&self) -> bool;
    /// Limpa o estado sujo.
    fn clear(&self);
}

/// `BufferDirtyTracker`: um rastreador simples das regiões sujas de um buffer.
#[derive(Debug, Default)]
pub struct BufferDirtyTracker {
    start_index: Cell<Option<usize>>,
    end_index: Cell<Option<usize>>,
}

impl BufferDirtyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Marca o índice como modificado e devolve o índice marcado.
    pub fn flag(&self, index: usize) -> usize {
        self.flag_range(index, 1)
    }

    /// Marca `length` índices a partir de `index` como modificados e devolve `index`.
    pub fn flag_range(&self, index: usize, length: usize) -> usize {
        self.flag_one(index);
        if length > 1 {
            self.flag_one(index + length - 1);
        }
        index
    }

    fn flag_one(&self, index: usize) {
        if self.start_index.get().is_none_or(|start| index < start) {
            self.start_index.set(Some(index));
        }
        if self.end_index.get().is_none_or(|end| index > end) {
            self.end_index.set(Some(index));
        }
    }
}

impl IBufferDirtyTrackerReader for BufferDirtyTracker {
    fn data_offset(&self) -> Option<usize> {
        self.start_index.get()
    }

    fn dirty_size(&self) -> Option<usize> {
        Some(self.end_index.get()? - self.start_index.get()? + 1)
    }

    fn is_dirty(&self) -> bool {
        self.start_index.get().is_some()
    }

    fn clear(&self) {
        self.start_index.set(None);
        self.end_index.set(None);
    }
}
