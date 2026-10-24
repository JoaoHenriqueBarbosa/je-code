//! Porte de `runtime/ShadowRealmPrototype.{h,cpp}` e `ShadowRealmPrototypeInlines.h`: o `ShadowRealm.prototype`
//! (`evaluate` e `importValue` são builtins JS de `builtins/ShadowRealmPrototype.js`, `@@toStringTag`) e as
//! três funções nativas privadas que esses builtins chamam: `importInRealm`, `evalInRealm` e
//! `moveFunctionToRealm` (os `LinkTimeConstant` de mesmo nome).
//!
//! DIVERGÊNCIAS:
//! - A tabela `shadowRealmPrototypeTable` (`evaluate`, `importValue`: `JSBuiltin`) fica no `ClassInfo` e a
//!   `Structure` leva `HasStaticPropertyTable`: as duas reificam no primeiro acesso.
//! - `computeNewSourceTaintedOriginFromStack` não existe no porte: o taint é `Untainted`; a origem do
//!   `evalInRealm` é a do chamador (`HostCall::caller_source_origin`).
//! - `moveFunctionToRealm` usa `setPrototypeDirect` no lugar de `setPrototype(vm, targetGlobalObj, ...)`:
//!   a função é recém-criada e extensível, então `setPrototype` não pode falhar nem tem ciclo a checar.
//! - `sanitizedMessageString` do `ErrorInstance` é a `message()` guardada no `ErrorData`.
//! - `createTypeErrorCopy` (`Error.cpp`), que o `error.rs` deixou de fora, mora aqui: só este arquivo a usa.

use crate::runtime::js_promise_host::PromiseHost;
use crate::bytecode::executable_info::{DerivedContextType, EvalContextType};
use crate::host_function;
use crate::parser::parser_modes::{NO_LEXICALLY_SCOPED_FEATURES, TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE};
use crate::parser::source_code::make_source;
use crate::parser::source_provider::SourceProviderSourceType;
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::error::{create_syntax_error, create_type_error};
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::error_type::ErrorType;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::throw_error_pending;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{builtin_entry};
use crate::runtime::indirect_eval_executable;
use crate::runtime::js_function::JSFunctionRef;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::load_and_evaluate_module;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectHandle, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{js_undefined, JSValue};
use crate::runtime::property_attribute::{BUILTIN, DONT_ENUM};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::proxy_object::ProxyObject;
use crate::runtime::shadow_realm_object::ShadowRealmObject;
use crate::runtime::string_regexp_support::to_wtf_string_value;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::text_position::TextPosition;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::{String as WtfString};

/// `const ClassInfo ShadowRealmPrototype::s_info`.
pub static SHADOW_REALM_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "ShadowRealm",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&SHADOW_REALM_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `shadowRealmPrototypeTableValues` de `ShadowRealmPrototype.lut.h`, na ordem do `@begin`.
static SHADOW_REALM_PROTOTYPE_TABLE_VALUES: [HashTableValue; 2] = [
    builtin_entry("evaluate", BuiltinCodeIndex::ShadowRealmPrototypeEvaluateCode, 1),
    builtin_entry("importValue", BuiltinCodeIndex::ShadowRealmPrototypeImportValueCode, 2),
];

/// `shadowRealmPrototypeTable`.
static SHADOW_REALM_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &SHADOW_REALM_PROTOTYPE_TABLE_VALUES };

/// `createTypeErrorCopy(globalObject, error)`: um `TypeError` do reino de `global_object` com a mensagem de
/// `error` (a de uma primitiva é a própria conversão em string; a de um objeto que não é `Proxy` é o
/// valor de dado da propriedade própria `message`).
fn create_type_error_copy(global_object: &JSGlobalObject, error: JSValue) -> Result<JSObjectHandle, Thrown> {
    let vm = global_object.vm();
    let mut error_string = WtfString::from_latin1(b"Error encountered during evaluation");

    if !error.is_object() {
        error_string = error.to_wtf_string();
        pending_or(global_object, ())?;
    } else if let Some(object) = JSObject::from_value(&error) {
        if ProxyObject::from_cell_id(object.cell_id()).is_none() {
            let message_name = PropertyName::from_identifier(&vm.property_names.message);
            let mut slot = PropertySlot::new(error, InternalMethodType::GetOwnProperty);
            let found = object.get_own_property_slot(vm, &message_name, &mut slot);
            pending_or(global_object, ())?;
            if found && slot.is_value() {
                let message = slot.get_value_for(&message_name);
                pending_or(global_object, ())?;
                error_string = message.to_wtf_string();
                pending_or(global_object, ())?;
            }
        }
    }

    Ok(create_type_error(global_object, &error_string))
}

/// `importInRealm(thisRealm, specifier)`: a promessa (do reino de quem chama) que resolve com o namespace do
/// módulo importado no global do `ShadowRealm`.
fn import_in_realm_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let realm = ShadowRealmObject::from_value(&call.argument(0)).expect("importInRealm: o argumento 0 não é um ShadowRealm");

    let promise = JSPromise::create(vm, &global_object.promise_structure());

    let specifier = to_wtf_string_value(global_object, call.argument(1))?;
    let specifier = String::from_utf8_lossy(&specifier.utf8(ConversionMode::LenientConversion)).into_owned();

    let realm_global_object = realm.global_object();
    // A promessa do namespace é a do reino importado (`load_and_evaluate_module`); a nossa a adota.
    let namespace_promise = load_and_evaluate_module(&realm_global_object, &specifier);
    promise.resolve(global_object, namespace_promise.as_value());
    Ok(promise.as_value())
}
host_function!(pub import_in_realm, import_in_realm_body);

/// `evalInRealm(thisRealm, sourceText)`: avalia `sourceText` como `eval` indireto no global do
/// `ShadowRealm`; erro de sintaxe vira `SyntaxError` e qualquer outro erro vira `TypeError` do reino de
/// quem chama.
fn eval_in_realm_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let realm = ShadowRealmObject::from_value(&call.argument(0)).expect("evalInRealm: o argumento 0 não é um ShadowRealm");
    let realm_global_object = realm.global_object();

    // eval code adapted from JSGlobalObjecFunctions::globalFuncEval
    let script = call.argument(1).to_wtf_string();
    pending_or(global_object, ())?;

    let source = make_source(
        &script,
        &call.caller_source_origin(global_object),
        SourceTaintedOrigin::Untainted,
        WtfString::default(),
        TextPosition::default(),
        SourceProviderSourceType::Program,
    );
    let lexically_scoped_features = if global_object.global_scope_extension().is_some() {
        TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE
    } else {
        NO_LEXICALLY_SCOPED_FEATURES
    };
    let eval = match indirect_eval_executable::create(
        &realm_global_object,
        &source,
        lexically_scoped_features,
        DerivedContextType::None,
        false,
        EvalContextType::None,
    ) {
        Ok(eval) => eval,
        Err(error) => {
            if let Some(instance) = ErrorInstance::from_cell_id(error.cell_id()) {
                if instance.error_type() == Some(ErrorType::SyntaxError) {
                    let message = instance.message();
                    return Err(throw_error_pending(global_object, create_syntax_error(global_object, &message)));
                }
            }
            let type_error = create_type_error_copy(global_object, error.as_value())?;
            return Err(throw_error_pending(global_object, type_error));
        }
    };

    let this_value = match realm_global_object.global_this() {
        Some(global_this) => global_this.as_value(),
        None => JSValue::from_cell(realm_global_object.cell_id()),
    };
    let result = vm.interpreter().execute_eval(&eval, this_value, &realm_global_object.global_scope());
    if let Some(exception) = vm.exception() {
        let error = exception.value();
        vm.clear_exception();
        let type_error = create_type_error_copy(global_object, error)?;
        return Err(throw_error_pending(global_object, type_error));
    }

    Ok(result)
}
host_function!(pub eval_in_realm, eval_in_realm_body);

/// `dynamicDowncast<JSFunction>(value)`.
fn as_function(value: &JSValue) -> Option<JSFunctionRef> {
    let JSValue::Cell(cell_id) = value else { return None };
    match cell_registry::get(*cell_id) {
        Some(CellEntry::Function(function)) => Some(function),
        _ => None,
    }
}

/// `moveFunctionToRealm(wrappedFn, targetRealm)`: troca o protótipo da função pelo `Function.prototype` do
/// reino de destino.
fn move_function_to_realm_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let wrapped_function = as_function(&call.argument(0)).expect("moveFunctionToRealm: o argumento 0 não é um JSFunction");
    let target_realm =
        ShadowRealmObject::from_value(&call.argument(1)).expect("moveFunctionToRealm: o argumento 1 não é um ShadowRealm");

    let target_global_object = target_realm.global_object();
    let prototype = target_global_object.strict_function_structure(false).stored_prototype();
    wrapped_function.set_prototype_direct(global_object.vm(), prototype);
    Ok(js_undefined())
}
host_function!(pub move_function_to_realm, move_function_to_realm_body);

/// `class ShadowRealmPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct ShadowRealmPrototype;

impl ShadowRealmPrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, ShadowRealmPrototype::STRUCTURE_FLAGS),
            &SHADOW_REALM_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, structure)` e `finishCreation`: só o `@@toStringTag`; `evaluate` e `importValue` (builtins JS,
    /// `DontEnum`) vêm de `shadowRealmPrototypeTable` e reificam no primeiro acesso.
    pub fn create(vm: &VM, _global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        prototype.finish_creation(vm);
        put_to_string_tag(vm, &prototype, SHADOW_REALM_PROTOTYPE_S_INFO.class_name);
        prototype
    }
}
