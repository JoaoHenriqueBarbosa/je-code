//! Porte de `runtime/JSGenericTypedArrayViewPrototypeFunctions.h`, segunda parte: as funções que chamam um
//! callback (`forEach`, `map`, `filter`, `find`, `findIndex`, `findLast`, `findLastIndex`, `every`, `some`,
//! `reduce`, `reduceRight`) e as que criam outra visão pela espécie (`slice`, `subarray`). A primeira parte
//! está em `typed_array_prototype_natives.rs`.
//!
//! DIVERGÊNCIAS:
//!
//! - As seis funções de busca (`find`, `findIndex`, `findLast`, `findLastIndex`, `every`, `some`) são um
//!   laço só, `search`, que o C++ repete seis vezes por ser `template`: o que as separa é o sentido, o
//!   valor do callback que interrompe o laço, o que sai ao interromper e o que sai se o laço termina.
//! - `reduce` e `reduceRight` são um laço só, `reduce`, pelo mesmo motivo (o sentido e a mensagem).
//! - `CachedCall` e a chamada comum são o mesmo `Callback::invoke` (ver `typed_array_prototype_support.rs`).
//! - `filter` converte os elementos guardados para o tipo da visão da espécie com `convert_to` (o
//!   `copyElements`/`convertTo` do C++); o mesmo tipo copia os bits.

use std::ops::ControlFlow;

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_array_buffer::to_js_array_buffer;
use crate::runtime::js_array_buffer_view::{CopyType, TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE};
use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::typed_array_adaptors::{convert_to, NativeElement};
use crate::runtime::typed_array_prototype_support::{
    argument_clamped_index_from_start_or_end, for_each_element, species_construct, this_view, validated_this, Callback,
    ForEachDirection,
};

fn detached_error() -> Thrown {
    Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE)
}

/// `[element, jsNumber(index), thisObject]`: os argumentos do callback.
fn callback_arguments(element: JSValue, index: usize, this_value: JSValue) -> [JSValue; 3] {
    [element, js_number(index as f64), this_value]
}

fn typed_array_view_proto_func_for_each_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.foreach
    let this_object = validated_this(call)?;

    let length = this_object.length();

    let callback = Callback::require(call.argument(0), "TypedArray.prototype.forEach callback must be a function")?;

    let this_arg = call.argument(1);
    let this_value = this_object.as_value();

    for_each_element(&this_object, length, ForEachDirection::Forward, |element, index, _| {
        callback.invoke(global_object, this_arg, &callback_arguments(element, index, this_value))?;
        Ok(ControlFlow::Continue(()))
    })?;
    Ok(JSValue::undefined())
}

fn typed_array_view_proto_func_map_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.map
    let this_object = validated_this(call)?;

    let length = this_object.length();

    let callback = Callback::require(call.argument(0), "TypedArray.prototype.map callback must be a function")?;

    let this_arg = call.argument(1);
    let this_value = this_object.as_value();
    let type_ = this_object.typed_array_type();

    let result = species_construct(
        global_object,
        &this_object,
        || {
            let structure = global_object.array_buffer_realm.typed_arrays.structure(type_, false);
            JSGenericTypedArrayView::create_uninitialized(global_object, &structure, length)
        },
        || Ok(vec![js_number(length as f64)]),
        Some(length),
    )?;

    for_each_element(&this_object, length, ForEachDirection::Forward, |element, index, _| {
        let mapped = callback.invoke(global_object, this_arg, &callback_arguments(element, index, this_value))?;
        result.set_index(global_object, index as u64, mapped)?;
        Ok(ControlFlow::Continue(()))
    })?;
    Ok(result.as_value())
}

fn typed_array_view_proto_func_filter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.filter
    let this_object = validated_this(call)?;

    let length = this_object.length();

    let callback = Callback::require(call.argument(0), "TypedArray.prototype.filter callback must be a function")?;

    let this_arg = call.argument(1);
    let this_value = this_object.as_value();
    let type_ = this_object.typed_array_type();
    let mut kept: Vec<NativeElement> = Vec::new();
    kept.try_reserve(length).map_err(|_| Thrown::OutOfMemory)?;

    for_each_element(&this_object, length, ForEachDirection::Forward, |element, index, native_value| {
        let result = callback.invoke(global_object, this_arg, &callback_arguments(element, index, this_value))?;
        if result.to_boolean() {
            kept.push(native_value);
        }
        Ok(ControlFlow::Continue(()))
    })?;
    let length = kept.len();

    let result = species_construct(
        global_object,
        &this_object,
        || {
            let structure = global_object.array_buffer_realm.typed_arrays.structure(type_, false);
            JSGenericTypedArrayView::create_uninitialized(global_object, &structure, length)
        },
        || Ok(vec![js_number(length as f64)]),
        Some(length),
    )?;

    let result_type = result.typed_array_type();
    // `if constexpr (contentType(name) == ViewClass::contentType)`: a espécie já foi conferida.
    if result_type.content_type() == type_.content_type() {
        for (index, element) in kept.into_iter().enumerate() {
            let converted = if result_type == type_ { element } else { convert_to(type_, result_type, element) };
            result.set_element(index, converted);
        }
    }

    Ok(result.as_value())
}

/// O que separa `find`, `findIndex`, `findLast`, `findLastIndex`, `every` e `some`.
struct SearchSpec {
    /// `TypedArray.prototype.find callback must be a function` e as irmãs.
    message: &'static str,
    direction: ForEachDirection,
    /// O `toBoolean` do resultado do callback que interrompe o laço.
    stop_when: bool,
    /// O valor que sai ao interromper, dado o elemento e o índice.
    found: fn(JSValue, usize) -> JSValue,
    /// O valor que sai se o laço termina sem interromper.
    not_found: fn() -> JSValue,
}

fn search(global_object: &JSGlobalObject, call: &HostCall, spec: SearchSpec) -> HostResult {
    let this_object = validated_this(call)?;

    let length = this_object.length();

    let callback = Callback::require(call.argument(0), spec.message)?;

    let this_arg = call.argument(1);
    let this_value = this_object.as_value();

    let mut found = (spec.not_found)();
    for_each_element(&this_object, length, spec.direction, |element, index, _| {
        let result = callback.invoke(global_object, this_arg, &callback_arguments(element, index, this_value))?;
        if result.to_boolean() == spec.stop_when {
            found = (spec.found)(element, index);
            return Ok(ControlFlow::Break(()));
        }
        Ok(ControlFlow::Continue(()))
    })?;
    Ok(found)
}

fn typed_array_view_proto_func_find_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.find
    search(
        global_object,
        call,
        SearchSpec {
            message: "TypedArray.prototype.find callback must be a function",
            direction: ForEachDirection::Forward,
            stop_when: true,
            found: |element, _| element,
            not_found: JSValue::undefined,
        },
    )
}

fn typed_array_view_proto_func_find_index_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.findindex
    search(
        global_object,
        call,
        SearchSpec {
            message: "TypedArray.prototype.findIndex callback must be a function",
            direction: ForEachDirection::Forward,
            stop_when: true,
            found: |_, index| js_number(index as f64),
            not_found: || js_number(-1),
        },
    )
}

fn typed_array_view_proto_func_find_last_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.findlast
    search(
        global_object,
        call,
        SearchSpec {
            message: "TypedArray.prototype.findLast callback must be a function",
            direction: ForEachDirection::Backward,
            stop_when: true,
            found: |element, _| element,
            not_found: JSValue::undefined,
        },
    )
}

fn typed_array_view_proto_func_find_last_index_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.findlastindex
    search(
        global_object,
        call,
        SearchSpec {
            message: "TypedArray.prototype.findLastIndex callback must be a function",
            direction: ForEachDirection::Backward,
            stop_when: true,
            found: |_, index| js_number(index as f64),
            not_found: || js_number(-1),
        },
    )
}

fn typed_array_view_proto_func_every_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.every
    search(
        global_object,
        call,
        SearchSpec {
            message: "TypedArray.prototype.every callback must be a function",
            direction: ForEachDirection::Forward,
            stop_when: false,
            found: |_, _| js_boolean(false),
            not_found: || js_boolean(true),
        },
    )
}

fn typed_array_view_proto_func_some_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.some
    search(
        global_object,
        call,
        SearchSpec {
            message: "TypedArray.prototype.some callback must be a function",
            direction: ForEachDirection::Forward,
            stop_when: true,
            found: |_, _| js_boolean(true),
            not_found: || js_boolean(false),
        },
    )
}

/// `reduce` e `reduceRight`: `name` é o que entra nas mensagens.
fn reduce(global_object: &JSGlobalObject, call: &HostCall, name: &str, direction: ForEachDirection) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.reduce
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.reduceright
    let this_object = validated_this(call)?;

    let length = this_object.length();

    let callback = Callback::require(call.argument(0), &format!("TypedArray.prototype.{name} callback must be a function"))?;

    let has_initial_value = call.argument_count() > 1;

    if !has_initial_value && length == 0 {
        return Err(Thrown::type_error(&format!("TypedArray.prototype.{name} of empty array with no initial value")));
    }

    let mut accumulator = if has_initial_value { call.argument(1) } else { JSValue::undefined() };
    let mut initialized = has_initial_value;
    let this_value = this_object.as_value();

    for_each_element(&this_object, length, direction, |element, index, _| {
        if !initialized {
            accumulator = element;
            initialized = true;
            return Ok(ControlFlow::Continue(()));
        }

        accumulator = callback.invoke(
            global_object,
            JSValue::undefined(),
            &[accumulator, element, js_number(index as f64), this_value],
        )?;
        Ok(ControlFlow::Continue(()))
    })?;

    Ok(accumulator)
}

fn typed_array_view_proto_func_reduce_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    reduce(global_object, call, "reduce", ForEachDirection::Forward)
}

fn typed_array_view_proto_func_reduce_right_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    reduce(global_object, call, "reduceRight", ForEachDirection::Backward)
}

fn typed_array_view_proto_func_slice_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 22.2.3.26
    let this_object = validated_this(call)?;

    let Some(this_length) = this_object.integer_indexed_object_length() else {
        return Err(detached_error());
    };

    let begin = argument_clamped_index_from_start_or_end(global_object, call.argument(0), this_length, 0)?;
    let mut end = argument_clamped_index_from_start_or_end(global_object, call.argument(1), this_length, this_length)?;

    // Clamp end to begin.
    end = end.max(begin);

    debug_assert!(end >= begin);
    let mut length = end - begin;

    let type_ = this_object.typed_array_type();
    let result = species_construct(
        global_object,
        &this_object,
        || {
            let structure = global_object.array_buffer_realm.typed_arrays.structure(type_, false);

            // If the source TypedArray is resizable, length can be changed.
            // In that case, it is possible that we will have some holes which is not initialized to the zero values.
            // We use initialized TypedArray if source TypedArray is resizable.
            // Note that regardless of the source TypedArray's resizability, resulted TypedArray should be unresizable.
            if this_object.is_resizable_or_growable_shared() {
                return JSGenericTypedArrayView::create(global_object, &structure, length);
            }

            JSGenericTypedArrayView::create_uninitialized(global_object, &structure, length)
        },
        || Ok(vec![js_number(length as f64)]),
        Some(length),
    )?;

    // We return early here since we don't allocate a backing store if length is 0.
    if length == 0 {
        return Ok(result.as_value());
    }

    {
        let Some(updated_length) = this_object.integer_indexed_object_length() else {
            return Err(detached_error());
        };
        end = end.min(updated_length);
    }

    // It is possible that |begin| becomes larger than |end| at this point. In this case, we do nothing.
    if begin >= end {
        return Ok(result.as_value());
    }

    debug_assert!(end > begin);
    // This length is always smaller than the previous length.
    length = end - begin;
    debug_assert!(result.length() >= length);

    result.set_from_typed_array(0, &this_object, begin, length, CopyType::LeftToRight)?;
    Ok(result.as_value())
}

fn typed_array_view_proto_func_subarray_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // https://tc39.es/ecma262/#sec-%typedarray%.prototype.subarray
    let this_object = this_view(call)?;

    let this_length = this_object.length();
    let src_byte_offset = this_object.byte_offset_raw();

    let mut start = call.argument(0);
    if !start.is_int32() {
        start = js_number(start.to_integer_or_infinity_checked()?);
    }

    let mut finish = call.argument(1);
    if !finish.is_undefined() && !finish.is_int32() {
        finish = js_number(finish.to_integer_or_infinity_checked()?);
    }

    let begin = argument_clamped_index_from_start_or_end(global_object, start, this_length, 0)?;

    let mut count: Option<usize> = None;
    if !(this_object.is_auto_length() && finish.is_undefined()) {
        let mut end = argument_clamped_index_from_start_or_end(global_object, finish, this_length, this_length)?;

        // Clamp end to begin.
        end = end.max(begin);

        debug_assert!(end >= begin);
        count = Some(end - begin);
    }

    let array_buffer = this_object.possibly_shared_buffer();

    let type_ = this_object.typed_array_type();
    let new_byte_offset = src_byte_offset + begin * type_.element_size();

    let result = species_construct(
        global_object,
        &this_object,
        || {
            let structure = global_object
                .array_buffer_realm
                .typed_arrays
                .structure(type_, array_buffer.is_resizable_or_growable_shared());
            JSGenericTypedArrayView::create_with_buffer(
                global_object,
                &structure,
                std::rc::Rc::clone(&array_buffer),
                new_byte_offset,
                count,
            )
        },
        || {
            let mut args = vec![
                to_js_array_buffer(global_object, &array_buffer).as_value(),
                js_number(new_byte_offset as f64),
            ];
            if let Some(count) = count {
                args.push(js_number(count as f64));
            }
            Ok(args)
        },
        None,
    )?;
    Ok(result.as_value())
}

host_function!(pub typed_array_view_proto_func_for_each, typed_array_view_proto_func_for_each_body);
host_function!(pub typed_array_view_proto_func_map, typed_array_view_proto_func_map_body);
host_function!(pub typed_array_view_proto_func_filter, typed_array_view_proto_func_filter_body);
host_function!(pub typed_array_view_proto_func_find, typed_array_view_proto_func_find_body);
host_function!(pub typed_array_view_proto_func_find_index, typed_array_view_proto_func_find_index_body);
host_function!(pub typed_array_view_proto_func_find_last, typed_array_view_proto_func_find_last_body);
host_function!(pub typed_array_view_proto_func_find_last_index, typed_array_view_proto_func_find_last_index_body);
host_function!(pub typed_array_view_proto_func_every, typed_array_view_proto_func_every_body);
host_function!(pub typed_array_view_proto_func_some, typed_array_view_proto_func_some_body);
host_function!(pub typed_array_view_proto_func_reduce, typed_array_view_proto_func_reduce_body);
host_function!(pub typed_array_view_proto_func_reduce_right, typed_array_view_proto_func_reduce_right_body);
host_function!(pub typed_array_view_proto_func_slice, typed_array_view_proto_func_slice_body);
host_function!(pub typed_array_view_proto_func_subarray, typed_array_view_proto_func_subarray_body);
