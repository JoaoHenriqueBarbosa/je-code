//! Porte de `runtime/JSArrayBufferConstructor.h` e `JSArrayBufferConstructor.cpp`: os construtores
//! `ArrayBuffer` e `SharedArrayBuffer` (o `JSGenericArrayBufferConstructor<sharingMode>`, no bun um
//! `JSFunction` sobre `NativeExecutable`), com `new ArrayBuffer(length, { maxByteLength })`,
//! `ArrayBuffer.isView` e
//! `@@species`.
//!
//! DIVERGÊNCIAS:
//!
//! - O `@@species` é o acessor que `create_collection_constructor` cria (o `JSFunction`
//!   `"get [Symbol.species]"` sobre `globalFuncSpeciesGetter`); no C++ ele é o
//!   `globalObject->arrayBufferSpeciesGetterSetter(sharingMode)`, criado em `JSGlobalObject::init` para
//!   o `tryInstallArrayBufferSpeciesWatchpoint` compará-lo. Sem watchpoints, não há o que comparar.
//! - O `RETURN_IF_EXCEPTION` das conversões é `check_exception` (o `toNumber` do porte deixa a exceção
//!   pendente no VM).
//! - O `JSC_GET_DERIVED_STRUCTURE` é `get_derived_structure_in_realm`, com a estrutura base do
//!   `ArrayBufferRealm` do realm de `getFunctionRealm(newTarget)`.

use crate::host_function;
use crate::runtime::array_buffer::{ArrayBuffer, ArrayBufferSharingMode};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{
    constructor_cannot_be_called_as_function, create_native_collection_constructor, native_constructor_structure, put_native_function,
};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_array_buffer::{JSArrayBuffer, JSArrayBufferRef};
use crate::runtime::js_function::{JSFunction, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_type::{FIRST_TYPED_ARRAY_TYPE, LAST_TYPED_ARRAY_TYPE};
use crate::runtime::js_value::{js_boolean, purify_nan, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_regexp_support::check_exception;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `const ClassInfo JSArrayBufferConstructor::s_info` (`"Function"`).
pub static ARRAY_BUFFER_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo JSSharedArrayBufferConstructor::s_info` (`"Function"`).
pub static SHARED_ARRAY_BUFFER_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `callArrayBuffer`: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, "ArrayBuffer")`
/// (o `SharedArrayBuffer` usa o mesmo).
fn call_array_buffer_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("ArrayBuffer")
}

/// `JSGenericArrayBufferConstructor<sharingMode>::constructImpl`.
fn construct_impl(global_object: &JSGlobalObject, call: &HostCall, sharing_mode: ArrayBufferSharingMode) -> HostResult {
    let vm = global_object.vm();

    let mut length_double = 0.0;
    let mut max_byte_length: Option<usize> = None;

    let has_arguments = call.argument_count() > 0;
    if has_arguments {
        length_double = call.argument(0).to_number();
        check_exception(global_object)?;
        let options = call.argument(1);
        if options.is_object() {
            let max_byte_length_value =
                get_value_property(global_object, options, &PropertyName::from_identifier(&vm.property_names.max_byte_length))?;
            if !max_byte_length_value.is_undefined() {
                max_byte_length = Some(max_byte_length_value.to_index("maxByteLength")? as usize);
            }
        }
    }

    // https://tc39.es/proposal-resizablearraybuffer/#sec-allocatesharedarraybuffer
    if max_byte_length.is_some_and(|max_byte_length| (max_byte_length as f64) < length_double) {
        return Err(Thrown::range_error("ArrayBuffer length exceeds maxByteLength option"));
    }

    let structure = get_derived_structure_in_realm(global_object, call.new_target(), call.callee(), |realm| {
        realm.array_buffer_realm.array_buffer_structure(sharing_mode)
    })?;

    let mut length = 0;
    if has_arguments {
        length = JSValue::double_number(purify_nan(length_double)).to_index("length")? as usize;
    }

    let mut buffer = None;
    if let (Some(max_byte_length), ArrayBufferSharingMode::Shared) = (max_byte_length, sharing_mode) {
        buffer = Some(ArrayBuffer::try_create_shared(length, 1, max_byte_length).ok_or(Thrown::OutOfMemory)?);
    }

    let buffer = match buffer {
        Some(buffer) => buffer,
        None => {
            let buffer = ArrayBuffer::try_create(length, 1, max_byte_length).ok_or(Thrown::OutOfMemory)?;
            if sharing_mode == ArrayBufferSharingMode::Shared {
                buffer.set_sharing_mode(ArrayBufferSharingMode::Shared);
            }
            buffer
        }
    };

    debug_assert_eq!(sharing_mode, buffer.sharing_mode());

    Ok(JSArrayBuffer::create(vm, &structure, buffer).as_value())
}

fn construct_array_buffer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_impl(global_object, call, ArrayBufferSharingMode::Default)
}

fn construct_shared_array_buffer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_impl(global_object, call, ArrayBufferSharingMode::Shared)
}

// ECMA 24.1.3.1
fn array_buffer_func_is_view_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let is_view = match call.argument(0) {
        JSValue::Cell(cell_id) => crate::runtime::cell_registry::cell_type(cell_id)
            .is_some_and(|type_| (FIRST_TYPED_ARRAY_TYPE..=LAST_TYPED_ARRAY_TYPE).contains(&(type_ as u32))),
        _ => false,
    };
    Ok(js_boolean(is_view))
}

host_function!(call_array_buffer, call_array_buffer_body);
host_function!(construct_array_buffer, construct_array_buffer_body);
host_function!(construct_shared_array_buffer, construct_shared_array_buffer_body);
host_function!(array_buffer_func_is_view, array_buffer_func_is_view_body);

/// `constructArrayBufferWithSize(globalObject, structure, length)`.
pub fn construct_array_buffer_with_size(
    global_object: &JSGlobalObject,
    structure: &StructureRef,
    length: usize,
) -> Result<JSArrayBufferRef, Thrown> {
    let buffer = ArrayBuffer::try_create(length, 1, None).ok_or(Thrown::OutOfMemory)?;

    if std::rc::Rc::ptr_eq(structure, &global_object.array_buffer_realm.array_buffer_structure(ArrayBufferSharingMode::Shared)) {
        buffer.set_sharing_mode(ArrayBufferSharingMode::Shared);
    }

    Ok(JSArrayBuffer::create(global_object.vm(), structure, buffer))
}

/// `class JSGenericArrayBufferConstructor<sharingMode>`: no bun um `JSFunction` sobre `NativeExecutable`
/// (não `InternalFunction`), sem campos próprios.
pub struct ArrayBufferConstructor;

impl ArrayBufferConstructor {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(
        vm: &VM,
        global_object: &JSGlobalObject,
        prototype: JSValue,
        sharing_mode: ArrayBufferSharingMode,
    ) -> StructureRef {
        let info = match sharing_mode {
            ArrayBufferSharingMode::Default => &ARRAY_BUFFER_CONSTRUCTOR_S_INFO,
            ArrayBufferSharingMode::Shared => &SHARED_ARRAY_BUFFER_CONSTRUCTOR_S_INFO,
        };
        native_constructor_structure(vm, global_object, prototype, info)
    }

    /// `create(vm, structure, prototype)`: `JSGenericArrayBufferConstructor(vm, structure)` e
    /// `finishCreation(vm, prototype)` (`length` 1, o nome do modo, `prototype`, `@@species` e, só no
    /// `ArrayBuffer`, `isView`).
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        prototype: &JSObject,
        sharing_mode: ArrayBufferSharingMode,
    ) -> JSFunctionRef {
        let construct_function: NativeFunction = match sharing_mode {
            ArrayBufferSharingMode::Default => construct_array_buffer,
            ArrayBufferSharingMode::Shared => construct_shared_array_buffer,
        };
        let constructor = create_native_collection_constructor(
            vm,
            global_object,
            structure,
            prototype,
            sharing_mode.name(),
            1,
            call_array_buffer,
            construct_function,
            true,
        );

        if sharing_mode == ArrayBufferSharingMode::Default {
            put_native_function(vm, global_object, &constructor, &vm.property_names.is_view, 1, array_buffer_func_is_view, Intrinsic::ArrayBufferIsViewIntrinsic);
        }
        constructor
    }
}
