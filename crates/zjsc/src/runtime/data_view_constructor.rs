//! Porte do `DataView` de `runtime/JSGenericTypedArrayViewConstructor.h`,
//! `JSGenericTypedArrayViewConstructorInlines.h` (`constructDataViewImpl` e
//! `constructGenericTypedArrayViewWithArrayBuffer<JSDataView>`) e `JSTypedArrayConstructors.{h,cpp}`: o
//! construtor `DataView` (o `JSGenericTypedArrayViewConstructor<JSDataView>`, no bun um `JSFunction`), com
//! `new DataView(buffer, byteOffset, byteLength)` e `BYTES_PER_ELEMENT`.
//!
//! O `JSGenericTypedArrayViewConstructor<ViewClass>` dos demais `TypedArray` não é desta fatia.
//!
//! DIVERGÊNCIAS:
//!
//! - A estrutura base do `JSC_GET_DERIVED_STRUCTURE` é a do `ArrayBufferRealm` (a de buffer
//!   redimensionável ou compartilhado crescível, ou a comum, conforme o buffer), lida no realm de
//!   `getFunctionRealm(newTarget)`.
//! - `toIndex` usa o `toIntegerOrInfinity` do porte, que ainda não converte objeto (ver `js_array_buffer.rs`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{constructor_cannot_be_called_as_function, create_native_collection_constructor, native_constructor_structure};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::js_function::{JSFunction, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_array_buffer::JSArrayBuffer;
use crate::runtime::js_data_view::{
    JSDataView, ARRAY_BUFFER_VIEW_ERROR_MESSAGE_OUT_OF_RANGE_OF_BUFFER, JS_DATA_VIEW_S_INFO,
    TYPED_ARRAY_ERROR_MESSAGE_BUFFER_IS_ALREADY_DETACHED, TYPED_ARRAY_ERROR_MESSAGE_BYTE_OFFSET_EXCEED_SOURCE_BUFFER_BYTE_LENGTH,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `const ClassInfo JSDataViewConstructor::s_info` (`"Function"`).
pub static DATA_VIEW_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `callDataView`: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, "DataView")`.
fn call_data_view_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function(JS_DATA_VIEW_S_INFO.class_name)
}

/// `constructDataViewImpl(globalObject, callFrame)`, que é o
/// https://tc39.es/ecma262/#sec-dataview-buffer-byteoffset-bytelength.
fn construct_data_view_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // [`DataView`][dataview] and [the other `%TypedArray%` constructors][typedarray] take almost same arguments and behave very similarly
    // but the spec has a different abstract operation steps.
    // It makes a difference for an order of checking their arguments and throwing errors in the detail.
    // And its difference comes with an user observable behavior in a corner case.
    // We should have a separate code path among them to implement the spec correctly & simply.
    //
    // [dataview]: https://tc39.es/ecma262/#sec-dataview-buffer-byteoffset-bytelength
    // [typedarray]: https://tc39.es/ecma262/#sec-typedarray
    let arg_count = call.argument_count();
    if arg_count == 0 {
        return Err(Thrown::type_error("DataView constructor requires at least one argument."));
    }

    let Some(array_buffer) = JSArrayBuffer::from_value(&call.argument(0)) else {
        return Err(Thrown::type_error("Expected ArrayBuffer for the first argument."));
    };

    let buffer = std::rc::Rc::clone(array_buffer.impl_());

    let mut offset = 0;
    if arg_count > 1 {
        offset = call.argument(1).to_index("byteOffset")? as usize;
    }

    if buffer.is_detached() {
        return Err(Thrown::type_error(TYPED_ARRAY_ERROR_MESSAGE_BUFFER_IS_ALREADY_DETACHED));
    }

    let buffer_byte_length = buffer.byte_length();
    if offset > buffer_byte_length {
        return Err(Thrown::range_error(TYPED_ARRAY_ERROR_MESSAGE_BYTE_OFFSET_EXCEED_SOURCE_BUFFER_BYTE_LENGTH));
    }

    let mut length: Option<usize> = None;
    if arg_count > 2 {
        // If the length value is present but undefined, treat it as missing.
        let length_value = call.argument(2);
        if !length_value.is_undefined() {
            let view_byte_length = length_value.to_index("byteLength")? as usize;

            // Accroding to the spec (April 24, 2026),
            // https://tc39.es/ecma262/#sec-dataview-buffer-byteoffset-bytelength defines as the step 9-b that
            // we should throw RangeError rather even if ToIndex(byteLength) happens to detach the buffer as:
            // As user observable behavior, the sequence would be:
            //
            //  9-a: Let viewByteLength be ? ToIndex(byteLength): the weird object can detach the buffer at here.
            //  9-b: If `(offset + viewByteLength) > bufferByteLength`, throw RangeError. <- here.
            //  11: If the buffer is detached, throw TypeError.
            if offset + view_byte_length > buffer_byte_length {
                return Err(Thrown::range_error(ARRAY_BUFFER_VIEW_ERROR_MESSAGE_OUT_OF_RANGE_OF_BUFFER));
            }

            length = Some(view_byte_length);
        }
    }

    let structure = get_derived_structure_in_realm(global_object, call.new_target(), call.callee(), |realm| {
        realm.array_buffer_realm.data_view_structure(buffer.is_resizable_or_growable_shared())
    })?;

    // `constructGenericTypedArrayViewWithArrayBuffer<JSDataView>(globalObject, structure, arrayBuffer, offset, length)`.
    if buffer.is_detached() {
        return Err(Thrown::type_error(TYPED_ARRAY_ERROR_MESSAGE_BUFFER_IS_ALREADY_DETACHED));
    }

    if length.is_none() {
        let byte_length = buffer.byte_length();
        if buffer.is_resizable_or_growable_shared() {
            if offset > byte_length {
                return Err(Thrown::range_error(TYPED_ARRAY_ERROR_MESSAGE_BYTE_OFFSET_EXCEED_SOURCE_BUFFER_BYTE_LENGTH));
            }
        } else {
            // O elemento do `DataView` tem 1 byte: `(byteLength - offset) % elementSize` é sempre 0.
            length = Some((byte_length - offset) / JSDataView::ELEMENT_SIZE);
        }
    }

    Ok(JSDataView::create(global_object, &structure, buffer, offset, length)?.as_value())
}

host_function!(call_data_view, call_data_view_body);
host_function!(construct_data_view, construct_data_view_body);

/// `class JSGenericTypedArrayViewConstructor<JSDataView>`: no bun um `JSFunction` sobre `NativeExecutable`
/// (não `InternalFunction`), sem campos próprios.
pub struct DataViewConstructor;

impl DataViewConstructor {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        native_constructor_structure(vm, global_object, prototype, &DATA_VIEW_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, globalObject, structure, prototype, name)`: `finishCreation` com `length` 1, o nome
    /// `DataView`, `prototype` e `BYTES_PER_ELEMENT` (`jsNumber(JSDataView::elementSize)`).
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: StructureRef, prototype: &JSObject) -> JSFunctionRef {
        let constructor = create_native_collection_constructor(
            vm,
            global_object,
            structure,
            prototype,
            JS_DATA_VIEW_S_INFO.class_name,
            1,
            call_data_view,
            construct_data_view,
            false,
        );
        constructor.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.bytes_per_element),
            js_number(JSDataView::ELEMENT_SIZE as f64),
            DONT_ENUM | READ_ONLY | DONT_DELETE,
        );
        constructor
    }
}
