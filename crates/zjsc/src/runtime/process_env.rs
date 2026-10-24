//! `process.env` do bun 1.4.2: um objeto comum (protótipo `Object.prototype`, não é `Proxy`) cujos ganchos de escrita
//! seguem a regra 24 de `wip/notes/process-plan.md`.
//!
//! - escrita: o valor vira texto (`env.U = undefined` guarda 'undefined'); a chave '' é ignorada; uma chave símbolo
//!   lança `Cannot convert a symbol to a string`; um valor símbolo lança o mesmo erro do `String(symbol)`;
//! - `defineProperty` só aceita o descritor de dados com `configurable`, `writable` e `enumerable` verdadeiros
//!   (`ERR_INVALID_OBJECT_DEFINE_PROPERTY`), o que faz `Object.freeze` e `Object.seal` lançarem;
//! - `Reflect.ownKeys` traz, depois das chaves reais, nove nomes que o bun reserva e que não são enumeráveis
//!   (`getOwnPropertyDescriptor` deles é `undefined`, `Object.keys` não os lista).
//!
//! O objeto é um `JSFinalObject` comum registrado pelo id de célula; os ganchos em `js_object.rs` e
//! `own_property_names.rs` consultam [`is_env`]. A herança do ambiente para processos filhos fica com o
//! `child_process`, fora desta fatia.

use std::cell::{Cell, RefCell};

use crate::runtime::current_realm::current_global_object;
use crate::runtime::enumeration_mode::DontEnumPropertiesMode;
use crate::runtime::host_call::Thrown;
use crate::runtime::identifier::Identifier;
use crate::runtime::intl_support::prop;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, PutError};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::error_type::ErrorType;
use crate::runtime::node_error::throw_validation_error;
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::string_regexp_support::to_wtf_string_value;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// Os nomes que o bun lista em `Reflect.ownKeys(process.env)` além do ambiente real.
const RESERVED_NAMES: [&str; 9] = [
    "BUN_CONFIG_VERBOSE_FETCH",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NODE_TLS_REJECT_UNAUTHORIZED",
    "NO_PROXY",
    "TZ",
    "http_proxy",
    "https_proxy",
    "no_proxy",
];

const ACCESSOR_MESSAGE: &str = "'process.env' does not accept an accessor(getter/setter) descriptor";
const DESCRIPTOR_MESSAGE: &str = "'process.env' only accepts a configurable, writable, and enumerable data descriptor";
const DEFINE_CODE: &str = "ERR_INVALID_OBJECT_DEFINE_PROPERTY";

thread_local! {
    static ENV_CELL: Cell<Option<usize>> = const { Cell::new(None) };
    static ENVIRONMENT: RefCell<Option<Vec<(String, String)>>> = const { RefCell::new(None) };
}

/// Fim do programa (`process_shape::reset_for_program`).
/// As raízes do coletor: o objeto `process.env`.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    if let Some(cell_id) = ENV_CELL.with(Cell::get) {
        marker.cell(cell_id);
    }
}

pub(crate) fn reset_for_program() {
    let _ = ENV_CELL.try_with(|cell| cell.set(None));
    // `ENVIRONMENT` é configuração do embedder (`set_environment`), feita antes de `run_program`: sobrevive ao reset.
}

/// O ambiente do próximo programa, na ordem dada (sem isso, o do processo).
pub fn set_environment(pairs: &[(&str, &str)]) {
    ENVIRONMENT.with(|environment| *environment.borrow_mut() = Some(pairs.iter().map(|(key, value)| ((*key).to_string(), (*value).to_string())).collect()));
}

fn current_environment() -> Vec<(String, String)> {
    ENVIRONMENT
        .with(|environment| environment.borrow().clone())
        .unwrap_or_else(|| crate::runtime::host::host().environ())
}

/// O valor da variável `name` no ambiente do programa (o mesmo que `process.env.NAME` mostra na instalação).
pub(crate) fn environment_variable(name: &str) -> Option<String> {
    current_environment().into_iter().find(|(key, _)| key == name).map(|(_, value)| value)
}

/// Cria o objeto `process.env` com o ambiente e o registra como o exótico.
pub(crate) fn create(global_object: &JSGlobalObject) -> JSValue {
    let vm = global_object.vm();
    let env = construct_empty_object(global_object);
    for (key, value) in current_environment() {
        let text = JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(value.as_bytes())));
        env.put_direct(vm, &prop(vm, &key), text, 0);
    }
    ENV_CELL.with(|cell| cell.set(Some(env.cell_id())));
    env.as_value()
}

/// `object` é o `process.env` do programa?
pub(crate) fn is_env(object: &JSObject) -> bool {
    ENV_CELL.with(|cell| cell.get()) == Some(object.cell_id())
}

/// O `process.env` do programa (o mesmo objeto que o `Bun.env` entrega), criado aqui se ainda não existir.
pub(crate) fn env_value(global_object: &JSGlobalObject) -> JSValue {
    ENV_CELL.with(|cell| cell.get()).map_or_else(|| create(global_object), JSValue::from_cell)
}

fn pending(_: Thrown) -> PutError {
    PutError::Pending
}

/// O valor como texto (`String(value)`, que lança para símbolo).
fn coerce_value(vm: &VM, value: JSValue) -> Result<JSValue, PutError> {
    let text = to_wtf_string_value(&current_global_object(), value).map_err(pending)?;
    Ok(JSValue::from_js_string(js_string(vm, &text)))
}

/// Gancho de `put` e `put_by_index`: `Ok(Some(valor))` segue a escrita comum com o valor já texto; `Ok(None)` descarta
/// a escrita (chave vazia). Chave símbolo lança.
pub(crate) fn before_put(vm: &VM, name: Option<&PropertyName>, value: JSValue) -> Result<Option<JSValue>, PutError> {
    if let Some(name) = name {
        if name.is_symbol() {
            return Err(PutError::TypeError("Cannot convert a symbol to a string"));
        }
        if name.uid().is_some_and(|uid| uid.0.is_empty()) {
            return Ok(None);
        }
    }
    coerce_value(vm, value).map(Some)
}

/// Gancho de `define_own_property`: valida o formato do descritor e devolve o descritor com o valor já texto.
pub(crate) fn before_define(vm: &VM, descriptor: &PropertyDescriptor) -> Result<PropertyDescriptor, PutError> {
    let global_object = current_global_object();
    if descriptor.is_accessor_descriptor() {
        return Err(pending(throw_validation_error(&global_object, ErrorType::TypeError, ACCESSOR_MESSAGE, DEFINE_CODE)));
    }
    let complete = descriptor.configurable_present()
        && descriptor.configurable()
        && descriptor.enumerable_present()
        && descriptor.enumerable()
        && descriptor.writable_present()
        && descriptor.writable();
    if !complete {
        return Err(pending(throw_validation_error(&global_object, ErrorType::TypeError, DESCRIPTOR_MESSAGE, DEFINE_CODE)));
    }
    let mut coerced = *descriptor;
    coerced.set_value(coerce_value(vm, descriptor.value())?);
    Ok(coerced)
}

/// `[[OwnPropertyKeys]]`: depois das chaves reais, os nomes reservados que ainda não existem (só com
/// `DontEnumPropertiesMode::Include`, porque eles não são enumeráveis).
pub(crate) fn add_reserved_names(vm: &VM, object: &JSObject, property_names: &mut PropertyNameArrayBuilder<'_>, mode: DontEnumPropertiesMode) {
    if mode != DontEnumPropertiesMode::Include || !is_env(object) {
        return;
    }
    for name in RESERVED_NAMES {
        property_names.add(&Identifier::from_span(vm, name.as_bytes()));
    }
}
