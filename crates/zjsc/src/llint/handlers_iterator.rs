//! Handlers de iteração e espalhamento: `op_iterator_open`, `op_iterator_next`, `op_spread` e
//! `op_new_array_with_spread` (`CommonSlowPaths.cpp`, `LLIntSlowPaths.cpp` e os handlers `.asm` de
//! `iteratorOpenGenericImpl`/`op_iterator_next`).
//!
//! O ponto de entrada é [`run_iterator`], no mesmo formato de `dispatch_ext::run_ext`.
//!
//! DIVERGÊNCIAS:
//!
//! - O `IterationMode` rápido segue o `getIterationMode` (`runtime/iterator_operations.rs`): `FastArray`,
//!   `FastMap`, `FastSet`, `FastString` (o iterador primordial é criado direto e a sentinela correspondente vai
//!   em `next`) e os de iterador já aberto (`FastArrayValues`/`Keys`/`Entries`, `FastMapKeys`..., o próprio
//!   iterável vira `iterator`). `op_iterator_next` com sentinela avança direto (`iteratorNextTryFastImpl`),
//!   com o `canUseFastIterationMode` (no máximo dois modos rápidos por site) lido do `seenModes` do metadata.
//!   Não há watchpoint no porte: a validade do protocolo é a conferência direta de `next` do protótipo do
//!   iterador contra o valor original (`JSGlobalObject::*_iterator_protocol_is_intact`, ver
//!   `runtime/iteration_protocol.rs`), e o `@@iterator` é comparado por identidade com a função original. O
//!   `Generic` (também o do watchpoint invalidado) valida o
//!   iterável (`validateIterable`), chama `iterable[Symbol.iterator]()` (o `callHelper` do `.asm`), confere
//!   que o iterador é objeto e lê `next` (`slow_path_iterator_open_get_next`). `op_iterator_next` chama
//!   `next` e lê `done` e `value` (`slow_path_iterator_next_get_done`/`get_value`).
//! - O `PROFILE_VALUE_IN` do `iteratorOpenTryFastImpl` e do `iteratorNextTryFastImpl` não existe.//! - `op_spread` não tem o `trySpreadFast` (cópia de `JSArray` com protocolo intacto, o mesmo resultado do
//!   genérico): coleta com `for_each_in_iterable`, que é o `iteratorProtocolFunction` do C++, e empacota o
//!   array num `JSCellButterfly` (`createFromArray`). O iterável primitivo (`String`) passa por
//!   `iterator_operations`, que lê `Symbol.iterator` do protótipo do invólucro (`JSValue::toObject`) com o
//!   primitivo como receptor. As mensagens de erro de `undefined`/`null` e de
//!   não-iterável vêm de `iterator_operations`, não do `iteratorProtocolFunction`.
//! - `op_new_array_with_spread` não tem `ArrayAllocationProfile` no C++: o array sai sempre na `Structure` de
//!   `ArrayWithContiguous` do realm (e o atalho de um único espalhamento vira o array copy-on-write, copiado
//!   por `allocate_new_array_buffer` de `handlers_array`, como em `op_new_array_buffer`).
//! - `op_async_iterator_*` estão em `handlers_async`, que reusa [`open_iterator_call`] e os helpers de
//!   `seenModes` daqui.

use crate::bytecode::array_allocation_profile::MIN_ARRAY_STORAGE_CONSTRUCTION_LENGTH;
use crate::bytecode::bytecode_ops::{OpIteratorNext, OpIteratorOpen, OpNewArrayWithSpread, OpSpread};
use crate::bytecode::instruction_stream::Ref as InstructionRef;
use crate::bytecode::metadata_table::MetadataFor;
use crate::bytecode::op_metadata::{
    IterationMode, IterationModeMetadata, OpAsyncIteratorNextMetadata, OpAsyncIteratorOpenMetadata, OpIteratorNextMetadata,
    OpIteratorOpenMetadata,
};
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::llint::dispatch::Step;
use crate::llint::handlers_array::allocate_new_array_buffer;
use crate::runtime::indexing_type::{is_copy_on_write, ARRAY_WITH_CONTIGUOUS, COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS};
use crate::llint::slow_paths::{throw_error_object, throw_out_of_memory_error, throw_stack_overflow_error, throw_type_error, thrown_failure};
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::llint::slow_paths_control::{
    get_js_function, slow_path_iterator_next_get_done, slow_path_iterator_next_get_value,
    slow_path_iterator_open_get_next,
};
use crate::llint::slow_paths_object::Ctx;
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::call_data::{call, call_from_llint, get_call_data};
use crate::runtime::collection_support::iterator_step_value;
use crate::runtime::exception_helpers::create_not_a_function_error;
use crate::runtime::host_call::Thrown;
use crate::runtime::iterator_operations::{for_each_in_iterable, get_iteration_mode};
use crate::runtime::property_offset::INVALID_OFFSET;
use crate::runtime::property_name::PropertyName;
use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::js_array::{construct_array, is_js_array, JSArray};
use crate::runtime::js_array_iterator::JSArrayIterator;
use crate::runtime::js_cell_butterfly::{JSCellButterfly, JSCellButterflyRef, MAXIMUM_LENGTH};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_map::{JSMap, JSMapIterator};
use crate::runtime::js_object::JSObject;
use crate::runtime::js_set::{JSSet, JSSetIterator};
use crate::runtime::js_string_iterator::JSStringIterator;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::runtime::symbol::Symbol;
use crate::runtime::property_attribute::{ACCESSOR, CUSTOM_ACCESSOR, CUSTOM_VALUE};
use crate::runtime::vm::VM;

/// `getIteratorErrorMessage(validateIterable(vm, iterable, symbolIterator), iterable)` para o iterável que
/// não tem `Symbol.iterator` chamável.
pub(crate) fn not_iterable_message(iterable: JSValue) -> String {
    // O V8 escreve o número (`5 is not iterable`, medido no Chromium 154).
    if iterable.is_number() && crate::runtime::stack_frame::v8_stack_format() {
        return format!("{} is not iterable", crate::runtime::v8_stack::number_text(iterable.as_number()));
    }
    jsc_not_iterable_message(iterable).to_string()
}

fn jsc_not_iterable_message(iterable: JSValue) -> &'static str {
    if iterable.is_number() {
        return "number is not iterable";
    }
    match iterable {
        JSValue::Bool(true) => return "true is not iterable",
        JSValue::Bool(false) => return "false is not iterable",
        JSValue::Null => return "null is not an object",
        JSValue::Undefined => return "undefined is not an object",
        _ => {}
    }
    if iterable.is_cell() && Symbol::from_cell_id(iterable.as_cell()).is_some() {
        return "value is not iterable";
    }
    if JSObject::from_value(&iterable).is_some() || get_js_function(iterable).is_some() {
        return "{} is not iterable";
    }
    "value is not iterable"
}

/// `validateIterable(vm, iterable, symbolIterator)` seguido de `getIteratorErrorMessage`: o `Symbol.iterator`
/// tem de ser chamável (só o `op_iterator_open`: o `op_async_iterator_open` vai direto à chamada).
fn validate_iterable(global_object: &JSGlobalObject, iterable: JSValue, symbol_iterator: JSValue) -> LLIntResult<()> {
    if get_call_data(symbol_iterator).is_none() {
        return Err(throw_type_error(global_object, &not_iterable_message(iterable)));
    }
    Ok(())
}

/// O `calleeFrame = cfr - m_stackOffset` do `prepareForRegularCall` no `callHelper`.
pub(super) fn llint_call_frame_base(f: &SlowPathFrame, stack_offset: u32) -> LLIntResult<usize> {
    f.call_frame.registers().checked_sub(stack_offset as usize).ok_or_else(|| throw_stack_overflow_error(&Ctx::new(f).global_object))
}

/// O `callHelper` de `iteratorOpenGenericImpl` (de `op_iterator_open` e de `op_async_iterator_open`, que só
/// diferem no tipo do op): chama o `Symbol.iterator` com o iterável como `this` (não chamável é o
/// `createNotAFunctionError` do `handleHostCall`) e lê `next` do iterador. Os registradores são `iterable`,
/// `symbol_iterator`, `iterator` e `next`.
pub(super) fn open_iterator_call(
    f: &mut SlowPathFrame,
    (iterable, symbol_iterator, iterator, next): (VirtualRegister, VirtualRegister, VirtualRegister, VirtualRegister),
    stack_offset: u32,
) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let iterable = f.get(iterable);
    let symbol_iterator = f.get(symbol_iterator);

    let call_data = get_call_data(symbol_iterator);
    if call_data.is_none() {
        // O erro leva o trecho do fonte da instrução em curso (o `topCallFrame` do `ErrorInstance`), e não o do
        // chamador do código: sem o `site`, o `for await` dentro de um `eval` citava o `(0, eval)(src)`.
        let site = f.error_site();
        return Err(throw_error_object(
            ctx.global_object,
            create_not_a_function_error(ctx.global_object, symbol_iterator, Some(&site)),
        ));
    }

    let frame_base = llint_call_frame_base(f, stack_offset)?;
    let result = call_from_llint(ctx.global_object, frame_base, symbol_iterator, &call_data, iterable, &[])?;
    f.set(iterator, result);
    slow_path_iterator_open_get_next(f, iterator, next).map_err(|_| LLIntFailure::Thrown)
}

/// `OpIteratorOpen::Metadata`, `OpIteratorNext::Metadata`, `OpAsyncIteratorOpen::Metadata` e
/// `OpAsyncIteratorNext::Metadata` têm todos um `m_iterationMetadata`.
pub(super) trait HasIterationMetadata {
    fn iteration_metadata_mut(&mut self) -> &mut IterationModeMetadata;
}

macro_rules! has_iteration_metadata {
    ($($metadata:ty),* $(,)?) => {$(
        impl HasIterationMetadata for $metadata {
            fn iteration_metadata_mut(&mut self) -> &mut IterationModeMetadata {
                &mut self.iteration_metadata
            }
        }
    )*};
}

has_iteration_metadata!(OpIteratorOpenMetadata, OpIteratorNextMetadata, OpAsyncIteratorOpenMetadata, OpAsyncIteratorNextMetadata);

/// `maxNumberOfFastIterationModes` (`IterationModeMetadata.h`).
const MAX_NUMBER_OF_FAST_ITERATION_MODES: u32 = 2;

/// `canUseFastIterationMode(metadata.m_iterationMetadata.seenModes, mode)`: o site aceita no máximo dois
/// modos rápidos distintos; passado disso, o iterável novo é tratado como `Generic`.
pub(super) fn can_use_fast_iteration_mode<M: MetadataFor + HasIterationMetadata>(
    f: &SlowPathFrame,
    metadata_id: u32,
    mode: IterationMode,
) -> bool {
    debug_assert!(mode != IterationMode::Generic);
    let seen_modes = f.code_block.with_metadata::<M, _>(metadata_id, |metadata| metadata.iteration_metadata_mut().seen_modes);
    let seen_fast_modes = seen_modes & !(IterationMode::Generic as u16);
    seen_fast_modes & (mode as u16) != 0 || seen_fast_modes.count_ones() < MAX_NUMBER_OF_FAST_ITERATION_MODES
}

/// `metadata.m_iterationMetadata.seenModes = metadata.m_iterationMetadata.seenModes | mode`.
pub(super) fn record_seen_mode<M: MetadataFor + HasIterationMetadata>(f: &SlowPathFrame, metadata_id: u32, mode: IterationMode) {
    f.code_block.with_metadata::<M, _>(metadata_id, |metadata| metadata.iteration_metadata_mut().seen_modes |= mode as u16);
}

/// A sentinela que `iteratorOpenTryFastImpl` grava em `next` para o modo rápido `mode` (`vm.fast*Sentinel()`):
/// o modo de contêiner (`FastMap`...) usa a do iterador que ele cria.
fn sentinel_for_mode(vm: &VM, mode: IterationMode) -> JSValue {
    match mode {
        IterationMode::FastArray | IterationMode::FastArrayValues => vm.fast_array_values_sentinel(),
        IterationMode::FastArrayKeys => vm.fast_array_keys_sentinel(),
        IterationMode::FastArrayEntries => vm.fast_array_entries_sentinel(),
        IterationMode::FastMap | IterationMode::FastMapEntries => vm.fast_map_entries_sentinel(),
        IterationMode::FastMapKeys => vm.fast_map_keys_sentinel(),
        IterationMode::FastMapValues => vm.fast_map_values_sentinel(),
        IterationMode::FastSet | IterationMode::FastSetValues => vm.fast_set_values_sentinel(),
        IterationMode::FastSetEntries => vm.fast_set_entries_sentinel(),
        IterationMode::FastString => vm.fast_string_values_sentinel(),
        IterationMode::Generic | IterationMode::FastAsyncGenerator | IterationMode::AsyncFromSync => {
            unreachable!("RELEASE_ASSERT_NOT_REACHED: sentinela de {mode:?}")
        }
    }
}

/// `op_iterator_open`: o `iteratorOpenTryFastImpl` (todos os modos `Fast*` de `getIterationMode`) e, sem ele
/// (ou com o `canUseFastIterationMode` esgotado), o `IterationMode::Generic`.
fn iterator_open(f: &mut SlowPathFrame, op: &OpIteratorOpen, metadata_id: u32) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let iterable = f.get(op.iterable);
    let symbol_iterator = f.get(op.symbol_iterator);

    let mut iteration_mode = get_iteration_mode(ctx.global_object, iterable, symbol_iterator);
    if iteration_mode != IterationMode::Generic
        && !can_use_fast_iteration_mode::<OpIteratorOpenMetadata>(f, metadata_id, iteration_mode)
    {
        iteration_mode = IterationMode::Generic;
    }

    let global_object = ctx.global_object;
    let vm = ctx.vm;
    let iterator = match iteration_mode {
        IterationMode::FastArray => {
            let iterated_object = JSObject::from_value(&iterable).expect("isJSArray(iterable)");
            JSArrayIterator::create(vm, &global_object.array_iterator_structure(), &iterated_object, IterationKind::Values).as_value()
        }
        IterationMode::FastMap => {
            let map = JSMap::from_value(&iterable).expect("FastMap sem JSMap");
            JSMapIterator::create(vm, &global_object.map_iterator_structure(), &map, IterationKind::Entries).as_value()
        }
        IterationMode::FastSet => {
            let set = JSSet::from_value(&iterable).expect("FastSet sem JSSet");
            JSSetIterator::create(vm, &global_object.set_iterator_structure(), &set, IterationKind::Values).as_value()
        }
        IterationMode::FastString => {
            JSStringIterator::create(vm, &global_object.string_iterator_structure(), &iterable.as_js_string()).as_value()
        }
        // O iterável já é um iterador primordial: o `@@iterator` devolveria ele mesmo.
        IterationMode::FastArrayValues
        | IterationMode::FastArrayKeys
        | IterationMode::FastArrayEntries
        | IterationMode::FastMapKeys
        | IterationMode::FastMapValues
        | IterationMode::FastMapEntries
        | IterationMode::FastSetValues
        | IterationMode::FastSetEntries => iterable,
        IterationMode::FastAsyncGenerator | IterationMode::AsyncFromSync => {
            unreachable!("RELEASE_ASSERT_NOT_REACHED: iterator_open com {iteration_mode:?}")
        }
        IterationMode::Generic => {
            validate_iterable(global_object, iterable, symbol_iterator)?;
            record_seen_mode::<OpIteratorOpenMetadata>(f, metadata_id, IterationMode::Generic);
            return open_iterator_call(f, (op.iterable, op.symbol_iterator, op.iterator, op.next), op.stack_offset);
        }
    };
    record_seen_mode::<OpIteratorOpenMetadata>(f, metadata_id, iteration_mode);
    f.set(op.next, sentinel_for_mode(vm, iteration_mode));
    f.set(op.iterator, iterator);
    Ok(())
}

/// `IterationMode` de um `JSArrayIterator` pelo `IterationKind` (o `switch (kind)` do `iteratorNextTryFastImpl`).
fn array_iteration_mode(kind: IterationKind) -> IterationMode {
    match kind {
        IterationKind::Values => IterationMode::FastArrayValues,
        IterationKind::Keys => IterationMode::FastArrayKeys,
        IterationKind::Entries => IterationMode::FastArrayEntries,
    }
}

/// O mesmo `switch (kind)` para o `JSMapIterator`.
fn map_iteration_mode(kind: IterationKind) -> IterationMode {
    match kind {
        IterationKind::Keys => IterationMode::FastMapKeys,
        IterationKind::Values => IterationMode::FastMapValues,
        IterationKind::Entries => IterationMode::FastMapEntries,
    }
}

/// O mesmo `switch (kind)` para o `JSSetIterator` (`keys` e `values` são o mesmo modo).
fn set_iteration_mode(kind: IterationKind) -> IterationMode {
    match kind {
        IterationKind::Keys | IterationKind::Values => IterationMode::FastSetValues,
        IterationKind::Entries => IterationMode::FastSetEntries,
    }
}

/// Grava `done` e `value` de um passo rápido (`value` vazio ao esgotar, como o `JSValue()` do C++).
fn set_fast_step(f: &mut SlowPathFrame, op: &OpIteratorNext, step: Option<JSValue>) {
    f.set(op.done, JSValue::Bool(step.is_none()));
    f.set(op.value, step.unwrap_or_else(JSValue::empty));
}

/// `iteratorNextTryFastImpl`: o iterador rápido é um `JSArrayIterator`, `JSMapIterator`, `JSSetIterator` ou
/// `JSStringIterator`; avança direto, grava `done` e `value` sem chamar `next`. O `CHECK_EXCEPTION` do
/// `JSArrayIterator::next` (getter indexado que lança) e do `constructArrayPair` é a exceção pendente no `VM`.
fn iterator_next_try_fast(f: &mut SlowPathFrame, op: &OpIteratorNext, metadata_id: u32) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let iterator = f.get(op.iterator);

    if let Some(array_iterator) = JSArrayIterator::from_value(&iterator) {
        // `downcast<JSArray>(arrayIterator->iteratedObject())`: por classe (o ASSERT do C++ some no release).
        let array = JSArray::from_value_by_class(&array_iterator.iterated_object()).expect("ASSERT: isJSArray(arrayIterator->iteratedObject())");
        let structure_id = array.cell().structure_id();
        f.code_block.with_metadata::<OpIteratorNextMetadata, _>(metadata_id, |metadata| {
            metadata.iterable_profile.observe_structure_id(structure_id);
        });
        record_seen_mode::<OpIteratorNextMetadata>(f, metadata_id, array_iteration_mode(array_iterator.kind()));

        let pair_structure = ctx.global_object.array_structure_for_indexing_type_during_allocation(ARRAY_WITH_CONTIGUOUS);
        let value = array_iterator.next(ctx.vm, &pair_structure);
        if ctx.vm.exception().is_some() {
            return Err(LLIntFailure::Thrown);
        }
        set_fast_step(f, op, value);
        return Ok(());
    }

    if let Some(map_iterator) = JSMapIterator::from_value(&iterator) {
        record_seen_mode::<OpIteratorNextMetadata>(f, metadata_id, map_iteration_mode(map_iterator.kind()));
        set_fast_step(f, op, iterator_step_value(ctx.global_object, map_iterator.next()));
        return Ok(());
    }

    if let Some(set_iterator) = JSSetIterator::from_value(&iterator) {
        record_seen_mode::<OpIteratorNextMetadata>(f, metadata_id, set_iteration_mode(set_iterator.kind()));
        set_fast_step(f, op, iterator_step_value(ctx.global_object, set_iterator.next()));
        return Ok(());
    }

    let string_iterator = JSStringIterator::from_value(&iterator).expect("RELEASE_ASSERT_NOT_REACHED: iterador rápido desconhecido");
    record_seen_mode::<OpIteratorNextMetadata>(f, metadata_id, IterationMode::FastString);
    set_fast_step(f, op, string_iterator.next_with_advance(ctx.vm).map(JSValue::from_js_string));
    Ok(())
}

/// `op_iterator_next`: com uma sentinela em `next`, o `iteratorNextTryFastImpl`; sem ela, o genérico:
/// `next.call(iterator)`, depois `done` e `value`.
fn iterator_next(f: &mut SlowPathFrame, op: &OpIteratorNext, metadata_id: u32) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let next = f.get(op.next);
    if ctx.vm.is_fast_iteration_sentinel(next) {
        return iterator_next_try_fast(f, op, metadata_id);
    }

    record_seen_mode::<OpIteratorNextMetadata>(f, metadata_id, IterationMode::Generic);
    let call_data = get_call_data(next);
    if call_data.is_none() {
        // A chamada genérica de `next` é um `op_call` no C++, então o erro leva o trecho do fonte da instrução.
        let site = f.error_site();
        return Err(throw_error_object(ctx.global_object, create_not_a_function_error(ctx.global_object, next, Some(&site))));
    }

    let frame_base = llint_call_frame_base(f, op.stack_offset)?;
    let result = call_from_llint(ctx.global_object, frame_base, next, &call_data, f.get(op.iterator), &[])?;
    f.set(op.value, result);
    slow_path_iterator_next_get_done(f, op).map_err(|_| LLIntFailure::Thrown)?;
    slow_path_iterator_next_get_value(f, op).map_err(|_| LLIntFailure::Thrown)
}

/// Lê o `@@iterator` de `iterable` sem efeito observável (só estrutura, como o `trySpreadFast`): percorre o
/// objeto e seus protótipos olhando propriedades de dados; um acessor, `Proxy` ou propriedade customizada
/// devolve `None`, e a leitura fica para o `performIteration`. `undefined` se a cadeia não tem o símbolo.
fn static_iterator_method(global_object: &JSGlobalObject, iterable: JSValue) -> Option<JSValue> {
    let vm = global_object.vm();
    let name = PropertyName::from_identifier(&vm.property_names.iterator_symbol);
    let mut current = if iterable.is_string() { global_object.string_prototype().as_value() } else { iterable };
    loop {
        let object = JSObject::from_value(&current)?;
        if object.type_() == JSType::ProxyObjectType {
            return None;
        }
        let (offset, attributes) = object.get_direct_offset_with_attributes(vm, &name);
        if offset != INVALID_OFFSET {
            if attributes & (ACCESSOR | CUSTOM_ACCESSOR | CUSTOM_VALUE) != 0 {
                return None;
            }
            return Some(object.get_direct(offset));
        }
        current = object.get_prototype_direct();
        if current.is_null() {
            return Some(JSValue::undefined());
        }
    }
}

/// `slow_path_spread`: o iterável vira um `JSCellButterfly` com os valores.
fn spread(f: &mut SlowPathFrame, op: &OpSpread) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    // O `iteratorProtocolFunction` do C++ (IteratorHelpers.js) valida antes de iterar; as duas mensagens
    // são as dele, e o `Thrown::type_error` já as entrega sem o trecho do fonte, como o `@throwTypeError`.
    let iterable = f.get(op.argument);
    if iterable.is_undefined_or_null() {
        return Err(thrown_failure(
            ctx.global_object,
            Thrown::type_error("Spread syntax requires ...iterable not be null or undefined"),
        ));
    }
    // `trySpreadFast`: array puro, `String`, `Set` e os `arguments` (um `Set` com protocolo intacto cai em
    // `FastSet`). Todo o resto vai para o `performIteration` (IteratorHelpers.js), que o C++ chama como
    // `iteratorProtocolFunction`; ele precisa existir como CodeBlock para o dump bater com o do bun.
    let is_arguments = iterable.is_cell()
        && JSObject::from_value(&iterable).is_some_and(|object| {
            matches!(object.type_(), JSType::DirectArgumentsType | JSType::ScopedArgumentsType | JSType::ClonedArgumentsType)
        });
    // `trySpreadFast` só olha estrutura (sem `get` observável): o `@@iterator` é lido sem executar getter nem
    // `Proxy`, e se a leitura não for estática (acessor, `Proxy`, tipo desconhecido) o modo é `Generic` e a
    // única leitura, observável, fica com o `performIteration`.
    let tries_fast = (iterable.is_string() || iterable.is_object()) && (iterable.is_string() || is_js_array(&iterable) || is_arguments || JSSet::from_value(&iterable).is_some());
    let static_method = if tries_fast { static_iterator_method(ctx.global_object, iterable) } else { None };
    let mode = if let Some(iterator_method) = static_method {
        // `JSCellButterfly::createFromArray` lança `OutOfMemoryError` para um array acima de `maximumLength`; o
        // resultado seria esse mesmo depois de iterar até 2^32 índices de um array esparso, então falha antes.
        if iterable.is_cell() && JSArray::from_cell_id(iterable.as_cell()).is_some_and(|array| array.length() > MAXIMUM_LENGTH) {
            return Err(throw_out_of_memory_error(ctx.global_object));
        }
        get_iteration_mode(ctx.global_object, iterable, iterator_method)
    } else {
        IterationMode::Generic
    };
    let mut values = Vec::new();
    if mode == IterationMode::FastArray {
        // `JSCellButterfly::createFromArray` copia o array direto, sem abrir iterador: passar por
        // `iterator_step` custava um objeto `{value, done}` por elemento (1e6 elementos levavam minutos).
        let array = JSArray::from_value(&iterable).expect("ASSERT: modo FastArray só vale para JSArray");
        let length = array.length();
        values.reserve(length as usize);
        for index in 0..length {
            let value = array.get_by_index(ctx.vm, index);
            if ctx.vm.exception().is_some() {
                return Err(LLIntFailure::Thrown);
            }
            values.push(value);
        }
    } else if matches!(mode, IterationMode::FastString | IterationMode::FastSet) || (mode == IterationMode::Generic && is_arguments && static_method.is_some_and(|method| method.is_callable())) {
        for_each_in_iterable(ctx.global_object, iterable, |value| {
            values.push(value);
            Ok(())
        })
        .map_err(|thrown| thrown_failure(ctx.global_object, thrown))?;
    } else {
        let function = ctx.global_object.link_time_constant(LinkTimeConstant::PerformIteration);
        let result = call(ctx.global_object, function, &get_call_data(function), JSValue::null(), &[iterable])?;
        let array = JSArray::from_value(&result).expect("ASSERT: uncheckedDowncast<JSArray>(arrayResult)");
        let butterfly = JSCellButterfly::create_from_array(ctx.vm, &array)
            .ok_or_else(|| throw_out_of_memory_error(ctx.global_object))?;
        f.set(op.dst, JSValue::from_cell(butterfly.cell_id()));
        return Ok(());
    }

    let array = construct_array(ctx.vm, &ctx.global_object.array_structure(), &values);
    let butterfly = JSCellButterfly::create_from_array(ctx.vm, &array)
        .ok_or_else(|| throw_out_of_memory_error(ctx.global_object))?;
    f.set(op.dst, JSValue::from_cell(butterfly.cell_id()));
    Ok(())
}

/// `slow_path_new_array_with_spread`: o elemento `i` está em `argv - i`, e o bit `i` do `BitVector` diz se é um
/// `JSCellButterfly` a espalhar.
fn new_array_with_spread(f: &mut SlowPathFrame, op: &OpNewArrayWithSpread) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let items = op.argc as usize;
    let is_spread: Vec<bool> = {
        let unlinked = f.code_block.unlinked_code_block();
        let mut unlinked = unlinked.borrow_mut();
        let bit_vector = unlinked.bit_vector(op.bit_vector as usize);
        (0..items).map(|i| bit_vector.get(i)).collect()
    };
    // Invariante do C++ (`uncheckedDowncast<JSCellButterfly>`): `op_spread` e `op_new_array_with_spread` só
    // deixam `JSCellButterfly` nos registros marcados no `BitVector`.
    let spread_butterfly = |value: JSValue| -> LLIntResult<JSCellButterflyRef> {
        Ok(value
            .is_cell()
            .then(|| JSCellButterfly::from_cell_id(value.as_cell()))
            .flatten()
            .expect("ASSERT: uncheckedDowncast<JSCellButterfly>(values[-i])"))
    };

    if items == 1 && is_spread[0] {
        let structure = ctx.global_object.array_structure_for_indexing_type_during_allocation(COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS);
        if is_copy_on_write(structure.indexing_mode()) {
            let butterfly = spread_butterfly(f.get(op.argv))?;
            let result = allocate_new_array_buffer(ctx.vm, &structure, &butterfly);
            f.set(op.dst, result.as_value());
            return Ok(());
        }
    }

    let mut values = Vec::new();
    for (i, spreading) in is_spread.into_iter().enumerate() {
        let value = f.get(VirtualRegister::new(op.argv.offset() - i as i32));
        if !spreading {
            values.push(value);
            continue;
        }
        let array = spread_butterfly(value)?;
        values.extend((0..array.public_length()).map(|index| array.get(index)));
    }
    if values.len() >= MIN_ARRAY_STORAGE_CONSTRUCTION_LENGTH as usize {
        return Err(throw_out_of_memory_error(ctx.global_object));
    }

    let structure = ctx.global_object.array_structure_for_indexing_type_during_allocation(ARRAY_WITH_CONTIGUOUS);
    let array = construct_array(ctx.vm, &structure, &values);
    f.set(op.dst, JSValue::from_cell(array.cell_id()));
    Ok(())
}

/// Os handlers deste arquivo. `None` é o opcode que não é daqui.
pub(super) fn run_iterator(f: &mut SlowPathFrame, instruction: &InstructionRef) -> LLIntResult<Option<Step>> {
    match instruction.opcode_id_enum() {
        OpcodeID::op_iterator_open => iterator_open(f, &instruction.as_op::<OpIteratorOpen>(), instruction.metadata_id())?,
        OpcodeID::op_iterator_next => iterator_next(f, &instruction.as_op::<OpIteratorNext>(), instruction.metadata_id())?,
        OpcodeID::op_spread => spread(f, &instruction.as_op::<OpSpread>())?,
        OpcodeID::op_new_array_with_spread => new_array_with_spread(f, &instruction.as_op::<OpNewArrayWithSpread>())?,
        _ => return Ok(None),
    }
    Ok(Some(Step::Next))
}

#[cfg(test)]
mod tests {
    use crate::api::eval::evaluate_script;
    use crate::runtime::js_value::JSValue;

    /// Roda o corpo de uma função e devolve o valor que ela devolve.
    fn run(body: &str) -> Result<JSValue, JSValue> {
        evaluate_script(&format!("(function () {{ {body} }})()"))
    }

    #[test]
    fn for_of_over_a_map_yields_the_entries() {
        let result = run("var s = 0; for (const e of new Map([[1, 10], [2, 20]])) s += e[0] * e[1]; return s;");
        assert_eq!(result, Ok(JSValue::Int32(50)));
    }

    #[test]
    fn for_of_over_a_set_yields_the_values() {
        assert_eq!(run("var s = 0; for (const x of new Set([1, 2, 3, 3])) s += x; return s;"), Ok(JSValue::Int32(6)));
    }

    #[test]
    fn for_of_over_a_string_yields_code_points() {
        assert_eq!(run("var n = 0; for (const c of 'a\\u{1F600}b') n++; return n;"), Ok(JSValue::Int32(3)));
    }

    #[test]
    fn for_of_reuses_an_already_opened_map_iterator() {
        let source = "var m = new Map([[1, 10], [2, 20]]); var it = m.entries(); it.next(); var s = 0;\
                      for (const e of it) s += e[1]; return s;";
        assert_eq!(run(source), Ok(JSValue::Int32(20)));
        assert_eq!(run("var s = 0; for (const k of new Map([[1, 10], [2, 20]]).keys()) s += k; return s;"), Ok(JSValue::Int32(3)));
        assert_eq!(run("var s = 0; for (const v of new Map([[1, 10], [2, 20]]).values()) s += v; return s;"), Ok(JSValue::Int32(30)));
    }

    #[test]
    fn for_of_reuses_an_already_opened_set_and_array_iterator() {
        assert_eq!(run("var r = 0; for (const e of new Set([5]).entries()) r = e[0] + e[1]; return r;"), Ok(JSValue::Int32(10)));
        let source = "var it = [1, 2, 3][Symbol.iterator](); it.next(); var s = 0; for (const x of it) s += x; return s;";
        assert_eq!(run(source), Ok(JSValue::Int32(5)));
        assert_eq!(run("var s = 0; for (const k of ['a', 'b', 'c'].keys()) s += k; return s;"), Ok(JSValue::Int32(3)));
    }

    #[test]
    fn one_site_with_more_than_two_fast_modes_falls_back_to_generic() {
        // `canUseFastIterationMode`: o terceiro modo rápido no mesmo site vira `Generic`, com o mesmo resultado.
        // `'4'.split('')` entrega a string "4", e `0 + "4"` concatenaria ("6045" no bun); o `Number` mantém a soma numérica.
        let source = "function sum(iterable) { var s = 0; for (const x of iterable) s += Number(x); return s; }\
                      return sum([1, 2]) + sum(new Set([3])) + sum('4'.split('')) + sum(new Set([5]).values());";
        assert_eq!(run(source), Ok(JSValue::Int32(15)));
    }

    #[test]
    fn patched_map_iterator_next_is_observable() {
        let source = "var proto = Object.getPrototypeOf(new Map()[Symbol.iterator]()); var original = proto.next; var calls = 0;\
                      proto.next = function () { calls++; return original.call(this); };\
                      for (const e of new Map([[1, 2]])) ; return calls;";
        assert_eq!(run(source), Ok(JSValue::Int32(2)));
    }

    #[test]
    fn patched_string_iterator_next_is_observable() {
        let source = "var proto = Object.getPrototypeOf(''[Symbol.iterator]()); var original = proto.next; var calls = 0;\
                      proto.next = function () { calls++; return original.call(this); };\
                      for (const c of 'ab') ; return calls;";
        assert_eq!(run(source), Ok(JSValue::Int32(3)));
    }

    #[test]
    fn own_next_on_a_map_iterator_is_observable() {
        let source = "var it = new Map([[1, 2]]).entries(); it.next = function () { return { done: true }; };\
                      var n = 0; for (const e of it) n++; return n;";
        assert_eq!(run(source), Ok(JSValue::Int32(0)));
    }

    #[test]
    fn indexed_getter_that_throws_propagates_from_the_fast_array_iterator() {
        let source = "var a = [1, 2]; Object.defineProperty(a, 1, { get: function () { throw new Error('boom'); } });\
                      try { for (const x of a) ; } catch (e) { return e.message === 'boom'; } return false;";
        assert_eq!(run(source), Ok(JSValue::Bool(true)));
    }

    #[test]
    fn spread_of_map_and_set_still_collects_every_item() {
        assert_eq!(run("return [...new Set([1, 2, 2, 3])].length + [...new Map([[1, 2]])].length;"), Ok(JSValue::Int32(4)));
    }
}
