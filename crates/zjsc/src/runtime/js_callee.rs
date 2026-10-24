//! Porte de `runtime/JSCallee.h` e `JSCallee.cpp`.
//!
//! DIVERGÊNCIAS (heap ausente, camada 3): o `JSValue::Cell(usize)` de um callee guarda o `cell_id` do
//! registro central (`runtime::cell_registry`, `CellEntry::Callee` ou `CellEntry::Function`), que mantém o
//! `JSCallee` ou `JSFunction` vivo, sem coleta. `JSFunction` é subclasse de `JSCallee`, então
//! `JSCalleeRef` é o `JSCallee*` polimórfico. `visitChildren` some com o GC;
//! `offsetOfScopeChain` serve ao layout de memória do LLInt e não existe.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_function::JSFunction;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_type::JSType;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::js_type_info::{TypeInfo, IMPLEMENTS_DEFAULT_HAS_INSTANCE, IMPLEMENTS_HAS_INSTANCE};
use crate::runtime::js_value::JSValue;
use crate::runtime::vm::VM;

/// `const ClassInfo JSCallee::s_info`.
pub static JS_CALLEE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Callee", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `JSCallee*`: o `JSCallee` puro ou um `JSFunction`.
#[derive(Clone)]
pub enum JSCalleeRef {
    Callee(Rc<JSCallee>),
    Function(Rc<JSFunction>),
}

/// `class JSCallee : public JSNonFinalObject`.
#[derive(Debug)]
pub struct JSCallee {
    base: JSNonFinalObject,
    cell_id: usize,
    /// `m_scope`.
    scope: RefCell<Option<JSScopeRef>>,
}

impl std::ops::Deref for JSCallee {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSCallee {
    /// `StructureFlags = Base::StructureFlags | ImplementsHasInstance | ImplementsDefaultHasInstance`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | IMPLEMENTS_HAS_INSTANCE | IMPLEMENTS_DEFAULT_HAS_INSTANCE;

    /// `JSCallee(VM&, JSGlobalObject*, Structure*)` e `JSCallee(VM&, JSScope*, Structure*)`: o
    /// escopo inicial (o global object é um `JSScope`). Reserva o `cell_id`; quem constrói a célula
    /// completa o cadastro com `cell_registry::set`.
    pub(crate) fn new(vm: &VM, scope: JSScopeRef, structure: StructureRef) -> JSCallee {
        let cell_id = cell_registry::reserve();
        let base = JSNonFinalObject::new(vm, structure);
        // A base `JSObject` é a mesma célula: `as_value` depende do `cell_id` (vale para `JSFunction` também).
        base.set_cell_id(cell_id);
        JSCallee { base, cell_id, scope: RefCell::new(Some(scope)) }
    }

    /// `create(vm, globalObject, scope)`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, scope: JSScopeRef) -> Rc<JSCallee> {
        let callee = Rc::new(JSCallee::new(vm, scope, global_object.callee_structure()));
        cell_registry::set(callee.cell_id, CellEntry::Callee(Rc::clone(&callee)));
        callee
    }

    /// `scope()`.
    pub fn scope(&self) -> Option<JSScopeRef> {
        self.scope.borrow().clone()
    }

    /// `setScope(vm, scope)`.
    pub fn set_scope(&self, scope: Option<JSScopeRef>) {
        *self.scope.borrow_mut() = scope;
    }

    /// `realm()` (`JSCell::realm()`): o `m_realm` da estrutura, que todo callee tem.
    pub fn realm(&self) -> JSGlobalObjectRef {
        self.structure().realm().expect("a estrutura de um callee sempre tem realm")
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// Procura o callee pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSCalleeRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::Callee(callee)) => Some(JSCalleeRef::Callee(callee)),
            Some(CellEntry::Function(function)) => Some(JSCalleeRef::Function(function)),
            _ => None,
        }
    }

    /// `createStructure(vm, globalObject, prototype)` (JSCalleeInlines: `Structure::create` com
    /// `TypeInfo(JSCalleeType, StructureFlags)`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::JSCalleeType, JSCallee::STRUCTURE_FLAGS),
            &JS_CALLEE_S_INFO,
        )
    }
}

impl JSCalleeRef {
    /// Acesso à base `JSCallee`.
    pub fn callee(&self) -> &JSCallee {
        match self {
            JSCalleeRef::Callee(callee) => callee,
            JSCalleeRef::Function(function) => function,
        }
    }

    pub fn cell_id(&self) -> usize {
        self.callee().cell_id()
    }

    /// `JSValue(JSCell*)`: o callee como célula do registro central.
    pub fn into_js_value(&self) -> JSValue {
        JSValue::from_cell(self.cell_id())
    }

    /// `scope()`.
    pub fn scope(&self) -> Option<JSScopeRef> {
        self.callee().scope()
    }

    /// `realm()`.
    pub fn realm(&self) -> JSGlobalObjectRef {
        self.callee().realm()
    }
}
