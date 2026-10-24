//! Tradução de `runtime/GetterSetter.h`, `GetterSetterInlines.h` e `GetterSetter.cpp`: o par de funções
//! (getter, setter) de uma propriedade de accessor, guardado como valor da propriedade (com o atributo
//! `Accessor`) no armazenamento do objeto.
//!
//! DIVERGÊNCIAS (sem heap e sem GC, camada 3):
//!
//! - É uma célula do `cell_registry` (`CellEntry::GetterSetter`), como `JSTemplateObjectDescriptor`:
//!   valor imutável por `Rc`, `cell_id` do registro. O `JSType` (`GetterSetterType`) vem da variante,
//!   o papel do cabeçalho `JSCell`; a `vm.getterSetterStructure` não existe ainda.
//! - O C++ guarda sempre um `JSObject*`: na falta de função usa `globalObject->nullGetterFunction()` /
//!   `nullSetterFunction()` (`NullGetterFunction`/`NullSetterFunction`, `JSFunction`s do realm que
//!   devolvem `undefined` e lançam o erro de escrita). Elas ainda não foram portadas, então `None` é o
//!   papel delas: `is_getter_null`/`is_setter_null` são `None`, e `GetterSetter::create` não precisa do
//!   `JSGlobalObject`. A semântica observável (`isGetterNull`, `isSetterNull`, `callSetter` com setter
//!   nulo lançando `ReadonlyPropertyWriteError`) é a mesma.
//! - O `JSObject*` do getter e do setter é guardado como `JSValue` de célula chamável: `ObjectRef` não alcança
//!   `InternalFunction`, função bound nem Proxy, e a chamada passa por `get_call_data`/`call`.
//! - `callGetter` e `callSetter` com função de verdade chamam `call` (`call_data.rs`) sobre o realm do
//!   próprio getter/setter. O `call` reentra no `Interpreter` do `VM`; o único `PutError::Unported` que
//!   sai daqui é a propagação de um opcode ou caminho ainda sem handler dentro do getter. `visitChildren`,
//!   `offsetOfGetter`/`offsetOfSetter` (JIT) e os métodos estáticos que o C++
//!   marca `RELEASE_ASSERT_NOT_REACHED` não têm o que traduzir.

use std::rc::Rc;

use crate::llint::LLIntFailure;
use crate::runtime::call_data::{call, get_call_data, realm_for_call};
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::error_messages::READONLY_PROPERTY_WRITE_ERROR;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_object::PutError;
use crate::runtime::js_value::JSValue;
use crate::runtime::vm::VM;

/// `class GetterSetter`.
#[derive(Debug)]
pub struct GetterSetter {
    /// `m_getter`: `None` é o `nullGetterFunction`. Qualquer objeto chamável (função, `InternalFunction`,
    /// função bound, Proxy de função), como o `JSObject*` do C++.
    getter: Option<JSValue>,
    /// `m_setter`: `None` é o `nullSetterFunction`.
    setter: Option<JSValue>,
    cell_id: usize,
}

/// A célula como o resto do porte a enxerga.
pub type GetterSetterRef = Rc<GetterSetter>;

impl GetterSetter {
    /// `visitChildren`: o getter e o setter.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.getter);
        marker.value(self.setter);
    }

    /// `create(vm, globalObject, JSObject* getter, JSObject* setter)`.
    pub fn create(vm: &VM, getter: Option<ObjectRef>, setter: Option<ObjectRef>) -> GetterSetterRef {
        GetterSetter::create_from_values(
            vm,
            getter.map_or_else(JSValue::undefined, |object| object.as_value()),
            setter.map_or_else(JSValue::undefined, |object| object.as_value()),
        )
    }

    /// `create(vm, globalObject, JSValue getter, JSValue setter)`: `undefined` ou qualquer objeto chamável.
    pub fn create_from_values(_vm: &VM, getter: JSValue, setter: JSValue) -> GetterSetterRef {
        debug_assert!(getter.is_undefined() || getter.is_cell());
        debug_assert!(setter.is_undefined() || setter.is_cell());
        let getter = if getter.is_cell() { Some(getter) } else { None };
        let setter = if setter.is_cell() { Some(setter) } else { None };
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(GetterSetter { getter, setter, cell_id });
        cell_registry::set(cell_id, CellEntry::GetterSetter(Rc::clone(&cell)));
        cell
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell` (o `uncheckedDowncast<GetterSetter>`).
    pub fn from_cell_id(cell_id: usize) -> Option<GetterSetterRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::GetterSetter(cell)) => Some(cell),
            _ => None,
        }
    }

    /// O `GetterSetter*` de um `JSValue` (`None` se não é a célula de um `GetterSetter`).
    pub fn from_value(value: &JSValue) -> Option<GetterSetterRef> {
        if !value.is_cell() {
            return None;
        }
        GetterSetter::from_cell_id(value.as_cell())
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// A célula como `JSValue` (o valor guardado na propriedade).
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.cell_id)
    }

    /// `getter()`: `None` quando o getter é o `NullGetterFunction`.
    pub fn getter(&self) -> Option<ObjectRef> {
        self.getter.as_ref().and_then(ObjectRef::from_value)
    }

    /// `setter()`: `None` quando o setter é o `NullSetterFunction`.
    pub fn setter(&self) -> Option<ObjectRef> {
        self.setter.as_ref().and_then(ObjectRef::from_value)
    }

    /// `isGetterNull()`.
    pub fn is_getter_null(&self) -> bool {
        self.getter.is_none()
    }

    /// `isSetterNull()`.
    pub fn is_setter_null(&self) -> bool {
        self.setter.is_none()
    }

    /// O getter como `JSValue` para o descritor: `undefined` se for o `NullGetterFunction`
    /// (`!accessor->isGetterNull() ? accessor->getter() : jsUndefined()`).
    pub fn getter_value_or_undefined(&self) -> JSValue {
        match self.getter {
            Some(getter) => getter,
            None => JSValue::undefined(),
        }
    }

    /// O setter como `JSValue` para o descritor (`!accessor->isSetterNull() ? accessor->setter() : jsUndefined()`).
    pub fn setter_value_or_undefined(&self) -> JSValue {
        match self.setter {
            Some(setter) => setter,
            None => JSValue::undefined(),
        }
    }

    /// `callGetter(globalObject, thisValue)`: o `NullGetterFunction` devolve `undefined`; senão chama a
    /// função (`call(globalObject, getter, callData, thisValue, ArgList())`). O `globalObject` é o
    /// realm do próprio getter (o `call` só o usa para alcançar o `VM`). Uma exceção lançada pelo getter
    /// fica pendente no `VM` e o valor é o `JSValue()` vazio, como no C++: quem chama confere o `VM`.
    pub fn call_getter(&self, this_value: JSValue) -> Result<JSValue, PutError> {
        let Some(getter_value) = self.getter else {
            return Ok(JSValue::undefined());
        };
        let call_data = get_call_data(getter_value);
        // `call` exige `callData.type != None` (ASSERT do C++): `defineProperty`, `__defineGetter__` e o
        // bytecode só guardam função chamável como getter ("Getter must be a function.").
        assert!(!call_data.is_none(), "Expected object to be callable but received CallData::Type::None");
        let global_object = realm_for_call(getter_value, &call_data);
        match call(&global_object, getter_value, &call_data, this_value, &[]) {
            Ok(value) => Ok(value),
            Err(LLIntFailure::Thrown) => Ok(JSValue::empty()),
            Err(LLIntFailure::Unported(what)) => Err(PutError::Unported(what)),
            Err(LLIntFailure::UnportedOpcode(_)) => Err(PutError::Unported("opcode sem handler dentro do getter")),
        }
    }

    /// `callSetter(globalObject, thisValue, value, shouldThrow)`. No C++ devolve `true` mesmo se o setter
    /// lançou, e o `RETURN_IF_EXCEPTION` do chamador confere o `VM`. Aqui os chamadores (`put` do `JSObject`,
    /// slow paths) não conferem, então a exceção do setter sai como `PutError::Pending`.
    pub fn call_setter(&self, this_value: JSValue, value: JSValue, should_throw: bool) -> Result<bool, PutError> {
        let Some(setter_value) = self.setter else {
            // Um atributo nativo (`CustomAccessor` do bun, sem `ReadOnly`) sem setter ignora a escrita, mesmo em modo estrito
            // (`MIMEType.prototype.essence`, medido no bun 1.4.2), marcado em `CustomAccessorFunction::ignores_put`.
            let custom = self.getter.is_some_and(|getter| {
                getter.is_cell()
                    && matches!(cell_registry::get(getter.as_cell()), Some(CellEntry::Function(function)) if function.custom_accessor.get().is_some_and(|custom| custom.ignores_put()))
            });
            if should_throw && !custom {
                return Err(PutError::TypeError(READONLY_PROPERTY_WRITE_ERROR));
            }
            return Ok(false);
        };
        let call_data = get_call_data(setter_value);
        // Mesmo ASSERT de `call`: o setter guardado é sempre chamável ("Setter must be a function.").
        assert!(!call_data.is_none(), "Expected object to be callable but received CallData::Type::None");
        let global_object = realm_for_call(setter_value, &call_data);
        match call(&global_object, setter_value, &call_data, this_value, &[value]) {
            Ok(_) => Ok(true),
            // `RETURN_IF_EXCEPTION` do `callSetter`: a exceção do setter segue pendente e o `put` falha,
            // senão o slow path devolve `Ok` e o laço só a vê (tarde) numa instrução posterior.
            Err(LLIntFailure::Thrown) => Err(PutError::Pending),
            Err(LLIntFailure::Unported(what)) => Err(PutError::Unported(what)),
            Err(LLIntFailure::UnportedOpcode(_)) => Err(PutError::Unported("opcode sem handler dentro do setter")),
        }
    }
}

