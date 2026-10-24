//! Porte de `runtime/JSGenericTypedArrayViewPrototypeFunctions.h` (e dos `JSTypedArrayViewPrototypeFunctions
//! {1..4}.cpp`, que só despacham por tipo), primeira parte: `set`, `copyWithin`, `includes`, `indexOf`,
//! `lastIndexOf`, `join`, `fill`, `reverse`, `toReversed`, `sort`, `toSorted`, `with` e os acessores
//! `buffer`, `length`, `byteLength` e `byteOffset`. A segunda parte (os que chamam callback, `slice` e
//! `subarray`) está em `typed_array_prototype_natives_part2.rs`.
//!
//! DIVERGÊNCIAS:
//!
//! - O elemento nativo (`ViewClass::ElementType`) é o `NativeElement` de `typed_array_adaptors.rs`; as buscas
//!   por SIMD (`WTF::find8`... `findDouble`) viram uma busca linear por `elements_equal`, que compara os
//!   bits dos inteiros e o valor dos pontos flutuantes (`-0 == +0`, `NaN != NaN`), o que as rotinas SIMD
//!   fazem.
//! - `memmove` do `copyWithin` e o `memset`/`std::fill` do `fill` são cópias elemento a elemento ou
//!   `copy_within` sobre os bytes da visão; o resultado é o mesmo.
//! - `JSStringJoiner` é um laço sobre unidades UTF-16 (`join`), com o separador entre todos os elementos,
//!   inclusive os vazios.
//! - A ordenação com comparador é `array_stable_sort_simple` (`stable_sort.rs`), a mesma `arrayStableSort<
//!   MergeStrategy::Simple>`.

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_array_buffer::to_js_array_buffer;
use crate::runtime::js_array_buffer_view::{CopyType, TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE};
use crate::runtime::js_generic_typed_array_view::{check_typed_array_in_bounds, JSGenericTypedArrayView, SortResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::math_common::max_safe_integer;
use crate::runtime::stable_sort::{array_stable_sort_simple, coerce_comparator_result_to_boolean};
use crate::runtime::string_prototype::{code_units, string_from_units};
use crate::runtime::string_regexp_support::to_string_value;
use crate::runtime::typed_array_adaptors::{
    float_value, to_js_value, to_native_from_undefined, to_native_from_value, to_native_from_value_without_coercion,
    NativeElement,
};
use crate::runtime::typed_array_prototype_support::{
    argument_clamped_index_from_start_or_end, this_view, validated_this, Callback, View,
};
use crate::runtime::typed_array_type::TypedArrayType;

/// `typedArrayBufferHasBeenDetachedErrorMessage` como `Thrown`.
fn detached_error() -> Thrown {
    Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE)
}

/// `globalObject->typedArrayStructure(ViewClass::TypedArrayStorageType, isResizableOrGrowableShared)` com o
/// segundo argumento falso: a estrutura de uma visão nova de comprimento fixo.
fn new_view_structure(global_object: &JSGlobalObject, type_: TypedArrayType) -> crate::runtime::structure::StructureRef {
    global_object.array_buffer_realm.typed_arrays.structure(type_, false)
}

/// Os dois elementos são iguais para as buscas (`find8`..`findDouble`).
fn elements_equal(type_: TypedArrayType, left: NativeElement, right: NativeElement) -> bool {
    if type_.is_float() {
        return float_value(type_, left) == float_value(type_, right);
    }
    left == right
}

/// `typedArrayIndexOfImpl(array, length, target, index)`: o primeiro índice em `[from, search_length)`.
fn index_of_element(view: &JSGenericTypedArrayView, target: NativeElement, from: usize, search_length: usize) -> Option<usize> {
    let type_ = view.typed_array_type();
    (from..search_length).find(|&index| elements_equal(type_, view.get_element(index), target))
}

/// `typedArrayLastIndexOfImpl(array, searchLength, target)`: o último índice em `[0, search_length)`.
fn last_index_of_element(view: &JSGenericTypedArrayView, target: NativeElement, search_length: usize) -> Option<usize> {
    let type_ = view.typed_array_type();
    (0..search_length).rev().find(|&index| elements_equal(type_, view.get_element(index), target))
}

// 22.2.3.22
fn typed_array_view_proto_func_set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = this_view(call)?;

    if call.argument_count() == 0 {
        return Err(Thrown::type_error("Expected at least one argument"));
    }

    let offset = if call.argument_count() >= 2 {
        let offset_number = call.argument(1).to_integer_or_infinity_checked()?;
        if offset_number < 0.0 {
            return Err(Thrown::range_error("Offset should not be negative"));
        }
        if offset_number <= max_safe_integer() && offset_number <= usize::MAX as f64 {
            offset_number as usize
        } else {
            usize::MAX
        }
    } else {
        0
    };

    check_typed_array_in_bounds(&this_object)?;

    let source = call.argument(0);

    if source.is_object() {
        if let Some(source_view) = JSGenericTypedArrayView::from_value(&source) {
            let Some(length) = source_view.integer_indexed_object_length() else {
                return Err(detached_error());
            };
            this_object.set_from_typed_array(offset, &source_view, 0, length, CopyType::Unobservable)?;
            return Ok(JSValue::undefined());
        }
    }

    this_object.set_from_array_like(global_object, offset, source)?;
    Ok(JSValue::undefined())
}

fn typed_array_view_proto_func_copy_within_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 22.2.3.5
    let this_object = validated_this(call)?;

    let mut length = this_object.length();
    let to = argument_clamped_index_from_start_or_end(global_object, call.argument(0), length, 0)?;
    let from = argument_clamped_index_from_start_or_end(global_object, call.argument(1), length, 0)?;
    let final_index = argument_clamped_index_from_start_or_end(global_object, call.argument(2), length, length)?;

    if final_index < from {
        return Ok(call.this_value());
    }

    debug_assert!(to <= length);
    debug_assert!(from <= length);
    let mut count = (length - to.max(from)).min(final_index - from);

    if count > 0 {
        let Some(updated_length) = this_object.integer_indexed_object_length() else {
            return Err(detached_error());
        };

        // ResizableArrayBuffer can shrink the length. Thus, we need to check again to see whether we can copy things.
        // https://tc39.es/proposal-resizablearraybuffer/#sec-%typedarray%.prototype.copywithin
        if updated_length != length {
            length = updated_length;
            if to.max(from) + count > length {
                // Either to or from index is larger than the updated length. In this case, we do not need to copy anything and finish copyWithin.
                if to.max(from) > length {
                    return Ok(call.this_value());
                }
                count = length - to.max(from);
            }
        }

        let element_size = this_object.typed_array_type().element_size();
        this_object.with_vector_mut(|bytes| bytes.copy_within(from * element_size..(from + count) * element_size, to * element_size));
    }

    Ok(call.this_value())
}

fn typed_array_view_proto_func_includes_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = validated_this(call)?;

    let length = this_object.length();

    if length == 0 {
        return Ok(js_boolean(false));
    }

    let value_to_find = call.argument(0);

    let index = argument_clamped_index_from_start_or_end(global_object, call.argument(1), length, 0)?;

    let Some(updated_length) = this_object.integer_indexed_object_length() else {
        return Ok(js_boolean(index < length && value_to_find.is_undefined()));
    };

    let type_ = this_object.typed_array_type();
    let Some(target) = to_native_from_value_without_coercion(type_, value_to_find) else {
        // Even though our TypedArray's length is updated, we iterate up to `length`.
        // So, if `updatedLength` is smaller than `length`, we will see undefined after that.
        return Ok(js_boolean(index < length && updated_length < length && value_to_find.is_undefined()));
    };

    debug_assert!(!this_object.is_detached());

    let search_length = length.min(updated_length);
    if type_.is_float() && float_value(type_, target).is_nan() {
        let found = (index..search_length).any(|i| float_value(type_, this_object.get_element(i)).is_nan());
        return Ok(js_boolean(found));
    }

    Ok(js_boolean(index_of_element(&this_object, target, index, search_length).is_some()))
}

fn typed_array_view_proto_func_index_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 22.2.3.13
    let this_object = validated_this(call)?;

    let length = this_object.length();

    if length == 0 {
        return Ok(js_number(-1));
    }

    let value_to_find = call.argument(0);
    let index = argument_clamped_index_from_start_or_end(global_object, call.argument(1), length, 0)?;

    let Some(updated_length) = this_object.integer_indexed_object_length() else {
        // indexOf only sees elements when HasProperty passed. Thus, even though length gets smaller, the trailing undefineds are not checked.
        return Ok(js_number(-1));
    };

    let Some(target) = to_native_from_value_without_coercion(this_object.typed_array_type(), value_to_find) else {
        return Ok(js_number(-1));
    };
    debug_assert!(!this_object.is_detached());

    let search_length = length.min(updated_length);
    match index_of_element(&this_object, target, index, search_length) {
        Some(result) => Ok(js_number(result as f64)),
        None => Ok(js_number(-1)),
    }
}

fn typed_array_view_proto_func_last_index_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 22.2.3.16
    let this_object = validated_this(call)?;

    let mut length = this_object.length();

    if length == 0 {
        return Ok(js_number(-1));
    }

    let value_to_find = call.argument(0);

    let mut index = length - 1;
    if call.argument_count() >= 2 {
        let mut from_double = call.argument(1).to_integer_or_infinity_checked()?;
        if from_double < 0.0 {
            from_double += length as f64;
            if from_double < 0.0 {
                return Ok(js_number(-1));
            }
        }
        if from_double < length as f64 {
            index = from_double as usize;
        }
    }

    {
        let Some(updated_length) = this_object.integer_indexed_object_length() else {
            return Ok(js_number(-1));
        };

        length = updated_length;
        if length == 0 {
            return Ok(js_number(-1));
        }
        index = index.min(length - 1);
    }

    let Some(target) = to_native_from_value_without_coercion(this_object.typed_array_type(), value_to_find) else {
        return Ok(js_number(-1));
    };
    debug_assert!(!this_object.is_detached());

    match last_index_of_element(&this_object, target, index + 1) {
        Some(result) => Ok(js_number(result as f64)),
        None => Ok(js_number(-1)),
    }
}

fn typed_array_view_proto_func_join_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = validated_this(call)?;
    let vm = global_object.vm();

    let length = this_object.length();

    let separator_value = call.argument(0);
    let separator: Vec<u16> = if separator_value.is_undefined() {
        vec![u16::from(b',')]
    } else {
        let separator_string = to_string_value(global_object, separator_value)?;
        code_units(&separator_string.value()).into_owned()
    };

    // Se o buffer foi destacado ou encolheu durante a conversão do separador, os elementos que sobraram
    // entram como texto vazio (`appendEmptyString`).
    let accessible_length = match this_object.integer_indexed_object_length() {
        Some(updated_length) => length.min(updated_length),
        None => 0,
    };

    let mut result: Vec<u16> = Vec::new();
    for index in 0..length {
        if index > 0 {
            result.extend_from_slice(&separator);
        }
        if index < accessible_length {
            let value = this_object.get_index_quickly(index)?;
            let element = to_string_value(global_object, value)?;
            result.extend_from_slice(&code_units(&element.value()));
        }
    }

    Ok(JSValue::from_js_string(js_string(vm, &string_from_units(&result))))
}

fn typed_array_view_proto_func_fill_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.fill
    let this_object = validated_this(call)?;

    let length = this_object.length();
    let native_value = to_native_from_value(global_object, this_object.typed_array_type(), call.argument(0))?;

    let start = argument_clamped_index_from_start_or_end(global_object, call.argument(1), length, 0)?;
    debug_assert!(start <= length);

    let mut end = argument_clamped_index_from_start_or_end(global_object, call.argument(2), length, length)?;
    debug_assert!(end <= length);

    // ResizableArrayBuffer can shrink the length. Thus, we need to check again to see whether we can copy things.
    // https://tc39.es/proposal-resizablearraybuffer/#sec-%typedarray%.prototype.fill
    let Some(updated_length) = this_object.integer_indexed_object_length() else {
        return Err(detached_error());
    };

    end = end.min(updated_length);

    for index in start..end {
        this_object.set_element(index, native_value);
    }

    Ok(call.this_value())
}

fn typed_array_view_proto_getter_func_buffer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 22.2.3.3
    let this_object = this_view(call)?;

    Ok(to_js_array_buffer(global_object, &this_object.possibly_shared_buffer()).as_value())
}

fn typed_array_view_proto_getter_func_length_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 22.2.3.17
    Ok(js_number(this_view(call)?.length() as f64))
}

fn typed_array_view_proto_getter_func_byte_length_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 22.2.3.2
    Ok(js_number(this_view(call)?.byte_length() as f64))
}

fn typed_array_view_proto_getter_func_byte_offset_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 22.2.3.3
    Ok(js_number(this_view(call)?.byte_offset() as f64))
}

fn typed_array_view_proto_func_reverse_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 22.2.3.21
    let this_object = validated_this(call)?;

    let length = this_object.length();
    for index in 0..length / 2 {
        let front = this_object.get_element(index);
        let back = this_object.get_element(length - 1 - index);
        this_object.set_element(index, back);
        this_object.set_element(length - 1 - index, front);
    }

    Ok(call.this_value())
}

fn typed_array_view_proto_func_to_reversed_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/proposal-change-array-by-copy/#sec-%typedarray%.prototype.toReversed
    let this_object = validated_this(call)?;

    // Snapshot the span at this time, as SABs may grow (but never shrink) in parallel.
    let original: Vec<NativeElement> = (0..this_object.length()).map(|index| this_object.get_element(index)).collect();

    let structure = new_view_structure(global_object, this_object.typed_array_type());
    let result = JSGenericTypedArrayView::create_uninitialized(global_object, &structure, original.len())?;

    for (index, element) in original.into_iter().rev().enumerate() {
        result.set_element(index, element);
    }

    Ok(result.as_value())
}

/// `genericTypedArrayViewProtoFuncSortImpl(vm, globalObject, thisObject, comparatorValue)`.
fn sort_impl(global_object: &JSGlobalObject, this_object: &View, comparator_value: JSValue) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.sort
    if comparator_value.is_undefined() {
        return match this_object.sort() {
            SortResult::Success => Ok(this_object.as_value()),
            SortResult::Failed => Err(detached_error()),
            SortResult::OutOfMemory => Err(Thrown::OutOfMemory),
        };
    }

    let comparator = Callback::require(comparator_value, "TypedArray.prototype.sort requires the comparator argument to be a function or undefined")?;

    let length = this_object.length();

    if length < 2 {
        return Ok(this_object.as_value());
    }

    let type_ = this_object.typed_array_type();
    let mut source: Vec<NativeElement> = (0..length).map(|index| this_object.get_element(index)).collect();
    let mut working_set = source.clone();

    array_stable_sort_simple(&mut source, &mut working_set, |left, right| {
        let left_value = to_js_value(type_, left)?;
        let right_value = to_js_value(type_, right)?;
        let js_result = comparator.invoke(global_object, JSValue::undefined(), &[left_value, right_value])?;
        coerce_comparator_result_to_boolean(global_object, js_result)
    })?;

    if this_object.is_detached() {
        return Ok(this_object.as_value());
    }

    // The comparator may trigger FastTypedArray -> WastefulTypedArray transition via .buffer access,
    // which relocates the backing store. Do not reuse the original span here.
    let copy_length = this_object.length().min(source.len());
    for (index, element) in source.iter().take(copy_length).enumerate() {
        this_object.set_element(index, *element);
    }

    Ok(this_object.as_value())
}

fn typed_array_view_proto_func_sort_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // O `switch` sobre o tipo do `this` (`typedArrayViewProtoFuncSort`) vem antes do comparador; o limite, depois.
    let this_object = this_view(call)?;
    let comparator_value = call.argument(0);
    if !comparator_value.is_undefined() && !comparator_value.is_callable() {
        return Err(Thrown::type_error("TypedArray.prototype.sort requires the comparator argument to be a function or undefined"));
    }

    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.sort
    check_typed_array_in_bounds(&this_object)?;

    sort_impl(global_object, &this_object, comparator_value)
}

fn typed_array_view_proto_func_to_sorted_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/proposal-change-array-by-copy/#sec-%typedarray%.prototype.toSorted
    let this_object = this_view(call)?;
    let comparator_value = call.argument(0);
    if !comparator_value.is_undefined() && !comparator_value.is_callable() {
        return Err(Thrown::type_error("TypedArray.prototype.toSorted requires the comparator argument to be a function or undefined"));
    }

    check_typed_array_in_bounds(&this_object)?;

    // Snapshot the span at this time, as SABs may grow (but never shrink) in parallel.
    let original: Vec<NativeElement> = (0..this_object.length()).map(|index| this_object.get_element(index)).collect();

    let structure = new_view_structure(global_object, this_object.typed_array_type());
    let result = JSGenericTypedArrayView::create_uninitialized(global_object, &structure, original.len())?;

    for (index, element) in original.into_iter().enumerate() {
        result.set_element(index, element);
    }

    sort_impl(global_object, &result, comparator_value)
}

/// `validateIntegerIndex(globalObject, thisObject, index)`
/// (https://tc39.es/proposal-resizablearraybuffer/#sec-isvalidintegerindex).
fn validate_integer_index(this_object: &JSGenericTypedArrayView, index: f64) -> Result<(), Thrown> {
    if !index.is_finite() || index.trunc() != index {
        return Err(Thrown::range_error("index should be integer"));
    }
    if index == 0.0 && index.is_sign_negative() {
        return Err(Thrown::range_error("index should not be negative zero"));
    }

    match this_object.integer_indexed_object_length() {
        Some(length) if index >= 0.0 && index < length as f64 => Ok(()),
        _ => Err(Thrown::range_error("index is out of range")),
    }
}

fn typed_array_view_proto_func_with_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/proposal-change-array-by-copy/#sec-%typedarray%.prototype.with
    let this_object = this_view(call)?;
    let Some(this_length) = this_object.integer_indexed_object_length() else {
        return Err(detached_error());
    };

    let relative_index = call.argument(0).to_integer_or_infinity_checked()?;
    let actual_index = if relative_index >= 0.0 { relative_index } else { this_length as f64 + relative_index };

    let type_ = this_object.typed_array_type();
    let native_value = to_native_from_value(global_object, type_, call.argument(1))?;

    validate_integer_index(&this_object, actual_index)?;
    debug_assert!(!this_object.is_detached());
    let replace_index = actual_index as usize;

    let structure = new_view_structure(global_object, type_);
    let result = JSGenericTypedArrayView::create_uninitialized(global_object, &structure, this_length)?;

    // Snapshot the span at this time, as SABs may grow (but never shrink) in parallel.
    let updated_length = this_object.length();
    if this_length != updated_length {
        // If TypedArray is shrunk, remaining part will be filled with NativeValue(undefined).
        // But BigInt64Array / BigUint64Array throws a TypeError since undefined cannot be converted to BigInt.
        if type_.is_big_int() && this_length > updated_length {
            return Err(Thrown::type_error("Cannot convert undefined to BigInt"));
        }

        for index in 0..this_length {
            let mut from_value = to_native_from_undefined(type_);
            if index == replace_index {
                from_value = native_value;
            } else if this_object.in_bounds(index as u64) {
                from_value = this_object.get_element(index);
            }
            result.set_element(index, from_value);
        }
    } else {
        for index in 0..this_length {
            result.set_element(index, if index == replace_index { native_value } else { this_object.get_element(index) });
        }
    }

    Ok(result.as_value())
}

host_function!(pub typed_array_view_proto_func_set, typed_array_view_proto_func_set_body);
host_function!(pub typed_array_view_proto_func_copy_within, typed_array_view_proto_func_copy_within_body);
host_function!(pub typed_array_view_proto_func_includes, typed_array_view_proto_func_includes_body);
host_function!(pub typed_array_view_proto_func_index_of, typed_array_view_proto_func_index_of_body);
host_function!(pub typed_array_view_proto_func_last_index_of, typed_array_view_proto_func_last_index_of_body);
host_function!(pub typed_array_view_proto_func_join, typed_array_view_proto_func_join_body);
host_function!(pub typed_array_view_proto_func_fill, typed_array_view_proto_func_fill_body);
host_function!(pub typed_array_view_proto_getter_func_buffer, typed_array_view_proto_getter_func_buffer_body);
host_function!(pub typed_array_view_proto_getter_func_length, typed_array_view_proto_getter_func_length_body);
host_function!(pub typed_array_view_proto_getter_func_byte_length, typed_array_view_proto_getter_func_byte_length_body);
host_function!(pub typed_array_view_proto_getter_func_byte_offset, typed_array_view_proto_getter_func_byte_offset_body);
host_function!(pub typed_array_view_proto_func_reverse, typed_array_view_proto_func_reverse_body);
host_function!(pub typed_array_view_proto_func_to_reversed, typed_array_view_proto_func_to_reversed_body);
host_function!(pub typed_array_view_proto_func_sort, typed_array_view_proto_func_sort_body);
host_function!(pub typed_array_view_proto_func_to_sorted, typed_array_view_proto_func_to_sorted_body);
host_function!(pub typed_array_view_proto_func_with, typed_array_view_proto_func_with_body);
