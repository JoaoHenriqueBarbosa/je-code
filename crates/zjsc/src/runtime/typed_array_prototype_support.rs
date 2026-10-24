//! O que as funções de `%TypedArray%.prototype` (`JSGenericTypedArrayViewPrototypeFunctions.h`) usam em
//! comum: a leitura do `this` (o despacho `CALL_GENERIC_TYPEDARRAY_PROTOTYPE_FUNCTION` de
//! `JSTypedArrayViewPrototypeInternal.h`), a conversão de índice relativo, o laço
//! `typedArrayViewForEachImpl`, o `callback` que o laço chama e o `speciesConstruct`
//! (https://tc39.es/ecma262/#typedarray-species-create).
//!
//! DIVERGÊNCIAS:
//!
//! - `JSGenericTypedArrayView<Adaptor>` é um tipo só (ver `js_generic_typed_array_view.rs`): o despacho por
//!   `switch` sobre o `JSType` do C++ vira a leitura do `TypedArrayType` na própria visão.
//! - `speciesWatchpointIsValid` está portado (`species_watchpoint_is_valid`) e é o caminho rápido do
//!   `species_construct`: não muda o observável, só pula a leitura de `constructor` e de `@@species`.
//! - `CachedCall` e a chamada direta (`callData.type == JS` ou nativa) são uma só chamada: `call`.

use std::ops::ControlFlow;

use crate::bytecode::watchpoint::WatchpointState;
use crate::runtime::call_data::{call, construct_with_error_message, get_call_data, CallData};
use crate::runtime::host_call::{HostCall, Thrown};
use crate::runtime::iterator_operations::{get_value_property, thrown_from_llint};
use crate::runtime::js_generic_typed_array_view::{
    check_typed_array_in_bounds, JSGenericTypedArrayView, JSGenericTypedArrayViewRef,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_name::PropertyName;
use crate::runtime::typed_array_adaptors::{to_js_value, to_native_from_undefined, NativeElement};

/// A visão que uma função de `%TypedArray%.prototype` opera.
pub type View = JSGenericTypedArrayViewRef;

/// `typedArrayViewProtoFunc*` (`JSTypedArrayViewPrototypeFunctions{1..4}.cpp`): o `this` objeto e da classe
/// de uma das 12 visões (o `JSDataView` e o resto caem no `default` do `switch`).
pub fn this_view(call: &HostCall) -> Result<View, Thrown> {
    let this_value = call.this_value();
    if !this_value.is_object() {
        return Err(Thrown::type_error("Receiver should be a typed array view but was not an object"));
    }
    JSGenericTypedArrayView::from_value(&this_value).ok_or_else(|| Thrown::type_error("Receiver should be a typed array view"))
}

/// O `this` mais o `validateTypedArray(globalObject, thisObject)`.
pub fn validated_this(call: &HostCall) -> Result<View, Thrown> {
    let view = this_view(call)?;
    check_typed_array_in_bounds(&view)?;
    Ok(view)
}

/// `argumentClampedIndexFromStartOrEnd(globalObject, value, length, undefinedValue)`.
pub fn argument_clamped_index_from_start_or_end(
    global_object: &JSGlobalObject,
    value: JSValue,
    length: usize,
    undefined_value: usize,
) -> Result<usize, Thrown> {
    if value.is_undefined() {
        return Ok(undefined_value);
    }

    if let JSValue::Int32(integer) = value {
        let mut index = i64::from(integer);
        if index < 0 {
            index += length as i64;
            return Ok(if index < 0 { 0 } else { index as usize });
        }
        return Ok(if index as usize > length { length } else { index as usize });
    }

    let mut index = value.to_integer_or_infinity_checked()?;
    if index < 0.0 {
        index += length as f64;
        return Ok(if index < 0.0 { 0 } else { index as usize });
    }
    Ok(if index > length as f64 { length } else { index as usize })
}

/// O callback já conferido como chamável (o `getCallDataInline(functorValue)` e a mensagem de
/// `callData.type == CallData::Type::None`).
pub struct Callback {
    function: JSValue,
    data: CallData,
}

impl Callback {
    /// `getCallDataInline(value)`, com `message` se não é chamável.
    pub fn require(function: JSValue, message: &str) -> Result<Callback, Thrown> {
        let data = get_call_data(function);
        if data.is_none() {
            return Err(Thrown::type_error(message));
        }
        Ok(Callback { function, data })
    }

    /// `call(globalObject, functorValue, callData, thisValue, args)`.
    pub fn invoke(&self, global_object: &JSGlobalObject, this_value: JSValue, args: &[JSValue]) -> Result<JSValue, Thrown> {
        call(global_object, self.function, &self.data, this_value, args).map_err(thrown_from_llint)
    }
}

/// `ForEachDirection`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ForEachDirection {
    Forward,
    Backward,
}

/// `typedArrayViewForEachImpl<direction>(globalObject, vm, thisObject, length, functor)`: o `functor` recebe
/// o elemento como `JSValue` (`undefined` se a visão foi destacada ou o índice saiu da faixa no meio do
/// laço), o índice e o elemento nativo (`toNativeFromUndefined` no mesmo caso). `ControlFlow::Break` é o
/// `IterationStatus::Done`.
pub fn for_each_element(
    view: &JSGenericTypedArrayView,
    length: usize,
    direction: ForEachDirection,
    mut functor: impl FnMut(JSValue, usize, NativeElement) -> Result<ControlFlow<()>, Thrown>,
) -> Result<(), Thrown> {
    let type_ = view.typed_array_type();
    // Uma visão de comprimento fixo sobre buffer não redimensionável não encolhe nem sai da faixa: só
    // precisa conferir se foi destacada. As redimensionáveis conferem a faixa a cada elemento.
    let mut visit = |index: usize| -> Result<ControlFlow<()>, Thrown> {
        let mut element = JSValue::undefined();
        let mut native_value = to_native_from_undefined(type_);
        if !view.is_detached() && (!view.is_resizable_non_shared() || view.in_bounds(index as u64)) {
            native_value = view.get_element(index);
            element = to_js_value(type_, native_value)?;
        }
        functor(element, index, native_value)
    };

    match direction {
        ForEachDirection::Forward => {
            for index in 0..length {
                if visit(index)?.is_break() {
                    return Ok(());
                }
            }
        }
        ForEachDirection::Backward => {
            for index in (0..length).rev() {
                if visit(index)?.is_break() {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

/// `speciesWatchpointIsValid(globalObject, thisObject)` (JSGenericTypedArrayViewPrototypeFunctions.h:86):
/// instala o watchpoint de espécie do tipo na primeira vez (set ainda `ClearWatchpoint`) e diz se a visão
/// segue o caminho padrão: sem propriedades próprias (`hasCustomProperties`, a estrutura já transitou), com o
/// protótipo do tipo, e com os dois sets (o do tipo e o do `%TypedArray%`) em `IsWatched`.
pub fn species_watchpoint_is_valid(global_object: &JSGlobalObject, this_object: &JSGenericTypedArrayView) -> bool {
    let realm = &global_object.array_buffer_realm.typed_arrays;
    let type_ = this_object.typed_array_type();

    if realm.species_watchpoint_state(type_) == WatchpointState::ClearWatchpoint {
        realm.try_install_species_watchpoint(global_object.vm(), global_object, type_);
        debug_assert!(realm.species_watchpoint_state(type_) != WatchpointState::ClearWatchpoint);
    }

    !this_object.structure().did_transition()
        && realm.prototype(type_).as_value() == this_object.get_prototype_direct()
        && realm.species_watchpoint_state(type_) == WatchpointState::IsWatched
        && realm.constructor_species_watchpoint_set().borrow().state() == WatchpointState::IsWatched
}

/// `speciesConstruct(globalObject, exemplar, defaultConstructor, constructArgs, length)`
/// (https://tc39.es/ecma262/#typedarray-species-create): `default_constructor` cria a visão do mesmo tipo
/// do `exemplar`, `construct_args` monta os argumentos do construtor da espécie e `length` é o comprimento
/// que o resultado precisa ter (só no caso de um único argumento numérico).
pub fn species_construct(
    global_object: &JSGlobalObject,
    exemplar: &JSGenericTypedArrayView,
    default_constructor: impl FnOnce() -> Result<View, Thrown>,
    construct_args: impl FnOnce() -> Result<Vec<JSValue>, Thrown>,
    length: Option<usize>,
) -> Result<View, Thrown> {
    let vm = global_object.vm();
    let names = &vm.property_names;

    let in_same_realm = exemplar.structure().realm().is_some_and(|realm| std::ptr::eq(&*realm, global_object));
    if in_same_realm && species_watchpoint_is_valid(global_object, exemplar) {
        return default_constructor();
    }

    let constructor_value =
        get_value_property(global_object, exemplar.as_value(), &PropertyName::from_identifier(&names.constructor))?;

    if constructor_value.is_undefined() {
        return default_constructor();
    }

    if !constructor_value.is_object() {
        return Err(Thrown::type_error("constructor Property should not be null"));
    }

    let view_class_constructor = global_object.array_buffer_realm.typed_arrays.constructor(exemplar.typed_array_type()).as_value();

    let species = get_value_property(global_object, constructor_value, &PropertyName::from_identifier(&names.species_symbol))?;

    if species.is_undefined_or_null() {
        return default_constructor();
    }

    // If species constructor ends up the same to viewClassConstructor, let's use default fast path.
    if species == view_class_constructor {
        return default_constructor();
    }

    let args = construct_args()?;

    let result = construct_with_error_message(global_object, species, &args, "species is not a constructor")
        .map_err(thrown_from_llint)?;

    let Some(view) = JSGenericTypedArrayView::from_value(&result) else {
        return Err(Thrown::type_error("species constructor did not return a TypedArray View"));
    };

    check_typed_array_in_bounds(&view)?;

    // https://tc39.es/ecma262/#typedarray-create
    // 3. If argumentList is a List of a single Number, then
    // a. If newTypedArray.[[ArrayLength]] < R(argumentList[0]), throw a TypeError exception.
    if let Some(length) = length {
        if view.length() < length {
            return Err(Thrown::type_error("TypedArray.prototype.slice constructed typed array of insufficient length"));
        }
    }

    // https://tc39.es/ecma262/#typedarray-species-create
    // If result.[[ContentType]] ≠ exemplar.[[ContentType]], throw a TypeError exception.
    if view.typed_array_type().content_type() != exemplar.typed_array_type().content_type() {
        return Err(Thrown::type_error("Content types of source and created typed arrays are different"));
    }

    Ok(view)
}
