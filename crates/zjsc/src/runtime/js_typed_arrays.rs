//! Porte de `runtime/JSTypedArrays.h`/`JSTypedArrays.cpp` (a parte dos `ClassInfo`): os 24 `s_info` de
//! `JSInt8Array`... `JSBigUint64Array` e de `JSResizableOrGrowableShared*Array`, e o
//! `get<Tipo>ArrayClassInfo()` de cada um.
//!
//! DIVERGÊNCIA: `JSGenericTypedArrayView<Adaptor>` é um tipo só (ver `js_generic_typed_array_view.rs`), então
//! `JSInt8Array` e as irmãs não existem como tipos; o que as distingue é o `TypedArrayType` e o
//! `ClassInfo`, que sai de `typed_array_class_info`.

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_array_buffer_view::JS_ARRAY_BUFFER_VIEW_S_INFO;
use crate::runtime::typed_array_type::TypedArrayType;

macro_rules! typed_array_class_infos {
    ($(($plain:ident, $resizable:ident, $name:literal)),* $(,)?) => {
        $(
            /// `JS<Tipo>Array::s_info`.
            pub static $plain: ClassInfo =
                ClassInfo { class_name: $name, parent_class: Some(&JS_ARRAY_BUFFER_VIEW_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
            /// `JSResizableOrGrowableShared<Tipo>Array::s_info`: o pai é o `s_info` da visão comum.
            pub static $resizable: ClassInfo =
                ClassInfo { class_name: $name, parent_class: Some(&$plain), static_prop_hash_table: None, inherits_js_type_range: None };
        )*
    };
}

typed_array_class_infos!(
    (JS_INT8_ARRAY_S_INFO, JS_RESIZABLE_INT8_ARRAY_S_INFO, "Int8Array"),
    (JS_UINT8_ARRAY_S_INFO, JS_RESIZABLE_UINT8_ARRAY_S_INFO, "Uint8Array"),
    (JS_UINT8_CLAMPED_ARRAY_S_INFO, JS_RESIZABLE_UINT8_CLAMPED_ARRAY_S_INFO, "Uint8ClampedArray"),
    (JS_INT16_ARRAY_S_INFO, JS_RESIZABLE_INT16_ARRAY_S_INFO, "Int16Array"),
    (JS_UINT16_ARRAY_S_INFO, JS_RESIZABLE_UINT16_ARRAY_S_INFO, "Uint16Array"),
    (JS_INT32_ARRAY_S_INFO, JS_RESIZABLE_INT32_ARRAY_S_INFO, "Int32Array"),
    (JS_UINT32_ARRAY_S_INFO, JS_RESIZABLE_UINT32_ARRAY_S_INFO, "Uint32Array"),
    (JS_FLOAT16_ARRAY_S_INFO, JS_RESIZABLE_FLOAT16_ARRAY_S_INFO, "Float16Array"),
    (JS_FLOAT32_ARRAY_S_INFO, JS_RESIZABLE_FLOAT32_ARRAY_S_INFO, "Float32Array"),
    (JS_FLOAT64_ARRAY_S_INFO, JS_RESIZABLE_FLOAT64_ARRAY_S_INFO, "Float64Array"),
    (JS_BIG_INT64_ARRAY_S_INFO, JS_RESIZABLE_BIG_INT64_ARRAY_S_INFO, "BigInt64Array"),
    (JS_BIG_UINT64_ARRAY_S_INFO, JS_RESIZABLE_BIG_UINT64_ARRAY_S_INFO, "BigUint64Array"),
);

/// `JSGenericTypedArrayView<Adaptor>::info()` e `JSGenericResizableOrGrowableSharedTypedArrayView::info()`.
pub fn typed_array_class_info(type_: TypedArrayType, resizable_or_growable_shared: bool) -> &'static ClassInfo {
    let (plain, resizable) = match type_ {
        TypedArrayType::Int8 => (&JS_INT8_ARRAY_S_INFO, &JS_RESIZABLE_INT8_ARRAY_S_INFO),
        TypedArrayType::Uint8 => (&JS_UINT8_ARRAY_S_INFO, &JS_RESIZABLE_UINT8_ARRAY_S_INFO),
        TypedArrayType::Uint8Clamped => (&JS_UINT8_CLAMPED_ARRAY_S_INFO, &JS_RESIZABLE_UINT8_CLAMPED_ARRAY_S_INFO),
        TypedArrayType::Int16 => (&JS_INT16_ARRAY_S_INFO, &JS_RESIZABLE_INT16_ARRAY_S_INFO),
        TypedArrayType::Uint16 => (&JS_UINT16_ARRAY_S_INFO, &JS_RESIZABLE_UINT16_ARRAY_S_INFO),
        TypedArrayType::Int32 => (&JS_INT32_ARRAY_S_INFO, &JS_RESIZABLE_INT32_ARRAY_S_INFO),
        TypedArrayType::Uint32 => (&JS_UINT32_ARRAY_S_INFO, &JS_RESIZABLE_UINT32_ARRAY_S_INFO),
        TypedArrayType::Float16 => (&JS_FLOAT16_ARRAY_S_INFO, &JS_RESIZABLE_FLOAT16_ARRAY_S_INFO),
        TypedArrayType::Float32 => (&JS_FLOAT32_ARRAY_S_INFO, &JS_RESIZABLE_FLOAT32_ARRAY_S_INFO),
        TypedArrayType::Float64 => (&JS_FLOAT64_ARRAY_S_INFO, &JS_RESIZABLE_FLOAT64_ARRAY_S_INFO),
        TypedArrayType::BigInt64 => (&JS_BIG_INT64_ARRAY_S_INFO, &JS_RESIZABLE_BIG_INT64_ARRAY_S_INFO),
        TypedArrayType::BigUint64 => (&JS_BIG_UINT64_ARRAY_S_INFO, &JS_RESIZABLE_BIG_UINT64_ARRAY_S_INFO),
        TypedArrayType::NotTypedArray | TypedArrayType::DataView => panic!("info() de {type_:?}"),
    };
    if resizable_or_growable_shared { resizable } else { plain }
}
