//! Os geradores `<name>Value(BytecodeGenerator&)` de `bytecode/BytecodeIntrinsicRegistry.{h,cpp}`
//! (`JSC_COMMON_BYTECODE_INTRINSIC_CONSTANTS_SIMPLE_EACH_NAME`) e o `orderedHashTableSentinelValue`.
//!
//! DIVERGÊNCIA: o C++ guarda cada valor num `Strong<Unknown>` (`m_<name>`) preenchido no construtor
//! do registro. Todos são imediatos (número ou `undefined`), sem referência a célula, então o
//! porte os calcula na chamada com a mesma expressão do construtor, sem `Strong` e sem estado.
//!
//! Fiel ao C++: `ModuleTranslate`, `proxyFieldTarget` e `proxyFieldHandler` estão na lista
//! `..._SIMPLE_EACH_NAME`, mas o construtor nunca chama `.set` neles. O `Strong<Unknown>` fica
//! vazio e `get()` devolve `JSValue()` (`HandleTypes<Unknown>::getFromSlot`), o valor vazio.
//!
//! `orderedHashTableSentinelValue` devolve `generator.vm().orderedHashTableSentinel()`, uma célula
//! criada por `JSOrderedHashMap::createSentinel`, que depende do heap (ausente no porte).

use crate::bytecode::bytecode_intrinsic_registry::BytecodeIntrinsicRegistry;
use crate::bytecode::bytecode_intrinsics_table::BytecodeIntrinsicEmitter as E;
use crate::bytecompiler::bytecode_generator::BytecodeGenerator;
use crate::runtime::identifier::MAX_ARRAY_INDEX;
use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::js_value::{js_number_i32, js_undefined, JSValue};
use crate::runtime::microtask::InternalMicrotask;
use crate::runtime::{
    js_array_iterator, js_async_disposable_stack, js_disposable_stack, js_generator, js_iterator_helper,
    js_module_loader, js_wrap_for_valid_iterator,
};

/// `JSString::MaxLength` (`JSString.h:134`): `std::numeric_limits<int32_t>::max()`.
const JS_STRING_MAX_LENGTH: u32 = i32::MAX as u32;

impl BytecodeIntrinsicRegistry {
    /// `<name>Value(BytecodeGenerator&)` da constante `emitter`, e `orderedHashTableSentinelValue`.
    /// `emitter` tem de ser um `emit_intrinsic_*` de constante (os de função não têm valor).
    pub fn constant_value(&self, emitter: E, generator: &BytecodeGenerator) -> JSValue {
        match emitter {
            E::Undefined => js_undefined(),
            E::Infinity => JSValue::double_number(f64::INFINITY),
            E::IterationKindKey => JSValue::from_u32(IterationKind::Keys as u32),
            E::IterationKindValue => JSValue::from_u32(IterationKind::Values as u32),
            E::IterationKindEntries => JSValue::from_u32(IterationKind::Entries as u32),
            E::MaxArrayIndex => JSValue::from_u32(MAX_ARRAY_INDEX),
            E::MaxStringLength => JSValue::from_u32(JS_STRING_MAX_LENGTH),
            E::MaxSafeInteger => JSValue::double_number(crate::runtime::math_common::max_safe_integer()),
            E::ModuleFetch => JSValue::from_u32(js_module_loader::Status::Fetch as u32),
            E::ModuleInstantiate => JSValue::from_u32(js_module_loader::Status::Instantiate as u32),
            E::ModuleSatisfy => JSValue::from_u32(js_module_loader::Status::Satisfy as u32),
            E::ModuleLink => JSValue::from_u32(js_module_loader::Status::Link as u32),
            E::ModuleReady => JSValue::from_u32(js_module_loader::Status::Ready as u32),
            E::ModuleTranslate | E::ProxyFieldTarget | E::ProxyFieldHandler => JSValue::empty(),
            E::GeneratorFieldState => JSValue::from_u32(js_generator::Field::State as u32),
            E::GeneratorFieldNext => JSValue::from_u32(js_generator::Field::Next as u32),
            E::GeneratorFieldThis => JSValue::from_u32(js_generator::Field::This as u32),
            E::GeneratorFieldFrame => JSValue::from_u32(js_generator::Field::Frame as u32),
            E::GeneratorResumeModeNormal => js_number_i32(js_generator::ResumeMode::NormalMode as i32),
            E::GeneratorResumeModeThrow => js_number_i32(js_generator::ResumeMode::ThrowMode as i32),
            E::GeneratorResumeModeReturn => js_number_i32(js_generator::ResumeMode::ReturnMode as i32),
            E::GeneratorStateCompleted => js_number_i32(js_generator::State::Completed as i32),
            E::GeneratorStateExecuting => js_number_i32(js_generator::State::Executing as i32),
            E::GeneratorStateInit => js_number_i32(js_generator::State::Init as i32),
            E::ArrayIteratorFieldIteratedObject => js_number_i32(js_array_iterator::Field::IteratedObject as i32),
            E::ArrayIteratorFieldIndex => js_number_i32(js_array_iterator::Field::Index as i32),
            E::ArrayIteratorFieldKind => js_number_i32(js_array_iterator::Field::Kind as i32),
            E::WrapForValidIteratorFieldIteratedIterator => {
                js_number_i32(js_wrap_for_valid_iterator::Field::IteratedIterator as i32)
            }
            E::WrapForValidIteratorFieldIteratedNextMethod => {
                js_number_i32(js_wrap_for_valid_iterator::Field::IteratedNextMethod as i32)
            }
            E::IteratorHelperFieldGenerator => js_number_i32(js_iterator_helper::Field::Generator as i32),
            E::IteratorHelperFieldUnderlyingIterator => {
                js_number_i32(js_iterator_helper::Field::UnderlyingIterator as i32)
            }
            E::DisposableStackFieldState => js_number_i32(js_disposable_stack::Field::State as i32),
            E::DisposableStackFieldCapability => js_number_i32(js_disposable_stack::Field::Capability as i32),
            E::DisposableStackStatePending => js_number_i32(js_disposable_stack::State::Pending as i32),
            E::DisposableStackStateDisposed => js_number_i32(js_disposable_stack::State::Disposed as i32),
            E::AsyncDisposableStackFieldState => js_number_i32(js_async_disposable_stack::Field::State as i32),
            E::AsyncDisposableStackFieldCapability => {
                js_number_i32(js_async_disposable_stack::Field::Capability as i32)
            }
            E::AsyncDisposableStackStatePending => js_number_i32(js_async_disposable_stack::State::Pending as i32),
            E::AsyncDisposableStackStateDisposed => js_number_i32(js_async_disposable_stack::State::Disposed as i32),
            E::InternalMicrotaskAsyncFromSyncIteratorContinue => {
                js_number_i32(InternalMicrotask::AsyncFromSyncIteratorContinue as i32)
            }
            E::InternalMicrotaskAsyncFromSyncIteratorDone => {
                js_number_i32(InternalMicrotask::AsyncFromSyncIteratorDone as i32)
            }
            // `generator.vm().orderedHashTableSentinel()`.
            E::OrderedHashTableSentinel => JSValue::from_cell(generator.vm().ordered_hash_table_sentinel()),
            _ => unreachable!("constant_value em emitter que não é de constante: {emitter:?}"),
        }
    }
}
