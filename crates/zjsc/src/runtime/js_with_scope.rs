//! Porte de `runtime/JSWithScope.h` e `JSWithScope.cpp`.
//!
//! DIVERGÊNCIAS (heap ausente, camada 3; mesmo padrão de `js_lexical_environment.rs`):
//!
//! - `m_object` é o `JSValue` do objeto do `with` (um `cell_id` do registro central, que o mantém vivo,
//!   sem coleta). `object()` o resolve como `JSObjectHandle`; só devolve `Some` para células que o
//!   `cell_registry` reconhece como objeto (`CellEntry::as_js_object`).
//! - `JSScope::objectAtScope` devolve `m_object` no C++. `JSScopeRef` só carrega escopos, então o
//!   resultado de `objectAtScope` e de `JSScope::resolve` para um escopo `with` é o próprio
//!   `JSWithScope`, e quem consulta propriedades (`JSScopeRef::has_property`, `isUnscopable`) o
//!   desembrulha por `object()`.
//! - `JSWithScope` não é um `JSSymbolTableObject`: `JSScopeRef::symbol_table()` devolve `None`.
//! - `visitChildren` some com o GC.

use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSObject, JSObjectHandle};
use crate::runtime::js_scope::{JSScope, JSScopeRef, JS_SCOPE_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSWithScope::s_info`.
pub static JS_WITH_SCOPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "WithScope", parent_class: Some(&JS_SCOPE_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSWithScope final : public JSScope`.
#[derive(Debug)]
pub struct JSWithScope {
    base: JSScope,
    /// `m_object`: o `JSValue` do `JSObject*` (sempre uma célula de objeto).
    object: JSValue,
    /// `true` para o `StrictEvalActivation` (ver `create_strict_eval_activation`).
    strict_eval_activation: bool,
}

/// O `JSWithScope*`.
pub type JSWithScopeRef = Rc<JSWithScope>;

impl std::ops::Deref for JSWithScope {
    type Target = JSScope;

    fn deref(&self) -> &JSScope {
        &self.base
    }
}

impl JSWithScope {
    /// `visitChildren`: o objeto do `with`.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.object);
    }

    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSScope::STRUCTURE_FLAGS;

    /// `JSWithScope(VM&, Structure*, JSObject*, JSScope* next)`.
    fn new(vm: &VM, structure: StructureRef, object: JSValue, next: Option<JSScopeRef>) -> JSWithScope {
        debug_assert!(object.is_cell(), "o objeto de um escopo with é uma célula");
        JSWithScope { base: JSScope::new(vm, structure, next), object, strict_eval_activation: false }
    }

    /// `StrictEvalActivation::create(vm, globalObject->strictEvalActivationStructure(), next)`.
    ///
    /// DIVERGÊNCIA: o `StrictEvalActivation` do C++ é um `JSScope` sem tabela de símbolos cujo `var` de
    /// `eval` vira propriedade dinâmica (`put`), e cujo `deleteProperty` devolve falso. Aqui é um
    /// `JSWithScope` com a marca `strict_eval_activation` sobre um objeto de protótipo nulo (que faz o papel
    /// das propriedades dinâmicas), para não abrir uma variante nova em `JSScopeRef` (mais de 80 `match`).
    /// A marca desliga o que é só do `with`: `isWithScope()` e `type()` (`StrictEvalActivationType`),
    /// `Symbol.unscopables` (`is_unscopable`) e o `delete` (falso, como no C++).
    pub fn create_strict_eval_activation(vm: &VM, global_object: &JSGlobalObject, next: Option<JSScopeRef>) -> JSWithScopeRef {
        let structure = JSFinalObject::create_structure(vm, None, JSValue::null(), JSFinalObject::DEFAULT_INLINE_CAPACITY);
        let object = JSFinalObject::create(vm, &structure).as_value();
        let structure = global_object.with_scope_structure();
        let mut activation = JSWithScope::new(vm, structure, object, next);
        activation.strict_eval_activation = true;
        let activation = Rc::new(activation);
        cell_registry::set(activation.cell_id(), CellEntry::Scope(JSScopeRef::WithScope(Rc::clone(&activation))));
        activation
    }

    /// `true` se este é o `StrictEvalActivation` (e não um escopo `with`).
    pub fn is_strict_eval_activation(&self) -> bool {
        self.strict_eval_activation
    }

    /// `create(vm, globalObject, next, object)`: usa a `withScopeStructure()` do `JSGlobalObject`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, next: Option<JSScopeRef>, object: JSValue) -> JSWithScopeRef {
        let structure = global_object.with_scope_structure();
        let with_scope = Rc::new(JSWithScope::new(vm, structure, object, next));
        cell_registry::set(with_scope.cell_id(), CellEntry::Scope(JSScopeRef::WithScope(Rc::clone(&with_scope))));
        with_scope
    }

    /// `createStructure(vm, globalObject, proto)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, proto: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            proto,
            TypeInfo::new(JSType::WithScopeType, JSWithScope::STRUCTURE_FLAGS),
            &JS_WITH_SCOPE_S_INFO,
        )
    }

    /// `object()` como `JSValue`.
    pub fn object_value(&self) -> JSValue {
        self.object
    }

    /// `object()`: `None` se a célula não é um objeto conhecido pelo registro (invariante quebrada).
    pub fn object(&self) -> Option<JSObjectHandle> {
        JSObject::from_value(&self.object)
    }
}
