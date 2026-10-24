//! O que os módulos embutidos escritos em JavaScript (`events`, `stream`) pedem ao hospedeiro: o objeto `host` com as
//! funções nativas que o JavaScript não faz sozinho (criar o erro com `code` no formato do bun, o `util.inspect`, o
//! `require` de outro embutido), o `node_internal.js` (erros, validadores e afins, montado uma vez por programa) e a
//! criação da fábrica do módulo como o executável builtin que o bun cria (`createBuiltinExecutable`), chamada com o `host`.

use crate::api::builtin_modules::{cached, load, resolve, Resolved};
use crate::api::eval::{describe_exception, js_text, key_of, latin1_program_source};
use crate::bytecode::executable_info::NeedsClassFieldInitializer;
use crate::bytecode::unlinked_function_executable::UnlinkedFunctionExecutable;
use crate::host_function;
use crate::parser::parser_modes::PrivateBrandRequirement;
use crate::runtime::builtin_executables::BuiltinExecutables;
use crate::runtime::construct_ability::ConstructAbility;
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::current_realm::current_global_object;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::inline_attribute::InlineAttribute;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::JSFunction;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::aggregate_error::get_if_property_exists;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_object::{JSFinalObject, JSObject};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::js_microtask::call_microtask;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_object::JSObjectRef;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::error_type::ErrorType;
use crate::runtime::node_error::{throw_coded, throw_coded_error};
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::timers::native_function;
use crate::runtime::util_inspect::inspect_with_options;

const INTERNAL_SOURCE: &str = include_str!("node_internal.js");

/// `host.throwNodeError(tipo, código, mensagem)`: lança o erro com `code` que o JavaScript captura para devolver como
/// valor (o `$ERR_*` do bun).
fn throw_node_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let kind = rust_string(&call.argument(0).to_wtf_string());
    let code = rust_string(&call.argument(1).to_wtf_string());
    let message = rust_string(&call.argument(2).to_wtf_string());
    let kind = match kind.as_str() {
        "TypeError" => ErrorType::TypeError,
        "RangeError" => ErrorType::RangeError,
        "URIError" => ErrorType::URIError,
        _ => ErrorType::Error,
    };
    Err(throw_coded(global_object, kind, &message, Some(&code)))
}
host_function!(throw_node_error, throw_node_error_body);

/// `host.inspect(valor, opções)`: o `util.inspect`.
fn inspect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let text = inspect_with_options(global_object, call.argument(0), call.argument(1))?;
    Ok(js_text(global_object.vm(), &text))
}
host_function!(inspect, inspect_body);

/// `host.requireBuiltin(nome)`: o `require` de outro módulo embutido.
fn require_builtin_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = rust_string(&call.argument(0).to_wtf_string());
    match resolve(&name) {
        Resolved::Builtin(entry) => load(global_object, entry),
        _ => Err(throw_coded_error(global_object, &format!("Cannot find module '{name}'"), "MODULE_NOT_FOUND")),
    }
}
host_function!(require_builtin, require_builtin_body);

/// `host.toClass(alvo, nome, base)`: o `$toClass` do bun (`jsFunctionToClass`), que imita `class Foo extends Base {}`
/// numa função comum. Uma função de módulo embutido nasce sem `prototype`, então é este o ponto que cria o dela: um
/// objeto novo (filho do `prototype` da `base`) com `constructor`, mais `name` e o `prototype`, todos não enumeráveis.
fn to_class_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let Some(target) = call.argument(0).to_object(global_object) else { return Err(Thrown::Pending) };
    let name = call.argument(1);
    let (base, prototype_base) = match ObjectRef::from_value(&call.argument(2)) {
        None => (global_object.function_prototype().as_value(), global_object.object_prototype().as_value()),
        Some(base) => {
            let holder: &JSObject = &base;
            let Some(prototype) = get_if_property_exists(global_object, holder, &PropertyName::from_identifier(&vm.property_names.prototype))? else {
                return Err(Thrown::type_error("Base class must have a prototype property"));
            };
            (base.as_value(), if prototype.is_object() { prototype } else { global_object.object_prototype().as_value() })
        }
    };
    let structure = JSFinalObject::create_structure(vm, Some(global_object), prototype_base, JSFinalObject::DEFAULT_INLINE_CAPACITY);
    let prototype = JSFinalObject::create(vm, &structure);
    prototype.put_direct(vm, &key_of(vm, "constructor"), target.as_value(), DONT_ENUM);
    target.set_prototype_direct(vm, base);
    target.put_direct(vm, &key_of(vm, "prototype"), prototype.as_value(), DONT_ENUM);
    target.put_direct(vm, &key_of(vm, "name"), name, DONT_ENUM);
    Ok(JSValue::undefined())
}
host_function!(to_class, to_class_body);

/// Põe as funções `members` (nome, `length`, corpo) em `host`.
pub(crate) fn put_functions(global_object: &JSGlobalObject, host: &JSObjectRef, members: &[(&str, u32, NativeFunction)]) {
    let vm = global_object.vm();
    for (name, length, function) in members {
        host.put_direct(vm, &key_of(vm, name), native_function(global_object, name, *length, *function).as_value(), 0);
    }
}

/// O `host` sem o `internal`: o que o próprio `node_internal.js` recebe.
fn base_host(global_object: &JSGlobalObject) -> JSObjectRef {
    let host = construct_empty_object(global_object);
    put_functions(
        global_object,
        &host,
        &[("throwNodeError", 3, throw_node_error), ("inspect", 2, inspect), ("requireBuiltin", 1, require_builtin), ("toClass", 3, to_class)],
    );
    host
}

/// A fábrica do módulo `node:<name>` como o bun cria um módulo embutido (`InternalModuleRegistry::createInternalModuleById`):
/// o texto `(function (host) { ... })` vira um executável builtin (`createBuiltinExecutable`, `CannotConstruct`), ligado ao
/// global, e todas as funções aninhadas herdam o modo builtin. É isso que faz uma função do módulo nascer sem `prototype`
/// próprio (só o ganha quem o atribui, como `EventEmitter.prototype = {}`) e mostrar o fonte no `Function.prototype.toString`
/// (a função builtin com `sourceURL` mostra o texto, ver `FunctionExecutable::is_private_builtin_function`).
fn module_factory(realm: &JSGlobalObject, source: &str, name: &str, visibility: ImplementationVisibility) -> JSValue {
    // O que vem antes do `(function (` é o cabeçalho de comentário do arquivo gerado; o bun começa o fonte na fábrica.
    let text = if source.starts_with("(function (") { source } else { source.find("\n(function (").map_or(source, |at| &source[at + 1..]) };
    let latin1: Vec<u8> = text
        .chars()
        .map(|character| u8::try_from(u32::from(character)).unwrap_or_else(|_| panic!("node:{name}: o fonte embutido tem caractere fora do Latin1: escreva o escape \\u{{...}}")))
        .collect();
    let code = latin1_program_source(&latin1, &format!("node:{name}"));
    let vm = realm.vm();
    let unlinked = BuiltinExecutables::create_executable(
        vm,
        &code,
        &Identifier::from_span(vm, name.as_bytes()),
        visibility,
        ConstructorKind::None,
        ConstructAbility::CannotConstruct,
        InlineAttribute::None,
        NeedsClassFieldInitializer::No,
        PrivateBrandRequirement::None,
    );
    let executable = UnlinkedFunctionExecutable::link(&unlinked, vm, None, &code, None, Intrinsic::NoIntrinsic, false);
    JSFunction::create(vm, realm, &executable, realm.global_scope()).as_value()
}

/// O `stack` de um valor lançado (vazio se não for um erro com pilha).
fn error_stack(global_object: &JSGlobalObject, value: JSValue) -> String {
    let identifier = crate::runtime::identifier::Identifier::from_span(global_object.vm(), b"stack");
    crate::runtime::string_regexp_support::get_object_property(global_object, value, &identifier)
        .map(|stack| crate::runtime::js_module_loader::rust_string(&stack.to_wtf_string()))
        .unwrap_or_default()
}

/// Cria a fábrica do módulo `node:<name>` e a chama com `host`. Falhar aqui é defeito do fonte embutido, não do programa
/// do usuário.
pub(crate) fn run_factory(global_object: &JSGlobalObject, source: &str, name: &str, host: &JSObjectRef) -> JSValue {
    run_factory_with(global_object, source, name, host, ImplementationVisibility::Public)
}

/// `run_factory` para o programa que no bun é código nativo (`Bun.serve`, `Bun.write`, `ReadableStream.values`...): a
/// fábrica e tudo o que ela contém são `PrivateRecursive`, então nenhum frame dele aparece na pilha de um erro e o
/// `toString` das funções dele mostra `[native code]`.
pub(crate) fn run_native_factory(global_object: &JSGlobalObject, source: &str, name: &str, host: &JSObjectRef) -> JSValue {
    run_factory_with(global_object, source, name, host, ImplementationVisibility::PrivateRecursive)
}

fn run_factory_with(global_object: &JSGlobalObject, source: &str, name: &str, host: &JSObjectRef, visibility: ImplementationVisibility) -> JSValue {
    let factory = module_factory(&current_global_object(), source, name, visibility);
    call_microtask(global_object, factory, JSValue::undefined(), &[host.as_value()], "o módulo embutido não é uma função").unwrap_or_else(|thrown| {
        let detail = match &thrown {
            crate::runtime::js_promise_host::Thrown::Value(value) => describe_exception(value),
            _ => global_object.vm().exception().map_or_else(|| format!("{thrown:?}"), |exception| describe_exception(&exception.value())),
        };
        // Defeito do fonte embutido: a pilha do erro (as linhas do texto gerado) acompanha a mensagem.
        let stack = match &thrown {
            crate::runtime::js_promise_host::Thrown::Value(value) => error_stack(global_object, *value),
            _ => global_object.vm().exception().map_or_else(String::new, |exception| error_stack(global_object, exception.value())),
        };
        panic!("node:{name}: a fábrica lançou {detail}\n{stack}")
    })
}

/// Chama a função JavaScript `function` com `this` e `arguments`; a exceção fica pendente (`Thrown::Pending`) e uma
/// falha do próprio motor vira `Thrown::Unported(who)`.
pub(crate) fn call_value(global_object: &JSGlobalObject, function: JSValue, this_value: JSValue, arguments: &[JSValue], who: &'static str) -> HostResult {
    let data = crate::runtime::call_data::get_call_data(function);
    match crate::runtime::call_data::call(global_object, function, &data, this_value, arguments) {
        Ok(value) => Ok(value),
        Err(crate::llint::LLIntFailure::Thrown) => Err(Thrown::Pending),
        Err(_) => Err(Thrown::Unported(who)),
    }
}

/// O `node_internal.js`: erros com código, validadores, `addAbortListener` e afins, uma vez por programa.
pub(crate) fn internal(global_object: &JSGlobalObject) -> HostResult {
    cached("internal:node", || Ok(run_factory(global_object, INTERNAL_SOURCE, "internal", &base_host(global_object))))
}

/// O `host` completo de um módulo: o básico mais o `internal` e as funções próprias do módulo.
pub(crate) fn module_host(global_object: &JSGlobalObject, extra: &[(&str, u32, NativeFunction)]) -> Result<JSObjectRef, crate::runtime::host_call::Thrown> {
    let host = base_host(global_object);
    let vm = global_object.vm();
    host.put_direct(vm, &key_of(vm, "internal"), internal(global_object)?, 0);
    put_functions(global_object, &host, extra);
    Ok(host)
}

/// Instala o módulo `node:<name>` escrito em JavaScript que só precisa do `host` básico com o `internal`.
pub(crate) fn install_js_module(global_object: &JSGlobalObject, source: &str, name: &str) -> HostResult {
    let host = module_host(global_object, &[])?;
    Ok(run_factory(global_object, source, name, &host))
}
