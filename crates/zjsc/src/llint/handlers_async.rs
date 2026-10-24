//! Handlers de iteração assíncrona e de promessa: `op_async_iterator_open`, `op_async_iterator_next`,
//! `op_new_promise` e `op_create_promise` (`LLIntSlowPaths.cpp`, `CommonSlowPaths.cpp` e os handlers `.asm`
//! de `iteratorOpenGenericImpl`/`op_async_iterator_next`).
//!
//! O ponto de entrada é [`run_async`], no mesmo formato de `dispatch_ext::run_ext`.
//!
//! DIVERGÊNCIAS:
//!
//! - Os modos rápidos de iterador assíncrono que existem são `AsyncFromSync` (sem `@@asyncIterator`: o
//!   iterável síncrono é embrulhado, e a sentinela do `fastAsyncGeneratorSentinel` em `next` só vale com a
//!   espécie de `Promise` primordial; senão `next` é o `asyncFromSyncIteratorPrototypeNextFunction`) e
//!   `FastAsyncGenerator` (um `JSAsyncGenerator` com o `@@asyncIterator` e o `next` originais, lido com
//!   `VMInquiry`). `op_async_iterator_next` com a sentinela roda o `asyncIteratorNextWithDriver`. Sem
//!   sentinela, o genérico: `op_async_iterator_open` NÃO valida o `Symbol.asyncIterator` (vai direto à
//!   chamada, e não chamável é `createNotAFunctionError`), confere que o iterador é objeto e lê `next`
//!   (`open_iterator_call`), e `op_async_iterator_next` chama `next` com `this = iterator` e, quando
//!   `m_hasValue`, o valor de retomada (`resumeValueOperandFor`) como único argumento.
//! - `createAsyncFromSyncIteratorForIterable` não tem os modos rápidos do `getIterationMode` (o `FastArray`
//!   e companhia do `fastSyncIteratorForIterable`): o invólucro é sempre `Generic`, chama `@@iterator` e lê
//!   `next` uma vez, o mesmo observável com o protocolo intacto. A espécie de `Promise` é conferida por
//!   `promise_species_is_watched` (não há `promiseSpeciesWatchpointSet`).
//! - `op_create_promise` lê o `callee` por `ObjectRef::from_value`, que alcança `JSFunction` (o `get` de
//!   `prototype` materializa o `reifyLazyPrototype`); o `asObject(callee)` do C++ é invariante e vira
//!   `expect`. Sem a metadata `m_cachedCallee`.

use crate::runtime::js_promise_host::PromiseHost;
use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::bytecode::bytecode_operands_for_checkpoint::resume_value_operand_for;
use crate::bytecode::bytecode_ops::{OpAsyncIteratorNext, OpAsyncIteratorOpen, OpCreatePromise, OpNewPromise};
use crate::bytecode::instruction_stream::Ref as InstructionRef;
use crate::bytecode::op_metadata::{IterationMode, OpAsyncIteratorNextMetadata, OpAsyncIteratorOpenMetadata};
use crate::bytecode::opcode::OpcodeID;
use crate::llint::dispatch::Step;
use crate::llint::dispatch_ext::check_exception;
use crate::llint::handlers_iterator::{can_use_fast_iteration_mode, open_iterator_call, record_seen_mode};
use crate::llint::slow_paths::{throw_error_object, thrown_failure};
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::llint::slow_paths_object::Ctx;
use crate::llint::LLIntResult;
use crate::runtime::call_data::{call, get_call_data};
use crate::runtime::exception_helpers::create_not_a_function_error;
use crate::runtime::internal_function::InternalFunction;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::iterator_operations::create_async_from_sync_iterator_for_iterable;
use crate::runtime::js_async_generator::JSAsyncGenerator;
use crate::runtime::js_microtask_async::{async_iterator_next_with_driver, promise_species_is_watched};
use crate::runtime::js_object::JSObject;
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};

/// `asyncIteratorOpenTryFastImpl`, o trecho do gerador assíncrono: `iterable` é um `JSAsyncGenerator`, o
/// `@@asyncIterator` é o `%AsyncIteratorPrototype%[@@asyncIterator]` original e o `next` (lido com
/// `VMInquiry`, sem getter nem efeito) é o `%AsyncGeneratorPrototype%.next` original.
fn is_primordial_async_generator(ctx: &Ctx, iterable: JSValue, symbol_iterator: JSValue) -> bool {
    if JSAsyncGenerator::from_value(&iterable).is_none()
        || symbol_iterator != ctx.global_object.link_time_constant(LinkTimeConstant::AsyncIteratorPrototypeSymbolAsyncIterator)
    {
        return false;
    }
    // Invariante: o `from_value` acima já provou que é `JSAsyncGenerator`, que é objeto.
    let object = JSObject::from_value(&iterable).expect("o JSAsyncGenerator é objeto");
    let mut slot = PropertySlot::new(iterable, InternalMethodType::VMInquiry);
    let found = object.get_property_slot(ctx.vm, &PropertyName::from_identifier(&ctx.vm.property_names.next), &mut slot);
    found
        && slot.is_value()
        && slot.get_value() == ctx.global_object.link_time_constant(LinkTimeConstant::AsyncGeneratorPrototypeNext)
}

/// `op_async_iterator_open`: o `asyncIteratorOpenTryFastImpl` e, sem ele, o genérico (`iteratorOpenGenericImpl`
/// sem o `validateIterable`: um `@@asyncIterator` que não é função falha na chamada).
fn async_iterator_open(f: &mut SlowPathFrame, op: &OpAsyncIteratorOpen, metadata_id: u32) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let iterable = f.get(op.iterable);
    let symbol_iterator = f.get(op.symbol_iterator);

    // Sem `@@asyncIterator`, o iterador síncrono é embrulhado no `%AsyncFromSyncIteratorPrototype%`. A
    // sentinela funde o `Await` do consumidor, o que elide um `PromiseResolve` observável: só vale com a
    // espécie primordial.
    if symbol_iterator.is_undefined_or_null() {
        let wrapper = create_async_from_sync_iterator_for_iterable(ctx.global_object, iterable)
            .map_err(|thrown| thrown_failure(ctx.global_object, thrown))?;
        record_seen_mode::<OpAsyncIteratorOpenMetadata>(f, metadata_id, IterationMode::AsyncFromSync);
        f.set(op.iterator, wrapper.as_value());
        let next = if promise_species_is_watched(ctx.global_object) {
            ctx.vm.fast_async_generator_sentinel()
        } else {
            ctx.global_object.async_from_sync_iterator_prototype_next_function()
        };
        f.set(op.next, next);
        return Ok(());
    }

    if is_primordial_async_generator(&ctx, iterable, symbol_iterator)
        && can_use_fast_iteration_mode::<OpAsyncIteratorOpenMetadata>(f, metadata_id, IterationMode::FastAsyncGenerator)
        && promise_species_is_watched(ctx.global_object)
    {
        record_seen_mode::<OpAsyncIteratorOpenMetadata>(f, metadata_id, IterationMode::FastAsyncGenerator);
        f.set(op.iterator, iterable);
        f.set(op.next, ctx.vm.fast_async_generator_sentinel());
        return Ok(());
    }

    record_seen_mode::<OpAsyncIteratorOpenMetadata>(f, metadata_id, IterationMode::Generic);
    open_iterator_call(f, (op.iterable, op.symbol_iterator, op.iterator, op.next), op.stack_offset)
}

/// `op_async_iterator_next`: com a sentinela em `next`, o `slow_path_async_iterator_next_with_driver`; sem ela,
/// o genérico: `next.call(iterator, resumeValue?)`.
fn async_iterator_next(f: &mut SlowPathFrame, op: &OpAsyncIteratorNext, metadata_id: u32) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let next = f.get(op.next);
    if next == ctx.vm.fast_async_generator_sentinel() {
        record_seen_mode::<OpAsyncIteratorNextMetadata>(f, metadata_id, IterationMode::FastAsyncGenerator);
        let resume_value = if op.has_value { f.get(resume_value_operand_for(op)) } else { JSValue::empty() };
        let result = async_iterator_next_with_driver(ctx.global_object, f.get(op.iterator), f.get(op.driver), resume_value);
        f.set(op.dst, result);
        return Ok(());
    }

    record_seen_mode::<OpAsyncIteratorNextMetadata>(f, metadata_id, IterationMode::Generic);
    let call_data = get_call_data(next);
    if call_data.is_none() {
        let site = f.error_site();
        return Err(throw_error_object(ctx.global_object, create_not_a_function_error(ctx.global_object, next, Some(&site))));
    }

    let arguments: Vec<JSValue> = if op.has_value { vec![f.get(resume_value_operand_for(op))] } else { Vec::new() };
    let frame_base = crate::llint::handlers_iterator::llint_call_frame_base(f, op.stack_offset)?;
    let result = crate::runtime::call_data::call_from_llint(ctx.global_object, frame_base, next, &call_data, f.get(op.iterator), &arguments)?;
    f.set(op.dst, result);
    Ok(())
}

/// `slow_path_new_promise`: `JSPromise::create(vm, globalObject->promiseStructure())`.
fn new_promise(f: &mut SlowPathFrame, op: &OpNewPromise) {
    let ctx = Ctx::new(f);
    let promise = JSPromise::create(ctx.vm, &ctx.global_object.promise_structure());
    f.set(op.dst, promise.as_value());
}

/// `slow_path_create_promise`: a estrutura vem de `JSC_GET_DERIVED_STRUCTURE(vm, promiseStructure, callee,
/// globalObject->promiseConstructor())`.
fn create_promise(f: &mut SlowPathFrame, op: &OpCreatePromise) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let callee = f.get(op.callee);
    let base = ctx.global_object.promise_structure();
    let structure = if callee == ctx.global_object.promise_constructor() {
        base
    } else {
        let constructor = ObjectRef::from_value(&callee).expect("ASSERT: asObject(GET(bytecode.m_callee).jsValue())");
        InternalFunction::create_subclass_structure(ctx.global_object, &constructor, base)
            .map_err(|thrown| crate::llint::slow_paths::thrown_failure(ctx.global_object, thrown))?
    };
    check_exception(f)?;
    f.set(op.dst, JSPromise::create(ctx.vm, &structure).as_value());
    Ok(())
}

/// Os handlers deste arquivo. `None` é o opcode que não é daqui.
pub(super) fn run_async(f: &mut SlowPathFrame, instruction: &InstructionRef) -> LLIntResult<Option<Step>> {
    match instruction.opcode_id_enum() {
        OpcodeID::op_async_iterator_open => {
            async_iterator_open(f, &instruction.as_op::<OpAsyncIteratorOpen>(), instruction.metadata_id())?
        }
        OpcodeID::op_async_iterator_next => {
            async_iterator_next(f, &instruction.as_op::<OpAsyncIteratorNext>(), instruction.metadata_id())?
        }
        OpcodeID::op_new_promise => new_promise(f, &instruction.as_op::<OpNewPromise>()),
        OpcodeID::op_create_promise => create_promise(f, &instruction.as_op::<OpCreatePromise>())?,
        _ => return Ok(None),
    }
    Ok(Some(Step::Next))
}
