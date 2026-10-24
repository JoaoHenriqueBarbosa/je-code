//! Porte de `runtime/ConsoleObject.{h,cpp}`, `runtime/ConsoleClient.{h,cpp}` e `runtime/ConsoleTypes.h`: o objeto
//! `console` (um `JSNonFinalObject` com o `ClassInfo` `"console"`) e as 26 funções, que só repassam os
//! argumentos ao `ConsoleClient` do global. Sem cliente, como no C++, nenhuma faz nada e todas devolvem
//! `undefined`.
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//! - `Inspector::ScriptArguments` (`createScriptArguments`) é a lista de valores a partir do argumento
//!   `skipArgumentCount`; a `Strong<Unknown>` do C++ não existe no porte, os valores ficam na lista como estão.
//! - Os métodos não virtuais de `ConsoleClient` (`logWithLevel`, `clear`, `dir`, ...) são métodos com corpo
//!   padrão do `trait`; o `Rust` não tem como impedir que um cliente os substitua.
//! - `ConsoleClient::printConsoleMessage` e `printConsoleMessageWithArguments` não existem: dependem de
//!   `ScriptCallStack`/`ScriptCallFrame` (inspetor) e de `WTFLogAlways`, e `MessageSource` só serve a eles.
//! - `WeakPtr<ConsoleClient> m_consoleClient` do `JSGlobalObject` é o campo `console_client`
//!   (`Weak<dyn ConsoleClient>`) com `set_console_client` e `console_client` abaixo.
//! - A propriedade `console` do global é `PropertyCallback` (criada na primeira leitura) no C++; aqui
//!   [`install_console`] a cria na hora, `DontEnum`.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_ENUM, NONE};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;
use std::rc::{Rc, Weak};

/// `const ClassInfo ConsoleObject::s_info`.
pub static CONSOLE_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "console", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `enum class MessageType` (`ConsoleTypes.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageType {
    Log,
    Dir,
    DirXML,
    Table,
    Trace,
    StartGroup,
    StartGroupCollapsed,
    EndGroup,
    Clear,
    Assert,
    Timing,
    Profile,
    ProfileEnd,
    Image,
}

/// `enum class MessageLevel` (`ConsoleTypes.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageLevel {
    Log,
    Warning,
    Error,
    Debug,
    Info,
}

/// `Inspector::ScriptArguments`: os argumentos que a chamada de `console` passou.
#[derive(Clone, Debug, Default)]
pub struct ScriptArguments {
    arguments: Vec<JSValue>,
}

impl ScriptArguments {
    /// `ScriptArguments::create(globalObject, arguments)`.
    pub fn create(arguments: Vec<JSValue>) -> ScriptArguments {
        ScriptArguments { arguments }
    }

    /// `argumentCount()`.
    pub fn argument_count(&self) -> usize {
        self.arguments.len()
    }

    /// `argumentAt(index)`.
    pub fn argument_at(&self, index: usize) -> JSValue {
        self.arguments[index]
    }
}

/// `Inspector::createScriptArguments(globalObject, callFrame, skipArgumentCount)`.
fn create_script_arguments(call: &HostCall, skip_argument_count: usize) -> ScriptArguments {
    ScriptArguments::create(call.arguments().iter().skip(skip_argument_count).copied().collect())
}

/// `ConsoleClient::ArgumentRequirement`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgumentRequirement {
    No,
    Yes,
}

/// `ConsoleClient::internalMessageWithTypeAndLevel`.
fn internal_message_with_type_and_level<C: ConsoleClient + ?Sized>(
    client: &C,
    message_type: MessageType,
    level: MessageLevel,
    global_object: &JSGlobalObject,
    arguments: ScriptArguments,
    argument_requirement: ArgumentRequirement,
) {
    if argument_requirement == ArgumentRequirement::Yes && arguments.argument_count() == 0 {
        return;
    }
    client.message_with_type_and_level(message_type, level, global_object, arguments);
}

/// `class ConsoleClient`: o destino das funções de `console`. Os métodos sem corpo são os virtuais puros do
/// C++; os com corpo são os não virtuais.
pub trait ConsoleClient {
    fn message_with_type_and_level(&self, message_type: MessageType, level: MessageLevel, global_object: &JSGlobalObject, arguments: ScriptArguments);
    fn count(&self, global_object: &JSGlobalObject, label: &WtfString);
    fn count_reset(&self, global_object: &JSGlobalObject, label: &WtfString);
    fn profile(&self, global_object: &JSGlobalObject, title: &WtfString);
    fn profile_end(&self, global_object: &JSGlobalObject, title: &WtfString);
    fn take_heap_snapshot(&self, global_object: &JSGlobalObject, title: &WtfString);
    fn time(&self, global_object: &JSGlobalObject, label: &WtfString);
    fn time_log(&self, global_object: &JSGlobalObject, label: &WtfString, arguments: ScriptArguments);
    fn time_end(&self, global_object: &JSGlobalObject, label: &WtfString);
    fn time_stamp(&self, global_object: &JSGlobalObject, arguments: ScriptArguments);
    fn record(&self, global_object: &JSGlobalObject, arguments: ScriptArguments);
    fn record_end(&self, global_object: &JSGlobalObject, arguments: ScriptArguments);
    fn screenshot(&self, global_object: &JSGlobalObject, arguments: ScriptArguments);

    /// `ConsoleClient::logWithLevel`.
    fn log_with_level(&self, global_object: &JSGlobalObject, arguments: ScriptArguments, level: MessageLevel) {
        internal_message_with_type_and_level(self, MessageType::Log, level, global_object, arguments, ArgumentRequirement::No);
    }

    /// `ConsoleClient::clear`.
    fn clear(&self, global_object: &JSGlobalObject) {
        internal_message_with_type_and_level(self, MessageType::Clear, MessageLevel::Log, global_object, ScriptArguments::default(), ArgumentRequirement::No);
    }

    /// `ConsoleClient::dir`.
    fn dir(&self, global_object: &JSGlobalObject, arguments: ScriptArguments) {
        internal_message_with_type_and_level(self, MessageType::Dir, MessageLevel::Log, global_object, arguments, ArgumentRequirement::Yes);
    }

    /// `ConsoleClient::dirXML`.
    fn dir_xml(&self, global_object: &JSGlobalObject, arguments: ScriptArguments) {
        internal_message_with_type_and_level(self, MessageType::DirXML, MessageLevel::Log, global_object, arguments, ArgumentRequirement::Yes);
    }

    /// `ConsoleClient::table`.
    fn table(&self, global_object: &JSGlobalObject, arguments: ScriptArguments) {
        internal_message_with_type_and_level(self, MessageType::Table, MessageLevel::Log, global_object, arguments, ArgumentRequirement::Yes);
    }

    /// `ConsoleClient::trace`.
    fn trace(&self, global_object: &JSGlobalObject, arguments: ScriptArguments) {
        internal_message_with_type_and_level(self, MessageType::Trace, MessageLevel::Log, global_object, arguments, ArgumentRequirement::No);
    }

    /// `ConsoleClient::assertion`.
    fn assertion(&self, global_object: &JSGlobalObject, arguments: ScriptArguments) {
        internal_message_with_type_and_level(self, MessageType::Assert, MessageLevel::Error, global_object, arguments, ArgumentRequirement::No);
    }

    /// `ConsoleClient::group`.
    fn group(&self, global_object: &JSGlobalObject, arguments: ScriptArguments) {
        internal_message_with_type_and_level(self, MessageType::StartGroup, MessageLevel::Log, global_object, arguments, ArgumentRequirement::No);
    }

    /// `ConsoleClient::groupCollapsed`.
    fn group_collapsed(&self, global_object: &JSGlobalObject, arguments: ScriptArguments) {
        internal_message_with_type_and_level(self, MessageType::StartGroupCollapsed, MessageLevel::Log, global_object, arguments, ArgumentRequirement::No);
    }

    /// `ConsoleClient::groupEnd`.
    fn group_end(&self, global_object: &JSGlobalObject, arguments: ScriptArguments) {
        internal_message_with_type_and_level(self, MessageType::EndGroup, MessageLevel::Log, global_object, arguments, ArgumentRequirement::No);
    }
}

impl JSGlobalObject {
    /// `JSGlobalObject::setConsoleClient`.
    pub fn set_console_client(&self, client: Weak<dyn ConsoleClient>) {
        *self.console_client.borrow_mut() = Some(client);
    }

    /// `JSGlobalObject::consoleClient`: `None` sem cliente ou com o cliente já destruído.
    pub fn console_client(&self) -> Option<Rc<dyn ConsoleClient>> {
        self.console_client.borrow().as_ref().and_then(Weak::upgrade)
    }
}

/// `value.toWTFString(globalObject)` com o `RETURN_IF_EXCEPTION` que vem depois.
fn to_wtf_string_checked(global_object: &JSGlobalObject, value: JSValue) -> Result<WtfString, Thrown> {
    pending_or(global_object, value.to_wtf_string())
}

/// `valueOrDefaultLabelString`.
fn value_or_default_label_string(global_object: &JSGlobalObject, call: &HostCall) -> Result<WtfString, Thrown> {
    if call.argument_count() < 1 || call.argument(0).is_undefined() {
        return Ok(WtfString::from_latin1(b"default"));
    }
    to_wtf_string_checked(global_object, call.argument(0))
}

/// `valueToStringWithUndefinedOrNullCheck`.
fn value_to_string_with_undefined_or_null_check(global_object: &JSGlobalObject, value: JSValue) -> Result<WtfString, Thrown> {
    if value.is_undefined() || value.is_null() {
        return Ok(WtfString::default());
    }
    to_wtf_string_checked(global_object, value)
}

/// Define o corpo e a função nativa de uma função de `console`: sem cliente devolve `undefined` antes de
/// qualquer conversão de argumento (`auto client = globalObject->consoleClient(); if (!client) ...`).
macro_rules! console_function {
    ($host:ident, $body:ident, |$client:ident, $global:ident, $call:ident| $code:block) => {
        #[allow(unused_variables)]
        fn $body($global: &JSGlobalObject, $call: &HostCall) -> HostResult {
            let Some($client) = $global.console_client() else {
                return Ok(JSValue::undefined());
            };
            $code
            // O cliente não devolve `Result`: uma exceção que ele deixou pendente (`%s` com Symbol) sobe aqui.
            pending_or($global, ())?;
            Ok(JSValue::undefined())
        }
        host_function!($host, $body);
    };
}

console_function!(console_proto_func_debug, console_proto_func_debug_body, |client, global_object, call| {
    client.log_with_level(global_object, create_script_arguments(call, 0), MessageLevel::Debug);
});
console_function!(console_proto_func_error, console_proto_func_error_body, |client, global_object, call| {
    client.log_with_level(global_object, create_script_arguments(call, 0), MessageLevel::Error);
});
console_function!(console_proto_func_log, console_proto_func_log_body, |client, global_object, call| {
    client.log_with_level(global_object, create_script_arguments(call, 0), MessageLevel::Log);
});
console_function!(console_proto_func_info, console_proto_func_info_body, |client, global_object, call| {
    client.log_with_level(global_object, create_script_arguments(call, 0), MessageLevel::Info);
});
console_function!(console_proto_func_warn, console_proto_func_warn_body, |client, global_object, call| {
    client.log_with_level(global_object, create_script_arguments(call, 0), MessageLevel::Warning);
});
console_function!(console_proto_func_clear, console_proto_func_clear_body, |client, global_object, call| {
    client.clear(global_object);
});
console_function!(console_proto_func_dir, console_proto_func_dir_body, |client, global_object, call| {
    client.dir(global_object, create_script_arguments(call, 0));
});
console_function!(console_proto_func_dir_xml, console_proto_func_dir_xml_body, |client, global_object, call| {
    client.dir_xml(global_object, create_script_arguments(call, 0));
});
console_function!(console_proto_func_table, console_proto_func_table_body, |client, global_object, call| {
    client.table(global_object, create_script_arguments(call, 0));
});
console_function!(console_proto_func_trace, console_proto_func_trace_body, |client, global_object, call| {
    client.trace(global_object, create_script_arguments(call, 0));
});
console_function!(console_proto_func_assert, console_proto_func_assert_body, |client, global_object, call| {
    let condition = call.argument(0).to_boolean();
    pending_or(global_object, ())?;
    if condition {
        return Ok(JSValue::undefined());
    }
    client.assertion(global_object, create_script_arguments(call, 1));
});
console_function!(console_proto_func_count, console_proto_func_count_body, |client, global_object, call| {
    let label = value_or_default_label_string(global_object, call)?;
    client.count(global_object, &label);
});
console_function!(console_proto_func_count_reset, console_proto_func_count_reset_body, |client, global_object, call| {
    let label = value_or_default_label_string(global_object, call)?;
    client.count_reset(global_object, &label);
});
console_function!(console_proto_func_profile, console_proto_func_profile_body, |client, global_object, call| {
    if call.argument_count() == 0 {
        client.profile(global_object, &WtfString::default());
        return Ok(JSValue::undefined());
    }
    let title = value_to_string_with_undefined_or_null_check(global_object, call.argument(0))?;
    client.profile(global_object, &title);
});
console_function!(console_proto_func_profile_end, console_proto_func_profile_end_body, |client, global_object, call| {
    if call.argument_count() == 0 {
        client.profile_end(global_object, &WtfString::default());
        return Ok(JSValue::undefined());
    }
    let title = value_to_string_with_undefined_or_null_check(global_object, call.argument(0))?;
    client.profile_end(global_object, &title);
});
console_function!(console_proto_func_take_heap_snapshot, console_proto_func_take_heap_snapshot_body, |client, global_object, call| {
    if call.argument_count() == 0 {
        client.take_heap_snapshot(global_object, &WtfString::default());
        return Ok(JSValue::undefined());
    }
    let title = value_to_string_with_undefined_or_null_check(global_object, call.argument(0))?;
    client.take_heap_snapshot(global_object, &title);
});
console_function!(console_proto_func_time, console_proto_func_time_body, |client, global_object, call| {
    let label = value_or_default_label_string(global_object, call)?;
    client.time(global_object, &label);
});
console_function!(console_proto_func_time_log, console_proto_func_time_log_body, |client, global_object, call| {
    let label = value_or_default_label_string(global_object, call)?;
    client.time_log(global_object, &label, create_script_arguments(call, 1));
});
console_function!(console_proto_func_time_end, console_proto_func_time_end_body, |client, global_object, call| {
    let label = value_or_default_label_string(global_object, call)?;
    client.time_end(global_object, &label);
});
console_function!(console_proto_func_time_stamp, console_proto_func_time_stamp_body, |client, global_object, call| {
    client.time_stamp(global_object, create_script_arguments(call, 0));
});
console_function!(console_proto_func_group, console_proto_func_group_body, |client, global_object, call| {
    client.group(global_object, create_script_arguments(call, 0));
});
console_function!(console_proto_func_group_collapsed, console_proto_func_group_collapsed_body, |client, global_object, call| {
    client.group_collapsed(global_object, create_script_arguments(call, 0));
});
console_function!(console_proto_func_group_end, console_proto_func_group_end_body, |client, global_object, call| {
    client.group_end(global_object, create_script_arguments(call, 0));
});
console_function!(console_proto_func_record, console_proto_func_record_body, |client, global_object, call| {
    client.record(global_object, create_script_arguments(call, 0));
});
console_function!(console_proto_func_record_end, console_proto_func_record_end_body, |client, global_object, call| {
    client.record_end(global_object, create_script_arguments(call, 0));
});
console_function!(console_proto_func_screenshot, console_proto_func_screenshot_body, |client, global_object, call| {
    client.screenshot(global_object, create_script_arguments(call, 0));
});

/// `class ConsoleObject final : public JSNonFinalObject`: sem campos próprios.
pub struct ConsoleObject;

impl ConsoleObject {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, ConsoleObject::STRUCTURE_FLAGS),
            &CONSOLE_OBJECT_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `ConsoleObject(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let object = JSObject::allocate(vm, structure);
        ConsoleObject::finish_creation(&object, vm, global_object);
        object
    }

    /// `finishCreation(vm, globalObject)`: por razões históricas as propriedades de `console` são enumeráveis,
    /// graváveis, apagáveis e todas têm `length` 0; depois vem o `@@toStringTag`.
    fn finish_creation(object: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        object.finish_creation(vm);

        let functions: [(&str, NativeFunction); 26] = [
            ("debug", console_proto_func_debug),
            ("error", console_proto_func_error),
            ("log", console_proto_func_log),
            ("info", console_proto_func_info),
            ("warn", console_proto_func_warn),
            ("clear", console_proto_func_clear),
            ("dir", console_proto_func_dir),
            ("dirxml", console_proto_func_dir_xml),
            ("table", console_proto_func_table),
            ("trace", console_proto_func_trace),
            ("assert", console_proto_func_assert),
            ("count", console_proto_func_count),
            ("countReset", console_proto_func_count_reset),
            ("profile", console_proto_func_profile),
            ("profileEnd", console_proto_func_profile_end),
            ("time", console_proto_func_time),
            ("timeLog", console_proto_func_time_log),
            ("timeEnd", console_proto_func_time_end),
            ("timeStamp", console_proto_func_time_stamp),
            ("takeHeapSnapshot", console_proto_func_take_heap_snapshot),
            ("group", console_proto_func_group),
            ("groupCollapsed", console_proto_func_group_collapsed),
            ("groupEnd", console_proto_func_group_end),
            ("record", console_proto_func_record),
            ("recordEnd", console_proto_func_record_end),
            ("screenshot", console_proto_func_screenshot),
        ];
        for (name, function) in functions {
            put_direct_native_function_without_transition(
                vm,
                global_object,
                object,
                &Identifier::from_span(vm, name.as_bytes()),
                0,
                function,
                ImplementationVisibility::Public,
                Intrinsic::NoIntrinsic,
                NONE,
            );
        }

        put_to_string_tag(vm, object, CONSOLE_OBJECT_S_INFO.class_name);
    }
}

/// `createConsoleProperty` e a entrada `console ... DontEnum|PropertyCallback` da tabela do global: cria o
/// `ConsoleObject` (protótipo `constructEmptyObject(global)`) e o grava como `console`. Chamada de
/// `JSGlobalObject::init`.
pub fn install_console(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let prototype = JSFinalObject::create(vm, &global_object.object_structure_for_object_constructor());
    let structure = ConsoleObject::create_structure(vm, global_object, prototype.as_value());
    let console = ConsoleObject::create(vm, global_object, &structure);
    global_object.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"console")), console.as_value(), DONT_ENUM);
}
