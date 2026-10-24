//! Porte de `runtime/TypedArrayType.h` e `TypedArrayType.cpp`: o `TypedArrayType` (qual das 12 visões e o
//! `DataView`), o tipo de conteúdo (número ou BigInt), o tamanho do elemento e as conversões de e para o
//! `JSType`.
//!
//! DIVERGÊNCIA: `constructorClassInfoForType` mora em `typed_array_constructors.rs`, junto dos
//! `ClassInfo` dos construtores. Os nomes das variantes não levam o prefixo `Type` do C++
//! (`TypedArrayType::Int8` é o `TypeInt8`).

use crate::runtime::js_type::{JSType, FIRST_TYPED_ARRAY_TYPE};

/// `enum TypedArrayType : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypedArrayType {
    NotTypedArray = 0,
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float16,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
    DataView,
}

/// `enum class TypedArrayContentType : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypedArrayContentType {
    None,
    Number,
    BigInt,
}

/// `FOR_EACH_TYPED_ARRAY_TYPE_EXCLUDING_DATA_VIEW`: as 12 visões na ordem do `JSType` e do enum.
pub const TYPED_ARRAY_TYPES_EXCLUDING_DATA_VIEW: [TypedArrayType; 12] = [
    TypedArrayType::Int8,
    TypedArrayType::Uint8,
    TypedArrayType::Uint8Clamped,
    TypedArrayType::Int16,
    TypedArrayType::Uint16,
    TypedArrayType::Int32,
    TypedArrayType::Uint32,
    TypedArrayType::Float16,
    TypedArrayType::Float32,
    TypedArrayType::Float64,
    TypedArrayType::BigInt64,
    TypedArrayType::BigUint64,
];

/// `NumberOfTypedArrayTypes`: as 12 visões e o `DataView`.
pub const NUMBER_OF_TYPED_ARRAY_TYPES: usize = 13;

/// O `JSType` de cada tipo, na ordem de `FOR_EACH_TYPED_ARRAY_TYPE` (`JSType.h`).
const JS_TYPES: [JSType; NUMBER_OF_TYPED_ARRAY_TYPES] = [
    JSType::Int8ArrayType,
    JSType::Uint8ArrayType,
    JSType::Uint8ClampedArrayType,
    JSType::Int16ArrayType,
    JSType::Uint16ArrayType,
    JSType::Int32ArrayType,
    JSType::Uint32ArrayType,
    JSType::Float16ArrayType,
    JSType::Float32ArrayType,
    JSType::Float64ArrayType,
    JSType::BigInt64ArrayType,
    JSType::BigUint64ArrayType,
    JSType::DataViewType,
];

impl TypedArrayType {
    /// `toIndex(type)`: `type - 1`, o índice dos arrays por tipo.
    pub fn to_index(self) -> usize {
        debug_assert!(self != TypedArrayType::NotTypedArray);
        self as usize - 1
    }

    /// `indexToTypedArrayType(index)`.
    pub fn from_index(index: usize) -> TypedArrayType {
        match index {
            0 => TypedArrayType::Int8,
            1 => TypedArrayType::Uint8,
            2 => TypedArrayType::Uint8Clamped,
            3 => TypedArrayType::Int16,
            4 => TypedArrayType::Uint16,
            5 => TypedArrayType::Int32,
            6 => TypedArrayType::Uint32,
            7 => TypedArrayType::Float16,
            8 => TypedArrayType::Float32,
            9 => TypedArrayType::Float64,
            10 => TypedArrayType::BigInt64,
            11 => TypedArrayType::BigUint64,
            12 => TypedArrayType::DataView,
            _ => panic!("indexToTypedArrayType fora de [0, 12]: {index}"),
        }
    }

    /// `isTypedView(type)`: uma das 12 visões, nem `NotTypedArray` nem `DataView`.
    pub fn is_typed_view(self) -> bool {
        !matches!(self, TypedArrayType::NotTypedArray | TypedArrayType::DataView)
    }

    /// `isBigIntTypedView(type)`.
    pub fn is_big_int_typed_view(self) -> bool {
        matches!(self, TypedArrayType::BigInt64 | TypedArrayType::BigUint64)
    }

    /// `logElementSize(type)`.
    pub fn log_element_size(self) -> u32 {
        match self {
            TypedArrayType::NotTypedArray => panic!("logElementSize de NotTypedArray"),
            TypedArrayType::Int8 | TypedArrayType::Uint8 | TypedArrayType::Uint8Clamped | TypedArrayType::DataView => 0,
            TypedArrayType::Int16 | TypedArrayType::Uint16 | TypedArrayType::Float16 => 1,
            TypedArrayType::Int32 | TypedArrayType::Uint32 | TypedArrayType::Float32 => 2,
            TypedArrayType::Float64 | TypedArrayType::BigInt64 | TypedArrayType::BigUint64 => 3,
        }
    }

    /// `elementSize(type)`.
    pub fn element_size(self) -> usize {
        1usize << self.log_element_size()
    }

    /// `isInt(type)`.
    pub fn is_int(self) -> bool {
        matches!(
            self,
            TypedArrayType::Int8
                | TypedArrayType::Uint8
                | TypedArrayType::Uint8Clamped
                | TypedArrayType::Int16
                | TypedArrayType::Uint16
                | TypedArrayType::Int32
                | TypedArrayType::Uint32
        )
    }

    /// `isFloat(type)`.
    pub fn is_float(self) -> bool {
        matches!(self, TypedArrayType::Float16 | TypedArrayType::Float32 | TypedArrayType::Float64)
    }

    /// `isBigInt(type)`.
    pub fn is_big_int(self) -> bool {
        self.is_big_int_typed_view()
    }

    /// `isSigned(type)`.
    pub fn is_signed(self) -> bool {
        matches!(
            self,
            TypedArrayType::Int8
                | TypedArrayType::Int16
                | TypedArrayType::Int32
                | TypedArrayType::Float16
                | TypedArrayType::Float32
                | TypedArrayType::Float64
                | TypedArrayType::BigInt64
        )
    }

    /// `isClamped(type)`.
    pub fn is_clamped(self) -> bool {
        self == TypedArrayType::Uint8Clamped
    }

    /// `isSomeUint8(type)`.
    pub fn is_some_uint8(self) -> bool {
        matches!(self, TypedArrayType::Uint8 | TypedArrayType::Uint8Clamped)
    }

    /// `contentType(type)`.
    pub fn content_type(self) -> TypedArrayContentType {
        match self {
            TypedArrayType::BigInt64 | TypedArrayType::BigUint64 => TypedArrayContentType::BigInt,
            TypedArrayType::NotTypedArray | TypedArrayType::DataView => TypedArrayContentType::None,
            _ => TypedArrayContentType::Number,
        }
    }

    /// `typeForTypedArrayType(type)`: o `JSType`.
    pub fn js_type(self) -> JSType {
        match self {
            TypedArrayType::NotTypedArray => panic!("typeForTypedArrayType de NotTypedArray"),
            _ => JS_TYPES[self.to_index()],
        }
    }

    /// O nome da classe (`"Int8Array"`, `"DataView"`): o `className` do `ClassInfo` e o
    /// `@@toStringTag`.
    pub fn class_name(self) -> &'static str {
        match self {
            TypedArrayType::NotTypedArray => panic!("className de NotTypedArray"),
            TypedArrayType::Int8 => "Int8Array",
            TypedArrayType::Uint8 => "Uint8Array",
            TypedArrayType::Uint8Clamped => "Uint8ClampedArray",
            TypedArrayType::Int16 => "Int16Array",
            TypedArrayType::Uint16 => "Uint16Array",
            TypedArrayType::Int32 => "Int32Array",
            TypedArrayType::Uint32 => "Uint32Array",
            TypedArrayType::Float16 => "Float16Array",
            TypedArrayType::Float32 => "Float32Array",
            TypedArrayType::Float64 => "Float64Array",
            TypedArrayType::BigInt64 => "BigInt64Array",
            TypedArrayType::BigUint64 => "BigUint64Array",
            TypedArrayType::DataView => "DataView",
        }
    }
}

/// `typedArrayType(JSType)`: `NotTypedArray` para o que não é visão.
pub fn typed_array_type(type_: JSType) -> TypedArrayType {
    match type_ {
        JSType::Int8ArrayType => TypedArrayType::Int8,
        JSType::Uint8ArrayType => TypedArrayType::Uint8,
        JSType::Uint8ClampedArrayType => TypedArrayType::Uint8Clamped,
        JSType::Int16ArrayType => TypedArrayType::Int16,
        JSType::Uint16ArrayType => TypedArrayType::Uint16,
        JSType::Int32ArrayType => TypedArrayType::Int32,
        JSType::Uint32ArrayType => TypedArrayType::Uint32,
        JSType::Float16ArrayType => TypedArrayType::Float16,
        JSType::Float32ArrayType => TypedArrayType::Float32,
        JSType::Float64ArrayType => TypedArrayType::Float64,
        JSType::BigInt64ArrayType => TypedArrayType::BigInt64,
        JSType::BigUint64ArrayType => TypedArrayType::BigUint64,
        JSType::DataViewType => TypedArrayType::DataView,
        _ => TypedArrayType::NotTypedArray,
    }
}

/// `isTypedView(JSType)`: `[FirstTypedArrayType, LastTypedArrayTypeExcludingDataView]`.
pub fn is_typed_view(type_: JSType) -> bool {
    let value = type_ as u32;
    value >= FIRST_TYPED_ARRAY_TYPE && value <= JSType::BigUint64ArrayType as u32
}

/// `contentType(JSType)`.
pub fn content_type(type_: JSType) -> TypedArrayContentType {
    typed_array_type(type_).content_type()
}

/// `elementSize(JSType)`.
pub fn element_size(type_: JSType) -> usize {
    typed_array_type(type_).element_size()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_type_round_trips_and_sizes() {
        for (index, ty) in TYPED_ARRAY_TYPES_EXCLUDING_DATA_VIEW.iter().enumerate() {
            assert_eq!(ty.to_index(), index);
            assert_eq!(TypedArrayType::from_index(index), *ty);
            assert_eq!(typed_array_type(ty.js_type()), *ty);
            assert!(is_typed_view(ty.js_type()));
            assert_eq!(
                ty.js_type() as u32 - FIRST_TYPED_ARRAY_TYPE,
                index as u32,
                "ordem do JSType diverge da de TypedArrayType"
            );
        }
        assert!(!is_typed_view(JSType::DataViewType));
        assert_eq!(TypedArrayType::DataView.js_type(), JSType::DataViewType);
        assert_eq!(TypedArrayType::Float64.element_size(), 8);
        assert_eq!(TypedArrayType::Float16.element_size(), 2);
        assert_eq!(TypedArrayType::Uint8Clamped.element_size(), 1);
    }
}
