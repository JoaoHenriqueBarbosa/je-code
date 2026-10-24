//! `JSONObject` (`runtime/JSONObject.cpp`, `JSONObject::finishCreation`) e as quatro funções nativas do
//! `JSON`: `jsonProtoFuncParse`, `jsonProtoFuncStringify`, `jsonProtoFuncRawJSON` e
//! `jsonProtoFuncIsRawJSON`, ligadas à lógica de `json_object.rs` pelo `impl JsonHost for
//! JSGlobalObject` de `json_host.rs`.
//!
//! DIVERGÊNCIAS:
//!
//! - `JSONObject` não tem campos próprios nem classe própria além do `ClassInfo` (`"JSON"`), então o
//!   objeto é um `JSObject` comum registrado como `CellEntry::Object` (o mesmo caminho do `MathObject`).
//! - `parse` e `stringify` são as entradas da tabela estática `jsonTable` (`JSON_TABLE`), reificadas no
//!   primeiro acesso como no C++; o `finishCreation` só põe o `@@toStringTag`, `isRawJSON` e `rawJSON`.
//! - `JSGlobalObject::rawJSONObjectStructure()` é um `LazyProperty` que o global ainda não tem: cada
//!   `JSON.rawJSON` monta a própria `Structure` (a identidade da `Structure` não é observável).
//! - `jsonProtoFuncParse` faz o `toString` do texto uma vez e entrega ao `json_parse`, que decide pelo
//!   reviver como o `jsonParseSlow`; o resultado é o mesmo do caminho sem reviver do C++.

use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::HostCall;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_raw_json_object::JSRawJSONObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry};
use crate::runtime::js_value::{js_boolean, EncodedJSValue, JSValue};
use crate::runtime::json_object::{json_parse, json_stringify, throw_json_error, validate_raw_json};
use crate::runtime::literal_parser::{JsonError, JsonHost};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::options_list::Options;
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::Structure;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo JSONObject::s_info` (`"JSON"`).
pub static JSON_OBJECT_S_INFO: ClassInfo = ClassInfo {
    class_name: "JSON",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&JSON_TABLE),
    inherits_js_type_range: None,
};

/// `jsonTableValues` de `JSONObject.lut.h`, na ordem do `@begin`.
static JSON_TABLE_VALUES: [HashTableValue; 2] =
    [native_entry("parse", json_proto_func_parse, 2), native_entry("stringify", json_proto_func_stringify, 3)];

/// `jsonTable`.
static JSON_TABLE: HashTable = HashTable { class_for_this: None, values: &JSON_TABLE_VALUES };

/// `jsonProtoFuncParse`: o `toString` do texto e, se há segundo argumento, o reviver.
fn json_parse_body(global_object: &JSGlobalObject, call: &HostCall) -> Result<JSValue, JsonError> {
    let text = JsonHost::to_string(global_object, call.argument(0))?;
    let reviver = (call.argument_count() >= 2).then(|| call.argument(1));
    // Os frames de `jsonParseSlow` e do `Walker` que ficam na pilha enquanto o reviver roda.
    let _stack_frame = match reviver {
        Some(_) => match global_object.vm().enter_logical_frame(crate::runtime::vm::stack_cost::JSON_REVIVER) {
            Some(frame) => Some(frame),
            None => {
                let mut scope = crate::runtime::throw_scope::ThrowScope::new(global_object.vm());
                let exception = crate::runtime::exception_helpers::throw_stack_overflow_error(global_object, &mut scope);
                return Err(JsonError::Thrown(crate::runtime::js_promise_host::Thrown::Value(exception.value())));
            }
        },
        None => None,
    };
    json_parse(global_object, &text, reviver, Options::use_json_source_text_access())
}

/// `jsonProtoFuncStringify`: a string nula do C++ é `undefined`.
fn json_stringify_body(global_object: &JSGlobalObject, call: &HostCall) -> Result<JSValue, JsonError> {
    let result = json_stringify(global_object, call.argument(0), call.argument(1), call.argument(2))?;
    Ok(match result {
        Some(text) => JSValue::from_js_string(js_string(global_object.vm(), &text)),
        None => JSValue::undefined(),
    })
}

/// `jsonProtoFuncRawJSON`: valida o texto e cria o `JSRawJSONObject` com ele.
fn json_raw_json_body(global_object: &JSGlobalObject, call: &HostCall) -> Result<JSValue, JsonError> {
    let vm = global_object.vm();
    let text = JsonHost::to_string(global_object, call.argument(0))?;
    validate_raw_json(global_object, &text)?;
    let structure = JSRawJSONObject::create_structure(vm, global_object, JSValue::null());
    Ok(JSRawJSONObject::create(vm, &structure, js_string(vm, &text)).as_value())
}

/// `jsonProtoFuncIsRawJSON`: `callFrame->argument(0).inherits<JSRawJSONObject>()`.
fn json_is_raw_json_body(_global_object: &JSGlobalObject, call: &HostCall) -> Result<JSValue, JsonError> {
    Ok(js_boolean(JSRawJSONObject::from_value(&call.argument(0)).is_some()))
}

/// O invólucro `JSC_DEFINE_HOST_FUNCTION`: lê o quadro, roda o corpo e devolve o valor codificado, ou o
/// `EncodedJSValue` nulo com a exceção lançada (`throw_json_error`).
fn run_json_function(
    global_object: &JSGlobalObject,
    call_frame: &NativeCallFrame<'_>,
    body: fn(&JSGlobalObject, &HostCall) -> Result<JSValue, JsonError>,
) -> EncodedJSValue {
    match body(global_object, &HostCall::read(call_frame)) {
        Ok(value) => value.encode(),
        Err(error) => {
            throw_json_error(global_object, error);
            JSValue::empty().encode()
        }
    }
}

/// Define o invólucro `NativeFunction` de um corpo de função do `JSON`.
macro_rules! json_host_function {
    ($wrapper:ident, $body:path) => {
        pub(crate) fn $wrapper(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
            run_json_function(global_object, call_frame, $body)
        }
    };
}

json_host_function!(json_proto_func_parse, json_parse_body);
json_host_function!(json_proto_func_stringify, json_stringify_body);
json_host_function!(json_proto_func_raw_json, json_raw_json_body);
json_host_function!(json_proto_func_is_raw_json, json_is_raw_json_body);

/// `JSONObject::createStructure` + `JSONObject::create` + `finishCreation`: o objeto `JSON` com
/// `@@toStringTag` `"JSON"`, `parse`, `stringify` e, com `useJSONSourceTextAccess`, `rawJSON` e
/// `isRawJSON`. O protótipo é o `Object.prototype` do global.
pub fn create_json_object(global_object: &JSGlobalObject) -> JSObjectRef {
    let vm = global_object.vm();
    let structure = Structure::create(
        vm,
        Some(global_object),
        global_object.object_prototype().as_value(),
        TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE),
        &JSON_OBJECT_S_INFO,
    );
    let object = JSObject::allocate(vm, &structure);
    object.finish_creation(vm);

    // JSC_TO_STRING_TAG_WITHOUT_TRANSITION()
    let class_name = js_string(vm, &WtfString::from_latin1(JSON_OBJECT_S_INFO.class_name.as_bytes()));
    object.put_direct(
        vm,
        &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol),
        JSValue::from_js_string(class_name),
        DONT_ENUM | READ_ONLY,
    );

    let names = &vm.property_names;
    let mut functions: Vec<(&Identifier, u32, NativeFunction)> = Vec::new();
    // `parse` e `stringify` ficam na `jsonTable` (reificadas no primeiro acesso); o `finishCreation` só põe
    // `isRawJSON` e `rawJSON`, nessa ordem.
    if Options::use_json_source_text_access() {
        functions.push((&names.is_raw_json, 1, json_proto_func_is_raw_json));
        functions.push((&names.raw_json, 1, json_proto_func_raw_json));
    }
    for (name, length, function) in functions {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            &object,
            name,
            length,
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            DONT_ENUM,
        );
    }
    object
}
