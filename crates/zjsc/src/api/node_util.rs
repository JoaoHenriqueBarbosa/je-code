//! `require("util")` do bun 1.4.2: o `util.ts` em JavaScript (`node_util.js`, gerado por `scripts/gen-node-util.js` junto
//! com os internos que ele carrega: `primordials`, `inspect`, `promisify`, `deprecate`, `colors` e `tty`) e as funções
//! nativas que o bun entrega a esse código pelo C++ e pelo Rust (`UtilInspect.cpp`, `node_util_binding.rs`,
//! `NodeUtilTypesModule.cpp` e os intrínsecos do JSC), aqui no objeto `host`.

use crate::api::eval::typeof_for_property_error;
use crate::api::node_js_host::{module_host, run_factory};
use crate::host_function;
use crate::runtime::cell_registry;
use crate::runtime::deep_equals::{deep_equals, Mode};
use crate::runtime::error_natives::ERROR_PROTOTYPE_WITH_TABLE_S_INFO;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::{parse_index_identifier, Identifier};
use crate::runtime::js_internal_field_object_impl::InternalFields as _;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_promise::{JSPromise, Status};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::native_class_support::property_key;
use crate::runtime::node_error::throw_coded_type_error;
use crate::runtime::object_constructor::{construct_array_of, construct_empty_object, identifier_to_js_value, put_own_data_property_may_be_index};
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::runtime::property_name::PropertyName;
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PrivateSymbolMode, PropertyNameMode};
use crate::runtime::js_promise_host::PromiseHost as _;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::proxy_object::{object_get_own_property_names, ProxyObject};
use crate::runtime::url::wtf_units;
use crate::wtf::text::wtf_string::String as WtfString;

const SOURCE: &str = include_str!("node_util.js");

/// A tabela `UV_E::ENTRIES` do bun para o Linux (código negativo, nome): os 16 sintéticos do libuv (`EOF`, `UNKNOWN`, os
/// `EAI_*`) e depois as 69 linhas por sistema, na ordem do `__uv_e_rows!`. Medida com `util.getSystemErrorMap()` no bun
/// 1.4.2 do oráculo; `ECHARSET` e `EFTYPE` são os códigos sintéticos do libuv, porque o kernel não os tem.
pub(crate) static UV_ERRORS: &[(i32, &str)] = &[
    (-4095, "EOF"), (-4094, "UNKNOWN"), (-3000, "EAI_ADDRFAMILY"), (-3001, "EAI_AGAIN"), (-3002, "EAI_BADFLAGS"),
    (-3003, "EAI_CANCELED"), (-3004, "EAI_FAIL"), (-3005, "EAI_FAMILY"), (-3006, "EAI_MEMORY"), (-3007, "EAI_NODATA"),
    (-3008, "EAI_NONAME"), (-3009, "EAI_OVERFLOW"), (-3010, "EAI_SERVICE"), (-3011, "EAI_SOCKTYPE"), (-3013, "EAI_BADHINTS"),
    (-3014, "EAI_PROTOCOL"), (-7, "E2BIG"), (-13, "EACCES"), (-98, "EADDRINUSE"), (-99, "EADDRNOTAVAIL"),
    (-97, "EAFNOSUPPORT"), (-11, "EAGAIN"), (-114, "EALREADY"), (-9, "EBADF"), (-16, "EBUSY"), (-125, "ECANCELED"),
    (-4080, "ECHARSET"), (-103, "ECONNABORTED"), (-111, "ECONNREFUSED"), (-104, "ECONNRESET"), (-89, "EDESTADDRREQ"),
    (-17, "EEXIST"), (-14, "EFAULT"), (-113, "EHOSTUNREACH"), (-4, "EINTR"), (-22, "EINVAL"), (-5, "EIO"), (-106, "EISCONN"),
    (-21, "EISDIR"), (-40, "ELOOP"), (-24, "EMFILE"), (-90, "EMSGSIZE"), (-36, "ENAMETOOLONG"), (-100, "ENETDOWN"),
    (-101, "ENETUNREACH"), (-23, "ENFILE"), (-105, "ENOBUFS"), (-19, "ENODEV"), (-2, "ENOENT"), (-12, "ENOMEM"),
    (-64, "ENONET"), (-28, "ENOSPC"), (-38, "ENOSYS"), (-107, "ENOTCONN"), (-20, "ENOTDIR"), (-39, "ENOTEMPTY"),
    (-88, "ENOTSOCK"), (-95, "ENOTSUP"), (-1, "EPERM"), (-32, "EPIPE"), (-71, "EPROTO"), (-93, "EPROTONOSUPPORT"),
    (-91, "EPROTOTYPE"), (-30, "EROFS"), (-108, "ESHUTDOWN"), (-29, "ESPIPE"), (-3, "ESRCH"), (-110, "ETIMEDOUT"),
    (-26, "ETXTBSY"), (-18, "EXDEV"), (-27, "EFBIG"), (-92, "ENOPROTOOPT"), (-34, "ERANGE"), (-6, "ENXIO"), (-31, "EMLINK"),
    (-112, "EHOSTDOWN"), (-121, "EREMOTEIO"), (-25, "ENOTTY"), (-4028, "EFTYPE"), (-84, "EILSEQ"), (-75, "EOVERFLOW"),
    (-94, "ESOCKTNOSUPPORT"), (-61, "ENODATA"), (-49, "EUNATCH"), (-8, "ENOEXEC"),
];

fn text(global_object: &JSGlobalObject, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(text.as_bytes())))
}

/// `internalErrorName(err)`: o nome do libuv para o código negativo, ou `Unknown system error N`.
fn internal_error_name_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let code = call.argument(0).to_int32();
    match UV_ERRORS.iter().find(|(known, _)| *known == code) {
        Some((_, name)) => Ok(text(global_object, name)),
        None => Ok(text(global_object, &format!("Unknown system error {code}"))),
    }
}
host_function!(internal_error_name, internal_error_name_body);

/// `internalErrorEntries()`: os pares `[código, nome, código, nome, ...]` da tabela inteira.
fn internal_error_entries_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let values: Vec<JSValue> = UV_ERRORS.iter().flat_map(|(code, name)| [js_number(*code), text(global_object, name)]).collect();
    Ok(construct_array_of(global_object, &values).as_value())
}
host_function!(internal_error_entries, internal_error_entries_body);

/// `extractedSplitNewLinesFastPathStringsOnly(valor)`: `valor.split(/(?<=\n)/)`: cada linha com o `\n`, sem campo vazio
/// no fim quando o texto acaba em `\n`, e `""` dá `[""]`.
fn split_new_lines_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let units = wtf_units(&call.argument(0).to_wtf_string());
    let mut lines: Vec<JSValue> = Vec::new();
    let mut start = 0;
    loop {
        if start == units.len() && start != 0 {
            break;
        }
        let end = units[start..].iter().position(|&unit| unit == u16::from(b'\n')).map_or(units.len(), |at| start + at + 1);
        lines.push(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf16(&units[start..end]))));
        if end == units.len() {
            break;
        }
        start = end;
    }
    Ok(construct_array_of(global_object, &lines).as_value())
}
host_function!(split_new_lines, split_new_lines_body);

/// `jsFunctionGetOwnNonIndexProperties(objeto, filtro)`: as chaves próprias sem os índices, na máscara do Node
/// (`ONLY_ENUMERABLE` 2, `SKIP_STRINGS` 8, `SKIP_SYMBOLS` 16).
fn get_own_non_index_properties_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    const ONLY_ENUMERABLE: i32 = 1 << 1;
    const SKIP_STRINGS: i32 = 1 << 3;
    const SKIP_SYMBOLS: i32 = 1 << 4;
    let Some(object) = call.argument(0).to_object(global_object) else { return Err(Thrown::Pending) };
    let filter = call.argument(1).to_int32();
    if filter & SKIP_STRINGS != 0 && filter & SKIP_SYMBOLS != 0 {
        return Ok(construct_array_of(global_object, &[]).as_value());
    }
    let mode = if filter & SKIP_STRINGS != 0 {
        PropertyNameMode::Symbols
    } else if filter & SKIP_SYMBOLS != 0 {
        PropertyNameMode::Strings
    } else {
        PropertyNameMode::StringsAndSymbols
    };
    let dont_enum = if filter & ONLY_ENUMERABLE != 0 { DontEnumPropertiesMode::Exclude } else { DontEnumPropertiesMode::Include };
    let vm = global_object.vm();
    let mut names = PropertyNameArrayBuilder::new(vm, mode, PrivateSymbolMode::Exclude);
    // O `[[OwnPropertyKeys]]` inteiro (índices primeiro, texto, símbolos) sem os índices é o mesmo que o
    // `getOwnNonIndexPropertyNames` do C++ pede aos objetos comuns, e vale também para `Proxy`, objetos `String`,
    // `arguments` e espaços de nomes de módulo, que o C++ consulta pela tabela de métodos.
    object_get_own_property_names(global_object, &object, &mut names, dont_enum)?;
    let keys: Vec<JSValue> =
        names.iter().filter(|name| name.is_symbol() || parse_index_identifier(name).is_none()).map(|name| identifier_to_js_value(vm, name)).collect();
    Ok(construct_array_of(global_object, &keys).as_value())
}
host_function!(get_own_non_index_properties, get_own_non_index_properties_body);

/// `$peekPromiseStatus(promise)`: 0 pendente, 1 cumprida, 2 rejeitada.
fn promise_status_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let status = JSPromise::from_value(&call.argument(0)).map_or(Status::Pending, |promise| promise.status());
    Ok(js_number(status as i32))
}
host_function!(promise_status, promise_status_body);

/// `$peekPromiseSettledValue(promise)`.
fn promise_result_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSPromise::from_value(&call.argument(0)).map_or(JSValue::undefined(), |promise| promise.result()))
}
host_function!(promise_result, promise_result_body);

/// `$isPromise(valor)`.
fn is_promise_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(JSPromise::from_value(&call.argument(0)).is_some()))
}
host_function!(is_promise, is_promise_body);

/// `$newPromise()`.
fn new_promise_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSPromise::create(global_object.vm(), &global_object.promise_structure()).as_value())
}
host_function!(new_promise, new_promise_body);

/// `$resolvePromiseWithFirstResolvingFunctionCallCheck(promise, valor)`.
fn resolve_promise_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if let Some(promise) = JSPromise::from_value(&call.argument(0)) {
        promise.resolve(global_object, call.argument(1));
    }
    Ok(JSValue::undefined())
}
host_function!(resolve_promise, resolve_promise_body);

/// `$isProxyObject(valor)`.
fn is_proxy_object_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(ProxyObject::from_value(&call.argument(0)).is_some()))
}
host_function!(is_proxy_object, is_proxy_object_body);

/// `$getProxyInternalField(proxy, $proxyFieldTarget)`.
fn proxy_target_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(ProxyObject::from_value(&call.argument(0)).map_or(JSValue::undefined(), |proxy| proxy.target()))
}
host_function!(proxy_target, proxy_target_body);

/// `$getProxyInternalField(proxy, $proxyFieldHandler)`: `null` no `Proxy` revogado.
fn proxy_handler_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(ProxyObject::from_value(&call.argument(0)).map_or(JSValue::undefined(), |proxy| proxy.handler()))
}
host_function!(proxy_handler, proxy_handler_body);

/// `$getInternalField(objeto, índice)`: o campo interno de uma célula `JSInternalFieldObjectImpl` (o iterador de `Map` e
/// de `Set`, no `previewEntries`).
fn get_internal_field_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let JSValue::Cell(cell_id) = call.argument(0) else { return Ok(JSValue::undefined()) };
    let index = call.argument(1).to_int32();
    let Some(entry) = cell_registry::get(cell_id) else { return Ok(JSValue::undefined()) };
    Ok(entry
        .internal_fields()
        .filter(|fields| u32::try_from(index).is_ok_and(|index| index < fields.field_count()))
        .map_or(JSValue::undefined(), |fields| fields.field(index as u32)))
}
host_function!(get_internal_field, get_internal_field_body);

/// `jsFunctionIsDeepStrictEqual` (`NodeUtilTypesModule.cpp`): a semântica do Node, com a identidade do protótipo que o
/// `Bun.deepEquals` não checa; o terceiro argumento verdadeiro é o `kStrictWithoutPrototypes` do Node.
fn is_deep_strict_equal_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let mode = Mode { strict: true, skip_prototype_identity: call.argument(2).to_boolean(), check_prototypes: true };
    Ok(js_boolean(deep_equals(global_object, mode, call.argument(0), call.argument(1))?))
}
host_function!(pub is_deep_strict_equal, is_deep_strict_equal_body);

/// `$isJSArray(valor)`: `ArrayType` ou `DerivedArrayType`, sem olhar o `Proxy`.
fn is_js_array_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let JSValue::Cell(cell_id) = call.argument(0) else { return Ok(js_boolean(false)) };
    Ok(js_boolean(matches!(cell_registry::cell_type(cell_id), Some(JSType::ArrayType | JSType::DerivedArrayType))))
}
host_function!(is_js_array, is_js_array_body);

fn is_error_instance(value: JSValue) -> bool {
    matches!(value, JSValue::Cell(cell_id) if cell_registry::cell_type(cell_id) == Some(JSType::ErrorInstanceType))
}

/// `jsFunctionIsError(valor)`: instância de erro, ou `Symbol.toStringTag` igual a `"Error"`, ou protótipo que é instância
/// de erro ou `Error.prototype` (o `util.isError` do Node, que lê o `toString`).
fn is_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    if !matches!(value, JSValue::Cell(_)) {
        return Ok(js_boolean(false));
    }
    if is_error_instance(value) {
        return Ok(js_boolean(true));
    }
    let vm = global_object.vm();
    let Some(object) = value.to_object(global_object) else { return Err(Thrown::Pending) };
    let mut slot = PropertySlot::new(object.as_value(), InternalMethodType::VMInquiry);
    let tag = PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol);
    if crate::runtime::proxy_object::get_property_slot(global_object, object.as_value(), &tag, &mut slot)? && slot.is_value() {
        let tag_value = slot.get_value();
        if tag_value.is_string() && tag_value.to_wtf_string() == WtfString::from_latin1(b"Error") {
            return Ok(js_boolean(true));
        }
    }
    let prototype = object.get_prototype(global_object)?;
    let JSValue::Cell(cell_id) = prototype else { return Ok(js_boolean(false)) };
    let entry = cell_registry::get(cell_id);
    let inherits_error_prototype =
        entry.as_ref().and_then(|entry| entry.as_js_object()).is_some_and(|prototype| prototype.cell().inherits(&ERROR_PROTOTYPE_WITH_TABLE_S_INFO));
    Ok(js_boolean(is_error_instance(prototype) || inherits_error_prototype))
}
host_function!(is_error, is_error_body);

/// `parseEnv(conteúdo)`: o `.env` como objeto. A chave que é índice (`0=`, `2023=`) entra como índice.
fn parse_env_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let content = call.argument(0);
    if !content.is_string() && !crate::runtime::console_client::is_string_object(content) {
        let message = format!("The \"content\" property must be of type string, got {}", typeof_for_property_error(content));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    let source = pending_or(global_object, content.to_wtf_string())?;
    let entries = crate::runtime::dotenv::parse(&source.utf8(ConversionMode::LenientConversion));
    let vm = global_object.vm();
    let object = construct_empty_object(global_object);
    for (key, value) in &entries {
        let name = PropertyName::from_identifier(&Identifier::from_span(vm, key));
        let value = JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(value)));
        put_own_data_property_may_be_index(vm, &object, &name, value)?;
    }
    Ok(object.as_value())
}
host_function!(parse_env, parse_env_body);

/// O nome em `builtin_modules::cached` dos internos que o `util` entrega (`primordials`, `colors`, `inspect`,
/// `deprecate`).
const UTIL_INTERNALS: &str = "internal:util";

/// `host.registerInternals(objeto)`: o `util` entrega ao hospedeiro as instâncias dos internos dele.
fn register_internals_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let internals = call.argument(0);
    crate::api::builtin_modules::cached(UTIL_INTERNALS, || Ok(internals))?;
    Ok(JSValue::undefined())
}
host_function!(register_internals, register_internals_body);

/// `host.utilInternal(nome)`: o interno `nome` do `util` (o mesmo objeto, como no registro único do bun), carregando o
/// `util` se ainda não foi carregado.
fn util_internal_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let internals = crate::api::builtin_modules::registered_by(global_object, "util", UTIL_INTERNALS, || Thrown::type_error("util internals"))?;
    let name = PropertyName::from_identifier(&Identifier::from_string(global_object.vm(), &call.argument(0).to_wtf_string()));
    pending_or(global_object, internals.as_object().get(global_object, &name))
}
host_function!(pub util_internal, util_internal_body);

/// Instalador de `node:util`.
pub(crate) fn install_util_module(global_object: &JSGlobalObject) -> HostResult {
    let host = module_host(
        global_object,
        &[
            ("internalErrorName", 1, internal_error_name),
            ("internalErrorEntries", 0, internal_error_entries),
            ("splitNewLines", 1, split_new_lines),
            ("getOwnNonIndexProperties", 2, get_own_non_index_properties),
            ("promiseStatus", 1, promise_status),
            ("promiseResult", 1, promise_result),
            ("isPromise", 1, is_promise),
            ("newPromise", 0, new_promise),
            ("resolvePromise", 2, resolve_promise),
            ("isProxyObject", 1, is_proxy_object),
            ("proxyTarget", 1, proxy_target),
            ("proxyHandler", 1, proxy_handler),
            ("getInternalField", 2, get_internal_field),
            ("jsFunctionIsDeepStrictEqual", 3, is_deep_strict_equal),
            ("isJSArray", 1, is_js_array),
            ("jsFunctionIsError", 1, is_error),
            ("parseEnv", 1, parse_env),
            ("registerInternals", 1, register_internals),
            ("parseArgs", 1, crate::api::node_parse_args::parse_args),
            ("requireInternal", 1, crate::api::node_stream::require_internal),
            ("stringWidth", 1, crate::runtime::bun_global::bun_string_width),
        ],
    )?;
    let vm = global_object.vm();
    let (mime_type, mime_params) = crate::runtime::mime::install_mime_classes(global_object);
    host.put_direct(vm, &property_key(vm, "MIMEType"), mime_type, 0);
    host.put_direct(vm, &property_key(vm, "MIMEParams"), mime_params, 0);
    Ok(run_factory(global_object, SOURCE, "util", &host))
}
