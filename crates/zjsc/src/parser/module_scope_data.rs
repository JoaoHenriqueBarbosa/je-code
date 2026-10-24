//! Tradução de `parser/ModuleScopeData.h`.
//!
//! `RefCounted<ModuleScopeData>` vira `Rc<ModuleScopeData>`. Como o C++ muta o objeto compartilhado
//! por `RefPtr`, os dois mapas ficam em `RefCell` e os métodos recebem `&self`. A chave
//! `RefPtr<UniquedStringImpl>` (com nulo possível, vindo de `Identifier::impl()`) vira
//! `Option<UniquedKey>`, que se compara e se espalha por ponteiro como o `IdentifierRepHash`.

use std::cell::{Ref, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::runtime::identifier::Identifier;
use crate::wtf::text::string_impl::UniquedKey;

/// `ModuleScopeData::IdentifierAliasMap`.
pub type IdentifierAliasMap = HashMap<Option<UniquedKey>, Vec<Option<UniquedKey>>>;

/// `class ModuleScopeData`.
#[derive(Default)]
pub struct ModuleScopeData {
    exported_names: RefCell<HashSet<Option<UniquedKey>>>,
    exported_bindings: RefCell<IdentifierAliasMap>,
}

impl ModuleScopeData {
    /// `ModuleScopeData::create()`.
    pub fn create() -> Rc<ModuleScopeData> {
        Rc::new(ModuleScopeData::default())
    }

    /// `exportedBindings()`.
    pub fn exported_bindings(&self) -> Ref<'_, IdentifierAliasMap> {
        self.exported_bindings.borrow()
    }

    /// `exportName(const Identifier&)`: verdadeiro se o nome é novo.
    pub fn export_name(&self, exported_name: &Identifier) -> bool {
        self.exported_names.borrow_mut().insert(exported_name.impl_())
    }

    /// `exportBinding(const Identifier& localName, const Identifier& exportedName)`.
    pub fn export_binding(&self, local_name: &Identifier, exported_name: &Identifier) {
        self.exported_bindings
            .borrow_mut()
            .entry(local_name.impl_())
            .or_default()
            .push(exported_name.impl_());
    }

    /// `exportBinding(const Identifier& localName)`.
    pub fn export_binding_same_name(&self, local_name: &Identifier) {
        self.export_binding(local_name, local_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_name_reports_new_entries() {
        let data = ModuleScopeData::create();
        let name = Identifier::empty_identifier();
        assert!(data.export_name(&name));
        assert!(!data.export_name(&name));
    }

    #[test]
    fn export_binding_appends_aliases() {
        let data = ModuleScopeData::create();
        let local = Identifier::empty_identifier();
        data.export_binding_same_name(&local);
        data.export_binding(&local, &Identifier::empty_identifier());
        let bindings = data.exported_bindings();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings.get(&local.impl_()).map(|aliases| aliases.len()), Some(2));
    }
}
