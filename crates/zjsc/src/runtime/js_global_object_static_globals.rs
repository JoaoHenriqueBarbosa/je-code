//! Porte de `JSGlobalObject::initStaticGlobals` e `JSGlobalObject::addStaticGlobals`
//! (JSGlobalObject.cpp:1077 e 3297): `NaN`, `Infinity` e `undefined` como variáveis da `SymbolTable` do
//! global, com `DontEnum | DontDelete | ReadOnly`.
//!
//! DIVERGÊNCIA: o quarto elemento do C++ (`assertPrivateName`, a `JSFunction` `assertCall`) só existe sob
//! `ASSERT_ENABLED`, que é um build de depuração do JavaScriptCore; o build de release do Debian não o
//! tem, então não é portado.
//!
//! Chamar `init_static_globals` no ponto de `initStaticGlobals(vm)` de `init` (JSGlobalObject.cpp:2271).

use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_symbol_table_object::SymbolTableObjectVariables;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_value::{js_number, js_undefined, pnan, JSValue};
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::symbol_table::SymbolTableEntry;
use crate::runtime::var_offset::VarOffset;

/// `GlobalPropertyInfo(identifier, value, attributes)`.
pub(crate) struct GlobalPropertyInfo<'a> {
    pub(crate) identifier: &'a Identifier,
    pub(crate) value: JSValue,
    pub(crate) attributes: u32,
}

impl JSGlobalObject {
    /// `addStaticGlobals(globals)`.
    pub(crate) fn add_static_globals(&self, globals: &[GlobalPropertyInfo<'_>]) {
        let start_offset = self.add_variables(globals.len() as u32, js_undefined());

        for (index, global) in globals.iter().enumerate() {
            // This `configurable = false` is necessary condition for static globals, otherwise lexical
            // bindings can change the result of GlobalVar queries too. We won't be able to declare a
            // global lexical variable with the same name to the static globals because configurable = false.
            debug_assert!(global.attributes & DONT_DELETE != 0);

            let key = global.identifier.impl_().expect("addStaticGlobals com Identifier nulo");
            let offset = {
                let symbol_table = self.symbol_table();
                let mut symbol_table = symbol_table.borrow_mut();
                let offset = symbol_table.take_next_scope_offset();
                assert!(offset.offset() == start_offset.offset() + index as u32);
                let mut new_entry = SymbolTableEntry::new(VarOffset::from_scope_offset(offset), global.attributes);
                new_entry.prepare_to_watch();
                symbol_table.add(key, new_entry);
                offset
            };
            // `symbolTablePutTouchWatchpointSet(vm(), this, identifier, value, variable, watchpointSet)`:
            // o conjunto de watchpoint ainda não tem observador, então o toque se reduz à escrita.
            self.set_variable_at(offset, global.value);
        }
    }

    /// `initStaticGlobals(vm)`.
    pub fn init_static_globals(&self) {
        let names = &self.vm().property_names;
        let attributes = DONT_ENUM | DONT_DELETE | READ_ONLY;
        // A ordem de inserção é a do C++. A enumeração (Infinity, undefined, NaN no bun) vem da ordem de
        // bucket do `KeyHashMap` da `SymbolTable`, não da inserção.
        self.add_static_globals(&[
            GlobalPropertyInfo { identifier: &names.na_n, value: js_number(pnan()), attributes },
            GlobalPropertyInfo { identifier: &names.infinity, value: js_number(f64::INFINITY), attributes },
            GlobalPropertyInfo { identifier: &names.undefined_keyword, value: js_undefined(), attributes },
        ]);
    }
}
