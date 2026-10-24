//! Tradução parcial de `runtime/ClassInfo.h`: `struct ClassInfo`.
//!
//! DIVERGÊNCIA: o C++ guarda no `ClassInfo` o `MethodTable` (ponteiros para `put`, `getOwnPropertySlot`,
//! `visitChildren`...), o `staticPropHashTable`, o `checkSubClassSnippet` (JIT) e `staticClassSize`.
//! Aqui o despacho dinâmico é por `JSType`/trait dentro do módulo de cada classe, e não existem tabelas
//! de propriedades estáticas nem coleta de lixo; ficam o nome, a classe-pai e a faixa de `JSType` do
//! `inheritsJSTypeRange`. A identidade do `ClassInfo` é o endereço da `static` (como `&Class::s_info`),
//! comparada por `std::ptr::eq`.

use crate::runtime::js_type::JSTypeRange;

/// `struct ClassInfo`.
#[derive(Debug)]
pub struct ClassInfo {
    /// `className`: por exemplo `"Object"`.
    pub class_name: &'static str,
    /// `parentClass`: `None` é o `nullptr`.
    pub parent_class: Option<&'static ClassInfo>,
    /// `inheritsJSTypeRange`.
    pub inherits_js_type_range: Option<JSTypeRange>,
    /// `staticPropHashTable`: as propriedades que o objeto reifica sob demanda (`Lookup.h`). `None` é o `nullptr`.
    pub static_prop_hash_table: Option<&'static crate::runtime::lookup::HashTable>,
}

impl ClassInfo {
    /// `hasStaticPropertyWithAnyOfAttributes(attributes)` (Structure.cpp): alguma tabela estática da cadeia de
    /// `ClassInfo` tem entrada com um dos atributos. O C++ guarda a união em `seenPropertyAttributes`; aqui a
    /// tabela é varrida, com o mesmo resultado.
    pub fn has_static_property_with_any_of_attributes(&self, attributes: u32) -> bool {
        let mut class_info = Some(self);
        while let Some(current) = class_info {
            if let Some(table) = current.static_prop_hash_table {
                if table.iter().any(|value| value.attributes & attributes != 0) {
                    return true;
                }
            }
            class_info = current.parent_class;
        }
        false
    }

    /// `isSubClassOf(const ClassInfo*)`.
    pub fn is_sub_class_of(&self, other: &ClassInfo) -> bool {
        let mut class_info = Some(self);
        while let Some(current) = class_info {
            if std::ptr::eq(current, other) {
                return true;
            }
            class_info = current.parent_class;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static BASE: ClassInfo = ClassInfo { class_name: "Base", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };
    static DERIVED: ClassInfo = ClassInfo { class_name: "Derived", parent_class: Some(&BASE), static_prop_hash_table: None, inherits_js_type_range: None };

    #[test]
    fn sub_class_of_walks_parents_by_identity() {
        assert!(DERIVED.is_sub_class_of(&BASE));
        assert!(DERIVED.is_sub_class_of(&DERIVED));
        assert!(!BASE.is_sub_class_of(&DERIVED));
    }
}
