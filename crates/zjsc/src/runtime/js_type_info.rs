//! Tradução de `runtime/JSTypeInfo.h`: as flags de tipo (inline e fora de linha) e `TypeInfo`.

use crate::runtime::js_type::{is_object_type, JSType};

// Inline flags.
pub const MASQUERADES_AS_UNDEFINED: u32 = 1;
pub const IMPLEMENTS_DEFAULT_HAS_INSTANCE: u32 = 1 << 1;
pub const OVERRIDES_GET_CALL_DATA: u32 = 1 << 2;
pub const OVERRIDES_GET_OWN_PROPERTY_SLOT: u32 = 1 << 3;
pub const OVERRIDES_GET_PROTOTYPE: u32 = 1 << 4;
pub const HAS_STATIC_PROPERTY_TABLE: u32 = 1 << 5;
/// `TypeInfoPerCellBit`: só fica na célula, nunca na `Structure`.
pub const TYPE_INFO_PER_CELL_BIT: u32 = 1 << 7;

// Out of line flags.
pub const IMPLEMENTS_HAS_INSTANCE: u32 = 1 << 8;
pub const OVERRIDES_GET_OWN_PROPERTY_NAMES: u32 = 1 << 9;
pub const OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES: u32 = 1 << 10;
pub const PROHIBITS_PROPERTY_CACHING: u32 = 1 << 11;
pub const GET_OWN_PROPERTY_SLOT_IS_IMPURE: u32 = 1 << 12;
pub const NEW_IMPURE_PROPERTY_FIRES_WATCHPOINTS: u32 = 1 << 13;
pub const IS_IMMUTABLE_PROTOTYPE_EXOTIC_OBJECT: u32 = 1 << 14;
pub const GET_OWN_PROPERTY_SLOT_IS_IMPURE_FOR_PROPERTY_ABSENCE: u32 = 1 << 15;
pub const INTERCEPTS_GET_OWN_PROPERTY_SLOT_BY_INDEX_EVEN_WHEN_LENGTH_IS_NOT_ZERO: u32 = 1 << 16;
pub const STRUCTURE_IS_IMMORTAL: u32 = 1 << 17;
pub const OVERRIDES_PUT: u32 = 1 << 18;
pub const GET_OWN_PROPERTY_SLOT_MAY_BE_WRONG_ABOUT_DONT_ENUM: u32 = 1 << 20;
pub const OVERRIDES_IS_EXTENSIBLE: u32 = 1 << 21;

pub const NUMBER_OF_INLINE_BITS: u32 = 8;

/// `TypeInfo::InlineTypeFlags`.
pub type InlineTypeFlags = u8;
/// `TypeInfo::OutOfLineTypeFlags`.
pub type OutOfLineTypeFlags = u16;

/// `class TypeInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeInfo {
    type_: JSType,
    flags: u8,
    flags2: u16,
}

impl TypeInfo {
    /// `TypeInfo(JSType, unsigned flags)`.
    pub const fn new(type_: JSType, flags: u32) -> TypeInfo {
        debug_assert!(flags >> 24 == 0);
        TypeInfo::with_split_flags(type_, (flags & 0xff) as u8, (flags >> NUMBER_OF_INLINE_BITS) as u16)
    }

    /// `TypeInfo(JSType, InlineTypeFlags, OutOfLineTypeFlags)`.
    pub const fn with_split_flags(type_: JSType, inline_type_flags: InlineTypeFlags, out_of_line_type_flags: OutOfLineTypeFlags) -> TypeInfo {
        TypeInfo { type_, flags: inline_type_flags, flags2: out_of_line_type_flags }
    }

    pub const fn type_(&self) -> JSType {
        self.type_
    }

    /// `isObject()`.
    pub fn is_object(&self) -> bool {
        is_object_type(self.type_)
    }

    pub fn is_final_object(&self) -> bool {
        self.type_ == JSType::FinalObjectType
    }

    pub fn is_number_object(&self) -> bool {
        self.type_ == JSType::NumberObjectType
    }

    /// `flags()`.
    pub fn flags(&self) -> u32 {
        ((self.flags2 as u32) << NUMBER_OF_INLINE_BITS) | self.flags as u32
    }

    fn is_set_on_flags1(&self, flag: u32) -> bool {
        debug_assert!(flag <= (1 << 7));
        self.flags as u32 & flag != 0
    }

    fn is_set_on_flags2(&self, flag: u32) -> bool {
        debug_assert!(flag >= (1 << NUMBER_OF_INLINE_BITS) && flag <= (1 << 24));
        self.flags2 as u32 & (flag >> NUMBER_OF_INLINE_BITS) != 0
    }

    pub fn masquerades_as_undefined(&self) -> bool {
        self.is_set_on_flags1(MASQUERADES_AS_UNDEFINED)
    }

    pub fn implements_has_instance(&self) -> bool {
        self.is_set_on_flags2(IMPLEMENTS_HAS_INSTANCE)
    }

    pub fn implements_default_has_instance(&self) -> bool {
        self.is_set_on_flags1(IMPLEMENTS_DEFAULT_HAS_INSTANCE)
    }

    pub fn overrides_get_call_data(&self) -> bool {
        self.is_set_on_flags1(OVERRIDES_GET_CALL_DATA)
    }

    pub fn overrides_get_own_property_slot(&self) -> bool {
        TypeInfo::overrides_get_own_property_slot_of(self.inline_type_flags())
    }

    pub fn has_static_property_table(&self) -> bool {
        self.is_set_on_flags1(HAS_STATIC_PROPERTY_TABLE)
    }

    /// `static overridesGetOwnPropertySlot(InlineTypeFlags)`.
    pub fn overrides_get_own_property_slot_of(flags: InlineTypeFlags) -> bool {
        flags as u32 & OVERRIDES_GET_OWN_PROPERTY_SLOT != 0
    }

    /// `static hasStaticPropertyTable(InlineTypeFlags)`.
    pub fn has_static_property_table_of(flags: InlineTypeFlags) -> bool {
        flags as u32 & HAS_STATIC_PROPERTY_TABLE != 0
    }

    /// `static perCellBit(InlineTypeFlags)`.
    pub fn per_cell_bit(flags: InlineTypeFlags) -> bool {
        flags as u32 & TYPE_INFO_PER_CELL_BIT != 0
    }

    pub fn structure_is_immortal(&self) -> bool {
        self.is_set_on_flags2(STRUCTURE_IS_IMMORTAL)
    }

    pub fn overrides_get_own_property_names(&self) -> bool {
        self.is_set_on_flags2(OVERRIDES_GET_OWN_PROPERTY_NAMES)
    }

    pub fn overrides_get_own_special_property_names(&self) -> bool {
        self.is_set_on_flags2(OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES)
    }

    pub fn overrides_any_form_of_get_own_property_names(&self) -> bool {
        self.overrides_get_own_property_names() || self.overrides_get_own_special_property_names()
    }

    pub fn overrides_put(&self) -> bool {
        self.is_set_on_flags2(OVERRIDES_PUT)
    }

    pub fn overrides_get_prototype(&self) -> bool {
        self.is_set_on_flags1(OVERRIDES_GET_PROTOTYPE)
    }

    pub fn overrides_is_extensible(&self) -> bool {
        self.is_set_on_flags2(OVERRIDES_IS_EXTENSIBLE)
    }

    pub fn prohibits_property_caching(&self) -> bool {
        self.is_set_on_flags2(PROHIBITS_PROPERTY_CACHING)
    }

    pub fn get_own_property_slot_is_impure(&self) -> bool {
        self.is_set_on_flags2(GET_OWN_PROPERTY_SLOT_IS_IMPURE)
    }

    pub fn get_own_property_slot_is_impure_for_property_absence(&self) -> bool {
        self.is_set_on_flags2(GET_OWN_PROPERTY_SLOT_IS_IMPURE_FOR_PROPERTY_ABSENCE)
    }

    pub fn get_own_property_slot_may_be_wrong_about_dont_enum(&self) -> bool {
        self.is_set_on_flags2(GET_OWN_PROPERTY_SLOT_MAY_BE_WRONG_ABOUT_DONT_ENUM)
    }

    pub fn new_impure_property_fires_watchpoints(&self) -> bool {
        self.is_set_on_flags2(NEW_IMPURE_PROPERTY_FIRES_WATCHPOINTS)
    }

    pub fn is_immutable_prototype_exotic_object(&self) -> bool {
        self.is_set_on_flags2(IS_IMMUTABLE_PROTOTYPE_EXOTIC_OBJECT)
    }

    pub fn intercepts_get_own_property_slot_by_index_even_when_length_is_not_zero(&self) -> bool {
        self.is_set_on_flags2(INTERCEPTS_GET_OWN_PROPERTY_SLOT_BY_INDEX_EVEN_WHEN_LENGTH_IS_NOT_ZERO)
    }

    /// `static isArgumentsType(JSType)`.
    pub fn is_arguments_type(type_: JSType) -> bool {
        type_ == JSType::DirectArgumentsType || type_ == JSType::ScopedArgumentsType || type_ == JSType::ClonedArgumentsType
    }

    /// `mergeInlineTypeFlags`: a `Structure` não guarda `TypeInfoPerCellBit`, então copia da célula.
    pub fn merge_inline_type_flags(structure_flags: InlineTypeFlags, old_cell_flags: InlineTypeFlags) -> InlineTypeFlags {
        structure_flags | (old_cell_flags & TYPE_INFO_PER_CELL_BIT as u8)
    }

    pub const fn inline_type_flags(&self) -> InlineTypeFlags {
        self.flags
    }

    pub const fn out_of_line_type_flags(&self) -> OutOfLineTypeFlags {
        self.flags2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_split_between_inline_and_out_of_line() {
        let info = TypeInfo::new(JSType::FinalObjectType, OVERRIDES_GET_OWN_PROPERTY_SLOT | OVERRIDES_PUT);
        assert!(info.overrides_get_own_property_slot());
        assert!(info.overrides_put());
        assert!(!info.overrides_get_prototype());
        assert_eq!(info.flags(), OVERRIDES_GET_OWN_PROPERTY_SLOT | OVERRIDES_PUT);
        assert!(info.is_object() && info.is_final_object());
        assert!(!TypeInfo::new(JSType::StringType, 0).is_object());
        assert_eq!(
            TypeInfo::merge_inline_type_flags(OVERRIDES_GET_PROTOTYPE as u8, TYPE_INFO_PER_CELL_BIT as u8),
            (OVERRIDES_GET_PROTOTYPE | TYPE_INFO_PER_CELL_BIT) as u8
        );
    }
}
