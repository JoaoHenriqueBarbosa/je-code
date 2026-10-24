//! A classe global `Worker`. O JavaScriptCore não a define: quem a instala é o bun (WebCore). Medido no bun 1.4.2
//! (`wip/notes/worker-plan.md`):
//!
//! - `Worker` (`length` 1) herda de `EventTarget`; chamada sem `new` lança `Use \`new Worker(...)\` instead of
//!   \`Worker(...)\``, sem argumento `Not enough arguments`;
//! - o protótipo tem, nesta ordem, `constructor`, os acessores `onerror`, `onmessage` e `onmessageerror`, os métodos
//!   `postMessage`, `ref` e `terminate`, o acessor `threadId` e `unref`; a instância não tem chave própria;
//! - o script vem de `data:`, `blob:` (registro de `URL.createObjectURL`), `file:` ou de um arquivo relativo ao cwd
//!   (o mesmo caminho do `require`); a falha de resolução não lança no construtor: chega depois, como evento `error`;
//! - o filho roda numa thread própria (`worker_host.rs`); `open`, `message`, `error` e `close` são despachados pelo
//!   laço de eventos do pai (`worker_host::deliver_pending`).
//!
//! LACUNA (fatia 3 do PLAN.md): o global do filho (`self`, `postMessage`, `onmessage`, `close`, `workerData`,
//! `parentPort`) e a entrega dos `postMessage` do pai dentro da thread; a transferência de `MessagePort` entre threads.

use crate::api::module_probe::{file_url_path, resolve_require};
use crate::host_function;
use crate::runtime::broadcast_channel::{checked_accessor_this, checked_method_this};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::event_target::{event_handler, put_methods, register_target, set_event_handler};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_value::{js_number, js_undefined, JSValue};
use crate::runtime::native_class_support::{create_native_subclass, install_global, instance_structure, property_key, put_native_accessor, throw_coded_type_error};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::process_system::current_directory;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::runtime::structured_clone::{transfer_items, value_wire};
use crate::runtime::worker_host::{self, WorkerSpec};

static WORKER_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Worker", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

fn method_this(global_object: &JSGlobalObject, call: &HostCall, name: &str) -> Result<JSValue, Thrown> {
    checked_method_this(global_object, call, "Worker", name, worker_host::is_worker)
}

fn accessor_this(global_object: &JSGlobalObject, call: &HostCall, what: &str, name: &str) -> Result<JSValue, Thrown> {
    checked_accessor_this(global_object, call, "Worker", what, name, worker_host::is_worker)
}

/// O fonte do script e a URL que o identifica, ou a mensagem da falha de resolução (que chega como evento `error`).
fn resolve_script(global_object: &JSGlobalObject, specifier: &str) -> Result<(String, String), String> {
    let not_found = || format!("ModuleNotFound resolving \"{specifier}\" (entry point)");
    if specifier.starts_with("data:") {
        let (bytes, _) = crate::runtime::fetch::parse_data_url(specifier).ok_or_else(not_found)?;
        return Ok((String::from_utf8_lossy(&bytes).into_owned(), specifier.to_string()));
    }
    if specifier.starts_with("blob:") {
        let state = crate::runtime::url::object_url_state(specifier).ok_or_else(not_found)?;
        return Ok((String::from_utf8_lossy(&state.bytes).into_owned(), specifier.to_string()));
    }
    let fs = global_object.module_fs().ok_or_else(not_found)?;
    let request = if let Some(path) = file_url_path(specifier) {
        path
    } else if specifier.starts_with('/') || specifier.starts_with("./") || specifier.starts_with("../") {
        specifier.to_string()
    } else {
        format!("./{specifier}")
    };
    let cwd = current_directory();
    let path = resolve_require(fs.as_ref(), cwd.trim_end_matches('/'), &request, false).ok_or_else(not_found)?;
    let source = fs.read_file(&path).ok_or_else(not_found)?;
    Ok((source, path))
}

/// `options.name` quando `options` é um objeto com ele; vazio no resto.
fn option_name(global_object: &JSGlobalObject, options: JSValue) -> Result<String, Thrown> {
    if !options.is_object() {
        return Ok(String::new());
    }
    let name = get_value_property(global_object, options, &property_key(global_object.vm(), "name"))?;
    if name.is_undefined() {
        return Ok(String::new());
    }
    let text = pending_or(global_object, name.to_wtf_string())?;
    Ok(String::from_utf8_lossy(&text.utf8(ConversionMode::LenientConversion)).into_owned())
}

fn illegal_call_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new Worker(...)` instead of `Worker(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    let specifier_text = pending_or(global_object, call.argument(0).to_wtf_string())?;
    let specifier = String::from_utf8_lossy(&specifier_text.utf8(ConversionMode::LenientConversion)).into_owned();
    let name = option_name(global_object, call.argument(1))?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    register_target(instance);
    let spawned = resolve_script(global_object, &specifier)
        .and_then(|(source, url)| worker_host::spawn(WorkerSpec { source, url, name }).map_err(|error| format!("Failed to start Worker: {error}")));
    worker_host::register(instance, spawned);
    Ok(instance)
}

fn onerror_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(event_handler(accessor_this(global_object, call, "getter", "onerror")?, "error"))
}

fn set_onerror_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    set_event_handler(accessor_this(global_object, call, "setter", "onerror")?, "error", call.argument(0));
    Ok(js_undefined())
}

fn onmessage_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(event_handler(accessor_this(global_object, call, "getter", "onmessage")?, "message"))
}

fn set_onmessage_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    set_event_handler(accessor_this(global_object, call, "setter", "onmessage")?, "message", call.argument(0));
    Ok(js_undefined())
}

fn onmessageerror_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(event_handler(accessor_this(global_object, call, "getter", "onmessageerror")?, "messageerror"))
}

fn set_onmessageerror_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    set_event_handler(accessor_this(global_object, call, "setter", "onmessageerror")?, "messageerror", call.argument(0));
    Ok(js_undefined())
}

fn thread_id_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = accessor_this(global_object, call, "getter", "threadId")?;
    Ok(js_number(f64::from(worker_host::thread_id_of(this).unwrap_or(0))))
}

/// A lista de transferência: um iterável, ou `{ transfer }`; ausente é vazia.
fn transfer_list(global_object: &JSGlobalObject, argument: JSValue) -> Result<Vec<JSValue>, Thrown> {
    if argument.is_undefined() {
        return Ok(Vec::new());
    }
    let invalid = "The \"transfer\" argument must be an instance of Array or an object with a \"transfer\" array";
    if argument.is_object() {
        let member = get_value_property(global_object, argument, &property_key(global_object.vm(), "transfer"))?;
        if !member.is_undefined() {
            return transfer_items(global_object, member, invalid);
        }
    }
    transfer_items(global_object, argument, invalid)
}

/// `postMessage(valor, transfer)`: serializa na borda (o erro de clone é síncrono) e entrega os bytes à thread.
fn post_message_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = method_this(global_object, call, "postMessage")?;
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    let transfer = transfer_list(global_object, call.argument(1))?;
    let serialized = value_wire::serialize(global_object, Some(call), call.argument(0), &transfer)?;
    worker_host::post_to(this, serialized.bytes);
    Ok(js_undefined())
}

fn ref_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    worker_host::set_ref(method_this(global_object, call, "ref")?, true);
    Ok(js_undefined())
}

fn unref_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    worker_host::set_ref(method_this(global_object, call, "unref")?, false);
    Ok(js_undefined())
}

fn terminate_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(worker_host::terminate(global_object, method_this(global_object, call, "terminate")?))
}

host_function!(call_worker, illegal_call_body);
host_function!(construct_worker, construct_body);
host_function!(worker_onerror, onerror_body);
host_function!(worker_set_onerror, set_onerror_body);
host_function!(worker_onmessage, onmessage_body);
host_function!(worker_set_onmessage, set_onmessage_body);
host_function!(worker_onmessageerror, onmessageerror_body);
host_function!(worker_set_onmessageerror, set_onmessageerror_body);
host_function!(worker_thread_id, thread_id_body);
host_function!(worker_post_message, post_message_body);
host_function!(worker_ref, ref_body);
host_function!(worker_unref, unref_body);
host_function!(worker_terminate, terminate_body);

/// Instala `Worker` no global; `event_target` é o par (protótipo, construtor) do `EventTarget`, de que ele herda.
/// A posição no global vem da tabela `ORDER`.
pub fn install_worker(global_object: &JSGlobalObject, event_target: (JSValue, JSValue)) {
    let vm = global_object.vm();
    let (prototype, constructor) =
        create_native_subclass(global_object, event_target, &WORKER_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "Worker", 1, call_worker, construct_worker);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    put_native_accessor(vm, global_object, &prototype, "onerror", worker_onerror, Some(worker_set_onerror), 0);
    put_native_accessor(vm, global_object, &prototype, "onmessage", worker_onmessage, Some(worker_set_onmessage), 0);
    put_native_accessor(vm, global_object, &prototype, "onmessageerror", worker_onmessageerror, Some(worker_set_onmessageerror), 0);
    put_methods(
        global_object,
        &prototype,
        &[("postMessage", 1, worker_post_message as NativeFunction), ("ref", 0, worker_ref), ("terminate", 0, worker_terminate)],
    );
    put_native_accessor(vm, global_object, &prototype, "threadId", worker_thread_id, None, 0);
    put_methods(global_object, &prototype, &[("unref", 0, worker_unref as NativeFunction)]);
    put_to_string_tag(vm, &prototype, "Worker");
    install_global(global_object, "Worker", constructor.as_value());
}
