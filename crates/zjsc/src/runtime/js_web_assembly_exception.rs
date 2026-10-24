//! `WebAssembly.Exception` (`JSWebAssemblyException`, `WebAssemblyExceptionConstructor`,
//! `WebAssemblyExceptionPrototype`): uma exceção wasm, com a `WebAssembly.Tag` e o payload convertido.
//!
//! LACUNA: `{ traceStack: true }` não captura pilha; `stack` devolve sempre `undefined` (o bun devolve uma
//! string só com `traceStack`). Referências não nulas de `funcref` ainda não existem no porte.

use crate::host_function;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_support::{construct_instance, prop, with_instance, IntlClass, IntlInstance};
use crate::runtime::iterator_operations::{for_each_in_iterable, get_value_property};
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_boolean, js_undefined, JSValue};
use crate::runtime::js_web_assembly::{check_pending, externref_type, put_enumerable_method, to_js_value, to_wasm_value, wrapper_structure};
use crate::runtime::js_web_assembly_tag::{tag_from_value, TagData};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::ACCESSOR;
use crate::wasm::wasm_format::{funcref_type, Type};
use std::cell::RefCell;
use std::rc::Rc;

/// `JSWebAssemblyException`: a tag e o payload, já como valores JS.
struct ExceptionState {
    tag: Rc<RefCell<TagData>>,
    payload: Vec<JSValue>,
}

/// `JSWebAssemblyException::visitChildren`: o payload.
pub(crate) fn visit_state(state: &dyn std::any::Any, marker: &mut crate::runtime::gc::Marker) {
    if let Some(state) = state.downcast_ref::<ExceptionState>() {
        marker.values(&state.payload);
    }
}

const NOT_EXCEPTION_MESSAGE: &str = "WebAssembly.Exception operation called on non-Exception object";

fn call_exception_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("calling WebAssembly.Exception constructor without new is invalid"))
}

/// O valor de payload que o parâmetro de tipo `ty` guarda (`toWebAssemblyValue` e de volta).
fn payload_value(global_object: &JSGlobalObject, value: JSValue, ty: Type) -> Result<JSValue, Thrown> {
    if ty == externref_type() {
        return Ok(value);
    }
    if ty == funcref_type() && !value.is_null() {
        return Err(Thrown::type_error("Argument value did not match the reference type"));
    }
    to_js_value(to_wasm_value(global_object, value, ty)?, ty)
}

/// `constructJSWebAssemblyException`.
fn construct_exception_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(tag) = tag_from_value(call.argument(0)) else {
        return Err(Thrown::type_error("WebAssembly.Exception constructor expects the first argument to be a WebAssembly.Tag"));
    };
    if crate::runtime::js_web_assembly_tag::is_js_tag(&tag) {
        return Err(Thrown::type_error("WebAssembly.Exception constructor does not accept WebAssembly.JSTag"));
    }
    let mut values = Vec::new();
    for_each_in_iterable(global_object, call.argument(1), |value| {
        values.push(value);
        Ok(())
    })?;
    if values.len() != tag.borrow().parameters.len() {
        return Err(Thrown::type_error("WebAssembly.Exception constructor expects the number of paremeters in WebAssembly.Tag to match the tags parameter count."));
    }
    let options = call.argument(2);
    if !(options.is_undefined() || options.is_null()) {
        if !options.is_object() {
            return Err(Thrown::type_error("WebAssembly.Exception expects its third argument to be an object"));
        }
        // `traceStack` é lido (um getter pode lançar), mas a pilha não é capturada.
        get_value_property(global_object, options, &prop(global_object.vm(), "traceStack"))?;
    }
    let parameters = tag.borrow().parameters.clone();
    let mut payload = Vec::with_capacity(values.len());
    for (value, ty) in values.into_iter().zip(parameters) {
        payload.push(payload_value(global_object, value, ty)?);
    }
    check_pending(global_object)?;
    construct_instance(global_object, call, |_| Ok(Box::new(ExceptionState { tag, payload })))
}

/// O índice de `getArg`: inteiro em `[0, 2^32 - 1]`.
fn arg_index(global_object: &JSGlobalObject, value: JSValue) -> Result<usize, Thrown> {
    let number = value.to_number();
    check_pending(global_object)?;
    if number.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&number) {
        return Err(Thrown::range_error("Expect an integer argument in the range: [0, 2^32 - 1]"));
    }
    Ok(number as usize)
}

/// `WebAssembly.Exception.prototype.is(tag)`.
fn exception_is_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<ExceptionState, _>(call.this_value(), NOT_EXCEPTION_MESSAGE, |state, _| {
        let Some(tag) = tag_from_value(call.argument(0)) else {
            return Err(Thrown::type_error("WebAssembly.Exception.is(): First argument must be a WebAssembly.Tag"));
        };
        Ok(js_boolean(Rc::ptr_eq(&state.tag, &tag)))
    })
}

/// `WebAssembly.Exception.prototype.getArg(tag, index)`.
fn exception_get_arg_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<ExceptionState, _>(call.this_value(), NOT_EXCEPTION_MESSAGE, |state, _| {
        if call.argument_count() < 2 {
            return Err(Thrown::type_error("Not enough arguments"));
        }
        let Some(tag) = tag_from_value(call.argument(0)) else {
            return Err(Thrown::type_error("WebAssembly.Exception.getArg(): First argument must be a WebAssembly.Tag"));
        };
        if !Rc::ptr_eq(&state.tag, &tag) {
            return Err(Thrown::type_error("WebAssembly.Exception.getArg(): First argument does not match the exception tag"));
        }
        let index = arg_index(global_object, call.argument(1))?;
        state.payload.get(index).copied().ok_or_else(|| Thrown::range_error("WebAssembly.Exception.getArg(): Index out of range"))
    })
}

/// O getter `stack` (sem `traceStack` é `undefined`).
fn exception_stack_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<ExceptionState, _>(call.this_value(), NOT_EXCEPTION_MESSAGE, |_, _| Ok(js_undefined()))
}

/// `JSWebAssemblyException::create` para uma exceção wasm que chega ao JS: o payload em bits vira valores
/// JS pelos tipos dos parâmetros da tag.
pub(crate) fn create_exception(global_object: &JSGlobalObject, tag: &Rc<RefCell<TagData>>, payload: &[u64]) -> Result<JSValue, Thrown> {
    let parameters = tag.borrow().parameters.clone();
    let values = payload.iter().zip(parameters).map(|(bits, ty)| to_js_value(*bits, ty)).collect::<Result<Vec<_>, _>>()?;
    let structure = wrapper_structure("Exception");
    Ok(IntlInstance::create(global_object.vm(), &structure, Box::new(ExceptionState { tag: Rc::clone(tag), payload: values })).as_value())
}

/// A tag e o payload de um `WebAssembly.Exception`, ou `None` se o valor não for um.
pub(crate) fn exception_parts(value: JSValue) -> Option<(Rc<RefCell<TagData>>, Vec<JSValue>)> {
    IntlInstance::from_value(&value)
        .and_then(|cell| cell.state::<ExceptionState>().map(|state| (Rc::clone(&state.tag), state.payload.clone())))
}

host_function!(call_web_assembly_exception, call_exception_body);
host_function!(construct_web_assembly_exception, construct_exception_body);
host_function!(web_assembly_exception_is, exception_is_body);
host_function!(web_assembly_exception_get_arg, exception_get_arg_body);
host_function!(web_assembly_exception_stack, exception_stack_body);

/// Instala `WebAssembly.Exception` (`length` 2; protótipo `getArg`, `is`, `stack`, `constructor`, com os
/// métodos e o acessor enumeráveis).
pub(crate) fn install_exception_class(global_object: &JSGlobalObject, namespace: &JSObject) {
    let vm = global_object.vm();
    let class = IntlClass {
        name: "Exception",
        length: 2,
        has_supported_locales_of: false,
        call: call_web_assembly_exception,
        construct: construct_web_assembly_exception,
    };
    let prototype = class.install_with(global_object, namespace, |prototype| {
        put_enumerable_method(global_object, prototype, "getArg", 2, web_assembly_exception_get_arg as NativeFunction);
        put_enumerable_method(global_object, prototype, "is", 1, web_assembly_exception_is);
        let getter = crate::runtime::js_custom_accessor_function::create_host_custom_accessor_getter_function(
            vm,
            global_object,
            "stack",
            web_assembly_exception_stack,
        );
        let accessor = GetterSetter::create_from_values(vm, getter.as_value(), JSValue::undefined());
        prototype.put_direct_non_index_accessor_without_transition(vm, &prop(vm, "stack"), &accessor, ACCESSOR);
    });
    put_to_string_tag(vm, &prototype, "WebAssembly.Exception");
    crate::runtime::js_web_assembly::register_wrapper_structure(global_object, "Exception", &prototype);
}
