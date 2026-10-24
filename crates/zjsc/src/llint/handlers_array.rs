//! Handlers de criação de array: `op_new_array`, `op_new_array_with_size`, `op_new_array_buffer` e
//! `op_new_array_with_species` (`LLIntSlowPaths.cpp` e `CommonSlowPaths.cpp`). `op_new_array_with_spread` está
//! em `handlers_iterator` e usa o [`allocate_new_array_buffer`] daqui.
//!
//! O ponto de entrada é [`run_array`], no mesmo formato de `dispatch_ext::run_ext`.
//!
//! Os quatro ops com `ArrayAllocationProfile` leem e gravam o metadata do op (`bytecode.metadata(codeBlock)`,
//! pelo `m_metadataID` da instrução): a `Structure` do array nasce de `profile.selectIndexingType()` e o array
//! criado vira o `m_lastArray` do perfil (`updateLastAllocationFor`), que é o que muda o indexing type na
//! próxima alocação do mesmo op. O metadata só é tomado emprestado dentro de cada passo, nunca durante o
//! `speciesConstructArray` (que roda código de usuário e pode reentrar no mesmo `CodeBlock`).
//!
//! DIVERGÊNCIAS:
//!
//! - `op_new_array_buffer` não compartilha o butterfly copy-on-write do `JSCellButterfly` (`allocateNewArrayBuffer`):
//!   o `JSCellButterfly` do porte tem armazenamento próprio, então os elementos são copiados para um array novo
//!   na `Structure` que o perfil escolheu (a forma é a mesma do array copy-on-write, que converte na primeira
//!   escrita). O C++ recria o butterfly constante quando o `indexingMode` dele difere do do perfil
//!   (`codeBlock->constantRegister(...).set`); como o array sai sempre na forma do perfil, essa recriação não é
//!   observável e não existe aqui.
//! - O `ValueProfile` de `op_new_array_with_species` (`RETURN_PROFILED`) só alimenta o JIT e não existe, como nos
//!   demais slow paths do porte; o `ArrayProfile` (`observeStructureID`) é gravado.

use crate::bytecode::bytecode_ops::{OpNewArray, OpNewArrayBuffer, OpNewArrayWithSize, OpNewArrayWithSpecies};
use crate::bytecode::array_allocation_profile::HeapArrays;
use crate::bytecode::instruction_stream::Ref as InstructionRef;
use crate::bytecode::op_metadata::{
    OpNewArrayBufferMetadata, OpNewArrayMetadata, OpNewArrayWithSizeMetadata, OpNewArrayWithSpeciesMetadata,
};
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::llint::dispatch::Step;
use crate::llint::dispatch_ext::check_exception;
use crate::llint::handlers_object::array_failure;
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::llint::slow_paths_object::Ctx;
use crate::llint::LLIntResult;
use crate::runtime::array_constructor::construct_array_with_size_quirk_with_profile;
use crate::runtime::array_prototype::new_array_with_species;
use crate::runtime::js_array::{construct_array, JSArray};
use crate::runtime::js_cell_butterfly::JSCellButterfly;
use crate::runtime::js_global_object_inlines::{construct_array_negative_indexed, construct_empty_array, heap_ref};
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;
use crate::wtf::math_extras::truncate_double_to_uint64;

/// `slow_path_new_array`: `constructArrayNegativeIndexed(globalObject, &metadata.m_arrayAllocationProfile, &argv,
/// argc)`; o elemento `i` está no registrador `argv - i`.
fn new_array(f: &mut SlowPathFrame, op: &OpNewArray, metadata_id: u32) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let values: Vec<JSValue> = (0..op.argc as i32).map(|i| f.get(VirtualRegister::new(op.argv.offset() - i))).collect();
    let array = f
        .code_block
        .with_metadata::<OpNewArrayMetadata, _>(metadata_id, |metadata| {
            construct_array_negative_indexed(ctx.vm, ctx.global_object, Some(&mut metadata.array_allocation_profile), &values)
        })
        .map_err(|error| array_failure(&ctx, error))?;
    f.set(op.dst, array.as_value());
    Ok(())
}

/// `slow_path_new_array_with_size`: `constructArrayWithSizeQuirk(globalObject, &metadata.m_arrayAllocationProfile,
/// getOperand(callFrame, bytecode.m_length))`.
fn new_array_with_size(f: &mut SlowPathFrame, op: &OpNewArrayWithSize, metadata_id: u32) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let length = f.get(op.length);
    let result = f
        .code_block
        .with_metadata::<OpNewArrayWithSizeMetadata, _>(metadata_id, |metadata| {
            construct_array_with_size_quirk_with_profile(ctx.vm, ctx.global_object, Some(&mut metadata.array_allocation_profile), length)
        })
        .map_err(|error| array_failure(&ctx, error))?;
    f.set(op.dst, result);
    Ok(())
}

/// `CommonSlowPaths::allocateNewArrayBuffer(vm, structure, immutableButterfly)`: o array com os elementos do
/// `JSCellButterfly`, na forma de `structure` (a original do `indexingMode` do butterfly, ou a de
/// `SlowPutArrayStorage` quando o realm está em `haveABadTime`, que é o `switchToSlowPutArrayStorage` do C++).
pub(super) fn allocate_new_array_buffer(vm: &VM, structure: &StructureRef, butterfly: &JSCellButterfly) -> JSArray {
    let values: Vec<JSValue> = (0..butterfly.public_length()).map(|index| butterfly.get(index)).collect();
    construct_array(vm, structure, &values)
}

/// `slow_path_new_array_buffer`: o array com os elementos do `JSCellButterfly` imutável do operando constante,
/// na `Structure` de `profile.selectIndexingType()`.
fn new_array_buffer(f: &mut SlowPathFrame, op: &OpNewArrayBuffer, metadata_id: u32) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let value = f.get(op.immutable_butterfly);
    let butterfly = value
        .is_cell()
        .then(|| JSCellButterfly::from_cell_id(value.as_cell()))
        .flatten()
        .expect("ASSERT: o operando constante de new_array_buffer é um JSCellButterfly");

    let structure = f.code_block.with_metadata::<OpNewArrayBufferMetadata, _>(metadata_id, |metadata| {
        let indexing_mode = metadata.array_allocation_profile.select_indexing_type(&HeapArrays);
        ctx.global_object.array_structure_for_indexing_type_during_allocation(indexing_mode)
    });
    let result = allocate_new_array_buffer(ctx.vm, &structure, &butterfly);
    f.code_block.with_metadata::<OpNewArrayBufferMetadata, _>(metadata_id, |metadata| {
        metadata.array_allocation_profile.update_last_allocation(heap_ref(&result));
    });
    f.set(op.dst, result.as_value());
    Ok(())
}

/// `slow_path_new_array_with_species`: `arrayProfile.observeStructureID(array->structureID())`, depois
/// `speciesConstructArray(globalObject, array, length)` com
/// `length = truncateDoubleToUint64(m_length.asNumber())` e, no `FastPath`,
/// `constructEmptyArray(globalObject, &arrayAllocationProfile, length)`.
fn new_array_with_species_op(f: &mut SlowPathFrame, op: &OpNewArrayWithSpecies, metadata_id: u32) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let code_block = f.code_block;
    let array = f.get(op.array);
    // Invariante (ASSERT do C++): o builtin só emite `new_array_with_species` com um objeto aqui.
    let object = JSObject::from_value(&array).expect("ASSERT: asObject(GET_C(bytecode.m_array))");
    let length = truncate_double_to_uint64(f.get(op.length).as_number());

    let structure_id = object.cell().structure_id();
    code_block.with_metadata::<OpNewArrayWithSpeciesMetadata, _>(metadata_id, |metadata| {
        metadata.array_profile.observe_structure_id(structure_id);
    });
    let result = new_array_with_species(ctx.global_object, array, length, |initial_length| {
        code_block.with_metadata::<OpNewArrayWithSpeciesMetadata, _>(metadata_id, |metadata| {
            construct_empty_array(ctx.vm, ctx.global_object, Some(&mut metadata.array_allocation_profile), initial_length)
        })
    })
    .map_err(|error| array_failure(&ctx, error))?;
    check_exception(f)?;
    f.set(op.dst, result);
    Ok(())
}

/// Os handlers deste arquivo. `None` é o opcode que não é daqui.
pub(super) fn run_array(f: &mut SlowPathFrame, instruction: &InstructionRef) -> LLIntResult<Option<Step>> {
    match instruction.opcode_id_enum() {
        OpcodeID::op_new_array => new_array(f, &instruction.as_op::<OpNewArray>(), instruction.metadata_id())?,
        OpcodeID::op_new_array_with_size => {
            new_array_with_size(f, &instruction.as_op::<OpNewArrayWithSize>(), instruction.metadata_id())?
        }
        OpcodeID::op_new_array_buffer => {
            new_array_buffer(f, &instruction.as_op::<OpNewArrayBuffer>(), instruction.metadata_id())?
        }
        OpcodeID::op_new_array_with_species => {
            new_array_with_species_op(f, &instruction.as_op::<OpNewArrayWithSpecies>(), instruction.metadata_id())?
        }
        _ => return Ok(None),
    }
    Ok(Some(Step::Next))
}
