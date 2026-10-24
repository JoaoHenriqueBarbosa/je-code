//! Tradução de `runtime/RuntimeType.h` e `RuntimeType.cpp`.
//!
//! Fica de fora, até o `JSValue` ter `isAnyInt`/`isCallable` e o registro de células a classe de
//! chamável: `runtimeTypeForValue(JSValue)`.

use crate::wtf::text::wtf_string::String as WtfString;

/// `enum RuntimeType : uint16_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum RuntimeType {
    TypeNothing = 0x0,
    TypeFunction = 0x1,
    TypeUndefined = 0x2,
    TypeNull = 0x4,
    TypeBoolean = 0x8,
    TypeAnyInt = 0x10,
    TypeNumber = 0x20,
    TypeString = 0x40,
    TypeObject = 0x80,
    TypeSymbol = 0x100,
    TypeBigInt = 0x200,
}

/// `typedef uint16_t RuntimeTypeMask`.
pub type RuntimeTypeMask = u16;

pub const TYPE_NOTHING: RuntimeTypeMask = RuntimeType::TypeNothing as u16;
pub const TYPE_FUNCTION: RuntimeTypeMask = RuntimeType::TypeFunction as u16;
pub const TYPE_UNDEFINED: RuntimeTypeMask = RuntimeType::TypeUndefined as u16;
pub const TYPE_NULL: RuntimeTypeMask = RuntimeType::TypeNull as u16;
pub const TYPE_BOOLEAN: RuntimeTypeMask = RuntimeType::TypeBoolean as u16;
pub const TYPE_ANY_INT: RuntimeTypeMask = RuntimeType::TypeAnyInt as u16;
pub const TYPE_NUMBER: RuntimeTypeMask = RuntimeType::TypeNumber as u16;
pub const TYPE_STRING: RuntimeTypeMask = RuntimeType::TypeString as u16;
pub const TYPE_OBJECT: RuntimeTypeMask = RuntimeType::TypeObject as u16;
pub const TYPE_SYMBOL: RuntimeTypeMask = RuntimeType::TypeSymbol as u16;
pub const TYPE_BIG_INT: RuntimeTypeMask = RuntimeType::TypeBigInt as u16;

/// `RuntimeTypeMaskAllTypes`.
pub const RUNTIME_TYPE_MASK_ALL_TYPES: RuntimeTypeMask = TYPE_FUNCTION
    | TYPE_UNDEFINED
    | TYPE_NULL
    | TYPE_BOOLEAN
    | TYPE_ANY_INT
    | TYPE_NUMBER
    | TYPE_STRING
    | TYPE_OBJECT
    | TYPE_SYMBOL
    | TYPE_BIG_INT;

/// `runtimeTypeIsPrimitive(RuntimeTypeMask)`.
pub fn runtime_type_is_primitive(type_: RuntimeTypeMask) -> bool {
    type_ & !(TYPE_FUNCTION | TYPE_OBJECT) != 0
}

/// `runtimeTypeAsString(RuntimeType)`.
pub fn runtime_type_as_string(type_: RuntimeType) -> WtfString {
    let name = match type_ {
        RuntimeType::TypeUndefined => "Undefined",
        RuntimeType::TypeNull => "Null",
        RuntimeType::TypeAnyInt => "Integer",
        RuntimeType::TypeNumber => "Number",
        RuntimeType::TypeString => "String",
        RuntimeType::TypeObject => "Object",
        RuntimeType::TypeBoolean => "Boolean",
        RuntimeType::TypeFunction => "Function",
        RuntimeType::TypeSymbol => "Symbol",
        RuntimeType::TypeBigInt => "BigInt",
        RuntimeType::TypeNothing => "(Nothing)",
    };
    WtfString::from_latin1(name.as_bytes())
}
