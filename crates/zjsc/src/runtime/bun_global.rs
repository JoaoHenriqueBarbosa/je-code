//! O global `Bun` do bun: o `JSBunObject` de `BunObject.cpp`, um objeto com a tabela estática `bunObjectTable`
//! (`HasStaticPropertyTable`) e `@@toStringTag` `Bun`. As entradas `PropertyCallback` nascem no primeiro acesso, as
//! `Function` viram funções nativas, e a ordem de `Object.getOwnPropertyNames(Bun)` é a da tabela. Aqui ficam só as
//! entradas que o porte já sustenta, na posição que têm na tabela do bun 1.4.2; as demais (servidor, arquivos,
//! transpilador, `which`...) entram nos degraus seguintes do plano (`wip/notes/bun-on-zjsc-plan.md`).
//!
//! Propriedade de dados comum do global; na ordem de chaves vem entre `global` e `File`, o que a tabela `ORDER` de
//! `js_global_object_init.rs` já resolve.

use std::rc::Rc;

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::console_client::inspect_one;
use crate::runtime::console_format::Formatter;
use crate::runtime::current_realm::current_global_object;
use crate::runtime::date_instance::DateInstance;
use crate::runtime::deep_equals::{deep_equals, Mode};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::object_bindings::get_if_property_exists_prototype_pollution_mitigation;
use crate::runtime::string_width;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array::JSArray;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_promise::{JSPromise, Status};
use crate::runtime::js_promise_host::PromiseHost;
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{js_number, EncodedJSValue, JSValue};
use crate::runtime::lookup::{custom_accessor_entry_with_attributes, lazy_entry_with_attributes, native_function_entry, HashTable, HashTableValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::node_error::throw_coded_type_error;
use crate::runtime::process_shape::{process_argv, read_slot, write_slot, BUN_REVISION, BUN_VERSION};
use crate::runtime::property_attribute::{DONT_DELETE, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::Structure;
use crate::runtime::symbol::Symbol;
use crate::runtime::vm::VM;
use crate::runtime::web_iterable::units_of;
use crate::wtf::text::string_impl::StringImpl;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url::URL;

/// O maior comprimento de string do JSC (`String::MaxLength`); o `escapeHTML` que passaria dele lança falta de memória.
const STRING_MAX_LENGTH: usize = i32::MAX as usize;

/// A chave em que `Bun.main = valor` guarda o `overridden_main` (some no fim do programa, como os demais).
const OVERRIDDEN_MAIN: &str = "Bun.main";

fn string_value(vm: &VM, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes())))
}

fn units_value(vm: &VM, units: &[u16]) -> JSValue {
    JSValue::from_js_string(js_string(vm, &WtfString::from_utf16(units)))
}

/// Função nativa com `name` e `length` dados e sem construtor, como as do `@begin` (`DontDelete|Function N`).
fn native(global_object: &JSGlobalObject, name: &str, length: u32, function: NativeFunction) -> JSValue {
    JSFunction::create_native(
        global_object.vm(),
        global_object,
        length,
        &WtfString::from_utf8(name.as_bytes()),
        function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    )
    .as_value()
}

/// `Bun.inspect(value, options)`.
fn inspect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // O formatador do bun (strings entre aspas duplas, arrays e objetos longos em linhas); `util.inspect` tem outro
    // estilo. As opções (`depth`, `colors`) ainda não são lidas.
    let Some(text) = inspect_one(global_object, Formatter::new(), call.argument(0)) else { return Err(Thrown::Pending) };
    Ok(string_value(global_object.vm(), &text))
}

host_function!(bun_inspect, inspect_body);

/// `Bun.inspect.table(tabularData, properties, options)`: o texto do `console.table`, ou `""` para o que não é objeto;
/// `properties` que não é array é ignorado (o `console.table` o recusa).
fn inspect_table_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let tabular_data = call.argument(0);
    if !tabular_data.is_object() {
        return Ok(string_value(global_object.vm(), ""));
    }
    let properties = call.argument(1);
    let properties = if JSArray::from_value(&properties).is_some() { properties } else { JSValue::undefined() };
    let Some(text) = crate::runtime::console_table::render(global_object, tabular_data, properties, 0) else { return Err(Thrown::Pending) };
    Ok(string_value(global_object.vm(), &text))
}

host_function!(bun_inspect_table, inspect_table_body);

/// `BunObject_lazyPropCb_wrap_inspect`: a função `inspect` com `custom` (o `Symbol.for('nodejs.util.inspect.custom')`) e
/// `table`, os dois propriedades de dados comuns.
fn create_inspect(vm: &VM, _bun: &JSObject) -> JSValue {
    let global_object = current_global_object();
    let inspect = native(&global_object, "inspect", 2, bun_inspect);
    let inspect_object = inspect.as_object();
    let registered = vm.symbol_registry().symbol_for_key(&StringImpl::create(b"nodejs.util.inspect.custom"));
    let custom = Symbol::create_with_registered_uid(vm, &registered).to_primitive();
    inspect_object.put_direct(vm, &PropertyName::from_identifier(&crate::runtime::identifier::Identifier::from_span(vm, b"custom")), custom, 0);
    let table = native(&global_object, "table", 3, bun_inspect_table);
    inspect_object.put_direct(vm, &PropertyName::from_identifier(&crate::runtime::identifier::Identifier::from_span(vm, b"table")), table, 0);
    inspect
}

fn create_argv(_vm: &VM, _bun: &JSObject) -> JSValue {
    process_argv(&current_global_object())
}

/// `BunObject_lazyPropCb_wrap_hash`.
fn create_hash(_vm: &VM, _bun: &JSObject) -> JSValue {
    crate::runtime::hash_object::create(&current_global_object())
}

fn create_dns(_vm: &VM, _bun: &JSObject) -> JSValue {
    crate::runtime::bun_dns::dns_object(&current_global_object()).unwrap_or_else(|_| JSValue::undefined())
}

fn create_env(_vm: &VM, _bun: &JSObject) -> JSValue {
    crate::runtime::process_env::env_value(&current_global_object())
}

fn create_stdin(_vm: &VM, _bun: &JSObject) -> JSValue {
    crate::runtime::blob_file::std_file(&current_global_object(), 0)
}

fn create_stdout(_vm: &VM, _bun: &JSObject) -> JSValue {
    crate::runtime::blob_file::std_file(&current_global_object(), 1)
}

fn create_stderr(_vm: &VM, _bun: &JSObject) -> JSValue {
    crate::runtime::blob_file::std_file(&current_global_object(), 2)
}

fn create_revision(vm: &VM, _bun: &JSObject) -> JSValue {
    string_value(vm, BUN_REVISION)
}

fn create_version(vm: &VM, _bun: &JSObject) -> JSValue {
    string_value(vm, BUN_VERSION)
}

/// `get_main` de `BunObject.rs`: o valor gravado por `Bun.main = ...`, ou o caminho do script principal.
fn main_getter(global_object: &JSGlobalObject, _this: EncodedJSValue, _name: &PropertyName) -> EncodedJSValue {
    read_slot(OVERRIDDEN_MAIN)
        .unwrap_or_else(|| string_value(global_object.vm(), &crate::runtime::process_system::main_path()))
        .encode()
}

/// `set_main`: guarda qualquer valor, que passa a ser o `Bun.main`.
fn main_setter(_global_object: &JSGlobalObject, _this: EncodedJSValue, value: EncodedJSValue, _name: &PropertyName) -> bool {
    write_slot(OVERRIDDEN_MAIN, JSValue::decode(value));
    true
}

/// `functionBunNanoseconds`: nanossegundos inteiros desde a origem do relógio do processo (a do `performance.now()`).
fn nanoseconds_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number((crate::runtime::performance::now_ms() * 1_000_000.0).round()))
}

host_function!(bun_nanoseconds, nanoseconds_body);

/// `sleep_sync` de `BunObject.rs`: o argumento tem de ser número; o inteiro de 32 bits saturado (NaN é 0) não pode ser
/// negativo; a thread dorme esse tanto de milissegundos.
fn sleep_sync_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments to 'sleepSync'. Expected 1, got 0.", "ERR_MISSING_ARGS"));
    }
    let argument = call.argument(0);
    if !argument.is_number() {
        return Err(throw_coded_type_error(global_object, "Expected milliseconds to be a number for 'sleepSync'.", "ERR_INVALID_ARG_TYPE"));
    }
    // `coerce::<i32>`: o `as` do Rust satura e leva NaN a 0.
    let milliseconds = argument.as_number() as i32;
    if milliseconds < 0 {
        return Err(throw_coded_type_error(global_object, &format!("argument to sleepSync must not be negative, got {milliseconds}"), "ERR_INVALID_ARG_TYPE"));
    }
    ul_common::time::sleep(std::time::Duration::from_millis(milliseconds as u64));
    Ok(JSValue::undefined())
}

host_function!(bun_sleep_sync, sleep_sync_body);

/// `functionBunSleep`: uma `Date` vira o prazo até ela (arredondado para cima, nunca negativo); o resto tem de ser
/// número. O timer é um `setTimeout` (`Kind::SetTimeout`, consome um id) cujo callback é a promessa, com o prazo
/// saturado em `[0, i32::MAX]` (`CountdownOverflowBehavior::Clamp`), e mantém o laço vivo.
fn sleep_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let mut milliseconds = call.argument(0);
    if let Some(date) = DateInstance::from_value(&milliseconds) {
        let now = crate::wtf::date_math::current_time_in_nanoseconds() as f64 / 1_000_000.0;
        let remaining = date.internal_number() - now;
        milliseconds = js_number(if remaining > 0.0 { remaining.ceil() } else { 0.0 });
    }
    if !milliseconds.is_number() {
        return Err(Thrown::type_error("sleep expects a number (milliseconds)"));
    }
    let countdown = (milliseconds.as_number() as u32).min(i32::MAX as u32);
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    let fired = promise.clone();
    crate::runtime::timers::schedule_native_with_ref(
        global_object,
        f64::from(countdown),
        vec![promise.as_value()],
        Rc::new(move |global: &JSGlobalObject| fired.resolve(global, JSValue::undefined())),
        true,
    );
    Ok(promise.as_value())
}

host_function!(bun_sleep, sleep_body);

/// `jsFunctionBunEscapeHTML` (`escapeHTML.cpp`): `&`, `<`, `>`, `"` e `'` viram `&amp;`, `&lt;`, `&gt;`, `&quot;` e
/// `&#x27;`. Número, booleano, `undefined` e `null` saem pelo `toString`; o resto passa pelo `toString` também.
fn escape_html_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let argument = call.argument(0);
    let units = units_of(global_object, argument)?;
    if !units.iter().any(|unit| matches!(*unit, 0x26 | 0x3c | 0x3e | 0x22 | 0x27)) {
        return Ok(if argument.is_string() { argument } else { units_value(vm, &units) });
    }
    let mut escaped: Vec<u16> = Vec::with_capacity(units.len() + units.len() / 2);
    for unit in units {
        let replacement: &str = match unit {
            0x26 => "&amp;",
            0x3c => "&lt;",
            0x3e => "&gt;",
            0x22 => "&quot;",
            0x27 => "&#x27;",
            _ => {
                escaped.push(unit);
                continue;
            }
        };
        escaped.extend(replacement.encode_utf16());
    }
    if escaped.len() > STRING_MAX_LENGTH {
        return Err(Thrown::OutOfMemory);
    }
    Ok(units_value(vm, &escaped))
}

host_function!(bun_escape_html, escape_html_body);

/// `functionPathToFileURL`: o caminho (relativo resolvido no diretório corrente, absoluto como veio) vira um `URL`
/// `file://` com os bytes escapados (`URL::fileURLWithFileSystemPath`).
fn path_to_file_url_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let path = pending_or(global_object, call.argument(0).to_wtf_string())?;
    let path = if path.starts_with_character('/' as u16) {
        path
    } else {
        let text = String::from_utf16_lossy(&(0..path.length()).map(|index| path.code_unit_at(index)).collect::<Vec<u16>>());
        let resolved = crate::runtime::process_system::resolve_in_current_directory(&text);
        WtfString::from_utf8(resolved.as_bytes())
    };
    Ok(crate::runtime::url::create_url_object(global_object, URL::file_url_with_file_system_path(&path)))
}

host_function!(bun_path_to_file_url, path_to_file_url_body);

/// `functionFileURLToPath`: aceita `URL` ou string; só `file:`, sem host (ou `localhost`), sem `/` escapado.
fn file_url_to_path_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    let url = match crate::runtime::url::url_of(argument) {
        Some(url) => url,
        None if argument.is_string() => URL::from_string(&pending_or(global_object, argument.to_wtf_string())?),
        None => {
            let received = crate::runtime::js_web_assembly::received_description(global_object, argument);
            return Err(throw_coded_type_error(global_object, &format!("The \"url\" argument must be of type string. Received {received}"), "ERR_INVALID_ARG_TYPE"));
        }
    };
    if !url.protocol_is_file() {
        return Err(throw_coded_type_error(global_object, "The URL must be of scheme file", "ERR_INVALID_URL_SCHEME"));
    }
    let host = url.host();
    if host.length() > 0 && host != WtfString::from_latin1(b"localhost") {
        return Err(throw_coded_type_error(global_object, "File URL host must be \"localhost\" or empty on linux", "ERR_INVALID_FILE_URL_HOST"));
    }
    let path = url.path();
    let units: Vec<u16> = (0..path.length()).map(|index| path.code_unit_at(index)).collect();
    if units.windows(3).any(|window| window[0] == u16::from(b'%') && window[1] == u16::from(b'2') && (window[2] == u16::from(b'f') || window[2] == u16::from(b'F'))) {
        return Err(throw_coded_type_error(global_object, "File URL path must not include encoded / characters", "ERR_INVALID_FILE_URL_PATH"));
    }
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &url.file_system_path())))
}

host_function!(bun_file_url_to_path, file_url_to_path_body);

/// `peek` de `Peek.ts`: o valor de uma promessa já resolvida (cumprida ou rejeitada), ou o próprio argumento.
fn peek_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    Ok(match JSPromise::from_value(&argument) {
        Some(promise) if promise.status() != Status::Pending => promise.result(),
        _ => argument,
    })
}

host_function!(bun_peek, peek_body);

/// `peekStatus` de `Peek.ts`: `pending`, `fulfilled` ou `rejected`; o que não é promessa conta como `fulfilled`.
fn peek_status_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let status = JSPromise::from_value(&call.argument(0)).map_or(Status::Fulfilled, |promise| promise.status());
    let name = match status {
        Status::Pending => "pending",
        Status::Fulfilled => "fulfilled",
        Status::Rejected => "rejected",
    };
    Ok(string_value(global_object.vm(), name))
}

host_function!(bun_peek_status, peek_status_body);

/// `constructBunPeekObject`: as duas funções nascem de builtins JS sem nome (`name` vazio), e `status` é
/// `ReadOnly|DontDelete`.
fn create_peek(vm: &VM, _bun: &JSObject) -> JSValue {
    let global_object = current_global_object();
    let peek = native(&global_object, "", 1, bun_peek);
    let status = native(&global_object, "", 1, bun_peek_status);
    peek.as_object().put_direct(vm, &PropertyName::from_identifier(&crate::runtime::identifier::Identifier::from_span(vm, b"status")), status, READ_ONLY | DONT_DELETE);
    peek
}

/// `functionBunDeepEquals`: menos de dois argumentos lança `TypeError`; `strict` e `skipPrototype` só valem como
/// booleano `true`.
fn deep_equals_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 2 {
        return Err(Thrown::type_error("Expected 2 values to compare"));
    }
    let is_true = |value: JSValue| value.is_boolean() && value.as_boolean();
    let strict = is_true(call.argument(2));
    let mode = Mode { strict, skip_prototype_identity: strict && is_true(call.argument(3)), check_prototypes: false };
    Ok(JSValue::Bool(deep_equals(global_object, mode, call.argument(0), call.argument(1))?))
}

host_function!(bun_deep_equals, deep_equals_body);

/// `jsFunctionBunStripANSI`: o argumento vira texto (`toString`); sem sequência ANSI volta a própria string.
fn strip_ansi_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    let units = units_of(global_object, argument)?;
    match crate::runtime::ansi::strip_ansi(&units) {
        Some(stripped) => Ok(units_value(global_object.vm(), &stripped)),
        None if argument.is_string() => Ok(argument),
        None => Ok(units_value(global_object.vm(), &units)),
    }
}

host_function!(bun_strip_ansi, strip_ansi_body);

/// `applyTruthyBooleanOption` do `stringWidth.cpp`: `undefined`, `null` e string vazia mantêm o padrão; o resto vira
/// booleano (`ToBoolean`).
fn truthy_option(global_object: &JSGlobalObject, options: &ObjectRef, name: &str, default: bool) -> Result<bool, Thrown> {
    let vm = global_object.vm();
    let property_name = PropertyName::from_identifier(&crate::runtime::identifier::Identifier::from_span(vm, name.as_bytes()));
    let value = get_if_property_exists_prototype_pollution_mitigation(global_object, options, &property_name)?.unwrap_or(JSValue::undefined());
    if value.is_undefined_or_null() || (value.is_string() && !value.to_boolean()) {
        return Ok(default);
    }
    Ok(value.to_boolean())
}

/// `jsFunctionBunStringWidth`: `undefined` mede 0; o resto vira texto. Opções: `countAnsiEscapeCodes` (falso),
/// `ambiguousIsNarrow` (verdadeiro) e `perCodePoint` (falso), lidas sem enxergar o `Object.prototype`.
fn string_width_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let input = call.argument(0);
    if input.is_undefined() {
        return Ok(js_number(0.0));
    }
    let units = units_of(global_object, input)?;
    if units.is_empty() {
        return Ok(js_number(0.0));
    }
    let (mut count_ansi, mut ambiguous_is_narrow, mut per_code_point) = (false, true, false);
    let options = call.argument(1);
    if let Some(options) = ObjectRef::from_value(&options).filter(|_| options.is_object()) {
        count_ansi = truthy_option(global_object, &options, "countAnsiEscapeCodes", count_ansi)?;
        ambiguous_is_narrow = truthy_option(global_object, &options, "ambiguousIsNarrow", ambiguous_is_narrow)?;
        per_code_point = truthy_option(global_object, &options, "perCodePoint", per_code_point)?;
    }
    let measure = if per_code_point { string_width::per_code_point_width } else { string_width::visible_width };
    Ok(js_number(measure(&units, !ambiguous_is_narrow, count_ansi) as f64))
}

host_function!(pub bun_string_width, string_width_body);

/// `bunObjectTable` de `BunObject.cpp`, na ordem do `@begin`, só com as entradas portadas.
static BUN_OBJECT_TABLE_VALUES: [HashTableValue; 28] = [
    lazy_entry_with_attributes("argv", DONT_DELETE, create_argv),
    native_function_entry("connect", DONT_DELETE, crate::runtime::bun_http::bun_connect, 1, Intrinsic::NoIntrinsic),
    native_function_entry("deepEquals", DONT_DELETE, bun_deep_equals, 2, Intrinsic::NoIntrinsic),
    lazy_entry_with_attributes("dns", DONT_DELETE, create_dns),
    lazy_entry_with_attributes("env", READ_ONLY | DONT_DELETE, create_env),
    native_function_entry("escapeHTML", DONT_DELETE, bun_escape_html, 2, Intrinsic::NoIntrinsic),
    native_function_entry("file", DONT_DELETE, crate::runtime::blob_file::bun_file, 1, Intrinsic::NoIntrinsic),
    native_function_entry("fileURLToPath", DONT_DELETE, bun_file_url_to_path, 1, Intrinsic::NoIntrinsic),
    lazy_entry_with_attributes("hash", DONT_DELETE, create_hash),
    lazy_entry_with_attributes("inspect", DONT_DELETE, create_inspect),
    native_function_entry("listen", DONT_DELETE, crate::runtime::bun_http::bun_listen, 1, Intrinsic::NoIntrinsic),
    custom_accessor_entry_with_attributes("main", DONT_DELETE, main_getter, main_setter),
    native_function_entry("nanoseconds", DONT_DELETE, bun_nanoseconds, 0, Intrinsic::NoIntrinsic),
    native_function_entry("pathToFileURL", DONT_DELETE, bun_path_to_file_url, 1, Intrinsic::NoIntrinsic),
    lazy_entry_with_attributes("peek", DONT_DELETE, create_peek),
    lazy_entry_with_attributes("revision", READ_ONLY | DONT_DELETE, create_revision),
    native_function_entry("serve", DONT_DELETE, crate::runtime::bun_http::bun_serve, 1, Intrinsic::NoIntrinsic),
    native_function_entry("sleep", DONT_DELETE, bun_sleep, 1, Intrinsic::NoIntrinsic),
    native_function_entry("sleepSync", DONT_DELETE, bun_sleep_sync, 1, Intrinsic::NoIntrinsic),
    native_function_entry("spawn", DONT_DELETE, crate::runtime::proc_spawn::bun_spawn, 1, Intrinsic::NoIntrinsic),
    native_function_entry("spawnSync", DONT_DELETE, crate::runtime::proc_spawn::bun_spawn_sync, 1, Intrinsic::NoIntrinsic),
    lazy_entry_with_attributes("stderr", DONT_DELETE, create_stderr),
    lazy_entry_with_attributes("stdin", DONT_DELETE, create_stdin),
    lazy_entry_with_attributes("stdout", DONT_DELETE, create_stdout),
    native_function_entry("stringWidth", DONT_DELETE, bun_string_width, 2, Intrinsic::NoIntrinsic),
    native_function_entry("stripANSI", DONT_DELETE, bun_strip_ansi, 1, Intrinsic::NoIntrinsic),
    lazy_entry_with_attributes("version", READ_ONLY | DONT_DELETE, create_version),
    native_function_entry("write", DONT_DELETE, crate::runtime::blob_file::bun_write, 1, Intrinsic::NoIntrinsic),
];

static BUN_OBJECT_TABLE: HashTable = HashTable { class_for_this: None, values: &BUN_OBJECT_TABLE_VALUES };

/// `const ClassInfo JSBunObject::s_info = { "Bun"_s, &Base::s_info, &bunObjectTable, ... }`.
static JS_BUN_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "Bun", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: Some(&BUN_OBJECT_TABLE), inherits_js_type_range: None };

/// `JSBunObject::create` e o global `Bun`.
pub fn install_bun(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let structure = Structure::create(
        vm,
        Some(global_object),
        global_object.object_prototype().as_value(),
        TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE),
        &JS_BUN_OBJECT_S_INFO,
    );
    let bun = JSObject::allocate(vm, &structure);
    bun.finish_creation(vm);
    put_to_string_tag(vm, &bun, "Bun");
    global_object.put_direct(vm, &PropertyName::from_identifier(&crate::runtime::identifier::Identifier::from_span(vm, b"Bun")), bun.as_value(), 0);
}
