//! Porte de `bytecode/InstructionStream.h` e `InstructionStream.cpp` (o `.cpp` só declara o alocador
//! `InstructionStream`, que o alocador do Rust substitui), mais o `JSInstruction`
//! (`BaseInstruction<JSOpcodeTraits>`, de `bytecode/Instruction.h`) que o fluxo guarda.
//!
//! No C++ uma `Ref` é um ponteiro para o `std::span` do fluxo mais um índice, e `->` devolve um
//! ponteiro para os bytes da instrução; uma `Ref` criada enquanto o escritor ainda acrescenta
//! continua enxergando os bytes atuais. Em Rust seguro o buffer é um `Rc<RefCell<Vec<u8>>>`
//! (`Bytes`) compartilhado entre o fluxo, o escritor e as referências, o que dá a mesma
//! propriedade. O acesso aos bytes da instrução se faz por fechamento (`with_instruction`,
//! `with_instruction_mut`), no lugar do ponteiro devolvido por `operator->`.
//!
//! Não portado, por depender de peças que ainda não existem ou por serem ponteiro cru:
//! - `BytecodeIndex` (`index()` das refs, `at(BytecodeIndex)`): a classe ainda não foi portada.
//!   `offset()` e `at(offset)` cobrem o mesmo.
//! - `InstructionStream(Bytes, BorrowTag)` / `isBorrowed()`: fluxo emprestado de um mapeamento do
//!   cache de bytecode (`CachedCodeBlock`), que não existe aqui. `owned_size_in_bytes` é sempre o
//!   tamanho.
//! - `rawPointer()` e `contains(InstructionType*)`: aritmética de ponteiro.
//! - `InstructionBufferMalloc::nextCapacity` (política de crescimento do `Vector`): sem efeito
//!   observável.
//! - `as<T>()` é `as_op`, `asKnownWidth<T>()` é `as_known_width` e `cast<T>()` não constante é
//!   `MutableRef::cast_mut` (ver `bytecode_ops_decode.rs`); o `cast<T>() const` não existe porque
//!   não tem chamador e devolveria um ponteiro para dentro dos bytes.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::bytecode_ops::BytecodeOp;
use crate::bytecode::bytecode_ops_decode::{DecodeOp, OpMut};
use crate::bytecode::opcode::{OpcodeID, OPCODE_LENGTHS, OPCODE_NAMES};
use crate::bytecode::opcode_size::{opcode_id_width_by_size, OpcodeSize, MAX_JS_OPCODE_ID_WIDTH};
use crate::bytecode::opcode_traits::JSOpcodeTraits;

/// `Instruction.h`: `NUMBER_OF_BYTECODE_WITH_CHECKPOINTS` (de `derived/.../Bytecodes.h`).
pub const NUMBER_OF_BYTECODE_WITH_CHECKPOINTS: u16 = 8;
/// `Instruction.h`: `NUMBER_OF_BYTECODE_WITH_METADATA` (de `derived/.../Bytecodes.h`).
pub const NUMBER_OF_BYTECODE_WITH_METADATA: u16 = 50;
/// `bytecodeCheckpointCountTable` (de `derived/.../Bytecodes.h`).
pub static BYTECODE_CHECKPOINT_COUNT_TABLE: [u32; NUMBER_OF_BYTECODE_WITH_CHECKPOINTS as usize] =
    [2, 2, 3, 2, 2, 2, 2, 3];

const OP_WIDE16: u8 = OpcodeID::op_wide16 as u16 as u8;
const OP_WIDE32: u8 = OpcodeID::op_wide32 as u16 as u8;

/// O buffer de bytes compartilhado (`InstructionBuffer` mais o `std::span` do C++).
pub type Bytes = Rc<RefCell<Vec<u8>>>;

/// Instrução vista como bytes a partir do começo dela (o `JSInstruction*` do C++, que tem
/// `sizeof == 1` e serve só de ponteiro para o primeiro byte). A fatia vai até o fim do buffer.
#[derive(Clone, Copy, Debug)]
pub struct JSInstruction<'a> {
    bytes: &'a [u8],
}

impl<'a> JSInstruction<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        JSInstruction { bytes }
    }

    /// Os bytes a partir do primeiro byte da instrução.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// `narrow()->opcodeID()`: o primeiro byte.
    fn narrow_opcode(&self) -> u8 {
        self.bytes[0]
    }

    /// `opcodeID()`. O valor numérico do `OpcodeID` (a posição em `FOR_EACH_BYTECODE_ID`).
    pub fn opcode_id(&self) -> u16 {
        // `wide32()`/`wide16()` apontam para `this + 1`, o opcode de verdade vem depois do prefixo.
        let at = if self.is_wide32() || self.is_wide16() { 1 } else { 0 };
        match self.opcode_id_width() {
            OpcodeSize::Narrow => self.bytes[at] as u16,
            _ => u16::from_ne_bytes([self.bytes[at], self.bytes[at + 1]]),
        }
    }

    /// `opcodeID()` como o enum `OpcodeID`.
    pub fn opcode_id_enum(&self) -> OpcodeID {
        OpcodeID::from_u32(self.opcode_id() as u32)
    }

    /// `name()`.
    pub fn name(&self) -> &'static str {
        OPCODE_NAMES[self.opcode_id() as usize]
    }

    /// `isWide16()`.
    pub fn is_wide16(&self) -> bool {
        self.narrow_opcode() == OP_WIDE16
    }

    /// `isWide32()`.
    pub fn is_wide32(&self) -> bool {
        self.narrow_opcode() == OP_WIDE32
    }

    /// `width()`.
    pub fn width(&self) -> OpcodeSize {
        if self.is_wide32() {
            return OpcodeSize::Wide32;
        }
        if self.is_wide16() {
            OpcodeSize::Wide16
        } else {
            OpcodeSize::Narrow
        }
    }

    /// `hasMetadata()`.
    pub fn has_metadata(&self) -> bool {
        self.opcode_id() < NUMBER_OF_BYTECODE_WITH_METADATA
    }

    /// `hasCheckpoints()`.
    pub fn has_checkpoints(&self) -> bool {
        self.opcode_id() < NUMBER_OF_BYTECODE_WITH_CHECKPOINTS
    }

    /// `numberOfCheckpoints()`.
    pub fn number_of_checkpoints(&self) -> u32 {
        if !self.has_checkpoints() {
            return 1;
        }
        BYTECODE_CHECKPOINT_COUNT_TABLE[self.opcode_id() as usize]
    }

    /// `sizeShiftAmount()`.
    pub fn size_shift_amount(&self) -> i32 {
        if self.is_wide32() {
            return 2;
        }
        if self.is_wide16() {
            1
        } else {
            0
        }
    }

    /// `opcodeIDWidth()`.
    pub fn opcode_id_width(&self) -> OpcodeSize {
        opcode_id_width_by_size(self.width(), MAX_JS_OPCODE_ID_WIDTH)
    }

    /// `opcodeIDBytes()`.
    pub fn opcode_id_bytes(&self) -> u32 {
        self.opcode_id_width() as u32
    }

    /// `size()`: prefixo wide, opcode e operandos.
    pub fn size(&self) -> usize {
        let size_shift_amount = self.size_shift_amount();
        let prefix_size: usize = if size_shift_amount != 0 { 1 } else { 0 };
        let operand_size: usize = 1usize << size_shift_amount;
        let size_of_bytecode = self.opcode_id_bytes() as usize;
        size_of_bytecode + OPCODE_LENGTHS[self.opcode_id() as usize] as usize * operand_size + prefix_size
    }

    /// `is<T>()`.
    pub fn is<T: BytecodeOp>(&self) -> bool {
        self.opcode_id() == T::OPCODE_ID as u16
    }

    /// `as<T>()`.
    pub fn as_op<T: DecodeOp>(&self) -> T {
        debug_assert!(self.is::<T>());
        T::decode(self.bytes)
    }

    /// `Op::m_metadataID`: o último operando da instrução (`OPCODE_LENGTHS` conta o `metadataID`), lido
    /// na largura dela. As structs `Op*` do porte não carregam o campo (ver `bytecode_ops_decode.rs`),
    /// então quem precisa do `bytecode.metadata(codeBlock)` lê o id daqui.
    pub fn metadata_id(&self) -> u32 {
        debug_assert!(self.has_metadata());
        let size = self.width();
        let first_operand = size.padding() as usize + self.opcode_id_bytes() as usize;
        let index = OPCODE_LENGTHS[self.opcode_id() as usize] as usize - 1;
        crate::bytecode::bytecode_ops_decode::read_operand(&self.bytes[first_operand..], index, size)
    }

    /// `asKnownWidth<T, width>()`: o operando começa depois do opcode (e do prefixo, se wide).
    pub fn as_known_width<T: DecodeOp>(&self, width: OpcodeSize) -> T {
        debug_assert!(self.is::<T>());
        let first_operand = if width == OpcodeSize::Narrow { 1 } else { 2 };
        T::from_operands(&self.bytes[first_operand..], width)
    }
}

/// `InstructionStream<InstructionType>::BaseRef`: o buffer mais o índice da instrução.
#[derive(Clone, Debug)]
struct BaseRef {
    bytes: Bytes,
    index: u32,
}

impl BaseRef {
    /// `operator==`: o mesmo buffer e o mesmo índice.
    fn same(&self, other: &BaseRef) -> bool {
        Rc::ptr_eq(&self.bytes, &other.bytes) && self.index == other.index
    }

    /// `operator->`/`ptr()`, por fechamento.
    fn with_instruction<R>(&self, f: impl FnOnce(JSInstruction<'_>) -> R) -> R {
        let bytes = self.bytes.borrow();
        f(JSInstruction::new(&bytes[self.index as usize..]))
    }

    /// `isValid()`.
    fn is_valid(&self) -> bool {
        (self.index as usize) < self.bytes.borrow().len()
    }

    /// `next()`: a instrução seguinte.
    fn next(&self) -> BaseRef {
        let size = self.with_instruction(|instruction| instruction.size());
        BaseRef { bytes: self.bytes.clone(), index: self.index + size as u32 }
    }
}

/// Os acessores que `Ref` e `MutableRef` herdam da `BaseRef`.
macro_rules! base_ref_accessors {
    () => {
        /// `offset()`.
        pub fn offset(&self) -> u32 {
            self.base.index
        }

        /// `isValid()`.
        pub fn is_valid(&self) -> bool {
            self.base.is_valid()
        }

        /// `operator->`/`ptr()`: roda `f` sobre a instrução apontada.
        pub fn with_instruction<R>(&self, f: impl FnOnce(JSInstruction<'_>) -> R) -> R {
            self.base.with_instruction(f)
        }

        /// `ptr()->opcodeID()`.
        pub fn opcode_id(&self) -> u16 {
            self.base.with_instruction(|instruction| instruction.opcode_id())
        }

        /// `ptr()->opcodeID()` como o enum `OpcodeID`.
        pub fn opcode_id_enum(&self) -> OpcodeID {
            self.base.with_instruction(|instruction| instruction.opcode_id_enum())
        }

        /// `m_metadataID` da instrução.
        pub fn metadata_id(&self) -> u32 {
            self.base.with_instruction(|instruction| instruction.metadata_id())
        }

        /// `ptr()->size()`.
        pub fn size(&self) -> usize {
            self.base.with_instruction(|instruction| instruction.size())
        }

        /// `ptr()->numberOfCheckpoints()`.
        pub fn number_of_checkpoints(&self) -> u32 {
            self.base.with_instruction(|instruction| instruction.number_of_checkpoints())
        }

        /// `ptr()->is<T>()`.
        pub fn is_op<T: BytecodeOp>(&self) -> bool {
            self.base.with_instruction(|instruction| instruction.is::<T>())
        }

        /// `ptr()->as<T>()`.
        pub fn as_op<T: DecodeOp>(&self) -> T {
            self.base.with_instruction(|instruction| instruction.as_op::<T>())
        }
    };
}

/// `[offset] nome`, a linha que o dump do C++ imprime por instrução.
fn fmt_ref(
    formatter: &mut std::fmt::Formatter<'_>,
    label: &str,
    base: &BaseRef,
) -> std::fmt::Result {
    if !base.is_valid() {
        return write!(formatter, "{label}[{}] <fim>", base.index);
    }
    let name = JSOpcodeTraits::opcode_names()[base.with_instruction(|instruction| instruction.opcode_id()) as usize];
    write!(formatter, "{label}[{}] {name}", base.index)
}

/// `InstructionStream::Ref`: referência somente de leitura.
#[derive(Clone)]
pub struct Ref {
    base: BaseRef,
}

impl std::fmt::Debug for Ref {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt_ref(formatter, "Ref", &self.base)
    }
}

impl PartialEq for Ref {
    fn eq(&self, other: &Ref) -> bool {
        self.base.same(&other.base)
    }
}

impl Ref {
    base_ref_accessors!();

    /// `BaseRef::next()`.
    pub fn next(&self) -> Ref {
        Ref { base: self.base.next() }
    }
}

/// `InstructionStream::MutableRef`: referência que também altera os bytes da instrução.
#[derive(Clone)]
pub struct MutableRef {
    base: BaseRef,
}

impl std::fmt::Debug for MutableRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt_ref(formatter, "MutableRef", &self.base)
    }
}

impl PartialEq for MutableRef {
    fn eq(&self, other: &MutableRef) -> bool {
        self.base.same(&other.base)
    }
}

impl MutableRef {
    base_ref_accessors!();

    /// `BaseRef::next()` (o C++ devolve uma `Ref`).
    pub fn next(&self) -> Ref {
        Ref { base: self.base.next() }
    }

    /// `freeze()` (e o `operator Ref()`).
    pub fn freeze(&self) -> Ref {
        Ref { base: self.base.clone() }
    }

    /// `operator->`/`ptr()` não constante: roda `f` sobre os bytes da instrução, do primeiro byte
    /// até o fim do buffer.
    pub fn with_instruction_mut<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        let mut bytes = self.base.bytes.borrow_mut();
        f(&mut bytes[self.base.index as usize..])
    }

    /// `ptr()->cast<T>()`: a instrução como `T`, para os setters que escrevem no fluxo.
    pub fn cast_mut<T: DecodeOp>(&mut self) -> OpMut<T> {
        debug_assert!(self.is_op::<T>());
        OpMut::new(self.clone())
    }
}

/// Percorre as instruções pelo tamanho de cada uma (`iterator` do fluxo e do escritor). O fim é o
/// tamanho do buffer no momento em que o iterador é criado.
pub struct InstructionIterator {
    base: BaseRef,
    end: u32,
}

impl InstructionIterator {
    fn new(bytes: &Bytes) -> Self {
        let end = bytes.borrow().len() as u32;
        InstructionIterator { base: BaseRef { bytes: bytes.clone(), index: 0 }, end }
    }
}

impl Iterator for InstructionIterator {
    type Item = MutableRef;

    /// `operator*` seguido de `operator++`: avança `ptr()->size()`.
    fn next(&mut self) -> Option<MutableRef> {
        if self.base.index >= self.end {
            return None;
        }
        let current = MutableRef { base: self.base.clone() };
        self.base = self.base.next();
        Some(current)
    }
}

/// `InstructionStream<JSInstruction>`.
#[derive(Default)]
pub struct InstructionStream {
    bytes: Bytes,
}

/// Dump simples: um `[offset] nome` por instrução.
impl std::fmt::Debug for InstructionStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(formatter, "InstructionStream ({} bytes)", self.size_in_bytes())?;
        for instruction in self.iter() {
            fmt_ref(formatter, "", &instruction.base)?;
            writeln!(formatter)?;
        }
        Ok(())
    }
}

pub type JSInstructionStream = InstructionStream;
pub type JSInstructionStreamMutableRef = MutableRef;

impl InstructionStream {
    /// `explicit InstructionStream(InstructionBuffer&&)`.
    pub fn new(instructions: Vec<u8>) -> Self {
        InstructionStream { bytes: Rc::new(RefCell::new(instructions)) }
    }

    /// `sizeInBytes()`.
    pub fn size_in_bytes(&self) -> usize {
        self.bytes.borrow().len()
    }

    /// `at(Offset)`.
    pub fn at(&self, offset: u32) -> Ref {
        debug_assert!((offset as usize) < self.size_in_bytes());
        Ref { base: BaseRef { bytes: self.bytes.clone(), index: offset } }
    }

    /// `begin()`/`end()` como um iterador.
    pub fn iter(&self) -> InstructionIterator {
        InstructionIterator::new(&self.bytes)
    }
}

/// `InstructionStreamWriter<JSInstruction>`. O `is-a` do C++ vira `Deref` para o fluxo.
#[derive(Debug, Default)]
pub struct InstructionStreamWriter {
    stream: InstructionStream,
    position: u32,
    finalized: bool,
}

pub type JSInstructionStreamWriter = InstructionStreamWriter;

impl std::ops::Deref for InstructionStreamWriter {
    type Target = InstructionStream;

    fn deref(&self) -> &InstructionStream {
        &self.stream
    }
}

/// O que `write(Args...)` aceita (`std::integral`): grava em ordem nativa (little endian no
/// Linux x86_64), como `WTF::unalignedStore`.
pub trait Integral: Copy {
    fn store(self, out: &mut Vec<u8>);
}

macro_rules! impl_integral {
    ($($t:ty),*) => {
        $(impl Integral for $t {
            fn store(self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_ne_bytes());
            }
        })*
    };
}

impl_integral!(u8, i8, u16, i16, u32, i32, u64, i64);

impl InstructionStreamWriter {
    /// `setInstructionBuffer(InstructionBuffer&&)`: só vale com o escritor e o buffer vazios.
    pub fn set_instruction_buffer(&mut self, buffer: Vec<u8>) {
        assert!(self.stream.bytes.borrow().is_empty());
        assert!(buffer.is_empty());
        *self.stream.bytes.borrow_mut() = buffer;
    }

    /// `ref(Offset)`.
    pub fn ref_at(&self, offset: u32) -> MutableRef {
        debug_assert!((offset as usize) < self.stream.bytes.borrow().len());
        MutableRef { base: BaseRef { bytes: self.stream.bytes.clone(), index: offset } }
    }

    /// `ref()`: a referência para a posição atual.
    pub fn ref_(&self) -> MutableRef {
        MutableRef { base: BaseRef { bytes: self.stream.bytes.clone(), index: self.position } }
    }

    /// `seek(unsigned)`.
    pub fn seek(&mut self, position: u32) {
        debug_assert!(position as usize <= self.stream.bytes.borrow().len());
        self.position = position;
    }

    /// `position()`.
    pub fn position(&self) -> u32 {
        self.position
    }

    /// `reserve<size>()`: abre `size` bytes na posição atual (crescendo o buffer se preciso),
    /// avança a posição e devolve o índice do primeiro byte reservado.
    fn reserve(&mut self, size: usize) -> usize {
        debug_assert!(!self.finalized);
        let start = self.position as usize;
        let mut bytes = self.stream.bytes.borrow_mut();
        if start + size > bytes.len() {
            bytes.resize(start + size, 0);
        }
        self.position += size as u32;
        start
    }

    /// `write(args...)`, um valor por vez.
    pub fn write<T: Integral>(&mut self, value: T) {
        let mut encoded = Vec::with_capacity(std::mem::size_of::<T>());
        value.store(&mut encoded);
        let start = self.reserve(encoded.len());
        self.stream.bytes.borrow_mut()[start..start + encoded.len()].copy_from_slice(&encoded);
    }

    /// `rewind(MutableRef&)`: descarta a instrução apontada e tudo depois dela.
    pub fn rewind(&mut self, reference: &MutableRef) {
        let offset = reference.offset();
        debug_assert!((offset as usize) < self.stream.bytes.borrow().len());
        self.stream.bytes.borrow_mut().truncate(offset as usize);
        self.position = offset;
    }

    /// `finalize()`: entrega o buffer ao fluxo imutável e deixa o escritor vazio.
    pub fn finalize(&mut self) -> Box<InstructionStream> {
        self.finalized = true;
        let mut instructions = std::mem::take(&mut *self.stream.bytes.borrow_mut());
        instructions.shrink_to_fit();
        Box::new(InstructionStream::new(instructions))
    }

    /// `finalize(InstructionBuffer& usedBuffer)`: o fluxo recebe uma cópia, e o buffer do escritor
    /// vai para `used_buffer`.
    pub fn finalize_with(&mut self, used_buffer: &mut Vec<u8>) -> Box<InstructionStream> {
        self.finalized = true;
        let result = Box::new(InstructionStream::new(self.stream.bytes.borrow().clone()));
        *used_buffer = std::mem::take(&mut *self.stream.bytes.borrow_mut());
        result
    }

    /// `m_instructions` (o `friend class BytecodeRewriter` do C++): uma cópia dos bytes escritos.
    pub fn instruction_bytes(&self) -> Vec<u8> {
        self.stream.bytes.borrow().clone()
    }

    /// `m_instructions.removeAt(offset, length)`, usado pelo `BytecodeRewriter`.
    pub fn remove_at(&mut self, offset: u32, length: usize) {
        let offset = offset as usize;
        self.stream.bytes.borrow_mut().drain(offset..offset + length);
    }

    /// `m_instructions.insertVector(offset, other)`, usado pelo `BytecodeRewriter`.
    pub fn insert_vector(&mut self, offset: u32, other: &[u8]) {
        let offset = offset as usize;
        self.stream.bytes.borrow_mut().splice(offset..offset, other.iter().copied());
    }

    /// `didMutateBuffer()`: o C++ reaponta o `span` para o buffer; aqui o `Rc` compartilhado já
    /// enxerga o buffer atual, então não há o que atualizar.
    pub fn did_mutate_buffer(&mut self) {}

    /// `swap(InstructionStreamWriter&)`: troca estado e conteúdo dos buffers (as refs continuam
    /// apontando para o `Rc` de cada escritor, como o `span` é atualizado no C++).
    pub fn swap(&mut self, other: &mut InstructionStreamWriter) {
        std::mem::swap(&mut self.finalized, &mut other.finalized);
        std::mem::swap(&mut self.position, &mut other.position);
        std::mem::swap(&mut *self.stream.bytes.borrow_mut(), &mut *other.stream.bytes.borrow_mut());
    }
}
