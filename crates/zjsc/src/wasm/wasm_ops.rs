//! Tradução de `wasm/WasmOps.h`, que o `generateWasmOpsHeader.py` produz a partir de `wasm.json`.
//!
//! O C++ gera uma lista de macros por categoria (`FOR_EACH_WASM_SPECIAL_OP`,
//! `FOR_EACH_WASM_CONTROL_FLOW_OP`, `FOR_EACH_WASM_UNARY_OP`...) e monta cada enum a partir delas.
//! Aqui cada lista também existe uma vez só, dentro de `wasm_op_lists!` (opcodes de um byte) e
//! `wasm_ext_op_lists!` (opcodes com prefixo `0xFB`, `0xFC` e `0xFE`), e cada enum sai de uma
//! invocação que escolhe os grupos que quer.
//!
//! Os nomes dos variantes são o `toCpp` do gerador (`i32.trunc_s/f32` vira `I32TruncSF32`), o
//! valor é o `value` (ou o `extendedOp`) de `wasm.json`. O que o C++ passa como argumentos extras
//! da macro vira dado do variante:
//!
//! - `types()`: os tipos dos parâmetros seguidos dos de retorno (unários, binários, saturados e
//!   aritmética larga), o tipo carregado (loads) ou o tipo do valor armazenado (stores e
//!   atômicos). Um `bool` do JSON é `I32`. Vazio nos opcodes cujas macros do C++ não trazem tipos.
//! - `log2_alignment()`: o `memoryLog2Alignment` dos acessos à memória e dos atômicos.
//!
//! Fica de fora o que só o JIT usa: o `b3op` e o `inc` de cada macro, `linearizeType`,
//! `linearizedToType` e `Type::width`. `ExtSIMDOpType` vem de `WasmSIMDOpcodes.h` e está em
//! `wasm_simd_opcodes.rs`; o único valor que as seções usam (`V128Const`) tem a constante
//! `EXT_SIMD_V128_CONST` aqui.

use crate::wasm::wasm_format::TypeKind;

/// `expectedVersionNumber`.
pub const EXPECTED_VERSION_NUMBER: u32 = 1;

/// `numTypes`.
pub const NUM_TYPES: usize = 20;

/// `minTypeValue`.
pub const MIN_TYPE_VALUE: i32 = -64;

/// `ExtSIMDOpType::V128Const` (`WasmSIMDOpcodes.h`), o opcode que `parseInitExpr` aceita depois do
/// prefixo `ExtSIMD`.
pub const EXT_SIMD_V128_CONST: u32 = 0x0c;

/// Um enum `repr(inteiro)` com `ALL`, `from_value`, `value` e `name` (o `makeString` do C++).
/// Cada item é `Nome = valor`, com `: [tipos]` e `@ alinhamento` opcionais, que este macro ignora.
macro_rules! wasm_enum {
    (
        $(#[$meta:meta])*
        $name:ident : $repr:ident {
            $($variant:ident = $value:literal $(: [$($ty:ident),*])? $(@ $align:literal)?),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[repr($repr)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant = $value),*
        }

        impl $name {
            /// Todos os valores do enum.
            pub const ALL: &'static [$name] = &[$($name::$variant),*];

            /// O `static_cast` do inteiro lido do binário; `None` é o valor que o enum não tem.
            pub fn from_value(value: $repr) -> Option<$name> {
                match value {
                    $($value => Some($name::$variant),)*
                    _ => None,
                }
            }

            pub fn value(self) -> $repr {
                self as $repr
            }

            /// `makeString`.
            pub fn name(self) -> &'static str {
                match self {
                    $($name::$variant => stringify!($variant)),*
                }
            }
        }
    };
}

/// O `types()` de um enum cujos itens trazem `: [tipos]`.
macro_rules! wasm_types_impl {
    (
        $name:ident {
            $($variant:ident = $value:literal : [$($ty:ident),*] $(@ $align:literal)?),* $(,)?
        }
    ) => {
        impl $name {
            pub fn types(self) -> &'static [TypeKind] {
                match self {
                    $($name::$variant => &[$(TypeKind::$ty),*]),*
                }
            }
        }
    };
}

/// O `log2_alignment()` de um enum cujos itens trazem `@ alinhamento`.
macro_rules! wasm_alignment_impl {
    (
        $name:ident {
            $($variant:ident = $value:literal $(: [$($ty:ident),*])? @ $align:literal),* $(,)?
        }
    ) => {
        impl $name {
            /// `memoryLog2Alignment`.
            pub fn log2_alignment(self) -> u32 {
                match self {
                    $($name::$variant => $align),*
                }
            }
        }
    };
}

/// As listas de opcodes de um byte, na ordem das macros do C++ (`special`, `control`, `unary`,
/// `binary`, `load`, `store`). Entrega as seis a um callback.
macro_rules! wasm_op_lists {
    ($callback:ident) => {
        $callback! {
            special {
                I32Const = 65, I64Const = 66, F64Const = 68, F32Const = 67,
                RefNull = 208, RefIsNull = 209, RefFunc = 210, RefEq = 211, RefAsNonNull = 212,
                GetLocal = 32, SetLocal = 33, TeeLocal = 34, GetGlobal = 35, SetGlobal = 36,
                TableGet = 37, TableSet = 38,
                Call = 16, CallIndirect = 17, CallRef = 20,
                TailCall = 18, TailCallIndirect = 19, TailCallRef = 21,
                CurrentMemory = 63, GrowMemory = 64,
            }
            control {
                Unreachable = 0, Nop = 1, Block = 2, Loop = 3, If = 4, Else = 5, Try = 6,
                Catch = 7, Throw = 8, Rethrow = 9, ThrowRef = 10, Br = 12, BrIf = 13,
                BrTable = 14, Return = 15, Delegate = 24, CatchAll = 25, Drop = 26, Select = 27,
                AnnotatedSelect = 28, TryTable = 31, End = 11, BrOnNull = 213, BrOnNonNull = 214,
            }
            unary {
                I32Clz = 103 : [I32, I32], I32Ctz = 104 : [I32, I32], I32Popcnt = 105 : [I32, I32],
                I64Clz = 121 : [I64, I64], I64Ctz = 122 : [I64, I64], I64Popcnt = 123 : [I64, I64],
                F32Abs = 139 : [F32, F32], F32Neg = 140 : [F32, F32], F32Ceil = 141 : [F32, F32],
                F32Floor = 142 : [F32, F32], F32Trunc = 143 : [F32, F32],
                F32Nearest = 144 : [F32, F32], F32Sqrt = 145 : [F32, F32],
                F64Abs = 153 : [F64, F64], F64Neg = 154 : [F64, F64], F64Ceil = 155 : [F64, F64],
                F64Floor = 156 : [F64, F64], F64Trunc = 157 : [F64, F64],
                F64Nearest = 158 : [F64, F64], F64Sqrt = 159 : [F64, F64],
                I32TruncSF32 = 168 : [F32, I32], I32TruncSF64 = 170 : [F64, I32],
                I32TruncUF32 = 169 : [F32, I32], I32TruncUF64 = 171 : [F64, I32],
                I32WrapI64 = 167 : [I64, I32],
                I64TruncSF32 = 174 : [F32, I64], I64TruncSF64 = 176 : [F64, I64],
                I64TruncUF32 = 175 : [F32, I64], I64TruncUF64 = 177 : [F64, I64],
                I64ExtendSI32 = 172 : [I32, I64], I64ExtendUI32 = 173 : [I32, I64],
                F32ConvertSI32 = 178 : [I32, F32], F32ConvertUI32 = 179 : [I32, F32],
                F32ConvertSI64 = 180 : [I64, F32], F32ConvertUI64 = 181 : [I64, F32],
                F32DemoteF64 = 182 : [F64, F32], F32ReinterpretI32 = 190 : [I32, F32],
                F64ConvertSI32 = 183 : [I32, F64], F64ConvertUI32 = 184 : [I32, F64],
                F64ConvertSI64 = 185 : [I64, F64], F64ConvertUI64 = 186 : [I64, F64],
                F64PromoteF32 = 187 : [F32, F64], F64ReinterpretI64 = 191 : [I64, F64],
                I32ReinterpretF32 = 188 : [F32, I32], I64ReinterpretF64 = 189 : [F64, I64],
                I32Extend8S = 192 : [I32, I32], I32Extend16S = 193 : [I32, I32],
                I64Extend8S = 194 : [I64, I64], I64Extend16S = 195 : [I64, I64],
                I64Extend32S = 196 : [I64, I64],
                I32Eqz = 69 : [I32, I32], I64Eqz = 80 : [I64, I32],
            }
            binary {
                I32Add = 106 : [I32, I32, I32], I32Sub = 107 : [I32, I32, I32],
                I32Mul = 108 : [I32, I32, I32], I32DivS = 109 : [I32, I32, I32],
                I32DivU = 110 : [I32, I32, I32], I32RemS = 111 : [I32, I32, I32],
                I32RemU = 112 : [I32, I32, I32], I32And = 113 : [I32, I32, I32],
                I32Or = 114 : [I32, I32, I32], I32Xor = 115 : [I32, I32, I32],
                I32Shl = 116 : [I32, I32, I32], I32ShrU = 118 : [I32, I32, I32],
                I32ShrS = 117 : [I32, I32, I32], I32Rotr = 120 : [I32, I32, I32],
                I32Rotl = 119 : [I32, I32, I32],
                I64Add = 124 : [I64, I64, I64], I64Sub = 125 : [I64, I64, I64],
                I64Mul = 126 : [I64, I64, I64], I64DivS = 127 : [I64, I64, I64],
                I64DivU = 128 : [I64, I64, I64], I64RemS = 129 : [I64, I64, I64],
                I64RemU = 130 : [I64, I64, I64], I64And = 131 : [I64, I64, I64],
                I64Or = 132 : [I64, I64, I64], I64Xor = 133 : [I64, I64, I64],
                I64Shl = 134 : [I64, I64, I64], I64ShrU = 136 : [I64, I64, I64],
                I64ShrS = 135 : [I64, I64, I64], I64Rotr = 138 : [I64, I64, I64],
                I64Rotl = 137 : [I64, I64, I64],
                F32Add = 146 : [F32, F32, F32], F32Sub = 147 : [F32, F32, F32],
                F32Mul = 148 : [F32, F32, F32], F32Div = 149 : [F32, F32, F32],
                F32Min = 150 : [F32, F32, F32], F32Max = 151 : [F32, F32, F32],
                F32Copysign = 152 : [F32, F32, F32],
                F64Add = 160 : [F64, F64, F64], F64Sub = 161 : [F64, F64, F64],
                F64Mul = 162 : [F64, F64, F64], F64Div = 163 : [F64, F64, F64],
                F64Min = 164 : [F64, F64, F64], F64Max = 165 : [F64, F64, F64],
                F64Copysign = 166 : [F64, F64, F64],
                I32Eq = 70 : [I32, I32, I32], I32Ne = 71 : [I32, I32, I32],
                I32LtS = 72 : [I32, I32, I32], I32LeS = 76 : [I32, I32, I32],
                I32LtU = 73 : [I32, I32, I32], I32LeU = 77 : [I32, I32, I32],
                I32GtS = 74 : [I32, I32, I32], I32GeS = 78 : [I32, I32, I32],
                I32GtU = 75 : [I32, I32, I32], I32GeU = 79 : [I32, I32, I32],
                I64Eq = 81 : [I64, I64, I32], I64Ne = 82 : [I64, I64, I32],
                I64LtS = 83 : [I64, I64, I32], I64LeS = 87 : [I64, I64, I32],
                I64LtU = 84 : [I64, I64, I32], I64LeU = 88 : [I64, I64, I32],
                I64GtS = 85 : [I64, I64, I32], I64GeS = 89 : [I64, I64, I32],
                I64GtU = 86 : [I64, I64, I32], I64GeU = 90 : [I64, I64, I32],
                F32Eq = 91 : [F32, F32, I32], F32Ne = 92 : [F32, F32, I32],
                F32Lt = 93 : [F32, F32, I32], F32Le = 95 : [F32, F32, I32],
                F32Gt = 94 : [F32, F32, I32], F32Ge = 96 : [F32, F32, I32],
                F64Eq = 97 : [F64, F64, I32], F64Ne = 98 : [F64, F64, I32],
                F64Lt = 99 : [F64, F64, I32], F64Le = 101 : [F64, F64, I32],
                F64Gt = 100 : [F64, F64, I32], F64Ge = 102 : [F64, F64, I32],
            }
            load {
                I32Load8S = 44 : [I32] @ 0, I32Load8U = 45 : [I32] @ 0,
                I32Load16S = 46 : [I32] @ 1, I32Load16U = 47 : [I32] @ 1,
                I64Load8S = 48 : [I64] @ 0, I64Load8U = 49 : [I64] @ 0,
                I64Load16S = 50 : [I64] @ 1, I64Load16U = 51 : [I64] @ 1,
                I64Load32S = 52 : [I64] @ 2, I64Load32U = 53 : [I64] @ 2,
                I32Load = 40 : [I32] @ 2, I64Load = 41 : [I64] @ 3,
                F32Load = 42 : [F32] @ 2, F64Load = 43 : [F64] @ 3,
            }
            store {
                I32Store8 = 58 : [I32] @ 0, I32Store16 = 59 : [I32] @ 1,
                I64Store8 = 60 : [I64] @ 0, I64Store16 = 61 : [I64] @ 1,
                I64Store32 = 62 : [I64] @ 2, I32Store = 54 : [I32] @ 2,
                I64Store = 55 : [I64] @ 3, F32Store = 56 : [F32] @ 2, F64Store = 57 : [F64] @ 3,
            }
        }
    };
}

/// Os enums de opcode de um byte e `is_control_op`.
macro_rules! define_op_enums {
    (
        special { $($special:tt)* }
        control { $($control_variant:ident = $control_value:literal),* $(,)? }
        unary { $($unary:tt)* }
        binary { $($binary:tt)* }
        load { $($load:tt)* }
        store { $($store:tt)* }
    ) => {
        wasm_enum!(
            /// `enum OpType : uint8_t`: `FOR_EACH_WASM_OP`, as listas acima mais os quatro prefixos.
            OpType : u8 {
                $($special)*
                $($control_variant = $control_value,)*
                $($unary)*
                $($binary)*
                $($load)*
                $($store)*
                ExtGC = 0xFB, Ext1 = 0xFC, ExtSIMD = 0xFD, ExtAtomic = 0xFE,
            }
        );

        impl OpType {
            /// `isControlOp` e `isControlFlowInstruction`, as duas funções idênticas do C++.
            pub fn is_control_op(self) -> bool {
                matches!(self, $(OpType::$control_variant)|*)
            }
        }

        wasm_enum!(
            /// `enum class UnaryOpType : uint8_t`, com os valores dos opcodes.
            UnaryOpType : u8 { $($unary)* }
        );
        wasm_types_impl!(UnaryOpType { $($unary)* });

        wasm_enum!(
            /// `enum class BinaryOpType : uint8_t`, com os valores dos opcodes.
            BinaryOpType : u8 { $($binary)* }
        );
        wasm_types_impl!(BinaryOpType { $($binary)* });

        wasm_enum!(
            /// `enum class LoadOpType : uint8_t`, com os valores dos opcodes.
            LoadOpType : u8 { $($load)* }
        );
        wasm_types_impl!(LoadOpType { $($load)* });
        wasm_alignment_impl!(LoadOpType { $($load)* });

        wasm_enum!(
            /// `enum class StoreOpType : uint8_t`, com os valores dos opcodes.
            StoreOpType : u8 { $($store)* }
        );
        wasm_types_impl!(StoreOpType { $($store)* });
        wasm_alignment_impl!(StoreOpType { $($store)* });
    };
}

wasm_op_lists!(define_op_enums);

/// As listas dos opcodes com prefixo: `table` (as ops de tabela e de memória em massa, sem tipos
/// no C++), `trunc_sat`, `wide`, `gc`, e os quatro grupos atômicos. Os itens sem tipos no C++ levam
/// `: []` quando o grupo precisa de `types()`.
macro_rules! wasm_ext_op_lists {
    ($callback:ident) => {
        $callback! {
            table {
                MemoryInit = 8 : [], DataDrop = 9 : [], MemoryCopy = 10 : [], MemoryFill = 11 : [],
                TableInit = 12 : [], ElemDrop = 13 : [], TableCopy = 14 : [], TableGrow = 15 : [],
                TableSize = 16 : [], TableFill = 17 : [],
            }
            trunc_sat {
                I32TruncSatF32S = 0 : [F32, I32], I32TruncSatF32U = 1 : [F32, I32],
                I32TruncSatF64S = 2 : [F64, I32], I32TruncSatF64U = 3 : [F64, I32],
                I64TruncSatF32S = 4 : [F32, I64], I64TruncSatF32U = 5 : [F32, I64],
                I64TruncSatF64S = 6 : [F64, I64], I64TruncSatF64U = 7 : [F64, I64],
            }
            wide {
                I64Add128 = 19 : [I64, I64, I64, I64, I64, I64],
                I64Sub128 = 20 : [I64, I64, I64, I64, I64, I64],
                I64MulWideS = 21 : [I64, I64, I64, I64],
                I64MulWideU = 22 : [I64, I64, I64, I64],
            }
            gc {
                StructNew = 0, StructNewDefault = 1, StructGet = 2, StructGetS = 3, StructGetU = 4,
                StructSet = 5, ArrayNew = 6, ArrayNewDefault = 7, ArrayNewFixed = 8,
                ArrayNewData = 9, ArrayNewElem = 10, ArrayGet = 11, ArrayGetS = 12, ArrayGetU = 13,
                ArraySet = 14, ArrayLen = 15, ArrayFill = 16, ArrayCopy = 17, ArrayInitData = 18,
                ArrayInitElem = 19, RefTest = 20, RefTestNull = 21, RefCast = 22, RefCastNull = 23,
                BrOnCast = 24, BrOnCastFail = 25, AnyConvertExtern = 26, ExternConvertAny = 27,
                RefI31 = 28, I31GetS = 29, I31GetU = 30,
            }
            atomic_load {
                I32AtomicLoad = 16 : [I32] @ 2, I64AtomicLoad = 17 : [I64] @ 3,
                I32AtomicLoad8U = 18 : [I32] @ 0, I32AtomicLoad16U = 19 : [I32] @ 1,
                I64AtomicLoad8U = 20 : [I64] @ 0, I64AtomicLoad16U = 21 : [I64] @ 1,
                I64AtomicLoad32U = 22 : [I64] @ 2,
            }
            atomic_store {
                I32AtomicStore = 23 : [I32] @ 2, I64AtomicStore = 24 : [I64] @ 3,
                I32AtomicStore8U = 25 : [I32] @ 0, I32AtomicStore16U = 26 : [I32] @ 1,
                I64AtomicStore8U = 27 : [I64] @ 0, I64AtomicStore16U = 28 : [I64] @ 1,
                I64AtomicStore32U = 29 : [I64] @ 2,
            }
            atomic_rmw_binary {
                I32AtomicRmwAdd = 30 : [I32] @ 2, I64AtomicRmwAdd = 31 : [I64] @ 3,
                I32AtomicRmw8AddU = 32 : [I32] @ 0, I32AtomicRmw16AddU = 33 : [I32] @ 1,
                I64AtomicRmw8AddU = 34 : [I64] @ 0, I64AtomicRmw16AddU = 35 : [I64] @ 1,
                I64AtomicRmw32AddU = 36 : [I64] @ 2,
                I32AtomicRmwSub = 37 : [I32] @ 2, I64AtomicRmwSub = 38 : [I64] @ 3,
                I32AtomicRmw8SubU = 39 : [I32] @ 0, I32AtomicRmw16SubU = 40 : [I32] @ 1,
                I64AtomicRmw8SubU = 41 : [I64] @ 0, I64AtomicRmw16SubU = 42 : [I64] @ 1,
                I64AtomicRmw32SubU = 43 : [I64] @ 2,
                I32AtomicRmwAnd = 44 : [I32] @ 2, I64AtomicRmwAnd = 45 : [I64] @ 3,
                I32AtomicRmw8AndU = 46 : [I32] @ 0, I32AtomicRmw16AndU = 47 : [I32] @ 1,
                I64AtomicRmw8AndU = 48 : [I64] @ 0, I64AtomicRmw16AndU = 49 : [I64] @ 1,
                I64AtomicRmw32AndU = 50 : [I64] @ 2,
                I32AtomicRmwOr = 51 : [I32] @ 2, I64AtomicRmwOr = 52 : [I64] @ 3,
                I32AtomicRmw8OrU = 53 : [I32] @ 0, I32AtomicRmw16OrU = 54 : [I32] @ 1,
                I64AtomicRmw8OrU = 55 : [I64] @ 0, I64AtomicRmw16OrU = 56 : [I64] @ 1,
                I64AtomicRmw32OrU = 57 : [I64] @ 2,
                I32AtomicRmwXor = 58 : [I32] @ 2, I64AtomicRmwXor = 59 : [I64] @ 3,
                I32AtomicRmw8XorU = 60 : [I32] @ 0, I32AtomicRmw16XorU = 61 : [I32] @ 1,
                I64AtomicRmw8XorU = 62 : [I64] @ 0, I64AtomicRmw16XorU = 63 : [I64] @ 1,
                I64AtomicRmw32XorU = 64 : [I64] @ 2,
                I32AtomicRmwXchg = 65 : [I32] @ 2, I64AtomicRmwXchg = 66 : [I64] @ 3,
                I32AtomicRmw8XchgU = 67 : [I32] @ 0, I32AtomicRmw16XchgU = 68 : [I32] @ 1,
                I64AtomicRmw8XchgU = 69 : [I64] @ 0, I64AtomicRmw16XchgU = 70 : [I64] @ 1,
                I64AtomicRmw32XchgU = 71 : [I64] @ 2,
            }
            atomic_other {
                MemoryAtomicNotify = 0 : [] @ 2, MemoryAtomicWait32 = 1 : [] @ 2,
                MemoryAtomicWait64 = 2 : [] @ 3, AtomicFence = 3 : [] @ 0,
                I32AtomicRmwCmpxchg = 72 : [] @ 2, I64AtomicRmwCmpxchg = 73 : [] @ 3,
                I32AtomicRmw8CmpxchgU = 74 : [] @ 0, I32AtomicRmw16CmpxchgU = 75 : [] @ 1,
                I64AtomicRmw8CmpxchgU = 76 : [] @ 0, I64AtomicRmw16CmpxchgU = 77 : [] @ 1,
                I64AtomicRmw32CmpxchgU = 78 : [] @ 2,
            }
        }
    };
}

/// Os enums dos opcodes com prefixo.
macro_rules! define_ext_op_enums {
    (
        table { $($table:tt)* }
        trunc_sat { $($trunc_sat:tt)* }
        wide { $($wide:tt)* }
        gc { $($gc:tt)* }
        atomic_load { $($atomic_load:tt)* }
        atomic_store { $($atomic_store:tt)* }
        atomic_rmw_binary { $($atomic_rmw_binary:tt)* }
        atomic_other { $($atomic_other:tt)* }
    ) => {
        wasm_enum!(
            /// `enum class Ext1OpType : uint32_t` (prefixo `0xFC`): tabela e memória em massa,
            /// conversões saturadas e aritmética larga.
            Ext1OpType : u32 { $($table)* $($trunc_sat)* $($wide)* }
        );
        wasm_types_impl!(Ext1OpType { $($table)* $($trunc_sat)* $($wide)* });

        wasm_enum!(
            /// `enum class ExtGCOpType : uint32_t` (prefixo `0xFB`).
            ExtGCOpType : u32 { $($gc)* }
        );

        wasm_enum!(
            /// `enum class ExtAtomicOpType : uint32_t` (prefixo `0xFE`): load, store, rmw binário e
            /// os demais (notify, wait, fence, cmpxchg).
            ExtAtomicOpType : u32 {
                $($atomic_load)* $($atomic_store)* $($atomic_rmw_binary)* $($atomic_other)*
            }
        );
        wasm_types_impl!(ExtAtomicOpType {
            $($atomic_load)* $($atomic_store)* $($atomic_rmw_binary)* $($atomic_other)*
        });
        wasm_alignment_impl!(ExtAtomicOpType {
            $($atomic_load)* $($atomic_store)* $($atomic_rmw_binary)* $($atomic_other)*
        });
    };
}

wasm_ext_op_lists!(define_ext_op_enums);

/// `isValidOpType`: o conjunto de bytes que são opcodes (os `value` de `wasm.json` mais o prefixo
/// `0xFD` do SIMD, que o gerador acrescenta à mão).
pub fn is_valid_op_type(value: i64) -> bool {
    u8::try_from(value).is_ok_and(|byte| OpType::from_value(byte).is_some())
}

/// `isControlFlowInstruction` com o `ExtGC`: `br_on_cast` e `br_on_cast_fail` também desviam.
/// `extended_opcode` é o `getExtendedOpcode` do C++, só chamado para o prefixo `ExtGC`.
pub fn is_control_flow_instruction_with_ext_gc(op: OpType, extended_opcode: impl FnOnce() -> u32) -> bool {
    if op.is_control_op() {
        return true;
    }
    if op == OpType::ExtGC {
        let extended = extended_opcode();
        return extended == ExtGCOpType::BrOnCast.value() || extended == ExtGCOpType::BrOnCastFail.value();
    }
    false
}

/// `memoryLog2Alignment(OpType)`: só para loads e stores (o `RELEASE_ASSERT_NOT_REACHED` do C++
/// cobre o resto).
pub fn memory_log2_alignment(op: OpType) -> u32 {
    if let Some(load) = LoadOpType::from_value(op.value()) {
        return load.log2_alignment();
    }
    if let Some(store) = StoreOpType::from_value(op.value()) {
        return store.log2_alignment();
    }
    // Invariante: só chamado para opcodes de memória já decodificados (ASSERT_NOT_REACHED do C++).
    unreachable!("memoryLog2Alignment de um opcode que não acessa a memória: {}", op.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_values_come_from_wasm_json() {
        assert_eq!(OpType::Unreachable.value(), 0);
        assert_eq!(OpType::End.value(), 0x0b);
        assert_eq!(OpType::GetGlobal.value(), 0x23);
        assert_eq!(OpType::I32Const.value(), 0x41);
        assert_eq!(OpType::RefNull.value(), 0xd0);
        assert_eq!(OpType::BrOnNonNull.value(), 0xd6);
        assert_eq!(OpType::ExtGC.value(), 0xfb);
        assert_eq!(OpType::ExtAtomic.value(), 0xfe);
        assert_eq!(OpType::from_value(0x6a), Some(OpType::I32Add));
        assert_eq!(OpType::from_value(0x16), None);
        assert_eq!(Ext1OpType::from_value(10), Some(Ext1OpType::MemoryCopy));
        assert_eq!(Ext1OpType::TableFill.value(), 17);
        assert_eq!(Ext1OpType::I64MulWideU.value(), 22);
        assert_eq!(ExtGCOpType::BrOnCastFail.value(), 25);
        assert_eq!(ExtAtomicOpType::I64AtomicRmw32XchgU.value(), 71);
    }

    #[test]
    fn names_are_the_to_cpp_names() {
        assert_eq!(OpType::I32TruncSF32.name(), "I32TruncSF32");
        assert_eq!(UnaryOpType::F64PromoteF32.name(), "F64PromoteF32");
        assert_eq!(Ext1OpType::I32TruncSatF32S.name(), "I32TruncSatF32S");
        assert_eq!(ExtAtomicOpType::I64AtomicRmw16CmpxchgU.name(), "I64AtomicRmw16CmpxchgU");
    }

    #[test]
    fn valid_op_type_is_the_set_of_known_bytes() {
        assert!(is_valid_op_type(0));
        assert!(is_valid_op_type(0xfd));
        assert!(is_valid_op_type(0xfe));
        assert!(!is_valid_op_type(0x16));
        assert!(!is_valid_op_type(0x1d));
        assert!(!is_valid_op_type(0xff));
        assert!(!is_valid_op_type(-1));
        assert!(!is_valid_op_type(256));
        // O primeiro buraco é 0x16, logo depois de `tail_call_ref` (0x15).
        assert!(is_valid_op_type(0x15));
    }

    #[test]
    fn every_typed_enum_is_a_subset_of_op_type() {
        for op in UnaryOpType::ALL {
            assert_eq!(OpType::from_value(op.value()).map(OpType::name), Some(op.name()));
        }
        for op in BinaryOpType::ALL {
            assert_eq!(OpType::from_value(op.value()).map(OpType::name), Some(op.name()));
        }
        for op in LoadOpType::ALL {
            assert_eq!(OpType::from_value(op.value()).map(OpType::name), Some(op.name()));
        }
        for op in StoreOpType::ALL {
            assert_eq!(OpType::from_value(op.value()).map(OpType::name), Some(op.name()));
        }
        // 24 de especial, 24 de controle, 4 prefixos mais as quatro listas tipadas.
        let typed = UnaryOpType::ALL.len() + BinaryOpType::ALL.len() + LoadOpType::ALL.len() + StoreOpType::ALL.len();
        assert_eq!(OpType::ALL.len(), 24 + 24 + 4 + typed);
    }

    #[test]
    fn typed_tables_follow_parameters_then_return() {
        assert_eq!(UnaryOpType::I32WrapI64.types(), &[TypeKind::I64, TypeKind::I32]);
        assert_eq!(UnaryOpType::I64Eqz.types(), &[TypeKind::I64, TypeKind::I32]);
        assert_eq!(BinaryOpType::F64Lt.types(), &[TypeKind::F64, TypeKind::F64, TypeKind::I32]);
        assert_eq!(BinaryOpType::I64Add.types(), &[TypeKind::I64, TypeKind::I64, TypeKind::I64]);
        assert_eq!(LoadOpType::I64Load32U.types(), &[TypeKind::I64]);
        assert_eq!(StoreOpType::F32Store.types(), &[TypeKind::F32]);
        assert_eq!(Ext1OpType::I64TruncSatF64U.types(), &[TypeKind::F64, TypeKind::I64]);
        assert_eq!(Ext1OpType::I64Add128.types().len(), 6);
        assert!(Ext1OpType::MemoryFill.types().is_empty());
    }

    #[test]
    fn memory_alignment_follows_the_access_width() {
        assert_eq!(memory_log2_alignment(OpType::I32Load8S), 0);
        assert_eq!(memory_log2_alignment(OpType::I64Load16U), 1);
        assert_eq!(memory_log2_alignment(OpType::I64Load32S), 2);
        assert_eq!(memory_log2_alignment(OpType::F64Load), 3);
        assert_eq!(memory_log2_alignment(OpType::I32Store16), 1);
        assert_eq!(memory_log2_alignment(OpType::I64Store), 3);
        assert_eq!(ExtAtomicOpType::AtomicFence.log2_alignment(), 0);
        assert_eq!(ExtAtomicOpType::MemoryAtomicNotify.log2_alignment(), 2);
        assert_eq!(ExtAtomicOpType::MemoryAtomicWait64.log2_alignment(), 3);
        assert_eq!(ExtAtomicOpType::I32AtomicRmw16CmpxchgU.log2_alignment(), 1);
        assert_eq!(ExtAtomicOpType::I64AtomicRmwXchg.log2_alignment(), 3);
    }

    #[test]
    fn control_flow_includes_br_on_cast() {
        assert!(OpType::BrTable.is_control_op());
        assert!(OpType::BrOnNull.is_control_op());
        assert!(!OpType::I32Add.is_control_op());
        assert!(!OpType::Call.is_control_op());
        assert!(is_control_flow_instruction_with_ext_gc(OpType::Br, || unreachable!()));
        assert!(is_control_flow_instruction_with_ext_gc(OpType::ExtGC, || 24));
        assert!(is_control_flow_instruction_with_ext_gc(OpType::ExtGC, || 25));
        assert!(!is_control_flow_instruction_with_ext_gc(OpType::ExtGC, || 22));
        assert!(!is_control_flow_instruction_with_ext_gc(OpType::I32Add, || unreachable!()));
    }
}
