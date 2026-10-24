//! `WebAssembly.Tag` (`JSWebAssemblyTag`, `WebAssemblyTagConstructor`): o tipo de uma exceção wasm, só com a
//! lista de parâmetros. A identidade do `Tag` é a do objeto: duas tags com os mesmos parâmetros são distintas.
//!
//! As instâncias reusam a célula `IntlInstance`, como as demais classes do `WebAssembly` na ponte JS.

use crate::host_function;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_support::{wtf_to_rust, IntlClass, IntlInstance};
use crate::runtime::iterator_operations::{for_each_in_iterable, get_value_property};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::property_attribute::ACCESSOR;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_web_assembly::{check_pending, construct_wrapper, externref_type, wrapper_key};
use crate::wasm::wasm_format::{funcref_type, Type, TYPE_F32, TYPE_F64, TYPE_I32, TYPE_I64, TYPE_V128};
use crate::runtime::js_value::JSValue;
use std::cell::RefCell;
use std::rc::Rc;

/// O tipo da tag: os parâmetros do payload das exceções que a usam.
pub struct TagData {
    pub parameters: Vec<Type>,
}

/// `JSWebAssemblyTag`.
pub(crate) struct TagState {
    pub(crate) tag: Rc<RefCell<TagData>>,
}

/// A tag de um `WebAssembly.Tag`, ou `None` se o valor não for um.
pub(crate) fn tag_from_value(value: JSValue) -> Option<Rc<RefCell<TagData>>> {
    IntlInstance::from_value(&value).and_then(|cell| cell.state::<TagState>().map(|state| Rc::clone(&state.tag)))
}

const PARAMETERS_PROPERTY_MESSAGE: &str = "WebAssembly.Tag constructor expects a tag type with the 'parameters' property.";
const PARAMETERS_SEQUENCE_MESSAGE: &str =
    "WebAssembly.Tag constructor expects the 'parameters' field of the first argument to be a sequence of WebAssembly value types.";

/// `parseValueType`: os nomes de tipo de valor que o construtor aceita.
fn value_type_from_name(name: &str) -> Option<Type> {
    Some(match name {
        "i32" => TYPE_I32,
        "i64" => TYPE_I64,
        "f32" => TYPE_F32,
        "f64" => TYPE_F64,
        "v128" => TYPE_V128,
        "anyfunc" | "funcref" => funcref_type(),
        "externref" => externref_type(),
        _ => return None,
    })
}

fn call_tag_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("calling WebAssembly.Tag constructor without new is invalid"))
}

/// `constructJSWebAssemblyTag`.
fn construct_tag_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() == 0 {
        return Err(Thrown::type_error("WebAssembly.Tag constructor expects the tag type as the first argument."));
    }
    let descriptor = call.argument(0);
    if descriptor.is_undefined() || descriptor.is_null() {
        return Err(Thrown::type_error("undefined is not an object"));
    }
    if !descriptor.is_object() {
        return Err(Thrown::type_error(PARAMETERS_PROPERTY_MESSAGE));
    }
    let vm = global_object.vm();
    let parameters_value = get_value_property(global_object, descriptor, &crate::runtime::intl_support::prop(vm, "parameters"))?;
    if !parameters_value.is_object() {
        return Err(Thrown::type_error(PARAMETERS_PROPERTY_MESSAGE));
    }
    let mut parameters = Vec::new();
    for_each_in_iterable(global_object, parameters_value, |element| {
        let name = wtf_to_rust(&element.to_string(vm).value());
        check_pending(global_object)?;
        parameters.push(value_type_from_name(&name).ok_or_else(|| Thrown::type_error(PARAMETERS_SEQUENCE_MESSAGE))?);
        Ok(())
    })?;
    construct_wrapper(global_object, call, || {
        let tag = Rc::new(RefCell::new(TagData { parameters }));
        Ok((wrapper_key(&tag), Box::new(TagState { tag })))
    })
}

host_function!(call_web_assembly_tag, call_tag_body);
host_function!(construct_web_assembly_tag, construct_tag_body);

thread_local! {
    /// A tag `WebAssembly.JSTag` (`(param externref)`): uma única identidade por thread.
    static JS_TAG: Rc<RefCell<TagData>> = Rc::new(RefCell::new(TagData { parameters: vec![externref_type()] }));
}

/// A tag `WebAssembly.JSTag`, a que carrega uma exceção de JS qualquer através do wasm.
pub(crate) fn js_tag() -> Rc<RefCell<TagData>> {
    JS_TAG.with(Rc::clone)
}

/// `tag` é a `WebAssembly.JSTag`.
pub(crate) fn is_js_tag(tag: &Rc<RefCell<TagData>>) -> bool {
    JS_TAG.with(|js_tag| Rc::ptr_eq(js_tag, tag))
}

/// O getter `get JSTag` de `WebAssembly`: o mesmo objeto `Tag` em toda leitura.
fn js_tag_getter_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(crate::runtime::js_web_assembly::tag_wrapper(global_object, &js_tag()))
}

host_function!(web_assembly_js_tag_getter, js_tag_getter_body);

/// Instala o acessor `WebAssembly.JSTag` (enumerável, configurável, sem setter), como no bun 1.4.2.
pub(crate) fn install_js_tag(global_object: &JSGlobalObject, namespace: &JSObject) {
    let vm = global_object.vm();
    let getter = JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_utf8(b"get JSTag"),
        web_assembly_js_tag_getter,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    let accessor = GetterSetter::create_from_values(vm, getter.as_value(), JSValue::undefined());
    namespace.put_direct_non_index_accessor_without_transition(vm, &crate::runtime::intl_support::prop(vm, "JSTag"), &accessor, ACCESSOR);
}

/// Instala `WebAssembly.Tag` (`length` 1, protótipo só com `constructor`).
pub(crate) fn install_tag_class(global_object: &JSGlobalObject, namespace: &JSObject) {
    let class = IntlClass { name: "Tag", length: 1, has_supported_locales_of: false, call: call_web_assembly_tag, construct: construct_web_assembly_tag };
    let prototype = class.install(global_object, namespace);
    crate::runtime::js_web_assembly::register_wrapper_structure(global_object, "Tag", &prototype);
    put_to_string_tag(global_object.vm(), &prototype, "WebAssembly.Tag");
}
