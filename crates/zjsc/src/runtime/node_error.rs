//! Os erros com `code` que as classes nativas do bun lançam (`ERR_ILLEGAL_CONSTRUCTOR`, `ERR_INVALID_THIS`,
//! `ERR_INVALID_ARG_TYPE`, `ERR_INVALID_ARG_VALUE`, `ERR_ENCODING_NOT_SUPPORTED`...), num lugar só. Medido no
//! bun 1.4.2 para `TextEncoder`, `TextDecoder`, `ResolveMessage`, `import.meta.resolveSync` e `structuredClone`:
//!
//! - o erro é um `TypeError` (ou `RangeError`) cujo protótipo NÃO é `TypeError.prototype`, e sim um objeto
//!   próprio por código, que herda de `TypeError.prototype` e tem, nesta ordem, `name` ("TypeError" ou
//!   "RangeError"), `code` e `toString` (`length` 0, nativo), todos graváveis, enumeráveis e configuráveis. O
//!   protótipo é o mesmo para erros do mesmo código, venham de onde vierem; `constructor` e `instanceof`
//!   continuam os do `TypeError`;
//! - o `toString` desse protótipo escreve `` `${this.name} [${this.code}]: ${this.message}` `` sem tratar
//!   mensagem vazia nem `this` alheio (`undefined [undefined]: undefined`), então `String(e)` dá
//!   `TypeError [ERR_X]: mensagem`; o cabeçalho de `e.stack` é o comum (`TypeError: mensagem`);
//! - as chaves próprias do erro são `message`, `originalLine`, `originalColumn`, `line`, `column`, `sourceURL` e
//!   `stack`, todas não enumeráveis (o `code` não é própria): a pilha é a do `ErrorInstance` com o frame
//!   nativo no topo (`at TextEncoder (unknown)`), e o resto vem de `ErrorInstance::materialize_stack`;
//! - a exceção a isso é `ERR_MISSING_ARGS` ("Not enough arguments"): vem pelo caminho comum do WebCore, com o protótipo
//!   do próprio `TypeError` e o `code` como propriedade própria enumerável, logo depois de `message`.
//!
//! DIVERGÊNCIAS:
//!
//! - o `originalLine` e o `originalColumn` do bun são a posição no fonte transpilado, que aqui não existe;
//!   valem a posição do frame de cima, como em `ErrorInstance::materialize_stack`.

use std::cell::RefCell;

use crate::host_function;
use crate::interpreter::call_frame::CallFrame;
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::error_natives::{capture_frames, put_message_property};
use crate::runtime::error_type::ErrorType;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSFinalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_class_support::property_key;
use crate::runtime::structure::StructureRef;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::wtf::text::string_concatenate::make_string_dyn;
use crate::wtf::text::wtf_string::String as WtfString;

/// O único código que pode levar `code` próprio e o protótipo comum (o caminho "argumentos faltando" do WebCore).
const OWN_CODE: &str = "ERR_MISSING_ARGS";

thread_local! {
    /// As `Structure`s dos erros por (realm, tipo, código); o protótipo do código vive na `Structure`.
    static STRUCTURES: RefCell<Vec<(usize, ErrorType, String, StructureRef)>> = const { RefCell::new(Vec::new()) };
}

/// Fim do programa (`cell_registry::reset_program_state`): as estruturas guardadas são do programa.
pub(crate) fn reset_for_program() {
    let _ = STRUCTURES.try_with(|structures| structures.borrow_mut().clear());
}

fn text_value(global_object: &JSGlobalObject, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(text.as_bytes())))
}

/// O `toString` do protótipo de um código: `${name} [${code}]: ${message}`.
fn coded_error_to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = call.this_value();
    let vm = global_object.vm();
    let mut parts = Vec::with_capacity(3);
    for name in ["name", "code", "message"] {
        let value = get_value_property(global_object, this, &property_key(vm, name))?;
        parts.push(pending_or(global_object, value.to_wtf_string())?);
    }
    let text = make_string_dyn(&[&parts[0], &" [", &parts[1], &"]: ", &parts[2]]);
    Ok(JSValue::from_js_string(js_string(vm, &text)))
}

host_function!(coded_error_to_string, coded_error_to_string_body);

/// A `Structure` do erro de `kind` com `code`: protótipo próprio (`name`, `code`, `toString`) sobre o
/// `prototype` do tipo, uma só por realm.
fn coded_structure(global_object: &JSGlobalObject, kind: ErrorType, code: &str) -> StructureRef {
    let realm = global_object.cell_id();
    let known = STRUCTURES.with(|structures| {
        structures.borrow().iter().find(|(id, known_kind, known_code, _)| *id == realm && *known_kind == kind && known_code == code).map(|entry| entry.3.clone())
    });
    if let Some(structure) = known {
        return structure;
    }
    let vm = global_object.vm();
    let parent = global_object.error_structure_for(kind).stored_prototype();
    let prototype_structure = JSFinalObject::create_structure(vm, Some(global_object), parent, JSFinalObject::DEFAULT_INLINE_CAPACITY);
    let prototype = JSFinalObject::create(vm, &prototype_structure);
    let kind_name = crate::runtime::error_type::error_type_name(kind);
    prototype.put_direct(vm, &property_key(vm, "name"), text_value(global_object, kind_name), 0);
    prototype.put_direct(vm, &property_key(vm, "code"), text_value(global_object, code), 0);
    put_direct_native_function_without_transition(
        vm,
        global_object,
        &prototype,
        &Identifier::from_span(vm, b"toString"),
        0,
        coded_error_to_string,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        0,
    );
    prototype.did_become_prototype(vm);
    let structure = ErrorInstance::create_structure(vm, Some(global_object), prototype.as_value());
    STRUCTURES.with(|structures| structures.borrow_mut().push((realm, kind, code.to_owned(), structure.clone())));
    structure
}

/// Lança o erro de `kind` com `code` e devolve `Thrown::Pending`. A pilha nasce aqui, com o frame da função
/// nativa que lança no topo (`vm.topCallFrame`), e as chaves `originalLine`...`stack` entram quando ela é lida.
pub(crate) fn throw_coded(global_object: &JSGlobalObject, kind: ErrorType, message: &str, code: Option<&str>) -> Thrown {
    throw_coded_message(global_object, kind, WtfString::from_utf8(message.as_bytes()), code, &[])
}

/// Lança um `Error` simples com `code` e a mensagem já em UTF-16 (surrogate solto sobrevive), como o bun, e devolve
/// `Thrown::Pending`.
pub(crate) fn throw_coded_error_with_message(global_object: &JSGlobalObject, message: WtfString, code: &str) -> Thrown {
    throw_coded_message(global_object, ErrorType::Error, message, Some(code), &[])
}

/// Valor de uma propriedade própria de `throw_network_error`.
pub(crate) enum NetworkProperty<'a> {
    Text(&'a str),
    Number(i32),
    /// Um objeto simples com as propriedades dadas, na ordem (o `info` dos `SystemError` do `node:os`).
    Object(&'a [(&'a str, NetworkProperty<'a>)]),
}

/// O valor JS de uma propriedade de erro; o `Object` vira um objeto comum com propriedades graváveis e enumeráveis.
fn network_property_value(global_object: &JSGlobalObject, property: &NetworkProperty) -> JSValue {
    match property {
        NetworkProperty::Text(text) => text_value(global_object, text),
        NetworkProperty::Number(number) => crate::runtime::js_value::js_number(*number),
        NetworkProperty::Object(entries) => {
            let vm = global_object.vm();
            let object = crate::runtime::object_constructor::construct_empty_object(global_object);
            for (name, value) in entries.iter() {
                object.put_direct(vm, &property_key(vm, name), network_property_value(global_object, value), 0);
            }
            object.as_value()
        }
    }
}

/// Lança o `TypeError` de rede do `fetch` do bun (`ConnectionRefused`, `FailedToOpenSocket`, `ETIMEOUT`...) e devolve
/// `Thrown::Pending`. Medido no bun 1.4.2: protótipo do próprio `TypeError`, `message` não enumerável e, depois dela, as
/// propriedades de `properties` (`code`, `path`, `syscall`, `hostname`, `errno`, nesta ordem) próprias, graváveis,
/// enumeráveis e não configuráveis, com o resto das chaves de pilha como nos demais erros nativos.
pub(crate) fn throw_network_error(global_object: &JSGlobalObject, message: &str, properties: &[(&str, NetworkProperty)]) -> Thrown {
    throw_coded_message(global_object, ErrorType::TypeError, WtfString::from_utf8(message.as_bytes()), None, properties)
}

/// `uv_err_name` e `uv_strerror` de um `errno` positivo (a tabela do `ERRORS` de `node_fs.js`).
pub(crate) fn uv_error_info(errno: i32) -> (&'static str, &'static str) {
    match errno {
        1 => ("EPERM", "operation not permitted"),
        2 => ("ENOENT", "no such file or directory"),
        3 => ("ESRCH", "no such process"),
        4 => ("EINTR", "interrupted system call"),
        5 => ("EIO", "i/o error"),
        6 => ("ENXIO", "no such device or address"),
        7 => ("E2BIG", "argument list too long"),
        9 => ("EBADF", "bad file descriptor"),
        11 => ("EAGAIN", "resource temporarily unavailable"),
        12 => ("ENOMEM", "not enough memory"),
        13 => ("EACCES", "permission denied"),
        14 => ("EFAULT", "bad address in system call argument"),
        16 => ("EBUSY", "resource busy or locked"),
        17 => ("EEXIST", "file already exists"),
        18 => ("EXDEV", "cross-device link not permitted"),
        19 => ("ENODEV", "no such device"),
        20 => ("ENOTDIR", "not a directory"),
        21 => ("EISDIR", "illegal operation on a directory"),
        22 => ("EINVAL", "invalid argument"),
        23 => ("ENFILE", "file table overflow"),
        24 => ("EMFILE", "too many open files"),
        25 => ("ENOTTY", "inappropriate ioctl for device"),
        26 => ("ETXTBSY", "text file is busy"),
        27 => ("EFBIG", "file too large"),
        28 => ("ENOSPC", "no space left on device"),
        29 => ("ESPIPE", "invalid seek"),
        30 => ("EROFS", "read-only file system"),
        31 => ("EMLINK", "too many links"),
        32 => ("EPIPE", "broken pipe"),
        34 => ("ERANGE", "result too large"),
        36 => ("ENAMETOOLONG", "name too long"),
        38 => ("ENOSYS", "function not implemented"),
        39 => ("ENOTEMPTY", "directory not empty"),
        40 => ("ELOOP", "too many symbolic links encountered"),
        95 => ("ENOTSUP", "operation not supported on socket"),
        _ => ("UNKNOWN", "unknown error"),
    }
}

/// Lança um `Error` simples sem `code` próprio de estrutura, com as propriedades próprias de `properties` (erro de
/// sistema: `errno`, `code`, `syscall`, `path`), e devolve `Thrown::Pending`.
pub(crate) fn throw_error_with_properties(global_object: &JSGlobalObject, message: &str, properties: &[(&str, NetworkProperty)]) -> Thrown {
    throw_coded_message(global_object, ErrorType::Error, WtfString::from_utf8(message.as_bytes()), None, properties)
}

fn throw_coded_message(global_object: &JSGlobalObject, kind: ErrorType, message: WtfString, code: Option<&str>, own: &[(&str, NetworkProperty)]) -> Thrown {
    throw_coded_message_with_attributes(global_object, kind, message, code, own, &locked_attributes)
}

/// Atributos das propriedades próprias dos erros de rede e de sistema do `chdir`: não configuráveis.
fn locked_attributes(_name: &str) -> u32 {
    crate::runtime::property_attribute::DONT_DELETE
}

/// Lança o `SystemError` de uma chamada de sistema do bun (`kill() failed: ESRCH: No such process`) e devolve
/// `Thrown::Pending`. Medido no bun 1.4.2: `message`, `syscall`, `errno` (positivo), `name` e `code`, nesta ordem, todas
/// próprias, graváveis e configuráveis; só `syscall`, `errno` e `code` enumeráveis (`name` e `message` não).
pub(crate) fn throw_system_error(global_object: &JSGlobalObject, syscall: &str, errno: i32, code: &str, description: &str) -> Thrown {
    let message = format!("{syscall}() failed: {code}: {description}");
    let properties = [
        ("syscall", NetworkProperty::Text(syscall)),
        ("errno", NetworkProperty::Number(errno)),
        ("name", NetworkProperty::Text("SystemError")),
        ("code", NetworkProperty::Text(code)),
    ];
    let attributes = |name: &str| if name == "name" { crate::runtime::property_attribute::DONT_ENUM } else { crate::runtime::property_attribute::NONE };
    throw_coded_message_with_attributes(global_object, ErrorType::Error, WtfString::from_utf8(message.as_bytes()), None, &properties, &attributes)
}

fn throw_coded_message_with_attributes(
    global_object: &JSGlobalObject,
    kind: ErrorType,
    message: WtfString,
    code: Option<&str>,
    own: &[(&str, NetworkProperty)],
    attributes: &dyn Fn(&str) -> u32,
) -> Thrown {
    throw_coded_message_in_frame(global_object, kind, message, code, own, attributes, true)
}

/// Lança o erro de validação de `process.exit`: `code` próprio e enumerável, e a pilha sem o frame nativo `exit`
/// (medido no bun 1.4.2: o relato de erro não capturado mostra ` code: "ERR_..."` e só o frame do chamador).
pub(crate) fn throw_validation_error(global_object: &JSGlobalObject, kind: ErrorType, message: &str, code: &str) -> Thrown {
    let own = [("code", NetworkProperty::Text(code))];
    throw_coded_message_in_frame(global_object, kind, WtfString::from_utf8(message.as_bytes()), None, &own, &locked_attributes, false)
}

fn throw_coded_message_in_frame(
    global_object: &JSGlobalObject,
    kind: ErrorType,
    message: WtfString,
    code: Option<&str>,
    own: &[(&str, NetworkProperty)],
    attributes: &dyn Fn(&str) -> u32,
    include_top_native: bool,
) -> Thrown {
    let vm = global_object.vm();
    // Só o `createNotEnoughArgumentsError` do WebCore põe `code` próprio, e a mensagem dele é exatamente esta; o
    // `ERR_MISSING_ARGS` do bun (`domainToASCII()`, `Bun.sleepSync()`, `URL.createObjectURL()`) tem o código no
    // protótipo (medido no bun 1.4.2 com `Object.hasOwn(erro, "code")`).
    let own_code = code == Some(OWN_CODE) && crate::runtime::js_module_loader::rust_string(&message) == "Not enough arguments";
    let structure = match code {
        Some(code) if !own_code => coded_structure(global_object, kind, code),
        _ => global_object.error_structure_for(kind),
    };
    let instance = ErrorInstance::create(vm, structure, message.clone(), kind);
    if !message.is_empty() {
        put_message_property(vm, &instance, &message);
    }
    for (name, value) in own {
        let value = network_property_value(global_object, value);
        instance.as_object().put_direct(vm, &property_key(vm, name), value, attributes(name));
    }
    if own_code {
        instance.as_object().put_direct(vm, &property_key(vm, "code"), text_value(global_object, OWN_CODE), 0);
    }
    let top = vm.top_call_frame();
    if top != 0 {
        if let Some(frames) = capture_frames(global_object, CallFrame::create(top), None, include_top_native) {
            instance.set_pending_stack(frames);
        }
    }
    let mut scope = ThrowScope::new(vm);
    throw_exception(global_object, &mut scope, instance.as_value());
    Thrown::Pending
}

/// Lança um `TypeError` com `code`, como o bun, e devolve `Thrown::Pending`.
pub(crate) fn throw_coded_type_error(global_object: &JSGlobalObject, message: &str, code: &str) -> Thrown {
    throw_coded(global_object, ErrorType::TypeError, message, Some(code))
}

/// Lança um `Error` simples com `code` (`ERR_DLOPEN_FAILED` de `require.extensions['.node']`), como o bun, e devolve
/// `Thrown::Pending`.
pub(crate) fn throw_coded_error(global_object: &JSGlobalObject, message: &str, code: &str) -> Thrown {
    throw_coded(global_object, ErrorType::Error, message, Some(code))
}

/// Lança um `Error` simples sem `code` (`Cannot write to a detached Blob` do bun) e devolve `Thrown::Pending`.
pub(crate) fn throw_plain_error(global_object: &JSGlobalObject, message: &str) -> Thrown {
    throw_coded(global_object, ErrorType::Error, message, None)
}

/// Lança um `RangeError` com `code`, como o bun, e devolve `Thrown::Pending`.
pub(crate) fn throw_coded_range_error(global_object: &JSGlobalObject, message: &str, code: &str) -> Thrown {
    throw_coded(global_object, ErrorType::RangeError, message, Some(code))
}

/// Lança um `SyntaxError` nativo sem `code` (a rejeição `SyntaxError` do WebCrypto do bun, que não é `DOMException`)
/// e devolve `Thrown::Pending`.
pub(crate) fn throw_native_syntax_error(global_object: &JSGlobalObject, message: &str) -> Thrown {
    throw_coded(global_object, ErrorType::SyntaxError, message, None)
}

/// Lança um `TypeError` nativo sem `code` (getter em objeto alheio, `Expected Uint8Array`, `atob()` sem argumento...)
/// e devolve `Thrown::Pending`. Medido no bun 1.4.2: protótipo do próprio `TypeError`, chaves próprias `message`,
/// `originalLine`, `originalColumn`, `line`, `column`, `sourceURL` e `stack` (não enumeráveis), com o frame da função
/// nativa no topo da pilha (`at get encoding (unknown)`), como nos erros com `code`.
pub(crate) fn throw_native_type_error(global_object: &JSGlobalObject, message: &str) -> Thrown {
    throw_coded(global_object, ErrorType::TypeError, message, None)
}
