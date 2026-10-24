//! Porte de `enum class PropertyAttribute` e dos operadores de `runtime/PropertySlot.h`.
//!
//! O C++ usa `PropertyAttribute` como enum de bits e converte para `unsigned` nos operadores; aqui
//! o enum guarda os valores (`as u32`) e as constantes `u32` com os mesmos nomes em
//! SCREAMING_SNAKE servem às contas de bits. `USE(BUN_JSC_ADDITIONS)` vale (cmakeconfig.h),
//! logo `Constructable` existe e é `LastAttribute`.

/// `enum class PropertyAttribute : unsigned`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropertyAttribute {
    // Precisa caber em 7 bits (definição do Structure).
    None = 0,
    ReadOnly = 1 << 1,
    DontEnum = 1 << 2,
    DontDelete = 1 << 3,
    Accessor = 1 << 4,
    CustomAccessor = 1 << 5,
    CustomValue = 1 << 6,
    CustomAccessorOrValue = (1 << 5) | (1 << 6),
    AccessorOrCustomAccessorOrValue = (1 << 4) | (1 << 5) | (1 << 6),
    ReadOnlyOrAccessorOrCustomAccessor = (1 << 1) | (1 << 4) | (1 << 5),
    ReadOnlyOrAccessorOrCustomAccessorOrValue = (1 << 1) | (1 << 4) | (1 << 5) | (1 << 6),

    // Só as tabelas estáticas usam os de 8 para cima; não ficam no byte de atributos do Structure.
    Function = 1 << 8,
    Builtin = 1 << 9,
    ConstantInteger = 1 << 10,
    CellProperty = 1 << 11,
    ClassStructure = 1 << 12,
    PropertyCallback = 1 << 13,
    DOMAttribute = 1 << 14,
    DOMJITAttribute = 1 << 15,
    DOMJITFunction = 1 << 16,
    Constructable = 1 << 17,

    BuiltinOrFunction = (1 << 9) | (1 << 8),
    BuiltinOrFunctionOrLazyProperty = (1 << 9) | (1 << 8) | (1 << 11) | (1 << 12) | (1 << 13),
    BuiltinOrFunctionOrAccessorOrLazyProperty = (1 << 9) | (1 << 8) | (1 << 4) | (1 << 11) | (1 << 12) | (1 << 13),
    BuiltinOrFunctionOrAccessorOrLazyPropertyOrConstant =
        (1 << 9) | (1 << 8) | (1 << 4) | (1 << 11) | (1 << 12) | (1 << 13) | (1 << 10),
}

pub const NONE: u32 = PropertyAttribute::None as u32;
pub const READ_ONLY: u32 = PropertyAttribute::ReadOnly as u32;
pub const DONT_ENUM: u32 = PropertyAttribute::DontEnum as u32;
pub const DONT_DELETE: u32 = PropertyAttribute::DontDelete as u32;
pub const ACCESSOR: u32 = PropertyAttribute::Accessor as u32;
pub const CUSTOM_ACCESSOR: u32 = PropertyAttribute::CustomAccessor as u32;
pub const CUSTOM_VALUE: u32 = PropertyAttribute::CustomValue as u32;
pub const CUSTOM_ACCESSOR_OR_VALUE: u32 = PropertyAttribute::CustomAccessorOrValue as u32;
pub const ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE: u32 = PropertyAttribute::AccessorOrCustomAccessorOrValue as u32;
pub const READ_ONLY_OR_ACCESSOR_OR_CUSTOM_ACCESSOR: u32 = PropertyAttribute::ReadOnlyOrAccessorOrCustomAccessor as u32;
pub const READ_ONLY_OR_ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE: u32 =
    PropertyAttribute::ReadOnlyOrAccessorOrCustomAccessorOrValue as u32;
pub const FUNCTION: u32 = PropertyAttribute::Function as u32;
pub const BUILTIN: u32 = PropertyAttribute::Builtin as u32;
pub const CONSTANT_INTEGER: u32 = PropertyAttribute::ConstantInteger as u32;
pub const CELL_PROPERTY: u32 = PropertyAttribute::CellProperty as u32;
pub const CLASS_STRUCTURE: u32 = PropertyAttribute::ClassStructure as u32;
pub const PROPERTY_CALLBACK: u32 = PropertyAttribute::PropertyCallback as u32;
pub const DOM_ATTRIBUTE: u32 = PropertyAttribute::DOMAttribute as u32;
pub const DOMJIT_ATTRIBUTE: u32 = PropertyAttribute::DOMJITAttribute as u32;
pub const DOMJIT_FUNCTION: u32 = PropertyAttribute::DOMJITFunction as u32;
pub const CONSTRUCTABLE: u32 = PropertyAttribute::Constructable as u32;
pub const LAST_ATTRIBUTE: u32 = CONSTRUCTABLE;
pub const BUILTIN_OR_FUNCTION: u32 = PropertyAttribute::BuiltinOrFunction as u32;
pub const BUILTIN_OR_FUNCTION_OR_LAZY_PROPERTY: u32 = PropertyAttribute::BuiltinOrFunctionOrLazyProperty as u32;
pub const BUILTIN_OR_FUNCTION_OR_ACCESSOR_OR_LAZY_PROPERTY: u32 =
    PropertyAttribute::BuiltinOrFunctionOrAccessorOrLazyProperty as u32;
pub const BUILTIN_OR_FUNCTION_OR_ACCESSOR_OR_LAZY_PROPERTY_OR_CONSTANT: u32 =
    PropertyAttribute::BuiltinOrFunctionOrAccessorOrLazyPropertyOrConstant as u32;

/// `attributesForStructure`: os atributos só das tabelas estáticas ficam do bit 8 para cima.
pub fn attributes_for_structure(attributes: u32) -> u32 {
    attributes as u8 as u32
}

impl From<PropertyAttribute> for u32 {
    fn from(attribute: PropertyAttribute) -> u32 {
        attribute as u32
    }
}

impl std::ops::BitOr for PropertyAttribute {
    type Output = u32;
    fn bitor(self, other: PropertyAttribute) -> u32 {
        self as u32 | other as u32
    }
}

impl std::ops::BitOr<PropertyAttribute> for u32 {
    type Output = u32;
    fn bitor(self, other: PropertyAttribute) -> u32 {
        self | other as u32
    }
}

impl std::ops::BitOr<u32> for PropertyAttribute {
    type Output = u32;
    fn bitor(self, other: u32) -> u32 {
        self as u32 | other
    }
}

impl std::ops::BitAnd<PropertyAttribute> for u32 {
    type Output = u32;
    fn bitand(self, other: PropertyAttribute) -> u32 {
        self & other as u32
    }
}

impl std::ops::Not for PropertyAttribute {
    type Output = u32;
    fn not(self) -> u32 {
        !(self as u32)
    }
}

impl std::ops::BitOrAssign<PropertyAttribute> for u32 {
    fn bitor_assign(&mut self, other: PropertyAttribute) {
        *self |= other as u32;
    }
}
