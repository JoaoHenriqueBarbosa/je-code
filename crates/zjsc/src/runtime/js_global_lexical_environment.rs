//! Porte de `runtime/JSGlobalLexicalEnvironment.h`, `.cpp` e `JSGlobalLexicalEnvironmentInlines.h`.
//!
//! DIVERGÊNCIAS: `getOwnPropertySlot` e `put` (escrevem em `PropertySlot`, lançam por `ThrowScope`)
//! entram com o `JSObject`; a lógica de tabela é `symbol_table_get`/`symbol_table_put` de
//! `js_symbol_table_object`, que elas chamam (`put` com `shouldThrow = true` e
//! `ignoreReadOnlyErrors = slot.isInitialization()`). `destroy` some com o GC.

use std::rc::Rc;

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_segmented_variable_object::{JSSegmentedVariableObject, JS_SEGMENTED_VARIABLE_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::js_null;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::symbol_table::ScopeType;
use crate::runtime::js_type_info::{TypeInfo, OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_PUT};
use crate::runtime::vm::VM;
use crate::wtf::text::string_impl::UniquedKey;

/// `const ClassInfo JSGlobalLexicalEnvironment::s_info`.
pub static JS_GLOBAL_LEXICAL_ENVIRONMENT_S_INFO: ClassInfo = ClassInfo {
    class_name: "JSGlobalLexicalEnvironment",
    parent_class: Some(&JS_SEGMENTED_VARIABLE_OBJECT_S_INFO),
    static_prop_hash_table: None, inherits_js_type_range: None,
};

/// `class JSGlobalLexicalEnvironment final : public JSSegmentedVariableObject`.
#[derive(Debug)]
pub struct JSGlobalLexicalEnvironment {
    base: JSSegmentedVariableObject,
}

pub type JSGlobalLexicalEnvironmentRef = Rc<JSGlobalLexicalEnvironment>;

impl std::ops::Deref for JSGlobalLexicalEnvironment {
    type Target = JSSegmentedVariableObject;

    fn deref(&self) -> &JSSegmentedVariableObject {
        &self.base
    }
}

impl JSGlobalLexicalEnvironment {
    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnPropertySlot | OverridesPut`.
    pub const STRUCTURE_FLAGS: u32 = JSSegmentedVariableObject::STRUCTURE_FLAGS | OVERRIDES_GET_OWN_PROPERTY_SLOT | OVERRIDES_PUT;

    /// `create(vm, structure, parentScope)`.
    pub fn create(vm: &VM, structure: StructureRef, parent_scope: Option<JSScopeRef>) -> JSGlobalLexicalEnvironmentRef {
        let result = Rc::new(JSGlobalLexicalEnvironment { base: JSSegmentedVariableObject::new(vm, structure, parent_scope) });
        cell_registry::set(result.cell_id(), CellEntry::Scope(JSScopeRef::GlobalLexicalEnvironment(Rc::clone(&result))));
        result.base.finish_creation(vm);
        result.symbol_table().borrow_mut().set_scope_type(ScopeType::GlobalLexicalScope);
        result
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.symbol_table().borrow().size() == 0
    }

    /// `isConstVariable(UniquedStringImpl*)`.
    pub fn is_const_variable(&self, key: &UniquedKey) -> bool {
        let entry = self.symbol_table().borrow().get(key);
        debug_assert!(!entry.is_null());
        entry.is_read_only()
    }

    /// `createStructure(vm, globalObject)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            js_null(),
            TypeInfo::new(JSType::GlobalLexicalEnvironmentType, JSGlobalLexicalEnvironment::STRUCTURE_FLAGS),
            &JS_GLOBAL_LEXICAL_ENVIRONMENT_S_INFO,
        )
    }
}
