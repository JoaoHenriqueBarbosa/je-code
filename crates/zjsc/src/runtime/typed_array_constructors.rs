//! Porte de `runtime/JSGenericTypedArrayViewConstructor.{h,cpp}`, `JSGenericTypedArrayViewConstructorInlines.h`
//! e `JSTypedArrayConstructors.{h,cpp}`: os 12 construtores concretos (`Int8Array`, `Uint8Array`,
//! `Uint8ClampedArray`, `Int16Array`, `Uint16Array`, `Int32Array`, `Uint32Array`, `Float16Array`,
//! `Float32Array`, `Float64Array`, `BigInt64Array`, `BigUint64Array`): `new Int8Array(length)`, `(typedArray)`,
//! `(object)` (iterável ou array-like), `(buffer, byteOffset, length)`, `BYTES_PER_ELEMENT` e o `ClassInfo`
//! de cada um (`constructorClassInfoForType`).
//!
//! O `DataView` está em `data_view_constructor.rs`.
//!
//! DIVERGÊNCIAS:
//!
//! - `JSGenericTypedArrayViewConstructor<ViewClass>` é um tipo só, com o `TypedArrayType` como parâmetro
//!   constante das funções nativas (`call_typed_array::<I>`, `construct_typed_array::<I>`, `I` o
//!   `TypedArrayType::to_index`): o `callConstructor()` e o `constructConstructor()` do C++ escolhem, por
//!   `switch`, uma função por tipo.
//! - O atalho de `constructGenericTypedArrayViewFromFastArray` (`JSArray` com
//!   `isIteratorProtocolFastAndNonObservable`) não existe (sem watchpoints): todo objeto que tem `@@iterator` é
//!   percorrido pelo protocolo de iteração e o que não tem é lido como array-like. O que o atalho evita é o que o
//!   protocolo, com o iterador original, faz sem efeito observável.
//! - O `Uint8Array.fromBase64` e `fromHex` estão em `uint8_array_base64.rs`.

use crate::host_function;
use crate::runtime::array_buffer::ArrayBufferRef;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{
    constructor_cannot_be_called_as_function, create_native_collection_constructor, native_constructor_structure,
};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::js_function::{JSFunction, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::iterator_operations::for_each_in_iterable_with_method;
use crate::runtime::js_array_buffer::JSArrayBuffer;
use crate::runtime::js_array_buffer_view::CopyType;
use crate::runtime::js_data_view::{
    TYPED_ARRAY_ERROR_MESSAGE_BUFFER_IS_ALREADY_DETACHED, TYPED_ARRAY_ERROR_MESSAGE_BYTE_OFFSET_EXCEED_SOURCE_BUFFER_BYTE_LENGTH,
};
use crate::runtime::js_generic_typed_array_view::{JSGenericTypedArrayView, JSGenericTypedArrayViewRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_regexp_support::get_object_property;
use crate::runtime::structure::StructureRef;
use crate::runtime::typed_array_type::TypedArrayType;
use crate::runtime::uint8_array_base64;
use crate::runtime::vm::VM;

/// Os `ClassInfo` de `JSInt8ArrayConstructor`... `JSBigUint64ArrayConstructor` (`MAKE_S_INFO` de
/// `JSTypedArrayConstructors.cpp`, `"Function"`), na ordem de `TypedArrayType::to_index`. Cada um tem
/// endereço próprio, que é como se distingue o construtor de cada tipo.
pub static TYPED_ARRAY_CONSTRUCTOR_S_INFOS: [ClassInfo; 12] = {
    macro_rules! infos {
        (@one $index:tt) => {
            ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None }
        };
        ($($index:tt)*) => {
            [$(infos!(@one $index)),*]
        };
    }
    infos!(0 1 2 3 4 5 6 7 8 9 10 11)
};

/// `constructorClassInfoForType(type)` (`TypedArrayType.cpp`).
pub fn constructor_class_info_for_type(type_: TypedArrayType) -> &'static ClassInfo {
    &TYPED_ARRAY_CONSTRUCTOR_S_INFOS[type_.to_index()]
}

/// `callInt8Array` e as irmãs: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope,
/// ViewClass::info()->className)`.
fn call_typed_array<const INDEX: usize>(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function(TypedArrayType::from_index(INDEX).class_name())
}

/// https://tc39.es/ecma262/#sec-initializetypedarrayfromlist, com a lista já coletada.
fn construct_from_list(
    global_object: &JSGlobalObject,
    structure: &StructureRef,
    storage: &[JSValue],
) -> Result<JSGenericTypedArrayViewRef, Thrown> {
    let result = JSGenericTypedArrayView::create_uninitialized(global_object, structure, storage.len())?;

    for (index, value) in storage.iter().enumerate() {
        result.set_index(global_object, index as u64, *value)?;
    }

    Ok(result)
}

/// `constructGenericTypedArrayViewFromIterator`.
fn construct_from_iterator(
    global_object: &JSGlobalObject,
    structure: &StructureRef,
    iterable: JSValue,
    iterator_method: JSValue,
) -> Result<JSGenericTypedArrayViewRef, Thrown> {
    let mut storage: Vec<JSValue> = Vec::new();
    for_each_in_iterable_with_method(global_object, iterable, iterator_method, |value| {
        storage.try_reserve(1).map_err(|_| Thrown::OutOfMemory)?;
        storage.push(value);
        Ok(())
    })?;

    construct_from_list(global_object, structure, &storage)
}

/// `constructGenericTypedArrayViewWithArrayBuffer<ViewClass>`.
fn construct_with_array_buffer(
    global_object: &JSGlobalObject,
    type_: TypedArrayType,
    structure: &StructureRef,
    buffer: ArrayBufferRef,
    offset: usize,
    length_opt: Option<usize>,
) -> Result<JSGenericTypedArrayViewRef, Thrown> {
    if buffer.is_detached() {
        return Err(Thrown::type_error(TYPED_ARRAY_ERROR_MESSAGE_BUFFER_IS_ALREADY_DETACHED));
    }

    let mut length = length_opt;
    if length.is_none() {
        let byte_length = buffer.byte_length();
        if buffer.is_resizable_or_growable_shared() {
            if offset > byte_length {
                return Err(Thrown::range_error(TYPED_ARRAY_ERROR_MESSAGE_BYTE_OFFSET_EXCEED_SOURCE_BUFFER_BYTE_LENGTH));
            }
        } else {
            let element_size = type_.element_size();
            // `size_t` sem sinal: o `offset` acima do comprimento dá um valor enorme, e a conferência de faixa
            // de `create` o recusa.
            let remaining = byte_length.wrapping_sub(offset);
            if remaining % element_size != 0 {
                return Err(Thrown::range_error("ArrayBuffer length minus the byteOffset is not a multiple of the element size"));
            }
            length = Some(remaining / element_size);
        }
    }

    JSGenericTypedArrayView::create_with_buffer(global_object, structure, buffer, offset, length)
}

/// `constructGenericTypedArrayViewWithArguments<ViewClass>(globalObject, structure, firstValue, offset,
/// lengthOpt)`.
fn construct_with_arguments(
    global_object: &JSGlobalObject,
    type_: TypedArrayType,
    structure: &StructureRef,
    first_value: JSValue,
    offset: usize,
    length_opt: Option<usize>,
) -> Result<JSGenericTypedArrayViewRef, Thrown> {
    let vm = global_object.vm();

    // https://tc39.es/ecma262/#sec-initializetypedarrayfromarraybuffer
    if let Some(js_buffer) = JSArrayBuffer::from_value(&first_value) {
        return construct_with_array_buffer(global_object, type_, structure, std::rc::Rc::clone(js_buffer.impl_()), offset, length_opt);
    }

    debug_assert!(offset == 0);

    // For everything but DataView, we allow construction with any of:
    // - Another array. This creates a copy of the of that array.
    // - A primitive. This creates a new typed array of that length and zero-initializes it.

    if first_value.is_object() {
        // https://tc39.es/proposal-resizablearraybuffer/#sec-initializetypedarrayfromtypedarray
        if let Some(view) = JSGenericTypedArrayView::from_value(&first_value) {
            let length = view.length();

            let result = JSGenericTypedArrayView::create_uninitialized(global_object, structure, length)?;

            if view.is_array_buffer_view_out_of_bounds() {
                return Err(Thrown::type_error(
                    crate::runtime::js_array_buffer_view::TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE,
                ));
            }

            if view.typed_array_type().content_type() != type_.content_type() {
                return Err(Thrown::type_error("Content types of source and new typed array are different"));
            }

            result.set_from_typed_array(0, &view, 0, length, CopyType::Unobservable)?;
            return Ok(result);
        }

        // "This getPropertySlot operation should not be observed by the Proxy. So we use VMInquiry": o `length` é
        // procurado sem trap antes do `@@iterator`; um `Proxy` (objeto opaco) na cadeia o deixa ausente, e então o
        // comprimento é 0. O getter achado só roda depois, na leitura.
        let length_name = PropertyName::from_identifier(&vm.property_names.length);
        let has_length = {
            let object = ObjectRef::from_value(&first_value).expect("o argumento objeto é objeto");
            let mut length_slot = PropertySlot::new(first_value, InternalMethodType::VMInquiry);
            object.get_property_slot(global_object, &length_name, &mut length_slot)
        };

        let iterator_function =
            crate::runtime::iterator_operations::get_value_property(
                global_object,
                first_value,
                &PropertyName::from_identifier(&vm.property_names.iterator_symbol),
            )?;

        if !iterator_function.is_undefined_or_null() {
            return construct_from_iterator(global_object, structure, first_value, iterator_function);
        }

        let length = if has_length {
            get_object_property(global_object, first_value, &vm.property_names.length)?.to_length_checked()? as usize
        } else {
            0
        };

        let result = JSGenericTypedArrayView::create_uninitialized(global_object, structure, length)?;

        result.set_from_array_like_object(global_object, 0, first_value, 0, length)?;
        return Ok(result);
    }

    debug_assert!(offset == 0 && length_opt.is_some());
    debug_assert!(!first_value.is_object());

    JSGenericTypedArrayView::create(global_object, structure, length_opt.unwrap_or(0))
}

/// `constructGenericTypedArrayViewImpl<ViewClass>(globalObject, callFrame)`
/// (https://tc39.es/ecma262/#sec-typedarray).
fn construct_generic_typed_array_view(global_object: &JSGlobalObject, call: &HostCall, type_: TypedArrayType) -> HostResult {
    let new_target = call.new_target();
    let callee = call.callee();
    let derived_structure = |resizable_or_growable_shared: bool| {
        get_derived_structure_in_realm(global_object, new_target, callee, |realm| {
            realm.array_buffer_realm.typed_arrays.structure(type_, resizable_or_growable_shared)
        })
    };

    let arg_count = call.argument_count();

    if arg_count == 0 {
        let structure = derived_structure(false)?;

        return Ok(JSGenericTypedArrayView::create(global_object, &structure, 0)?.as_value());
    }

    let first_value = call.argument(0);
    let mut offset = 0usize;
    let mut length: Option<usize> = None;
    let structure;
    if let Some(array_buffer) = JSArrayBuffer::from_value(&first_value) {
        structure = derived_structure(array_buffer.impl_().is_resizable_or_growable_shared())?;

        if arg_count > 1 {
            offset = call.argument(1).to_index("byteOffset")? as usize;

            if offset % type_.element_size() != 0 {
                return Err(Thrown::range_error("byteOffset modulo TypedArray.BYTES_PER_ELEMENT must be 0"));
            }
        }

        if arg_count > 2 {
            // If the length value is present but undefined, treat it as missing.
            let length_value = call.argument(2);
            if !length_value.is_undefined() {
                length = Some(length_value.to_index("length")? as usize);
            }
        }
    } else {
        if !first_value.is_object() {
            // the step 9 of https://tc39.es/ecma262/2026/#sec-typedarray
            // O V8 diz `Invalid typed array length: -1` (medido no Chromium 154).
            if crate::runtime::stack_frame::v8_stack_format() && first_value.is_number() && first_value.as_number().trunc() < 0.0 {
                return Err(Thrown::range_error(&format!(
                    "Invalid typed array length: {}",
                    crate::runtime::v8_stack::number_text(first_value.as_number())
                )));
            }
            length = Some(first_value.to_index("length")? as usize);
        }

        structure = derived_structure(false)?;
    }

    Ok(construct_with_arguments(global_object, type_, &structure, first_value, offset, length)?.as_value())
}

/// `constructInt8Array` e as irmãs.
fn construct_typed_array<const INDEX: usize>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_generic_typed_array_view(global_object, call, TypedArrayType::from_index(INDEX))
}

/// Define `callXxxArray` e `constructXxxArray` de cada tipo (`index` é o `TypedArrayType::to_index`).
macro_rules! typed_array_host_functions {
    ($(($index:tt, $call:ident, $construct:ident)),* $(,)?) => {
        $(
            host_function!($call, call_typed_array::<$index>);
            host_function!($construct, construct_typed_array::<$index>);
        )*

        /// `callConstructor()` e `constructConstructor()` por tipo, na ordem de `TypedArrayType::to_index`.
        const NATIVE_FUNCTIONS: [(NativeFunction, NativeFunction); 12] = [$(($call, $construct)),*];
    };
}

typed_array_host_functions!(
    (0, call_int8_array, construct_int8_array),
    (1, call_uint8_array, construct_uint8_array),
    (2, call_uint8_clamped_array, construct_uint8_clamped_array),
    (3, call_int16_array, construct_int16_array),
    (4, call_uint16_array, construct_uint16_array),
    (5, call_int32_array, construct_int32_array),
    (6, call_uint32_array, construct_uint32_array),
    (7, call_float16_array, construct_float16_array),
    (8, call_float32_array, construct_float32_array),
    (9, call_float64_array, construct_float64_array),
    (10, call_big_int64_array, construct_big_int64_array),
    (11, call_big_uint64_array, construct_big_uint64_array),
);

/// `class JSGenericTypedArrayViewConstructor<ViewClass> final : public InternalFunction`: sem campos próprios.
/// No bun é um `JSFunction` sobre `NativeExecutable`.
pub struct TypedArrayConstructor;

impl TypedArrayConstructor {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`: o `prototype` é o `%TypedArray%`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, type_: TypedArrayType, prototype: JSValue) -> StructureRef {
        native_constructor_structure(vm, global_object, prototype, constructor_class_info_for_type(type_))
    }

    /// `create(vm, globalObject, structure, prototype, name)` e `finishCreation`: `length` 3, o nome da
    /// classe, `prototype` e `BYTES_PER_ELEMENT`.
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        prototype: &JSObject,
        type_: TypedArrayType,
    ) -> JSFunctionRef {
        let (call_function, construct_function) = NATIVE_FUNCTIONS[type_.to_index()];
        let constructor = create_native_collection_constructor(
            vm,
            global_object,
            structure,
            prototype,
            type_.class_name(),
            3,
            call_function,
            construct_function,
            false,
        );
        constructor.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.bytes_per_element),
            js_number(type_.element_size() as f64),
            DONT_ENUM | READ_ONLY | DONT_DELETE,
        );
        if type_ == TypedArrayType::Uint8 {
            uint8_array_base64::install_constructor_functions(vm, global_object, &constructor);
        }
        constructor
    }
}
