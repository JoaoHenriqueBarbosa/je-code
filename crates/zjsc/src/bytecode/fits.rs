//! Porte de `bytecode/Fits.h`.
//!
//! `template<typename, OpcodeSize> struct Fits` vira dois traits indexados pelo tipo do operando,
//! com o `OpcodeSize` como argumento de execução (o tamanho do `emit` é conhecido só ali):
//!
//! - `Fits`: `check(T)` e `convert(T)` (valor para o padrão de bits na largura pedida, `Fitted`).
//!   É seguro para objeto: `OpcodeTraits`/`write_opcode` recebem `&[&dyn Fits]`.
//! - `FitsFrom`: `convert(TargetType)`, a direção inversa usada pelos construtores
//!   `Op*(const uint8_t*)` e pelo `decode`.
//!
//! Mapa das especializações do C++:
//!
//! - mesmo tamanho (`sizeof(T) == size`): identidade em bits; é o caso `u8`/`u16`/`u32`/`i8`/`i16`/
//!   `i32` na própria largura, que a verificação de faixa dos inteiros já cobre (`check` é sempre
//!   verdadeiro ali);
//! - inteiros de tamanho diferente: faixa do `TargetType` com a mesma assinatura de `T`;
//! - `bool`: `Fits<uint8_t>`; `enum`: `Fits<underlying>` (`fits_enum!`);
//! - `VirtualRegister`, `GetPutInfo`, `OperandTypes`, `ECMAMode`, `ResultType`: à parte;
//! - `GenericBoundLabel`: `fits_check`/`fits_convert` (inerentes, precisam de `&mut`).
//!
//! `SymbolTableOrScopeDepth` (`Fits<unsigned>` sobre `raw()`), `PutByIdFlags` (`isDirect | isStrict
//! << 1`) e `PrivateFieldPutKind` (`Fits<uint8_t>` sobre `value()`) têm o impl aqui, ao lado dos
//! irmãos. O `Fits<OpcodeID>` mora em `opcode_traits.rs`.

use crate::bytecode::opcode_size::OpcodeSize;
use crate::bytecode::put_by_id_flags::PutByIdFlags;
use crate::bytecode::put_kind::PrivateFieldPutKind;
use crate::runtime::symbol_table_or_scope_depth::SymbolTableOrScopeDepth;
use crate::bytecode::virtual_register::{
    VirtualRegister, FIRST_CONSTANT_REGISTER_INDEX, FIRST_CONSTANT_REGISTER_INDEX16,
    FIRST_CONSTANT_REGISTER_INDEX8,
};
use crate::bytecompiler::label::{GenericBoundLabel, LabelGenerator};
use crate::bytecompiler::profile_type_bytecode_flag::ProfileTypeBytecodeFlag;
use crate::interpreter::interpreter::DebugHookType;
use crate::parser::result_type::{OperandTypes, ResultType};
use crate::runtime::ecma_mode::ECMAMode;
use crate::runtime::error_type::ErrorTypeWithExtension;
use crate::runtime::get_put_info::{GetPutInfo, InitializationMode, ResolveMode, ResolveType};

/// `enum FitsAssertion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitsAssertion {
    Assert,
    NoAssert,
}

/// Resultado de `Fits<T, size>::convert`: o valor já na largura do operando (padrão de bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fitted {
    Narrow(u8),
    Wide16(u16),
    Wide32(u32),
}

impl Fitted {
    /// O operando lido na largura `size` (bits de baixa ordem de `raw`), a entrada de `FitsFrom`.
    pub const fn from_raw(raw: u32, size: OpcodeSize) -> Fitted {
        pack(raw, size)
    }
}

/// `Fits<T, size>::check` e `convert(T)`.
pub trait Fits {
    /// `static bool check(T)`.
    fn check(&self, size: OpcodeSize) -> bool;
    /// `static TargetType convert(T)`.
    fn convert(&self, size: OpcodeSize) -> Fitted;
}

/// `Fits<T, size>::convert(TargetType)`: o inverso de `Fits::convert`.
pub trait FitsFrom: Fits + Sized {
    fn from_fitted(fitted: Fitted) -> Self;
}

/// Um valor já convertido é o próprio operando (serve para passar a `write_opcode` o resultado de
/// `fits_convert` de um rótulo, que precisa de `&mut`).
impl Fits for Fitted {
    fn check(&self, _size: OpcodeSize) -> bool {
        true
    }

    fn convert(&self, _size: OpcodeSize) -> Fitted {
        *self
    }
}

/// `std::numeric_limits<unsignedType>::max()` de `TypeBySize<size>`.
const fn max_unsigned(size: OpcodeSize) -> u64 {
    match size {
        OpcodeSize::Narrow => u8::MAX as u64,
        OpcodeSize::Wide16 => u16::MAX as u64,
        OpcodeSize::Wide32 => u32::MAX as u64,
    }
}

/// `numeric_limits<signedType>::min()` e `max()` de `TypeBySize<size>`.
const fn signed_range(size: OpcodeSize) -> (i64, i64) {
    match size {
        OpcodeSize::Narrow => (i8::MIN as i64, i8::MAX as i64),
        OpcodeSize::Wide16 => (i16::MIN as i64, i16::MAX as i64),
        OpcodeSize::Wide32 => (i32::MIN as i64, i32::MAX as i64),
    }
}

/// Os bits de baixa ordem de `bits` na largura `size`.
const fn pack(bits: u32, size: OpcodeSize) -> Fitted {
    match size {
        OpcodeSize::Narrow => Fitted::Narrow(bits as u8),
        OpcodeSize::Wide16 => Fitted::Wide16(bits as u16),
        OpcodeSize::Wide32 => Fitted::Wide32(bits),
    }
}

/// `static_cast<unsignedType>` de volta para 32 bits.
const fn unpack_unsigned(fitted: Fitted) -> u32 {
    match fitted {
        Fitted::Narrow(value) => value as u32,
        Fitted::Wide16(value) => value as u32,
        Fitted::Wide32(value) => value,
    }
}

/// `static_cast<signedType>` de volta para 32 bits, com extensão de sinal.
const fn unpack_signed(fitted: Fitted) -> i32 {
    match fitted {
        Fitted::Narrow(value) => value as i8 as i32,
        Fitted::Wide16(value) => value as i16 as i32,
        Fitted::Wide32(value) => value as i32,
    }
}

/// `Fits<T>` de inteiro sem sinal: o `TargetType` é o `unsignedType` da largura.
macro_rules! fits_unsigned {
    ($($t:ty),* $(,)?) => {$(
        impl Fits for $t {
            fn check(&self, size: OpcodeSize) -> bool {
                (*self as u64) <= max_unsigned(size)
            }

            fn convert(&self, size: OpcodeSize) -> Fitted {
                debug_assert!(self.check(size));
                pack(*self as u32, size)
            }
        }

        impl FitsFrom for $t {
            fn from_fitted(fitted: Fitted) -> Self {
                unpack_unsigned(fitted) as $t
            }
        }
    )*};
}

/// `Fits<T>` de inteiro com sinal: o `TargetType` é o `signedType` da largura.
macro_rules! fits_signed {
    ($($t:ty),* $(,)?) => {$(
        impl Fits for $t {
            fn check(&self, size: OpcodeSize) -> bool {
                let (min, max) = signed_range(size);
                (*self as i64) >= min && (*self as i64) <= max
            }

            fn convert(&self, size: OpcodeSize) -> Fitted {
                debug_assert!(self.check(size));
                pack(*self as i32 as u32, size)
            }
        }

        impl FitsFrom for $t {
            fn from_fitted(fitted: Fitted) -> Self {
                unpack_signed(fitted) as $t
            }
        }
    )*};
}

fits_unsigned!(u8, u16, u32);
fits_signed!(i8, i16, i32);

/// `Fits<bool, size> : Fits<uint8_t, size>`.
impl Fits for bool {
    fn check(&self, size: OpcodeSize) -> bool {
        (*self as u8).check(size)
    }

    fn convert(&self, size: OpcodeSize) -> Fitted {
        (*self as u8).convert(size)
    }
}

impl FitsFrom for bool {
    fn from_fitted(fitted: Fitted) -> Self {
        u8::from_fitted(fitted) != 0
    }
}

/// `Fits<E, size> : Fits<underlying_type_t<E>, size>` para um enum sem dados. `$raw` é o tipo
/// subjacente do C++ (`unsigned` para enum sem tipo fixado, o que o enum fixa nos demais).
macro_rules! fits_enum {
    ($t:ty, $raw:ty, [$($variant:ident),* $(,)?]) => {
        impl Fits for $t {
            fn check(&self, size: OpcodeSize) -> bool {
                (*self as $raw).check(size)
            }

            fn convert(&self, size: OpcodeSize) -> Fitted {
                (*self as $raw).convert(size)
            }
        }

        impl FitsFrom for $t {
            fn from_fitted(fitted: Fitted) -> Self {
                let raw = <$raw as FitsFrom>::from_fitted(fitted);
                $(
                    if raw == <$t>::$variant as $raw {
                        return <$t>::$variant;
                    }
                )*
                panic!(concat!("valor inválido para ", stringify!($t), ": {}"), raw)
            }
        }
    };
}

fits_enum!(ProfileTypeBytecodeFlag, u32, [
    ProfileTypeBytecodeClosureVar,
    ProfileTypeBytecodeLocallyResolved,
    ProfileTypeBytecodeDoesNotHaveGlobalID,
    ProfileTypeBytecodeFunctionArgument,
    ProfileTypeBytecodeFunctionReturnStatement,
]);

// `enum DebugHookType` não fixa o tipo no C++ (subjacente `unsigned`), mesmo que o porte o guarde em `u8`.
fits_enum!(DebugHookType, u32, [
    WillExecuteProgram,
    DidExecuteProgram,
    DidEnterCallFrame,
    DidReachDebuggerStatement,
    WillLeaveCallFrame,
    WillExecuteStatement,
    WillExecuteExpression,
    WillAwait,
    DidAwait,
]);

fits_enum!(ResolveMode, u32, [ThrowIfNotFound, DoNotThrowIfNotFound]);

fits_enum!(InitializationMode, u32, [
    Initialization,
    ConstInitialization,
    NotInitialization,
    ScopedArgumentInitialization,
]);

fits_enum!(ResolveType, u32, [
    GlobalProperty,
    GlobalVar,
    GlobalLexicalVar,
    ClosureVar,
    ResolvedClosureVar,
    ModuleVar,
    GlobalPropertyWithVarInjectionChecks,
    GlobalVarWithVarInjectionChecks,
    GlobalLexicalVarWithVarInjectionChecks,
    ClosureVarWithVarInjectionChecks,
    UnresolvedProperty,
    UnresolvedPropertyWithVarInjectionChecks,
    Dynamic,
]);

fits_enum!(ErrorTypeWithExtension, u8, [
    Error,
    EvalError,
    RangeError,
    ReferenceError,
    SyntaxError,
    TypeError,
    URIError,
    AggregateError,
    SuppressedError,
    OutOfMemoryError,
]);

/// `Fits<ECMAMode, size> : Fits<uint8_t, size>`.
impl Fits for ECMAMode {
    fn check(&self, size: OpcodeSize) -> bool {
        self.value().check(size)
    }

    fn convert(&self, size: OpcodeSize) -> Fitted {
        self.value().convert(size)
    }
}

impl FitsFrom for ECMAMode {
    fn from_fitted(fitted: Fitted) -> Self {
        ECMAMode::from_byte(u8::from_fitted(fitted))
    }
}

/// `Fits<ResultType, size> : Fits<uint8_t, size>`.
impl Fits for ResultType {
    fn check(&self, size: OpcodeSize) -> bool {
        self.bits().check(size)
    }

    fn convert(&self, size: OpcodeSize) -> Fitted {
        self.bits().convert(size)
    }
}

impl FitsFrom for ResultType {
    fn from_fitted(fitted: Fitted) -> Self {
        ResultType::new(u8::from_fitted(fitted))
    }
}

/// `Fits<SymbolTableOrScopeDepth, size> : Fits<unsigned, size>`.
impl Fits for SymbolTableOrScopeDepth {
    fn check(&self, size: OpcodeSize) -> bool {
        self.raw_value().check(size)
    }

    fn convert(&self, size: OpcodeSize) -> Fitted {
        self.raw_value().convert(size)
    }
}

impl FitsFrom for SymbolTableOrScopeDepth {
    fn from_fitted(fitted: Fitted) -> Self {
        SymbolTableOrScopeDepth::raw(u32::from_fitted(fitted))
    }
}

/// `Fits<PutByIdFlags, size>`: `isDirect` no bit 0 (`s_isDirectBit`) e `isStrict` no bit 1
/// (`s_isStrictBit`); sempre cabe.
impl Fits for PutByIdFlags {
    fn check(&self, _size: OpcodeSize) -> bool {
        true
    }

    fn convert(&self, size: OpcodeSize) -> Fitted {
        let is_direct = self.is_direct() as u32;
        let is_strict = self.ecma_mode().is_strict() as u32;
        pack((is_strict << 1) | is_direct, size)
    }
}

impl FitsFrom for PutByIdFlags {
    fn from_fitted(fitted: Fitted) -> Self {
        const IS_DIRECT_BIT: u32 = 1;
        const IS_STRICT_BIT: u32 = 2;
        let bits = unpack_unsigned(fitted);
        let is_direct = bits & IS_DIRECT_BIT != 0;
        let is_strict = bits & IS_STRICT_BIT != 0;
        let ecma_mode = if is_strict { ECMAMode::strict() } else { ECMAMode::sloppy() };
        if is_direct {
            PutByIdFlags::create_direct(ecma_mode)
        } else {
            PutByIdFlags::create(ecma_mode)
        }
    }
}

/// `Fits<PrivateFieldPutKind, size> : Fits<uint8_t, size>`.
impl Fits for PrivateFieldPutKind {
    fn check(&self, size: OpcodeSize) -> bool {
        self.value().check(size)
    }

    fn convert(&self, size: OpcodeSize) -> Fitted {
        self.value().convert(size)
    }
}

impl FitsFrom for PrivateFieldPutKind {
    fn from_fitted(fitted: Fitted) -> Self {
        PrivateFieldPutKind::from_byte(u8::from_fitted(fitted))
    }
}

/// `FirstConstant<size>::index`.
const fn first_constant_index(size: OpcodeSize) -> i32 {
    match size {
        OpcodeSize::Narrow => FIRST_CONSTANT_REGISTER_INDEX8,
        OpcodeSize::Wide16 => FIRST_CONSTANT_REGISTER_INDEX16,
        // `Fits<VirtualRegister, Wide32>` é a identidade em bits e não usa este índice.
        OpcodeSize::Wide32 => FIRST_CONSTANT_REGISTER_INDEX,
    }
}

/// `Fits<VirtualRegister, size>`.
///
/// Narrow: -128..-1 locais, 0..15 argumentos, 16..127 constantes.
/// Wide16: -2**15..-1 locais, 0..64 argumentos, 64..2**15-1 constantes.
/// Wide32: o `offset()` cru (`sizeof(VirtualRegister) == size`).
impl Fits for VirtualRegister {
    fn check(&self, size: OpcodeSize) -> bool {
        if size == OpcodeSize::Wide32 {
            return true;
        }
        let (min, max) = signed_range(size);
        let first_constant_index = first_constant_index(size) as i64;
        if self.is_constant() {
            return first_constant_index + self.to_constant_index() as i64 <= max;
        }
        self.offset() as i64 >= min && (self.offset() as i64) < first_constant_index
    }

    fn convert(&self, size: OpcodeSize) -> Fitted {
        debug_assert!(self.check(size));
        if size == OpcodeSize::Wide32 {
            return pack(self.offset() as u32, size);
        }
        if self.is_constant() {
            return pack((first_constant_index(size) + self.to_constant_index()) as u32, size);
        }
        pack(self.offset() as u32, size)
    }
}

impl FitsFrom for VirtualRegister {
    fn from_fitted(fitted: Fitted) -> Self {
        let size = match fitted {
            Fitted::Narrow(_) => OpcodeSize::Narrow,
            Fitted::Wide16(_) => OpcodeSize::Wide16,
            Fitted::Wide32(_) => return VirtualRegister::new(unpack_signed(fitted)),
        };
        let i = unpack_signed(fitted);
        let first_constant_index = first_constant_index(size);
        if i >= first_constant_index {
            return VirtualRegister::new((i - first_constant_index) + FIRST_CONSTANT_REGISTER_INDEX);
        }
        VirtualRegister::new(i)
    }
}

/// `Fits<GetPutInfo, size>` (`size != Wide32`); no Wide32 vale a identidade em bits do `operand`.
///
/// 13 Resolve Types, 3 Initialization Modes, 2 Resolve Modes e 1 bit `isStrict`, codificados como
///
/// ```text
///            initialization mode
///                     v
/// isStrict -> 0|0000|00|0
///                  ^    ^
///      resolve  type    resolve mode
/// ```
mod get_put_info_bits {
    pub const RESOLVE_TYPE_MAX: u32 = 1 << 4;
    pub const INITIALIZATION_MODE_MAX: u32 = 1 << 2;
    pub const RESOLVE_MODE_MAX: u32 = 1 << 1;

    pub const IS_STRICT_BIT: u32 = 1 << 7;
    pub const RESOLVE_TYPE_BITS: u32 = (RESOLVE_TYPE_MAX - 1) << 3;
    pub const INITIALIZATION_MODE_BITS: u32 = (INITIALIZATION_MODE_MAX - 1) << 1;
    pub const RESOLVE_MODE_BITS: u32 = RESOLVE_MODE_MAX - 1;

    // There should be no intersection between ResolveMode, ResolveType and InitializationMode.
    const _: () = assert!(RESOLVE_TYPE_BITS & INITIALIZATION_MODE_BITS & RESOLVE_MODE_BITS == 0);
}

impl Fits for GetPutInfo {
    fn check(&self, size: OpcodeSize) -> bool {
        use get_put_info_bits::*;
        if size == OpcodeSize::Wide32 {
            return true;
        }
        (self.resolve_type() as u32) < RESOLVE_TYPE_MAX
            && (self.initialization_mode() as u32) < INITIALIZATION_MODE_MAX
            && (self.resolve_mode() as u32) < RESOLVE_MODE_MAX
    }

    fn convert(&self, size: OpcodeSize) -> Fitted {
        debug_assert!(self.check(size));
        if size == OpcodeSize::Wide32 {
            return pack(self.operand(), size);
        }
        let resolve_type = self.resolve_type() as u32;
        let initialization_mode = self.initialization_mode() as u32;
        let resolve_mode = self.resolve_mode() as u32;
        let is_strict = self.ecma_mode().is_strict() as u32;
        pack((is_strict << 7) | (resolve_type << 3) | (initialization_mode << 1) | resolve_mode, size)
    }
}

impl FitsFrom for GetPutInfo {
    fn from_fitted(fitted: Fitted) -> Self {
        use get_put_info_bits::*;
        if let Fitted::Wide32(operand) = fitted {
            return GetPutInfo::from_operand(operand);
        }
        let gpi = unpack_unsigned(fitted);
        let resolve_type = ResolveType::from_u32((gpi & RESOLVE_TYPE_BITS) >> 3);
        let initialization_mode = InitializationMode::from_u32((gpi & INITIALIZATION_MODE_BITS) >> 1);
        let resolve_mode = ResolveMode::from_u32(gpi & RESOLVE_MODE_BITS);
        let is_strict = gpi & IS_STRICT_BIT != 0;
        GetPutInfo::new(
            resolve_mode,
            resolve_type,
            initialization_mode,
            if is_strict { ECMAMode::strict() } else { ECMAMode::sloppy() },
        )
    }
}

/// `Fits<OperandTypes, size>`: um par de `ResultType`, cada um em 4 bits no Narrow (o tipo
/// desconhecido é codificado como 0 em vez do `|` de todos os tipos).
mod operand_types_bits {
    pub const TYPE_WIDTH: u32 = 4;
    pub const MAX_TYPE: u32 = (1 << TYPE_WIDTH) - 1;
}

/// `first`/`second` no Narrow: o tipo desconhecido vira 0.
fn narrow_operand_type(bits: u8) -> u32 {
    if bits == ResultType::unknown_type().bits() {
        0
    } else {
        bits as u32
    }
}

impl Fits for OperandTypes {
    fn check(&self, size: OpcodeSize) -> bool {
        use operand_types_bits::*;
        if size == OpcodeSize::Narrow {
            let first = narrow_operand_type(self.first().bits());
            let second = narrow_operand_type(self.second().bits());
            return first <= MAX_TYPE && second <= MAX_TYPE;
        }
        true
    }

    fn convert(&self, size: OpcodeSize) -> Fitted {
        use operand_types_bits::*;
        if size == OpcodeSize::Narrow {
            debug_assert!(self.check(size));
            let first = narrow_operand_type(self.first().bits());
            let second = narrow_operand_type(self.second().bits());
            return pack((first << TYPE_WIDTH) | second, size);
        }
        pack(self.bits() as u32, size)
    }
}

impl FitsFrom for OperandTypes {
    fn from_fitted(fitted: Fitted) -> Self {
        use operand_types_bits::*;
        if let Fitted::Narrow(types) = fitted {
            let mut first = (types as u32) >> TYPE_WIDTH;
            let mut second = types as u32 & MAX_TYPE;
            if first == 0 {
                first = ResultType::unknown_type().bits() as u32;
            }
            if second == 0 {
                second = ResultType::unknown_type().bits() as u32;
            }
            return OperandTypes::new(ResultType::new(first as u8), ResultType::new(second as u8));
        }
        OperandTypes::from_bits(unpack_unsigned(fitted) as u16)
    }
}

/// `Fits<GenericBoundLabel<GeneratorTraits>, size> : Fits<int, size>`.
///
/// É um pouco hacky: precisamos adiar o cálculo dos alvos dos saltos, pois pode ser preciso emitir
/// `nop`s para alinhar o fluxo de instruções. Além disso, o alvo é calculado antes de começar a
/// escrever no fluxo, porque o deslocamento parte do início do bytecode. O alvo é calculado e
/// guardado no `check` e o `convert` usa o valor guardado.
///
/// Os dois precisam de `&mut` (e o `check` do gerador, que no C++ o rótulo guarda por ponteiro),
/// por isso não são `Fits`: o chamador faz `fits_check` na hora do `checkImpl` e passa o resultado
/// de `fits_convert` (um `Fitted`, que é `Fits`) a `write_opcode`.
impl<Traits> GenericBoundLabel<Traits> {
    /// `Fits::check(GenericBoundLabel&)`: `Base::check(label.saveTarget())`.
    pub fn fits_check(&mut self, size: OpcodeSize, generator: &dyn LabelGenerator) -> bool {
        self.save_target(generator).check(size)
    }

    /// `Fits::convert(GenericBoundLabel&)`: `Base::convert(label.commitTarget())`.
    pub fn fits_convert(&mut self, size: OpcodeSize) -> Fitted {
        self.commit_target().convert(size)
    }

    /// `Fits::convert(TargetType)`: `GenericBoundLabel(Base::convert(target))`.
    pub fn fits_from(fitted: Fitted) -> Self {
        GenericBoundLabel::from_offset(i32::from_fitted(fitted))
    }
}
