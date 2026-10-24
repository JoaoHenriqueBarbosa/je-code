//! Porte reduzido de `heap/Heap.h` e `heap/MarkedSpace.cpp`: só o que o porte sem GC precisa, a tabela
//! de size classes do `MarkedSpace` e a consulta `subspaceFor<JSFinalObject>(vm)->allocatorFor(size,
//! EnsureAllocator)` que o `ObjectAllocationProfile` faz.
//!
//! Divergências:
//!
//! - Não há coleta, blocos nem `LocalAllocator`: as células são `Rc` e o `Heap` só guarda a tabela
//!   de size classes (`sizeClasses()` de `MarkedSpace.cpp`, com o `add(256)` incluso). O `Allocator`
//!   do C++ (um `LocalAllocator*` por size class) vira o `cellSize` do size class, que é único por
//!   size class dentro de um subspace e nunca zero, ou seja, serve de identidade.
//! - `MarkedSpace::blockPayload` depende do `sizeof(MarkedBlock::Header)`: aqui é `BLOCK_PAYLOAD`,
//!   calculado à mão para x86_64 Linux (bloco de 16 KB, cabeçalho de 320 bytes). Um erro de poucos
//!   bytes nele não muda as size classes abaixo de 8 KB que os objetos finais usam.
//! - Um único subspace: o de `JSFinalObject` e o de qualquer outro tipo teriam as mesmas size classes.

/// `MarkedBlock::atomSize` e `MarkedSpace::sizeStep`.
const SIZE_STEP: usize = 16;
/// `MarkedSpace::preciseCutoff`.
const PRECISE_CUTOFF: usize = 80;
/// `MarkedBlock::blockSize` (`max(16 * KB, CeilingOnPageSize)` em x86_64 Linux).
const BLOCK_SIZE: usize = 16 * 1024;
/// `MarkedBlock::headerSize`, arredondado ao átomo.
const BLOCK_HEADER_SIZE: usize = 320;
/// `MarkedSpace::blockPayload`.
const BLOCK_PAYLOAD: usize = BLOCK_SIZE - BLOCK_HEADER_SIZE;
/// `MarkedSpace::largeCutoff`.
const LARGE_CUTOFF: usize = (BLOCK_PAYLOAD / 2) & !(SIZE_STEP - 1);
/// `Options::sizeClassProgression`.
const SIZE_CLASS_PROGRESSION: f64 = 1.4;
/// `Options::preciseAllocationCutoff`.
const PRECISE_ALLOCATION_CUTOFF: usize = 100000;

/// O `Allocator` com o tamanho de célula do size class (`Allocator::cellSize()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocatorInfo {
    /// Identidade do `Allocator` (o `LocalAllocator*` do C++): o tamanho de célula do size class.
    pub allocator: u64,
    pub cell_size: usize,
}

/// `sizeClasses()` de `MarkedSpace.cpp`.
fn size_classes() -> Vec<usize> {
    let mut result: Vec<usize> = Vec::new();
    let round_up = |value: usize| value.div_ceil(SIZE_STEP) * SIZE_STEP;

    let mut size = SIZE_STEP;
    while size < PRECISE_CUTOFF {
        result.push(round_up(size));
        size += SIZE_STEP;
    }

    let mut i = 0u32;
    loop {
        let approximate_size = PRECISE_CUTOFF as f64 * SIZE_CLASS_PROGRESSION.powf(i as f64);
        let approximate_size_in_bytes = approximate_size as usize;
        assert!(approximate_size_in_bytes >= PRECISE_CUTOFF);
        if approximate_size_in_bytes > LARGE_CUTOFF {
            break;
        }
        i += 1;

        let size_class = round_up(approximate_size_in_bytes);
        // Escolhe o size class sem sobra no fim do bloco.
        let cells_per_block = BLOCK_PAYLOAD / size_class;
        let possibly_better_size_class = (BLOCK_PAYLOAD / cells_per_block) & !(SIZE_STEP - 1);
        let original_wastage = BLOCK_PAYLOAD - cells_per_block * size_class;
        let new_wastage = (possibly_better_size_class - size_class) * cells_per_block;
        let better_size_class = if new_wastage > original_wastage { size_class } else { possibly_better_size_class };

        if Some(&better_size_class) == result.last() {
            continue;
        }
        if better_size_class > LARGE_CUTOFF || better_size_class > PRECISE_ALLOCATION_CUTOFF {
            break;
        }
        result.push(round_up(better_size_class));
    }

    // Size class injetado à mão para objetos de alto volume.
    result.push(256);
    result.sort_unstable();
    result.dedup();
    result
}

/// `class Heap`, reduzido (ver o topo do módulo).
#[derive(Debug)]
pub struct Heap {
    size_classes: Vec<usize>,
    /// `unsigned m_deferralDepth`: o contador que o `DeferGC` mexe. Sem coleta ele só é observável por
    /// `isDeferred()`; `decrementDeferralDepthAndGCIfNeeded` não tem GC para disparar (`m_didDeferGCWork`
    /// nunca é setado).
    deferral_depth: std::cell::Cell<usize>,
}

impl Default for Heap {
    fn default() -> Heap {
        Heap::new()
    }
}

impl Heap {
    pub fn new() -> Heap {
        Heap { size_classes: size_classes(), deferral_depth: std::cell::Cell::new(0) }
    }

    /// `isDeferred()`: `!!m_deferralDepth`.
    pub fn is_deferred(&self) -> bool {
        self.deferral_depth.get() != 0
    }

    /// `incrementDeferralDepth()`.
    pub fn increment_deferral_depth(&self) {
        self.deferral_depth.set(self.deferral_depth.get() + 1);
    }

    /// `decrementDeferralDepthAndGCIfNeeded()`: sem GC, só o decremento.
    pub fn decrement_deferral_depth_and_gc_if_needed(&self) {
        debug_assert!(self.deferral_depth.get() > 0);
        self.deferral_depth.set(self.deferral_depth.get() - 1);
    }

    /// `subspaceFor<JSFinalObject>(vm)->allocatorFor(allocationSize, AllocatorForMode::EnsureAllocator)`:
    /// o allocator do size class que comporta `allocation_size`, ou `None` se passa de `largeCutoff`
    /// (o `Allocator()` vazio).
    pub fn final_object_allocator_for(&self, allocation_size: usize) -> Option<AllocatorInfo> {
        debug_assert!(allocation_size > 0);
        if allocation_size > LARGE_CUTOFF {
            return None;
        }
        let cell_size = if allocation_size <= PRECISE_CUTOFF {
            allocation_size.div_ceil(SIZE_STEP) * SIZE_STEP
        } else {
            *self.size_classes.iter().find(|size_class| **size_class >= allocation_size)?
        };
        Some(AllocatorInfo { allocator: cell_size as u64, cell_size })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_object_size_classes() {
        let heap = Heap::new();
        // JSFinalObject com 6 slots inline: 16 + 48 = 64, sem sobra.
        assert_eq!(heap.final_object_allocator_for(64).unwrap().cell_size, 64);
        // 72 bytes caem no size class de 80.
        assert_eq!(heap.final_object_allocator_for(72).unwrap().cell_size, 80);
        // Acima de 80 vale a progressão: 112, 160, 224, 256 (injetado), 320...
        assert_eq!(heap.final_object_allocator_for(88).unwrap().cell_size, 112);
        assert_eq!(heap.final_object_allocator_for(228).unwrap().cell_size, 256);
        // O maior objeto final (62 slots) cabe.
        assert!(heap.final_object_allocator_for(16 + 62 * 8).is_some());
        assert!(heap.final_object_allocator_for(LARGE_CUTOFF + 1).is_none());
    }
}
