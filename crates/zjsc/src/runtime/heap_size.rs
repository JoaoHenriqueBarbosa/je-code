//! A contabilidade de bytes do coletor: o relógio de bytes alocados, as classes de tamanho do `MarkedSpace` e a
//! política de tamanho de heap de `Heap::updateAllocationLimits` / `proportionalHeapSize`.
//!
//! O JSC dispara a coleta quando os bytes alocados no ciclo passam do que o heap permite (`Heap::shouldRequestGC`),
//! contados em tamanho de classe de alocação (`MarkedSpace::optimalSizeFor`), não em número de células. Este porte
//! mantém um relógio de bytes: cada célula nova soma o tamanho da classe dela, e o armazenamento auxiliar (a
//! `Butterfly`: propriedades fora de linha e elementos indexados) soma quando é alocado ou cresce, como o
//! `AuxiliarySpace` do C++.
//!
//! Calibração (bun 1.4.2 do oráculo, medida com `process.memoryUsage().heapUsed`): a primeira coleta cai com
//! ~8 MiB alocados (o heap do bun nasce com ~0,5 MB), igual para objeto pequeno, objeto de seis propriedades,
//! array de oito elementos, string e array de duzentos. É `minHeapSize(Large, ramSize)` com
//! `ramSize * smallHeapRAMFraction = 8 MiB`, isto é, um `ramSize` efetivo de 32 MiB; o crescimento seguinte é a
//! heurística antiga de `proportionalHeapSize` (fator 2 abaixo de 8 MiB vivos, 1,5 abaixo de 16 MiB e 1,24 acima),
//! que o ramSize abaixo de 16 GiB seleciona. Os tamanhos de célula são os de `sizeof` do C++ (release, x86-64).

use std::cell::Cell;
use std::sync::OnceLock;

use crate::runtime::cell_registry::CellEntry;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_type::JSType;

/// `MarkedBlock::atomSize` (= `MarkedSpace::sizeStep`).
const ATOM_SIZE: usize = 16;
/// `MarkedSpace::preciseCutoff`: até aqui cada múltiplo de `ATOM_SIZE` é uma classe.
const PRECISE_CUTOFF: usize = 80;
/// `MarkedBlock::payloadSize`: o bloco de 16 KiB menos o `Header` (304 bytes).
const BLOCK_PAYLOAD: usize = 16_080;
/// `MarkedSpace::largeCutoff`: acima disso a alocação é precisa (tamanho exato).
const LARGE_CUTOFF: usize = (BLOCK_PAYLOAD / 2) & !(ATOM_SIZE - 1);
/// `Options::sizeClassProgression`.
const SIZE_CLASS_PROGRESSION: f64 = 1.4;
/// `Options::preciseAllocationCutoff`.
const PRECISE_ALLOCATION_CUTOFF: usize = 100_000;

/// `Heap::m_minBytesPerCycle` do `HeapType::Large` medido no oráculo (ver o cabeçalho do módulo).
pub const MIN_BYTES_PER_CYCLE: usize = 8 * 1024 * 1024;
/// `sizeof(StackFrame)`: um `Variant<JSFrameData, WasmFrameData>` (callee, codeBlock, bytecodeIndex e a marca de
/// async em 24 bytes mais o discriminante, alinhados a 32). `Exception::finishCreation` reporta
/// `m_stack.size() * sizeof(StackFrame)` como memória extra.
pub const STACK_FRAME_BYTES: usize = 32;
/// `ramSize * Options::smallHeapRAMFraction` e `* mediumHeapRAMFraction` do ramSize efetivo de 32 MiB.
const SMALL_HEAP_LIMIT: usize = 8 * 1024 * 1024;
const MEDIUM_HEAP_LIMIT: usize = 16 * 1024 * 1024;

/// `MarkedSpace::sizeClasses()`: as classes de tamanho em ordem crescente.
fn size_classes() -> &'static [usize] {
    static CLASSES: OnceLock<Vec<usize>> = OnceLock::new();
    CLASSES.get_or_init(|| {
        let mut result: Vec<usize> = (1..PRECISE_CUTOFF / ATOM_SIZE + 1).map(|step| step * ATOM_SIZE).filter(|size| *size < PRECISE_CUTOFF).collect();
        for i in 0u32.. {
            let approximate = (PRECISE_CUTOFF as f64 * SIZE_CLASS_PROGRESSION.powi(i as i32)) as usize;
            if approximate > LARGE_CUTOFF {
                break;
            }
            let size_class = approximate.div_ceil(ATOM_SIZE) * ATOM_SIZE;
            // Escolhe a classe que não deixa sobra no fim do bloco, se a troca reduz o desperdício.
            let cells_per_block = BLOCK_PAYLOAD / size_class;
            let possibly_better = (BLOCK_PAYLOAD / cells_per_block) & !(ATOM_SIZE - 1);
            let original_wastage = BLOCK_PAYLOAD - cells_per_block * size_class;
            let new_wastage = (possibly_better - size_class) * cells_per_block;
            let better = if new_wastage > original_wastage { size_class } else { possibly_better };
            if result.last() == Some(&better) {
                continue;
            }
            if better > LARGE_CUTOFF || better > PRECISE_ALLOCATION_CUTOFF {
                break;
            }
            result.push(better.div_ceil(ATOM_SIZE) * ATOM_SIZE);
        }
        result.push(256);
        result.sort_unstable();
        result.dedup();
        result
    })
}

/// `MarkedSpace::optimalSizeFor`: o tamanho da classe que serve `bytes`.
pub fn optimal_size_for(bytes: usize) -> usize {
    if bytes == 0 {
        return 0;
    }
    if bytes <= PRECISE_CUTOFF {
        return bytes.div_ceil(ATOM_SIZE) * ATOM_SIZE;
    }
    if bytes <= LARGE_CUTOFF {
        let classes = size_classes();
        return classes[classes.partition_point(|class| *class < bytes)];
    }
    bytes
}

thread_local! {
    /// Os bytes alocados desde o início da thread: o relógio do gatilho do coletor.
    static BYTES_ALLOCATED: Cell<usize> = const { Cell::new(0) };
}

/// `Heap::didAllocate(bytes)` (o tamanho já na classe de alocação).
pub(crate) fn did_allocate(bytes: usize) {
    let _ = BYTES_ALLOCATED.try_with(|clock| clock.set(clock.get().saturating_add(bytes)));
}

/// O relógio de bytes alocados.
pub fn bytes_allocated() -> usize {
    BYTES_ALLOCATED.try_with(Cell::get).unwrap_or(0)
}

/// `Heap::reportExtraMemoryAllocated`: memória fora do heap (conteúdo de string, `ArrayBuffer`) que o C++ também
/// põe no relógio, em bytes exatos.
pub(crate) fn report_extra_memory(bytes: usize) {
    did_allocate(bytes);
}

/// A `Butterfly` nova de um objeto: o auxiliar que `allocateMoreOutOfLineStorage` e `ensureLength` alocam inteiro (o
/// novo, não o acréscimo), em classe de alocação.
pub(crate) fn did_allocate_butterfly(out_of_line_capacity: usize, vector_length: usize, has_indexing_header: bool) {
    did_allocate(butterfly_bytes(out_of_line_capacity, vector_length, has_indexing_header));
}

/// `Butterfly::totalSize` em classe de alocação; zero quando não há propriedade fora de linha nem índice.
fn butterfly_bytes(out_of_line_capacity: usize, vector_length: usize, has_indexing_header: bool) -> usize {
    let header = if has_indexing_header { 8 } else { 0 };
    let total = out_of_line_capacity * 8 + header + vector_length * 8;
    if total == 0 { 0 } else { optimal_size_for(total) }
}

/// `Heap::proportionalHeapSize` com o `ramSize` efetivo do oráculo.
fn proportional_heap_size(heap_size: usize) -> usize {
    let factor = if heap_size < SMALL_HEAP_LIMIT {
        2.0
    } else if heap_size < MEDIUM_HEAP_LIMIT {
        1.5
    } else {
        1.24
    };
    (factor * heap_size as f64) as usize
}

/// `Heap::updateAllocationLimits` depois de uma coleta completa: quantos bytes o próximo ciclo pode alocar
/// (`m_maxEdenSize = m_maxHeapSize - currentHeapSize`).
pub(crate) fn bytes_allowed_after_full_collection(live_bytes: usize) -> usize {
    let max_heap = MIN_BYTES_PER_CYCLE.max(proportional_heap_size(live_bytes));
    max_heap - live_bytes.min(max_heap)
}

/// O que a alocação da célula soma ao relógio: a célula na classe de alocação. O armazenamento auxiliar entra à parte,
/// quando é alocado (`did_allocate_butterfly`), e o conteúdo de string não entra (medido: o bun conta 50 bytes por
/// `'x'.repeat(n) + i`, um rope de 32 mais uma string de 16).
pub(crate) fn allocation_bytes(entry: &CellEntry) -> usize {
    cell_bytes_with(entry, false)
}

/// O tamanho da célula na classe de alocação mais o armazenamento auxiliar atual dela: o que a célula pesa no heap
/// vivo (`m_totalBytesVisited`).
pub(crate) fn cell_bytes(entry: &CellEntry) -> usize {
    cell_bytes_with(entry, true)
}

fn cell_bytes_with(entry: &CellEntry, with_auxiliary: bool) -> usize {
    match entry {
        CellEntry::String(string) => {
            if string.is_rope() { 32 } else { 16 }
        }
        CellEntry::Symbol(_) | CellEntry::BigInt(_) => 32,
        CellEntry::RegExp(_) => 64,
        CellEntry::CellButterfly(_) | CellEntry::TemplateObjectDescriptor(_) => 32,
        CellEntry::SymbolTable(_) => 64,
        CellEntry::Callee(_) => 32,
        // `Exception` (célula de 48) mais o `m_stack` que `reportExtraMemoryAllocated` contou e o coletor visita.
        CellEntry::Exception(exception) => 48 + exception.stack_len() * STACK_FRAME_BYTES,
        CellEntry::Promise(_) => 32,
        CellEntry::SlimPromiseReaction(_) => 32,
        CellEntry::FullPromiseReaction(_) | CellEntry::PromiseCombinatorsContext(_) | CellEntry::PromiseCombinatorsGlobalContext(_) => 64,
        _ => entry.as_js_object().map_or(48, |object| object_bytes(object, with_auxiliary)),
    }
}

/// Uma célula de objeto: `JSFinalObject` e os demais por `sizeof` do C++, mais (se pedido) a `Butterfly` atual.
fn object_bytes(object: &JSObject, with_auxiliary: bool) -> usize {
    let structure = object.structure();
    let inline_capacity = structure.inline_capacity() as usize;
    let cell = match object.type_() {
        JSType::FinalObjectType => optimal_size_for(16 + 8 * inline_capacity),
        JSType::ArrayType | JSType::DerivedArrayType => optimal_size_for(16 + 8 * inline_capacity),
        JSType::JSFunctionType => 48,
        JSType::ErrorInstanceType => 64,
        _ => optimal_size_for((24 + 8 * inline_capacity).max(32)),
    };
    if !with_auxiliary {
        return cell;
    }
    let (vector_length, has_indexing) = object.butterfly_shape();
    cell + butterfly_bytes(structure.out_of_line_capacity() as usize, vector_length, has_indexing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_sizes_round_to_the_atom() {
        assert_eq!(optimal_size_for(24), 32);
        assert_eq!(optimal_size_for(64), 64);
        assert_eq!(optimal_size_for(80), 80);
    }

    #[test]
    fn classes_above_the_precise_cutoff_grow_geometrically() {
        let classes = size_classes();
        assert!(classes.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(classes[0], 16);
        assert!(classes.contains(&256));
        assert!(optimal_size_for(1608) >= 1608 && optimal_size_for(1608) < 2048);
        assert_eq!(optimal_size_for(100_000), 100_000);
    }

    #[test]
    fn first_cycle_allows_eight_mebibytes_and_growth_follows_the_old_heuristic() {
        assert_eq!(bytes_allowed_after_full_collection(0), MIN_BYTES_PER_CYCLE);
        assert_eq!(bytes_allowed_after_full_collection(1024 * 1024), MIN_BYTES_PER_CYCLE - 1024 * 1024);
        assert_eq!(bytes_allowed_after_full_collection(6 * 1024 * 1024), 6 * 1024 * 1024);
        assert_eq!(bytes_allowed_after_full_collection(32 * 1024 * 1024), (0.24 * 32.0 * 1024.0 * 1024.0) as usize);
    }
}
