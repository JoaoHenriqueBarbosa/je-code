//! Tradução de `wasm/WasmSIMDOpcodes.h`: `SIMDLaneOperation`, a tabela `ExtSIMDOpType`
//! (`FOR_EACH_WASM_EXT_SIMD_GENERAL_OP` mais `FOR_EACH_WASM_EXT_SIMD_REL_OP`, 256 opcodes), e o que
//! `jit/SIMDInfo.h` e `WasmTypeDefinition.h` dão a ela (`SIMDLane`, `SIMDSignMode`, `elementCount`,
//! `simdScalarType`, `isRelaxedSIMDOperation`).
//!
//! O quinto argumento das linhas de comparação do C++ (`B3::Air::Arg::relCond(...)`) só serve ao BBQ
//! e ao OMG, que não se portam, então a tabela não o carrega. Cada linha é
//! `nome = opcode, operação, lane, modo de sinal`, na mesma ordem do C++. As duas últimas
//! comparações de `i64x2` (`I64x2LeU` e `I64x2GeU`, 0xda e 0xdb) estão com o modo `Unsigned` porque o
//! cabeçalho do JSC as declara assim, embora o opcode seja o `le_s`/`ge_s` da especificação: o C++
//! decide.

use crate::wasm::wasm_format::{Type, TypeIndex, TypeKind};

/// `enum class SIMDLane`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimdLane {
    V128,
    I8x16,
    I16x8,
    I32x4,
    I64x2,
    F32x4,
    F64x2,
}

impl SimdLane {
    /// `elementCount(SIMDLane)`.
    pub fn element_count(self) -> u8 {
        match self {
            SimdLane::I8x16 => 16,
            SimdLane::I16x8 => 8,
            SimdLane::F32x4 | SimdLane::I32x4 => 4,
            SimdLane::F64x2 | SimdLane::I64x2 => 2,
            SimdLane::V128 => unreachable!("elementCount de v128"),
        }
    }

    /// `simdScalarType(SIMDLane)`.
    pub fn scalar_type(self) -> Type {
        let kind = match self {
            SimdLane::V128 => unreachable!("simdScalarType de v128"),
            SimdLane::I64x2 => TypeKind::I64,
            SimdLane::F64x2 => TypeKind::F64,
            SimdLane::I8x16 | SimdLane::I16x8 | SimdLane::I32x4 => TypeKind::I32,
            SimdLane::F32x4 => TypeKind::F32,
        };
        Type::new(kind, TypeIndex::Invalid)
    }
}

/// `enum class SIMDSignMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimdSignMode {
    None,
    Signed,
    Unsigned,
}

macro_rules! simd_lane_operations {
    ($($name:ident),* $(,)?) => {
        /// `enum class SIMDLaneOperation`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum SimdLaneOperation {
            $($name),*
        }

        impl SimdLaneOperation {
            /// `dumpSIMDLaneOperation`.
            pub fn name(self) -> &'static str {
                match self {
                    $(SimdLaneOperation::$name => stringify!($name)),*
                }
            }
        }
    };
}

simd_lane_operations! {
    Not, AddSat, LoadLane16, Andnot, GreaterThan, Abs, And, Store, StoreLane8, Xor, Trunc, ExtmulHigh, Splat,
    LoadExtend16S, LoadExtend8U, GreaterThanOrEqual, LoadLane8, Shl, DotProduct, Const, Demote, Convert,
    NotEqual, Sqrt, Pmin, StoreLane32, Pmax, LoadExtend32S, ExtendHigh, StoreLane64, LessThan, AvgRound,
    Min, Equal, LoadSplat8, LoadSplat32, LoadPad64, LoadExtend32U, Promote, Narrow, Load, Shuffle, SubSat,
    Max, AnyTrue, Ceil, LoadLane32, Mul, StoreLane16, Nearest, LoadExtend8S, ExtmulLow, Swizzle, Div,
    ConvertLow, Floor, LoadSplat16, AllTrue, Popcnt, LessThanOrEqual, Or, LoadExtend16U, ExtaddPairwise,
    TruncSat, LoadPad32, ExtractLane, ReplaceLane, ExtendLow, Shr, Add, LoadSplat64, LoadLane64,
    BitwiseSelect, Sub, Bitmask, Neg, MulSat,
    // relaxed SIMD
    RelaxedSwizzle, RelaxedTruncSat, RelaxedMAdd, RelaxedNMAdd, RelaxedLaneSelect, RelaxedMin, RelaxedMax,
    RelaxedQ15Mulr, RelaxedDotI8x16I7x16, RelaxedDotI8x16I7x16Add,
}

impl SimdLaneOperation {
    /// `isRelaxedSIMDOperation`.
    pub fn is_relaxed(self) -> bool {
        matches!(
            self,
            SimdLaneOperation::RelaxedSwizzle
                | SimdLaneOperation::RelaxedTruncSat
                | SimdLaneOperation::RelaxedMAdd
                | SimdLaneOperation::RelaxedNMAdd
                | SimdLaneOperation::RelaxedLaneSelect
                | SimdLaneOperation::RelaxedMin
                | SimdLaneOperation::RelaxedMax
                | SimdLaneOperation::RelaxedQ15Mulr
                | SimdLaneOperation::RelaxedDotI8x16I7x16
                | SimdLaneOperation::RelaxedDotI8x16I7x16Add
        )
    }
}

macro_rules! ext_simd_ops {
    ($($name:ident = $value:literal, $op:ident, $lane:ident, $sign:ident);* $(;)?) => {
        /// `enum class ExtSIMDOpType : uint32_t` (prefixo `0xFD`).
        #[repr(u32)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum ExtSimdOpType {
            $($name = $value),*
        }

        impl ExtSimdOpType {
            /// Todos os valores do enum.
            pub const ALL: &'static [ExtSimdOpType] = &[$(ExtSimdOpType::$name),*];

            /// O `static_cast` do inteiro lido do binário; `None` é o `default:` do `switch` do C++
            /// ("invalid extended simd op").
            pub fn from_value(value: u32) -> Option<ExtSimdOpType> {
                match value {
                    $($value => Some(ExtSimdOpType::$name),)*
                    _ => None,
                }
            }

            pub fn value(self) -> u32 {
                self as u32
            }

            /// `makeString(ExtSIMDOpType)`.
            pub fn name(self) -> &'static str {
                match self {
                    $(ExtSimdOpType::$name => stringify!($name)),*
                }
            }

            /// Os três argumentos que `CREATE_SIMD_CASE` passa a `simd<isReachable>`.
            pub fn info(self) -> (SimdLaneOperation, SimdLane, SimdSignMode) {
                match self {
                    $(ExtSimdOpType::$name => (SimdLaneOperation::$op, SimdLane::$lane, SimdSignMode::$sign)),*
                }
            }
        }
    };
}

ext_simd_ops! {
    // FOR_EACH_WASM_EXT_SIMD_REL_OP
    I8x16Eq = 0x23, Equal, I8x16, None;
    I8x16Ne = 0x24, NotEqual, I8x16, None;
    I8x16LtS = 0x25, LessThan, I8x16, Signed;
    I8x16LtU = 0x26, LessThan, I8x16, Unsigned;
    I8x16GtS = 0x27, GreaterThan, I8x16, Signed;
    I8x16GtU = 0x28, GreaterThan, I8x16, Unsigned;
    I8x16LeS = 0x29, LessThanOrEqual, I8x16, Signed;
    I8x16LeU = 0x2a, LessThanOrEqual, I8x16, Unsigned;
    I8x16GeS = 0x2b, GreaterThanOrEqual, I8x16, Signed;
    I8x16GeU = 0x2c, GreaterThanOrEqual, I8x16, Unsigned;
    I16x8Eq = 0x2d, Equal, I16x8, None;
    I16x8Ne = 0x2e, NotEqual, I16x8, None;
    I16x8LtS = 0x2f, LessThan, I16x8, Signed;
    I16x8LtU = 0x30, LessThan, I16x8, Unsigned;
    I16x8GtS = 0x31, GreaterThan, I16x8, Signed;
    I16x8GtU = 0x32, GreaterThan, I16x8, Unsigned;
    I16x8LeS = 0x33, LessThanOrEqual, I16x8, Signed;
    I16x8LeU = 0x34, LessThanOrEqual, I16x8, Unsigned;
    I16x8GeS = 0x35, GreaterThanOrEqual, I16x8, Signed;
    I16x8GeU = 0x36, GreaterThanOrEqual, I16x8, Unsigned;
    I32x4Eq = 0x37, Equal, I32x4, None;
    I32x4Ne = 0x38, NotEqual, I32x4, None;
    I32x4LtS = 0x39, LessThan, I32x4, Signed;
    I32x4LtU = 0x3a, LessThan, I32x4, Unsigned;
    I32x4GtS = 0x3b, GreaterThan, I32x4, Signed;
    I32x4GtU = 0x3c, GreaterThan, I32x4, Unsigned;
    I32x4LeS = 0x3d, LessThanOrEqual, I32x4, Signed;
    I32x4LeU = 0x3e, LessThanOrEqual, I32x4, Unsigned;
    I32x4GeS = 0x3f, GreaterThanOrEqual, I32x4, Signed;
    I32x4GeU = 0x40, GreaterThanOrEqual, I32x4, Unsigned;
    F32x4Eq = 0x41, Equal, F32x4, None;
    F32x4Ne = 0x42, NotEqual, F32x4, None;
    F32x4Lt = 0x43, LessThan, F32x4, None;
    F32x4Gt = 0x44, GreaterThan, F32x4, None;
    F32x4Le = 0x45, LessThanOrEqual, F32x4, None;
    F32x4Ge = 0x46, GreaterThanOrEqual, F32x4, None;
    F64x2Eq = 0x47, Equal, F64x2, None;
    F64x2Ne = 0x48, NotEqual, F64x2, None;
    F64x2Lt = 0x49, LessThan, F64x2, None;
    F64x2Gt = 0x4a, GreaterThan, F64x2, None;
    F64x2Le = 0x4b, LessThanOrEqual, F64x2, None;
    F64x2Ge = 0x4c, GreaterThanOrEqual, F64x2, None;
    I64x2Eq = 0xd6, Equal, I64x2, None;
    I64x2Ne = 0xd7, NotEqual, I64x2, None;
    I64x2LtS = 0xd8, LessThan, I64x2, Signed;
    I64x2GtS = 0xd9, GreaterThan, I64x2, Signed;
    I64x2LeU = 0xda, LessThanOrEqual, I64x2, Unsigned;
    I64x2GeU = 0xdb, GreaterThanOrEqual, I64x2, Unsigned;
    // FOR_EACH_WASM_EXT_SIMD_GENERAL_OP
    I8x16Swizzle = 0x0e, Swizzle, I8x16, None;
    I8x16ExtractLaneS = 0x15, ExtractLane, I8x16, Signed;
    I8x16ExtractLaneU = 0x16, ExtractLane, I8x16, Unsigned;
    I8x16ReplaceLane = 0x17, ReplaceLane, I8x16, None;
    I16x8ExtractLaneS = 0x18, ExtractLane, I16x8, Signed;
    I16x8ExtractLaneU = 0x19, ExtractLane, I16x8, Unsigned;
    I16x8ReplaceLane = 0x1a, ReplaceLane, I16x8, None;
    I32x4ExtractLane = 0x1b, ExtractLane, I32x4, None;
    I32x4ReplaceLane = 0x1c, ReplaceLane, I32x4, None;
    I64x2ExtractLane = 0x1d, ExtractLane, I64x2, None;
    I64x2ReplaceLane = 0x1e, ReplaceLane, I64x2, None;
    F32x4ExtractLane = 0x1f, ExtractLane, F32x4, None;
    F32x4ReplaceLane = 0x20, ReplaceLane, F32x4, None;
    F64x2ExtractLane = 0x21, ExtractLane, F64x2, None;
    F64x2ReplaceLane = 0x22, ReplaceLane, F64x2, None;
    V128Not = 0x4d, Not, V128, None;
    V128And = 0x4e, And, V128, None;
    V128Andnot = 0x4f, Andnot, V128, None;
    V128Or = 0x50, Or, V128, None;
    V128Xor = 0x51, Xor, V128, None;
    V128Bitselect = 0x52, BitwiseSelect, V128, None;
    V128AnyTrue = 0x53, AnyTrue, V128, None;
    F32x4DemoteF64x2Zero = 0x5e, Demote, F64x2, None;
    F64x2PromoteLowF32x4 = 0x5f, Promote, F32x4, None;
    I8x16Abs = 0x60, Abs, I8x16, None;
    I8x16Neg = 0x61, Neg, I8x16, None;
    I8x16Popcnt = 0x62, Popcnt, I8x16, None;
    I8x16AllTrue = 0x63, AllTrue, I8x16, None;
    I8x16Bitmask = 0x64, Bitmask, I8x16, None;
    I8x16NarrowI16x8S = 0x65, Narrow, I16x8, Signed;
    I8x16NarrowI16x8U = 0x66, Narrow, I16x8, Unsigned;
    F32x4Ceil = 0x67, Ceil, F32x4, None;
    F32x4Floor = 0x68, Floor, F32x4, None;
    F32x4Trunc = 0x69, Trunc, F32x4, None;
    F32x4Nearest = 0x6a, Nearest, F32x4, None;
    I8x16Add = 0x6e, Add, I8x16, None;
    I8x16AddSatS = 0x6f, AddSat, I8x16, Signed;
    I8x16AddSatU = 0x70, AddSat, I8x16, Unsigned;
    I8x16Sub = 0x71, Sub, I8x16, None;
    I8x16SubSatS = 0x72, SubSat, I8x16, Signed;
    I8x16SubSatU = 0x73, SubSat, I8x16, Unsigned;
    F64x2Ceil = 0x74, Ceil, F64x2, None;
    F64x2Floor = 0x75, Floor, F64x2, None;
    I8x16MinS = 0x76, Min, I8x16, Signed;
    I8x16MinU = 0x77, Min, I8x16, Unsigned;
    I8x16MaxS = 0x78, Max, I8x16, Signed;
    I8x16MaxU = 0x79, Max, I8x16, Unsigned;
    F64x2Trunc = 0x7a, Trunc, F64x2, None;
    I8x16AvgrU = 0x7b, AvgRound, I8x16, None;
    I16x8ExtaddPairwiseI8x16S = 0x7c, ExtaddPairwise, I8x16, Signed;
    I16x8ExtaddPairwiseI8x16U = 0x7d, ExtaddPairwise, I8x16, Unsigned;
    I32x4ExtaddPairwiseI16x8S = 0x7e, ExtaddPairwise, I16x8, Signed;
    I32x4ExtaddPairwiseI16x8U = 0x7f, ExtaddPairwise, I16x8, Unsigned;
    I16x8Abs = 0x80, Abs, I16x8, None;
    I16x8Neg = 0x81, Neg, I16x8, None;
    I16x8Q15mulrSatS = 0x82, MulSat, I16x8, None;
    I16x8AllTrue = 0x83, AllTrue, I16x8, None;
    I16x8Bitmask = 0x84, Bitmask, I16x8, None;
    I16x8NarrowI32x4S = 0x85, Narrow, I32x4, Signed;
    I16x8NarrowI32x4U = 0x86, Narrow, I32x4, Unsigned;
    I16x8ExtendLowI8x16S = 0x87, ExtendLow, I16x8, Signed;
    I16x8ExtendHighI8x16S = 0x88, ExtendHigh, I16x8, Signed;
    I16x8ExtendLowI8x16U = 0x89, ExtendLow, I16x8, Unsigned;
    I16x8ExtendHighI8x16U = 0x8a, ExtendHigh, I16x8, Unsigned;
    I16x8Add = 0x8e, Add, I16x8, None;
    I16x8AddSatS = 0x8f, AddSat, I16x8, Signed;
    I16x8AddSatU = 0x90, AddSat, I16x8, Unsigned;
    I16x8Sub = 0x91, Sub, I16x8, None;
    I16x8SubSatS = 0x92, SubSat, I16x8, Signed;
    I16x8SubSatU = 0x93, SubSat, I16x8, Unsigned;
    F64x2Nearest = 0x94, Nearest, F64x2, None;
    I16x8Mul = 0x95, Mul, I16x8, None;
    I16x8MinS = 0x96, Min, I16x8, Signed;
    I16x8MinU = 0x97, Min, I16x8, Unsigned;
    I16x8MaxS = 0x98, Max, I16x8, Signed;
    I16x8MaxU = 0x99, Max, I16x8, Unsigned;
    I16x8AvgrU = 0x9b, AvgRound, I16x8, None;
    I32x4Abs = 0xa0, Abs, I32x4, None;
    I32x4Neg = 0xa1, Neg, I32x4, None;
    I32x4AllTrue = 0xa3, AllTrue, I32x4, None;
    I32x4Bitmask = 0xa4, Bitmask, I32x4, None;
    I32x4ExtendLowI16x8S = 0xa7, ExtendLow, I32x4, Signed;
    I32x4ExtendHighI16x8S = 0xa8, ExtendHigh, I32x4, Signed;
    I32x4ExtendLowI16x8U = 0xa9, ExtendLow, I32x4, Unsigned;
    I32x4ExtendHighI16x8U = 0xaa, ExtendHigh, I32x4, Unsigned;
    I32x4Add = 0xae, Add, I32x4, None;
    I32x4Sub = 0xb1, Sub, I32x4, None;
    I32x4Mul = 0xb5, Mul, I32x4, None;
    I32x4MinS = 0xb6, Min, I32x4, Signed;
    I32x4MinU = 0xb7, Min, I32x4, Unsigned;
    I32x4MaxS = 0xb8, Max, I32x4, Signed;
    I32x4MaxU = 0xb9, Max, I32x4, Unsigned;
    I32x4DotI16x8S = 0xba, DotProduct, I32x4, None;
    I64x2Abs = 0xc0, Abs, I64x2, None;
    I64x2Neg = 0xc1, Neg, I64x2, None;
    I64x2AllTrue = 0xc3, AllTrue, I64x2, None;
    I64x2Bitmask = 0xc4, Bitmask, I64x2, None;
    I64x2ExtendLowI32x4S = 0xc7, ExtendLow, I64x2, Signed;
    I64x2ExtendHighI32x4S = 0xc8, ExtendHigh, I64x2, Signed;
    I64x2ExtendLowI32x4U = 0xc9, ExtendLow, I64x2, Unsigned;
    I64x2ExtendHighI32x4U = 0xca, ExtendHigh, I64x2, Unsigned;
    I64x2Add = 0xce, Add, I64x2, None;
    I64x2Sub = 0xd1, Sub, I64x2, None;
    I64x2Mul = 0xd5, Mul, I64x2, None;
    F32x4Abs = 0xe0, Abs, F32x4, None;
    F32x4Neg = 0xe1, Neg, F32x4, None;
    F32x4Sqrt = 0xe3, Sqrt, F32x4, None;
    F32x4Add = 0xe4, Add, F32x4, None;
    F32x4Sub = 0xe5, Sub, F32x4, None;
    F32x4Mul = 0xe6, Mul, F32x4, None;
    F32x4Div = 0xe7, Div, F32x4, None;
    F32x4Min = 0xe8, Min, F32x4, None;
    F32x4Max = 0xe9, Max, F32x4, None;
    F32x4Pmin = 0xea, Pmin, F32x4, None;
    F32x4Pmax = 0xeb, Pmax, F32x4, None;
    F64x2Abs = 0xec, Abs, F64x2, None;
    F64x2Neg = 0xed, Neg, F64x2, None;
    F64x2Sqrt = 0xef, Sqrt, F64x2, None;
    F64x2Add = 0xf0, Add, F64x2, None;
    F64x2Sub = 0xf1, Sub, F64x2, None;
    F64x2Mul = 0xf2, Mul, F64x2, None;
    F64x2Div = 0xf3, Div, F64x2, None;
    F64x2Min = 0xf4, Min, F64x2, None;
    F64x2Max = 0xf5, Max, F64x2, None;
    F64x2Pmin = 0xf6, Pmin, F64x2, None;
    F64x2Pmax = 0xf7, Pmax, F64x2, None;
    I32x4TruncSatF32x4S = 0xf8, TruncSat, F32x4, Signed;
    I32x4TruncSatF32x4U = 0xf9, TruncSat, F32x4, Unsigned;
    F32x4ConvertI32x4S = 0xfa, Convert, I32x4, Signed;
    F32x4ConvertI32x4U = 0xfb, Convert, I32x4, Unsigned;
    I32x4TruncSatF64x2SZero = 0xfc, TruncSat, F64x2, Signed;
    I32x4TruncSatF64x2UZero = 0xfd, TruncSat, F64x2, Unsigned;
    F64x2ConvertLowI32x4S = 0xfe, ConvertLow, I32x4, Signed;
    F64x2ConvertLowI32x4U = 0xff, ConvertLow, I32x4, Unsigned;
    V128Load = 0x00, Load, V128, None;
    V128Load8x8S = 0x01, LoadExtend8S, V128, Signed;
    V128Load8x8U = 0x02, LoadExtend8U, V128, Unsigned;
    V128Load16x4S = 0x03, LoadExtend16S, V128, Signed;
    V128Load16x4U = 0x04, LoadExtend16U, V128, Unsigned;
    V128Load32x2S = 0x05, LoadExtend32S, V128, Signed;
    V128Load32x2U = 0x06, LoadExtend32U, V128, Unsigned;
    V128Load8Splat = 0x07, LoadSplat8, V128, None;
    V128Load16Splat = 0x08, LoadSplat16, V128, None;
    V128Load32Splat = 0x09, LoadSplat32, V128, None;
    V128Load64Splat = 0x0a, LoadSplat64, V128, None;
    V128Store = 0x0b, Store, V128, None;
    V128Const = 0x0c, Const, V128, None;
    I8x16Shuffle = 0x0d, Shuffle, I8x16, None;
    I8x16Splat = 0x0f, Splat, I8x16, None;
    I16x8Splat = 0x10, Splat, I16x8, None;
    I32x4Splat = 0x11, Splat, I32x4, None;
    I64x2Splat = 0x12, Splat, I64x2, None;
    F32x4Splat = 0x13, Splat, F32x4, None;
    F64x2Splat = 0x14, Splat, F64x2, None;
    V128Load8Lane = 0x54, LoadLane8, V128, None;
    V128Load16Lane = 0x55, LoadLane16, V128, None;
    V128Load32Lane = 0x56, LoadLane32, V128, None;
    V128Load64Lane = 0x57, LoadLane64, V128, None;
    V128Store8Lane = 0x58, StoreLane8, V128, None;
    V128Store16Lane = 0x59, StoreLane16, V128, None;
    V128Store32Lane = 0x5a, StoreLane32, V128, None;
    V128Store64Lane = 0x5b, StoreLane64, V128, None;
    V128Load32Zero = 0x5c, LoadPad32, V128, None;
    V128Load64Zero = 0x5d, LoadPad64, V128, None;
    I8x16Shl = 0x6b, Shl, I8x16, None;
    I8x16ShrS = 0x6c, Shr, I8x16, Signed;
    I8x16ShrU = 0x6d, Shr, I8x16, Unsigned;
    I16x8Shl = 0x8b, Shl, I16x8, None;
    I16x8ShrS = 0x8c, Shr, I16x8, Signed;
    I16x8ShrU = 0x8d, Shr, I16x8, Unsigned;
    I16x8ExtmulLowI8x16S = 0x9c, ExtmulLow, I16x8, Signed;
    I16x8ExtmulHighI8x16S = 0x9d, ExtmulHigh, I16x8, Signed;
    I16x8ExtmulLowI8x16U = 0x9e, ExtmulLow, I16x8, Unsigned;
    I16x8ExtmulHighI8x16U = 0x9f, ExtmulHigh, I16x8, Unsigned;
    I32x4Shl = 0xab, Shl, I32x4, None;
    I32x4ShrS = 0xac, Shr, I32x4, Signed;
    I32x4ShrU = 0xad, Shr, I32x4, Unsigned;
    I32x4ExtmulLowI16x8S = 0xbc, ExtmulLow, I32x4, Signed;
    I32x4ExtmulHighI16x8S = 0xbd, ExtmulHigh, I32x4, Signed;
    I32x4ExtmulLowI16x8U = 0xbe, ExtmulLow, I32x4, Unsigned;
    I32x4ExtmulHighI16x8U = 0xbf, ExtmulHigh, I32x4, Unsigned;
    I64x2Shl = 0xcb, Shl, I64x2, None;
    I64x2ShrS = 0xcc, Shr, I64x2, Signed;
    I64x2ShrU = 0xcd, Shr, I64x2, Unsigned;
    I64x2ExtmulLowI32x4S = 0xdc, ExtmulLow, I64x2, Signed;
    I64x2ExtmulHighI32x4S = 0xdd, ExtmulHigh, I64x2, Signed;
    I64x2ExtmulLowI32x4U = 0xde, ExtmulLow, I64x2, Unsigned;
    I64x2ExtmulHighI32x4U = 0xdf, ExtmulHigh, I64x2, Unsigned;
    // relaxed SIMD
    I8x16RelaxedSwizzle = 0x100, RelaxedSwizzle, I8x16, None;
    I32x4RelaxedTruncF32x4S = 0x101, RelaxedTruncSat, F32x4, Signed;
    I32x4RelaxedTruncF32x4U = 0x102, RelaxedTruncSat, F32x4, Unsigned;
    I32x4RelaxedTruncF64x2SZero = 0x103, RelaxedTruncSat, F64x2, Signed;
    I32x4RelaxedTruncF64x2UZero = 0x104, RelaxedTruncSat, F64x2, Unsigned;
    F32x4RelaxedMAdd = 0x105, RelaxedMAdd, F32x4, None;
    F32x4RelaxedNMAdd = 0x106, RelaxedNMAdd, F32x4, None;
    F64x2RelaxedMAdd = 0x107, RelaxedMAdd, F64x2, None;
    F64x2RelaxedNMAdd = 0x108, RelaxedNMAdd, F64x2, None;
    I8x16RelaxedLaneSelect = 0x109, RelaxedLaneSelect, I8x16, None;
    I16x8RelaxedLaneSelect = 0x10a, RelaxedLaneSelect, I16x8, None;
    I32x4RelaxedLaneSelect = 0x10b, RelaxedLaneSelect, I32x4, None;
    I64x2RelaxedLaneSelect = 0x10c, RelaxedLaneSelect, I64x2, None;
    F32x4RelaxedMin = 0x10d, RelaxedMin, F32x4, None;
    F32x4RelaxedMax = 0x10e, RelaxedMax, F32x4, None;
    F64x2RelaxedMin = 0x10f, RelaxedMin, F64x2, None;
    F64x2RelaxedMax = 0x110, RelaxedMax, F64x2, None;
    I16x8RelaxedQ15MulrS = 0x111, RelaxedQ15Mulr, I16x8, Signed;
    I16x8RelaxedDotI8x16I7x16S = 0x112, RelaxedDotI8x16I7x16, I16x8, Signed;
    I32x4RelaxedDotI8x16I7x16AddS = 0x113, RelaxedDotI8x16I7x16Add, I32x4, Signed;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_the_256_opcodes_of_the_header() {
        assert_eq!(ExtSimdOpType::ALL.len(), 256);
        let mut seen = std::collections::HashSet::new();
        for op in ExtSimdOpType::ALL {
            assert!(seen.insert(op.value()), "{} repetido", op.name());
            assert_eq!(ExtSimdOpType::from_value(op.value()), Some(*op));
        }
    }

    #[test]
    fn spot_checks_against_the_header() {
        assert_eq!(ExtSimdOpType::V128Const.value(), 0x0c);
        assert_eq!(
            ExtSimdOpType::I32x4ShrU.info(),
            (SimdLaneOperation::Shr, SimdLane::I32x4, SimdSignMode::Unsigned)
        );
        // O cabeçalho do JSC declara 0xda como `LeU`, não `LeS`: o C++ decide.
        assert_eq!(
            ExtSimdOpType::from_value(0xda).unwrap().info(),
            (SimdLaneOperation::LessThanOrEqual, SimdLane::I64x2, SimdSignMode::Unsigned)
        );
        assert_eq!(ExtSimdOpType::from_value(0x0e), Some(ExtSimdOpType::I8x16Swizzle));
        assert_eq!(ExtSimdOpType::from_value(0x0f), Some(ExtSimdOpType::I8x16Splat));
        // Buracos da tabela: 0x9a, 0xa2, 0xaf..0xb0, 0xb2..0xb4, 0x114.
        for hole in [0x9a, 0xa2, 0xaf, 0xb0, 0xb2, 0xb3, 0xb4, 0x114] {
            assert_eq!(ExtSimdOpType::from_value(hole), None, "{:#x}", hole);
        }
    }

    #[test]
    fn lanes() {
        assert_eq!(SimdLane::I16x8.element_count(), 8);
        assert_eq!(SimdLane::F64x2.scalar_type().kind, TypeKind::F64);
        assert_eq!(SimdLane::I8x16.scalar_type().kind, TypeKind::I32);
        assert!(SimdLaneOperation::RelaxedQ15Mulr.is_relaxed());
        assert!(!SimdLaneOperation::Add.is_relaxed());
        assert_eq!(SimdLaneOperation::LoadSplat16.name(), "LoadSplat16");
    }
}
