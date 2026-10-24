//! Porte de `runtime/JSBoundFunction.h` e `JSBoundFunction.cpp`: a função que `Function.prototype.bind`
//! devolve (`boundFunctionCall`, `boundFunctionConstruct`, `create`, `name`, `length`, `canConstruct`,
//! `customHasInstance` e o `JSObject::hasInstance` que ela usa).
//!
//! DIVERGÊNCIAS, e por quê:
//! - `JSBoundFunction` é subclasse de `JSFunction`; aqui é o campo `bound` do `JSFunction`
//!   ([`JSFunction::as_bound_function`]), porque o registro de células, o `getCallData` e o resto do porte
//!   alcançam funções só por `CellEntry::Function`. A célula é a mesma `JSFunction`, com o
//!   `ClassInfo` e as `StructureFlags` do `JSBoundFunction` na `Structure`.
//! - `m_boundArgs`/`maxEmbeddedArgs` e o `JSCellButterfly` dos argumentos acima de três são um `Vec`.
//! - `vm.getBoundFunction(isJSFunction, ...)` e o `boundThisNoArgsFunctionCall` (variante otimizada do
//!   JIT, sem efeito observável) não existem: todo `JSBoundFunction` usa `boundFunctionCall`.
//! - `JSGlobalObject::boundFunctionStructure()` não existe: a `Structure` vem do cache do
//!   `FunctionRareData` do alvo, quando ele é `JSFunction`, ou nasce nova. `StructureCache` não é consultado.
//! - `m_isTainted`/`SourceCode` (taint de origem) e `boundArgsCopy` (precisa de `constructEmptyArray`)
//!   não existem. `visitChildren` some.
//! - `JSObject::hasInstance` lê o frame do topo (`vm.topCallFrame`) para o texto-fonte dos erros de
//!   `instanceof`; aqui o chamador o passa como `site` ([`object_has_instance_with_value`]).

use std::cell::{Cell, RefCell};

use crate::interpreter::call_frame::NativeCallFrame;
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::call_data::{call, construct, get_call_data, get_construct_data};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::exception_helpers::{
    create_invalid_instanceof_parameter_error_has_instance_value_not_function, create_invalid_instanceof_parameter_error_not_function,
    create_not_a_constructor_error, SourceSite,
};
use crate::runtime::host_call::throw_thrown;
use crate::runtime::host_function_support::{default_has_instance, ObjectRef};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{JSFunction, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::{js_empty_string, js_string, JSStringRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, IMPLEMENTS_DEFAULT_HAS_INSTANCE};
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::operations::js_string_concat_strings;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::throw_scope::{throw_exception, throw_vm_exception, ThrowScope};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo JSBoundFunction::s_info`.
pub static JS_BOUND_FUNCTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// Os campos de `class JSBoundFunction : public JSFunction`.
pub struct JSBoundFunction {
    /// `m_targetFunction`: o `JSObject*` (sempre célula chamável).
    target_function: JSValue,
    /// `m_boundThis`.
    bound_this: JSValue,
    /// `m_boundArgs` (`m_boundArgsLength` é o tamanho do `Vec`).
    bound_args: Vec<JSValue>,
    /// `m_nameMayBeNull`.
    name_may_be_null: RefCell<Option<JSStringRef>>,
    /// `m_length`: `NaN` (PNaN) é "ainda não calculado".
    length: Cell<f64>,
    /// `m_canConstruct`: `None` é `TriState::Indeterminate`.
    can_construct: Cell<Option<bool>>,
}

impl JSBoundFunction {
    /// `visitChildren`: o alvo, o `this` e os argumentos ligados.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.target_function);
        marker.value(self.bound_this);
        marker.values(&self.bound_args);
    }
    /// `StructureFlags = Base::StructureFlags & ~ImplementsDefaultHasInstance`.
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS & !IMPLEMENTS_DEFAULT_HAS_INSTANCE;

    /// `create(vm, globalObject, targetFunction, boundThis, args, length, nameMayBeNull, source)`: `None`
    /// é o `nullptr` (o `source` do taint não existe). `length` NaN adia o cálculo, e `name` vazio
    /// também (só o `bind` que pode assumir `name`/`length` originais faz isso).
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        target_function: JSValue,
        bound_this: JSValue,
        args: &[JSValue],
        length: f64,
        name_may_be_null: Option<JSStringRef>,
    ) -> Option<JSFunctionRef> {
        let target = target_function.as_object();
        let executable = vm.get_host_function_with_intrinsic(
            bound_function_call,
            ImplementationVisibility::Private,
            Intrinsic::NoIntrinsic,
            bound_function_construct,
            0,
            &WtfString::default(),
        );
        let structure = get_bound_function_structure(vm, global_object, &target)?;
        let bound = JSBoundFunction {
            target_function,
            bound_this,
            bound_args: args.to_vec(),
            name_may_be_null: RefCell::new(name_may_be_null),
            length: Cell::new(length),
            can_construct: Cell::new(None),
        };
        Some(JSFunction::create_bound(vm, executable, structure, bound))
    }

    /// `targetFunction()`.
    pub fn target_function(&self) -> JSValue {
        self.target_function
    }

    /// `boundThis()`.
    pub fn bound_this(&self) -> JSValue {
        self.bound_this
    }

    /// `forEachBoundArg`: os argumentos ligados, em ordem.
    pub fn bound_args(&self) -> &[JSValue] {
        &self.bound_args
    }

    /// `boundArgsLength()`.
    pub fn bound_args_length(&self) -> usize {
        self.bound_args.len()
    }

    /// `nameMayBeNull()`.
    pub fn name_may_be_null(&self) -> Option<JSStringRef> {
        self.name_may_be_null.borrow().clone()
    }

    /// `name(vm)`: o `globalObject` é o `realm()` do `nameSlow`.
    pub fn name(&self, vm: &VM, global_object: &JSGlobalObject) -> JSStringRef {
        if let Some(name) = self.name_may_be_null() {
            return name;
        }
        self.name_slow(vm, global_object)
    }

    /// `nameSlow(vm)`: a exceção que `originalName` ou a concatenação lançam é descartada, como no C++
    /// (`DeferTerminationForAWhile` + `scope.clearException()`).
    fn name_slow(&self, vm: &VM, global_object: &JSGlobalObject) -> JSStringRef {
        let mut nesting_count = 0usize;
        let mut cursor = self.target_function;
        let mut terminal: JSStringRef;
        loop {
            let function = cursor.as_js_function().expect("alvo de JSBoundFunction que não é JSFunction (o nome seria materializado cedo)");
            match function.as_bound_function() {
                None => {
                    terminal = match function.original_name(global_object) {
                        Some(name) => name,
                        None => {
                            vm.clear_exception();
                            js_empty_string(vm)
                        }
                    };
                    break;
                }
                Some(bound) => {
                    nesting_count += 1;
                    if let Some(name) = bound.name_may_be_null() {
                        terminal = name;
                        break;
                    }
                    cursor = bound.target_function;
                }
            }
        }

        if nesting_count != 0 {
            let prefix = js_string(vm, &WtfString::from_latin1(&b"bound ".repeat(nesting_count)));
            terminal = js_string_concat_strings(vm, &prefix, &terminal).unwrap_or_else(|| js_empty_string(vm));
        }

        *self.name_may_be_null.borrow_mut() = Some(terminal.clone());
        terminal
    }

    /// `length(vm)`.
    pub fn length(&self, vm: &VM) -> f64 {
        let length = self.length.get();
        if length.is_nan() {
            return self.length_slow(vm);
        }
        length
    }

    /// `lengthSlow(vm)`.
    fn length_slow(&self, vm: &VM) -> f64 {
        let mut length;
        let mut num_bound_args = self.bound_args_length();
        let mut cursor = self.target_function;
        loop {
            let function = cursor.as_js_function().expect("alvo de JSBoundFunction que não é JSFunction (o length seria materializado cedo)");
            match function.as_bound_function() {
                None => {
                    length = function.original_length(vm);
                    break;
                }
                Some(bound) => {
                    let bound_length = bound.length.get();
                    if !bound_length.is_nan() {
                        length = bound_length;
                        break;
                    }
                    num_bound_args += bound.bound_args_length();
                    cursor = bound.target_function;
                }
            }
        }
        if length > num_bound_args as f64 {
            length -= num_bound_args as f64;
        } else {
            length = 0.0;
        }
        self.length.set(length);
        length
    }

    /// `canConstruct()`.
    pub fn can_construct(&self) -> bool {
        match self.can_construct.get() {
            Some(result) => result,
            None => self.can_construct_slow(),
        }
    }

    /// `canConstructSlow()`.
    fn can_construct_slow(&self) -> bool {
        let mut cursor = self.target_function;
        loop {
            let bound = cursor.as_js_function().and_then(|function| function.as_bound_function().map(|bound| bound.target_and_state()));
            match bound {
                None => {
                    let result = !get_construct_data(cursor).is_none();
                    self.can_construct.set(Some(result));
                    return result;
                }
                Some((target, state)) => {
                    if let Some(result) = state {
                        self.can_construct.set(Some(result));
                        return result;
                    }
                    cursor = target;
                }
            }
        }
    }

    /// O `m_targetFunction` e o `m_canConstruct` de um elo da cadeia, lidos de uma vez.
    fn target_and_state(&self) -> (JSValue, Option<bool>) {
        (self.target_function, self.can_construct.get())
    }

    /// `customHasInstance(object, globalObject, value)`: `None` com a exceção pendente.
    pub fn custom_has_instance(&self, global_object: &JSGlobalObject, value: JSValue) -> Option<bool> {
        object_has_instance(global_object, self.target_function, value)
    }
}

/// `getBoundFunctionStructure(vm, globalObject, targetFunction)`: `None` com a exceção pendente (o
/// `getPrototype` de um `Proxy` lançou).
fn get_bound_function_structure(vm: &VM, global_object: &JSGlobalObject, target: &ObjectRef) -> Option<StructureRef> {
    let target_function = target.as_value().as_js_function();
    let prototype = match target.get_prototype(global_object) {
        Ok(prototype) => prototype,
        Err(thrown) => {
            throw_thrown(global_object, thrown);
            return None;
        }
    };

    // We only cache the structure of the bound function if the bindee is a JSFunction since there
    // isn't any good place to put the structure on Internal Functions.
    if let Some(function) = &target_function {
        if let Some(structure) = function.ensure_rare_data(vm).get_bound_function_structure() {
            let same_realm = structure.realm().is_some_and(|realm| std::ptr::eq(&*realm, global_object));
            if structure.stored_prototype() == prototype && same_realm {
                return Some(structure);
            }
        }
    }

    let result = Structure::create(
        vm,
        Some(global_object),
        prototype,
        TypeInfo::new(JSType::JSFunctionType, JSBoundFunction::STRUCTURE_FLAGS),
        &JS_BOUND_FUNCTION_S_INFO,
    );

    if let Some(function) = &target_function {
        function.ensure_rare_data(vm).set_bound_function_structure(vm, result.clone());
    }
    Some(result)
}

/// `JSObject::hasInstance(globalObject, value)` para o objeto chamável `target`: `None` com a exceção
/// pendente.
pub fn object_has_instance(global_object: &JSGlobalObject, target: JSValue, value: JSValue) -> Option<bool> {
    let vm = global_object.vm();
    let object = target.as_object();
    let has_instance_name = PropertyName::from_identifier(&vm.property_names.has_instance_symbol);
    let has_instance_value = object.get(global_object, &has_instance_name);
    if vm.exception().is_some() {
        return None;
    }
    object_has_instance_with_value(global_object, target, value, has_instance_value, None)
}

/// `JSObject::hasInstance(globalObject, value, hasInstanceValue)`: `None` com a exceção pendente. `site` é
/// o frame do topo de onde os `createInvalidInstanceofParameterError*` tiram o texto-fonte; o
/// `JSObject::hasInstance` do C++ o lê do `vm.topCallFrame`, e quem chama do LLInt o passa (`None` quando
/// o chamador é uma função nativa sem instrução associada).
pub fn object_has_instance_with_value(
    global_object: &JSGlobalObject,
    target: JSValue,
    value: JSValue,
    has_instance_value: JSValue,
    site: Option<&dyn SourceSite>,
) -> Option<bool> {
    let vm = global_object.vm();
    let object = target.as_object();
    let has_instance_name = PropertyName::from_identifier(&vm.property_names.has_instance_symbol);

    // `globalObject->functionProtoHasInstanceSymbolFunction()` é o valor que `Function.prototype` guarda.
    let function_proto_has_instance = global_object.function_prototype().get_direct_by_name(vm, &has_instance_name);
    if !has_instance_value.is_undefined_or_null() && has_instance_value != function_proto_has_instance {
        let call_data = get_call_data(has_instance_value);
        if call_data.is_none() {
            let mut scope = ThrowScope::new(vm);
            let error = create_invalid_instanceof_parameter_error_has_instance_value_not_function(global_object, target, site);
            throw_exception(global_object, &mut scope, error);
            return None;
        }
        let result = call(global_object, has_instance_value, &call_data, target, &[value]);
        return match result {
            Ok(result) => Some(result.to_boolean()),
            Err(failure) => {
                debug_assert!(matches!(failure, LLIntFailure::Thrown));
                None
            }
        };
    }

    let type_info = object.structure().type_info();
    if type_info.implements_default_has_instance() {
        let prototype = object.get(global_object, &PropertyName::from_identifier(&vm.property_names.prototype));
        if vm.exception().is_some() {
            return None;
        }
        return default_has_instance(global_object, value, prototype);
    }
    if type_info.implements_has_instance() {
        // `File` divide o protótipo com `Blob`: o `customHasInstance` dele olha o estado, não a cadeia.
        if let Some(result) = crate::runtime::file::file_has_instance(target, value) {
            return Some(result);
        }
        let function = target.as_js_function().expect("ImplementsHasInstance sem default só existe em JSBoundFunction e File");
        let bound = function.as_bound_function().expect("customHasInstance de função que não é JSBoundFunction");
        return bound.custom_has_instance(global_object, value);
    }

    let mut scope = ThrowScope::new(vm);
    let error = create_invalid_instanceof_parameter_error_not_function(global_object, target, site);
    throw_exception(global_object, &mut scope, error);
    None
}

/// O `Err` de um `call`/`construct` nativo: `Thrown` é a exceção pendente (o `JSValue()` do C++); as
/// demais falhas são lacunas do interpretador, que um `EncodedJSValue` não consegue carregar.
fn encode_call_result(result: LLIntResult<JSValue>) -> EncodedJSValue {
    match result {
        Ok(value) => value.encode(),
        Err(LLIntFailure::Thrown) => JSValue::empty().encode(),
        Err(_) => panic!("chamada do alvo de JSBoundFunction falhou por lacuna do interpretador (LLIntFailure que não é Thrown)"),
    }
}

/// `uncheckedDowncast<JSBoundFunction>(callFrame->jsCallee())`: a `JSFunction` do callee.
fn callee_function(call_frame: &NativeCallFrame<'_>) -> JSFunctionRef {
    JSValue::from_cell(call_frame.js_callee()).as_js_function().expect("callee de boundFunctionCall que não é JSFunction")
}

/// Os argumentos finais: os ligados, depois os do chamador.
fn final_args(bound: &JSBoundFunction, call_frame: &NativeCallFrame<'_>) -> Vec<JSValue> {
    let mut args = Vec::with_capacity(bound.bound_args_length() + call_frame.argument_count());
    args.extend_from_slice(bound.bound_args());
    args.extend(call_frame.arguments_span());
    args
}

/// `JSC_DEFINE_HOST_FUNCTION(boundFunctionCall)`.
pub fn bound_function_call(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let function = callee_function(call_frame);
    let bound = function.as_bound_function().expect("boundFunctionCall com callee que não é JSBoundFunction");
    let args = final_args(bound, call_frame);

    let target = bound.target_function();
    let call_data = get_call_data(target);
    debug_assert!(!call_data.is_none());
    encode_call_result(call(global_object, target, &call_data, bound.bound_this(), &args))
}

/// `JSC_DEFINE_HOST_FUNCTION(boundFunctionConstruct)`.
pub fn bound_function_construct(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let function = callee_function(call_frame);
    let bound = function.as_bound_function().expect("boundFunctionConstruct com callee que não é JSBoundFunction");
    let callee = function.as_value();

    let target = bound.target_function();
    let construct_data = get_construct_data(target);
    if construct_data.is_none() {
        let mut scope = ThrowScope::new(global_object.vm());
        return throw_vm_exception(global_object, &mut scope, create_not_a_constructor_error(global_object, callee)).encode();
    }

    let args = final_args(bound, call_frame);
    let mut new_target = call_frame.this_value();
    if new_target == callee {
        new_target = target;
    }
    encode_call_result(construct(global_object, target, &construct_data, &args, new_target))
}
