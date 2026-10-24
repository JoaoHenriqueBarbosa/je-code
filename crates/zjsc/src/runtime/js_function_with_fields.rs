//! Porte de `runtime/JSFunctionWithFields.{h,cpp}`: uma `JSFunction` de host que carrega dois campos
//! internos (`m_internalFields`), a base das funções de resolução de `JSPromise`, das funções de
//! elemento de `Promise.all`/`allSettled`/`any` e das funções de `finally`.
//!
//! DIVERGÊNCIAS:
//! - `JSFunctionWithFields` é subclasse de `JSFunction` no C++, com `ClassInfo` e `Structure`
//!   (`functionWithFieldsStructure()`) próprios. Como `JSBoundFunction`, ela compartilha o
//!   `CellEntry::Function`: os campos moram em `JSFunction::fields` (vazio nas funções comuns), e
//!   `dynamicDowncast<JSFunctionWithFields>` é "a função tem campos". A `Structure` é a
//!   `hostFunctionStructure` do global (o `ClassInfo` a mais não é observável).
//! - `create(vm, globalObject, NativeExecutable*)` recebe os dados do `getHostFunction` que o `VM` usa
//!   para cada executável (função, comprimento, nome vazio), e o `NativeExecutable` vem de
//!   `vm.get_host_function_with_intrinsic`, que já o compartilha por função nativa.
//! - Sem GC, `visitChildren` e as barreiras de escrita somem.

use std::cell::Cell;

use crate::runtime::host_call::HostCall;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction, JSFunctionRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_promise_host::FunctionField;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::empty_string;

/// `class JSFunctionWithFields final : public JSFunction`: espaço de nomes de `create` e dos campos.
pub struct JSFunctionWithFields;

impl JSFunctionWithFields {
    /// `numberOfInternalFields`.
    pub const NUMBER_OF_INTERNAL_FIELDS: usize = 2;

    /// `create(vm, globalObject, executable)`: a função de host de `function` (comprimento `length`,
    /// nome vazio, `callHostFunctionAsConstructor`) com os dois campos vazios (`JSValue()`).
    pub fn create(vm: &VM, global_object: &JSGlobalObject, length: u32, function: NativeFunction) -> JSFunctionRef {
        let created = JSFunction::create_native(
            vm,
            global_object,
            length,
            &empty_string(),
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            call_host_function_as_constructor,
        );
        let initialized = created.fields.set([Cell::new(JSValue::empty()), Cell::new(JSValue::empty())]);
        debug_assert!(initialized.is_ok());
        created
    }

    /// `dynamicDowncast<JSFunctionWithFields>(value)`: a função de `value`, se ela tem campos.
    pub fn from_value(value: &JSValue) -> Option<JSFunctionRef> {
        value.as_js_function().filter(|function| function.fields.get().is_some())
    }

    /// `uncheckedDowncast<JSFunctionWithFields>(callFrame->jsCallee())`.
    pub fn callee(call: &HostCall) -> JSFunctionRef {
        JSFunctionWithFields::from_value(&JSValue::from_cell(call.callee()))
            .expect("uncheckedDowncast<JSFunctionWithFields>(jsCallee()) em função sem campos")
    }

    /// `getField(index)`: o `m_internalFields[index]` (`JSValue()` se ainda não foi posto).
    pub fn get_field(function: &JSFunction, field: FunctionField) -> JSValue {
        JSFunctionWithFields::fields_of(function)[field.0 as usize].get()
    }

    /// `setField(vm, index, value)`.
    pub fn set_field(function: &JSFunction, field: FunctionField, value: JSValue) {
        JSFunctionWithFields::fields_of(function)[field.0 as usize].set(value);
    }

    /// `fields()`: invariante de `uncheckedDowncast<JSFunctionWithFields>`.
    fn fields_of(function: &JSFunction) -> &[Cell<JSValue>; 2] {
        function.fields.get().expect("uncheckedDowncast<JSFunctionWithFields> em função sem campos")
    }
}
