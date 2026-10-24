//! Os campos do `JSGlobalObject` que `String.prototype` e `RegExp.prototype` leem e que o resto do
//! `JSGlobalObject` do porte ainda não tem: `m_regExpConstructor` (o `regExpConstructor()` do
//! `SpeciesConstructor`), `m_stringIteratorStructure` e `m_regExpStringIteratorStructure`. Ficam num
//! só bloco para o `init` preencher de uma vez (`js_global_object_init.rs`).

use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::StructureRef;

/// Os campos (o `JSValue` do construtor porque `InternalFunction` não implementa `Debug`).
#[derive(Debug, Default)]
pub struct StringRegExpGlobals {
    pub reg_exp_constructor: Option<JSValue>,
    pub string_iterator_structure: Option<StructureRef>,
    pub reg_exp_string_iterator_structure: Option<StructureRef>,
}

impl JSGlobalObject {
    /// `regExpConstructor()`.
    pub fn reg_exp_constructor(&self) -> JSValue {
        // Invariante: `js_global_object_init` preenche o campo antes de qualquer código JS rodar.
        self.string_regexp_globals.borrow().reg_exp_constructor.expect("JSGlobalObject sem regExpConstructor")
    }

    /// `stringIteratorStructure()`.
    pub fn string_iterator_structure(&self) -> StructureRef {
        // Invariante: preenchido na inicialização do objeto global, antes de qualquer código JS.
        self.string_regexp_globals.borrow().string_iterator_structure.clone().expect("JSGlobalObject sem stringIteratorStructure")
    }

    /// `regExpStringIteratorStructure()`.
    pub fn reg_exp_string_iterator_structure(&self) -> StructureRef {
        // Invariante: preenchido na inicialização do objeto global, antes de qualquer código JS.
        self.string_regexp_globals
            .borrow()
            .reg_exp_string_iterator_structure
            .clone()
            .expect("JSGlobalObject sem regExpStringIteratorStructure")
    }
}
