//! `DOMException` do global. O JavaScriptCore não o define: quem o instala é o bun (WebCore), como propriedade
//! de dados comum (`writable`, `enumerable`, `configurable`). Medido no bun 1.4.2:
//!
//! - o construtor é nativo (`[native code]`), `length` 0, `name` "DOMException", com `length`, `name`,
//!   `prototype` e depois as 25 constantes legadas (`INDEX_SIZE_ERR`...`DATA_CLONE_ERR`, valor 1 a 25,
//!   enumeráveis, não graváveis, não configuráveis) como propriedades próprias; o protótipo do construtor é
//!   `Function.prototype`;
//! - `DOMException.prototype` herda de `Error.prototype` e tem, nesta ordem: `constructor` (não enumerável),
//!   os acessores `code`, `name` e `message` (enumeráveis, configuráveis, sem setter, getters nativos de nome
//!   `get code` e `toString()` `function code() { [native code] }`), as mesmas 25 constantes e
//!   `@@toStringTag` "DOMException" (não gravável, não enumerável);
//! - `new DOMException(message = "", options = "Error")`: `message` e `name` passam por `ToString`; `options`
//!   objeto (função e array inclusos) dá `name` pela propriedade `name` (indefinida vira "Error") e, se a
//!   propriedade `cause` existir (mesmo indefinida), um `cause` próprio (gravável, não enumerável); `name`
//!   é lido antes de `cause`. A instância não tem propriedade própria alguma (nem `stack`);
//! - o `code` sai da tabela dos nomes legados (`IndexSizeError` 1, `HierarchyRequestError` 3, ...); nome
//!   desconhecido, e também `DOMStringSizeError`, `NoDataAllowedError` e `ValidationError`, dá 0;
//! - sem `new`: `TypeError: Use `new DOMException(...)` instead of `DOMException(...)``; getter com `this`
//!   que não é `DOMException`: `TypeError: The DOMException.<campo> getter can only be used on instances of
//!   DOMException`; subclasse e `Reflect.construct` com `newTarget` diferente usam o `prototype` dele.
//!
//! DIVERGÊNCIAS:
//!
//! - `e.name = "x"` (e `code`, `message`) é ignorado em sloppy e lança `TypeError: Attempted to assign to
//!   readonly property.` em strict (acessor sem setter). A medida antiga de que o bun lançava também em sloppy
//!   vinha de um arquivo do bun, que é módulo e portanto strict; por indirect eval o bun ignora, como aqui;
//! - um `DOMException` lançado por função nativa do bun (`atob`, `btoa`, `structuredClone`) ganha as
//!   propriedades próprias `line` e `column` (enumeráveis), `sourceURL` e `stack` (ver
//!   `throw_dom_exception_from_host`). `throw_dom_exception` (sem `HostCall`) não as grava: `structuredClone`
//!   ainda o usa e diverge até migrar. A `stack` está no formato do JavaScriptCore, mas o frame da função
//!   nativa `eval` não aparece no percurso do porte (o bun mostra `eval@[native code]`);
//! - a propriedade global entra no fim da ordem de chaves, junto de `ResolveMessage` e `BuildMessage` (no bun
//!   vem depois delas e de `atob`/`btoa`, antes de `ErrorEvent`; a ordem relativa a essas três bate).

use std::cell::RefCell;

use crate::host_function;
use crate::interpreter::call_frame::CallFrame;
use crate::interpreter::stack_visitor::{IterationStatus, StackVisitor};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{collection_constructor_structure, create_collection_constructor, derived_structure, put_to_string_tag};
use crate::runtime::error_type::ErrorType;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::native_class_support::{property_key as key, put_native_accessor, throw_coded_type_error, throw_native_type_error};
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_internal_field_object_impl::define_internal_field_cell;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_number, js_number_i32, js_undefined, JSValue};
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// Os campos internos da instância.
#[derive(Clone, Copy)]
enum Field {
    Name = 0,
    Message = 1,
    Code = 2,
}

define_internal_field_cell!(
    JSDOMException,
    JSDOMExceptionRef,
    DOMException,
    ObjectType,
    JS_DOM_EXCEPTION_S_INFO,
    "DOMException",
    3,
    [js_undefined(), js_undefined(), js_number_i32(0)]
);

/// `const ClassInfo` do protótipo.
static DOM_EXCEPTION_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "DOMException", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo` do construtor (`"Function"`).
static DOM_EXCEPTION_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `true` se o valor é um objeto cuja cadeia de protótipos contém o `DOMException.prototype`: uma instância, de uma
/// subclasse, ou um objeto qualquer criado com esse protótipo (`Object.create(DOMException.prototype)`).
pub fn has_dom_exception_prototype(value: &JSValue) -> bool {
    let mut current = value.clone();
    while current.is_object() {
        current = current.as_object().get_prototype_direct();
        if current.is_object() && std::ptr::eq(current.as_object().class_info(), &DOM_EXCEPTION_PROTOTYPE_S_INFO) {
            return true;
        }
    }
    false
}

/// As constantes legadas, na ordem do bun: `(nome da constante, valor)`.
const LEGACY_CONSTANTS: [(&str, i32); 25] = [
    ("INDEX_SIZE_ERR", 1),
    ("DOMSTRING_SIZE_ERR", 2),
    ("HIERARCHY_REQUEST_ERR", 3),
    ("WRONG_DOCUMENT_ERR", 4),
    ("INVALID_CHARACTER_ERR", 5),
    ("NO_DATA_ALLOWED_ERR", 6),
    ("NO_MODIFICATION_ALLOWED_ERR", 7),
    ("NOT_FOUND_ERR", 8),
    ("NOT_SUPPORTED_ERR", 9),
    ("INUSE_ATTRIBUTE_ERR", 10),
    ("INVALID_STATE_ERR", 11),
    ("SYNTAX_ERR", 12),
    ("INVALID_MODIFICATION_ERR", 13),
    ("NAMESPACE_ERR", 14),
    ("INVALID_ACCESS_ERR", 15),
    ("VALIDATION_ERR", 16),
    ("TYPE_MISMATCH_ERR", 17),
    ("SECURITY_ERR", 18),
    ("NETWORK_ERR", 19),
    ("ABORT_ERR", 20),
    ("URL_MISMATCH_ERR", 21),
    ("QUOTA_EXCEEDED_ERR", 22),
    ("TIMEOUT_ERR", 23),
    ("INVALID_NODE_TYPE_ERR", 24),
    ("DATA_CLONE_ERR", 25),
];

/// Os nomes que têm código legado diferente de 0.
const LEGACY_NAMES: [(&str, i32); 22] = [
    ("IndexSizeError", 1),
    ("HierarchyRequestError", 3),
    ("WrongDocumentError", 4),
    ("InvalidCharacterError", 5),
    ("NoModificationAllowedError", 7),
    ("NotFoundError", 8),
    ("NotSupportedError", 9),
    ("InUseAttributeError", 10),
    ("InvalidStateError", 11),
    ("SyntaxError", 12),
    ("InvalidModificationError", 13),
    ("NamespaceError", 14),
    ("InvalidAccessError", 15),
    ("TypeMismatchError", 17),
    ("SecurityError", 18),
    ("NetworkError", 19),
    ("AbortError", 20),
    ("URLMismatchError", 21),
    ("QuotaExceededError", 22),
    ("TimeoutError", 23),
    ("InvalidNodeTypeError", 24),
    ("DataCloneError", 25),
];

thread_local! {
    /// A `Structure` das instâncias de cada realm (a chave é o `cell_id` do `JSGlobalObject`), para as
    /// exceções que as funções nativas lançam sem passar pelo construtor.
    static INSTANCE_STRUCTURES: RefCell<Vec<(usize, StructureRef)>> = const { RefCell::new(Vec::new()) };
}

/// Fim do programa (`cell_registry::reset_program_state`): as estruturas guardadas são do programa.
pub(crate) fn reset_for_program() {
    let _ = INSTANCE_STRUCTURES.try_with(|structures| structures.borrow_mut().clear());
}

fn code_for_name(name: &WtfString) -> i32 {
    let units: Vec<u16> = (0..name.length()).map(|index| name.code_unit_at(index)).collect();
    LEGACY_NAMES.iter().find(|(known, _)| known.encode_utf16().eq(units.iter().copied())).map_or(0, |(_, code)| *code)
}

fn string_value(vm: &VM, text: &WtfString) -> JSValue {
    JSValue::from_js_string(js_string(vm, text))
}

/// `ToString(value)`, ou `default` quando `value` é `undefined` (o `= ""` e o `= "Error"` do WebIDL).
fn string_or(global_object: &JSGlobalObject, value: JSValue, default: &str) -> Result<WtfString, Thrown> {
    if value.is_undefined() {
        return Ok(WtfString::from_latin1(default.as_bytes()));
    }
    pending_or(global_object, value.to_wtf_string())
}

fn init_cell(vm: &VM, cell: &JSDOMExceptionRef, name: &WtfString, message: &WtfString) {
    cell.set_internal_field(Field::Name as u32, string_value(vm, name));
    cell.set_internal_field(Field::Message as u32, string_value(vm, message));
    cell.set_internal_field(Field::Code as u32, js_number_i32(code_for_name(name)));
}

/// Lança um `DOMException` criado fora de uma função nativa (sem frame de host) e devolve `Thrown::Pending`.
pub fn throw_dom_exception(global_object: &JSGlobalObject, name: &str, message: &str) -> Thrown {
    throw_created(global_object, None, name, message)
}

/// Lança um `DOMException` a partir da função nativa `call` (`atob`, `btoa`, `structuredClone`): o bun grava nele
/// `line`, `column`, `sourceURL` (se o fonte tem nome) e `stack`, como o `addErrorInfo` e o
/// `addStackTraceIfNecessary` fazem num objeto lançado que não é `ErrorInstance`.
pub fn throw_dom_exception_from_host(global_object: &JSGlobalObject, call: &HostCall, name: &str, message: &str) -> Thrown {
    throw_created(global_object, Some(call), name, message)
}

fn throw_created(global_object: &JSGlobalObject, call: Option<&HostCall>, name: &str, message: &str) -> Thrown {
    let vm = global_object.vm();
    let Some(cell) = create_cell(global_object, name, message) else {
        return Thrown::Unported("DOMException sem instalação no realm");
    };
    if let Some(call) = call {
        add_native_error_info(global_object, call, &cell);
    }
    let mut scope = ThrowScope::new(vm);
    throw_exception(global_object, &mut scope, cell.as_value());
    Thrown::Pending
}

/// A célula `DOMException` de `name` e `message` no realm, ou `None` se o realm não a instalou.
fn create_cell(global_object: &JSGlobalObject, name: &str, message: &str) -> Option<JSDOMExceptionRef> {
    let vm = global_object.vm();
    let realm = global_object.cell_id();
    let structure = INSTANCE_STRUCTURES.with(|structures| structures.borrow().iter().find(|(id, _)| *id == realm).map(|(_, structure)| structure.clone()))?;
    let cell = JSDOMException::create(vm, &structure);
    init_cell(vm, &cell, &WtfString::from_latin1(name.as_bytes()), &WtfString::from_latin1(message.as_bytes()));
    Some(cell)
}

/// Um `DOMException` novo (não lançado) criado pela função nativa `call`, com as mesmas propriedades próprias
/// (`line`, `column`, `sourceURL`, `stack`) de um lançado: medido no bun, o `reason` padrão do `AbortSignal` as tem.
pub fn new_dom_exception(global_object: &JSGlobalObject, call: &HostCall, name: &str, message: &str) -> Result<JSValue, Thrown> {
    let cell = create_cell(global_object, name, message).ok_or(Thrown::Unported("DOMException sem instalação no realm"))?;
    add_native_error_info(global_object, call, &cell);
    Ok(cell.as_value())
}

/// Um `DOMException` novo criado fora de qualquer chamada de script (dentro de um timer nativo, como o
/// `TimeoutError` do `AbortSignal.timeout`): medido no bun, a única propriedade própria é `stack`, vazia e não
/// enumerável (sem `line`, `column` nem `sourceURL`).
pub fn new_detached_dom_exception(global_object: &JSGlobalObject, name: &str, message: &str) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    let cell = create_cell(global_object, name, message).ok_or(Thrown::Unported("DOMException sem instalação no realm"))?;
    cell.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.stack), string_value(vm, &WtfString::from_utf8(b"")), DONT_ENUM);
    Ok(cell.as_value())
}

/// As propriedades próprias da exceção lançada por função nativa. Medido no bun 1.4.2: `Error.stackTraceLimit`
/// não numérico ou apagado não grava nada; `0`, negativo ou `NaN` grava só `stack` vazia; senão vêm, nesta
/// ordem, `line`, `column` (enumeráveis, graváveis, configuráveis; a posição do primeiro frame JS dentro do
/// limite, `0` e `0` se nenhum), `sourceURL` (mesmo frame, só se não vazio) e `stack` (não enumerável), a pilha
/// no formato do JavaScriptCore (`nome@url:linha:coluna`, uma linha por frame, o frame nativo no topo).
fn add_native_error_info(global_object: &JSGlobalObject, call: &HostCall, cell: &JSDOMExceptionRef) {
    let vm = global_object.vm();
    let (Some(registers), Some(limit)) = (call.native_frame_registers(), global_object.stack_trace_limit.get()) else {
        return;
    };
    let mut lines: Vec<String> = Vec::new();
    let mut position: Option<(u32, u32, String)> = None;
    if limit > 0 {
        let interpreter = vm.interpreter();
        // Medido no bun 1.4.2: `Reflect.apply(atob, null, ['!'])` não mostra o `apply` na pilha, porque o builtin
        // chama `target.@apply(...)` em posição de cauda e o frame dele é trocado pelo do nativo. O frame logo abaixo
        // do nativo (índice 1) some, e a posição vem do chamador do builtin.
        let skip_tail_caller = vm.native_call_tail();
        let mut visited = 0usize;
        StackVisitor::visit(&interpreter, Some(CallFrame::create(registers)), false, |frame| {
            if lines.len() >= limit as usize {
                return IterationStatus::Done;
            }
            visited += 1;
            if skip_tail_caller && visited == 2 {
                return IterationStatus::Continue;
            }
            if frame.is_implementation_visibility_private() {
                return IterationStatus::Continue;
            }
            if position.is_none() && frame.has_line_and_column_info() {
                // Medido no bun 1.4.2: o primeiro frame com `CodeBlock` dá a posição, inclusive um builtin em JS
                // (`[1].map(atob)`, `forEach`, `new Promise(atob)`): aí `line` 1 e `column` 11 (o `native:1:11` do
                // frame `map@`), sem `sourceURL`. Só a chamada direta (`atob('!')`, `Reflect.apply`, `call`) aponta
                // o script.
                position = Some(if frame.is_builtin_function() {
                    (1, 11, String::new())
                } else {
                    let line_column = frame.compute_line_and_column();
                    // Medido no bun 1.4.2: o código de `eval` não tem `sourceURL` próprio (a pilha mostra a origem do chamador, o
                    // acessor não).
                    let from_eval = frame.code_block().is_some_and(|code_block| code_block.borrow().owner_executable().source().provider().is_some_and(|provider| provider.is_eval_code()));
                    let source_url = if from_eval { String::new() } else { frame.source_url() };
                    (line_column.line, line_column.column, source_url)
                });
            }
            lines.push(frame.to_string(vm));
            IterationStatus::Continue
        });
        let (line, column, source_url) = position.unwrap_or_default();
        let names = &vm.property_names;
        cell.put_direct(vm, &PropertyName::from_identifier(&names.line), js_number(f64::from(line)), 0);
        cell.put_direct(vm, &PropertyName::from_identifier(&names.column), js_number(f64::from(column)), 0);
        if !source_url.is_empty() {
            let url = string_value(vm, &WtfString::from_utf8(source_url.as_bytes()));
            cell.put_direct(vm, &PropertyName::from_identifier(&names.source_url), url, 0);
        }
    }
    let stack = string_value(vm, &WtfString::from_utf8(lines.join("\n").as_bytes()));
    cell.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.stack), stack, DONT_ENUM);
}

/// `DOMException(...)` sem `new`.
fn call_dom_exception_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new DOMException(...)` instead of `DOMException(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

/// `new DOMException(message, options)`.
fn construct_dom_exception_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let structure = derived_structure(global_object, call, JSDOMException::create_structure)?;
    let message = string_or(global_object, call.argument(0), "")?;
    let options = call.argument(1);
    let cause_key = key(vm, "cause");
    let mut cause = None;
    let name = match ObjectRef::from_value(&options) {
        Some(object) => {
            let name_value = pending_or(global_object, object.get(global_object, &PropertyName::from_identifier(&vm.property_names.name)))?;
            let name = string_or(global_object, name_value, "Error")?;
            let mut slot = PropertySlot::new(object.as_value(), InternalMethodType::HasProperty);
            let has_cause = object.get_property_slot(global_object, &cause_key, &mut slot);
            if has_cause {
                cause = Some(pending_or(global_object, object.get(global_object, &cause_key))?);
            } else {
                pending_or(global_object, ())?;
            }
            name
        }
        None => string_or(global_object, options, "Error")?,
    };
    let cell = JSDOMException::create(vm, &structure);
    init_cell(vm, &cell, &name, &message);
    if let Some(cause) = cause {
        cell.put_direct(vm, &cause_key, cause, DONT_ENUM);
    }
    Ok(cell.as_value())
}

host_function!(call_dom_exception, call_dom_exception_body);
host_function!(construct_dom_exception, construct_dom_exception_body);

/// O campo `field` do `this`, ou o `TypeError` de getter em objeto alheio.
fn field_getter(global_object: &JSGlobalObject, call: &HostCall, field: Field, label: &str) -> HostResult {
    match JSDOMException::from_value(&call.this_value()) {
        Some(exception) => Ok(exception.internal_field(field as u32)),
        None => Err(throw_native_type_error(global_object, &format!("The DOMException.{label} getter can only be used on instances of DOMException"))),
    }
}

fn dom_exception_code_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    field_getter(global_object, call, Field::Code, "code")
}

fn dom_exception_name_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    field_getter(global_object, call, Field::Name, "name")
}

fn dom_exception_message_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    field_getter(global_object, call, Field::Message, "message")
}

host_function!(dom_exception_code, dom_exception_code_body);
host_function!(dom_exception_name, dom_exception_name_body);
host_function!(dom_exception_message, dom_exception_message_body);

/// As 25 constantes legadas em `object` (`ReadOnly|DontDelete`, enumeráveis).
fn put_legacy_constants(vm: &VM, object: &JSObject) {
    for (name, value) in LEGACY_CONSTANTS {
        object.put_direct(vm, &key(vm, name), js_number_i32(value), READ_ONLY | DONT_DELETE);
    }
}

/// Instala `DOMException` no global: protótipo herdando de `Error.prototype`, construtor, constantes e a
/// `Structure` das instâncias lançadas pelo código nativo.
pub fn install_dom_exception(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let error_prototype = global_object.error_structure_for(ErrorType::Error).stored_prototype();
    let prototype_structure = Structure::create(
        vm,
        Some(global_object),
        error_prototype,
        TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
        &DOM_EXCEPTION_PROTOTYPE_S_INFO,
    );
    let prototype = JSObject::allocate(vm, &prototype_structure);
    prototype.did_become_prototype(vm);

    let constructor_structure =
        collection_constructor_structure(vm, global_object, global_object.function_prototype().as_value(), &DOM_EXCEPTION_CONSTRUCTOR_S_INFO);
    let constructor = create_collection_constructor(
        vm,
        global_object,
        constructor_structure,
        &prototype,
        "DOMException",
        0,
        call_dom_exception,
        construct_dom_exception,
        false,
    );
    put_legacy_constants(vm, &constructor);

    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    // Acessores do WebCore: nome `get code`, mas `toString()` com o nome puro (`function code() ...`).
    put_native_accessor(vm, global_object, &prototype, "code", dom_exception_code, None, 0);
    put_native_accessor(vm, global_object, &prototype, "name", dom_exception_name, None, 0);
    put_native_accessor(vm, global_object, &prototype, "message", dom_exception_message, None, 0);
    put_legacy_constants(vm, &prototype);
    put_to_string_tag(vm, &prototype, "DOMException");

    let instance_structure = JSDOMException::create_structure(vm, Some(global_object), prototype.as_value());
    let realm = global_object.cell_id();
    INSTANCE_STRUCTURES.with(|structures| {
        let mut structures = structures.borrow_mut();
        structures.retain(|(id, _)| *id != realm);
        structures.push((realm, instance_structure));
    });
    global_object.put_direct(vm, &key(vm, "DOMException"), constructor.as_value(), 0);
}
