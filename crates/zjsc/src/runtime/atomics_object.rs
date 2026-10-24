//! Porte de `runtime/AtomicsObject.{h,cpp}`: o objeto `Atomics` (`add`, `and`, `compareExchange`, `exchange`,
//! `isLockFree`, `load`, `notify`, `or`, `store`, `sub`, `wait`, `xor`, `pause`, `waitAsync`) e as operações
//! que o C++ expõe à JIT (`operationAtomics*`, que não se portam: o interpretador chama as funções nativas).
//!
//! A maquinaria dos elementos é a de `typed_array_adaptors.rs` (o `Adaptor` é o `TypedArrayType`) e a visão é
//! o `JSGenericTypedArrayView` de `js_generic_typed_array_view.rs` (sobre o `JSArrayBufferView`);
//! `typedVector()` + `WTF::atomicExchangeAdd(ptr, x)` e irmãs viram `get_element`/`set_element` (uma thread:
//! a leitura e a escrita seguidas são atômicas). `validateTypedArray` é o de `js_generic_typed_array_view.rs`.
//!
//! DIVERGÊNCIAS:
//! - `WaiterListManager` (ver `waiter_list_manager.rs`): `waitAsync` e `notify` usam as listas de uma thread e
//!   o relógio virtual de lá (o prazo de um `waitAsync` só vence quando o driver de `api/eval.rs` avança o
//!   relógio; o bun também não mantém o processo vivo por ele). `wait` devolve `"not-equal"` ou, com prazo
//!   finito, dorme o prazo e devolve `"timed-out"`; `wait` sem prazo (o bun mede que trava para sempre na
//!   thread principal: `Atomics.wait(i32, 0, 0, Infinity)` não volta, `timeout 5` o mata) bloqueia
//!   para sempre (`park` em laço), como o C++ sem outra thread que notifique. `getWaiterListSize` (só do `jsc` de teste) não
//!   se porta.
//! - `vm.vmType == VM::VMType::Default` (a condição de `waitAsync`) vale sempre: o porte tem um tipo de VM.
//! - `vm.m_typedArrayController->isAtomicsWaitAllowedOnCurrentThread()` vale sempre (sem `TypedArrayController`).
//! - `simde_mm_pause()` (`Atomics.pause`) é `std::hint::spin_loop()`.

use std::rc::Rc;
use std::time::Duration;

use crate::runtime::intl_support::{new_object, put};
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_promise_host::PromiseHost;
use crate::runtime::waiter_list_manager::{add_async_waiter, notify_waiters};

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{put_native_function, put_to_string_tag};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array_buffer_view::{JSArrayBufferView, TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE};
use crate::runtime::js_big_int_ops::{to_big_int, to_big_int64_value};
use crate::runtime::js_generic_typed_array_view::{validate_typed_array, JSGenericTypedArrayViewRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::math_common::is_integer;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::typed_array_adaptors::{to_js_value, to_native_from_value, NativeElement};
use crate::runtime::typed_array_type::TypedArrayType;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo AtomicsObject::s_info`.
pub static ATOMICS_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "Atomics", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

const WAIT_TYPED_ARRAY_MESSAGE: &str = "Typed array argument must be an Int32Array or BigInt64Array.";
const READ_WRITE_TYPED_ARRAY_MESSAGE: &str =
    "Typed array argument must be an Int8Array, Int16Array, Int32Array, Uint8Array, Uint16Array, Uint32Array, BigInt64Array, or BigUint64Array.";
const ACCESS_INDEX_OUT_OF_BOUNDS_MESSAGE: &str = "Access index out of bounds for atomic access.";
const WAIT_NOT_SHARED_MESSAGE: &str = "Typed array for wait/waitAsync/notify must wrap a SharedArrayBuffer.";
const PAUSE_ARGUMENT_MESSAGE: &str = "Atomics.pause argument needs to be either undefined or integer number";

/// `enum class TypedArrayOperationMode { ReadWrite, Wait }`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TypedArrayOperationMode {
    ReadWrite,
    Wait,
}

/// `validateIntegerTypedArray<mode>(globalObject, typedArrayValue)`.
fn validate_integer_typed_array(
    mode: TypedArrayOperationMode,
    typed_array_value: JSValue,
) -> Result<JSGenericTypedArrayViewRef, Thrown> {
    let typed_array = validate_typed_array(typed_array_value)?;
    let type_ = typed_array.typed_array_type();
    match mode {
        TypedArrayOperationMode::Wait => {
            if !matches!(type_, TypedArrayType::Int32 | TypedArrayType::BigInt64) {
                return Err(Thrown::type_error(WAIT_TYPED_ARRAY_MESSAGE));
            }
        }
        TypedArrayOperationMode::ReadWrite => {
            if !matches!(
                type_,
                TypedArrayType::Int8
                    | TypedArrayType::Int16
                    | TypedArrayType::Int32
                    | TypedArrayType::Uint8
                    | TypedArrayType::Uint16
                    | TypedArrayType::Uint32
                    | TypedArrayType::BigInt64
                    | TypedArrayType::BigUint64
            ) {
                return Err(Thrown::type_error(READ_WRITE_TYPED_ARRAY_MESSAGE));
            }
        }
    }
    Ok(typed_array)
}

/// `validateAtomicAccess(globalObject, vm, typedArrayView, accessIndexValue)`.
fn validate_atomic_access(typed_array_view: &JSArrayBufferView, access_index_value: JSValue) -> Result<u64, Thrown> {
    let length = typed_array_view.length();
    let access_index = if access_index_value.is_uint32() {
        u64::from(access_index_value.as_uint32())
    } else {
        access_index_value.to_index("accessIndex")?
    };

    if access_index >= length as u64 {
        return Err(Thrown::range_error(ACCESS_INDEX_OUT_OF_BOUNDS_MESSAGE));
    }
    Ok(access_index)
}

/// Os `AddFunc`, `AndFunc`, `CompareExchangeFunc`, `ExchangeFunc`, `LoadFunc`, `OrFunc`, `SubFunc` e `XorFunc`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadModifyWrite {
    Add,
    And,
    CompareExchange,
    Exchange,
    Load,
    Or,
    Sub,
    Xor,
}

impl ReadModifyWrite {
    /// `Func::numExtraArgs`.
    fn num_extra_args(self) -> usize {
        match self {
            ReadModifyWrite::Load => 0,
            ReadModifyWrite::CompareExchange => 2,
            _ => 1,
        }
    }

    /// O valor que fica na memória depois da operação sobre `old` (`WTF::atomicExchangeAdd(ptr, args[0])` e
    /// irmãs, que gravam o resultado e devolvem o valor antigo). `args` são os `extraArgs` já convertidos
    /// para o tipo do elemento.
    fn new_value(self, type_: TypedArrayType, old: NativeElement, args: &[NativeElement]) -> NativeElement {
        let mask = u64::MAX >> (64 - 8 * type_.element_size());
        match self {
            ReadModifyWrite::Add => old.wrapping_add(args[0]) & mask,
            ReadModifyWrite::Sub => old.wrapping_sub(args[0]) & mask,
            ReadModifyWrite::And => old & args[0],
            ReadModifyWrite::Or => old | args[0],
            ReadModifyWrite::Xor => old ^ args[0],
            ReadModifyWrite::Exchange => args[0],
            ReadModifyWrite::CompareExchange => {
                if old == args[0] {
                    args[1]
                } else {
                    old
                }
            }
            ReadModifyWrite::Load => old,
        }
    }
}

/// `atomicReadModifyWriteCase<Adaptor>(globalObject, vm, args, typedArrayView, accessIndex, func)`.
fn atomic_read_modify_write_case(
    global_object: &JSGlobalObject,
    args: &[JSValue],
    typed_array: &JSArrayBufferView,
    access_index: u64,
    func: ReadModifyWrite,
) -> HostResult {
    let type_ = typed_array.typed_array_type();

    let mut extra_args = Vec::with_capacity(func.num_extra_args());
    for i in 0..func.num_extra_args() {
        extra_args.push(to_native_from_value(global_object, type_, args[2 + i])?);
    }

    if typed_array.is_detached() || !typed_array.in_bounds(access_index) {
        return Err(Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE));
    }

    let index = access_index as usize;
    let old = typed_array.get_element(index);
    typed_array.set_element(index, func.new_value(type_, old, &extra_args));
    to_js_value(type_, old)
}

/// `atomicReadModifyWrite(globalObject, vm, args, func)`.
fn atomic_read_modify_write(global_object: &JSGlobalObject, args: &[JSValue], func: ReadModifyWrite) -> HostResult {
    let typed_array_view = validate_integer_typed_array(TypedArrayOperationMode::ReadWrite, args[0])?;
    let access_index = validate_atomic_access(&typed_array_view, args[1])?;
    atomic_read_modify_write_case(global_object, args, &typed_array_view, access_index, func)
}

/// `atomicReadModifyWrite(globalObject, callFrame, func)`: lê `2 + numExtraArgs` argumentos.
fn atomic_read_modify_write_call(global_object: &JSGlobalObject, call: &HostCall, func: ReadModifyWrite) -> HostResult {
    let args: Vec<JSValue> = (0..2 + func.num_extra_args()).map(|i| call.argument(i)).collect();
    atomic_read_modify_write(global_object, &args, func)
}

/// `atomicStoreCase<Adaptor>(globalObject, vm, operand, typedArrayView, accessIndex)`.
fn atomic_store_case(
    global_object: &JSGlobalObject,
    operand: JSValue,
    typed_array: &JSArrayBufferView,
    access_index: u64,
) -> HostResult {
    let type_ = typed_array.typed_array_type();

    let value = if type_.is_big_int_typed_view() {
        let value = to_big_int(operand);
        pending_or(global_object, value)?
    } else {
        let integer = operand.to_integer_or_infinity_checked()?;
        js_number(integer)
    };
    let extra_arg = to_native_from_value(global_object, type_, value)?;

    if typed_array.is_detached() || !typed_array.in_bounds(access_index) {
        return Err(Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE));
    }

    typed_array.set_element(access_index as usize, extra_arg);
    Ok(value)
}

/// `atomicStore(globalObject, vm, base, index, operand)` (https://tc39.es/ecma262/#sec-atomics.store).
fn atomic_store(global_object: &JSGlobalObject, base: JSValue, index: JSValue, operand: JSValue) -> HostResult {
    let typed_array_view = validate_integer_typed_array(TypedArrayOperationMode::ReadWrite, base)?;
    let access_index = validate_atomic_access(&typed_array_view, index)?;
    atomic_store_case(global_object, operand, &typed_array_view, access_index)
}

/// `isLockFree(globalObject, arg)`.
fn is_lock_free(global_object: &JSGlobalObject, arg: JSValue) -> HostResult {
    let size = arg.to_int32();
    pending_or(global_object, ())?;
    Ok(js_boolean(matches!(size, 1 | 2 | 4 | 8)))
}

/// `atomicsFuncAdd`.
fn atomics_func_add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    atomic_read_modify_write_call(global_object, call, ReadModifyWrite::Add)
}

/// `atomicsFuncAnd`.
fn atomics_func_and_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    atomic_read_modify_write_call(global_object, call, ReadModifyWrite::And)
}

/// `atomicsFuncCompareExchange`.
fn atomics_func_compare_exchange_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    atomic_read_modify_write_call(global_object, call, ReadModifyWrite::CompareExchange)
}

/// `atomicsFuncExchange`.
fn atomics_func_exchange_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    atomic_read_modify_write_call(global_object, call, ReadModifyWrite::Exchange)
}

/// `atomicsFuncIsLockFree`.
fn atomics_func_is_lock_free_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    is_lock_free(global_object, call.argument(0))
}

/// `atomicsFuncLoad`.
fn atomics_func_load_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    atomic_read_modify_write_call(global_object, call, ReadModifyWrite::Load)
}

/// `atomicsFuncOr`.
fn atomics_func_or_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    atomic_read_modify_write_call(global_object, call, ReadModifyWrite::Or)
}

/// `atomicsFuncStore`.
fn atomics_func_store_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    atomic_store(global_object, call.argument(0), call.argument(1), call.argument(2))
}

/// `atomicsFuncSub`.
fn atomics_func_sub_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    atomic_read_modify_write_call(global_object, call, ReadModifyWrite::Sub)
}

/// `atomicsFuncXor`.
fn atomics_func_xor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    atomic_read_modify_write_call(global_object, call, ReadModifyWrite::Xor)
}

/// `enum class AtomicsWaitType { Sync, Async }`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AtomicsWaitType {
    Sync,
    Async,
}

/// `atomicsFuncWait` e `atomicsFuncWaitAsync`: a parte comum (`validateIntegerTypedArray<Wait>`, a conferência
/// de `isShared`, `validateAtomicAccess`, o valor esperado) e `atomicsWaitImpl`.
fn atomics_wait(global_object: &JSGlobalObject, call: &HostCall, wait_type: AtomicsWaitType) -> HostResult {
    let typed_array_view = validate_integer_typed_array(TypedArrayOperationMode::Wait, call.argument(0))?;

    if !typed_array_view.is_shared() {
        return Err(Thrown::type_error(WAIT_NOT_SHARED_MESSAGE));
    }

    let access_index = validate_atomic_access(&typed_array_view, call.argument(1))?;

    let type_ = typed_array_view.typed_array_type();
    let expected_value: NativeElement = if type_ == TypedArrayType::Int32 {
        let expected = call.argument(2).to_int32();
        pending_or(global_object, expected)? as u32 as u64
    } else {
        // `toBigInt64(globalObject)`: `toBigInt` e os 64 bits baixos.
        let big_int = pending_or(global_object, to_big_int(call.argument(2)))?;
        to_big_int64_value(big_int) as u64
    };

    // `atomicsWaitImpl`: o prazo.
    let timeout_in_milliseconds = call.argument(3).to_number();
    pending_or(global_object, ())?;
    let timeout = if timeout_in_milliseconds.is_nan() { None } else { Some(timeout_in_milliseconds.max(0.0)) };

    let vm = global_object.vm();
    if wait_type == AtomicsWaitType::Async {
        // `WaiterListManager::waitAsync`: `{ async: false, value }` quando o resultado já se sabe, senão
        // `{ async: true, value: Promise }` e o waiter entra na lista (o prazo corre no relógio virtual).
        let result = new_object(global_object);
        if typed_array_view.get_element(access_index as usize) != expected_value {
            put(global_object, &result, "async", js_boolean(false));
            put(global_object, &result, "value", JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(b"not-equal"))));
        } else if timeout == Some(0.0) {
            put(global_object, &result, "async", js_boolean(false));
            put(global_object, &result, "value", JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(b"timed-out"))));
        } else {
            let promise = JSPromise::create(vm, &global_object.promise_structure());
            add_async_waiter(waiter_key(&typed_array_view, access_index), promise.clone(), timeout);
            put(global_object, &result, "async", js_boolean(true));
            put(global_object, &result, "value", promise.as_value());
        }
        return Ok(result.as_value());
    }

    // `WaiterListManager::waitSync`: o valor atual contra o esperado, depois a espera.
    if typed_array_view.get_element(access_index as usize) != expected_value {
        return Ok(JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(b"not-equal"))));
    }
    match timeout.filter(|milliseconds| milliseconds.is_finite()).and_then(|milliseconds| Duration::try_from_secs_f64(milliseconds / 1000.0).ok()) {
        Some(duration) => {
            ul_common::time::sleep(duration);
            Ok(JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(b"timed-out"))))
        }
        // Sem prazo, ou com um tão grande que a `Duration` não o representa (a espera seria eterna).
        None => loop {
            // `WaiterListManager::waitSync` sem prazo: bloqueia até outra thread notificar; o porte não tem
            // outra thread, então a espera é eterna, como o bun mede (`Atomics.wait(i32, 0, 0, Infinity)`).
            std::thread::park();
        },
    }
}

/// `atomicsFuncWait`.
fn atomics_func_wait_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    atomics_wait(global_object, call, AtomicsWaitType::Sync)
}

/// `atomicsFuncWaitAsync`.
fn atomics_func_wait_async_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    atomics_wait(global_object, call, AtomicsWaitType::Async)
}

/// `atomicsFuncPause`.
fn atomics_func_pause_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // O inteiro ainda não é usado, como no C++ ("Right now, argument integer is not used").
    let argument = call.argument(0);
    if !argument.is_undefined() && (!argument.is_number() || !is_integer(argument.as_number())) {
        return Err(Thrown::type_error(PAUSE_ARGUMENT_MESSAGE));
    }

    std::hint::spin_loop();

    Ok(JSValue::Undefined)
}

/// `atomicsFuncNotify`.
fn atomics_func_notify_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let typed_array_view = validate_integer_typed_array(TypedArrayOperationMode::Wait, call.argument(0))?;

    let access_index = validate_atomic_access(&typed_array_view, call.argument(1))?;

    // `count`: `undefined` é +Infinity, senão `max(toIntegerOrInfinity, 0)`.
    let count_value = call.argument(2);
    let count = if count_value.is_undefined() { f64::INFINITY } else { count_value.to_integer_or_infinity_checked()?.max(0.0) };

    // `if (!typedArrayView->isShared()) return 0`, e `WaiterListManager::notifyWaiter`.
    if !typed_array_view.is_shared() {
        return Ok(js_number(0));
    }
    Ok(js_number(notify_waiters(global_object, waiter_key(&typed_array_view, access_index), count) as f64))
}

/// A identidade do elemento para a lista de espera: o `ArrayBuffer` e o deslocamento em bytes (o endereço
/// do C++).
fn waiter_key(typed_array_view: &JSArrayBufferView, access_index: u64) -> (usize, usize) {
    let buffer = typed_array_view.possibly_shared_buffer();
    let byte_offset = typed_array_view.byte_offset() + access_index as usize * typed_array_view.typed_array_type().element_size();
    (Rc::as_ptr(&buffer) as usize, byte_offset)
}

host_function!(atomics_func_add, atomics_func_add_body);
host_function!(atomics_func_and, atomics_func_and_body);
host_function!(atomics_func_compare_exchange, atomics_func_compare_exchange_body);
host_function!(atomics_func_exchange, atomics_func_exchange_body);
host_function!(atomics_func_is_lock_free, atomics_func_is_lock_free_body);
host_function!(atomics_func_load, atomics_func_load_body);
host_function!(atomics_func_notify, atomics_func_notify_body);
host_function!(atomics_func_or, atomics_func_or_body);
host_function!(atomics_func_store, atomics_func_store_body);
host_function!(atomics_func_sub, atomics_func_sub_body);
host_function!(atomics_func_wait, atomics_func_wait_body);
host_function!(atomics_func_xor, atomics_func_xor_body);
host_function!(atomics_func_pause, atomics_func_pause_body);
host_function!(atomics_func_wait_async, atomics_func_wait_async_body);

/// `class AtomicsObject final : public JSNonFinalObject`: sem campos próprios.
pub struct AtomicsObject;

impl AtomicsObject {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`: `TypeInfo(ObjectType, StructureFlags)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, AtomicsObject::STRUCTURE_FLAGS),
            &ATOMICS_OBJECT_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `AtomicsObject(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let object = JSObject::allocate(vm, structure);
        AtomicsObject::finish_creation(&object, vm, global_object);
        object
    }

    /// `finishCreation(vm, globalObject)`: as funções de `FOR_EACH_ATOMICS_FUNC` (`DontEnum`, com o
    /// intrínseco `Atomics<Nome>Intrinsic`), `pause`, `waitAsync` e `@@toStringTag`.
    fn finish_creation(object: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        object.finish_creation(vm);

        let functions: [(&[u8], u32, crate::runtime::native_function::NativeFunction, Intrinsic); 14] = [
            (b"add", 3, atomics_func_add, Intrinsic::AtomicsAddIntrinsic),
            (b"and", 3, atomics_func_and, Intrinsic::AtomicsAndIntrinsic),
            (b"compareExchange", 4, atomics_func_compare_exchange, Intrinsic::AtomicsCompareExchangeIntrinsic),
            (b"exchange", 3, atomics_func_exchange, Intrinsic::AtomicsExchangeIntrinsic),
            (b"isLockFree", 1, atomics_func_is_lock_free, Intrinsic::AtomicsIsLockFreeIntrinsic),
            (b"load", 2, atomics_func_load, Intrinsic::AtomicsLoadIntrinsic),
            (b"notify", 3, atomics_func_notify, Intrinsic::AtomicsNotifyIntrinsic),
            (b"or", 3, atomics_func_or, Intrinsic::AtomicsOrIntrinsic),
            (b"store", 3, atomics_func_store, Intrinsic::AtomicsStoreIntrinsic),
            (b"sub", 3, atomics_func_sub, Intrinsic::AtomicsSubIntrinsic),
            (b"wait", 4, atomics_func_wait, Intrinsic::AtomicsWaitIntrinsic),
            (b"xor", 3, atomics_func_xor, Intrinsic::AtomicsXorIntrinsic),
            (b"pause", 0, atomics_func_pause, Intrinsic::AtomicsPauseIntrinsic),
            // `if (vm.vmType == VM::VMType::Default)`: sempre.
            (b"waitAsync", 4, atomics_func_wait_async, Intrinsic::AtomicsWaitAsyncIntrinsic),
        ];
        for (name, length, function, intrinsic) in functions {
            put_native_function(vm, global_object, object, &Identifier::from_span(vm, name), length, function, intrinsic);
        }
        put_to_string_tag(vm, object, ATOMICS_OBJECT_S_INFO.class_name);
    }
}

/// `createAtomicsProperty(vm, object)` mais a propriedade global `Atomics` (`DontEnum|PropertyCallback`):
/// `AtomicsObject::create(vm, global, AtomicsObject::createStructure(vm, global, global->objectPrototype()))`.
/// DIVERGÊNCIA: o C++ cria o objeto na primeira leitura; aqui nasce com o global.
pub fn install_atomics(global_object: &JSGlobalObject, object_prototype: &JSObjectRef) -> JSObjectRef {
    let vm = global_object.vm();
    let structure = AtomicsObject::create_structure(vm, global_object, object_prototype.as_value());
    let atomics = AtomicsObject::create(vm, global_object, &structure);
    global_object.put_direct(
        vm,
        &crate::runtime::property_name::PropertyName::from_identifier(&Identifier::from_span(vm, b"Atomics".as_slice())),
        atomics.as_value(),
        crate::runtime::property_attribute::DONT_ENUM,
    );
    atomics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::typed_array_adaptors::read_element;

    #[test]
    fn read_modify_write_wraps_at_the_element_width() {
        let args = [0x01];
        assert_eq!(ReadModifyWrite::Add.new_value(TypedArrayType::Uint8, 0xff, &args), 0x00);
        assert_eq!(ReadModifyWrite::Sub.new_value(TypedArrayType::Uint8, 0x00, &args), 0xff);
        assert_eq!(ReadModifyWrite::Add.new_value(TypedArrayType::Int16, 0x7fff, &args), 0x8000);
        assert_eq!(ReadModifyWrite::Sub.new_value(TypedArrayType::BigUint64, 0, &args), u64::MAX);
    }

    #[test]
    fn compare_exchange_stores_only_on_match() {
        assert_eq!(ReadModifyWrite::CompareExchange.new_value(TypedArrayType::Int32, 5, &[5, 9]), 9);
        assert_eq!(ReadModifyWrite::CompareExchange.new_value(TypedArrayType::Int32, 4, &[5, 9]), 4);
        assert_eq!(ReadModifyWrite::Exchange.new_value(TypedArrayType::Int32, 4, &[7]), 7);
        assert_eq!(ReadModifyWrite::Load.new_value(TypedArrayType::Int32, 4, &[]), 4);
        assert_eq!(ReadModifyWrite::Load.num_extra_args(), 0);
        assert_eq!(ReadModifyWrite::CompareExchange.num_extra_args(), 2);
    }

    #[test]
    fn bitwise_operations() {
        assert_eq!(ReadModifyWrite::And.new_value(TypedArrayType::Uint16, 0b1100, &[0b1010]), 0b1000);
        assert_eq!(ReadModifyWrite::Or.new_value(TypedArrayType::Uint16, 0b1100, &[0b1010]), 0b1110);
        assert_eq!(ReadModifyWrite::Xor.new_value(TypedArrayType::Uint16, 0b1100, &[0b1010]), 0b0110);
        assert_eq!(read_element(TypedArrayType::Uint16, &[0x34, 0x12]), 0x1234);
    }
}
