//! O interpretador de bytecode WebAssembly, no espírito do IPInt (`llint/InPlaceInterpreter64.asm`):
//! um laço sobre o corpo já validado, com pilha de valores e controle estruturado. O IPInt lê o
//! bytecode in loco com um metadado por função (comprimento de instrução, alvo de salto); aqui o
//! metadado é o mapa `jumps` (posição do `block`/`loop`/`if` para a do `else` e a do `end`),
//! calculado por uma varredura única na primeira chamada de cada função (`CompiledFunction`).
//!
//! Valores são `u64` na pilha: i32 com os 32 bits altos zerados, f32 e f64 em bits, referência
//! codificada (ver `wasm_instance::null_ref` e `func_ref`). Cobertos: controle, chamadas diretas,
//! indiretas, importadas e de cauda, locais, globais, memória (com `memory.grow`, multi-memória e
//! memória de 64 bits), bulk memory, tabelas, referências, aritmética inteira e de ponto flutuante
//! com os traps da especificação e as conversões saturadas, e exceções (`throw`, `throw_ref`, `rethrow`, `try`
//! legado com `catch`/`catch_all`/`delegate`, `try_table`; pilha de `Handler` por `try`). Ficam para as fatias 28
//! e 29 do plano: GC, atômicos e SIMD; uma instrução destas responde com um
//! `WasmError::Runtime("unsupported ...")` em vez de executar errado.
//!
//! Desvio: a profundidade de chamadas é limitada por `MAX_CALL_DEPTH` (o IPInt usa a pilha da máquina e o
//! limite vem de `Options::softReservedZoneSize`). As chamadas wasm para wasm não recursam no Rust: `run_frames`
//! mantém um `Vec<Frame>` explícito. Só a importação do host e a reentrada vinda do JS passam por `invoke`.

use std::collections::HashMap;
use std::rc::Rc;

use crate::wasm::wasm_function_validator::validate_function_with_wide_operands;
use crate::wasm::wasm_simd;
use crate::wasm::wasm_simd_opcodes::{ExtSimdOpType, SimdLane, SimdLaneOperation, SimdSignMode};

use crate::wasm::page_count::PageCount;
use crate::wasm::wasm_memory::Memory;
use crate::wasm::wasm_exception_type::{error_message_for_exception_type, ExceptionType};
use crate::wasm::wasm_format::{FieldType, PackedType, StorageType, Type, TypeIndex, TypeKind};
use crate::wasm::wasm_limits::MAX_FUNCTION_LOCALS;
use crate::wasm::wasm_instance::{
    func_ref, func_ref_local, func_ref_target, gc_ref, gc_ref_index, i31_ref, i31_ref_value, instance_of, is_func_ref, null_ref, shared_gc_heap, GcCell, Instance, TagRef, WasmError,
    EXNREF_TAG, InstanceId,
};
use crate::wasm::wasm_module_information::{is_subtype_index, ModuleInformation, StructuralType};

/// A profundidade máxima de chamadas wasm aninhadas.
pub const MAX_CALL_DEPTH: u32 = 400;
/// Quantos quadros terminados o laço guarda para reaproveitar.
const SPARE_FRAMES: usize = 64;

/// `Callee` do IPInt: o corpo e o que se calcula uma vez por função.
#[derive(Debug)]
pub struct CompiledFunction {
    /// Slots dos parâmetros e dos resultados da assinatura (um `v128` ocupa dois), fixos por função: o laço de
    /// quadros não consulta o tipo a cada chamada.
    param_slots: usize,
    return_slots: usize,
    code: Vec<u8>,
    /// Onde a primeira instrução começa (depois dos locais declarados).
    code_start: usize,
    /// O valor inicial de cada slot dos locais declarados (os parâmetros vêm dos argumentos); um local `v128`
    /// ocupa dois slots.
    extra_locals: Vec<u64>,
    /// O primeiro slot de cada local (parâmetros primeiro), já que um `v128` ocupa dois.
    local_offsets: Vec<usize>,
    /// Quais locais (parâmetros primeiro) são `v128`.
    local_wide: Vec<bool>,
    /// Posições dos `drop` e `select` cujo operando é um `v128` (dois slots), pela pilha de tipos do validador.
    wide_operands: PcMap<()>,
    /// Posição do `block`/`loop`/`if` para (posição depois do `else`, posição depois do `end`).
    jumps: PcMap<(Option<usize>, usize)>,
    /// Posição do `try` legado para as posições dos seus `catch` e `catch_all`, em ordem.
    catches: PcMap<Vec<usize>>,
    /// Posição do `try` legado que fecha em `delegate` para a profundidade do `delegate`.
    delegates: PcMap<u32>,
}

/// Mapa de posição de código para metadado: vetor ordenado por posição com busca binária. O laço quente consulta
/// `jumps` a cada `block`/`loop`/`if`; o `HashMap` com SipHash custava um hash por consulta, e a busca binária
/// em poucas dezenas de entradas contíguas é mais barata e não aloca.
#[derive(Debug)]
struct PcMap<V> {
    entries: Vec<(usize, V)>,
}

impl<V> PcMap<V> {
    fn new(mut entries: Vec<(usize, V)>) -> PcMap<V> {
        entries.sort_by_key(|(position, _)| *position);
        PcMap { entries }
    }

    fn get(&self, position: &usize) -> Option<&V> {
        self.entries.binary_search_by_key(position, |(key, _)| *key).ok().map(|slot| &self.entries[slot].1)
    }

    fn contains(&self, position: &usize) -> bool {
        self.get(position).is_some()
    }
}

impl<V> std::ops::Index<&usize> for PcMap<V> {
    type Output = V;

    fn index(&self, position: &usize) -> &V {
        self.get(position).expect("posição sem metadado de controle")
    }
}

/// Um tratador ativo, ligado ao rótulo do `try` (índice em `labels`).
struct Handler {
    label_index: usize,
    kind: HandlerKind,
}

enum HandlerKind {
    /// `try` legado com `catch`/`catch_all`: as cláusulas estão em `CompiledFunction::catches`.
    Legacy { try_pc: usize },
    /// `try ... delegate depth`.
    Delegate { depth: u32 },
    /// `try_table`: (tipo 0 a 3, tag, profundidade do rótulo fora do bloco).
    TryTable { clauses: Vec<(u8, u32, u32)> },
}

/// O objeto GC atende ao tipo heap `target` (índice definido por subtipo do RTT, ou tipo abstrato any/eq/struct/array).
fn gc_cell_matches(cell: &GcCell, target: TypeIndex) -> bool {
    match target {
        TypeIndex::Concrete(_) => is_subtype_index(TypeIndex::Concrete(cell.canonical_type()), target),
        // externref: um objeto GC externalizado (`extern.convert_any`) continua sendo o mesmo objeto, e todo
        // valor não nulo é um externref válido.
        TypeIndex::Abstract(TypeKind::Anyref | TypeKind::Eqref | TypeKind::Externref) => true,
        TypeIndex::Abstract(TypeKind::Structref) => matches!(cell, GcCell::Struct { .. }),
        TypeIndex::Abstract(TypeKind::Arrayref) => matches!(cell, GcCell::Array { .. }),
        _ => false,
    }
}

/// A referência (objeto GC ou função de qualquer instância) atende ao tipo de referência `ty` (a checagem de
/// `toWebAssemblyValue`). Os RTTs canônicos e o heap de objetos são globais, então não precisa de instância dona:
/// serve a Table e Global soltas. `null` e outras referências não atendem aqui.
pub fn reference_matches_type(reference: u64, ty: Type) -> bool {
    if let Some(index) = gc_ref_index(reference) {
        return shared_gc_heap().borrow().get(index).is_some_and(|cell| gc_cell_matches(cell, ty.index));
    }
    let Some((owner, function_index)) = func_ref_target(reference) else { return false };
    let (TypeIndex::Concrete(_), Some(instance)) = (ty.index, instance_of(owner)) else { return false };
    let info = instance.info();
    let signature = info.type_signature_index_from_function_index_space(function_index as usize) as usize;
    is_subtype_index(info.type_index_of(signature), ty.index)
}

/// `ref.test`/`ref.cast` (`WasmOperations::refCast` e `isSubtype` sobre o valor): a referência atende ao tipo heap
/// (`heap_type` negativo é o `TypeKind` abstrato, não negativo é um índice de tipo). Os valores que esta fatia
/// produz são `null`, `i31` e função; struct e array entram com a fatia dos objetos.
fn reference_matches(own: InstanceId, info: &ModuleInformation, cells: &[GcCell], reference: u64, heap_type: i64, nullable: bool) -> bool {
    if reference == null_ref() {
        return nullable;
    }
    if let Some(index) = gc_ref_index(reference) {
        let target = if heap_type >= 0 {
            info.type_index_of(heap_type as usize)
        } else {
            let Some(kind) = i8::try_from(heap_type).ok().and_then(TypeKind::from_i8) else { return false };
            TypeIndex::Abstract(kind)
        };
        return gc_cell_matches(&cells[index], target);
    }
    if heap_type >= 0 {
        // Tipo definido: só uma função própria pode atender, e o seu tipo precisa ser subtipo do pedido.
        let Some(index) = func_ref_local(own, reference) else { return false };
        let function_type = info.type_index_of(info.type_signature_index_from_function_index_space(index as usize) as usize);
        return is_subtype_index(function_type, info.type_index_of(heap_type as usize));
    }
    let Some(kind) = i8::try_from(heap_type).ok().and_then(TypeKind::from_i8) else { return false };
    match kind {
        TypeKind::Funcref => is_func_ref(reference),
        TypeKind::Anyref | TypeKind::Eqref | TypeKind::I31ref => i31_ref_value(reference).is_some(),
        // externref: qualquer valor que não seja null; os demais (none, nofunc, noextern, struct, array, exn)
        // só aceitam null, já tratado.
        TypeKind::Externref => i31_ref_value(reference).is_none() && !is_func_ref(reference),
        _ => false,
    }
}

/// O limite do tamanho de um array GC em bytes (medido no bun 1.4.2 com `array.new_default`: 2^30 bytes passam e um
/// elemento a mais falha, para i8 (2^30 elementos), i16, i32 (2^28), i64, f64 e referências (2^27, 8 bytes cada)).
const MAX_GC_ARRAY_BYTES: u64 = 1 << 30;

/// O tamanho em bytes de um elemento de array: packed 1 ou 2, i32/f32 4, i64/f64 e referências 8, v128 16.
fn storage_byte_size(storage: StorageType) -> u64 {
    match storage {
        StorageType::Packed(PackedType::I8) => 1,
        StorageType::Packed(PackedType::I16) => 2,
        StorageType::Type(ty) => match ty.kind {
            TypeKind::I32 | TypeKind::F32 => 4,
            TypeKind::V128 => 16,
            _ => 8,
        },
    }
}

/// Cabe um array de `length` elementos do tipo `storage` no limite do motor?
fn array_fits(storage: StorageType, length: u64) -> bool {
    length.checked_mul(storage_byte_size(storage)).is_some_and(|bytes| bytes <= MAX_GC_ARRAY_BYTES)
}

/// Lê um elemento de array do segmento de dados (little-endian, `size` bytes).
fn read_segment_element(bytes: &[u8], size: u64) -> u64 {
    bytes[..size as usize].iter().rev().fold(0, |acc, byte| (acc << 8) | u64::from(*byte))
}

/// Os slots de `count` elementos de `size` bytes lidos do segmento a partir de `offset`: um elemento `v128`
/// (16 bytes) vira dois slots (baixo, alto).
fn read_segment_slots(bytes: &[u8], offset: u64, count: u64, size: u64) -> Vec<u64> {
    let mut slots = Vec::new();
    for i in 0..count {
        let at = (offset + i * size) as usize;
        if size == 16 {
            slots.push(read_segment_element(&bytes[at..], 8));
            slots.push(read_segment_element(&bytes[at + 8..], 8));
        } else {
            slots.push(read_segment_element(&bytes[at..], size));
        }
    }
    slots
}

/// O valor de um campo ou elemento como a célula o guarda: os campos packed são truncados.
fn pack_storage(storage: StorageType, value: u64) -> u64 {
    match storage {
        StorageType::Packed(PackedType::I8) => value & 0xff,
        StorageType::Packed(PackedType::I16) => value & 0xffff,
        StorageType::Type(_) => value,
    }
}

/// O valor lido de um campo ou elemento: os packed são estendidos (com sinal em `get_s`, sem sinal no resto).
fn unpack_storage(storage: StorageType, value: u64, signed: bool) -> u64 {
    match (storage, signed) {
        (StorageType::Packed(PackedType::I8), true) => u64::from(value as u8 as i8 as i32 as u32),
        (StorageType::Packed(PackedType::I16), true) => u64::from(value as u16 as i16 as i32 as u32),
        _ => value,
    }
}

/// Quantos slots `u64` de uma célula GC um campo ou elemento ocupa: `v128` toma dois (baixo, alto), o resto um.
fn storage_slots(storage: StorageType) -> usize {
    match storage {
        StorageType::Type(ty) if ty.is_v128() => 2,
        _ => 1,
    }
}

/// O deslocamento, em slots, do campo `index` de uma struct (os `v128` anteriores contam dois).
fn struct_slot_offset(fields: &[FieldType], index: usize) -> usize {
    fields[..index].iter().map(|field| storage_slots(field.ty)).sum()
}

/// Os slots de uma sequência de valores vindos da pilha (um `v128` já são os dois slots): os packed são
/// truncados e o resto segue como está.
fn pack_stack_values(storages: impl Iterator<Item = StorageType>, popped: &[u64]) -> Vec<u64> {
    let mut cursor = 0;
    let mut slots = Vec::with_capacity(popped.len());
    for storage in storages {
        let width = storage_slots(storage);
        if width == 2 {
            slots.extend_from_slice(&popped[cursor..cursor + 2]);
        } else {
            slots.push(pack_storage(storage, popped[cursor]));
        }
        cursor += width;
    }
    slots
}

/// O valor padrão de um campo, nos slots que ele ocupa: zero nos numéricos (dois zeros em `v128`), null nas
/// referências anuláveis.
fn default_storage_value(storage: StorageType) -> Vec<u64> {
    match storage {
        StorageType::Packed(_) => vec![0],
        StorageType::Type(ty) => match ty.kind {
            TypeKind::V128 => vec![0, 0],
            TypeKind::RefNull => vec![null_ref()],
            _ => vec![0],
        },
    }
}

fn trap(kind: ExceptionType) -> WasmError {
    WasmError::Runtime(error_message_for_exception_type(kind).to_string())
}

/// Larguras dos sete formatos dos atômicos (`i32`, `i64`, `i32_8`, `i32_16`, `i64_8`, `i64_16`, `i64_32`), na
/// ordem em que o prefixo 0xFE os agrupa em load, store, cada rmw e cmpxchg.
const ATOMIC_WIDTHS: [usize; 7] = [4, 8, 1, 2, 1, 2, 4];

/// O endereço efetivo de um acesso atômico: alinhamento natural primeiro (`unaligned` é o erro de quem não
/// confere, medido no bun: `Unaligned memory access` nos load/store/rmw, `Out of bounds memory access` no
/// notify e no wait), depois os limites.
fn atomic_effective(memory: &Memory, address: u64, offset: u64, width: usize, unaligned: ExceptionType) -> Result<u64, WasmError> {
    let effective = address.checked_add(offset).ok_or_else(|| trap(ExceptionType::OutOfBoundsMemoryAccess))?;
    if effective % width as u64 != 0 {
        return Err(trap(unaligned));
    }
    memory.with_slice(effective, width as u64, |_| ()).ok_or_else(|| trap(ExceptionType::OutOfBoundsMemoryAccess))?;
    Ok(effective)
}

fn unsupported(what: &str) -> WasmError {
    WasmError::Runtime(format!("unsupported {}", what))
}

// LEB128 sem as conferências do `LEBDecoder`: a validação já aceitou o corpo.
fn read_u64(code: &[u8], pc: &mut usize) -> u64 {
    let mut result = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = code[*pc];
        *pc += 1;
        if shift < 64 {
            result |= u64::from(byte & 0x7f) << shift;
        }
        shift += 7;
        if byte & 0x80 == 0 {
            return result;
        }
    }
}

fn read_u32(code: &[u8], pc: &mut usize) -> u32 {
    read_u64(code, pc) as u32
}

fn read_s64(code: &[u8], pc: &mut usize) -> i64 {
    let mut result = 0i64;
    let mut shift = 0u32;
    loop {
        let byte = code[*pc];
        *pc += 1;
        if shift < 64 {
            result |= i64::from(byte & 0x7f) << shift;
        }
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 64 && byte & 0x40 != 0 {
                result |= (-1i64).wrapping_shl(shift);
            }
            return result;
        }
    }
}

/// O tipo de bloco: vazio, um valor, ou um índice na seção de tipos.
enum BlockType {
    Empty,
    Value,
    /// `v128`: o único tipo de valor que toma dois slots da pilha.
    V128,
    Index(u32),
}

fn read_block_type(code: &[u8], pc: &mut usize) -> BlockType {
    match code[*pc] {
        0x40 => {
            *pc += 1;
            BlockType::Empty
        }
        // (ref null ht) e (ref ht): o byte e o tipo heap que o segue.
        0x63 | 0x64 => {
            *pc += 1;
            read_s64(code, pc);
            BlockType::Value
        }
        // Qualquer outro byte `0x41..=0x7f` é um s33 negativo de um byte: um tipo de valor (incluindo os
        // abstratos do GC: anyref, eqref, i31ref, structref, arrayref, nullref...), nunca um índice de tipo.
        0x7b => {
            *pc += 1;
            BlockType::V128
        }
        0x41..=0x7f => {
            *pc += 1;
            BlockType::Value
        }
        _ => BlockType::Index(read_s64(code, pc) as u32),
    }
}

/// Slots da pilha que uma lista de tipos ocupa: um `v128` toma dois.
fn slot_count(types: &[Type]) -> usize {
    types.iter().map(|ty| if ty.is_v128() { 2 } else { 1 }).sum()
}

/// A largura em bytes do acesso à memória e a lane do elemento, de um load/store SIMD parcial.
fn simd_load_shape(operation: SimdLaneOperation) -> (usize, SimdLane) {
    use SimdLaneOperation as Op;
    match operation {
        Op::LoadSplat8 | Op::LoadLane8 | Op::StoreLane8 => (1, SimdLane::I8x16),
        Op::LoadSplat16 | Op::LoadLane16 | Op::StoreLane16 => (2, SimdLane::I16x8),
        Op::LoadSplat32 | Op::LoadLane32 | Op::StoreLane32 | Op::LoadPad32 => (4, SimdLane::I32x4),
        Op::LoadSplat64 | Op::LoadLane64 | Op::StoreLane64 | Op::LoadPad64 => (8, SimdLane::I64x2),
        _ => (8, SimdLane::V128),
    }
}

/// Os imediatos de uma instrução do prefixo 0xFD (SIMD), depois do opcode `sub`.
fn skip_simd_immediates(code: &[u8], pc: &mut usize, sub: u32) -> Result<(), WasmError> {
    match sub {
        // loads, store, e loads/stores de lane: memarg (e o índice de lane nos de lane).
        0x00..=0x0b | 0x54..=0x5d => {
            let align = read_u32(code, pc);
            if align & 0x40 != 0 {
                read_u32(code, pc);
            }
            read_u64(code, pc);
            if (0x54..=0x5b).contains(&sub) {
                *pc += 1;
            }
        }
        // v128.const e i8x16.shuffle: 16 bytes.
        0x0c | 0x0d => *pc += 16,
        // extract_lane e replace_lane: o índice da lane.
        0x15..=0x22 => *pc += 1,
        _ if ExtSimdOpType::from_value(sub).is_some() => {}
        _ => return Err(unsupported(&format!("instruction 0xfd {}", sub))),
    }
    Ok(())
}

/// Os imediatos de um opcode, avançando `pc`. `Err` para o que o interpretador não cobre.
fn skip_immediates(code: &[u8], pc: &mut usize, op: u8) -> Result<(), WasmError> {
    match op {
        0x0c | 0x0d | 0x10 | 0x12 | 0x14 | 0x15 | 0x20..=0x26 | 0xd2 => {
            read_u32(code, pc);
        }
        0x0e => {
            let count = read_u32(code, pc);
            for _ in 0..=count {
                read_u32(code, pc);
            }
        }
        0x11 | 0x13 => {
            read_u32(code, pc);
            read_u32(code, pc);
        }
        0x1c => {
            let count = read_u32(code, pc);
            for _ in 0..count {
                read_block_type(code, pc);
            }
        }
        0x28..=0x3e => {
            let align = read_u32(code, pc);
            if align & 0x40 != 0 {
                read_u32(code, pc);
            }
            read_u64(code, pc);
        }
        0x3f | 0x40 => {
            read_u32(code, pc);
        }
        0x41 | 0x42 => {
            read_s64(code, pc);
        }
        0x43 => *pc += 4,
        0x44 => *pc += 8,
        0xd0 => {
            read_s64(code, pc);
        }
        // throw, rethrow, catch (tag), delegate (profundidade): um índice; catch_all e throw_ref: nada.
        0x07 | 0x08 | 0x09 | 0x18 => {
            read_u32(code, pc);
        }
        0x19 | 0x0a => {}
        0x00 | 0x01 | 0x0f | 0x1a | 0x1b | 0x45..=0xc4 | 0xd1 | 0xd3 | 0xd4 => {}
        0xfe => {
            let sub = read_u32(code, pc);
            match sub {
                // atomic.fence: um byte reservado.
                3 => {
                    read_u32(code, pc);
                }
                0..=2 | 16..=78 => {
                    let align = read_u32(code, pc);
                    if align & 0x40 != 0 {
                        read_u32(code, pc);
                    }
                    read_u64(code, pc);
                }
                _ => return Err(unsupported(&format!("instruction 0xfe {}", sub))),
            }
        }
        0xfd => {
            let sub = read_u32(code, pc);
            skip_simd_immediates(code, pc, sub)?;
        }
        // GC (fatia i31): ref.test/ref.cast (e as `null` deles) levam um tipo heap; i31.new/get_s/get_u, nada.
        0xfb => {
            let sub = read_u32(code, pc);
            match sub {
                20..=23 => {
                    read_s64(code, pc);
                }
                28..=30 | 15 | 26 | 27 => {}
                // struct.new/new_default, array.new/new_default/get/get_s/get_u/set/fill: o tipo.
                0 | 1 | 6 | 7 | 11..=14 | 16 => {
                    read_u32(code, pc);
                }
                // struct.get/get_s/get_u/set (tipo, campo), array.new_fixed (tipo, tamanho), array.copy (dois tipos).
                2..=5 | 8..=10 | 17..=19 => {
                    read_u32(code, pc);
                    read_u32(code, pc);
                }
                // br_on_cast e br_on_cast_fail: flags, rótulo, tipo heap de origem e de destino.
                24 | 25 => {
                    *pc += 1;
                    read_u32(code, pc);
                    read_s64(code, pc);
                    read_s64(code, pc);
                }
                _ => return Err(unsupported(&format!("instruction 0xfb {}", sub))),
            }
        }
        0xfc => {
            let sub = read_u32(code, pc);
            match sub {
                0..=7 => {}
                8 | 12 | 14 | 10 => {
                    read_u32(code, pc);
                    read_u32(code, pc);
                }
                9 | 11 | 13 | 15 | 16 | 17 => {
                    read_u32(code, pc);
                }
                _ => return Err(unsupported(&format!("instruction 0xfc {}", sub))),
            }
        }
        _ => return Err(unsupported(&format!("instruction {:#04x}", op))),
    }
    Ok(())
}

/// Lê os locais declarados e varre o corpo para o mapa de saltos.
fn compile_function(info: &ModuleInformation, function_index: usize, code_index: usize) -> Result<CompiledFunction, WasmError> {
    let code = info.functions[code_index].data.clone();
    let StructuralType::Function { arguments: parameter_types, returns: return_types } =
        &info.rtt_from_function_index_space(function_index).structural
    else {
        unreachable!("função com tipo que não é de função");
    };
    let param_slots = slot_count(parameter_types);
    let return_slots = slot_count(return_types);
    let mut local_wide: Vec<bool> = parameter_types.iter().map(|ty| ty.is_v128()).collect();
    let mut pc = 0usize;
    let mut extra_locals = Vec::new();
    let groups = read_u32(&code, &mut pc);
    let mut total_locals = 0u64;
    for _ in 0..groups {
        let count = read_u32(&code, &mut pc);
        total_locals += u64::from(count);
        if total_locals > MAX_FUNCTION_LOCALS as u64 {
            return Err(WasmError::Runtime(format!(
                "Function's number of locals is too big {} maximum {}",
                total_locals, MAX_FUNCTION_LOCALS
            )));
        }
        let first = code[pc];
        pc += 1;
        let value = match first {
            0x7f | 0x7e | 0x7d | 0x7c | 0x7b => 0,
            0x63 | 0x64 => {
                read_s64(&code, &mut pc);
                null_ref()
            }
            _ => null_ref(),
        };
        let wide = first == 0x7b;
        local_wide.extend(std::iter::repeat(wide).take(count as usize));
        extra_locals.extend(std::iter::repeat(value).take(count as usize * if wide { 2 } else { 1 }));
    }
    let mut local_offsets = Vec::with_capacity(local_wide.len());
    let mut next_slot = 0usize;
    for wide in &local_wide {
        local_offsets.push(next_slot);
        next_slot += if *wide { 2 } else { 1 };
    }
    let code_start = pc;
    // A pilha de tipos do validador diz quais `drop` e `select` têm um `v128` no topo (dois slots), em vez
    // de adivinhar pela instrução anterior. As chaves são o deslocamento depois da instrução.
    let (_, wide_operands_at) = validate_function_with_wide_operands(
        info,
        &code,
        info.type_signature_index_from_function_index_space(function_index),
    )
    .map_err(WasmError::Runtime)?;
    let mut wide_operands: Vec<(usize, ())> = Vec::new();

    let mut jumps: Vec<(usize, (Option<usize>, usize))> = Vec::new();
    let mut catches: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut delegates: Vec<(usize, u32)> = Vec::new();
    // (posição do opcode, posição depois do `else`)
    let mut open: Vec<(usize, Option<usize>)> = Vec::new();
    while pc < code.len() {
        let op_pc = pc;
        let op = code[pc];
        pc += 1;
        match op {
            0x02..=0x04 => {
                read_block_type(&code, &mut pc);
                open.push((op_pc, None));
            }
            // `try` (legado) abre um bloco, como `block`.
            0x06 => {
                read_block_type(&code, &mut pc);
                open.push((op_pc, None));
            }
            // `try_table`: o tipo de bloco e a lista de catches (tipo, tag se for catch/catch_ref, rótulo).
            0x1f => {
                read_block_type(&code, &mut pc);
                let count = read_u32(&code, &mut pc);
                for _ in 0..count {
                    let kind = code[pc];
                    pc += 1;
                    if kind < 2 {
                        read_u32(&code, &mut pc);
                    }
                    read_u32(&code, &mut pc);
                }
                open.push((op_pc, None));
            }
            // `delegate` fecha o `try` como um `end`.
            0x18 => {
                let depth = read_u32(&code, &mut pc);
                if let Some((start, else_after)) = open.pop() {
                    jumps.push((start, (else_after, pc)));
                    delegates.push((start, depth));
                }
            }
            // `catch` e `catch_all`: cláusulas do `try` aberto mais interno.
            0x07 | 0x19 => {
                skip_immediates(&code, &mut pc, op)?;
                let (start, _) = *open.last().expect("catch sem try");
                catches.entry(start).or_default().push(op_pc);
            }
            0x05 => {
                open.last_mut().expect("else sem bloco").1 = Some(pc);
            }
            0x0b => match open.pop() {
                Some((start, else_after)) => {
                    jumps.push((start, (else_after, pc)));
                }
                None => break,
            },
            _ => {
                skip_immediates(&code, &mut pc, op)?;
                if matches!(op, 0x1a..=0x1c) && wide_operands_at.contains(&pc) {
                    wide_operands.push((op_pc, ()));
                }
            }
        }
    }
    Ok(CompiledFunction {
        param_slots,
        return_slots,
        code,
        code_start,
        extra_locals,
        local_offsets,
        local_wide,
        wide_operands: PcMap::new(wide_operands),
        jumps: PcMap::new(jumps),
        catches: PcMap::new(catches.into_iter().collect()),
        delegates: PcMap::new(delegates),
    })
}

#[derive(Clone, Copy)]
struct Label {
    /// O primeiro byte do corpo de um `loop`, para onde um desvio volta.
    start: usize,
    /// A posição depois do `end`.
    end_after: usize,
    /// Quantos valores um desvio leva (resultados, ou parâmetros num `loop`).
    arity: usize,
    /// A altura da pilha sem os parâmetros do bloco.
    height: usize,
    is_loop: bool,
}

/// O estado de uma invocação de função Wasm: tudo o que `run_frame` precisa para suspender numa chamada e
/// continuar depois (a base da pilha explícita de quadros do JSPI).
struct Frame {
    function_index: u32,
    function: Rc<CompiledFunction>,
    pc: usize,
    locals: Vec<u64>,
    stack: Vec<u64>,
    labels: Vec<Label>,
    handlers: Vec<Handler>,
    /// (índice do rótulo do `catch` em curso, exceção capturada), para `rethrow`.
    caught: Vec<(usize, WasmError)>,
    /// A chamada pendente é `return_call`/`return_call_indirect`: ao voltar, a função sai pelo rótulo mais externo.
    tail_call: bool,
}

/// Os quadros de uma execução suspensa numa importação `WebAssembly.Suspending` (o `Suspender` do JSPI). A
/// profundidade de chamadas guardada é o número de quadros: `resume` a soma de novo ao `call_depth` atual.
pub struct Suspender {
    frames: Vec<Frame>,
    /// A suspensão aconteceu dentro de uma chamada a função de OUTRA instância (`CallOther`): o `Suspender` dela
    /// e a instância dona. `resume` retoma o mais interno primeiro e depois continua estes quadros.
    inner: Option<(Rc<Instance>, Box<Suspender>)>,
}

/// Uma suspensão que atravessou uma chamada direta a função exportada de OUTRA instância (importação que é um
/// export wasm): vai dentro do `WasmError::Suspend` que a importação devolve, e `run_frames` a desembrulha em
/// `Suspender::inner`. Dentro de um `promising`, a pilha de JSPI cobre as duas instâncias.
pub struct NestedSuspension {
    pub owner: Rc<Instance>,
    pub suspender: std::cell::RefCell<Option<Suspender>>,
    pub request: crate::wasm::wasm_instance::JsThrown,
}

/// Como uma execução retomável terminou.
pub enum Completion {
    Done(Vec<u64>),
    /// Suspensa; o `JsThrown` é o pedido de suspensão da importação (a promessa e os tipos de retorno).
    Suspended(Suspender, crate::wasm::wasm_instance::JsThrown),
}

/// Como `run_frame` reentra no quadro.
enum Resume {
    Start,
    /// A chamada pedida terminou: empilha estes resultados.
    Results(Vec<u64>),
    /// Como `Results`, mas os resultados já foram empilhados no quadro (chamada wasm para wasm, sem vetor).
    Pushed,
    /// A chamada pedida lançou: o quadro procura um tratador.
    Raise(WasmError),
}

/// Por que `run_frame` parou.
enum Outcome {
    /// A função terminou: os resultados são o topo da pilha do quadro (`return_slots` slots).
    Return,
    /// Chamada wasm ou importação: os argumentos ficam no topo da pilha do quadro, `run_frames` os consome.
    Call { callee: u32 },
    /// `call_indirect` de uma função de OUTRA instância: roda na instância dona e devolve os resultados.
    CallOther { owner: Rc<Instance>, callee: u32, arguments: Vec<u64> },
    Trap(WasmError),
}

impl Frame {
    fn new(function_index: u32, function: Rc<CompiledFunction>, arguments: &[u64]) -> Frame {
        let mut locals: Vec<u64> = Vec::with_capacity(arguments.len() + function.extra_locals.len());
        locals.extend_from_slice(arguments);
        locals.extend_from_slice(&function.extra_locals);
        let labels = vec![Label { start: 0, end_after: function.code.len(), arity: function.return_slots, height: 0, is_loop: false }];
        let pc = function.code_start;
        Frame {
            function_index,
            function,
            pc,
            locals,
            stack: Vec::with_capacity(32),
            labels,
            handlers: Vec::new(),
            caught: Vec::new(),
            tail_call: false,
        }
    }

    /// Como `new`, mas aproveita os vetores de `old`, o quadro que a chamada em cauda acabou de tirar: um laço de
    /// `return_call` não aloca quatro vetores por volta (o C++ reaproveita o mesmo trecho da pilha).
    fn recycle(mut old: Frame, function_index: u32, function: Rc<CompiledFunction>, arguments: &[u64]) -> Frame {
        old.locals.clear();
        old.locals.extend_from_slice(arguments);
        Frame::reinit(old, function_index, function)
    }

    /// `recycle` para a chamada em cauda: os argumentos estão no topo da pilha do próprio `old`, e vão direto
    /// para os locais, sem vetor intermediário.
    fn recycle_tail(mut old: Frame, function_index: u32, function: Rc<CompiledFunction>) -> Frame {
        let first = old.stack.len() - function.param_slots;
        old.locals.clear();
        old.locals.extend_from_slice(&old.stack[first..]);
        Frame::reinit(old, function_index, function)
    }

    /// Termina o `recycle`: os locais já têm os argumentos; completa-os e zera o resto do estado.
    fn reinit(mut old: Frame, function_index: u32, function: Rc<CompiledFunction>) -> Frame {
        old.locals.extend_from_slice(&function.extra_locals);
        old.stack.clear();
        old.labels.clear();
        old.labels.push(Label { start: 0, end_after: function.code.len(), arity: function.return_slots, height: 0, is_loop: false });
        old.handlers.clear();
        old.caught.clear();
        old.pc = function.code_start;
        old.function_index = function_index;
        old.function = function;
        old.tail_call = false;
        old
    }

    /// Guarda o estado do laço ao suspender numa chamada.
    #[allow(clippy::too_many_arguments)]
    fn save(
        &mut self,
        locals: Vec<u64>,
        stack: Vec<u64>,
        labels: Vec<Label>,
        pc: usize,
        handlers: Vec<Handler>,
        caught: Vec<(usize, WasmError)>,
        tail_call: bool,
    ) {
        self.locals = locals;
        self.stack = stack;
        self.labels = labels;
        self.pc = pc;
        self.handlers = handlers;
        self.caught = caught;
        self.tail_call = tail_call;
    }
}

fn fmin32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { a } else { b }
    } else {
        a.min(b)
    }
}

fn fmax32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { b } else { a }
    } else {
        a.max(b)
    }
}

fn fmin64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { a } else { b }
    } else {
        a.min(b)
    }
}

fn fmax64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { b } else { a }
    } else {
        a.max(b)
    }
}

/// A parte inteira de `x`, ou o trap `OutOfBoundsTrunc` (NaN ou fora de `[lo, hi)`).
fn trunc_checked(x: f64, lo: f64, hi: f64) -> Result<f64, WasmError> {
    if x.is_nan() {
        return Err(trap(ExceptionType::OutOfBoundsTrunc));
    }
    let truncated = x.trunc();
    if truncated < lo || truncated >= hi {
        return Err(trap(ExceptionType::OutOfBoundsTrunc));
    }
    Ok(truncated)
}

impl Instance {

    fn compiled_function(&self, function_index: u32) -> Result<Rc<CompiledFunction>, WasmError> {
        if let Some(compiled) = &self.compiled.borrow()[function_index as usize] {
            return Ok(compiled.clone());
        }
        let code_index = self.info.to_code_index(function_index as usize);
        let compiled = Rc::new(compile_function(&self.info, function_index as usize, code_index)?);
        self.compiled.borrow_mut()[function_index as usize] = Some(compiled.clone());
        Ok(compiled)
    }

    /// Chama a função `function_index` (do espaço de funções) com os argumentos em bits.
    pub fn invoke(&self, function_index: u32, arguments: &[u64]) -> Result<Vec<u64>, WasmError> {
        let import_count = self.host_functions.len();
        if (function_index as usize) < import_count {
            // `v128` não cruza a fronteira com o JS: o `WasmToJS` do JSC lança antes de chamar o host.
            if let StructuralType::Function { arguments, returns } =
                &self.info.rtt_from_function_index_space(function_index as usize).structural
            {
                if arguments.iter().chain(returns).any(|ty| ty.is_v128()) {
                    return Err(WasmError::Type(
                        error_message_for_exception_type(ExceptionType::TypeErrorInvalidValueUse).to_string(),
                    ));
                }
            }
            let results = (self.host_functions[function_index as usize])(arguments)?;
            return self.coerce_host_results(function_index, results);
        }
        if self.call_depth.get() >= MAX_CALL_DEPTH {
            return Err(trap(ExceptionType::StackOverflow));
        }
        self.call_depth.set(self.call_depth.get() + 1);
        let result = self.compiled_function(function_index).and_then(|compiled| self.run(function_index, compiled, arguments));
        self.call_depth.set(self.call_depth.get() - 1);
        result
    }

    /// Coage os resultados de uma função importada à assinatura do tipo, como a camada de import
    /// (`WasmToJS`) faz: contagem diferente é `TypeError` (o iterável de retorno precisa ter
    /// exatamente a aridade da assinatura) e valores de 32 bits perdem os bits altos.
    fn coerce_host_results(&self, function_index: u32, mut results: Vec<u64>) -> Result<Vec<u64>, WasmError> {
        let StructuralType::Function { returns, .. } =
            &self.info.rtt_from_function_index_space(function_index as usize).structural
        else {
            unreachable!("função com tipo que não é de função");
        };
        if results.len() != returns.len() {
            return Err(WasmError::Runtime(format!(
                "TypeError: multi-return length mismatch: expected {} results, got {}",
                returns.len(),
                results.len()
            )));
        }
        for (value, kind) in results.iter_mut().zip(returns.iter()) {
            if matches!(kind.kind, TypeKind::I32 | TypeKind::F32) {
                *value = u64::from(*value as u32);
            }
        }
        Ok(results)
    }

    /// Tira da pilha os argumentos de uma chamada a `function_index`.
    fn take_call_arguments(&self, function_index: u32, stack: &mut Vec<u64>, owner_info: &ModuleInformation) -> Vec<u64> {
        let StructuralType::Function { arguments, .. } =
            &owner_info.rtt_from_function_index_space(function_index as usize).structural
        else {
            unreachable!("função com tipo que não é de função");
        };
        let first = stack.len() - slot_count(arguments);
        stack.split_off(first)
    }

    fn block_arity(&self, block_type: BlockType) -> (usize, usize) {
        match block_type {
            BlockType::Empty => (0, 0),
            BlockType::Value => (0, 1),
            BlockType::V128 => (0, 2),
            BlockType::Index(position) => match &self.info.rtt(position as usize).structural {
                StructuralType::Function { arguments, returns } => (slot_count(arguments), slot_count(returns)),
                _ => unreachable!("tipo de bloco que não é de função"),
            },
        }
    }

    fn table_index_of(&self, value: u64, table_index: usize) -> u64 {
        if self.tables[table_index].borrow().address_type().is_64_bit() { value } else { u64::from(value as u32) }
    }

    /// Executa a função `function_index` até o fim. `call_depth` já conta o quadro de entrada (`invoke` o
    /// incrementou); os quadros empilhados por `run_frames` saem da conta ao terminar, qualquer que seja o motivo.
    fn run(&self, function_index: u32, function: Rc<CompiledFunction>, arguments: &[u64]) -> Result<Vec<u64>, WasmError> {
        let base_depth = self.call_depth.get();
        let frames = vec![Frame::new(function_index, function, arguments)];
        let result = self.run_frames(frames, Resume::Start, false);
        self.call_depth.set(base_depth);
        match result? {
            Completion::Done(values) => Ok(values),
            Completion::Suspended(..) => unreachable!("suspensão num laço que não é retomável"),
        }
    }

    /// Como `invoke`, mas a importação `WebAssembly.Suspending` que devolve promessa suspende a execução: o laço
    /// entrega os quadros guardados num `Suspender` em vez de desenrolar (a entrada de `WebAssembly.promising`).
    pub fn invoke_resumable(&self, function_index: u32, arguments: &[u64]) -> Result<Completion, WasmError> {
        if (function_index as usize) < self.host_functions.len() {
            return self.invoke(function_index, arguments).map(Completion::Done);
        }
        if self.call_depth.get() >= MAX_CALL_DEPTH {
            return Err(trap(ExceptionType::StackOverflow));
        }
        let base_depth = self.call_depth.get();
        self.call_depth.set(base_depth + 1);
        let result = self
            .compiled_function(function_index)
            .and_then(|compiled| self.run_frames(vec![Frame::new(function_index, compiled, arguments)], Resume::Start, true));
        self.call_depth.set(base_depth);
        result
    }

    /// Reentra nos quadros suspensos de `suspender` com o resultado da importação que suspendeu (os valores já na
    /// representação do tipo de retorno da importação, ou o erro a lançar no ponto da chamada).
    pub fn resume(&self, suspender: Suspender, result: Result<Vec<u64>, WasmError>) -> Result<Completion, WasmError> {
        let base_depth = self.call_depth.get();
        self.call_depth.set(base_depth + suspender.frames.len() as u32);
        let Suspender { frames, inner } = suspender;
        let result = match inner {
            Some((owner, inner)) => match owner.resume(*inner, result) {
                Ok(Completion::Done(values)) => Ok(values),
                Ok(Completion::Suspended(again, request)) => {
                    self.call_depth.set(base_depth);
                    return Ok(Completion::Suspended(Suspender { frames, inner: Some((owner, Box::new(again))) }, request));
                }
                Err(error) => Err(error),
            },
            None => result,
        };
        let resume = match result {
            Ok(values) => Resume::Results(values),
            Err(error) => Resume::Raise(error),
        };
        let outcome = self.run_frames(frames, resume, true);
        self.call_depth.set(base_depth);
        outcome
    }

    /// O laço de quadros explícito: cada `call` wasm para wasm devolvida por `run_frame` empilha um `Frame` em vez de
    /// recursar na pilha do Rust, e o resultado (ou a exceção) volta ao quadro de baixo para ele continuar. Só a
    /// chamada a uma importação do host passa por `invoke`. A profundidade (`MAX_CALL_DEPTH`) conta os quadros.
    /// Com `resumable`, uma importação que pede suspensão (`WasmError::Suspend`) faz o laço devolver
    /// `Completion::Suspended` com os quadros, sem desempilhar nada; `resume` reentra aqui.
    fn run_frames(&self, mut frames: Vec<Frame>, first: Resume, resumable: bool) -> Result<Completion, WasmError> {
        let mut resume = first;
        // A pilha de `Error.stack` espelha `frames`: uma entrada por quadro (ver `wasm_call_stack`).
        let _call_stack = crate::wasm::wasm_call_stack::enter_frames(frames.len());
        // Quadros já terminados, para a próxima chamada reaproveitar os vetores em vez de alocar.
        let mut spare: Vec<Frame> = Vec::new();
        loop {
            let Some(frame) = frames.last_mut() else {
                unreachable!("pilha de quadros vazia");
            };
            match self.run_frame(frame, resume) {
                Outcome::Return => {
                    let Some(mut done) = frames.pop() else {
                        unreachable!("pilha de quadros vazia");
                    };
                    crate::wasm::wasm_call_stack::pop_frame();
                    let count = done.function.return_slots;
                    let first = done.stack.len() - count;
                    let Some(caller) = frames.last_mut() else {
                        return Ok(Completion::Done(done.stack.split_off(first)));
                    };
                    self.call_depth.set(self.call_depth.get() - 1);
                    caller.stack.extend_from_slice(&done.stack[first..]);
                    if spare.len() < SPARE_FRAMES {
                        spare.push(done);
                    }
                    resume = Resume::Pushed;
                }
                Outcome::Trap(error) => {
                    if matches!(error, WasmError::Runtime(_)) {
                        // O `RuntimeError` nasce depois do pop; a pilha do trap fica guardada para ele.
                        crate::wasm::wasm_call_stack::capture_trap_stack();
                    }
                    frames.pop();
                    crate::wasm::wasm_call_stack::pop_frame();
                    if frames.is_empty() {
                        return Err(error);
                    }
                    self.call_depth.set(self.call_depth.get() - 1);
                    resume = Resume::Raise(error);
                }
                Outcome::CallOther { owner, callee, arguments } => {
                    // Dentro de um `promising()` a pilha de JSPI atravessa a chamada entre instâncias: uma
                    // suspensão da instância dona suspende também esta.
                    if resumable {
                        resume = match owner.invoke_resumable(callee, &arguments) {
                            Ok(Completion::Done(results)) => Resume::Results(results),
                            Ok(Completion::Suspended(inner, request)) => {
                                return Ok(Completion::Suspended(Suspender { frames, inner: Some((owner, Box::new(inner))) }, request));
                            }
                            Err(error) => Resume::Raise(error),
                        };
                    } else {
                        resume = match owner.invoke(callee, &arguments) {
                            Ok(results) => Resume::Results(results),
                            Err(error) => Resume::Raise(error),
                        };
                    }
                }
                Outcome::Call { callee } => {
                    if (callee as usize) < self.host_functions.len() {
                        let Some(caller) = frames.last_mut() else {
                            unreachable!("pilha de quadros vazia");
                        };
                        let arguments = self.take_call_arguments(callee, &mut caller.stack, &self.info);
                        resume = match self.invoke(callee, &arguments) {
                            Ok(results) => Resume::Results(results),
                            Err(WasmError::Suspend(request)) if resumable => {
                                if let Some(nested) = request.0.downcast_ref::<NestedSuspension>() {
                                    let inner = nested.suspender.borrow_mut().take().map(|inner| (nested.owner.clone(), Box::new(inner)));
                                    return Ok(Completion::Suspended(Suspender { frames, inner }, nested.request.clone()));
                                }
                                return Ok(Completion::Suspended(Suspender { frames, inner: None }, request));
                            }
                            Err(WasmError::Suspend(_)) => {
                                Resume::Raise(WasmError::Runtime("suspension outside of a promising() context".to_string()))
                            }
                            Err(error) => Resume::Raise(error),
                        };
                    } else if self.call_depth.get() >= MAX_CALL_DEPTH && !frames.last().is_some_and(|caller| caller.tail_call) {
                        if let Some(caller) = frames.last_mut() {
                            drop(self.take_call_arguments(callee, &mut caller.stack, &self.info));
                        }
                        resume = Resume::Raise(trap(ExceptionType::StackOverflow));
                    } else {
                        // `return_call`/`return_call_indirect` para wasm: o quadro do chamador sai antes do
                        // novo entrar, e o resultado volta direto ao quadro de baixo (profundidade constante).
                        // Os argumentos passam da pilha do chamador direto aos locais do novo quadro.
                        let tail = frames.last().is_some_and(|caller| caller.tail_call);
                        let recycled = if tail {
                            self.call_depth.set(self.call_depth.get() - 1);
                            crate::wasm::wasm_call_stack::pop_frame();
                            frames.pop()
                        } else {
                            None
                        };
                        match self.compiled_function(callee) {
                            Ok(compiled) => {
                                self.call_depth.set(self.call_depth.get() + 1);
                                let frame = match recycled {
                                    Some(old) => Frame::recycle_tail(old, callee, compiled),
                                    None => {
                                        let Some(caller) = frames.last_mut() else {
                                            unreachable!("pilha de quadros vazia");
                                        };
                                        let first = caller.stack.len() - compiled.param_slots;
                                        let frame = match spare.pop() {
                                            Some(old) => Frame::recycle(old, callee, compiled, &caller.stack[first..]),
                                            None => Frame::new(callee, compiled, &caller.stack[first..]),
                                        };
                                        caller.stack.truncate(first);
                                        frame
                                    }
                                };
                                frames.push(frame);
                                crate::wasm::wasm_call_stack::push_frame();
                                resume = Resume::Start;
                            }
                            Err(error) => {
                                if !tail {
                                    if let Some(caller) = frames.last_mut() {
                                        drop(self.take_call_arguments(callee, &mut caller.stack, &self.info));
                                    }
                                }
                                if frames.is_empty() {
                                    return Err(error);
                                }
                                resume = Resume::Raise(error);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Roda o quadro até ele terminar, falhar ou pedir uma chamada; neste caso o estado fica guardado em `frame`.
    fn run_frame(&self, frame: &mut Frame, resume: Resume) -> Outcome {
        match self.step_frame(frame, resume) {
            Ok(outcome) => outcome,
            Err(error) => Outcome::Trap(error),
        }
    }

    fn step_frame(&self, frame: &mut Frame, resume: Resume) -> Result<Outcome, WasmError> {
        let info = &*self.info;
        let function_rc = frame.function.clone();
        let function: &CompiledFunction = &function_rc;
        let code = &function.code;
        let mut locals: Vec<u64> = std::mem::take(&mut frame.locals);
        let mut stack: Vec<u64> = std::mem::take(&mut frame.stack);
        let mut labels: Vec<Label> = std::mem::take(&mut frame.labels);
        let mut pc = frame.pc;
        let mut handlers: Vec<Handler> = std::mem::take(&mut frame.handlers);
        // (índice do rótulo do `catch` em curso, exceção capturada), para `rethrow`.
        let mut caught: Vec<(usize, WasmError)> = std::mem::take(&mut frame.caught);

        macro_rules! pop {
            () => {
                stack.pop().expect("pilha vazia")
            };
        }
        // Descarta tratadores e capturas cujo rótulo já saiu de `labels`. Sem tratador nem captura (o caso comum
        // de um laço sem `try`) não há o que sincronizar, e o desvio sai sem tocar nos dois vetores.
        macro_rules! sync_handlers {
            () => {{
                if !handlers.is_empty() {
                    while handlers.last().is_some_and(|handler| handler.label_index >= labels.len()) {
                        handlers.pop();
                    }
                }
                if !caught.is_empty() {
                    caught.retain(|(index, _)| *index < labels.len());
                }
            }};
        }
        macro_rules! branch {
            ($depth:expr) => {{
                let index = labels.len() - 1 - $depth as usize;
                let label = labels[index];
                let top = stack.len() - label.arity;
                if label.height != top {
                    drop(stack.drain(label.height..top));
                }
                if label.is_loop {
                    labels.truncate(index + 1);
                    pc = label.start;
                } else {
                    labels.truncate(index);
                    pc = label.end_after;
                }
                sync_handlers!();
            }};
        }
        // Lança `$error`: uma exceção wasm procura um tratador (`catch`, `catch_all`, `delegate`, `try_table`)
        // e continua o laço no corpo dele; qualquer outro erro, ou sem tratador, sai de `run`.
        macro_rules! raise {
            ($error:expr) => {{
                let error: WasmError = $error;
                // `JsException` não tem tag nem payload: só `catch_all` e `catch_all_ref` a capturam.
                let (thrown_tag, payload): (Option<&TagRef>, &[u64]) = match &error {
                    WasmError::Exception { tag, payload } => (Some(tag), payload.as_slice()),
                    WasmError::JsException(_) => (None, &[]),
                    _ => return Err(error),
                };
                loop {
                    let Some(handler) = handlers.pop() else {
                        return Err(error);
                    };
                    match handler.kind {
                        HandlerKind::Delegate { depth } => {
                            // O alvo é um rótulo fora do `try`; o tratador dele (se for um `try`) ainda vale.
                            let target = handler.label_index - 1 - depth as usize;
                            labels.truncate(target + 1);
                            sync_handlers!();
                        }
                        HandlerKind::Legacy { try_pc } => {
                            let mut found = None;
                            for &clause_pc in function.catches.get(&try_pc).map(Vec::as_slice).unwrap_or(&[]) {
                                if code[clause_pc] == 0x19 {
                                    found = Some((clause_pc, clause_pc + 1));
                                    break;
                                }
                                let mut after = clause_pc + 1;
                                let tag_index = read_u32(code, &mut after) as usize;
                                if thrown_tag.is_some_and(|thrown| Rc::ptr_eq(&self.tags[tag_index], &thrown.0)) {
                                    found = Some((clause_pc, after));
                                    break;
                                }
                            }
                            let Some((clause_pc, after)) = found else {
                                continue;
                            };
                            labels.truncate(handler.label_index + 1);
                            sync_handlers!();
                            stack.truncate(labels[handler.label_index].height);
                            if code[clause_pc] == 0x07 {
                                stack.extend_from_slice(payload);
                            }
                            caught.push((handler.label_index, error.clone()));
                            pc = after;
                            break;
                        }
                        HandlerKind::TryTable { clauses } => {
                            let mut selected = None;
                            for &(kind, tag_index, depth) in &clauses {
                                if kind >= 2 || thrown_tag.is_some_and(|thrown| Rc::ptr_eq(&self.tags[tag_index as usize], &thrown.0)) {
                                    selected = Some((kind, depth));
                                    break;
                                }
                            }
                            let Some((kind, depth)) = selected else {
                                continue;
                            };
                            let height = labels[handler.label_index].height;
                            labels.truncate(handler.label_index);
                            sync_handlers!();
                            stack.truncate(height);
                            if kind == 0 || kind == 1 {
                                stack.extend_from_slice(payload);
                            }
                            if kind == 1 || kind == 3 {
                                let mut exnrefs = self.exnrefs.borrow_mut();
                                stack.push(EXNREF_TAG | exnrefs.len() as u64);
                                exnrefs.push(error.clone());
                            }
                            branch!(depth);
                            break;
                        }
                    }
                }
                // O laço de cima só sai quando um tratador assumiu: segue no corpo dele.
                continue;
            }};
        }
        macro_rules! bin32 {
            ($a:ident, $b:ident => $e:expr) => {{
                let $b = pop!() as u32;
                let $a = pop!() as u32;
                stack.push(u64::from($e));
            }};
        }
        macro_rules! bin64 {
            ($a:ident, $b:ident => $e:expr) => {{
                let $b = pop!();
                let $a = pop!();
                stack.push(u64::from($e));
            }};
        }
        macro_rules! un32 {
            ($a:ident => $e:expr) => {{
                let $a = pop!() as u32;
                stack.push(u64::from($e));
            }};
        }
        macro_rules! un64 {
            ($a:ident => $e:expr) => {{
                let $a = pop!();
                stack.push(u64::from($e));
            }};
        }
        macro_rules! binf32 {
            ($a:ident, $b:ident => $e:expr) => {{
                let $b = f32::from_bits(pop!() as u32);
                let $a = f32::from_bits(pop!() as u32);
                let result: f32 = $e;
                stack.push(u64::from(result.to_bits()));
            }};
        }
        macro_rules! cmpf32 {
            ($a:ident, $b:ident => $e:expr) => {{
                let $b = f32::from_bits(pop!() as u32);
                let $a = f32::from_bits(pop!() as u32);
                stack.push(u64::from($e));
            }};
        }
        macro_rules! unf32 {
            ($a:ident => $e:expr) => {{
                let $a = f32::from_bits(pop!() as u32);
                let result: f32 = $e;
                stack.push(u64::from(result.to_bits()));
            }};
        }
        macro_rules! binf64 {
            ($a:ident, $b:ident => $e:expr) => {{
                let $b = f64::from_bits(pop!());
                let $a = f64::from_bits(pop!());
                let result: f64 = $e;
                stack.push(result.to_bits());
            }};
        }
        macro_rules! cmpf64 {
            ($a:ident, $b:ident => $e:expr) => {{
                let $b = f64::from_bits(pop!());
                let $a = f64::from_bits(pop!());
                stack.push(u64::from($e));
            }};
        }
        macro_rules! unf64 {
            ($a:ident => $e:expr) => {{
                let $a = f64::from_bits(pop!());
                let result: f64 = $e;
                stack.push(result.to_bits());
            }};
        }
        macro_rules! memarg {
            () => {{
                let align = read_u32(code, &mut pc);
                let memory_index = if align & 0x40 != 0 { read_u32(code, &mut pc) as usize } else { 0 };
                let offset = read_u64(code, &mut pc);
                (memory_index, offset)
            }};
        }

        // Retoma depois de uma chamada: empilha os resultados (e, em `return_call`, desvia para a saída da
        // função) ou lança a exceção da chamada no quadro, como o laço fazia quando `call` recursava.
        let tail_call = std::mem::take(&mut frame.tail_call);
        let mut pending_raise: Option<WasmError> = None;
        match resume {
            Resume::Start => {}
            Resume::Results(results) => {
                stack.extend(results);
                if tail_call {
                    let depth = labels.len() - 1;
                    branch!(depth);
                }
            }
            Resume::Pushed => {
                if tail_call {
                    let depth = labels.len() - 1;
                    branch!(depth);
                }
            }
            Resume::Raise(error) => pending_raise = Some(error),
        }

        while pc < code.len() {
            // O `raise!` termina em `continue`, então a exceção da chamada entra aqui, dentro do laço.
            if let Some(error) = pending_raise.take() {
                raise!(error);
            }
            let op_pc = pc;
            let op = code[pc];
            pc += 1;
            match op {
                0x00 => return Err(trap(ExceptionType::Unreachable)),
                0x01 => {}
                0x02 => {
                    let (parameters, results) = self.block_arity(read_block_type(code, &mut pc));
                    labels.push(Label {
                        start: pc,
                        end_after: function.jumps[&op_pc].1,
                        arity: results,
                        height: stack.len() - parameters,
                        is_loop: false,
                    });
                }
                0x03 => {
                    let (parameters, results) = self.block_arity(read_block_type(code, &mut pc));
                    labels.push(Label {
                        start: pc,
                        end_after: function.jumps[&op_pc].1,
                        arity: parameters,
                        height: stack.len() - parameters,
                        is_loop: true,
                    });
                    let _ = results;
                }
                0x04 => {
                    let (parameters, results) = self.block_arity(read_block_type(code, &mut pc));
                    let condition = pop!() as u32;
                    let (else_after, end_after) = function.jumps[&op_pc];
                    let label = Label {
                        start: pc,
                        end_after,
                        arity: results,
                        height: stack.len() - parameters,
                        is_loop: false,
                    };
                    if condition != 0 {
                        labels.push(label);
                    } else if let Some(else_pc) = else_after {
                        labels.push(label);
                        pc = else_pc;
                    } else {
                        // Sem `else`, os parâmetros passam como resultados.
                        pc = end_after;
                    }
                }
                0x05 => {
                    // O ramo verdadeiro terminou: salta para depois do `end`.
                    let label = labels.pop().expect("else sem rótulo");
                    pc = label.end_after;
                    sync_handlers!();
                }
                0x0b => {
                    labels.pop();
                    sync_handlers!();
                }
                // `try` legado: um bloco com um tratador (`delegate` ou `catch`/`catch_all`).
                0x06 => {
                    let (parameters, results) = self.block_arity(read_block_type(code, &mut pc));
                    labels.push(Label {
                        start: pc,
                        end_after: function.jumps[&op_pc].1,
                        arity: results,
                        height: stack.len() - parameters,
                        is_loop: false,
                    });
                    let kind = match function.delegates.get(&op_pc) {
                        Some(&depth) => HandlerKind::Delegate { depth },
                        None => HandlerKind::Legacy { try_pc: op_pc },
                    };
                    handlers.push(Handler { label_index: labels.len() - 1, kind });
                }
                // `catch` e `catch_all` alcançados sem exceção: o corpo do `try` terminou, salta para o `end`.
                0x07 | 0x19 => {
                    if op == 0x07 {
                        read_u32(code, &mut pc);
                    }
                    let label = labels.pop().expect("catch sem rótulo");
                    pc = label.end_after;
                    sync_handlers!();
                }
                // `delegate` alcançado sem exceção: fecha o `try` como um `end`.
                0x18 => {
                    read_u32(code, &mut pc);
                    labels.pop();
                    sync_handlers!();
                }
                // `throw`: o payload sai da pilha segundo os parâmetros da tag.
                0x08 => {
                    let tag = self.tags[read_u32(code, &mut pc) as usize].clone();
                    let count = tag.borrow().parameters.len();
                    let payload = stack.split_off(stack.len() - count);
                    raise!(WasmError::Exception { tag: TagRef(tag), payload });
                }
                // `rethrow`: relança a exceção capturada pelo `catch` no rótulo dessa profundidade.
                0x09 => {
                    let depth = read_u32(code, &mut pc) as usize;
                    let target = labels.len() - 1 - depth;
                    let (_, error) = caught.iter().rev().find(|(index, _)| *index == target).expect("rethrow fora de catch");
                    raise!(error.clone());
                }
                // `throw_ref`: relança a exceção de um `exnref`.
                0x0a => {
                    let reference = pop!();
                    if reference & EXNREF_TAG == 0 {
                        return Err(trap(ExceptionType::NullExnrefReference));
                    }
                    let error = self.exnrefs.borrow()[(reference & !EXNREF_TAG) as usize].clone();
                    raise!(error);
                }
                // `try_table`: um bloco com um tratador de cláusulas (`catch`, `catch_ref`, `catch_all`, `catch_all_ref`).
                0x1f => {
                    let (parameters, results) = self.block_arity(read_block_type(code, &mut pc));
                    labels.push(Label {
                        start: pc,
                        end_after: function.jumps[&op_pc].1,
                        arity: results,
                        height: stack.len() - parameters,
                        is_loop: false,
                    });
                    let count = read_u32(code, &mut pc);
                    let mut clauses = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        let kind = code[pc];
                        pc += 1;
                        let tag_index = if kind < 2 { read_u32(code, &mut pc) } else { 0 };
                        let depth = read_u32(code, &mut pc);
                        clauses.push((kind, tag_index, depth));
                    }
                    handlers.push(Handler { label_index: labels.len() - 1, kind: HandlerKind::TryTable { clauses } });
                }
                0x0c => {
                    let depth = read_u32(code, &mut pc);
                    branch!(depth);
                }
                0x0d => {
                    let depth = read_u32(code, &mut pc);
                    if pop!() as u32 != 0 {
                        branch!(depth);
                    }
                }
                0x0e => {
                    let count = read_u32(code, &mut pc);
                    let selector = (pop!() as u32).min(count);
                    let mut chosen = 0;
                    for entry in 0..=count {
                        let target = read_u32(code, &mut pc);
                        if entry == selector {
                            chosen = target;
                        }
                    }
                    branch!(chosen);
                }
                0x0f => {
                    let depth = labels.len() - 1;
                    branch!(depth);
                }
                0x10 | 0x12 => {
                    let callee = read_u32(code, &mut pc);
                    frame.save(locals, stack, labels, pc, handlers, caught, op == 0x12);
                    return Ok(Outcome::Call { callee });
                }
                0x11 | 0x13 => {
                    let type_position = read_u32(code, &mut pc);
                    let table_index = read_u32(code, &mut pc) as usize;
                    let selector = pop!();
                    let entry = {
                        let table = self.tables[table_index].borrow();
                        let selector = self.table_index_of(selector, table_index);
                        if selector >= u64::from(table.length()) {
                            return Err(trap(ExceptionType::OutOfBoundsCallIndirect));
                        }
                        table.get(selector as u32)
                    };
                    // A entrada nula do JSC (`FuncRefTable::Function`) tem o tipo de assinatura nulo, que nunca casa
                    // com o esperado: o `call_indirect` falha pela assinatura, não pelo nulo (medido no bun 1.4.2).
                    let Some((owner, callee)) = func_ref_target(entry) else {
                        return Err(trap(ExceptionType::BadSignature));
                    };
                    // A referência carrega a instância dona (como no C++): a assinatura se compara pelo tipo
                    // canônico, que vale entre módulos, e a chamada roda na dona.
                    let other = if owner == self.id { None } else { instance_of(owner) };
                    let owner_info = match &other {
                        Some(instance) => &*instance.info,
                        None if owner == self.id => info,
                        None => return Err(trap(ExceptionType::BadSignature)),
                    };
                    if callee as usize >= owner_info.function_index_space_size() {
                        return Err(trap(ExceptionType::BadSignature));
                    }
                    let callee_type = owner_info.type_signature_index_from_function_index_space(callee as usize);
                    if owner_info.canonical_type_id(callee_type as usize) != info.canonical_type_id(type_position as usize) {
                        return Err(trap(ExceptionType::BadSignature));
                    }
                    // Na mesma instância os argumentos ficam na pilha: o laço os passa ao novo quadro.
                    let arguments = other.is_some().then(|| self.take_call_arguments(callee, &mut stack, owner_info));
                    let outcome = match (other, arguments) {
                        (Some(owner), Some(arguments)) => Outcome::CallOther { owner, callee, arguments },
                        _ => Outcome::Call { callee },
                    };
                    frame.save(locals, stack, labels, pc, handlers, caught, op == 0x13);
                    return Ok(outcome);
                }
                // call_ref e return_call_ref: a validação já garantiu a assinatura, só falta a referência nula.
                0x14 | 0x15 => {
                    read_u32(code, &mut pc);
                    let reference = pop!();
                    let Some((owner, callee)) = func_ref_target(reference) else {
                        return Err(trap(ExceptionType::NullReference));
                    };
                    let other = if owner == self.id { None } else { instance_of(owner) };
                    let owner_info = match &other {
                        Some(instance) => &*instance.info,
                        None if owner == self.id => info,
                        None => return Err(trap(ExceptionType::BadSignature)),
                    };
                    // Na mesma instância os argumentos ficam na pilha: o laço os passa ao novo quadro.
                    let arguments = other.is_some().then(|| self.take_call_arguments(callee, &mut stack, owner_info));
                    let outcome = match (other, arguments) {
                        (Some(owner), Some(arguments)) => Outcome::CallOther { owner, callee, arguments },
                        _ => Outcome::Call { callee },
                    };
                    frame.save(locals, stack, labels, pc, handlers, caught, op == 0x15);
                    return Ok(outcome);
                }
                0x1a => {
                    pop!();
                    if function.wide_operands.contains(&op_pc) {
                        pop!();
                    }
                }
                0x1b | 0x1c => {
                    if op == 0x1c {
                        let count = read_u32(code, &mut pc);
                        for _ in 0..count {
                            read_block_type(code, &mut pc);
                        }
                    }
                    let wide = function.wide_operands.contains(&op_pc);
                    let condition = pop!() as u32;
                    if wide {
                        let second = stack.split_off(stack.len() - 2);
                        let first = stack.split_off(stack.len() - 2);
                        stack.extend(if condition != 0 { first } else { second });
                    } else {
                        let second = pop!();
                        let first = pop!();
                        stack.push(if condition != 0 { first } else { second });
                    }
                }
                0x20 => {
                    let index = read_u32(code, &mut pc) as usize;
                    let slot = function.local_offsets[index];
                    stack.push(locals[slot]);
                    if function.local_wide[index] {
                        stack.push(locals[slot + 1]);
                    }
                }
                0x21 | 0x22 => {
                    let index = read_u32(code, &mut pc) as usize;
                    let slot = function.local_offsets[index];
                    if function.local_wide[index] {
                        let high = pop!();
                        let low = pop!();
                        locals[slot] = low;
                        locals[slot + 1] = high;
                        if op == 0x22 {
                            stack.push(low);
                            stack.push(high);
                        }
                    } else if op == 0x21 {
                        locals[slot] = pop!();
                    } else {
                        locals[slot] = *stack.last().expect("pilha vazia");
                    }
                }
                0x23 => {
                    let index = read_u32(code, &mut pc) as usize;
                    if info.globals[index].ty.is_v128() {
                        let (low, high) = wasm_simd::split(wasm_simd::from_bytes(self.globals[index].borrow().get_vector()));
                        stack.push(low);
                        stack.push(high);
                    } else {
                        stack.push(self.globals[index].borrow().get());
                    }
                }
                0x24 => {
                    let index = read_u32(code, &mut pc) as usize;
                    if info.globals[index].ty.is_v128() {
                        let high = pop!();
                        let low = pop!();
                        self.globals[index].borrow_mut().set_vector(wasm_simd::to_bytes(wasm_simd::join(low, high)));
                    } else {
                        let value = pop!();
                        self.globals[index].borrow_mut().set(value);
                    }
                }
                0x25 => {
                    let table_index = read_u32(code, &mut pc) as usize;
                    let index = self.table_index_of(pop!(), table_index);
                    let table = self.tables[table_index].borrow();
                    if index >= u64::from(table.length()) {
                        return Err(trap(ExceptionType::OutOfBoundsTableAccess));
                    }
                    stack.push(table.get(index as u32));
                }
                0x26 => {
                    let table_index = read_u32(code, &mut pc) as usize;
                    let value = pop!();
                    let index = self.table_index_of(pop!(), table_index);
                    let mut table = self.tables[table_index].borrow_mut();
                    if index >= u64::from(table.length()) {
                        return Err(trap(ExceptionType::OutOfBoundsTableAccess));
                    }
                    table.set(index as u32, value);
                }
                0x28..=0x35 => {
                    let (memory_index, offset) = memarg!();
                    let address = pop!();
                    let (width, signed, wide) = match op {
                        0x28 | 0x2a => (4usize, false, false),
                        0x29 | 0x2b => (8, false, true),
                        0x2c => (1, true, false),
                        0x2d => (1, false, false),
                        0x2e => (2, true, false),
                        0x2f => (2, false, false),
                        0x30 => (1, true, true),
                        0x31 => (1, false, true),
                        0x32 => (2, true, true),
                        0x33 => (2, false, true),
                        0x34 => (4, true, true),
                        _ => (4, false, true),
                    };
                    let loaded = address
                        .checked_add(offset)
                        .and_then(|effective| self.memories[memory_index].borrow().load(effective, width))
                        .ok_or_else(|| trap(ExceptionType::OutOfBoundsMemoryAccess))?;
                    let value = if signed {
                        let shift = 64 - 8 * width as u32;
                        let extended = (((loaded << shift) as i64) >> shift) as u64;
                        if wide { extended } else { u64::from(extended as u32) }
                    } else {
                        loaded
                    };
                    stack.push(value);
                }
                0x36..=0x3e => {
                    let (memory_index, offset) = memarg!();
                    let value = pop!();
                    let address = pop!();
                    let width = match op {
                        0x36 | 0x38 | 0x3e => 4usize,
                        0x37 | 0x39 => 8,
                        0x3a | 0x3c => 1,
                        _ => 2,
                    };
                    let stored = address
                        .checked_add(offset)
                        .is_some_and(|effective| self.memories[memory_index].borrow_mut().store(effective, width, value));
                    if !stored {
                        return Err(trap(ExceptionType::OutOfBoundsMemoryAccess));
                    }
                }
                0x3f => {
                    let memory_index = read_u32(code, &mut pc) as usize;
                    stack.push(self.memories[memory_index].borrow().page_count().page_count());
                }
                0x40 => {
                    let memory_index = read_u32(code, &mut pc) as usize;
                    let delta = pop!();
                    let memory = &self.memories[memory_index];
                    let is_64_bit = memory.borrow().address_type().is_64_bit();
                    let delta = if is_64_bit { delta } else { u64::from(delta as u32) };
                    let failure = if is_64_bit { u64::MAX } else { u64::from(u32::MAX) };
                    let result = memory.borrow_mut().grow(PageCount::new(delta));
                    stack.push(result.map_or(failure, |old| old.page_count()));
                }
                0x41 => {
                    let value = read_s64(code, &mut pc) as i32;
                    stack.push(u64::from(value as u32));
                }
                0x42 => {
                    let value = read_s64(code, &mut pc);
                    stack.push(value as u64);
                }
                0x43 => {
                    let bits = u32::from_le_bytes(code[pc..pc + 4].try_into().expect("f32.const truncado"));
                    pc += 4;
                    stack.push(u64::from(bits));
                }
                0x44 => {
                    let bits = u64::from_le_bytes(code[pc..pc + 8].try_into().expect("f64.const truncado"));
                    pc += 8;
                    stack.push(bits);
                }
                // i32 comparações
                0x45 => un32!(a => a == 0),
                0x46 => bin32!(a, b => a == b),
                0x47 => bin32!(a, b => a != b),
                0x48 => bin32!(a, b => (a as i32) < (b as i32)),
                0x49 => bin32!(a, b => a < b),
                0x4a => bin32!(a, b => (a as i32) > (b as i32)),
                0x4b => bin32!(a, b => a > b),
                0x4c => bin32!(a, b => (a as i32) <= (b as i32)),
                0x4d => bin32!(a, b => a <= b),
                0x4e => bin32!(a, b => (a as i32) >= (b as i32)),
                0x4f => bin32!(a, b => a >= b),
                // i64 comparações
                0x50 => un64!(a => a == 0),
                0x51 => bin64!(a, b => a == b),
                0x52 => bin64!(a, b => a != b),
                0x53 => bin64!(a, b => (a as i64) < (b as i64)),
                0x54 => bin64!(a, b => a < b),
                0x55 => bin64!(a, b => (a as i64) > (b as i64)),
                0x56 => bin64!(a, b => a > b),
                0x57 => bin64!(a, b => (a as i64) <= (b as i64)),
                0x58 => bin64!(a, b => a <= b),
                0x59 => bin64!(a, b => (a as i64) >= (b as i64)),
                0x5a => bin64!(a, b => a >= b),
                // f32 e f64 comparações
                0x5b => cmpf32!(a, b => a == b),
                0x5c => cmpf32!(a, b => a != b),
                0x5d => cmpf32!(a, b => a < b),
                0x5e => cmpf32!(a, b => a > b),
                0x5f => cmpf32!(a, b => a <= b),
                0x60 => cmpf32!(a, b => a >= b),
                0x61 => cmpf64!(a, b => a == b),
                0x62 => cmpf64!(a, b => a != b),
                0x63 => cmpf64!(a, b => a < b),
                0x64 => cmpf64!(a, b => a > b),
                0x65 => cmpf64!(a, b => a <= b),
                0x66 => cmpf64!(a, b => a >= b),
                // i32 aritmética
                0x67 => un32!(a => a.leading_zeros()),
                0x68 => un32!(a => a.trailing_zeros()),
                0x69 => un32!(a => a.count_ones()),
                0x6a => bin32!(a, b => a.wrapping_add(b)),
                0x6b => bin32!(a, b => a.wrapping_sub(b)),
                0x6c => bin32!(a, b => a.wrapping_mul(b)),
                0x6d => {
                    let b = pop!() as u32 as i32;
                    let a = pop!() as u32 as i32;
                    if b == 0 {
                        return Err(trap(ExceptionType::DivisionByZero));
                    }
                    if a == i32::MIN && b == -1 {
                        return Err(trap(ExceptionType::IntegerOverflow));
                    }
                    stack.push(u64::from((a / b) as u32));
                }
                0x6e => {
                    let b = pop!() as u32;
                    let a = pop!() as u32;
                    if b == 0 {
                        return Err(trap(ExceptionType::DivisionByZero));
                    }
                    stack.push(u64::from(a / b));
                }
                0x6f => {
                    let b = pop!() as u32 as i32;
                    let a = pop!() as u32 as i32;
                    if b == 0 {
                        return Err(trap(ExceptionType::DivisionByZero));
                    }
                    stack.push(u64::from(a.wrapping_rem(b) as u32));
                }
                0x70 => {
                    let b = pop!() as u32;
                    let a = pop!() as u32;
                    if b == 0 {
                        return Err(trap(ExceptionType::DivisionByZero));
                    }
                    stack.push(u64::from(a % b));
                }
                0x71 => bin32!(a, b => a & b),
                0x72 => bin32!(a, b => a | b),
                0x73 => bin32!(a, b => a ^ b),
                0x74 => bin32!(a, b => a.wrapping_shl(b)),
                0x75 => bin32!(a, b => (a as i32).wrapping_shr(b) as u32),
                0x76 => bin32!(a, b => a.wrapping_shr(b)),
                0x77 => bin32!(a, b => a.rotate_left(b % 32)),
                0x78 => bin32!(a, b => a.rotate_right(b % 32)),
                // i64 aritmética
                0x79 => un64!(a => a.leading_zeros()),
                0x7a => un64!(a => a.trailing_zeros()),
                0x7b => un64!(a => a.count_ones()),
                0x7c => bin64!(a, b => a.wrapping_add(b)),
                0x7d => bin64!(a, b => a.wrapping_sub(b)),
                0x7e => bin64!(a, b => a.wrapping_mul(b)),
                0x7f => {
                    let b = pop!() as i64;
                    let a = pop!() as i64;
                    if b == 0 {
                        return Err(trap(ExceptionType::DivisionByZero));
                    }
                    if a == i64::MIN && b == -1 {
                        return Err(trap(ExceptionType::IntegerOverflow));
                    }
                    stack.push((a / b) as u64);
                }
                0x80 => {
                    let b = pop!();
                    let a = pop!();
                    if b == 0 {
                        return Err(trap(ExceptionType::DivisionByZero));
                    }
                    stack.push(a / b);
                }
                0x81 => {
                    let b = pop!() as i64;
                    let a = pop!() as i64;
                    if b == 0 {
                        return Err(trap(ExceptionType::DivisionByZero));
                    }
                    stack.push(a.wrapping_rem(b) as u64);
                }
                0x82 => {
                    let b = pop!();
                    let a = pop!();
                    if b == 0 {
                        return Err(trap(ExceptionType::DivisionByZero));
                    }
                    stack.push(a % b);
                }
                0x83 => bin64!(a, b => a & b),
                0x84 => bin64!(a, b => a | b),
                0x85 => bin64!(a, b => a ^ b),
                0x86 => bin64!(a, b => a.wrapping_shl(b as u32)),
                0x87 => bin64!(a, b => (a as i64).wrapping_shr(b as u32) as u64),
                0x88 => bin64!(a, b => a.wrapping_shr(b as u32)),
                0x89 => bin64!(a, b => a.rotate_left((b % 64) as u32)),
                0x8a => bin64!(a, b => a.rotate_right((b % 64) as u32)),
                // f32 aritmética
                0x8b => unf32!(a => a.abs()),
                0x8c => unf32!(a => -a),
                0x8d => unf32!(a => a.ceil()),
                0x8e => unf32!(a => a.floor()),
                0x8f => unf32!(a => a.trunc()),
                0x90 => unf32!(a => a.round_ties_even()),
                0x91 => unf32!(a => a.sqrt()),
                0x92 => binf32!(a, b => a + b),
                0x93 => binf32!(a, b => a - b),
                0x94 => binf32!(a, b => a * b),
                0x95 => binf32!(a, b => a / b),
                0x96 => binf32!(a, b => fmin32(a, b)),
                0x97 => binf32!(a, b => fmax32(a, b)),
                0x98 => binf32!(a, b => a.copysign(b)),
                // f64 aritmética
                0x99 => unf64!(a => a.abs()),
                0x9a => unf64!(a => -a),
                0x9b => unf64!(a => a.ceil()),
                0x9c => unf64!(a => a.floor()),
                0x9d => unf64!(a => a.trunc()),
                0x9e => unf64!(a => a.round_ties_even()),
                0x9f => unf64!(a => a.sqrt()),
                0xa0 => binf64!(a, b => a + b),
                0xa1 => binf64!(a, b => a - b),
                0xa2 => binf64!(a, b => a * b),
                0xa3 => binf64!(a, b => a / b),
                0xa4 => binf64!(a, b => fmin64(a, b)),
                0xa5 => binf64!(a, b => fmax64(a, b)),
                0xa6 => binf64!(a, b => a.copysign(b)),
                // conversões
                0xa7 => un64!(a => a as u32),
                0xa8 | 0xaa | 0xa9 | 0xab | 0xae | 0xb0 | 0xaf | 0xb1 => {
                    let source = if matches!(op, 0xa8 | 0xa9 | 0xae | 0xaf) {
                        f64::from(f32::from_bits(pop!() as u32))
                    } else {
                        f64::from_bits(pop!())
                    };
                    let value = match op {
                        0xa8 | 0xaa => u64::from(trunc_checked(source, -2147483648.0, 2147483648.0)? as i32 as u32),
                        0xa9 | 0xab => u64::from(trunc_checked(source, 0.0, 4294967296.0)? as u32),
                        0xae | 0xb0 => trunc_checked(source, -9223372036854775808.0, 9223372036854775808.0)? as i64 as u64,
                        _ => trunc_checked(source, 0.0, 18446744073709551616.0)? as u64,
                    };
                    stack.push(value);
                }
                0xac => un64!(a => a as u32 as i32 as i64 as u64),
                0xad => un64!(a => a as u32),
                0xb2 => {
                    let a = pop!() as u32 as i32;
                    stack.push(u64::from((a as f32).to_bits()));
                }
                0xb3 => {
                    let a = pop!() as u32;
                    stack.push(u64::from((a as f32).to_bits()));
                }
                0xb4 => {
                    let a = pop!() as i64;
                    stack.push(u64::from((a as f32).to_bits()));
                }
                0xb5 => {
                    let a = pop!();
                    stack.push(u64::from((a as f32).to_bits()));
                }
                0xb6 => {
                    let a = f64::from_bits(pop!());
                    stack.push(u64::from((a as f32).to_bits()));
                }
                0xb7 => {
                    let a = pop!() as u32 as i32;
                    stack.push(f64::from(a).to_bits());
                }
                0xb8 => {
                    let a = pop!() as u32;
                    stack.push(f64::from(a).to_bits());
                }
                0xb9 => {
                    let a = pop!() as i64;
                    stack.push((a as f64).to_bits());
                }
                0xba => {
                    let a = pop!();
                    stack.push((a as f64).to_bits());
                }
                0xbb => {
                    let a = f32::from_bits(pop!() as u32);
                    stack.push(f64::from(a).to_bits());
                }
                // As reinterpretações não mudam os bits da pilha.
                0xbc..=0xbf => {}
                0xc0 => un32!(a => a as u8 as i8 as i32 as u32),
                0xc1 => un32!(a => a as u16 as i16 as i32 as u32),
                0xc2 => un64!(a => a as u8 as i8 as i64 as u64),
                0xc3 => un64!(a => a as u16 as i16 as i64 as u64),
                0xc4 => un64!(a => a as u32 as i32 as i64 as u64),
                0xd0 => {
                    read_s64(code, &mut pc);
                    stack.push(null_ref());
                }
                0xd1 => {
                    let value = pop!();
                    stack.push(u64::from(value == null_ref()));
                }
                0xd2 => {
                    let index = read_u32(code, &mut pc);
                    stack.push(func_ref(self.id, index));
                }
                // ref.eq: as referências comparáveis (null, i31, nas fatias seguintes struct e array) são iguais
                // quando os bits são.
                0xd3 => {
                    let right = pop!();
                    let left = pop!();
                    stack.push(u64::from(left == right));
                }
                0xd4 => {
                    if stack.last() == Some(&null_ref()) {
                        return Err(trap(ExceptionType::NullRefAsNonNull));
                    }
                }
                0xfb => {
                    let sub = read_u32(code, &mut pc);
                    match sub {
                        20..=23 => {
                            let heap_type = read_s64(code, &mut pc);
                            let reference = pop!();
                            let nullable = sub == 21 || sub == 23;
                            let matches = reference_matches(self.id, info, &self.gc_cells.borrow(), reference, heap_type, nullable);
                            if sub < 22 {
                                stack.push(u64::from(matches));
                            } else if matches {
                                stack.push(reference);
                            } else {
                                // `ref_cast` do IPInt: com índice de tipo, nulo num cast não anulável é NullAccess.
                                if !nullable && heap_type >= 0 && reference == null_ref() {
                                    return Err(trap(ExceptionType::NullAccess));
                                }
                                return Err(trap(ExceptionType::CastFailure));
                            }
                        }
                        28 => {
                            let value = pop!();
                            stack.push(i31_ref(value as u32));
                        }
                        29 | 30 => {
                            let reference = pop!();
                            let Some(bits) = i31_ref_value(reference) else {
                                return Err(trap(ExceptionType::NullI31Get));
                            };
                            let value = if sub == 29 { ((bits << 1) as i32 >> 1) as u32 } else { bits };
                            stack.push(u64::from(value));
                        }
                        26 | 27 => {
                            // any.convert_extern / extern.convert_any: a mesma referência (o modelo de células
                            // não distingue o invólucro de externref).
                        }
                        0..=14 | 16..=19 => {
                            let type_position = read_u32(code, &mut pc);
                            let definition = info.rtt(type_position as usize);
                            let canonical_type = info.canonical_type_id(type_position as usize);
                            match (&definition.structural, sub) {
                                (StructuralType::Struct { fields }, 0 | 1) => {
                                    let values = if sub == 0 {
                                        let slots: usize = fields.iter().map(|field| storage_slots(field.ty)).sum();
                                        let start = stack.len() - slots;
                                        let popped = stack.split_off(start);
                                        pack_stack_values(fields.iter().map(|field| field.ty), &popped)
                                    } else {
                                        fields.iter().flat_map(|field| default_storage_value(field.ty)).collect()
                                    };
                                    let mut cells = self.gc_cells.borrow_mut();
                                    cells.push(GcCell::Struct { canonical_type, fields: values });
                                    stack.push(gc_ref(cells.len() - 1));
                                }
                                (StructuralType::Struct { fields }, 2..=4) => {
                                    let field_index = read_u32(code, &mut pc) as usize;
                                    let reference = pop!();
                                    let Some(index) = gc_ref_index(reference) else { return Err(trap(ExceptionType::NullAccess)) };
                                    let cells = self.gc_cells.borrow();
                                    let GcCell::Struct { fields: values, .. } = &cells[index] else { unreachable!("struct.get em array") };
                                    let offset = struct_slot_offset(fields, field_index);
                                    if storage_slots(fields[field_index].ty) == 2 {
                                        stack.push(values[offset]);
                                        stack.push(values[offset + 1]);
                                    } else {
                                        stack.push(unpack_storage(fields[field_index].ty, values[offset], sub == 3));
                                    }
                                }
                                (StructuralType::Struct { fields }, 5) => {
                                    let field_index = read_u32(code, &mut pc) as usize;
                                    let wide = storage_slots(fields[field_index].ty) == 2;
                                    let high = if wide { pop!() } else { 0 };
                                    let value = pop!();
                                    let reference = pop!();
                                    let Some(index) = gc_ref_index(reference) else { return Err(trap(ExceptionType::NullAccess)) };
                                    let mut cells = self.gc_cells.borrow_mut();
                                    let GcCell::Struct { fields: values, .. } = &mut cells[index] else { unreachable!("struct.set em array") };
                                    let offset = struct_slot_offset(fields, field_index);
                                    if wide {
                                        values[offset] = value;
                                        values[offset + 1] = high;
                                    } else {
                                        values[offset] = pack_storage(fields[field_index].ty, value);
                                    }
                                }
                                (StructuralType::Array { element }, 6 | 7) => {
                                    let (length, init) = if sub == 6 {
                                        let length = pop!() as u32;
                                        let width = storage_slots(element.ty);
                                        let start = stack.len() - width;
                                        let popped = stack.split_off(start);
                                        (length, pack_stack_values(std::iter::once(element.ty), &popped))
                                    } else {
                                        (pop!() as u32, default_storage_value(element.ty))
                                    };
                                    if !array_fits(element.ty, u64::from(length)) {
                                        return Err(trap(ExceptionType::BadArrayNew));
                                    }
                                    // Preenchimento todo em zero usa `vec![0; n]` (páginas zeradas preguiçosas do alocador):
                                    // um array de 2^30 elementos de 1 byte não escreve 8 GiB de zeros.
                                    let elements = if init.iter().all(|slot| *slot == 0) {
                                        vec![0u64; init.len() * length as usize]
                                    } else {
                                        init.repeat(length as usize)
                                    };
                                    let mut cells = self.gc_cells.borrow_mut();
                                    cells.push(GcCell::Array { canonical_type, elements });
                                    stack.push(gc_ref(cells.len() - 1));
                                }
                                (StructuralType::Array { element }, 8) => {
                                    let length = read_u32(code, &mut pc) as usize;
                                    let start = stack.len() - length * storage_slots(element.ty);
                                    let popped = stack.split_off(start);
                                    let elements = pack_stack_values(std::iter::repeat_n(element.ty, length), &popped);
                                    let mut cells = self.gc_cells.borrow_mut();
                                    cells.push(GcCell::Array { canonical_type, elements });
                                    stack.push(gc_ref(cells.len() - 1));
                                }
                                (StructuralType::Array { element }, 11..=13) => {
                                    let width = storage_slots(element.ty);
                                    let element_index = pop!() as u32 as usize;
                                    let reference = pop!();
                                    let Some(index) = gc_ref_index(reference) else { return Err(trap(ExceptionType::NullAccess)) };
                                    let cells = self.gc_cells.borrow();
                                    let GcCell::Array { elements, .. } = &cells[index] else { unreachable!("array.get em struct") };
                                    let Some(slots) = elements.get(element_index * width..(element_index + 1) * width) else {
                                        return Err(trap(ExceptionType::OutOfBoundsArrayGet));
                                    };
                                    if width == 2 {
                                        stack.extend_from_slice(slots);
                                    } else {
                                        stack.push(unpack_storage(element.ty, slots[0], sub == 12));
                                    }
                                }
                                (StructuralType::Array { element }, 14) => {
                                    let width = storage_slots(element.ty);
                                    let start = stack.len() - width;
                                    let popped = stack.split_off(start);
                                    let value = pack_stack_values(std::iter::once(element.ty), &popped);
                                    let element_index = pop!() as u32 as usize;
                                    let reference = pop!();
                                    let Some(index) = gc_ref_index(reference) else { return Err(trap(ExceptionType::NullAccess)) };
                                    let mut cells = self.gc_cells.borrow_mut();
                                    let GcCell::Array { elements, .. } = &mut cells[index] else { unreachable!("array.set em struct") };
                                    let Some(slots) = elements.get_mut(element_index * width..(element_index + 1) * width) else {
                                        return Err(trap(ExceptionType::OutOfBoundsArraySet));
                                    };
                                    slots.copy_from_slice(&value);
                                }
                                (StructuralType::Array { element }, 16) => {
                                    let width = storage_slots(element.ty);
                                    let length = pop!() as u32 as usize;
                                    let start = stack.len() - width;
                                    let popped = stack.split_off(start);
                                    let value = pack_stack_values(std::iter::once(element.ty), &popped);
                                    let offset = pop!() as u32 as usize;
                                    let reference = pop!();
                                    let Some(index) = gc_ref_index(reference) else { return Err(trap(ExceptionType::NullAccess)) };
                                    let mut cells = self.gc_cells.borrow_mut();
                                    let GcCell::Array { elements, .. } = &mut cells[index] else { unreachable!("array.fill em struct") };
                                    if offset.checked_add(length).is_none_or(|end| end > elements.len() / width) {
                                        return Err(trap(ExceptionType::OutOfBoundsArrayFill));
                                    }
                                    for slots in elements[offset * width..(offset + length) * width].chunks_exact_mut(width) {
                                        slots.copy_from_slice(&value);
                                    }
                                }
                                (StructuralType::Array { element }, 9 | 10) => {
                                    let segment_index = read_u32(code, &mut pc) as usize;
                                    let size = u64::from(pop!() as u32);
                                    let offset = u64::from(pop!() as u32);
                                    let (failure, elements) = if sub == 9 {
                                        let element_size = storage_byte_size(element.ty);
                                        let bytes: &[u8] = if self.dropped_data.borrow()[segment_index] { &[] } else { &info.data[segment_index].bytes };
                                        let range = size
                                            .checked_mul(element_size)
                                            .and_then(|byte_count| offset.checked_add(byte_count))
                                            .filter(|end| *end <= bytes.len() as u64)
                                            .filter(|_| array_fits(element.ty, size));
                                        let elements = range.map(|_| read_segment_slots(bytes, offset, size, element_size));
                                        (ExceptionType::BadArrayNewInitData, elements)
                                    } else {
                                        let length = if self.dropped_elements.borrow()[segment_index] {
                                            0
                                        } else {
                                            u64::from(info.elements[segment_index].length())
                                        };
                                        let in_range = offset.checked_add(size).is_some_and(|end| end <= length) && array_fits(element.ty, size);
                                        let elements = if in_range {
                                            (0..size).map(|i| self.element_entry_value(segment_index, (offset + i) as usize)).collect::<Result<Vec<u64>, _>>().map(Some)?
                                        } else {
                                            None
                                        };
                                        (ExceptionType::BadArrayNewInitElem, elements)
                                    };
                                    let Some(elements) = elements else { return Err(trap(failure)) };
                                    let mut cells = self.gc_cells.borrow_mut();
                                    cells.push(GcCell::Array { canonical_type, elements });
                                    stack.push(gc_ref(cells.len() - 1));
                                }
                                (StructuralType::Array { element }, 18 | 19) => {
                                    let segment_index = read_u32(code, &mut pc) as usize;
                                    let size = u64::from(pop!() as u32);
                                    let source_offset = u64::from(pop!() as u32);
                                    let destination_offset = u64::from(pop!() as u32);
                                    let reference = pop!();
                                    let Some(index) = gc_ref_index(reference) else {
                                        return Err(trap(if sub == 18 { ExceptionType::NullArrayInitData } else { ExceptionType::NullArrayInitElem }));
                                    };
                                    let out_of_bounds = if sub == 18 { ExceptionType::OutOfBoundsArrayInitData } else { ExceptionType::OutOfBoundsArrayInitElem };
                                    let mut cells = self.gc_cells.borrow_mut();
                                    let GcCell::Array { elements, .. } = &mut cells[index] else { unreachable!("array.init em struct") };
                                    let width = storage_slots(element.ty) as u64;
                                    let destination_fits = destination_offset.checked_add(size).is_some_and(|end| end <= elements.len() as u64 / width);
                                    let values = if !destination_fits {
                                        None
                                    } else if sub == 18 {
                                        let element_size = storage_byte_size(element.ty);
                                        let bytes: &[u8] = if self.dropped_data.borrow()[segment_index] { &[] } else { &info.data[segment_index].bytes };
                                        size.checked_mul(element_size)
                                            .and_then(|byte_count| source_offset.checked_add(byte_count))
                                            .filter(|end| *end <= bytes.len() as u64)
                                            .map(|_| read_segment_slots(bytes, source_offset, size, element_size))
                                    } else {
                                        let length = if self.dropped_elements.borrow()[segment_index] {
                                            0
                                        } else {
                                            u64::from(info.elements[segment_index].length())
                                        };
                                        if source_offset.checked_add(size).is_some_and(|end| end <= length) {
                                            Some((0..size).map(|i| self.element_entry_value(segment_index, (source_offset + i) as usize)).collect::<Result<Vec<u64>, _>>()?)
                                        } else {
                                            None
                                        }
                                    };
                                    let Some(values) = values else { return Err(trap(out_of_bounds)) };
                                    let start = destination_offset as usize * width as usize;
                                    elements[start..start + values.len()].copy_from_slice(&values);
                                }
                                (StructuralType::Array { element }, 17) => {
                                    let width = storage_slots(element.ty);
                                    read_u32(code, &mut pc);
                                    let length = pop!() as u32 as usize;
                                    let source_offset = pop!() as u32 as usize;
                                    let source_reference = pop!();
                                    let destination_offset = pop!() as u32 as usize;
                                    let destination_reference = pop!();
                                    let (Some(source_index), Some(destination_index)) =
                                        (gc_ref_index(source_reference), gc_ref_index(destination_reference))
                                    else {
                                        return Err(trap(ExceptionType::NullAccess));
                                    };
                                    let mut cells = self.gc_cells.borrow_mut();
                                    let GcCell::Array { elements: source, .. } = &cells[source_index] else { unreachable!("array.copy em struct") };
                                    let GcCell::Array { elements: destination, .. } = &cells[destination_index] else { unreachable!("array.copy em struct") };
                                    if source_offset.checked_add(length).is_none_or(|end| end > source.len() / width)
                                        || destination_offset.checked_add(length).is_none_or(|end| end > destination.len() / width)
                                    {
                                        return Err(trap(ExceptionType::OutOfBoundsArrayCopy));
                                    }
                                    let chunk = source[source_offset * width..(source_offset + length) * width].to_vec();
                                    let GcCell::Array { elements: destination, .. } = &mut cells[destination_index] else { unreachable!() };
                                    destination[destination_offset * width..(destination_offset + length) * width].copy_from_slice(&chunk);
                                }
                                _ => return Err(unsupported(&format!("instruction 0xfb {}", sub))),
                            }
                        }
                        15 => {
                            let reference = pop!();
                            let Some(index) = gc_ref_index(reference) else { return Err(trap(ExceptionType::NullAccess)) };
                            let cells = self.gc_cells.borrow();
                            let GcCell::Array { elements, canonical_type } = &cells[index] else { unreachable!("array.len em struct") };
                            let width = match &info.canonical_rtt(*canonical_type).structural {
                                StructuralType::Array { element } => storage_slots(element.ty),
                                _ => 1,
                            };
                            stack.push((elements.len() / width) as u64);
                        }
                        24 | 25 => {
                            let flags = code[pc];
                            pc += 1;
                            let depth = read_u32(code, &mut pc);
                            read_s64(code, &mut pc);
                            let target = read_s64(code, &mut pc);
                            let reference = *stack.last().expect("pilha vazia");
                            let matches = reference_matches(self.id, info, &self.gc_cells.borrow(), reference, target, flags & 2 != 0);
                            if matches == (sub == 24) {
                                branch!(depth);
                            }
                        }
                        _ => return Err(unsupported(&format!("instruction 0xfb {}", sub))),
                    }
                }
                0xfe => {
                    let sub = read_u32(code, &mut pc);
                    if sub == 3 {
                        // atomic.fence: sem efeito com uma thread só.
                        read_u32(code, &mut pc);
                        continue;
                    }
                    let (memory_index, offset) = memarg!();
                    let memory = &self.memories[memory_index];
                    match sub {
                        // memory.atomic.notify: sem espera de outras threads, ninguém acorda.
                        0 => {
                            let _count = pop!();
                            let address = pop!();
                            atomic_effective(&memory.borrow(), address, offset, 4, ExceptionType::OutOfBoundsMemoryAccess)?;
                            stack.push(0);
                        }
                        // memory.atomic.wait32/wait64: 1 se o valor difere, 2 (esgotou) quando igual; sem
                        // outra thread para acordar, o tempo sempre se esgota. Memória não compartilhada
                        // trapa com `Out of bounds memory access` (medido no bun).
                        1 | 2 => {
                            let _timeout = pop!();
                            let expected = pop!();
                            let address = pop!();
                            let width = if sub == 1 { 4 } else { 8 };
                            let memory = memory.borrow();
                            let effective = atomic_effective(&memory, address, offset, width, ExceptionType::OutOfBoundsMemoryAccess)?;
                            if !memory.is_shared() {
                                return Err(trap(ExceptionType::OutOfBoundsMemoryAccess));
                            }
                            let current = memory.load(effective, width).expect("limites já conferidos");
                            let expected = if sub == 1 { expected & u64::from(u32::MAX) } else { expected };
                            stack.push(if current == expected { 2 } else { 1 });
                        }
                        16..=22 => {
                            let width = ATOMIC_WIDTHS[(sub - 16) as usize];
                            let address = pop!();
                            let effective = atomic_effective(&memory.borrow(), address, offset, width, ExceptionType::UnalignedMemoryAccess)?;
                            stack.push(memory.borrow().load(effective, width).expect("limites já conferidos"));
                        }
                        23..=29 => {
                            let width = ATOMIC_WIDTHS[(sub - 23) as usize];
                            let value = pop!();
                            let address = pop!();
                            let effective = atomic_effective(&memory.borrow(), address, offset, width, ExceptionType::UnalignedMemoryAccess)?;
                            memory.borrow_mut().store(effective, width, value);
                        }
                        // rmw: add, sub, and, or, xor, xchg, cada um com os sete formatos.
                        30..=71 => {
                            let width = ATOMIC_WIDTHS[((sub - 30) % 7) as usize];
                            let operand = pop!();
                            let address = pop!();
                            let effective = atomic_effective(&memory.borrow(), address, offset, width, ExceptionType::UnalignedMemoryAccess)?;
                            let old = memory.borrow().load(effective, width).expect("limites já conferidos");
                            let new = match (sub - 30) / 7 {
                                0 => old.wrapping_add(operand),
                                1 => old.wrapping_sub(operand),
                                2 => old & operand,
                                3 => old | operand,
                                4 => old ^ operand,
                                _ => operand,
                            };
                            memory.borrow_mut().store(effective, width, new);
                            stack.push(old);
                        }
                        72..=78 => {
                            let width = ATOMIC_WIDTHS[(sub - 72) as usize];
                            let replacement = pop!();
                            let expected = pop!();
                            let address = pop!();
                            let effective = atomic_effective(&memory.borrow(), address, offset, width, ExceptionType::UnalignedMemoryAccess)?;
                            let old = memory.borrow().load(effective, width).expect("limites já conferidos");
                            let mask = if width == 8 { u64::MAX } else { (1u64 << (8 * width)) - 1 };
                            if old == expected & mask {
                                memory.borrow_mut().store(effective, width, replacement);
                            }
                            stack.push(old);
                        }
                        _ => return Err(unsupported(&format!("instruction 0xfe {}", sub))),
                    }
                }
                0xfd => {
                    use SimdLaneOperation as Op;
                    macro_rules! pop_v128 {
                        () => {{
                            let high = pop!();
                            let low = pop!();
                            wasm_simd::join(low, high)
                        }};
                    }
                    macro_rules! push_v128 {
                        ($value:expr) => {{
                            let (low, high) = wasm_simd::split($value);
                            stack.push(low);
                            stack.push(high);
                        }};
                    }
                    let sub = read_u32(code, &mut pc);
                    let Some(simd) = ExtSimdOpType::from_value(sub) else {
                        return Err(unsupported(&format!("instruction 0xfd {}", sub)));
                    };
                    let (operation, lane, sign) = simd.info();
                    let signed = sign == SimdSignMode::Signed;
                    match operation {
                        Op::Const => {
                            let bytes: [u8; 16] = code[pc..pc + 16].try_into().expect("v128.const truncado");
                            pc += 16;
                            push_v128!(wasm_simd::from_bytes(bytes));
                        }
                        Op::Shuffle => {
                            let indices: [u8; 16] = code[pc..pc + 16].try_into().expect("shuffle truncado");
                            pc += 16;
                            let b = pop_v128!();
                            let a = pop_v128!();
                            push_v128!(wasm_simd::shuffle(a, b, indices));
                        }
                        Op::Splat => {
                            let bits = pop!();
                            push_v128!(wasm_simd::splat(lane, bits));
                        }
                        Op::ExtractLane => {
                            let index = code[pc];
                            pc += 1;
                            let value = pop_v128!();
                            stack.push(wasm_simd::extract_lane(lane, value, index, signed));
                        }
                        Op::ReplaceLane => {
                            let index = code[pc];
                            pc += 1;
                            let bits = pop!();
                            let value = pop_v128!();
                            push_v128!(wasm_simd::lane_set(lane, value, index, bits));
                        }
                        Op::AnyTrue => {
                            let value = pop_v128!();
                            stack.push(u64::from(wasm_simd::any_true(value)));
                        }
                        Op::AllTrue => {
                            let value = pop_v128!();
                            stack.push(u64::from(wasm_simd::all_true(lane, value)));
                        }
                        Op::Bitmask => {
                            let value = pop_v128!();
                            stack.push(u64::from(wasm_simd::bitmask(lane, value)));
                        }
                        Op::RelaxedMAdd | Op::RelaxedNMAdd | Op::RelaxedLaneSelect | Op::RelaxedDotI8x16I7x16Add => {
                            let c = pop_v128!();
                            let b = pop_v128!();
                            let a = pop_v128!();
                            push_v128!(wasm_simd::ternary(operation, lane, a, b, c).expect("relaxed ternary"));
                        }
                        Op::BitwiseSelect => {
                            let mask = pop_v128!();
                            let b = pop_v128!();
                            let a = pop_v128!();
                            push_v128!(wasm_simd::bitselect(a, b, mask));
                        }
                        Op::Shl | Op::Shr => {
                            let count = pop!() as u32;
                            let value = pop_v128!();
                            push_v128!(wasm_simd::shift(operation, lane, sign, value, count).expect("shift"));
                        }
                        Op::Load | Op::LoadExtend8S | Op::LoadExtend8U | Op::LoadExtend16S | Op::LoadExtend16U
                        | Op::LoadExtend32S | Op::LoadExtend32U | Op::LoadSplat8 | Op::LoadSplat16
                        | Op::LoadSplat32 | Op::LoadSplat64 | Op::LoadPad32 | Op::LoadPad64 => {
                            let (memory_index, offset) = memarg!();
                            let address = pop!();
                            let out_of_bounds = || trap(ExceptionType::OutOfBoundsMemoryAccess);
                            let effective = address.checked_add(offset).ok_or_else(out_of_bounds)?;
                            let memory = self.memories[memory_index].borrow();
                            if operation == Op::Load {
                                let low = memory.load(effective, 8).ok_or_else(out_of_bounds)?;
                                let high = effective
                                    .checked_add(8)
                                    .and_then(|next| memory.load(next, 8))
                                    .ok_or_else(out_of_bounds)?;
                                drop(memory);
                                stack.push(low);
                                stack.push(high);
                            } else {
                                let (width, element) = simd_load_shape(operation);
                                let loaded = memory.load(effective, width).ok_or_else(out_of_bounds)?;
                                drop(memory);
                                let value = match operation {
                                    Op::LoadExtend8S | Op::LoadExtend8U => {
                                        wasm_simd::unary(Op::ExtendLow, SimdLane::I16x8, sign, u128::from(loaded))
                                    }
                                    Op::LoadExtend16S | Op::LoadExtend16U => {
                                        wasm_simd::unary(Op::ExtendLow, SimdLane::I32x4, sign, u128::from(loaded))
                                    }
                                    Op::LoadExtend32S | Op::LoadExtend32U => {
                                        wasm_simd::unary(Op::ExtendLow, SimdLane::I64x2, sign, u128::from(loaded))
                                    }
                                    Op::LoadPad32 | Op::LoadPad64 => Some(u128::from(loaded)),
                                    _ => Some(wasm_simd::splat(element, loaded)),
                                };
                                push_v128!(value.expect("load simd"));
                            }
                        }
                        Op::Store => {
                            let (memory_index, offset) = memarg!();
                            let value = pop_v128!();
                            let address = pop!();
                            let (low, high) = wasm_simd::split(value);
                            let stored = address.checked_add(offset).is_some_and(|effective| {
                                let mut memory = self.memories[memory_index].borrow_mut();
                                let next = effective.checked_add(8);
                                next.is_some_and(|next| memory.load(effective, 8).is_some() && memory.load(next, 8).is_some())
                                    && memory.store(effective, 8, low)
                                    && memory.store(effective + 8, 8, high)
                            });
                            if !stored {
                                return Err(trap(ExceptionType::OutOfBoundsMemoryAccess));
                            }
                        }
                        Op::LoadLane8 | Op::LoadLane16 | Op::LoadLane32 | Op::LoadLane64 => {
                            let (memory_index, offset) = memarg!();
                            let index = code[pc];
                            pc += 1;
                            let value = pop_v128!();
                            let address = pop!();
                            let (width, element) = simd_load_shape(operation);
                            let loaded = address
                                .checked_add(offset)
                                .and_then(|effective| self.memories[memory_index].borrow().load(effective, width))
                                .ok_or_else(|| trap(ExceptionType::OutOfBoundsMemoryAccess))?;
                            push_v128!(wasm_simd::lane_set(element, value, index, loaded));
                        }
                        Op::StoreLane8 | Op::StoreLane16 | Op::StoreLane32 | Op::StoreLane64 => {
                            let (memory_index, offset) = memarg!();
                            let index = code[pc];
                            pc += 1;
                            let value = pop_v128!();
                            let address = pop!();
                            let (width, element) = simd_load_shape(operation);
                            let bits = wasm_simd::lane_get(element, value, index);
                            let stored = address
                                .checked_add(offset)
                                .is_some_and(|effective| self.memories[memory_index].borrow_mut().store(effective, width, bits));
                            if !stored {
                                return Err(trap(ExceptionType::OutOfBoundsMemoryAccess));
                            }
                        }
                        _ => {
                            let unary = matches!(
                                operation,
                                Op::Not | Op::Abs | Op::Neg | Op::Popcnt | Op::Sqrt | Op::Ceil | Op::Floor | Op::Trunc
                                    | Op::Nearest | Op::ExtendLow | Op::ExtendHigh | Op::ExtaddPairwise | Op::Convert
                                    | Op::ConvertLow | Op::TruncSat | Op::Demote | Op::Promote | Op::RelaxedTruncSat
                            );
                            let result = if unary {
                                let a = pop_v128!();
                                wasm_simd::unary(operation, lane, sign, a)
                            } else {
                                let b = pop_v128!();
                                let a = pop_v128!();
                                wasm_simd::binary(operation, lane, sign, a, b)
                            };
                            match result {
                                Some(value) => push_v128!(value),
                                None => return Err(unsupported(&format!("instruction 0xfd {}", sub))),
                            }
                        }
                    }
                }
                0xfc => {
                    let sub = read_u32(code, &mut pc);
                    match sub {
                        0 | 2 => {
                            let a = if sub == 0 { f64::from(f32::from_bits(pop!() as u32)) } else { f64::from_bits(pop!()) };
                            stack.push(u64::from(a as i32 as u32));
                        }
                        1 | 3 => {
                            let a = if sub == 1 { f64::from(f32::from_bits(pop!() as u32)) } else { f64::from_bits(pop!()) };
                            stack.push(u64::from(a as u32));
                        }
                        4 | 6 => {
                            let a = if sub == 4 { f64::from(f32::from_bits(pop!() as u32)) } else { f64::from_bits(pop!()) };
                            stack.push(a as i64 as u64);
                        }
                        5 | 7 => {
                            let a = if sub == 5 { f64::from(f32::from_bits(pop!() as u32)) } else { f64::from_bits(pop!()) };
                            stack.push(a as u64);
                        }
                        8 => {
                            let data_index = read_u32(code, &mut pc) as usize;
                            let memory_index = read_u32(code, &mut pc) as usize;
                            let length = pop!();
                            let source = pop!();
                            let destination = pop!();
                            let dropped = self.dropped_data.borrow()[data_index];
                            let bytes: &[u8] = if dropped { &[] } else { &info.data[data_index].bytes };
                            let range = source.checked_add(length).filter(|end| *end <= bytes.len() as u64);
                            let Some(end) = range else {
                                return Err(trap(ExceptionType::OutOfBoundsMemoryAccess));
                            };
                            let written = self.memories[memory_index]
                                .borrow_mut()
                                .init(destination, &bytes[source as usize..end as usize]);
                            if !written {
                                return Err(trap(ExceptionType::OutOfBoundsMemoryAccess));
                            }
                        }
                        9 => {
                            let data_index = read_u32(code, &mut pc) as usize;
                            self.dropped_data.borrow_mut()[data_index] = true;
                        }
                        10 => {
                            let destination_memory = read_u32(code, &mut pc) as usize;
                            let source_memory = read_u32(code, &mut pc) as usize;
                            let length = pop!();
                            let source = pop!();
                            let destination = pop!();
                            let copied = if destination_memory == source_memory {
                                self.memories[destination_memory].borrow_mut().copy_within(destination, source, length)
                            } else {
                                let chunk = self.memories[source_memory].borrow().with_slice(source, length, <[u8]>::to_vec);
                                match chunk {
                                    Some(chunk) => self.memories[destination_memory].borrow_mut().init(destination, &chunk),
                                    None => false,
                                }
                            };
                            if !copied {
                                return Err(trap(ExceptionType::OutOfBoundsMemoryAccess));
                            }
                        }
                        11 => {
                            let memory_index = read_u32(code, &mut pc) as usize;
                            let length = pop!();
                            let value = pop!() as u8;
                            let destination = pop!();
                            if !self.memories[memory_index].borrow_mut().fill(destination, value, length) {
                                return Err(trap(ExceptionType::OutOfBoundsMemoryAccess));
                            }
                        }
                        12 => {
                            let element_index = read_u32(code, &mut pc) as usize;
                            let table_index = read_u32(code, &mut pc);
                            let length = pop!();
                            let source = pop!();
                            let destination = pop!();
                            let segment_length = if self.dropped_elements.borrow()[element_index] {
                                0
                            } else {
                                u64::from(info.elements[element_index].length())
                            };
                            let table_length = u64::from(self.tables[table_index as usize].borrow().length());
                            let in_segment = source.checked_add(length).is_some_and(|end| end <= segment_length);
                            let in_table = destination.checked_add(length).is_some_and(|end| end <= table_length);
                            if !in_segment || !in_table {
                                return Err(trap(ExceptionType::OutOfBoundsTableAccess));
                            }
                            self.init_element_segment(table_index, element_index, destination as u32, source as u32, length as u32)?;
                        }
                        13 => {
                            let element_index = read_u32(code, &mut pc) as usize;
                            self.dropped_elements.borrow_mut()[element_index] = true;
                        }
                        14 => {
                            let destination_table = read_u32(code, &mut pc) as usize;
                            let source_table = read_u32(code, &mut pc) as usize;
                            let length = pop!();
                            let source = pop!();
                            let destination = pop!();
                            let source_length = u64::from(self.tables[source_table].borrow().length());
                            let destination_length = u64::from(self.tables[destination_table].borrow().length());
                            let in_source = source.checked_add(length).is_some_and(|end| end <= source_length);
                            let in_destination = destination.checked_add(length).is_some_and(|end| end <= destination_length);
                            if !in_source || !in_destination {
                                return Err(trap(ExceptionType::OutOfBoundsTableAccess));
                            }
                            if destination_table == source_table {
                                self.tables[destination_table].borrow_mut().copy_within(
                                    destination as u32,
                                    source as u32,
                                    length as u32,
                                );
                            } else {
                                let chunk = self.tables[source_table].borrow().elements()
                                    [source as usize..(source + length) as usize]
                                    .to_vec();
                                let mut table = self.tables[destination_table].borrow_mut();
                                for (offset, value) in chunk.into_iter().enumerate() {
                                    table.set(destination as u32 + offset as u32, value);
                                }
                            }
                        }
                        15 => {
                            let table_index = read_u32(code, &mut pc) as usize;
                            let delta = pop!();
                            let value = pop!();
                            let is_64_bit = self.tables[table_index].borrow().address_type().is_64_bit();
                            let delta = if is_64_bit { delta } else { u64::from(delta as u32) };
                            let failure = if is_64_bit { u64::MAX } else { u64::from(u32::MAX) };
                            let result = self.tables[table_index].borrow_mut().grow(delta, value);
                            stack.push(result.map_or(failure, u64::from));
                        }
                        16 => {
                            let table_index = read_u32(code, &mut pc) as usize;
                            stack.push(u64::from(self.tables[table_index].borrow().length()));
                        }
                        _ => {
                            // 17: table.fill
                            let table_index = read_u32(code, &mut pc) as usize;
                            let length = self.table_index_of(pop!(), table_index);
                            let value = pop!();
                            let start = self.table_index_of(pop!(), table_index);
                            let table_length = u64::from(self.tables[table_index].borrow().length());
                            if start.checked_add(length).map_or(true, |end| end > table_length) {
                                return Err(trap(ExceptionType::OutOfBoundsTableAccess));
                            }
                            self.tables[table_index].borrow_mut().fill_range(start as u32, value, length as u32);
                        }
                    }
                }
                _ => return Err(unsupported(&format!("instruction {:#04x}", op))),
            }
        }
        frame.stack = stack;
        Ok(Outcome::Return)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::wasm_format::{TYPE_I32, TYPE_I64};
    use crate::wasm::wasm_instance::{ExportValue, HostFunction, ImportValue};
    use crate::wasm::wasm_streaming_parser::validate_module;

    fn leb(mut value: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn section(id: u8, payload: Vec<u8>) -> Vec<u8> {
        let mut out = vec![id];
        out.extend(leb(payload.len()));
        out.extend(payload);
        out
    }

    /// Um vetor: a contagem e os itens.
    fn vector(items: Vec<Vec<u8>>) -> Vec<u8> {
        let mut out = leb(items.len());
        for item in items {
            out.extend(item);
        }
        out
    }

    fn name(text: &str) -> Vec<u8> {
        let mut out = leb(text.len());
        out.extend(text.bytes());
        out
    }

    fn func_type(arguments: &[u8], returns: &[u8]) -> Vec<u8> {
        let mut out = vec![0x60];
        out.extend(leb(arguments.len()));
        out.extend(arguments);
        out.extend(leb(returns.len()));
        out.extend(returns);
        out
    }

    fn body(code: &[u8]) -> Vec<u8> {
        let mut out = leb(code.len());
        out.extend(code);
        out
    }

    fn export(field: &str, kind: u8, index: usize) -> Vec<u8> {
        let mut out = name(field);
        out.push(kind);
        out.extend(leb(index));
        out
    }

    const HEADER: [u8; 8] = [0, b'a', b's', b'm', 1, 0, 0, 0];
    const I32: u8 = 0x7f;
    const I64: u8 = 0x7e;

    fn instantiate(sections: Vec<Vec<u8>>, imports: Vec<ImportValue>) -> Result<Instance, WasmError> {
        let mut bytes = HEADER.to_vec();
        for section in sections {
            bytes.extend(section);
        }
        let info = validate_module(&bytes, true).unwrap_or_else(|error| panic!("módulo inválido: {error}"));
        Instance::instantiate(Rc::new(info), imports)
    }

    fn call(instance: &Instance, name: &str, arguments: &[u64]) -> Result<Vec<u64>, WasmError> {
        instance.call_export(name, arguments)
    }

    #[test]
    fn factorial_recurses_through_if_else() {
        // (func (param i64) (result i64)
        //   local.get 0; i64.eqz; if (result i64) i64.const 1 else local.get 0
        //   local.get 0; i64.const 1; i64.sub; call 0; i64.mul end)
        let code = [
            0x00, 0x20, 0x00, 0x50, 0x04, 0x7e, 0x42, 0x01, 0x05, 0x20, 0x00, 0x20, 0x00, 0x42, 0x01, 0x7d, 0x10, 0x00, 0x7e,
            0x0b, 0x0b,
        ];
        let instance = instantiate(
            vec![
                section(1, vector(vec![func_type(&[I64], &[I64])])),
                section(3, vector(vec![vec![0]])),
                section(7, vector(vec![export("fact", 0, 0)])),
                section(10, vector(vec![body(&code)])),
            ],
            vec![],
        )
        .unwrap();
        assert_eq!(call(&instance, "fact", &[10]), Ok(vec![3_628_800]));
        assert_eq!(call(&instance, "fact", &[20]), Ok(vec![2_432_902_008_176_640_000]));
        assert_eq!(call(&instance, "fact", &[0]), Ok(vec![1]));
    }

    #[test]
    fn fibonacci_loops_with_br_if() {
        let code = [
            0x01, 0x03, 0x7f, 0x41, 0x01, 0x21, 0x02, 0x02, 0x40, 0x03, 0x40, 0x20, 0x00, 0x45, 0x0d, 0x01, 0x20, 0x01, 0x20,
            0x02, 0x6a, 0x21, 0x03, 0x20, 0x02, 0x21, 0x01, 0x20, 0x03, 0x21, 0x02, 0x20, 0x00, 0x41, 0x01, 0x6b, 0x21, 0x00,
            0x0c, 0x00, 0x0b, 0x0b, 0x20, 0x01, 0x0b,
        ];
        let instance = instantiate(
            vec![
                section(1, vector(vec![func_type(&[I32], &[I32])])),
                section(3, vector(vec![vec![0]])),
                section(7, vector(vec![export("fib", 0, 0)])),
                section(10, vector(vec![body(&code)])),
            ],
            vec![],
        )
        .unwrap();
        let expected = [0u64, 1, 1, 2, 3, 5, 8, 13, 21, 34, 55];
        for (n, value) in expected.iter().enumerate() {
            assert_eq!(call(&instance, "fib", &[n as u64]), Ok(vec![*value]));
        }
    }

    fn memory_module() -> Instance {
        // 0: store_load(addr, value) -> value; 1: grow(delta); 2: size(); 3: load(addr)
        let store_load = [0x00, 0x20, 0x00, 0x20, 0x01, 0x36, 0x02, 0x00, 0x20, 0x00, 0x28, 0x02, 0x00, 0x0b];
        let grow = [0x00, 0x20, 0x00, 0x40, 0x00, 0x0b];
        let size = [0x00, 0x3f, 0x00, 0x0b];
        let load = [0x00, 0x20, 0x00, 0x28, 0x02, 0x00, 0x0b];
        let load8 = [0x00, 0x20, 0x00, 0x2d, 0x00, 0x00, 0x0b];
        instantiate(
            vec![
                section(1, vector(vec![func_type(&[I32, I32], &[I32]), func_type(&[I32], &[I32]), func_type(&[], &[I32])])),
                section(3, vector(vec![vec![0], vec![1], vec![2], vec![1], vec![1]])),
                section(5, vector(vec![vec![0x01, 0x01, 0x02]])),
                section(
                    7,
                    vector(vec![
                        export("store_load", 0, 0),
                        export("grow", 0, 1),
                        export("size", 0, 2),
                        export("load", 0, 3),
                        export("load8", 0, 4),
                        export("mem", 2, 0),
                    ]),
                ),
                section(10, vector(vec![body(&store_load), body(&grow), body(&size), body(&load), body(&load8)])),
                section(11, vector(vec![vec![0x00, 0x41, 0x08, 0x0b, 0x02, b'h', b'i']])),
            ],
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn memory_loads_stores_and_data_segments() {
        let instance = memory_module();
        assert_eq!(call(&instance, "store_load", &[16, 0xdead_beef]), Ok(vec![0xdead_beef]));
        assert_eq!(call(&instance, "load8", &[8]), Ok(vec![u64::from(b'h')]));
        assert_eq!(call(&instance, "load8", &[9]), Ok(vec![u64::from(b'i')]));
        assert_eq!(call(&instance, "load8", &[16]), Ok(vec![0xef]));
        let Some(ExportValue::Memory(memory)) = instance.export("mem") else { panic!("sem memória exportada") };
        assert_eq!(memory.borrow().load(16, 4), Some(0xdead_beef));
    }

    #[test]
    fn memory_grow_and_bounds() {
        let instance = memory_module();
        assert_eq!(call(&instance, "size", &[]), Ok(vec![1]));
        assert_eq!(call(&instance, "load", &[65532]), Ok(vec![0]));
        let out_of_bounds = Err(WasmError::Runtime("Out of bounds memory access".to_string()));
        assert_eq!(call(&instance, "load", &[65533]), out_of_bounds);
        assert_eq!(call(&instance, "load", &[65536]), out_of_bounds);
        assert_eq!(call(&instance, "grow", &[1]), Ok(vec![1]));
        assert_eq!(call(&instance, "size", &[]), Ok(vec![2]));
        assert_eq!(call(&instance, "load", &[65536]), Ok(vec![0]));
        // O máximo declarado é 2 páginas: o crescimento seguinte devolve -1.
        assert_eq!(call(&instance, "grow", &[1]), Ok(vec![0xffff_ffff]));
        assert_eq!(call(&instance, "size", &[]), Ok(vec![2]));
    }

    fn table_module(table_size: u8) -> Instance {
        // tipos: 0 = () -> i32, 1 = (i32) -> i32. Funções: 0 e 1 devolvem 7 e 9, 2 despacha.
        let seven = [0x00, 0x41, 0x07, 0x0b];
        let nine = [0x00, 0x41, 0x09, 0x0b];
        let dispatch = [0x00, 0x20, 0x00, 0x11, 0x00, 0x00, 0x0b];
        instantiate(
            vec![
                section(1, vector(vec![func_type(&[], &[I32]), func_type(&[I32], &[I32])])),
                section(3, vector(vec![vec![0], vec![0], vec![1]])),
                section(4, vector(vec![vec![0x70, 0x00, table_size]])),
                section(7, vector(vec![export("dispatch", 0, 2)])),
                // elemento ativo: i32.const 0, funções 0 e 1 (e, no segundo teste, a 2).
                section(9, vector(vec![vec![0x00, 0x41, 0x00, 0x0b, 0x03, 0x00, 0x01, 0x02]])),
                section(10, vector(vec![body(&seven), body(&nine), body(&dispatch)])),
            ],
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn call_indirect_dispatches_and_traps() {
        let instance = table_module(4);
        assert_eq!(call(&instance, "dispatch", &[0]), Ok(vec![7]));
        assert_eq!(call(&instance, "dispatch", &[1]), Ok(vec![9]));
        // A função 2 tem o tipo (i32) -> i32, não o esperado () -> i32.
        assert_eq!(
            call(&instance, "dispatch", &[2]),
            Err(WasmError::Runtime("call_indirect to a signature that does not match".to_string()))
        );
        // A entrada nula falha pela assinatura, como no JSC (o tipo da entrada nula nunca casa).
        assert_eq!(
            call(&instance, "dispatch", &[3]),
            Err(WasmError::Runtime("call_indirect to a signature that does not match".to_string()))
        );
        assert_eq!(
            call(&instance, "dispatch", &[4]),
            Err(WasmError::Runtime("Out of bounds call_indirect".to_string()))
        );
    }

    #[test]
    fn call_indirect_passes_arguments_to_the_callee() {
        // tipo 0 = (i32, i32) -> i32. Função 0 subtrai; a 1 chama a 0 pela tabela com 100 e o parâmetro (a pilha
        // do chamador tem só os argumentos e o seletor), e a 2 faz o mesmo com um valor a mais embaixo na pilha.
        let sub = [0x00, 0x20, 0x00, 0x20, 0x01, 0x6b, 0x0b];
        let shallow = [0x00, 0x41, 0xe4, 0x00, 0x20, 0x00, 0x41, 0x00, 0x11, 0x00, 0x00, 0x0b];
        let deep = [0x00, 0x41, 0x05, 0x41, 0xe4, 0x00, 0x20, 0x00, 0x41, 0x00, 0x11, 0x00, 0x00, 0x6a, 0x0b];
        let instance = instantiate(
            vec![
                section(1, vector(vec![func_type(&[I32, I32], &[I32]), func_type(&[I32], &[I32])])),
                section(3, vector(vec![vec![0], vec![1], vec![1]])),
                section(4, vector(vec![vec![0x70, 0x00, 0x01]])),
                section(7, vector(vec![export("shallow", 0, 1), export("deep", 0, 2)])),
                section(9, vector(vec![vec![0x00, 0x41, 0x00, 0x0b, 0x01, 0x00]])),
                section(10, vector(vec![body(&sub), body(&shallow), body(&deep)])),
            ],
            vec![],
        )
        .unwrap();
        assert_eq!(call(&instance, "shallow", &[1]), Ok(vec![99]));
        assert_eq!(call(&instance, "deep", &[1]), Ok(vec![104]));
    }

    #[test]
    fn element_segment_out_of_bounds_fails_instantiation() {
        // Uma tabela de 2 entradas não comporta o segmento de 3.
        let error = instantiate(
            vec![
                section(1, vector(vec![func_type(&[], &[I32])])),
                section(3, vector(vec![vec![0]])),
                section(4, vector(vec![vec![0x70, 0x00, 0x02]])),
                section(9, vector(vec![vec![0x00, 0x41, 0x00, 0x0b, 0x03, 0x00, 0x00, 0x00]])),
                section(10, vector(vec![body(&[0x00, 0x41, 0x01, 0x0b])])),
            ],
            vec![],
        )
        .err();
        assert_eq!(error, Some(WasmError::Runtime("Element is trying to set an out of bounds table index".to_string())));
    }

    #[test]
    fn arithmetic_traps_follow_the_header_messages() {
        let div_s = [0x00, 0x20, 0x00, 0x20, 0x01, 0x6d, 0x0b];
        let rem_u = [0x00, 0x20, 0x00, 0x20, 0x01, 0x70, 0x0b];
        let trunc = [0x00, 0x20, 0x00, 0xa8, 0x0b];
        let unreachable = [0x00, 0x00, 0x0b];
        let instance = instantiate(
            vec![
                section(1, vector(vec![func_type(&[I32, I32], &[I32]), func_type(&[0x7d], &[I32]), func_type(&[], &[])])),
                section(3, vector(vec![vec![0], vec![0], vec![1], vec![2]])),
                section(
                    7,
                    vector(vec![export("div_s", 0, 0), export("rem_u", 0, 1), export("trunc", 0, 2), export("unreachable", 0, 3)]),
                ),
                section(10, vector(vec![body(&div_s), body(&rem_u), body(&trunc), body(&unreachable)])),
            ],
            vec![],
        )
        .unwrap();
        assert_eq!(call(&instance, "div_s", &[(-7i32) as u32 as u64, 2]), Ok(vec![(-3i32) as u32 as u64]));
        assert_eq!(
            call(&instance, "div_s", &[1, 0]),
            Err(WasmError::Runtime("Division by zero".to_string()))
        );
        assert_eq!(
            call(&instance, "div_s", &[i32::MIN as u32 as u64, (-1i32) as u32 as u64]),
            Err(WasmError::Runtime("Integer overflow".to_string()))
        );
        assert_eq!(call(&instance, "rem_u", &[7, 3]), Ok(vec![1]));
        assert_eq!(call(&instance, "rem_u", &[7, 0]), Err(WasmError::Runtime("Division by zero".to_string())));
        assert_eq!(call(&instance, "trunc", &[u64::from(2.9f32.to_bits())]), Ok(vec![2]));
        assert_eq!(
            call(&instance, "trunc", &[u64::from(f32::NAN.to_bits())]),
            Err(WasmError::Runtime("Out of bounds Trunc operation".to_string()))
        );
        assert_eq!(
            call(&instance, "trunc", &[u64::from(3e10f32.to_bits())]),
            Err(WasmError::Runtime("Out of bounds Trunc operation".to_string()))
        );
        assert_eq!(
            call(&instance, "unreachable", &[]),
            Err(WasmError::Runtime("Unreachable code should not be executed".to_string()))
        );
    }

    #[test]
    fn globals_and_the_start_function() {
        // global mutável i32 = 5; start soma 1; "get" lê a global.
        let start = [0x00, 0x23, 0x00, 0x41, 0x01, 0x6a, 0x24, 0x00, 0x0b];
        let get = [0x00, 0x23, 0x00, 0x0b];
        let instance = instantiate(
            vec![
                section(1, vector(vec![func_type(&[], &[]), func_type(&[], &[I32])])),
                section(3, vector(vec![vec![0], vec![1]])),
                section(6, vector(vec![vec![I32, 0x01, 0x41, 0x05, 0x0b]])),
                section(7, vector(vec![export("get", 0, 1), export("g", 3, 0)])),
                section(8, leb(0)),
                section(10, vector(vec![body(&start), body(&get)])),
            ],
            vec![],
        )
        .unwrap();
        assert_eq!(call(&instance, "get", &[]), Ok(vec![6]));
        let Some(ExportValue::Global(global)) = instance.export("g") else { panic!("sem global exportada") };
        assert_eq!(global.borrow().get(), 6);
    }

    #[test]
    fn imported_function_and_mutable_global_are_shared() {
        // import env.double: (i32) -> i32; "run" chama a importada com 21.
        let run = [0x00, 0x41, 0x15, 0x10, 0x00, 0x0b];
        let mut import = name("env");
        import.extend(name("double"));
        import.extend([0x00, 0x00]);
        let host: HostFunction = Rc::new(|arguments: &[u64]| Ok(vec![arguments[0] * 2]));
        let instance = instantiate(
            vec![
                section(1, vector(vec![func_type(&[I32], &[I32]), func_type(&[], &[I32])])),
                section(2, vector(vec![import])),
                section(3, vector(vec![vec![1]])),
                section(7, vector(vec![export("run", 0, 1)])),
                section(10, vector(vec![body(&run)])),
            ],
            vec![ImportValue::Function(host)],
        )
        .unwrap();
        assert_eq!(call(&instance, "run", &[]), Ok(vec![42]));
        assert_eq!(TYPE_I32.kind, crate::wasm::wasm_format::TypeKind::I32);
        assert_eq!(TYPE_I64.kind, crate::wasm::wasm_format::TypeKind::I64);
    }

    #[test]
    fn memory_import_link_errors_use_the_header_text() {
        use crate::wasm::page_count::PageCount;
        use crate::wasm::wasm_address_type::AddressType;
        use crate::wasm::wasm_memory::Memory;
        use std::cell::RefCell;
        let mut import = name("env");
        import.extend(name("mem"));
        import.extend([0x02, 0x00, 0x02]);
        let sections = || vec![section(2, vector(vec![import.clone()]))];
        let small = Memory::try_create(PageCount::new(1), PageCount::default(), false, AddressType::new(false)).unwrap();
        let error = instantiate(sections(), vec![ImportValue::Memory(Rc::new(RefCell::new(small)))]).err();
        assert_eq!(
            error,
            Some(WasmError::Link(
                "Memory import env:mem provided a 'size' that is smaller than the module's declared 'initial' import memory size"
                    .to_string()
            ))
        );
        let enough = Memory::try_create(PageCount::new(2), PageCount::default(), false, AddressType::new(false)).unwrap();
        assert!(instantiate(sections(), vec![ImportValue::Memory(Rc::new(RefCell::new(enough)))]).is_ok());
    }

    #[test]
    fn runaway_recursion_overflows_the_stack() {
        // (func (call 0))
        let instance = instantiate(
            vec![
                section(1, vector(vec![func_type(&[], &[])])),
                section(3, vector(vec![vec![0]])),
                section(7, vector(vec![export("loop", 0, 0)])),
                section(10, vector(vec![body(&[0x00, 0x10, 0x00, 0x0b])])),
            ],
            vec![],
        )
        .unwrap();
        assert_eq!(call(&instance, "loop", &[]), Err(WasmError::Runtime("Stack overflow".to_string())));
        assert_eq!(instance.call_depth.get(), 0);
    }
}
