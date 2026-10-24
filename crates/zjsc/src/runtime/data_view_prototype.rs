//! Porte de `runtime/JSDataViewPrototype.h` e `JSDataViewPrototype.cpp`: o `DataView.prototype` (um
//! `JSNonFinalObject` com o `ClassInfo` `"DataView"`): os 22 `getInt8`..`setBigUint64`, os acessores
//! `buffer`, `byteLength` e `byteOffset`, e o `@@toStringTag`.
//!
//! O `getData<Adaptor>`/`setData<Adaptor>` do C++ é um `template` sobre o `TypedArrayAdaptors.h`: aqui o
//! `Adaptor` é o `TypedArrayType` de `typed_array_adaptors.rs`, que sabe a conversão do valor JS para o
//! elemento (`toNativeFromValue`) e a de volta (`toJSValue`). Os bytes de um elemento passam sempre em
//! ordem little-endian (a representação do valor): o `littleEndian` do JS decide só se a ordem do buffer
//! é a inversa.
//!
//! DIVERGÊNCIAS:
//!
//! - `dataViewTable` (as 22 funções `DontEnum|Function` e os acessores `buffer` e `byteOffset`,
//!   `DontEnum|ReadOnly|CustomAccessor`) fica no `ClassInfo` e a `Structure` leva `HasStaticPropertyTable`:
//!   as entradas reificam no primeiro acesso. Só o `byteLength` (`JSC_NATIVE_INTRINSIC_GETTER_WITHOUT_TRANSITION`)
//!   e o `@@toStringTag` seguem no `finishCreation`, como no C++. Ler um `CustomAccessor` não o reifica.
//! - O `toNumber` do porte deixa a exceção pendente no VM (`valueOf`, `@@toPrimitive` de objeto): o
//!   `RETURN_IF_EXCEPTION` do C++ é o `check_exception`/`has_pending_exception` depois de cada conversão.
//!   O `toBigInt` é o de `js_big_int_ops.rs`.

use std::rc::Rc;

use crate::custom_getter;
use crate::host_function;
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array_buffer::to_js_array_buffer;
use crate::runtime::js_data_view::{JSDataView, TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{custom_getter_entry, native_entry_with_intrinsic};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{CUSTOM_ACCESSOR, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::typed_array_adaptors::{read_element, to_js_value, to_native_from_value, write_element};
use crate::runtime::typed_array_type::TypedArrayType;
use crate::runtime::vm::VM;

/// `const ClassInfo JSDataViewPrototype::s_info`.
pub static JS_DATA_VIEW_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "DataView",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&DATA_VIEW_TABLE),
    inherits_js_type_range: None,
};

/// O trecho de `getData`/`setData` depois da leitura do `littleEndian`: o comprimento da visão e o limite.
fn check_bounds(data_view: &JSDataView, byte_offset: u64, element_size: usize) -> Result<usize, Thrown> {
    let Some(byte_length) = data_view.view_byte_length() else {
        return Err(Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE));
    };

    if element_size > byte_length || byte_offset > (byte_length - element_size) as u64 {
        return Err(Thrown::range_error("Out of bounds access"));
    }

    Ok(byte_offset as usize)
}

/// `callFrame->argument(index).toBoolean(globalObject)` quando há o argumento (e o elemento tem mais de um
/// byte): o `littleEndian`.
fn little_endian_argument(call: &HostCall, index: usize, element_size: usize) -> bool {
    element_size > 1 && call.argument_count() > index && call.argument(index).to_boolean()
}

/// O `JSDataView` do `this` de `getData`/`setData`.
fn this_data_view(call: &HostCall) -> Result<Rc<JSDataView>, Thrown> {
    JSDataView::from_value(&call.this_value()).ok_or_else(|| Thrown::type_error("Receiver of DataView method must be a DataView"))
}

/// Os bytes do elemento na ordem do buffer: a representação do valor é little-endian, então o
/// `littleEndian` falso inverte os `element_size` bytes (o `Adaptor` só conhece a ordem do valor).
fn buffer_order(element: TypedArrayType, raw_bytes: &mut [u8; 8], little_endian: bool) -> &[u8] {
    let bytes = &mut raw_bytes[..element.element_size()];
    if !little_endian {
        bytes.reverse();
    }
    bytes
}

fn get_data(_global_object: &JSGlobalObject, call: &HostCall, element: TypedArrayType) -> HostResult {
    let data_view = this_data_view(call)?;

    let byte_offset = call.argument(0).to_index("byteOffset")?;

    let element_size = element.element_size();
    let little_endian = little_endian_argument(call, 1, element_size);

    let byte_offset = check_bounds(&data_view, byte_offset, element_size)?;

    let mut raw_bytes = [0u8; 8];
    data_view.read_bytes(byte_offset, &mut raw_bytes[..element_size]);
    to_js_value(element, read_element(element, buffer_order(element, &mut raw_bytes, little_endian)))
}

fn set_data(global_object: &JSGlobalObject, call: &HostCall, element: TypedArrayType) -> HostResult {
    let data_view = this_data_view(call)?;

    let byte_offset = call.argument(0).to_index("byteOffset")?;

    let element_size = element.element_size();
    let native = to_native_from_value(global_object, element, call.argument(1))?;

    let little_endian = little_endian_argument(call, 2, element_size);

    let byte_offset = check_bounds(&data_view, byte_offset, element_size)?;

    let mut raw_bytes = [0u8; 8];
    write_element(element, &mut raw_bytes, native);
    data_view.write_bytes(byte_offset, buffer_order(element, &mut raw_bytes, little_endian));

    Ok(JSValue::undefined())
}

/// O `this` de um acessor (`expects |this| to be a DataView object`).
fn this_data_view_for_getter(this_value: JSValue, property: &str) -> Result<Rc<JSDataView>, Thrown> {
    JSDataView::from_value(&this_value)
        .ok_or_else(|| Thrown::type_error(&format!("DataView.prototype.{property} expects |this| to be a DataView object")))
}

/// `dataViewProtoGetterBuffer` (`CustomAccessor`).
fn data_view_proto_getter_buffer_body(global_object: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    let view = this_data_view_for_getter(this_value, "buffer")?;
    Ok(to_js_array_buffer(global_object, view.possibly_shared_buffer()).as_value())
}

fn data_view_proto_getter_byte_length_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = this_data_view_for_getter(call.this_value(), "byteLength")?;
    let Some(byte_length) = view.view_byte_length() else {
        return Err(Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE));
    };
    Ok(js_number(byte_length as f64))
}

/// `dataViewProtoGetterByteOffset` (`CustomAccessor`).
fn data_view_proto_getter_byte_offset_body(_global_object: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    let view = this_data_view_for_getter(this_value, "byteOffset")?;
    if view.view_byte_length().is_none() {
        return Err(Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE));
    }
    Ok(js_number(view.byte_offset_raw() as f64))
}

custom_getter!(data_view_proto_getter_buffer, data_view_proto_getter_buffer_body);
host_function!(data_view_proto_getter_byte_length, data_view_proto_getter_byte_length_body);
custom_getter!(data_view_proto_getter_byte_offset, data_view_proto_getter_byte_offset_body);

/// `JSC_DEFINE_HOST_FUNCTION(dataViewProtoFuncGetInt8, ...)` e as 21 irmãs: `getData<Adaptor>` ou
/// `setData<Adaptor>` do `TypedArrayType`.
macro_rules! data_view_host_function {
    ($host:ident, $body:ident, $access:ident, $element:ident) => {
        fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            $access(global_object, call, TypedArrayType::$element)
        }
        host_function!($host, $body);
    };
}

data_view_host_function!(data_view_proto_func_get_int8, data_view_proto_func_get_int8_body, get_data, Int8);
data_view_host_function!(data_view_proto_func_get_uint8, data_view_proto_func_get_uint8_body, get_data, Uint8);
data_view_host_function!(data_view_proto_func_get_int16, data_view_proto_func_get_int16_body, get_data, Int16);
data_view_host_function!(data_view_proto_func_get_uint16, data_view_proto_func_get_uint16_body, get_data, Uint16);
data_view_host_function!(data_view_proto_func_get_int32, data_view_proto_func_get_int32_body, get_data, Int32);
data_view_host_function!(data_view_proto_func_get_uint32, data_view_proto_func_get_uint32_body, get_data, Uint32);
data_view_host_function!(data_view_proto_func_get_float16, data_view_proto_func_get_float16_body, get_data, Float16);
data_view_host_function!(data_view_proto_func_get_float32, data_view_proto_func_get_float32_body, get_data, Float32);
data_view_host_function!(data_view_proto_func_get_float64, data_view_proto_func_get_float64_body, get_data, Float64);
data_view_host_function!(data_view_proto_func_get_big_int64, data_view_proto_func_get_big_int64_body, get_data, BigInt64);
data_view_host_function!(data_view_proto_func_get_big_uint64, data_view_proto_func_get_big_uint64_body, get_data, BigUint64);
data_view_host_function!(data_view_proto_func_set_int8, data_view_proto_func_set_int8_body, set_data, Int8);
data_view_host_function!(data_view_proto_func_set_uint8, data_view_proto_func_set_uint8_body, set_data, Uint8);
data_view_host_function!(data_view_proto_func_set_int16, data_view_proto_func_set_int16_body, set_data, Int16);
data_view_host_function!(data_view_proto_func_set_uint16, data_view_proto_func_set_uint16_body, set_data, Uint16);
data_view_host_function!(data_view_proto_func_set_int32, data_view_proto_func_set_int32_body, set_data, Int32);
data_view_host_function!(data_view_proto_func_set_uint32, data_view_proto_func_set_uint32_body, set_data, Uint32);
data_view_host_function!(data_view_proto_func_set_float16, data_view_proto_func_set_float16_body, set_data, Float16);
data_view_host_function!(data_view_proto_func_set_float32, data_view_proto_func_set_float32_body, set_data, Float32);
data_view_host_function!(data_view_proto_func_set_float64, data_view_proto_func_set_float64_body, set_data, Float64);
data_view_host_function!(data_view_proto_func_set_big_int64, data_view_proto_func_set_big_int64_body, set_data, BigInt64);
data_view_host_function!(data_view_proto_func_set_big_uint64, data_view_proto_func_set_big_uint64_body, set_data, BigUint64);

/// As linhas de funções de `dataViewTable`: nome, `length` e intrínseco (`DontEnum|Function`).
const DATA_VIEW_FUNCTIONS: [(&str, u32, NativeFunction, Intrinsic); 22] = [
    ("getInt8", 1, data_view_proto_func_get_int8, Intrinsic::DataViewGetInt8),
    ("getUint8", 1, data_view_proto_func_get_uint8, Intrinsic::DataViewGetUint8),
    ("getInt16", 1, data_view_proto_func_get_int16, Intrinsic::DataViewGetInt16),
    ("getUint16", 1, data_view_proto_func_get_uint16, Intrinsic::DataViewGetUint16),
    ("getInt32", 1, data_view_proto_func_get_int32, Intrinsic::DataViewGetInt32),
    ("getUint32", 1, data_view_proto_func_get_uint32, Intrinsic::DataViewGetUint32),
    ("getFloat16", 1, data_view_proto_func_get_float16, Intrinsic::DataViewGetFloat16),
    ("getFloat32", 1, data_view_proto_func_get_float32, Intrinsic::DataViewGetFloat32),
    ("getFloat64", 1, data_view_proto_func_get_float64, Intrinsic::DataViewGetFloat64),
    ("getBigInt64", 1, data_view_proto_func_get_big_int64, Intrinsic::DataViewGetBigInt64),
    ("getBigUint64", 1, data_view_proto_func_get_big_uint64, Intrinsic::DataViewGetBigUint64),
    ("setInt8", 2, data_view_proto_func_set_int8, Intrinsic::DataViewSetInt8),
    ("setUint8", 2, data_view_proto_func_set_uint8, Intrinsic::DataViewSetUint8),
    ("setInt16", 2, data_view_proto_func_set_int16, Intrinsic::DataViewSetInt16),
    ("setUint16", 2, data_view_proto_func_set_uint16, Intrinsic::DataViewSetUint16),
    ("setInt32", 2, data_view_proto_func_set_int32, Intrinsic::DataViewSetInt32),
    ("setUint32", 2, data_view_proto_func_set_uint32, Intrinsic::DataViewSetUint32),
    ("setFloat16", 2, data_view_proto_func_set_float16, Intrinsic::DataViewSetFloat16),
    ("setFloat32", 2, data_view_proto_func_set_float32, Intrinsic::DataViewSetFloat32),
    ("setFloat64", 2, data_view_proto_func_set_float64, Intrinsic::DataViewSetFloat64),
    ("setBigInt64", 2, data_view_proto_func_set_big_int64, Intrinsic::DataViewSetBigInt64),
    ("setBigUint64", 2, data_view_proto_func_set_big_uint64, Intrinsic::DataViewSetBigUint64),
];

/// `dataViewTableValues` de `JSDataViewPrototype.lut.h`, na ordem do `@begin`: as 22 funções
/// (`DontEnum|Function`) de `DATA_VIEW_FUNCTIONS`, depois `buffer` e `byteOffset`.
const DATA_VIEW_TABLE_VALUES_ARRAY: [HashTableValue; 24] = {
    let placeholder = custom_getter_entry("buffer", data_view_proto_getter_buffer);
    let mut values = [placeholder; 24];
    let mut index = 0;
    while index < DATA_VIEW_FUNCTIONS.len() {
        let (key, length, function, intrinsic) = DATA_VIEW_FUNCTIONS[index];
        values[index] = native_entry_with_intrinsic(key, function, length as i32, intrinsic);
        index += 1;
    }
    values[22] = custom_getter_entry("buffer", data_view_proto_getter_buffer);
    values[23] = custom_getter_entry("byteOffset", data_view_proto_getter_byte_offset);
    values
};

static DATA_VIEW_TABLE_VALUES: [HashTableValue; 24] = DATA_VIEW_TABLE_VALUES_ARRAY;

/// `dataViewTable`.
static DATA_VIEW_TABLE: HashTable = HashTable { class_for_this: None, values: &DATA_VIEW_TABLE_VALUES };

/// `class JSDataViewPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct DataViewPrototype;

impl DataViewPrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, DataViewPrototype::STRUCTURE_FLAGS),
            &JS_DATA_VIEW_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `JSDataViewPrototype(vm, structure)` e o `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        DataViewPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`: só o `byteLength`; as 22 funções, `buffer` e `byteOffset` vêm de
    /// `dataViewTable` e reificam no primeiro acesso. Ordem do golden (`Reflect.ownKeys` do bun): os nomes da
    /// tabela primeiro, depois `byteLength`, `constructor` e o `@@toStringTag` (ver `put_to_string_tag_tail`).
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        // JSC_NATIVE_INTRINSIC_GETTER_WITHOUT_TRANSITION(byteLength, ..., DataViewByteLengthIntrinsic).
        put_native_getter(
            vm,
            global_object,
            prototype,
            "byteLength",
            data_view_proto_getter_byte_length,
            Intrinsic::DataViewByteLengthIntrinsic,
            DONT_ENUM | READ_ONLY,
        );
    }

    /// `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`: no golden vem depois do `constructor`, que o global
    /// instala depois de criar o construtor.
    pub fn put_to_string_tag_tail(prototype: &JSObject, vm: &VM) {
        put_to_string_tag(vm, prototype, JS_DATA_VIEW_PROTOTYPE_S_INFO.class_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_big_int_ops::to_big_uint64_value;
    use crate::runtime::typed_array_adaptors::to_native_from_double;

    /// O valor JS que o elemento do tipo guarda ao receber o número de `value`.
    fn round_trip(type_: TypedArrayType, value: JSValue) -> JSValue {
        to_js_value(type_, to_native_from_double(type_, value.to_number())).unwrap()
    }

    #[test]
    fn buffer_order_reverses_only_the_element_bytes() {
        let mut raw_bytes = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(buffer_order(TypedArrayType::Uint32, &mut raw_bytes, true), &[1, 2, 3, 4]);
        assert_eq!(buffer_order(TypedArrayType::Uint32, &mut raw_bytes, false), &[4, 3, 2, 1]);
        assert_eq!(buffer_order(TypedArrayType::Uint8, &mut raw_bytes, false), &[4]);
    }

    #[test]
    fn integer_elements_wrap_like_to_int32() {
        let cases = [
            (TypedArrayType::Int8, JSValue::Int32(200), -56),
            (TypedArrayType::Uint8, JSValue::Int32(-1), 255),
            (TypedArrayType::Int16, JSValue::Double(65535.5), -1),
            (TypedArrayType::Uint16, JSValue::Int32(-1), 65535),
            (TypedArrayType::Int32, JSValue::Double(4294967295.0), -1),
        ];
        for (element, value, expected) in cases {
            assert_eq!(round_trip(element, value), JSValue::Int32(expected), "{element:?}");
        }

        assert_eq!(round_trip(TypedArrayType::Uint32, JSValue::Int32(-1)), JSValue::Double(4294967295.0));
        assert_eq!(round_trip(TypedArrayType::Int32, JSValue::Undefined), JSValue::Int32(0));
    }

    #[test]
    fn float_elements_round_to_their_precision() {
        assert_eq!(round_trip(TypedArrayType::Float32, JSValue::Double(0.1)), JSValue::Double(0.1f32 as f64));
        assert_eq!(round_trip(TypedArrayType::Float16, JSValue::Double(1.337)), JSValue::Double(1.3369140625));
        assert_eq!(round_trip(TypedArrayType::Float64, JSValue::Int32(7)), JSValue::Double(7.0));
        assert!(matches!(round_trip(TypedArrayType::Float64, JSValue::Undefined), JSValue::Double(value) if value.is_nan()));
    }

    #[test]
    fn big_int_elements_hold_the_value_modulo_two_to_the_64() {
        // O BigInt é uma célula: a conversão precisa de um `JSGlobalObject` vivo.
        let (_vm, global_object) = crate::api::eval::new_global_object();
        let _realm = crate::runtime::current_realm::CurrentRealmScope::enter(&global_object);
        let value = to_js_value(TypedArrayType::BigInt64, (-3i64) as u64).unwrap();
        assert_eq!(to_big_uint64_value(value) as i64, -3);

        let value = to_js_value(TypedArrayType::BigUint64, u64::MAX).unwrap();
        assert_eq!(to_big_uint64_value(value), u64::MAX);
    }
}
