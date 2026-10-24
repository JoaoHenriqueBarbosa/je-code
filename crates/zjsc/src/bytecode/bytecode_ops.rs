//! Porte das structs `Op*` que o gerador Ruby (`bytecode/generator/`) produz a partir de
//! `bytecode/BytecodeList.rb` em `BytecodeStructs.h`.
//!
//! Cada struct tem os campos (`args:` do `.rb`, na mesma ordem e com os mesmos tipos) e o `emit`
//! gerado por `Opcode#emitter` (`generator/Opcode.rb`): `emit` (menor tamanho desde `Narrow`),
//! `emit_with_smallest_size_requirement` e `emit_at_size` (o `emit<__size, shouldAssert>`).
//! O que o `.rb` faz por `op` fica em `emit_with_smallest_size_requirement`/`emit_impl`: `checkImpl`
//! (`Fits::check` de cada operando, nesta ordem, mais o `opcodeID`), `recordOpcode` e
//! `writeOpcode<size>`. Metadata (`metadata:`) entra só como o operando `__metadataID` que o
//! `emit` acrescenta no fim (`has_metadata`); `tmps:` não entra. `checkpoints:` só chama
//! `setUsesCheckpoints` (`has_checkpoints`). `is`, `as` e `decode` vêm numa fatia seguinte.
//!
//! `Fits.h` (`bytecode::fits` no mapa de nomes) está aqui, na seção `Operand`, até o módulo ganhar
//! arquivo próprio: `Fits<T, size>::check`/`convert` de cada tipo de operando. A direção inversa
//! (`convert(TargetType)`, usada pelos construtores `Op*(const uint8_t*)` e pelo `decode`) vem com
//! o `decode`. O gerador (`BytecodeGenerator`) precisa implementar `OpWriter`: `record_opcode` e
//! `write_opcode` já existem em `BytecodeGeneratorBase`; `add_metadata_for` e
//! `set_uses_checkpoints` são do `BytecodeGenerator.h`.
//!
//! Tipos do `.rb` sem porte próprio ainda:
//! - `JSType` (`enum JSType : uint8_t`) vira `u8`: `runtime::js_type` ainda não existe. Trocar o
//!   tipo do campo quando existir.
//! - `IndexingType` é `u8` (`runtime::indexing_type`).
//! - `BoundLabel` é `GenericBoundLabel<JSGeneratorTraits>`.
//! - Operando marcado com `?` no `.rb` (`thisValue?`) continua `VirtualRegister`; o `?` só permite
//!   o valor inválido na codificação.

use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::put_by_id_flags::PutByIdFlags;
use crate::bytecode::put_kind::PrivateFieldPutKind;
pub use crate::runtime::symbol_table_or_scope_depth::SymbolTableOrScopeDepth;
use crate::bytecode::bytecode_ops_decode::impl_op_decode;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::bytecompiler::bytecode_generator_base::{Fitted, Fits, OpcodeSize};
use crate::bytecompiler::label::{GenericBoundLabel, JSGeneratorTraits, LabelGenerator};
use crate::bytecompiler::profile_type_bytecode_flag::ProfileTypeBytecodeFlag;
use crate::bytecompiler::register_id::{RegisterIDRef, RegisterRef};
use crate::interpreter::interpreter::DebugHookType;
use crate::parser::result_type::{OperandTypes, ResultType};
use crate::runtime::ecma_mode::ECMAMode;
use crate::runtime::error_type::ErrorTypeWithExtension;
use crate::runtime::get_put_info::{GetPutInfo, ResolveType};
use crate::runtime::indexing_type::IndexingType;

/// `BoundLabel` do `.rb`.
pub type BoundLabel = GenericBoundLabel<JSGeneratorTraits>;

/// O que toda struct `Op*` gerada expõe: `static constexpr OpcodeID opcodeID`.
pub trait BytecodeOp {
    const OPCODE_ID: OpcodeID;
}

/// O que o `emit` gerado exige do `BytecodeGenerator` (o `template<typename BytecodeGenerator>`
/// do C++): `recordOpcode`, `writeOpcode<size>`, `addMetadataFor` e `setUsesCheckpoints`.
/// `LabelGenerator` dá o `m_writer.position()` que o `BoundLabel` lê.
pub trait OpWriter: LabelGenerator {
    fn record_opcode(&mut self, opcode_id: OpcodeID);
    fn write_opcode(&mut self, size: OpcodeSize, opcode_id: OpcodeID, ops: &[&dyn Fits]);
    fn add_metadata_for(&mut self, opcode_id: OpcodeID) -> u32;
    fn set_uses_checkpoints(&mut self);
}

/// `Fits<T, size>` (`check` e `convert`) de cada tipo de operando. `convert` recebe `&mut self`
/// porque o `BoundLabel` confirma o alvo (`commitTarget`) ao converter.
pub trait Operand {
    fn check(&mut self, generator: &dyn LabelGenerator, size: OpcodeSize) -> bool;
    fn convert(&mut self, size: OpcodeSize) -> Fitted;
}

/// Conversão do que o bytecompiler tem (`RegisterID*`) para o tipo do campo, como o
/// `VirtualRegister(RegisterID*)` do C++. `None` é o ponteiro nulo: registrador inválido.
pub trait IntoOperand<T> {
    fn into_operand(self) -> T;
}

impl<T> IntoOperand<T> for T {
    fn into_operand(self) -> T {
        self
    }
}

impl IntoOperand<VirtualRegister> for &RegisterRef {
    fn into_operand(self) -> VirtualRegister {
        VirtualRegister::from_register_id(&self.borrow())
    }
}

impl IntoOperand<VirtualRegister> for RegisterRef {
    fn into_operand(self) -> VirtualRegister {
        (&self).into_operand()
    }
}

impl IntoOperand<VirtualRegister> for Option<RegisterRef> {
    fn into_operand(self) -> VirtualRegister {
        (&self).into_operand()
    }
}

impl IntoOperand<VirtualRegister> for &Option<RegisterRef> {
    fn into_operand(self) -> VirtualRegister {
        self.as_ref().map_or_else(VirtualRegister::default, IntoOperand::into_operand)
    }
}

impl IntoOperand<VirtualRegister> for Option<&RegisterRef> {
    fn into_operand(self) -> VirtualRegister {
        self.map_or_else(VirtualRegister::default, IntoOperand::into_operand)
    }
}

/// `JSType` é `enum JSType : uint8_t`; o campo do operando é o byte.
impl IntoOperand<u8> for crate::runtime::js_type::JSType {
    fn into_operand(self) -> u8 {
        self as u8
    }
}

impl IntoOperand<VirtualRegister> for &RegisterIDRef {
    fn into_operand(self) -> VirtualRegister {
        VirtualRegister::from_register_id(&self.borrow())
    }
}

impl IntoOperand<VirtualRegister> for RegisterIDRef {
    fn into_operand(self) -> VirtualRegister {
        (&self).into_operand()
    }
}

impl IntoOperand<VirtualRegister> for Option<RegisterIDRef> {
    fn into_operand(self) -> VirtualRegister {
        (&self).into_operand()
    }
}

impl IntoOperand<VirtualRegister> for &Option<RegisterIDRef> {
    fn into_operand(self) -> VirtualRegister {
        self.as_ref().map_or_else(VirtualRegister::default, IntoOperand::into_operand)
    }
}

/// `FirstConstantRegisterIndex8` e `FirstConstantRegisterIndex16` (`BytecodeConventions.h`).
const FIRST_CONSTANT_REGISTER_INDEX8: i32 = 16;
const FIRST_CONSTANT_REGISTER_INDEX16: i32 = 64;

/// `Fits<unsigned-like, size>::check`.
fn fits_unsigned(value: u32, size: OpcodeSize) -> bool {
    match size {
        OpcodeSize::Narrow => value <= u8::MAX as u32,
        OpcodeSize::Wide16 => value <= u16::MAX as u32,
        OpcodeSize::Wide32 => true,
    }
}

/// `std::numeric_limits<TargetType>::min()` e `max()` do alvo com sinal.
fn min_signed(size: OpcodeSize) -> i32 {
    match size {
        OpcodeSize::Narrow => i8::MIN as i32,
        OpcodeSize::Wide16 => i16::MIN as i32,
        OpcodeSize::Wide32 => i32::MIN,
    }
}

fn max_signed(size: OpcodeSize) -> i32 {
    match size {
        OpcodeSize::Narrow => i8::MAX as i32,
        OpcodeSize::Wide16 => i16::MAX as i32,
        OpcodeSize::Wide32 => i32::MAX,
    }
}

/// `Fits<int, size>::check`.
fn fits_signed(value: i32, size: OpcodeSize) -> bool {
    value >= min_signed(size) && value <= max_signed(size)
}

/// `Fits<unsigned-like, size>::convert`.
fn fitted_unsigned(value: u32, size: OpcodeSize) -> Fitted {
    debug_assert!(fits_unsigned(value, size));
    match size {
        OpcodeSize::Narrow => Fitted::Narrow(value as u8),
        OpcodeSize::Wide16 => Fitted::Wide16(value as u16),
        OpcodeSize::Wide32 => Fitted::Wide32(value),
    }
}

/// `Fits<int, size>::convert`.
fn fitted_signed(value: i32, size: OpcodeSize) -> Fitted {
    debug_assert!(fits_signed(value, size));
    match size {
        OpcodeSize::Narrow => Fitted::Narrow(value as i8 as u8),
        OpcodeSize::Wide16 => Fitted::Wide16(value as i16 as u16),
        OpcodeSize::Wide32 => Fitted::Wide32(value as u32),
    }
}

/// Inteiros sem sinal, `bool`, `ECMAMode`/`ResultType` (um byte) e enums: `Fits<E, size>` por
/// cima do tipo subjacente sem sinal. `$value` extrai o valor.
macro_rules! unsigned_operand {
    ($($ty:ty => |$this:ident| $value:expr),* $(,)?) => {
        $(impl Operand for $ty {
            fn check(&mut self, _generator: &dyn LabelGenerator, size: OpcodeSize) -> bool {
                let $this = &*self;
                fits_unsigned($value, size)
            }

            fn convert(&mut self, size: OpcodeSize) -> Fitted {
                let $this = &*self;
                fitted_unsigned($value, size)
            }
        })*
    };
}

unsigned_operand!(
    u8 => |this| *this as u32,
    u32 => |this| *this,
    bool => |this| *this as u32,
    ResolveType => |this| *this as u32,
    ProfileTypeBytecodeFlag => |this| *this as u32,
    DebugHookType => |this| *this as u32,
    ErrorTypeWithExtension => |this| *this as u32,
    ECMAMode => |this| this.value() as u32,
    ResultType => |this| this.bits() as u32,
    PrivateFieldPutKind => |this| this.value() as u32,
    SymbolTableOrScopeDepth => |this| this.raw_value(),
);

/// `Fits<PutByIdFlags, size>`: `check` sempre passa; `convert` é `isStrict << 1 | isDirect`.
impl Operand for PutByIdFlags {
    fn check(&mut self, _generator: &dyn LabelGenerator, _size: OpcodeSize) -> bool {
        true
    }

    fn convert(&mut self, size: OpcodeSize) -> Fitted {
        fitted_unsigned(((self.ecma_mode().is_strict() as u32) << 1) | self.is_direct() as u32, size)
    }
}

impl Operand for i32 {
    fn check(&mut self, _generator: &dyn LabelGenerator, size: OpcodeSize) -> bool {
        fits_signed(*self, size)
    }

    fn convert(&mut self, size: OpcodeSize) -> Fitted {
        fitted_signed(*self, size)
    }
}

impl VirtualRegister {
    /// `FirstConstant<size>::index`.
    fn first_constant_index(size: OpcodeSize) -> i32 {
        match size {
            OpcodeSize::Narrow => FIRST_CONSTANT_REGISTER_INDEX8,
            _ => FIRST_CONSTANT_REGISTER_INDEX16,
        }
    }
}

/// `Fits<VirtualRegister, size>`: em `Wide32` o `offset()` cru; em `Narrow`/`Wide16` as
/// constantes vão para depois de `FirstConstantRegisterIndex8/16`.
impl Operand for VirtualRegister {
    fn check(&mut self, _generator: &dyn LabelGenerator, size: OpcodeSize) -> bool {
        if size == OpcodeSize::Wide32 {
            return true;
        }
        let first_constant = Self::first_constant_index(size);
        if self.is_constant() {
            return first_constant + self.to_constant_index() <= max_signed(size);
        }
        self.offset() >= min_signed(size) && self.offset() < first_constant
    }

    fn convert(&mut self, size: OpcodeSize) -> Fitted {
        if size == OpcodeSize::Wide32 {
            return Fitted::Wide32(self.offset() as u32);
        }
        let value = if self.is_constant() {
            Self::first_constant_index(size) + self.to_constant_index()
        } else {
            self.offset()
        };
        fitted_signed(value, size)
    }
}

/// `Fits<GetPutInfo, size>`: em `Wide32` o operando cru; senão
/// `isStrict << 7 | resolveType << 3 | initializationMode << 1 | resolveMode`.
impl Operand for GetPutInfo {
    fn check(&mut self, _generator: &dyn LabelGenerator, size: OpcodeSize) -> bool {
        size == OpcodeSize::Wide32
            || ((self.resolve_type() as u32) < (1 << 4)
                && (self.initialization_mode() as u32) < (1 << 2)
                && (self.resolve_mode() as u32) < (1 << 1))
    }

    fn convert(&mut self, size: OpcodeSize) -> Fitted {
        if size == OpcodeSize::Wide32 {
            return Fitted::Wide32(self.operand());
        }
        let packed = ((self.ecma_mode().is_strict() as u32) << 7)
            | ((self.resolve_type() as u32) << 3)
            | ((self.initialization_mode() as u32) << 1)
            | self.resolve_mode() as u32;
        fitted_unsigned(packed, size)
    }
}

impl OperandTypes {
    /// Os dois nibbles de `Fits<OperandTypes, Narrow>`: tipo desconhecido vira 0.
    fn narrow_nibbles(&self) -> (u32, u32) {
        let unknown = ResultType::unknown_type().bits();
        let nibble = |bits: u8| if bits == unknown { 0 } else { bits as u32 };
        (nibble(self.first().bits()), nibble(self.second().bits()))
    }
}

/// `Fits<OperandTypes, size>`: em `Narrow` dois nibbles; senão `bits()`.
impl Operand for OperandTypes {
    fn check(&mut self, _generator: &dyn LabelGenerator, size: OpcodeSize) -> bool {
        if size != OpcodeSize::Narrow {
            return true;
        }
        let (first, second) = self.narrow_nibbles();
        first <= 0xf && second <= 0xf
    }

    fn convert(&mut self, size: OpcodeSize) -> Fitted {
        if size != OpcodeSize::Narrow {
            return fitted_unsigned(self.bits() as u32, size);
        }
        let (first, second) = self.narrow_nibbles();
        fitted_unsigned((first << 4) | second, size)
    }
}

/// `Fits<GenericBoundLabel, size>`: `check` salva o alvo, `convert` o confirma.
impl Operand for BoundLabel {
    fn check(&mut self, generator: &dyn LabelGenerator, size: OpcodeSize) -> bool {
        fits_signed(self.save_target(generator), size)
    }

    fn convert(&mut self, size: OpcodeSize) -> Fitted {
        fitted_signed(self.commit_target(), size)
    }
}

/// `m_writer.position()` congelado: o `check` não escreve nada.
struct Position(i32);

impl LabelGenerator for Position {
    fn writer_position(&self) -> i32 {
        self.0
    }
}

/// Operandos por instrução, no máximo: `op_iterator_next` tem 9 mais o `__metadataID`.
const MAX_OPERANDS: usize = 16;

/// `op :...` com `metadata:` no `BytecodeList.rb` (inclui o `op_group :CreateInternalFieldObjectOp`).
fn has_metadata(opcode_id: OpcodeID) -> bool {
    matches!(
        opcode_id,
        OpcodeID::op_tail_call_varargs
            | OpcodeID::op_call_varargs
            | OpcodeID::op_iterator_next
            | OpcodeID::op_construct_varargs
            | OpcodeID::op_super_construct_varargs
            | OpcodeID::op_iterator_open
            | OpcodeID::op_async_iterator_open
            | OpcodeID::op_instanceof
            | OpcodeID::op_set_private_brand
            | OpcodeID::op_check_private_brand
            | OpcodeID::op_put_by_id
            | OpcodeID::op_construct
            | OpcodeID::op_super_construct
            | OpcodeID::op_tail_call
            | OpcodeID::op_call_direct_eval
            | OpcodeID::op_create_generator
            | OpcodeID::op_create_async_generator
            | OpcodeID::op_create_promise
            | OpcodeID::op_catch
            | OpcodeID::op_new_array_with_size
            | OpcodeID::op_new_array_buffer
            | OpcodeID::op_get_by_id
            | OpcodeID::op_get_length
            | OpcodeID::op_profile_type
            | OpcodeID::op_profile_control_flow
            | OpcodeID::op_new_array_with_species
            | OpcodeID::op_call
            | OpcodeID::op_call_ignore_result
            | OpcodeID::op_async_iterator_next
            | OpcodeID::op_resolve_scope
            | OpcodeID::op_get_from_scope
            | OpcodeID::op_put_to_scope
            | OpcodeID::op_create_this
            | OpcodeID::op_new_object
            | OpcodeID::op_new_array
            | OpcodeID::op_put_private_name
            | OpcodeID::op_get_private_name
            | OpcodeID::op_get_by_val_with_this
            | OpcodeID::op_get_by_val
            | OpcodeID::op_put_by_val
            | OpcodeID::op_put_by_val_direct
            | OpcodeID::op_in_by_val
            | OpcodeID::op_enumerator_next
            | OpcodeID::op_enumerator_in_by_val
            | OpcodeID::op_enumerator_has_own_property
            | OpcodeID::op_enumerator_put_by_val
            | OpcodeID::op_to_this
            | OpcodeID::op_enumerator_get_by_val
            | OpcodeID::op_get_by_id_direct
            | OpcodeID::op_jneq_ptr
    )
}

/// `op :...` com `checkpoints:` no `BytecodeList.rb`.
fn has_checkpoints(opcode_id: OpcodeID) -> bool {
    matches!(
        opcode_id,
        OpcodeID::op_tail_call_varargs
            | OpcodeID::op_call_varargs
            | OpcodeID::op_iterator_next
            | OpcodeID::op_construct_varargs
            | OpcodeID::op_super_construct_varargs
            | OpcodeID::op_iterator_open
            | OpcodeID::op_async_iterator_open
            | OpcodeID::op_instanceof
    )
}

/// `emitImpl<size, recordOpcode = true>`: `checkImpl` (operandos na ordem, o `__metadataID`, o
/// `opcodeID` e os prefixos wide), depois `recordOpcode` e `writeOpcode<size>`.
fn emit_impl(
    generator: &mut dyn OpWriter,
    size: OpcodeSize,
    opcode_id: OpcodeID,
    operands: &mut [&mut dyn Operand],
    mut metadata_id: Option<u32>,
) -> bool {
    if has_checkpoints(opcode_id) {
        generator.set_uses_checkpoints();
    }
    let position = Position(generator.writer_position());
    let opcode_id_size = if size == OpcodeSize::Narrow { OpcodeSize::Narrow } else { OpcodeSize::Wide16 };
    let fits = operands.iter_mut().all(|operand| operand.check(&position, size))
        && metadata_id.as_mut().is_none_or(|metadata| metadata.check(&position, size))
        && fits_unsigned(opcode_id as u32, opcode_id_size)
        && (size != OpcodeSize::Wide16 || fits_unsigned(OpcodeID::op_wide16 as u32, OpcodeSize::Narrow))
        && (size != OpcodeSize::Wide32 || fits_unsigned(OpcodeID::op_wide32 as u32, OpcodeSize::Narrow));
    if !fits {
        return false;
    }
    generator.record_opcode(opcode_id);
    let mut fitted = [Fitted::Narrow(0); MAX_OPERANDS];
    let mut count = 0;
    for operand in operands.iter_mut() {
        fitted[count] = operand.convert(size);
        count += 1;
    }
    if let Some(metadata) = metadata_id.as_mut() {
        fitted[count] = metadata.convert(size);
        count += 1;
    }
    let refs: [&dyn Fits; MAX_OPERANDS] = std::array::from_fn(|index| &fitted[index] as &dyn Fits);
    generator.write_opcode(size, opcode_id, &refs[..count]);
    true
}

/// `emitWithSmallestSizeRequirement<size>`: tenta `Narrow`, `Wide16` e por fim `Wide32`.
pub fn emit_with_smallest_size_requirement(
    generator: &mut dyn OpWriter,
    smallest: OpcodeSize,
    opcode_id: OpcodeID,
    operands: &mut [&mut dyn Operand],
) {
    let metadata_id = has_metadata(opcode_id).then(|| generator.add_metadata_for(opcode_id));
    if smallest == OpcodeSize::Narrow && emit_impl(generator, OpcodeSize::Narrow, opcode_id, operands, metadata_id) {
        return;
    }
    if smallest != OpcodeSize::Wide32 && emit_impl(generator, OpcodeSize::Wide16, opcode_id, operands, metadata_id) {
        return;
    }
    let did_emit = emit_impl(generator, OpcodeSize::Wide32, opcode_id, operands, metadata_id);
    assert!(did_emit);
}

/// `emit<__size, BytecodeGenerator, shouldAssert>`: `false` se não coube em `size`.
pub fn emit_at_size(
    generator: &mut dyn OpWriter,
    size: OpcodeSize,
    should_assert: bool,
    opcode_id: OpcodeID,
    operands: &mut [&mut dyn Operand],
) -> bool {
    let metadata_id = has_metadata(opcode_id).then(|| generator.add_metadata_for(opcode_id));
    let did_emit = emit_impl(generator, size, opcode_id, operands, metadata_id);
    if should_assert {
        debug_assert!(did_emit);
    }
    did_emit
}

/// Gera uma struct `Op*` com os campos do `args:`, o `opcodeID` e o `emit` do gerador Ruby.
macro_rules! bytecode_op {
    ($(#[$meta:meta])* $name:ident, $id:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        pub struct $name {
            $(pub $field: $ty,)*
        }

        impl BytecodeOp for $name {
            const OPCODE_ID: OpcodeID = OpcodeID::$id;
        }

        impl $name {
            /// `static void emit(BytecodeGenerator*, args...)`.
            #[allow(clippy::too_many_arguments)]
            pub fn emit<G: OpWriter>(generator: &mut G $(, $field: impl IntoOperand<$ty>)*) {
                Self::emit_with_smallest_size_requirement(generator, OpcodeSize::Narrow $(, $field)*)
            }

            /// `emitWithSmallestSizeRequirement<size>`.
            #[allow(clippy::too_many_arguments)]
            pub fn emit_with_smallest_size_requirement<G: OpWriter>(
                generator: &mut G,
                size: OpcodeSize
                $(, $field: impl IntoOperand<$ty>)*
            ) {
                $(let mut $field: $ty = $field.into_operand();)*
                emit_with_smallest_size_requirement(
                    generator,
                    size,
                    OpcodeID::$id,
                    &mut [$(&mut $field as &mut dyn Operand),*] as &mut [&mut dyn Operand],
                )
            }

            /// `emit<__size, BytecodeGenerator, shouldAssert>`: `false` se não coube em `size`.
            #[allow(clippy::too_many_arguments)]
            pub fn emit_at_size<G: OpWriter>(
                generator: &mut G,
                size: OpcodeSize,
                should_assert: bool
                $(, $field: impl IntoOperand<$ty>)*
            ) -> bool {
                $(let mut $field: $ty = $field.into_operand();)*
                emit_at_size(
                    generator,
                    size,
                    should_assert,
                    OpcodeID::$id,
                    &mut [$(&mut $field as &mut dyn Operand),*] as &mut [&mut dyn Operand],
                )
            }
        }

        // Lado de leitura (`decode`, construtores por largura): `bytecode_ops_decode.rs`.
        impl_op_decode!($name { $($field: $ty),* });

        // `Op*::dump`: `bytecode_dumper.rs`.
        $crate::bytecode::bytecode_dumper::impl_op_dump!($name, $id { $($field),* });
    };
}

/// `BinaryOp::emit(this, dst, src1, src2[, profile[, types]])` do `emitBinaryOp`. O `if constexpr`
/// do C++ escolhe a aridade pelo `opcodeID`; aqui cada op implementa só o braço que tem e o
/// chamador escolhe pelo `OPCODE_ID`, como o C++.
pub trait BinaryOpcode: BytecodeOp {
    fn emit<G: OpWriter>(
        _generator: &mut G,
        _dst: impl IntoOperand<VirtualRegister>,
        _lhs: impl IntoOperand<VirtualRegister>,
        _rhs: impl IntoOperand<VirtualRegister>,
    ) {
        unreachable!("ASSERT_NOT_REACHED")
    }

    fn emit_with_profile<G: OpWriter>(
        _generator: &mut G,
        _dst: impl IntoOperand<VirtualRegister>,
        _lhs: impl IntoOperand<VirtualRegister>,
        _rhs: impl IntoOperand<VirtualRegister>,
        _profile_index: u32,
    ) {
        unreachable!("ASSERT_NOT_REACHED")
    }

    fn emit_with_profile_and_types<G: OpWriter>(
        _generator: &mut G,
        _dst: impl IntoOperand<VirtualRegister>,
        _lhs: impl IntoOperand<VirtualRegister>,
        _rhs: impl IntoOperand<VirtualRegister>,
        _profile_index: u32,
        _operand_types: OperandTypes,
    ) {
        unreachable!("ASSERT_NOT_REACHED")
    }
}

/// `UnaryOp::emit(this, dst, src[, profile])` do `emitUnaryOp` (mesma regra do `BinaryOpcode`).
pub trait UnaryOpcode: BytecodeOp {
    fn emit<G: OpWriter>(
        _generator: &mut G,
        _dst: impl IntoOperand<VirtualRegister>,
        _src: impl IntoOperand<VirtualRegister>,
    ) {
        unreachable!("ASSERT_NOT_REACHED")
    }

    fn emit_with_profile<G: OpWriter>(
        _generator: &mut G,
        _dst: impl IntoOperand<VirtualRegister>,
        _src: impl IntoOperand<VirtualRegister>,
        _profile_index: u32,
    ) {
        unreachable!("ASSERT_NOT_REACHED")
    }
}

/// Os braços de `BinaryOpcode`: `plain` é o `emit(dst, a, b)`, `profile` acrescenta o
/// `profileIndex` e `profile_types` também o `operandTypes`.
macro_rules! binary_opcode_arm {
    (plain $name:ident) => {
        impl BinaryOpcode for $name {
            fn emit<G: OpWriter>(
                generator: &mut G,
                dst: impl IntoOperand<VirtualRegister>,
                lhs: impl IntoOperand<VirtualRegister>,
                rhs: impl IntoOperand<VirtualRegister>,
            ) {
                $name::emit(generator, dst, lhs, rhs)
            }
        }
    };
    (profile $name:ident) => {
        impl BinaryOpcode for $name {
            fn emit_with_profile<G: OpWriter>(
                generator: &mut G,
                dst: impl IntoOperand<VirtualRegister>,
                lhs: impl IntoOperand<VirtualRegister>,
                rhs: impl IntoOperand<VirtualRegister>,
                profile_index: u32,
            ) {
                $name::emit(generator, dst, lhs, rhs, profile_index)
            }
        }
    };
    (profile_types $name:ident) => {
        impl BinaryOpcode for $name {
            fn emit_with_profile_and_types<G: OpWriter>(
                generator: &mut G,
                dst: impl IntoOperand<VirtualRegister>,
                lhs: impl IntoOperand<VirtualRegister>,
                rhs: impl IntoOperand<VirtualRegister>,
                profile_index: u32,
                operand_types: OperandTypes,
            ) {
                $name::emit(generator, dst, lhs, rhs, profile_index, operand_types)
            }
        }
    };
}

/// Os braços de `UnaryOpcode`: `plain` é o `emit(dst, src)`, `profile` acrescenta o `profileIndex`.
macro_rules! unary_opcode_arm {
    (plain $name:ident) => {
        impl UnaryOpcode for $name {
            fn emit<G: OpWriter>(
                generator: &mut G,
                dst: impl IntoOperand<VirtualRegister>,
                src: impl IntoOperand<VirtualRegister>,
            ) {
                $name::emit(generator, dst, src)
            }
        }
    };
    (profile $name:ident) => {
        impl UnaryOpcode for $name {
            fn emit_with_profile<G: OpWriter>(
                generator: &mut G,
                dst: impl IntoOperand<VirtualRegister>,
                src: impl IntoOperand<VirtualRegister>,
                profile_index: u32,
            ) {
                $name::emit(generator, dst, src, profile_index)
            }
        }
    };
}

/// `op_group :BinaryOp`: `dst`, `lhs`, `rhs`.
macro_rules! binary_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { dst: VirtualRegister, lhs: VirtualRegister, rhs: VirtualRegister }
        );
        binary_opcode_arm!(plain $name);)*
    };
}

/// `op_group :ProfiledBinaryOpWithOperandTypes`: `BinaryOp` mais `profileIndex` e `operandTypes`.
macro_rules! profiled_binary_op_with_operand_types {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id {
                dst: VirtualRegister,
                lhs: VirtualRegister,
                rhs: VirtualRegister,
                profile_index: u32,
                operand_types: OperandTypes,
            }
        );
        binary_opcode_arm!(profile_types $name);)*
    };
}

/// `op_group :ProfiledBinaryOp`: `BinaryOp` mais `profileIndex`.
macro_rules! profiled_binary_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id {
                dst: VirtualRegister,
                lhs: VirtualRegister,
                rhs: VirtualRegister,
                profile_index: u32,
            }
        );
        binary_opcode_arm!(profile $name);)*
    };
}

/// `op_group :UnaryOp`: `dst`, `operand`.
macro_rules! unary_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { dst: VirtualRegister, operand: VirtualRegister }
        );
        unary_opcode_arm!(plain $name);)*
    };
}

/// `op_call_varargs` e variações: o `.rb` repete os mesmos `args:` (`valueProfile` em todas,
/// menos em `tail_call_varargs`).
macro_rules! varargs_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id {
                dst: VirtualRegister,
                callee: VirtualRegister,
                this_value: VirtualRegister,
                arguments: VirtualRegister,
                first_free: VirtualRegister,
                first_var_arg: i32,
                value_profile: u32,
            }
        );)*
    };
}

binary_op! {
    OpEq => op_eq,
    OpNeq => op_neq,
    OpStricteq => op_stricteq,
    OpNstricteq => op_nstricteq,
    OpLess => op_less,
    OpLesseq => op_lesseq,
    OpGreater => op_greater,
    OpGreatereq => op_greatereq,
    OpBelow => op_below,
    OpBeloweq => op_beloweq,
    OpMod => op_mod,
    OpPow => op_pow,
    OpUrshift => op_urshift,
}

profiled_binary_op_with_operand_types! {
    OpAdd => op_add,
    OpMul => op_mul,
    OpDiv => op_div,
    OpSub => op_sub,
    OpBitand => op_bitand,
    OpBitor => op_bitor,
    OpBitxor => op_bitxor,
}

profiled_binary_op! {
    OpLshift => op_lshift,
    OpRshift => op_rshift,
}

unary_op! {
    OpEqNull => op_eq_null,
    OpNeqNull => op_neq_null,
    OpToString => op_to_string,
    OpIsEmpty => op_is_empty,
    OpTypeofIsUndefined => op_typeof_is_undefined,
    OpTypeofIsObject => op_typeof_is_object,
    OpTypeofIsFunction => op_typeof_is_function,
    OpIsUndefinedOrNull => op_is_undefined_or_null,
    OpIsBoolean => op_is_boolean,
    OpIsNumber => op_is_number,
    OpIsBigInt => op_is_big_int,
    OpIsObject => op_is_object,
    OpIsCallable => op_is_callable,
    OpIsConstructor => op_is_constructor,
}

/// `op_group :ProfiledUnaryOp`: `dst`, `operand`, `profileIndex`.
macro_rules! profiled_unary_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { dst: VirtualRegister, operand: VirtualRegister, profile_index: u32 }
        );
        unary_opcode_arm!(profile $name);)*
    };
}

profiled_unary_op! {
    OpToNumber => op_to_number,
    OpToNumeric => op_to_numeric,
    OpBitnot => op_bitnot,
    OpUnsigned => op_unsigned,
}

/// `op_group :UnaryInPlaceProfiledOp`: `srcDst`, `profileIndex`.
macro_rules! unary_in_place_profiled_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { src_dst: VirtualRegister, profile_index: u32 }
        );)*
    };
}

unary_in_place_profiled_op! {
    OpInc => op_inc,
    OpDec => op_dec,
}

/// `op_group :CreateInternalFieldObjectOp`: `dst`, `callee`. `op :create_promise` repete o
/// mesmo `args:` fora do grupo.
macro_rules! create_internal_field_object_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { dst: VirtualRegister, callee: VirtualRegister }
        );)*
    };
}

create_internal_field_object_op! {
    OpCreateGenerator => op_create_generator,
    OpCreateAsyncGenerator => op_create_async_generator,
    OpCreatePromise => op_create_promise,
}

/// `op_group :BinaryJmp`: `lhs`, `rhs`, `targetLabel`. Sem `Clone`/`PartialEq`: o
/// `GenericBoundLabel` não os tem.
macro_rules! binary_jmp {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            $name, $id { lhs: VirtualRegister, rhs: VirtualRegister, target_label: BoundLabel }
        );)*
    };
}

binary_jmp! {
    OpJeq => op_jeq,
    OpJstricteq => op_jstricteq,
    OpJneq => op_jneq,
    OpJnstricteq => op_jnstricteq,
    OpJless => op_jless,
    OpJlesseq => op_jlesseq,
    OpJgreater => op_jgreater,
    OpJgreatereq => op_jgreatereq,
    OpJnless => op_jnless,
    OpJnlesseq => op_jnlesseq,
    OpJngreater => op_jngreater,
    OpJngreatereq => op_jngreatereq,
    OpJbelow => op_jbelow,
    OpJbeloweq => op_jbeloweq,
}

/// `op :jtrue`, `jfalse`, `jeq_null`, `jneq_null`, `jundefined_or_null`, `jnundefined_or_null`:
/// um registrador e `targetLabel` (o nome do primeiro campo muda: `condition` ou `value`).
macro_rules! unary_jmp {
    ($($name:ident => $id:ident { $field:ident }),* $(,)?) => {
        $(bytecode_op!(
            $name, $id { $field: VirtualRegister, target_label: BoundLabel }
        );)*
    };
}

unary_jmp! {
    OpJtrue => op_jtrue { condition },
    OpJfalse => op_jfalse { condition },
    OpJeqNull => op_jeq_null { value },
    OpJneqNull => op_jneq_null { value },
    OpJundefinedOrNull => op_jundefined_or_null { value },
    OpJnundefinedOrNull => op_jnundefined_or_null { value },
}

/// `op_group :SwitchValue`: `tableIndex`, `scrutinee`.
macro_rules! switch_value {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { table_index: u32, scrutinee: VirtualRegister }
        );)*
    };
}

switch_value! {
    OpSwitchImm => op_switch_imm,
    OpSwitchChar => op_switch_char,
    OpSwitchString => op_switch_string,
}

/// `op_group :NewFunction`: `dst`, `scope`, `functionDecl`.
macro_rules! new_function {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { dst: VirtualRegister, scope: VirtualRegister, function_decl: u32 }
        );)*
    };
}

new_function! {
    OpNewFunc => op_new_func,
    OpNewFuncExp => op_new_func_exp,
    OpNewGeneratorFunc => op_new_generator_func,
    OpNewGeneratorFuncExp => op_new_generator_func_exp,
    OpNewAsyncFunc => op_new_async_func,
    OpNewAsyncFuncExp => op_new_async_func_exp,
    OpNewAsyncGeneratorFunc => op_new_async_generator_func,
    OpNewAsyncGeneratorFuncExp => op_new_async_generator_func_exp,
}

/// Ops com `dst` e `src` (`to_primitive`, `to_property_key`, `to_property_key_or_number`).
macro_rules! dst_src_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { dst: VirtualRegister, src: VirtualRegister }
        );)*
    };
}

dst_src_op! {
    OpToPrimitive => op_to_primitive,
    OpToPropertyKey => op_to_property_key,
    OpToPropertyKeyOrNumber => op_to_property_key_or_number,
}

/// Ops sem `args:` (`nop`, `unreachable`, `check_traps`, `loop_hint`, `super_sampler_begin/end`,
/// `wide16`, `wide32`). `op :enter` está mais abaixo.
macro_rules! no_args_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id {}
        );)*
    };
}

no_args_op! {
    OpLoopHint => op_loop_hint,
    OpUnreachable => op_unreachable,
    OpCheckTraps => op_check_traps,
    OpNop => op_nop,
    OpSuperSamplerBegin => op_super_sampler_begin,
    OpWide16 => op_wide16,
    OpSuperSamplerEnd => op_super_sampler_end,
    OpWide32 => op_wide32,
}

/// Ops com só `dst` (`get_scope`, `create_direct_arguments`, `create_cloned_arguments`,
/// `new_promise`, `new_generator`, `new_async_function_generator`, `argument_count`).
macro_rules! dst_only_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { dst: VirtualRegister }
        );)*
    };
}

dst_only_op! {
    OpGetScope => op_get_scope,
    OpCreateDirectArguments => op_create_direct_arguments,
    OpCreateClonedArguments => op_create_cloned_arguments,
    OpNewPromise => op_new_promise,
    OpNewGenerator => op_new_generator,
    OpNewAsyncFunctionGenerator => op_new_async_function_generator,
    OpArgumentCount => op_argument_count,
}

/// `op :create_lexical_environment` e `op :create_generator_frame_environment` (ops separados no
/// `.rb`) repetem `dst`, `scope`, `symbolTable`, `initialValue`.
macro_rules! lexical_environment_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id {
                dst: VirtualRegister,
                scope: VirtualRegister,
                symbol_table: VirtualRegister,
                initial_value: VirtualRegister,
            }
        );)*
    };
}

lexical_environment_op! {
    OpCreateLexicalEnvironment => op_create_lexical_environment,
    OpCreateGeneratorFrameEnvironment => op_create_generator_frame_environment,
}

varargs_op! {
    OpCallVarargs => op_call_varargs,
    OpConstructVarargs => op_construct_varargs,
    OpSuperConstructVarargs => op_super_construct_varargs,
}

// `op :tail_call_varargs` não tem `valueProfile`.
bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpTailCallVarargs, op_tail_call_varargs {
        dst: VirtualRegister,
        callee: VirtualRegister,
        this_value: VirtualRegister,
        arguments: VirtualRegister,
        first_free: VirtualRegister,
        first_var_arg: i32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpCall, op_call {
        dst: VirtualRegister,
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
        value_profile: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpConstruct, op_construct {
        dst: VirtualRegister,
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
        value_profile: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpSuperConstruct, op_super_construct {
        dst: VirtualRegister,
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
        value_profile: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpCallIgnoreResult, op_call_ignore_result {
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpTailCall, op_tail_call {
        dst: VirtualRegister,
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpCallDirectEval, op_call_direct_eval {
        dst: VirtualRegister,
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
        this_value: VirtualRegister,
        scope: VirtualRegister,
        lexically_scoped_features: u32,
        value_profile: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpNot, op_not { dst: VirtualRegister, operand: VirtualRegister }
);
unary_opcode_arm!(plain OpNot);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpTypeof, op_typeof { dst: VirtualRegister, value: VirtualRegister }
);
unary_opcode_arm!(plain OpTypeof);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpMov, op_mov { dst: VirtualRegister, src: VirtualRegister }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpPutToScope, op_put_to_scope {
        scope: VirtualRegister,
        var: u32,
        value: VirtualRegister,
        get_put_info: GetPutInfo,
        symbol_table_or_scope_depth: SymbolTableOrScopeDepth,
        offset: u32,
    }
);

// Ops com `BoundLabel` (sem `Clone`/`PartialEq`, o `GenericBoundLabel` não os tem).
bytecode_op!(
    OpJmp, op_jmp { target_label: BoundLabel }
);

bytecode_op!(
    OpJneqPtr, op_jneq_ptr {
        value: VirtualRegister,
        special_pointer: VirtualRegister,
        target_label: BoundLabel,
    }
);

// `op :jeq_ptr` tem os mesmos `args:` do `jneq_ptr`, mas sem `metadata:`.
bytecode_op!(
    OpJeqPtr, op_jeq_ptr {
        value: VirtualRegister,
        special_pointer: VirtualRegister,
        target_label: BoundLabel,
    }
);

// `op :enter` não tem `args:`.
bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpEnter, op_enter {}
);

/// Ops soltas do `.rb`, cada uma com seus `args:` na ordem do arquivo. Os campos `?` (`identifier?`)
/// seguem a regra do cabeçalho: mesmo tipo, o `?` só admite o valor inválido na codificação.
macro_rules! plain_ops {
    ($($name:ident => $id:ident { $($field:ident: $ty:ty),* $(,)? }),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { $($field: $ty),* }
        );)*
    };
}

plain_ops! {
    OpIteratorNext => op_iterator_next {
        done: VirtualRegister,
        value: VirtualRegister,
        iterable: VirtualRegister,
        next: VirtualRegister,
        iterator: VirtualRegister,
        stack_offset: u32,
        next_result_value_profile: u32,
        done_value_profile: u32,
        value_value_profile: u32,
    },
    OpIteratorOpen => op_iterator_open {
        iterator: VirtualRegister,
        next: VirtualRegister,
        symbol_iterator: VirtualRegister,
        iterable: VirtualRegister,
        stack_offset: u32,
        iterable_value_profile: u32,
        iterator_value_profile: u32,
        next_value_profile: u32,
    },
    OpAsyncIteratorOpen => op_async_iterator_open {
        iterator: VirtualRegister,
        next: VirtualRegister,
        symbol_iterator: VirtualRegister,
        iterable: VirtualRegister,
        stack_offset: u32,
        iterable_value_profile: u32,
        iterator_value_profile: u32,
        next_value_profile: u32,
    },
    OpAsyncIteratorNext => op_async_iterator_next {
        dst: VirtualRegister,
        next: VirtualRegister,
        iterator: VirtualRegister,
        driver: VirtualRegister,
        has_value: bool,
        stack_offset: u32,
        value_profile: u32,
    },
    OpInstanceof => op_instanceof {
        dst: VirtualRegister,
        value: VirtualRegister,
        constructor: VirtualRegister,
        has_instance_or_prototype: VirtualRegister,
        has_instance_value_profile: u32,
        prototype_value_profile: u32,
    },
    OpSetPrivateBrand => op_set_private_brand { base: VirtualRegister, brand: VirtualRegister },
    OpCheckPrivateBrand => op_check_private_brand { base: VirtualRegister, brand: VirtualRegister },
    OpPutById => op_put_by_id {
        base: VirtualRegister,
        property: u32,
        value: VirtualRegister,
        flags: PutByIdFlags,
    },
    OpCatch => op_catch { exception: VirtualRegister, thrown_value: VirtualRegister },
    OpNewArrayWithSize => op_new_array_with_size { dst: VirtualRegister, length: VirtualRegister },
    OpNewArrayBuffer => op_new_array_buffer {
        dst: VirtualRegister,
        immutable_butterfly: VirtualRegister,
        recommended_indexing_type: IndexingType,
    },
    OpGetById => op_get_by_id {
        dst: VirtualRegister,
        base: VirtualRegister,
        property: u32,
        value_profile: u32,
    },
    OpGetLength => op_get_length { dst: VirtualRegister, base: VirtualRegister, value_profile: u32 },
    OpProfileType => op_profile_type {
        target_virtual_register: VirtualRegister,
        symbol_table_or_scope_depth: SymbolTableOrScopeDepth,
        flag: ProfileTypeBytecodeFlag,
        identifier: u32,
        resolve_type: ResolveType,
    },
    OpProfileControlFlow => op_profile_control_flow { text_offset: i32 },
    OpNewArrayWithSpecies => op_new_array_with_species {
        dst: VirtualRegister,
        length: VirtualRegister,
        array: VirtualRegister,
        value_profile: u32,
    },
    OpResolveScope => op_resolve_scope {
        dst: VirtualRegister,
        scope: VirtualRegister,
        var: u32,
        resolve_type: ResolveType,
        local_scope_depth: u32,
    },
    OpGetFromScope => op_get_from_scope {
        dst: VirtualRegister,
        scope: VirtualRegister,
        var: u32,
        get_put_info: GetPutInfo,
        local_scope_depth: u32,
        offset: u32,
        value_profile: u32,
    },
    OpCreateThis => op_create_this {
        dst: VirtualRegister,
        callee: VirtualRegister,
        inline_capacity: u32,
    },
    OpNewObject => op_new_object { dst: VirtualRegister, inline_capacity: u32 },
    OpNewArray => op_new_array {
        dst: VirtualRegister,
        argv: VirtualRegister,
        argc: u32,
        recommended_indexing_type: IndexingType,
    },
    OpPutPrivateName => op_put_private_name {
        base: VirtualRegister,
        property: VirtualRegister,
        value: VirtualRegister,
        put_kind: PrivateFieldPutKind,
    },
    OpGetPrivateName => op_get_private_name {
        dst: VirtualRegister,
        base: VirtualRegister,
        property: VirtualRegister,
        value_profile: u32,
    },
    OpGetByValWithThis => op_get_by_val_with_this {
        dst: VirtualRegister,
        base: VirtualRegister,
        this_value: VirtualRegister,
        property: VirtualRegister,
        value_profile: u32,
    },
    OpGetByVal => op_get_by_val {
        dst: VirtualRegister,
        base: VirtualRegister,
        property: VirtualRegister,
        value_profile: u32,
    },
    OpPutByVal => op_put_by_val {
        base: VirtualRegister,
        property: VirtualRegister,
        value: VirtualRegister,
        ecma_mode: ECMAMode,
    },
    OpPutByValDirect => op_put_by_val_direct {
        base: VirtualRegister,
        property: VirtualRegister,
        value: VirtualRegister,
        ecma_mode: ECMAMode,
    },
    OpInByVal => op_in_by_val {
        dst: VirtualRegister,
        base: VirtualRegister,
        property: VirtualRegister,
    },
    OpEnumeratorNext => op_enumerator_next {
        property_name: VirtualRegister,
        mode: VirtualRegister,
        index: VirtualRegister,
        base: VirtualRegister,
        enumerator: VirtualRegister,
    },
    OpEnumeratorInByVal => op_enumerator_in_by_val {
        dst: VirtualRegister,
        base: VirtualRegister,
        mode: VirtualRegister,
        property_name: VirtualRegister,
        index: VirtualRegister,
        enumerator: VirtualRegister,
    },
    OpEnumeratorHasOwnProperty => op_enumerator_has_own_property {
        dst: VirtualRegister,
        base: VirtualRegister,
        mode: VirtualRegister,
        property_name: VirtualRegister,
        index: VirtualRegister,
        enumerator: VirtualRegister,
    },
    OpEnumeratorPutByVal => op_enumerator_put_by_val {
        base: VirtualRegister,
        mode: VirtualRegister,
        property_name: VirtualRegister,
        index: VirtualRegister,
        enumerator: VirtualRegister,
        value: VirtualRegister,
        ecma_mode: ECMAMode,
    },
    OpToThis => op_to_this {
        src_dst: VirtualRegister,
        ecma_mode: ECMAMode,
        value_profile: u32,
    },
    OpEnumeratorGetByVal => op_enumerator_get_by_val {
        dst: VirtualRegister,
        base: VirtualRegister,
        mode: VirtualRegister,
        property_name: VirtualRegister,
        index: VirtualRegister,
        enumerator: VirtualRegister,
        value_profile: u32,
    },
    OpGetByIdDirect => op_get_by_id_direct {
        dst: VirtualRegister,
        base: VirtualRegister,
        property: u32,
        value_profile: u32,
    },
    OpGetArgument => op_get_argument { dst: VirtualRegister, index: i32, value_profile: u32 },
    OpGetFromArguments => op_get_from_arguments {
        dst: VirtualRegister,
        arguments: VirtualRegister,
        index: u32,
        value_profile: u32,
    },
    OpGetPrototypeOf => op_get_prototype_of {
        dst: VirtualRegister,
        value: VirtualRegister,
        value_profile: u32,
    },
    OpGetInternalField => op_get_internal_field {
        dst: VirtualRegister,
        base: VirtualRegister,
        index: u32,
        value_profile: u32,
    },
    OpGetByIdWithThis => op_get_by_id_with_this {
        dst: VirtualRegister,
        base: VirtualRegister,
        this_value: VirtualRegister,
        property: u32,
        value_profile: u32,
    },
    OpToObject => op_to_object {
        dst: VirtualRegister,
        operand: VirtualRegister,
        message: u32,
        value_profile: u32,
    },
    OpInById => op_in_by_id { dst: VirtualRegister, base: VirtualRegister, property: u32 },
    OpHasPrivateName => op_has_private_name {
        dst: VirtualRegister,
        base: VirtualRegister,
        property: VirtualRegister,
    },
    OpHasPrivateBrand => op_has_private_brand {
        dst: VirtualRegister,
        base: VirtualRegister,
        brand: VirtualRegister,
    },
    OpPutByIdWithThis => op_put_by_id_with_this {
        base: VirtualRegister,
        this_value: VirtualRegister,
        property: u32,
        value: VirtualRegister,
        ecma_mode: ECMAMode,
    },
    OpDelById => op_del_by_id {
        dst: VirtualRegister,
        base: VirtualRegister,
        property: u32,
        ecma_mode: ECMAMode,
    },
    OpPutByValWithThis => op_put_by_val_with_this {
        base: VirtualRegister,
        this_value: VirtualRegister,
        property: VirtualRegister,
        value: VirtualRegister,
        ecma_mode: ECMAMode,
    },
    OpDelByVal => op_del_by_val {
        dst: VirtualRegister,
        base: VirtualRegister,
        property: VirtualRegister,
        ecma_mode: ECMAMode,
    },
    OpPutGetterById => op_put_getter_by_id {
        base: VirtualRegister,
        property: u32,
        attributes: u32,
        accessor: VirtualRegister,
    },
    OpPutSetterById => op_put_setter_by_id {
        base: VirtualRegister,
        property: u32,
        attributes: u32,
        accessor: VirtualRegister,
    },
    OpPutGetterSetterById => op_put_getter_setter_by_id {
        base: VirtualRegister,
        property: u32,
        attributes: u32,
        getter: VirtualRegister,
        setter: VirtualRegister,
    },
    OpPutGetterByVal => op_put_getter_by_val {
        base: VirtualRegister,
        property: VirtualRegister,
        attributes: u32,
        accessor: VirtualRegister,
    },
    OpPutSetterByVal => op_put_setter_by_val {
        base: VirtualRegister,
        property: VirtualRegister,
        attributes: u32,
        accessor: VirtualRegister,
    },
    OpDefineDataProperty => op_define_data_property {
        base: VirtualRegister,
        property: VirtualRegister,
        value: VirtualRegister,
        attributes: VirtualRegister,
    },
    OpDefineAccessorProperty => op_define_accessor_property {
        base: VirtualRegister,
        property: VirtualRegister,
        getter: VirtualRegister,
        setter: VirtualRegister,
        attributes: VirtualRegister,
    },
    OpSetFunctionName => op_set_function_name { function: VirtualRegister, name: VirtualRegister },
    OpRet => op_ret { value: VirtualRegister },
    OpStrcat => op_strcat { dst: VirtualRegister, src: VirtualRegister, count: i32 },
    OpPutToArguments => op_put_to_arguments {
        arguments: VirtualRegister,
        index: u32,
        value: VirtualRegister,
    },
    OpPushWithScope => op_push_with_scope {
        dst: VirtualRegister,
        current_scope: VirtualRegister,
        new_scope: VirtualRegister,
    },
    OpGetParentScope => op_get_parent_scope { dst: VirtualRegister, scope: VirtualRegister },
    OpThrow => op_throw { value: VirtualRegister },
    OpThrowStaticError => op_throw_static_error {
        message: VirtualRegister,
        error_type: ErrorTypeWithExtension,
    },
    OpDebug => op_debug { debug_hook_type: DebugHookType, data: VirtualRegister },
    OpGetPropertyEnumerator => op_get_property_enumerator {
        dst: VirtualRegister,
        base: VirtualRegister,
    },
    OpCreateRest => op_create_rest { dst: VirtualRegister, num_parameters_to_skip: u32 },
    OpYield => op_yield { yield_point: u32, argument: VirtualRegister },
    OpLogShadowChickenPrologue => op_log_shadow_chicken_prologue { scope: VirtualRegister },
    OpLogShadowChickenTail => op_log_shadow_chicken_tail {
        this_value: VirtualRegister,
        scope: VirtualRegister,
    },
    OpResolveScopeForHoistingFuncDeclInEval => op_resolve_scope_for_hoisting_func_decl_in_eval {
        dst: VirtualRegister,
        scope: VirtualRegister,
        property: u32,
    },
    OpPutInternalField => op_put_internal_field {
        base: VirtualRegister,
        index: u32,
        value: VirtualRegister,
    },
    OpCreateScopedArguments => op_create_scoped_arguments {
        dst: VirtualRegister,
        scope: VirtualRegister,
    },
    OpCheckTdz => op_check_tdz {
        target_virtual_register: VirtualRegister,
        identifier: VirtualRegister,
    },
    OpNewArrayWithSpread => op_new_array_with_spread {
        dst: VirtualRegister,
        argv: VirtualRegister,
        argc: u32,
        bit_vector: u32,
    },
    OpSpread => op_spread { dst: VirtualRegister, argument: VirtualRegister },
    OpNewRegExp => op_new_reg_exp { dst: VirtualRegister, regexp: VirtualRegister },
    OpNegate => op_negate {
        dst: VirtualRegister,
        operand: VirtualRegister,
        profile_index: u32,
        result_type: ResultType,
    },
    OpIdentityWithProfile => op_identity_with_profile {
        src_dst: VirtualRegister,
        top_profile: u32,
        bottom_profile: u32,
    },
    OpIsCellWithType => op_is_cell_with_type {
        dst: VirtualRegister,
        operand: VirtualRegister,
        type_: u8,
    },
    OpHasStructureWithFlags => op_has_structure_with_flags {
        dst: VirtualRegister,
        operand: VirtualRegister,
        flags: u32,
    },
}
