//! `CallSitePrototype`: o `CallSite.prototype` com os métodos de `CallSite` do V8 que o `Bun` expõe
//! (`getThis`, `getTypeName`, `getFunction`, `getFunctionName`, `getMethodName`, `getFileName`,
//! `getLineNumber`, `getColumnNumber`, `getEvalOrigin`, `isToplevel`, `isEval`, `isNative`,
//! `isConstructor`, `isAsync`, `isPromiseAll`, `getPromiseIndex`, `getScriptId`, `toJSON` e `toString`).
//!
//! `CallSitePrototype.cpp` não está em `upstream/JavaScriptCore` (é do `Bun`): a semântica abaixo é a do
//! V8 que o `Bun` imita, conferida em `bun` 1.4.2 só para `getFunctionName`, `getMethodName`,
//! `getTypeName`, `isToplevel`, `isConstructor` e `getLineNumber` num módulo estrito.
//!
//! DIVERGÊNCIAS:
//! - `getThis` dá `undefined` sempre, e `getTypeName` a string `"undefined"` (medido no `bun`). `getFunction`
//!   dá a função em código sloppy, `null` sem função e `undefined` em código estrito.
//! - `getMethodName` devolve o nome da função (o V8 procura a propriedade do receptor que guarda a função).
//! - `getEvalOrigin` dá `undefined`; `isNative` vale `true` nas frames nativas e de builtin em JS, `isAsync`
//!   e `isPromiseAll` dão `false` e `getPromiseIndex` dá `null`.
//! - O `CallSite` não tem construtor global.

use crate::bytecode::code_type::CodeType;
use crate::host_function;
use crate::runtime::cell_registry;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{put_native_function, put_to_string_tag};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_call_site::{JSCallSite, JSCallSiteRef};
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo CallSitePrototype::s_info`.
pub static CALL_SITE_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "CallSite", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// A string JS de um texto.
fn string_value(vm: &VM, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes())))
}

/// `callSite->frame().functionName()` como valor JS: a string, e a string vazia (nunca `null`) sem nome.
/// Medido no `bun` 1.4.2: a função anônima, o código de programa e o de `eval` devolvem `""` em `getFunctionName`
/// e em `getMethodName`.
fn function_name_value(vm: &VM, site: &JSCallSite) -> JSValue {
    string_value(vm, site.frame().call_site_function_name().unwrap_or_default())
}

/// `CallSite.isToplevel`: sem receptor, ou o receptor é o objeto global (ou o proxy dele).
fn is_top_level(site: &JSCallSite) -> bool {
    // Medido no `bun` 1.4.2: no frame estrito (ou abaixo de um, ver `call_hook`) `isToplevel` é sempre `true` (o
    // `this` fica oculto). A função nativa fora disso segue o receptor (`[].reduce` dá `false`).
    if site.frame().is_strict {
        return true;
    }
    // Medido no `bun` 1.4.2 em CommonJS sloppy: a função de host em C++ (`[native code]`: `JSON.parse`, `sort`,
    // `module._compile`) é sempre toplevel, porque o frame dela não guarda o receptor; o builtin em JS (`[unknown]`:
    // `reduce`, `forEach`) segue o receptor.
    if !site.frame().has_line_and_column_info() {
        return true;
    }
    // Medido no `bun` 1.4.2: o frame de função JS em código sloppy nunca é toplevel, qualquer que seja o `this`.
    if site.frame().code_type == CodeType::FunctionCode && !site.frame().is_native_function() {
        return false;
    }
    let this_value = site.frame().this_value;
    if this_value.is_undefined_or_null() {
        return true;
    }
    this_value.is_cell()
        && matches!(cell_registry::cell_type(this_value.as_cell()), Some(JSType::GlobalObjectType | JSType::GlobalProxyType))
}

/// O receptor do método, ou o `TypeError` do V8.
fn receiver(call: &HostCall, _method: &str) -> Result<JSCallSiteRef, Thrown> {
    // Medido no `bun` 1.4.2: o texto não nomeia o método.
    JSCallSite::from_value(&call.this_value())
        .ok_or_else(|| Thrown::TypeError("CallSite operation called on non-CallSite object".to_string()))
}

/// Define o corpo e a função nativa de um método: `$result` vê `global_object` e `site`.
macro_rules! call_site_method {
    ($host:ident, $body:ident, $name:literal, |$global_object:ident, $site:ident| $result:expr) => {
        fn $body($global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            let $site = receiver(call, $name)?;
            if crate::runtime::stack_frame::v8_stack_format() {
                if let Some(value) = crate::runtime::v8_stack::call_site_method($global_object, $site.frame(), $name) {
                    return Ok(value);
                }
            }
            Ok($result)
        }
        host_function!($host, $body);
    };
}

// Medido no `bun` 1.4.2: `getThis`, `getFunction` e `getEvalOrigin` dão `undefined` em qualquer frame
// (estrita ou não, método, nativa), e `getTypeName` dá a string `"undefined"`.
call_site_method!(call_site_get_this, get_this_body, "getThis", |_global_object, _site| { JSValue::undefined() });
call_site_method!(call_site_get_function, get_function_body, "getFunction", |_global_object, site| {
    // Medido no `bun` 1.4.2: em código estrito `undefined`; em código sloppy a função chamada, e `null`
    // quando o frame não tem função (programa, `eval`).
    let frame = site.frame();
    if frame.is_strict {
        JSValue::undefined()
    } else if frame.callee != 0 {
        JSValue::from_cell(frame.callee)
    } else {
        JSValue::null()
    }
});
call_site_method!(call_site_get_function_name, get_function_name_body, "getFunctionName", |global_object, site| {
    function_name_value(global_object.vm(), &site)
});
call_site_method!(call_site_get_method_name, get_method_name_body, "getMethodName", |global_object, site| {
    function_name_value(global_object.vm(), &site)
});
/// O nome de arquivo do frame (`undefined` sem URL), comum a `getFileName` e `getScriptNameOrSourceURL`.
fn get_file_name_body_value(global_object: &JSGlobalObject, site: &JSCallSiteRef) -> JSValue {
    match site.frame().call_site_file_name() {
        Some(url) => string_value(global_object.vm(), &url),
        None => JSValue::undefined(),
    }
}
call_site_method!(call_site_get_file_name, get_file_name_body, "getFileName", |global_object, site| {
    get_file_name_body_value(global_object, &site)
});
// Medido no `bun` 1.4.2: `getScriptNameOrSourceURL` devolve o mesmo texto de `getFileName`.
call_site_method!(call_site_get_script_name_or_source_url, get_script_name_or_source_url_body, "getScriptNameOrSourceURL", |global_object, site| {
    get_file_name_body_value(global_object, &site)
});
call_site_method!(call_site_get_line_number, get_line_number_body, "getLineNumber", |_global_object, site| {
    js_number(f64::from(site.frame().call_site_line_number()))
});
call_site_method!(call_site_get_column_number, get_column_number_body, "getColumnNumber", |_global_object, site| {
    js_number(f64::from(site.frame().call_site_column_number()))
});
// Medido no `bun` 1.4.2: número, 0 sem `CodeBlock`.
call_site_method!(call_site_get_script_id, get_script_id_body, "getScriptId", |_global_object, site| {
    js_number(f64::from(site.frame().script_id))
});
call_site_method!(call_site_get_eval_origin, get_eval_origin_body, "getEvalOrigin", |_global_object, _site| {
    JSValue::undefined()
});
call_site_method!(call_site_is_toplevel, is_toplevel_body, "isToplevel", |_global_object, site| {
    js_boolean(is_top_level(&site))
});
call_site_method!(call_site_is_eval, is_eval_body, "isEval", |_global_object, site| {
    js_boolean(site.frame().code_type == CodeType::EvalCode)
});
call_site_method!(call_site_is_native, is_native_body, "isNative", |_global_object, site| { js_boolean(site.frame().is_native_function()) });
call_site_method!(call_site_is_constructor, is_constructor_body, "isConstructor", |_global_object, site| {
    js_boolean(site.frame().is_constructor)
});
call_site_method!(call_site_is_async, is_async_body, "isAsync", |_global_object, site| { js_boolean(site.frame().is_async) });
call_site_method!(call_site_is_promise_all, is_promise_all_body, "isPromiseAll", |_global_object, _site| {
    js_boolean(false)
});
call_site_method!(call_site_get_promise_index, get_promise_index_body, "getPromiseIndex", |_global_object, _site| {
    JSValue::null()
});
call_site_method!(call_site_to_string, to_string_body, "toString", |global_object, site| {
    string_value(global_object.vm(), &site.frame().call_site_text())
});

/// `getTypeName` devolve `Result` (lê `constructor` e `name`, que podem lançar), então não usa o macro.
fn get_type_name_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let site = receiver(call, "getTypeName")?;
    if crate::runtime::stack_frame::v8_stack_format() {
        if let Some(value) = crate::runtime::v8_stack::call_site_method(global_object, site.frame(), "getTypeName") {
            return Ok(value);
        }
    }
    // Medido no `bun` 1.4.2: sempre a string `"undefined"` (`typeof` é `string`), em qualquer frame.
    Ok(string_value(global_object.vm(), "undefined"))
}
host_function!(call_site_get_type_name, get_type_name_body);

/// Os métodos de `CallSitePrototype::finishCreation` (`DontEnum`, comprimento 0), na ordem de
/// `Reflect.ownKeys` medida no `bun` 1.4.2. O `toJSON` vem depois, enumerável (ver `create_prototype`).
const METHODS: [(&[u8], NativeFunction); 19] = [
    (b"getThis", call_site_get_this),
    (b"getTypeName", call_site_get_type_name),
    (b"getFunction", call_site_get_function),
    (b"getFunctionName", call_site_get_function_name),
    (b"getMethodName", call_site_get_method_name),
    (b"getFileName", call_site_get_file_name),
    (b"getLineNumber", call_site_get_line_number),
    (b"getColumnNumber", call_site_get_column_number),
    (b"getScriptId", call_site_get_script_id),
    (b"getEvalOrigin", call_site_get_eval_origin),
    (b"getScriptNameOrSourceURL", call_site_get_script_name_or_source_url),
    (b"isToplevel", call_site_is_toplevel),
    (b"isEval", call_site_is_eval),
    (b"isNative", call_site_is_native),
    (b"isConstructor", call_site_is_constructor),
    (b"isAsync", call_site_is_async),
    (b"isPromiseAll", call_site_is_promise_all),
    (b"getPromiseIndex", call_site_get_promise_index),
    (b"toString", call_site_to_string),
];

/// `CallSite.toJSON`: `{sourceURL, lineNumber, columnNumber, functionName}` (medido no `bun` 1.4.2; o nome é
/// `""` sem função).
fn to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let site = receiver(call, "toJSON")?;
    let vm = global_object.vm();
    let (url, line, column, name) = site.frame().call_site_json_fields();
    let result = construct_empty_object(global_object);
    for (key, value) in [
        (&b"sourceURL"[..], string_value(vm, &url)),
        (&b"lineNumber"[..], js_number(line as f64)),
        (&b"columnNumber"[..], js_number(column as f64)),
        (&b"functionName"[..], string_value(vm, &name)),
    ] {
        result.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, key)), value, 0);
    }
    Ok(result.as_value())
}
host_function!(call_site_to_json, to_json_body);

/// `CallSitePrototype::createStructure`.
fn create_prototype_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
    Structure::create(
        vm,
        Some(global_object),
        prototype,
        TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
        &CALL_SITE_PROTOTYPE_S_INFO,
    )
}

/// `CallSitePrototype::create`: o protótipo com os métodos.
fn create_prototype(vm: &VM, global_object: &JSGlobalObject, object_prototype: JSValue) -> JSObjectRef {
    let structure = create_prototype_structure(vm, global_object, object_prototype);
    let prototype = JSObject::allocate(vm, &structure);
    prototype.finish_creation(vm);
    for (name, function) in METHODS {
        put_native_function(vm, global_object, &prototype, &Identifier::from_span(vm, name), 0, function, Intrinsic::NoIntrinsic);
    }
    // Medido: `toJSON` é a única propriedade enumerável do protótipo (os atributos zero).
    put_direct_native_function_without_transition(
        vm,
        global_object,
        &prototype,
        &Identifier::from_span(vm, b"toJSON".as_slice()),
        0,
        call_site_to_json,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        0,
    );
    put_to_string_tag(vm, &prototype, CALL_SITE_PROTOTYPE_S_INFO.class_name);
    prototype
}

/// Cria o `CallSite.prototype` e a `Structure` dos `CallSite` que o global guarda
/// (`call_site_structure`). O `constructor` do protótipo é o `Object` herdado: o `CallSite` não tem
/// construtor global.
pub fn install_call_site(vm: &VM, global_object: &JSGlobalObject, object_prototype: JSValue) {
    let prototype = create_prototype(vm, global_object, object_prototype);
    prototype.did_become_prototype(vm);
    *global_object.call_site_structure.borrow_mut() =
        Some(JSCallSite::create_structure(vm, Some(global_object), prototype.as_value()));
}
