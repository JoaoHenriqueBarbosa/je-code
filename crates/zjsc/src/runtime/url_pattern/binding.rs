//! A ligação JS do `URLPattern` (`JSURLPattern.cpp`, `JSURLPatternInit.cpp`, `JSURLPatternOptions.cpp` e
//! `JSURLPatternResult.cpp` do bun 1.4.2). Medido no bun (`tests/golden/url_pattern_main.js`):
//!
//! - global de dados `writable`, `enumerable` e `configurable` (ao contrário do `URL`); construtor `length` 0, chaves
//!   próprias `length`, `name`, `prototype`; o mesmo valor em `require("node:url").URLPattern`;
//! - protótipo, nesta ordem: `constructor` (dado não enumerável), os atributos nativos enumeráveis `protocol`,
//!   `username`, `password`, `hostname`, `port`, `pathname`, `search`, `hash`, `hasRegExpGroups` (sem setter), os
//!   métodos enumeráveis `test` e `exec` (`length` 0) e `@@toStringTag` "URLPattern";
//! - sem `new`: ``Use `new URLPattern(...)` instead of `URLPattern(...)` `` (`ERR_ILLEGAL_CONSTRUCTOR`); getter com
//!   `this` alheio: `The URLPattern.<nome> getter can only be used on instances of URLPattern` (sem `code`); `test` e
//!   `exec`: `Can only call URLPattern.<método> on instances of URLPattern` (`ERR_INVALID_THIS`), antes de converter os
//!   argumentos;
//! - os erros do algoritmo são `TypeError` sem `code` com o texto do C++; o dicionário de opções que não é objeto é o
//!   `throwTypeError` sem mensagem (`Type error`).
//!
//! A escolha da sobrecarga do construtor é a do gerador de bindings: com dois argumentos, o segundo `undefined`,
//! `null` ou objeto vai para `(input, options)`; senão (ou com três) para `(input, baseURL, options)`, onde a base é
//! sempre convertida em texto (`undefined` vira "undefined").

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::component::ComponentResult;
use super::{URLPattern, URLPatternInit, URLPatternInput, URLPatternResult, Units};
use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{
    create_native_class_with_length, install_global_with_attributes, instance_structure, property_key, put_native_accessor, throw_coded_type_error,
};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_constructor::{construct_array_of, construct_empty_object, put_own_data_property_may_be_index};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::proxy_object::object_get;
use crate::runtime::web_iterable::{put_enumerable_methods, string_value, usv_units_of};

thread_local! {
    /// As instâncias do programa (o valor codificado da célula); o padrão compilado não segura valor JS.
    static PATTERNS: RefCell<HashMap<EncodedJSValue, Rc<URLPattern>>> = RefCell::new(HashMap::new());
}

/// O padrão de cada instância é campo do objeto: sai com ele.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    crate::runtime::gc::side_table(&PATTERNS, marker, phase, |_, _| {})
}

pub(crate) fn reset_for_program() {
    let _ = PATTERNS.try_with(|patterns| patterns.borrow_mut().clear());
}

static PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "URLPattern", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

fn pattern_of(value: JSValue) -> Option<Rc<URLPattern>> {
    PATTERNS.with(|patterns| patterns.borrow().get(&value.encode()).cloned())
}

fn get_property(global_object: &JSGlobalObject, object: &JSObject, name: &str) -> Result<JSValue, Thrown> {
    object_get(global_object, object, &property_key(global_object.vm(), name), object.as_value())
}

/// `convertDictionary<URLPatternInit>`: as chaves lidas em ordem alfabética, `undefined` é ausente.
fn convert_init(global_object: &JSGlobalObject, value: JSValue) -> Result<URLPatternInit, Thrown> {
    let mut init = URLPatternInit::default();
    if value.is_undefined_or_null() {
        return Ok(init);
    }
    if !value.is_object() {
        return Err(Thrown::type_error("Type error"));
    }
    let object = value.as_object();
    let fields: [(&str, &mut Option<Units>); 9] = [
        ("baseURL", &mut init.base_url),
        ("hash", &mut init.hash),
        ("hostname", &mut init.hostname),
        ("password", &mut init.password),
        ("pathname", &mut init.pathname),
        ("port", &mut init.port),
        ("protocol", &mut init.protocol),
        ("search", &mut init.search),
        ("username", &mut init.username),
    ];
    for (name, slot) in fields {
        let field = get_property(global_object, &object, name)?;
        if !field.is_undefined() {
            *slot = Some(usv_units_of(global_object, field)?);
        }
    }
    Ok(init)
}

/// `convert<IDLUnion<IDLUSVString, IDLDictionary<URLPatternInit>>>`: `undefined`, `null` e objeto são o dicionário.
fn convert_input(global_object: &JSGlobalObject, value: JSValue) -> Result<URLPatternInput, Thrown> {
    if value.is_undefined_or_null() || value.is_object() {
        return Ok(URLPatternInput::Init(convert_init(global_object, value)?));
    }
    Ok(URLPatternInput::String(usv_units_of(global_object, value)?))
}

/// `convertDictionary<URLPatternOptions>`: o `ignoreCase`.
fn convert_ignore_case(global_object: &JSGlobalObject, value: JSValue) -> Result<bool, Thrown> {
    if value.is_undefined_or_null() {
        return Ok(false);
    }
    if !value.is_object() {
        return Err(Thrown::type_error("Type error"));
    }
    let ignore_case = get_property(global_object, &value.as_object(), "ignoreCase")?;
    Ok(!ignore_case.is_undefined() && ignore_case.to_boolean())
}

fn call_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new URLPattern(...)` instead of `URLPattern(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

/// `JSURLPatternDOMConstructor::construct`: a escolha da sobrecarga e a conversão dos argumentos na ordem dela.
fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let count = call.argument_count().min(3);
    let second = call.argument(1);
    let with_base = count == 3 || (count == 2 && !(second.is_undefined_or_null() || second.is_object()));
    let (input, base_url, ignore_case) = if with_base {
        let input = convert_input(global_object, call.argument(0))?;
        let base_url = usv_units_of(global_object, second)?;
        let ignore_case = convert_ignore_case(global_object, call.argument(2))?;
        (input, Some(base_url), ignore_case)
    } else {
        let input = convert_input(global_object, call.argument(0))?;
        let ignore_case = convert_ignore_case(global_object, second)?;
        (input, None, ignore_case)
    };
    let pattern = URLPattern::create(global_object.vm(), input, base_url, ignore_case).map_err(|message| Thrown::type_error(&message))?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    PATTERNS.with(|patterns| patterns.borrow_mut().insert(instance.encode(), Rc::new(pattern)));
    Ok(instance)
}

macro_rules! pattern_getter {
    ($function:ident, $body:ident, $name:literal, $read:expr) => {
        fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            let Some(pattern) = pattern_of(call.this_value()) else {
                return Err(Thrown::type_error(concat!("The URLPattern.", $name, " getter can only be used on instances of URLPattern")));
            };
            let read: fn(&JSGlobalObject, &URLPattern) -> JSValue = $read;
            Ok(read(global_object, &pattern))
        }
        host_function!($function, $body);
    };
}

pattern_getter!(protocol_get, protocol_get_body, "protocol", |global, pattern| string_value(global, pattern.pattern_string(0)));
pattern_getter!(username_get, username_get_body, "username", |global, pattern| string_value(global, pattern.pattern_string(1)));
pattern_getter!(password_get, password_get_body, "password", |global, pattern| string_value(global, pattern.pattern_string(2)));
pattern_getter!(hostname_get, hostname_get_body, "hostname", |global, pattern| string_value(global, pattern.pattern_string(3)));
pattern_getter!(port_get, port_get_body, "port", |global, pattern| string_value(global, pattern.pattern_string(4)));
pattern_getter!(pathname_get, pathname_get_body, "pathname", |global, pattern| string_value(global, pattern.pattern_string(5)));
pattern_getter!(search_get, search_get_body, "search", |global, pattern| string_value(global, pattern.pattern_string(6)));
pattern_getter!(hash_get, hash_get_body, "hash", |global, pattern| string_value(global, pattern.pattern_string(7)));
pattern_getter!(has_regexp_groups_get, has_regexp_groups_get_body, "hasRegExpGroups", |_, pattern| JSValue::Bool(pattern.has_regexp_groups()));

/// O corpo comum de `test` e `exec`: `this`, a entrada opcional e a base (`undefined` é nula).
fn run_match(global_object: &JSGlobalObject, call: &HostCall, method: &str) -> Result<Option<URLPatternResult>, Thrown> {
    let Some(pattern) = pattern_of(call.this_value()) else {
        return Err(throw_coded_type_error(global_object, &format!("Can only call URLPattern.{method} on instances of URLPattern"), "ERR_INVALID_THIS"));
    };
    let input = convert_input(global_object, call.argument(0))?;
    let base = call.argument(1);
    let base_url = if base.is_undefined() { None } else { Some(usv_units_of(global_object, base)?) };
    pattern.match_input(input, base_url).map_err(|message| Thrown::type_error(&message))
}

fn test_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSValue::Bool(run_match(global_object, call, "test")?.is_some()))
}

fn exec_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(match run_match(global_object, call, "exec")? {
        Some(result) => result_to_js(global_object, &result)?,
        None => JSValue::null(),
    })
}

fn put_value(global_object: &JSGlobalObject, object: &JSObject, name: &str, value: JSValue) {
    object.put_direct(global_object.vm(), &property_key(global_object.vm(), name), value, 0);
}

/// `convertDictionaryToJS(URLPatternInit)`: só os campos não nulos, em ordem alfabética.
fn init_to_js(global_object: &JSGlobalObject, init: &URLPatternInit) -> JSValue {
    let object = construct_empty_object(global_object);
    let fields: [(&str, &Option<Units>); 9] = [
        ("baseURL", &init.base_url),
        ("hash", &init.hash),
        ("hostname", &init.hostname),
        ("password", &init.password),
        ("pathname", &init.pathname),
        ("port", &init.port),
        ("protocol", &init.protocol),
        ("search", &init.search),
        ("username", &init.username),
    ];
    for (name, value) in fields {
        if let Some(value) = value {
            put_value(global_object, &object, name, string_value(global_object, value));
        }
    }
    object.as_value()
}

/// `convertDictionaryToJS(URLPatternComponentResult)`: `{ input, groups }`, com os nomes de índice em
/// `putDirectIndex` e o grupo que não participou como `undefined`.
fn component_to_js(global_object: &JSGlobalObject, component: &ComponentResult) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    let groups = construct_empty_object(global_object);
    for (name, value) in &component.groups {
        let value = value.as_ref().map_or_else(JSValue::undefined, |text| string_value(global_object, text));
        let key = PropertyName::from_identifier(&Identifier::from_span(vm, name.as_slice()));
        put_own_data_property_may_be_index(vm, &groups, &key, value)?;
    }
    let object = construct_empty_object(global_object);
    put_value(global_object, &object, "input", string_value(global_object, &component.input));
    put_value(global_object, &object, "groups", groups.as_value());
    Ok(object.as_value())
}

/// `convertDictionaryToJS(URLPatternResult)`.
fn result_to_js(global_object: &JSGlobalObject, result: &URLPatternResult) -> Result<JSValue, Thrown> {
    let inputs: Vec<JSValue> = result
        .inputs
        .iter()
        .map(|input| match input {
            URLPatternInput::String(text) => string_value(global_object, text),
            URLPatternInput::Init(init) => init_to_js(global_object, init),
        })
        .collect();
    let object = construct_empty_object(global_object);
    put_value(global_object, &object, "inputs", construct_array_of(global_object, &inputs).as_value());
    let names = ["protocol", "username", "password", "hostname", "port", "pathname", "search", "hash"];
    for (name, component) in names.iter().zip(&result.components) {
        let value = component_to_js(global_object, component)?;
        put_value(global_object, &object, name, value);
    }
    Ok(object.as_value())
}

host_function!(call_fn, call_body);
host_function!(construct_fn, construct_body);
host_function!(test_fn, test_body);
host_function!(exec_fn, exec_body);

/// Instala `URLPattern` no global.
pub(crate) fn install_url_pattern(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let (prototype, constructor) =
        create_native_class_with_length(global_object, &PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "URLPattern", 0, call_fn, construct_fn);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    let getters: [(&str, NativeFunction); 9] = [
        ("protocol", protocol_get),
        ("username", username_get),
        ("password", password_get),
        ("hostname", hostname_get),
        ("port", port_get),
        ("pathname", pathname_get),
        ("search", search_get),
        ("hash", hash_get),
        ("hasRegExpGroups", has_regexp_groups_get),
    ];
    for (name, getter) in getters {
        put_native_accessor(vm, global_object, &prototype, name, getter, None, 0);
    }
    put_enumerable_methods(global_object, &prototype, &[("test", 0, test_fn), ("exec", 0, exec_fn)]);
    put_to_string_tag(vm, &prototype, "URLPattern");
    install_global_with_attributes(global_object, "URLPattern", constructor.as_value(), 0);
}
