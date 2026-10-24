//! Porte de `runtime/JSSegmentedVariableObject.h` e `JSSegmentedVariableObject.cpp`.
//!
//! DIVERGÊNCIAS: o `SegmentedVector<WriteBarrier<Unknown>, 16>` existe para o endereço de uma
//! variável nunca mudar (o JIT e o `GlobalVar` do bytecode guardam o ponteiro). Aqui a variável é
//! sempre lida e escrita por `ScopeOffset` (`variable_at`/`set_variable_at`), então um `Vec` basta.
//! `findVariableIndex(void*)` e `assertVariableIsInThisObject` operam sobre endereços e só servem a
//! depuração (`CRASH()` se não acha): não se portam. `visitChildren` e `analyzeHeap` somem com o GC.

use std::cell::RefCell;

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_symbol_table_object::{JSSymbolTableObject, SymbolTableObjectVariables, JS_SYMBOL_TABLE_OBJECT_S_INFO};
use crate::runtime::js_value::JSValue;
use crate::runtime::scope_offset::ScopeOffset;
use crate::runtime::structure::StructureRef;
use crate::runtime::symbol_table::SymbolTable;
use crate::runtime::vm::VM;

/// `const ClassInfo JSSegmentedVariableObject::s_info`.
pub static JS_SEGMENTED_VARIABLE_OBJECT_S_INFO: ClassInfo = ClassInfo {
    class_name: "SegmentedVariableObject",
    parent_class: Some(&JS_SYMBOL_TABLE_OBJECT_S_INFO),
    static_prop_hash_table: None, inherits_js_type_range: None,
};

/// `class JSSegmentedVariableObject : public JSSymbolTableObject`.
#[derive(Debug)]
pub struct JSSegmentedVariableObject {
    base: JSSymbolTableObject,
    /// `m_variables`.
    variables: RefCell<Vec<JSValue>>,
}

impl std::ops::Deref for JSSegmentedVariableObject {
    type Target = JSSymbolTableObject;

    fn deref(&self) -> &JSSymbolTableObject {
        &self.base
    }
}

impl JSSegmentedVariableObject {
    /// `visitChildren`: as variáveis.
    pub(crate) fn visit_variables(&self, marker: &mut crate::runtime::gc::Marker) {
        match self.variables.try_borrow() {
            Ok(variables) => marker.values(&variables),
            Err(_) => marker.abort(),
        }
    }

    /// `StructureFlags` (herdado sem acréscimo).
    pub const STRUCTURE_FLAGS: u32 = JSSymbolTableObject::STRUCTURE_FLAGS;

    /// `JSSegmentedVariableObject(VM&, Structure*, JSScope*)`.
    pub(crate) fn new(vm: &VM, structure: StructureRef, scope: Option<JSScopeRef>) -> JSSegmentedVariableObject {
        JSSegmentedVariableObject { base: JSSymbolTableObject::new(vm, structure, scope), variables: RefCell::new(Vec::new()) }
    }

    /// `finishCreation(vm)` (a parte própria): `setSymbolTable(vm, SymbolTable::create(vm))`.
    pub(crate) fn finish_creation(&self, vm: &VM) {
        self.base.set_symbol_table(SymbolTable::create(vm));
    }

    /// `addVariables(numberOfVariablesToAdd, initialValue)`: o índice da primeira variável nova.
    pub fn add_variables(&self, number_of_variables_to_add: u32, initial_value: JSValue) -> ScopeOffset {
        let mut variables = self.variables.borrow_mut();
        let old_size = variables.len();
        variables.resize(old_size + number_of_variables_to_add as usize, initial_value);
        ScopeOffset::new(old_size as u32)
    }
}

impl SymbolTableObjectVariables for JSSegmentedVariableObject {
    fn symbol_table_object(&self) -> &JSSymbolTableObject {
        &self.base
    }

    /// `isValidScopeOffset(offset)`.
    fn is_valid_scope_offset(&self, offset: ScopeOffset) -> bool {
        offset.is_valid() && (offset.offset() as usize) < self.variables.borrow().len()
    }

    /// `variableAt(offset).get()`.
    fn variable_at(&self, offset: ScopeOffset) -> JSValue {
        self.variables.borrow()[offset.offset() as usize]
    }

    fn set_variable_at(&self, offset: ScopeOffset, value: JSValue) {
        self.variables.borrow_mut()[offset.offset() as usize] = value;
    }
}
