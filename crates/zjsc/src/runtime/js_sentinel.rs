//! Porte de `runtime/JSSentinel.{h,cpp}` e `JSSentinelInlines.h`: a célula marcadora só de identidade que o
//! protocolo de iteração rápida usa (`vm.fastAsyncGeneratorSentinel()`); o consumidor compara por
//! identidade.
//!
//! DIVERGÊNCIA: `JSSentinel` é um `JSCell` puro no C++; o porte não tem `JSCell` fora de objeto, então a
//! instância é um `JSObject` sem propriedades, com a `Structure` de `JSType::SentinelType` e o `ClassInfo`
//! `Sentinel`. Ela nunca escapa para o código do usuário.

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_null, JSValue};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSSentinel::s_info = { "Sentinel"_s, nullptr, ... }`.
pub static JS_SENTINEL_S_INFO: ClassInfo = ClassInfo { class_name: "Sentinel", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSSentinel final : public JSCell`: espaço de nomes de `createStructure` e `create`.
pub struct JSSentinel;

impl JSSentinel {
    /// `createStructure(vm, globalObject, prototype)`: `TypeInfo(SentinelType, StructureFlags)`; o
    /// `StructureIsImmortal` é do GC e não existe aqui.
    pub fn create_structure(vm: &VM, prototype: JSValue) -> StructureRef {
        Structure::create(vm, None, prototype, TypeInfo::new(JSType::SentinelType, 0), &JS_SENTINEL_S_INFO)
    }

    /// `create(vm, structure)`: a célula como `JSValue`.
    pub fn create(vm: &VM, structure: &StructureRef) -> JSValue {
        JSObject::allocate(vm, structure).as_value()
    }

    /// `JSSentinel::createStructure(*this, nullptr, jsNull())` seguido de `JSSentinel::create(*this,
    /// sentinelStructure)`, como `VM::VM` cria cada sentinela.
    pub fn create_for_vm(vm: &VM) -> JSValue {
        JSSentinel::create(vm, &JSSentinel::create_structure(vm, js_null()))
    }
}
