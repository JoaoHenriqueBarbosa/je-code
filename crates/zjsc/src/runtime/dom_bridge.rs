//! Ponte entre o JavaScript do documento e a árvore DOM do hospedeiro (o navegador do sandbox).
//!
//! O DOM mora no hospedeiro (a crate `ul-chromium`, sobre o html5ever); aqui só existe uma função nativa de passagem,
//! `__pl_dom(op, ...args)`, que converte os argumentos para [`DomArg`], chama [`DomHost::call`] e converte o resultado
//! de volta. Os objetos `Node`, `Element`, `Document` e o resto são escritos em JavaScript por cima dessa ponte, com
//! identidade por número de nó. O prelúdio captura a função e a apaga do global, então o programa da página não a vê.

use std::cell::RefCell;
use std::rc::Rc;

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_array::JSArray;
use crate::runtime::js_dom_exception::throw_dom_exception_from_host;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::native_class_support::install_global_function_with_visibility;
use crate::runtime::process_shape::{array_value, text_value};

/// Um valor que cruza a ponte: o que o JavaScript e o hospedeiro trocam.
#[derive(Clone, Debug, PartialEq)]
pub enum DomArg {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<DomArg>),
}

/// Uma `DOMException` que o hospedeiro pede para lançar no JavaScript (`name` e `message`).
#[derive(Clone, Debug, PartialEq)]
pub struct DomError {
    pub name: String,
    pub message: String,
}

/// A árvore DOM do hospedeiro, operada por nome de operação.
pub trait DomHost {
    fn call(&self, op: &str, args: &[DomArg]) -> Result<DomArg, DomError>;
}

impl std::fmt::Debug for dyn DomHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DomHost")
    }
}

/// Instala o hospedeiro e a função global `__pl_dom`.
pub fn install(global_object: &JSGlobalObject, host: Rc<dyn DomHost>) {
    *global_object.dom_host.borrow_mut() = Some(host);
    install_global_function_with_visibility(global_object, "__pl_dom", 1, pl_dom, ImplementationVisibility::Private);
}

fn to_arg(global_object: &JSGlobalObject, value: JSValue) -> DomArg {
    let vm = global_object.vm();
    if value.is_undefined() {
        DomArg::Undefined
    } else if value.is_null() {
        DomArg::Null
    } else if let JSValue::Bool(flag) = value {
        DomArg::Bool(flag)
    } else if value.is_number() {
        DomArg::Num(value.as_number())
    } else if let (Some(array), Some(object)) = (JSArray::from_value(&value), JSObject::from_value(&value)) {
        DomArg::List((0..array.length()).map(|index| to_arg(global_object, object.get_by_index(vm, index))).collect())
    } else {
        DomArg::Str(rust_string(&value.to_wtf_string()))
    }
}

fn to_value(global_object: &JSGlobalObject, arg: &DomArg) -> JSValue {
    match arg {
        DomArg::Undefined => JSValue::undefined(),
        DomArg::Null => JSValue::null(),
        DomArg::Bool(flag) => JSValue::Bool(*flag),
        DomArg::Num(number) => js_number(*number),
        DomArg::Str(text) => text_value(global_object.vm(), text),
        DomArg::List(items) => {
            let values: Vec<JSValue> = items.iter().map(|item| to_value(global_object, item)).collect();
            array_value(global_object, &values)
        }
    }
}

fn pl_dom_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let host = global_object.dom_host.borrow().clone();
    let Some(host) = host else { return Ok(JSValue::undefined()) };
    let op = rust_string(&call.argument(0).to_wtf_string());
    let args: Vec<DomArg> = (1..call.argument_count()).map(|index| to_arg(global_object, call.argument(index))).collect();
    match host.call(&op, &args) {
        Ok(result) => Ok(to_value(global_object, &result)),
        Err(error) => Err(throw_dom_exception_from_host(global_object, call, &error.name, &error.message)),
    }
}
host_function!(pl_dom, pl_dom_body);
