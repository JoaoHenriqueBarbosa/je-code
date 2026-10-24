//! Lado de leitura das structs `Op*` que o gerador Ruby (`generator/Opcode.rb`) produz em
//! `BytecodeStructs.h`: os construtores `Op(const uintN_t* stream)` (um por largura), `decode`, e o
//! que `BaseInstruction::as<T>`, `asKnownWidth<T>` e `cast<T>` precisam delas.
//!
//! `Fits<T, size>::convert(TargetType)` (de `bytecode/Fits.h`) vira o trait `OperandDecode`: cada
//! tipo de operando sabe reconstruir o valor a partir do inteiro lido na largura do operando. A
//! lista de campos é a mesma de `bytecode_op!` (o macro `impl_op_decode!` é chamado de dentro dele),
//! por isso nada se repete por struct.
//!
//! Diferenças deliberadas em relação ao C++:
//! - `m_metadataID` (campo extra das structs com `metadata:`) não é lido: as structs daqui não o
//!   têm (ver o cabeçalho de `bytecode_ops.rs`).
//! - `cast<T>()` devolve `T*` aliasando os bytes do fluxo; em Rust seguro isso vira `OpMut<T>`
//!   (referência mutável ao fluxo) com os setters que o `BytecodeGenerator` usa. Hoje só
//!   `set_target_label` (`TargetLabelOp`); os demais `set<Campo>` entram quando houver chamador,
//!   pois sem `paste` o nome do método não pode ser derivado do campo.
//! - Os `ASSERT` dos construtores sobre `stream[-1] == opcodeID` não existem: a fatia que
//!   `JSInstruction` entrega começa no primeiro byte da instrução, e `as_op` já confere `is::<T>()`.

use std::marker::PhantomData;

use crate::bytecode::bytecode_ops::{BoundLabel, BytecodeOp};
use crate::bytecode::instruction_stream::{JSInstruction, MutableRef};
use crate::bytecode::fits::{Fits, Fitted, FitsFrom};
use crate::bytecode::opcode_size::{opcode_id_width_by_size, OpcodeSize, MAX_JS_OPCODE_ID_WIDTH};
use crate::bytecode::put_by_id_flags::PutByIdFlags;
use crate::bytecode::put_kind::PrivateFieldPutKind;
use crate::runtime::symbol_table_or_scope_depth::SymbolTableOrScopeDepth;
use crate::bytecode::virtual_register::{
    VirtualRegister, FIRST_CONSTANT_REGISTER_INDEX, FIRST_CONSTANT_REGISTER_INDEX16, FIRST_CONSTANT_REGISTER_INDEX8,
};
use crate::bytecompiler::label::{GenericBoundLabel, LabelGenerator};
use crate::bytecompiler::profile_type_bytecode_flag::ProfileTypeBytecodeFlag;
use crate::interpreter::interpreter::DebugHookType;
use crate::parser::result_type::{OperandTypes, ResultType};
use crate::runtime::ecma_mode::ECMAMode;
use crate::runtime::error_type::ErrorTypeWithExtension;
use crate::runtime::get_put_info::{GetPutInfo, InitializationMode, ResolveMode, ResolveType};

/// `Fits<T, size>::convert(TargetType)`: `raw` é o operando lido na largura `size`, estendido com
/// zeros para 32 bits (o padrão de bits do `TargetType` sem sinal).
pub trait OperandDecode: Sized {
    fn decode_operand(raw: u32, size: OpcodeSize) -> Self;
}

/// O `static_cast<signed TargetType>` do `Fits`: reinterpreta `raw` com o sinal da largura.
fn sign_extend(raw: u32, size: OpcodeSize) -> i32 {
    match size {
        OpcodeSize::Narrow => raw as u8 as i8 as i32,
        OpcodeSize::Wide16 => raw as u16 as i16 as i32,
        OpcodeSize::Wide32 => raw as i32,
    }
}

impl OperandDecode for u32 {
    fn decode_operand(raw: u32, _size: OpcodeSize) -> u32 {
        raw
    }
}

impl OperandDecode for i32 {
    fn decode_operand(raw: u32, size: OpcodeSize) -> i32 {
        sign_extend(raw, size)
    }
}

impl OperandDecode for u8 {
    fn decode_operand(raw: u32, _size: OpcodeSize) -> u8 {
        raw as u8
    }
}

impl OperandDecode for bool {
    fn decode_operand(raw: u32, _size: OpcodeSize) -> bool {
        raw as u8 != 0
    }
}

impl OperandDecode for VirtualRegister {
    fn decode_operand(raw: u32, size: OpcodeSize) -> VirtualRegister {
        let first_constant_index = match size {
            // `Fits<VirtualRegister, Wide32>` não existe: vale o bit_cast do tipo de mesmo tamanho.
            OpcodeSize::Wide32 => return VirtualRegister::new(raw as i32),
            OpcodeSize::Narrow => FIRST_CONSTANT_REGISTER_INDEX8,
            OpcodeSize::Wide16 => FIRST_CONSTANT_REGISTER_INDEX16,
        };
        let index = sign_extend(raw, size);
        if index >= first_constant_index {
            return VirtualRegister::new((index - first_constant_index) + FIRST_CONSTANT_REGISTER_INDEX);
        }
        VirtualRegister::new(index)
    }
}

impl OperandDecode for ECMAMode {
    fn decode_operand(raw: u32, _size: OpcodeSize) -> ECMAMode {
        ECMAMode::from_byte(raw as u8)
    }
}

impl OperandDecode for ResolveType {
    fn decode_operand(raw: u32, _size: OpcodeSize) -> ResolveType {
        ResolveType::from_u32(raw)
    }
}

impl OperandDecode for GetPutInfo {
    fn decode_operand(raw: u32, size: OpcodeSize) -> GetPutInfo {
        // `Fits<GetPutInfo, Wide32>` não existe: o bit_cast do `unsigned` é o próprio operando.
        if size == OpcodeSize::Wide32 {
            return GetPutInfo::from_operand(raw);
        }
        const IS_STRICT_BIT: u32 = 1 << 7;
        const RESOLVE_TYPE_BITS: u32 = ((1 << 4) - 1) << 3;
        const INITIALIZATION_MODE_BITS: u32 = ((1 << 2) - 1) << 1;
        const RESOLVE_MODE_BITS: u32 = (1 << 1) - 1;
        GetPutInfo::new(
            ResolveMode::from_u32(raw & RESOLVE_MODE_BITS),
            ResolveType::from_u32((raw & RESOLVE_TYPE_BITS) >> 3),
            InitializationMode::from_u32((raw & INITIALIZATION_MODE_BITS) >> 1),
            ECMAMode::from_bool(raw & IS_STRICT_BIT != 0),
        )
    }
}

impl OperandDecode for ResultType {
    fn decode_operand(raw: u32, _size: OpcodeSize) -> ResultType {
        ResultType::new(raw as u8)
    }
}

impl OperandDecode for OperandTypes {
    fn decode_operand(raw: u32, size: OpcodeSize) -> OperandTypes {
        if size != OpcodeSize::Narrow {
            return OperandTypes::from_bits(raw as u16);
        }
        // Dois tipos de 4 bits; 0 codifica o tipo desconhecido.
        const TYPE_WIDTH: u32 = 4;
        const MAX_TYPE: u32 = (1 << TYPE_WIDTH) - 1;
        let unknown = ResultType::unknown_type().bits();
        let mut first = (raw >> TYPE_WIDTH) as u8;
        let mut second = (raw & MAX_TYPE) as u8;
        if first == 0 {
            first = unknown;
        }
        if second == 0 {
            second = unknown;
        }
        OperandTypes::new(ResultType::new(first), ResultType::new(second))
    }
}

impl<Traits> OperandDecode for GenericBoundLabel<Traits> {
    fn decode_operand(raw: u32, size: OpcodeSize) -> GenericBoundLabel<Traits> {
        GenericBoundLabel::from_offset(sign_extend(raw, size))
    }
}

/// Tipos cujo `Fits<T, size>::convert(TargetType)` já está em `fits.rs` (`FitsFrom`).
macro_rules! decode_via_fits_from {
    ($($ty:ty),* $(,)?) => {$(
        impl OperandDecode for $ty {
            fn decode_operand(raw: u32, size: OpcodeSize) -> $ty {
                <$ty as FitsFrom>::from_fitted(Fitted::from_raw(raw, size))
            }
        }
    )*};
}

decode_via_fits_from!(SymbolTableOrScopeDepth, PutByIdFlags, PrivateFieldPutKind);

/// `Fits<E, size>` de um `enum` com valores sequenciais a partir de 0: `static_cast<E>(raw)`.
macro_rules! decode_sequential_enum {
    ($ty:ty, [$($variant:expr),* $(,)?]) => {
        impl OperandDecode for $ty {
            fn decode_operand(raw: u32, _size: OpcodeSize) -> $ty {
                const VARIANTS: &[$ty] = &[$($variant),*];
                VARIANTS[raw as usize]
            }
        }
    };
}

decode_sequential_enum!(
    ProfileTypeBytecodeFlag,
    [
        ProfileTypeBytecodeFlag::ProfileTypeBytecodeClosureVar,
        ProfileTypeBytecodeFlag::ProfileTypeBytecodeLocallyResolved,
        ProfileTypeBytecodeFlag::ProfileTypeBytecodeDoesNotHaveGlobalID,
        ProfileTypeBytecodeFlag::ProfileTypeBytecodeFunctionArgument,
        ProfileTypeBytecodeFlag::ProfileTypeBytecodeFunctionReturnStatement,
    ]
);

decode_sequential_enum!(
    DebugHookType,
    [
        DebugHookType::WillExecuteProgram,
        DebugHookType::DidExecuteProgram,
        DebugHookType::DidEnterCallFrame,
        DebugHookType::DidReachDebuggerStatement,
        DebugHookType::WillLeaveCallFrame,
        DebugHookType::WillExecuteStatement,
        DebugHookType::WillExecuteExpression,
        DebugHookType::WillAwait,
        DebugHookType::DidAwait,
    ]
);

decode_sequential_enum!(
    ErrorTypeWithExtension,
    [
        ErrorTypeWithExtension::Error,
        ErrorTypeWithExtension::EvalError,
        ErrorTypeWithExtension::RangeError,
        ErrorTypeWithExtension::ReferenceError,
        ErrorTypeWithExtension::SyntaxError,
        ErrorTypeWithExtension::TypeError,
        ErrorTypeWithExtension::URIError,
        ErrorTypeWithExtension::AggregateError,
        ErrorTypeWithExtension::SuppressedError,
        ErrorTypeWithExtension::OutOfMemoryError,
    ]
);

/// Lê o operando `index` (em unidades de `size`) a partir do primeiro operando, sem alinhamento
/// (o C++ lê por `const uintN_t*`, e as instruções não são alinhadas).
pub fn read_operand(operands: &[u8], index: usize, size: OpcodeSize) -> u32 {
    let at = index * size.bytes();
    match size {
        OpcodeSize::Narrow => operands[at] as u32,
        OpcodeSize::Wide16 => u16::from_ne_bytes([operands[at], operands[at + 1]]) as u32,
        OpcodeSize::Wide32 => u32::from_ne_bytes([operands[at], operands[at + 1], operands[at + 2], operands[at + 3]]),
    }
}

/// O que cada struct `Op*` gerada expõe para a leitura.
pub trait DecodeOp: BytecodeOp + Sized {
    /// Os nomes dos campos (`args:`), na ordem do `.rb`.
    const FIELD_NAMES: &'static [&'static str];

    /// `Op(const uintN_t* stream)`: `operands` aponta para o primeiro operando (opcode e prefixo
    /// não entram) e `size` escolhe o construtor.
    fn from_operands(operands: &[u8], size: OpcodeSize) -> Self;

    /// `static Op decode(const uint8_t* stream)`: `stream` começa no primeiro byte da instrução.
    fn decode(stream: &[u8]) -> Self {
        let size = JSInstruction::new(stream).width();
        let first_operand = size.padding() as usize + opcode_id_width_by_size(size, MAX_JS_OPCODE_ID_WIDTH).bytes();
        Self::from_operands(&stream[first_operand..], size)
    }

    /// Índice do campo de nome `name`, em unidades de operando.
    fn field_index(name: &str) -> Option<usize> {
        Self::FIELD_NAMES.iter().position(|field| *field == name)
    }
}

/// Gera `DecodeOp` a partir da lista de campos de `bytecode_op!`.
macro_rules! impl_op_decode {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        impl $crate::bytecode::bytecode_ops_decode::DecodeOp for $name {
            const FIELD_NAMES: &'static [&'static str] = &[$(stringify!($field)),*];

            #[allow(unused_mut, unused_variables)]
            fn from_operands(operands: &[u8], size: $crate::bytecode::opcode_size::OpcodeSize) -> Self {
                let mut index = 0usize;
                $name {
                    $($field: {
                        let raw = $crate::bytecode::bytecode_ops_decode::read_operand(operands, index, size);
                        index += 1;
                        <$ty as $crate::bytecode::bytecode_ops_decode::OperandDecode>::decode_operand(raw, size)
                    }),*
                }
            }
        }

        $($crate::bytecode::bytecode_ops_decode::impl_target_label_op!($field, $name);)*
        $($crate::bytecode::bytecode_ops_decode::impl_inline_capacity_op!($field, $name);)*

        $crate::bytecode::bytecode_ops_accessors::impl_op_accessors!($name { $($field: $ty),* });
    };
}

/// Marca a struct que tem o operando `targetLabel` (só o campo de nome `target_label` gera algo).
macro_rules! impl_target_label_op {
    (target_label, $name:ident) => {
        impl $crate::bytecode::bytecode_ops_decode::TargetLabelOp for $name {}
    };
    ($other:ident, $name:ident) => {};
}

/// Marca a struct que tem o operando `inlineCapacity` (`OpNewObject`, `OpCreateThis`).
macro_rules! impl_inline_capacity_op {
    (inline_capacity, $name:ident) => {
        impl $crate::bytecode::bytecode_ops_decode::InlineCapacityOp for $name {}
    };
    ($other:ident, $name:ident) => {};
}

pub(crate) use impl_inline_capacity_op;
pub(crate) use impl_op_decode;
pub(crate) use impl_target_label_op;

/// Struct `Op*` com o operando `inlineCapacity` (e, portanto, o setter `setInlineCapacity`).
pub trait InlineCapacityOp: DecodeOp {}

/// Struct `Op*` com o operando `targetLabel` (e, portanto, o setter `setTargetLabel`).
pub trait TargetLabelOp: DecodeOp {}

/// `GenericBoundLabel::saveTarget` só consulta o gerador para rótulos presos a ele; o setter do
/// bytecode recebe sempre um rótulo por deslocamento (`Offset`), então a posição nunca é pedida.
pub(crate) struct NoGenerator;

impl LabelGenerator for NoGenerator {
    fn writer_position(&self) -> i32 {
        unreachable!("setTargetLabel recebe só rótulos por deslocamento")
    }
}

/// `Fits<int, size>::check`.
fn int_fits(value: i32, size: OpcodeSize) -> bool {
    match size {
        OpcodeSize::Narrow => i8::try_from(value).is_ok(),
        OpcodeSize::Wide16 => i16::try_from(value).is_ok(),
        OpcodeSize::Wide32 => true,
    }
}

/// O `T*` de `cast<T>()` para escrita: a instrução viva no fluxo, vista como a struct `T`.
pub struct OpMut<T: DecodeOp> {
    reference: MutableRef,
    marker: PhantomData<T>,
}

impl<T: DecodeOp> OpMut<T> {
    pub(crate) fn new(reference: MutableRef) -> Self {
        OpMut { reference, marker: PhantomData }
    }

    /// `set<Campo>(value, func)` genérico (Opcode/Argument.rb): `index` é a posição do campo em
    /// `args:`. Grava `value` na largura da instrução; `Fits` já resolvido em `stored`.
    fn store(&self, index: usize, stored: impl FnOnce(OpcodeSize) -> i32) {
        self.reference.with_instruction_mut(|bytes| {
            let size = JSInstruction::new(bytes).width();
            let at = size.padding() as usize
                + opcode_id_width_by_size(size, MAX_JS_OPCODE_ID_WIDTH).bytes()
                + index * size.bytes();
            let value = stored(size);
            match size {
                OpcodeSize::Narrow => bytes[at] = value as i8 as u8,
                OpcodeSize::Wide16 => bytes[at..at + 2].copy_from_slice(&(value as i16).to_ne_bytes()),
                OpcodeSize::Wide32 => bytes[at..at + 4].copy_from_slice(&value.to_ne_bytes()),
            }
        });
    }
}

impl<T: InlineCapacityOp> OpMut<T> {
    /// `setInlineCapacity(unsigned value, Functor func)`: se o valor não cabe na largura da
    /// instrução, `func` produz o substituto.
    pub fn set_inline_capacity(&self, value: u32, func: &mut dyn FnMut() -> u32) {
        let index = T::field_index("inline_capacity").expect("InlineCapacityOp sem campo inline_capacity");
        self.store(index, |size| {
            let value = if value.check(size) { value } else { func() };
            value as i32
        });
    }
}

impl<T: TargetLabelOp> OpMut<T> {
    /// `setTargetLabel(BoundLabel value, Functor func)`: se o alvo não cabe na largura da
    /// instrução, `func` produz o rótulo substituto (o salto fora de linha).
    pub fn set_target_label(&self, value: BoundLabel, func: &mut dyn FnMut() -> BoundLabel) {
        let index = T::field_index("target_label").expect("TargetLabelOp sem campo target_label");
        let mut value = value;
        self.store(index, |size| {
            if !int_fits(value.save_target(&NoGenerator), size) {
                value = func();
            }
            value.commit_target()
        });
    }
}
