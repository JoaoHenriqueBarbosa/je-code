//! Tradução parcial de `runtime/TypeSet.h` e `TypeSet.cpp`: o `TypeSet` com os tipos vistos.
//!
//! O que entrou: `create`, `isOverflown`, `isEmpty`, `seenTypes`, `doesTypeConformTo`, `displayName`
//! e `dumpTypes` (o ramo sem estruturas).
//!
//! O que FALTA e por quê (depende de peças ainda não portadas, não de decisão):
//!
//! - `StructureShape` (`m_structureHistory`, `propertyHash`, `merge`, `leastCommonAncestor`,
//!   `stringRepresentation`, `toJSONString`) e, com ele, `addTypeInformation`,
//!   `allStructureRepresentations`, `toJSONString` e o ramo de estruturas de `displayName`/`dumpTypes`:
//!   exigem o `StructureSet` (`runtime/StructureSet.h`) e o conjunto de `UniquedStringImpl` com
//!   `IdentifierRepHash`.
//! - `invalidateCache(VM&)` (filtra o `m_structureSet` pelo que o GC marcou).
//! - `inspectorTypeSet` e os tipos `Inspector::Protocol::*` (inspetor remoto, fora do porte).
//!
//! Sem `m_structureHistory` preenchível, o histórico é sempre vazio e os métodos portados valem o
//! comportamento do C++ para ele. `ConcurrentJSLock m_lock` some (um fio só); `ThreadSafeRefCounted` é
//! `Rc`.

use std::rc::Rc;

use crate::runtime::runtime_type::{
    RuntimeTypeMask, TYPE_ANY_INT, TYPE_BIG_INT, TYPE_BOOLEAN, TYPE_FUNCTION, TYPE_NOTHING, TYPE_NULL, TYPE_NUMBER,
    TYPE_OBJECT, TYPE_STRING, TYPE_SYMBOL, TYPE_UNDEFINED,
};
use crate::wtf::text::wtf_string::{empty_string, String as WtfString};

/// `class TypeSet`.
#[derive(Debug)]
pub struct TypeSet {
    is_overflown: bool,
    seen_types: RuntimeTypeMask,
}

impl TypeSet {
    /// `TypeSet()`.
    pub fn new() -> TypeSet {
        TypeSet { is_overflown: false, seen_types: TYPE_NOTHING }
    }

    /// `create()`.
    pub fn create() -> Rc<TypeSet> {
        Rc::new(TypeSet::new())
    }

    /// `isOverflown()`.
    pub fn is_overflown(&self) -> bool {
        self.is_overflown
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.seen_types == TYPE_NOTHING
    }

    /// `seenTypes()`.
    pub fn seen_types(&self) -> RuntimeTypeMask {
        self.seen_types
    }

    /// `dumpTypes() const`.
    pub fn dump_types(&self) -> WtfString {
        if self.seen_types == TYPE_NOTHING {
            return WtfString::from_latin1(b"(Unreached Statement)");
        }

        let mut seen = std::string::String::new();

        if self.seen_types & TYPE_FUNCTION != 0 {
            seen.push_str("Function ");
        }
        if self.seen_types & TYPE_UNDEFINED != 0 {
            seen.push_str("Undefined ");
        }
        if self.seen_types & TYPE_NULL != 0 {
            seen.push_str("Null ");
        }
        if self.seen_types & TYPE_BOOLEAN != 0 {
            seen.push_str("Boolean ");
        }
        if self.seen_types & TYPE_ANY_INT != 0 {
            seen.push_str("AnyInt ");
        }
        if self.seen_types & TYPE_NUMBER != 0 {
            seen.push_str("Number ");
        }
        if self.seen_types & TYPE_STRING != 0 {
            seen.push_str("String ");
        }
        if self.seen_types & TYPE_OBJECT != 0 {
            seen.push_str("Object ");
        }
        if self.seen_types & TYPE_SYMBOL != 0 {
            seen.push_str("Symbol ");
        }

        WtfString::from_latin1(seen.as_bytes())
    }

    /// `doesTypeConformTo(RuntimeTypeMask) const`.
    pub fn does_type_conform_to(&self, test: RuntimeTypeMask) -> bool {
        // This function checks if our seen types conform  to the types described by the test bitstring. (i.e we haven't seen more types than test).
        // We are <= to those types if ANDing with the bitstring doesn't zero out any of our bits.
        self.seen_types != TYPE_NOTHING && (self.seen_types & test) == self.seen_types
    }

    /// `displayName() const` (com o histórico de estruturas sempre vazio, ver o topo do módulo).
    pub fn display_name(&self) -> WtfString {
        if self.seen_types == TYPE_NOTHING {
            return empty_string();
        }

        // The order of these checks are important. For example, if a value is only a function, it conforms to TypeFunction, but it also conforms to TypeFunction | TypeNull.
        // Therefore, more specific types must be checked first.
        const NULLABLE: RuntimeTypeMask = TYPE_NULL | TYPE_UNDEFINED;
        let name = if self.does_type_conform_to(TYPE_FUNCTION) {
            "Function"
        } else if self.does_type_conform_to(TYPE_UNDEFINED) {
            "Undefined"
        } else if self.does_type_conform_to(TYPE_NULL) {
            "Null"
        } else if self.does_type_conform_to(TYPE_BOOLEAN) {
            "Boolean"
        } else if self.does_type_conform_to(TYPE_ANY_INT) {
            "Integer"
        } else if self.does_type_conform_to(TYPE_NUMBER | TYPE_ANY_INT) {
            "Number"
        } else if self.does_type_conform_to(TYPE_STRING) {
            "String"
        } else if self.does_type_conform_to(TYPE_SYMBOL) {
            "Symbol"
        } else if self.does_type_conform_to(TYPE_BIG_INT) {
            "BigInt"
        } else if self.does_type_conform_to(NULLABLE) {
            "(?)"
        } else if self.does_type_conform_to(TYPE_FUNCTION | NULLABLE) {
            "Function?"
        } else if self.does_type_conform_to(TYPE_BOOLEAN | NULLABLE) {
            "Boolean?"
        } else if self.does_type_conform_to(TYPE_ANY_INT | NULLABLE) {
            "Integer?"
        } else if self.does_type_conform_to(TYPE_NUMBER | TYPE_ANY_INT | NULLABLE) {
            "Number?"
        } else if self.does_type_conform_to(TYPE_STRING | NULLABLE) {
            "String?"
        } else if self.does_type_conform_to(TYPE_SYMBOL | NULLABLE) {
            "Symbol?"
        } else if self.does_type_conform_to(TYPE_BIG_INT | NULLABLE) {
            "BigInt?"
        } else if self.does_type_conform_to(TYPE_OBJECT | TYPE_FUNCTION | TYPE_STRING) {
            "Object"
        } else if self.does_type_conform_to(TYPE_OBJECT | TYPE_FUNCTION | TYPE_STRING | NULLABLE) {
            "Object?"
        } else {
            "(many)"
        };
        WtfString::from_latin1(name.as_bytes())
    }
}

impl Default for TypeSet {
    fn default() -> TypeSet {
        TypeSet::new()
    }
}
