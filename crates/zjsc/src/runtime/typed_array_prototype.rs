//! Porte de `runtime/JSTypedArrayViewPrototype.{h,cpp}` (o `%TypedArray%.prototype`),
//! `JSGenericTypedArrayViewPrototype.h`, `JSGenericTypedArrayViewPrototypeInlines.h` e
//! `JSTypedArrayPrototypes.{h,cpp}` (os protótipos concretos, `Int8Array.prototype`...): a criação dos
//! protótipos e as funções nativas do `%TypedArray%.prototype` que não dependem do tipo
//! (`entries`/`keys`/`values`, `@@toStringTag`) mais as `LinkTimeConstant` privadas
//! `typedArrayLength`, `isTypedArrayView`, `isSharedTypedArrayView`,
//! `isResizableOrGrowableSharedTypedArrayView`, `typedArrayFromFast`, `isDetached` e
//! `isTypedArrayOutOfBounds`, que os builtins JS (`TypedArrayConstructor.js`, `TypedArrayPrototype.js`,
//! `ArrayIteratorPrototype.js`) chamam.
//!
//! As funções que dependem do tipo estão em `typed_array_prototype_natives.rs` e
//! `typed_array_prototype_natives_part2.rs`.
//!
//! DIVERGÊNCIAS:
//!
//! - `installTypedArrayPrototypeIteratorProtocolWatchpoint` e `installTypedArrayIteratorProtocolWatchpoint`
//!   (watchpoints) não existem.
//! - `typedArrayFromFast` (`genericTypedArrayViewPrivateFuncFromFast`) é só uma otimização de `from`
//!   (`TypedArrayConstructor.js` cai no caminho genérico quando ela devolve `undefined`) que depende de
//!   `isIteratorProtocolFastAndNonObservable` e dos atalhos `copyFromInt32ShapeArray` e
//!   `copyFromDoubleShapeArray`: aqui devolve sempre `undefined`, e o resultado observável é o do caminho
//!   genérico.
//! - `toString` é a função que o `Array.prototype` já tem (`arrayProtoToStringFunction()` do C++ é essa
//!   mesma função).
//! - O `Uint8Array` base64 e hex (`setFromBase64`, `setFromHex`, `toBase64`, `toHex`, `fromBase64`,
//!   `fromHex`) está em `uint8_array_base64.rs`.

use crate::host_function;
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::js_array_iterator::JSArrayIterator;
use crate::runtime::js_function::{
    call_host_function_as_constructor, put_direct_builtin_function_without_transition,
    put_direct_native_function_without_transition, JSFunction,
};
use crate::runtime::js_generic_typed_array_view::{check_typed_array_in_bounds, JSGenericTypedArrayView};
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_number, js_undefined, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{ACCESSOR, DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::typed_array_prototype_natives::{
    typed_array_view_proto_func_copy_within, typed_array_view_proto_func_fill, typed_array_view_proto_func_includes,
    typed_array_view_proto_func_index_of, typed_array_view_proto_func_join, typed_array_view_proto_func_last_index_of,
    typed_array_view_proto_func_reverse, typed_array_view_proto_func_set, typed_array_view_proto_func_sort,
    typed_array_view_proto_func_to_reversed, typed_array_view_proto_func_to_sorted, typed_array_view_proto_func_with,
    typed_array_view_proto_getter_func_buffer, typed_array_view_proto_getter_func_byte_length,
    typed_array_view_proto_getter_func_byte_offset, typed_array_view_proto_getter_func_length,
};
use crate::runtime::typed_array_prototype_natives_part2::{
    typed_array_view_proto_func_every, typed_array_view_proto_func_filter, typed_array_view_proto_func_find,
    typed_array_view_proto_func_find_index, typed_array_view_proto_func_find_last,
    typed_array_view_proto_func_find_last_index, typed_array_view_proto_func_for_each, typed_array_view_proto_func_map,
    typed_array_view_proto_func_reduce, typed_array_view_proto_func_reduce_right, typed_array_view_proto_func_slice,
    typed_array_view_proto_func_some, typed_array_view_proto_func_subarray,
};
use crate::runtime::typed_array_type::TypedArrayType;
use crate::runtime::uint8_array_base64;
use crate::runtime::vm::VM;
use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo JSTypedArrayViewPrototype::s_info` (`"Prototype"`).
pub static TYPED_ARRAY_VIEW_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Prototype", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// Os `ClassInfo` de `JSInt8ArrayPrototype`... `JSBigUint64ArrayPrototype` (`MAKE_S_INFO` de
/// `JSTypedArrayPrototypes.cpp`), na ordem de `TypedArrayType::to_index`.
pub static TYPED_ARRAY_PROTOTYPE_S_INFOS: [ClassInfo; 12] = {
    macro_rules! infos {
        ($($name:literal),* $(,)?) => {
            [$(ClassInfo { class_name: $name, parent_class: Some(&TYPED_ARRAY_VIEW_PROTOTYPE_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None }),*]
        };
    }
    infos!(
        "Int8ArrayPrototype",
        "Uint8ArrayPrototype",
        "Uint8ClampedArrayPrototype",
        "Int16ArrayPrototype",
        "Uint16ArrayPrototype",
        "Int32ArrayPrototype",
        "Uint32ArrayPrototype",
        "Float16ArrayPrototype",
        "Float32ArrayPrototype",
        "Float64ArrayPrototype",
        "BigInt64ArrayPrototype",
        "BigUint64ArrayPrototype",
    )
};

fn typed_array_view_private_func_is_typed_array_view_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(JSGenericTypedArrayView::from_value(&call.argument(0)).is_some()))
}

fn typed_array_view_private_func_is_shared_typed_array_view_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(JSGenericTypedArrayView::from_value(&call.argument(0)).is_some_and(|view| view.is_shared())))
}

fn typed_array_view_private_func_is_resizable_or_growable_shared_typed_array_view_body(
    _global_object: &JSGlobalObject,
    call: &HostCall,
) -> HostResult {
    Ok(js_boolean(JSGenericTypedArrayView::from_value(&call.argument(0)).is_some_and(|view| view.is_resizable_or_growable_shared())))
}

fn typed_array_view_private_func_is_detached_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    debug_assert!(JSGenericTypedArrayView::from_value(&argument).is_some());
    Ok(js_boolean(JSGenericTypedArrayView::from_value(&argument).is_some_and(|view| view.is_detached())))
}

fn typed_array_view_private_func_is_out_of_bounds_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    debug_assert!(JSGenericTypedArrayView::from_value(&argument).is_some());
    Ok(js_boolean(JSGenericTypedArrayView::from_value(&argument).is_some_and(|view| view.is_out_of_bounds())))
}

fn typed_array_view_private_func_length_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(this_object) = JSGenericTypedArrayView::from_value(&call.argument(0)) else {
        return Err(Thrown::type_error("Receiver should be a typed array view"));
    };

    check_typed_array_in_bounds(&this_object)?;

    Ok(js_number(this_object.length() as f64))
}

fn typed_array_view_private_func_typed_array_from_fast_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_undefined())
}

fn create_typed_array_iterator_object(global_object: &JSGlobalObject, call: &HostCall, kind: IterationKind) -> HostResult {
    let Some(this_object) = JSGenericTypedArrayView::from_value(&call.this_value()) else {
        return Err(Thrown::type_error("Receiver should be a typed array view"));
    };

    check_typed_array_in_bounds(&this_object)?;

    Ok(JSArrayIterator::create(global_object.vm(), &global_object.array_iterator_structure(), &this_object, kind).as_value())
}

fn typed_array_view_proto_func_values_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    create_typed_array_iterator_object(global_object, call, IterationKind::Values)
}

fn typed_array_proto_view_func_entries_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    create_typed_array_iterator_object(global_object, call, IterationKind::Entries)
}

fn typed_array_view_proto_func_keys_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    create_typed_array_iterator_object(global_object, call, IterationKind::Keys)
}

fn typed_array_view_proto_getter_func_to_string_tag_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = call.this_value();
    if !this_value.is_object() {
        return Ok(js_undefined());
    }

    match JSGenericTypedArrayView::from_value(&this_value) {
        Some(view) => Ok(JSValue::from_js_string(js_string(
            global_object.vm(),
            &WtfString::from_latin1(view.typed_array_type().class_name().as_bytes()),
        ))),
        None => Ok(js_undefined()),
    }
}

host_function!(typed_array_view_private_func_is_typed_array_view, typed_array_view_private_func_is_typed_array_view_body);
host_function!(
    typed_array_view_private_func_is_shared_typed_array_view,
    typed_array_view_private_func_is_shared_typed_array_view_body
);
host_function!(
    typed_array_view_private_func_is_resizable_or_growable_shared_typed_array_view,
    typed_array_view_private_func_is_resizable_or_growable_shared_typed_array_view_body
);
host_function!(typed_array_view_private_func_is_detached, typed_array_view_private_func_is_detached_body);
host_function!(typed_array_view_private_func_is_out_of_bounds, typed_array_view_private_func_is_out_of_bounds_body);
host_function!(typed_array_view_private_func_length, typed_array_view_private_func_length_body);
host_function!(typed_array_view_private_func_typed_array_from_fast, typed_array_view_private_func_typed_array_from_fast_body);
host_function!(typed_array_view_proto_func_values, typed_array_view_proto_func_values_body);
host_function!(typed_array_proto_view_func_entries, typed_array_proto_view_func_entries_body);
host_function!(typed_array_view_proto_func_keys, typed_array_view_proto_func_keys_body);
host_function!(typed_array_view_proto_getter_func_to_string_tag, typed_array_view_proto_getter_func_to_string_tag_body);

/// As `m_linkTimeConstants[...].initLater` de `typedArrayLength`, `isTypedArrayView`,
/// `isSharedTypedArrayView`, `isResizableOrGrowableSharedTypedArrayView`, `typedArrayFromFast`,
/// `isDetached` e `isTypedArrayOutOfBounds` (`JSGlobalObject.cpp:2102`), criadas de uma vez.
pub fn install_private_functions(vm: &VM, global_object: &JSGlobalObject) {
    let install = |id: LinkTimeConstant, length: u32, name: &str, function: NativeFunction, intrinsic: Intrinsic| {
        let function = JSFunction::create_native(
            vm,
            global_object,
            length,
            &WtfString::from_latin1(name.as_bytes()),
            function,
            ImplementationVisibility::Private,
            intrinsic,
            call_host_function_as_constructor,
        );
        global_object.set_link_time_constant(id, function.as_value());
    };

    install(
        LinkTimeConstant::TypedArrayLength,
        0,
        "typedArrayViewLength",
        typed_array_view_private_func_length,
        Intrinsic::NoIntrinsic,
    );
    install(
        LinkTimeConstant::IsTypedArrayView,
        1,
        "typedArrayViewIsTypedArrayView",
        typed_array_view_private_func_is_typed_array_view,
        Intrinsic::IsTypedArrayViewIntrinsic,
    );
    install(
        LinkTimeConstant::IsSharedTypedArrayView,
        1,
        "typedArrayViewIsSharedTypedArrayView",
        typed_array_view_private_func_is_shared_typed_array_view,
        Intrinsic::NoIntrinsic,
    );
    install(
        LinkTimeConstant::IsResizableOrGrowableSharedTypedArrayView,
        1,
        "typedArrayViewPrivateFuncIsResizableOrGrowableSharedTypedArrayView",
        typed_array_view_private_func_is_resizable_or_growable_shared_typed_array_view,
        Intrinsic::NoIntrinsic,
    );
    install(
        LinkTimeConstant::TypedArrayFromFast,
        2,
        "typedArrayViewTypedArrayFromFast",
        typed_array_view_private_func_typed_array_from_fast,
        Intrinsic::NoIntrinsic,
    );
    install(
        LinkTimeConstant::IsDetached,
        1,
        "typedArrayViewIsDetached",
        typed_array_view_private_func_is_detached,
        Intrinsic::NoIntrinsic,
    );
    install(
        LinkTimeConstant::IsTypedArrayOutOfBounds,
        1,
        "typedArrayViewIsOutOfBounds",
        typed_array_view_private_func_is_out_of_bounds,
        Intrinsic::NoIntrinsic,
    );
}

/// `class JSTypedArrayViewPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct TypedArrayViewPrototype;

impl TypedArrayViewPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TypedArrayViewPrototype::STRUCTURE_FLAGS),
            &TYPED_ARRAY_VIEW_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `JSTypedArrayViewPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        TypedArrayViewPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`, na ordem do C++.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        let names = &vm.property_names;
        let literal = |text: &str| Identifier::from_span(vm, text.as_bytes());
        let dont_enum_read_only = DONT_ENUM | READ_ONLY;

        // `putDirectWithoutTransition(vm, vm.propertyNames->toString, globalObject->arrayProtoToStringFunction(), DontEnum)`:
        // o `toString` que o `Array.prototype` tem.
        let array_prototype = JSObject::from_value(&global_object.array_structure().stored_prototype())
            .expect("o protótipo da estrutura de Array é o Array.prototype");
        let array_proto_to_string =
            array_prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&names.to_string));
        prototype.put_direct_without_transition(vm, &PropertyName::from_identifier(&names.to_string), array_proto_to_string, DONT_ENUM);

        let getter = |name: &str, function: NativeFunction, intrinsic: Intrinsic| {
            put_native_getter(vm, global_object, prototype, name, function, intrinsic, dont_enum_read_only);
        };
        let method = |name: &Identifier, length: u32, function: NativeFunction, intrinsic: Intrinsic| {
            put_direct_native_function_without_transition(
                vm,
                global_object,
                prototype,
                name,
                length,
                function,
                ImplementationVisibility::Public,
                intrinsic,
                DONT_ENUM,
            );
        };
        let no_intrinsic = Intrinsic::NoIntrinsic;

        getter("buffer", typed_array_view_proto_getter_func_buffer, no_intrinsic);
        getter("byteLength", typed_array_view_proto_getter_func_byte_length, Intrinsic::TypedArrayByteLengthIntrinsic);
        getter("byteOffset", typed_array_view_proto_getter_func_byte_offset, Intrinsic::TypedArrayByteOffsetIntrinsic);
        method(&literal("copyWithin"), 2, typed_array_view_proto_func_copy_within, no_intrinsic);
        method(&names.sort, 1, typed_array_view_proto_func_sort, no_intrinsic);
        method(&literal("every"), 1, typed_array_view_proto_func_every, no_intrinsic);
        method(&literal("filter"), 1, typed_array_view_proto_func_filter, no_intrinsic);
        method(&literal("entries"), 0, typed_array_proto_view_func_entries, Intrinsic::TypedArrayEntriesIntrinsic);
        method(&literal("includes"), 1, typed_array_view_proto_func_includes, no_intrinsic);
        method(&names.fill, 1, typed_array_view_proto_func_fill, no_intrinsic);
        method(&literal("find"), 1, typed_array_view_proto_func_find, no_intrinsic);
        method(&literal("findLast"), 1, typed_array_view_proto_func_find_last, no_intrinsic);
        method(&literal("findIndex"), 1, typed_array_view_proto_func_find_index, no_intrinsic);
        method(&literal("findLastIndex"), 1, typed_array_view_proto_func_find_last_index, no_intrinsic);
        method(&names.for_each, 1, typed_array_view_proto_func_for_each, no_intrinsic);
        method(&literal("indexOf"), 1, typed_array_view_proto_func_index_of, no_intrinsic);
        method(&names.join, 1, typed_array_view_proto_func_join, no_intrinsic);
        method(&literal("keys"), 0, typed_array_view_proto_func_keys, Intrinsic::TypedArrayKeysIntrinsic);
        method(&literal("lastIndexOf"), 1, typed_array_view_proto_func_last_index_of, no_intrinsic);

        // `JSFunction::create(vm, globalObject, 0, "get length"_s, typedArrayViewProtoGetterFuncLength, ...,
        // TypedArrayLengthIntrinsic)` num `GetterSetter` sob `length`.
        getter("length", typed_array_view_proto_getter_func_length, Intrinsic::TypedArrayLengthIntrinsic);

        method(&literal("map"), 1, typed_array_view_proto_func_map, no_intrinsic);
        method(&literal("reduce"), 1, typed_array_view_proto_func_reduce, no_intrinsic);
        method(&literal("reduceRight"), 1, typed_array_view_proto_func_reduce_right, no_intrinsic);
        method(&literal("reverse"), 0, typed_array_view_proto_func_reverse, no_intrinsic);
        method(&names.set, 1, typed_array_view_proto_func_set, no_intrinsic);
        method(&names.slice, 2, typed_array_view_proto_func_slice, no_intrinsic);
        method(&literal("some"), 1, typed_array_view_proto_func_some, no_intrinsic);
        method(&names.subarray, 2, typed_array_view_proto_func_subarray, no_intrinsic);
        put_direct_builtin_function_without_transition(
            vm,
            global_object,
            prototype,
            &names.to_locale_string,
            BuiltinCodeIndex::TypedArrayPrototypeToLocaleStringCode,
            DONT_ENUM,
        );
        method(&literal("toReversed"), 0, typed_array_view_proto_func_to_reversed, no_intrinsic);
        method(&literal("toSorted"), 1, typed_array_view_proto_func_to_sorted, no_intrinsic);
        method(&names.with_keyword, 2, typed_array_view_proto_func_with, no_intrinsic);
        put_direct_builtin_function_without_transition(
            vm,
            global_object,
            prototype,
            &literal("at"),
            BuiltinCodeIndex::TypedArrayPrototypeAtCode,
            DONT_ENUM,
        );

        // `@@toStringTag`: o `GetterSetter` do `JSFunction` `"get [Symbol.toStringTag]"`.
        let to_string_tag_function = JSFunction::create_native(
            vm,
            global_object,
            0,
            &WtfString::from_latin1(b"get [Symbol.toStringTag]"),
            typed_array_view_proto_getter_func_to_string_tag,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            call_host_function_as_constructor,
        );
        let to_string_tag_accessor = GetterSetter::create_from_values(vm, to_string_tag_function.as_value(), js_undefined());
        prototype.put_direct_non_index_accessor_without_transition(
            vm,
            &PropertyName::from_identifier(&names.to_string_tag_symbol),
            &to_string_tag_accessor,
            dont_enum_read_only | ACCESSOR,
        );

        // `values` e `@@iterator` são a mesma função.
        let values_name = literal("values");
        let values_function = JSFunction::create_native(
            vm,
            global_object,
            0,
            &WtfString::from_latin1(b"values"),
            typed_array_view_proto_func_values,
            ImplementationVisibility::Public,
            Intrinsic::TypedArrayValuesIntrinsic,
            call_host_function_as_constructor,
        );
        prototype.put_direct_without_transition(vm, &PropertyName::from_identifier(&values_name), values_function.as_value(), DONT_ENUM);
        prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&names.iterator_symbol),
            values_function.as_value(),
            DONT_ENUM,
        );
    }

    /// `JSGenericTypedArrayViewPrototype<ViewClass>::createStructure(vm, globalObject, prototype)` com o
    /// `ClassInfo` do protótipo concreto.
    pub fn create_concrete_structure(vm: &VM, global_object: &JSGlobalObject, type_: TypedArrayType, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TypedArrayViewPrototype::STRUCTURE_FLAGS),
            &TYPED_ARRAY_PROTOTYPE_S_INFOS[type_.to_index()],
        )
    }

    /// `JSGenericTypedArrayViewPrototype<ViewClass>::create(vm, globalObject, structure)` e `finishCreation`:
    /// `BYTES_PER_ELEMENT`.
    pub fn create_concrete(vm: &VM, global_object: &JSGlobalObject, type_: TypedArrayType, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        prototype.finish_creation(vm);
        prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.bytes_per_element),
            js_number(type_.element_size() as f64),
            DONT_ENUM | READ_ONLY | DONT_DELETE,
        );
        if type_ == TypedArrayType::Uint8 {
            uint8_array_base64::install_prototype_functions(vm, global_object, &prototype);
        }
        prototype
    }
}
