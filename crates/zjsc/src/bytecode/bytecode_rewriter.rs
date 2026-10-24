//! Porte de `bytecode/BytecodeRewriter.h` e `BytecodeRewriter.cpp`.
//!
//! O `BytecodeRewriter` insere e remove bytecodes, inclusive saltos. Os deslocamentos originais
//! são os rótulos: quando um salto é emitido num fragmento, o alvo é dado pelo deslocamento
//! original, e o rewriter o converte no final. Os fragmentos entram antes (`Before`) ou depois
//! (`After`) de um rótulo, e isso importa para quem salta para ele:
//!
//! ```text
//!                      |  [bytecode] [before]  |  [after] [bytecode]  |
//!   offsets            A                       B              C
//!                                              ^
//!                                              jump to here.
//! ```
//!
//! o salto para "B" não executa o `[before]`.
//!
//! Diferenças de forma em relação ao C++, sem mudar o cálculo:
//! - o C++ guarda `BytecodeGenerator&`, `UnlinkedCodeBlockGenerator*` e o escritor, que são o mesmo
//!   objeto do gerador visto por três ponteiros. Em Rust seguro os três chegam por parâmetro no
//!   ponto de uso: o gerador na criação do fragmento, o bloco e o escritor em `execute`. O `m_graph`
//!   (só tinha o acessor `graph()`, sem chamador) não é guardado.
//! - `appendInstruction<Op>(args...)` recebe o `emit` como fechamento (`Op::emit(generator, ...)`),
//!   porque os argumentos variam por opcode.
//! - `applyModification` e `adjustJumpTargetsInFragment` ficam acessíveis ao
//!   `UnlinkedCodeBlockGenerator` (o `friend class` do C++) como `pub(crate)`.

use crate::bytecode::bytecode_ops::BytecodeOp;
use crate::bytecode::instruction_stream::{InstructionStreamWriter, Ref};
use crate::bytecode::opcode_inlines::is_branch;
use crate::bytecode::precise_jump_targets::{
    update_stored_jump_targets_for_instruction, update_stored_jump_targets_for_instruction_in_block, OutOfLineJumpSource,
};
use crate::bytecode::unlinked_code_block_generator::UnlinkedCodeBlockGenerator;
use crate::bytecompiler::bytecode_generator::BytecodeGenerator;

/// `enum class Position : int8_t`. A ordem de declaração é a ordem numérica.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i8)]
pub enum Position {
    Entrypoint = -2,
    Before = -1,
    LabelPoint = 0,
    After = 1,
    OriginalBytecodePoint = 2,
}

/// `enum class IncludeBranch : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IncludeBranch {
    No = 0,
    Yes = 1,
}

/// `struct InsertionPoint`: `operator<` compara o deslocamento e depois a posição, que é a ordem
/// lexicográfica dos campos.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct InsertionPoint {
    pub bytecode_offset: i32,
    pub position: Position,
}

impl InsertionPoint {
    /// `InsertionPoint(JSInstructionStream::Offset, Position)`.
    pub fn new(offset: u32, position: Position) -> InsertionPoint {
        InsertionPoint { bytecode_offset: offset as i32, position }
    }
}

/// `Insertion::Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InsertionType {
    Insert = 0,
    Remove = 1,
}

/// `struct Insertion`.
struct Insertion {
    index: InsertionPoint,
    type_: InsertionType,
    include_branch: IncludeBranch,
    remove_length: usize,
    instructions: InstructionStreamWriter,
}

impl Insertion {
    /// `length()`.
    fn length(&self) -> usize {
        if self.type_ == InsertionType::Remove {
            return self.remove_length;
        }
        self.instructions.size_in_bytes()
    }
}

/// `class BytecodeRewriter::Fragment`.
pub struct Fragment<'a> {
    generator: &'a mut BytecodeGenerator,
    writer: &'a mut InstructionStreamWriter,
    include_branch: &'a mut IncludeBranch,
}

impl Fragment<'_> {
    /// `m_bytecodeGenerator`: o gerador que o fragmento usa para emitir (o C++ o deixa acessível
    /// ao `Function` pela captura da lambda; aqui o fragmento o empresta).
    pub fn generator(&mut self) -> &mut BytecodeGenerator {
        self.generator
    }

    /// `appendInstruction<Op>(args...)`: `emit` faz `Op::emit(generator, args...)`.
    pub fn append_instruction<Op: BytecodeOp>(&mut self, emit: impl FnOnce(&mut BytecodeGenerator)) {
        if is_branch(Op::OPCODE_ID) {
            *self.include_branch = IncludeBranch::Yes;
        }

        self.generator.with_writer(self.writer, emit);
    }
}

/// `class BytecodeRewriter`.
#[derive(Default)]
pub struct BytecodeRewriter {
    insertions: Vec<Insertion>,
}

impl BytecodeRewriter {
    /// O miolo de `insertFragmentBefore` e `insertFragmentAfter`.
    fn insert_fragment_at(
        &mut self,
        generator: &mut BytecodeGenerator,
        instruction: &Ref,
        position: Position,
        function: impl FnOnce(&mut Fragment<'_>),
    ) {
        let mut include_branch = IncludeBranch::No;
        let mut writer = InstructionStreamWriter::default();
        {
            let mut fragment = Fragment { generator, writer: &mut writer, include_branch: &mut include_branch };
            function(&mut fragment);
        }
        self.insert_impl(InsertionPoint::new(instruction.offset(), position), include_branch, writer);
    }

    /// `insertFragmentBefore(instruction, function)`.
    pub fn insert_fragment_before(
        &mut self,
        generator: &mut BytecodeGenerator,
        instruction: &Ref,
        function: impl FnOnce(&mut Fragment<'_>),
    ) {
        self.insert_fragment_at(generator, instruction, Position::Before, function);
    }

    /// `insertFragmentAfter(instruction, function)`.
    pub fn insert_fragment_after(
        &mut self,
        generator: &mut BytecodeGenerator,
        instruction: &Ref,
        function: impl FnOnce(&mut Fragment<'_>),
    ) {
        self.insert_fragment_at(generator, instruction, Position::After, function);
    }

    /// `replaceBytecodeWithFragment(instruction, function)`.
    pub fn replace_bytecode_with_fragment(
        &mut self,
        generator: &mut BytecodeGenerator,
        instruction: &Ref,
        function: impl FnOnce(&mut Fragment<'_>),
    ) {
        self.insertions.push(Insertion {
            index: InsertionPoint::new(instruction.offset(), Position::OriginalBytecodePoint),
            type_: InsertionType::Remove,
            include_branch: IncludeBranch::No,
            remove_length: instruction.size(),
            instructions: InstructionStreamWriter::default(),
        });
        self.insert_fragment_after(generator, instruction, function);
    }

    /// `execute()`: ordena as inserções (o `bubbleSort` é estável, como o `sort_by`) e pede ao bloco
    /// que aplique a modificação.
    pub fn execute(&mut self, code_block: &mut UnlinkedCodeBlockGenerator, writer: &mut InstructionStreamWriter) {
        self.insertions.sort_by(|lhs, rhs| lhs.index.cmp(&rhs.index));

        code_block.apply_modification(self, writer);
    }

    /// `adjustAbsoluteOffset(absoluteOffset)`.
    pub fn adjust_absolute_offset(&self, absolute_offset: u32) -> i32 {
        self.adjust_jump_target_points(
            InsertionPoint::new(0, Position::Entrypoint),
            InsertionPoint::new(absolute_offset, Position::LabelPoint),
        )
    }

    /// `adjustJumpTarget(JSInstructionStream::Offset originalBytecodeOffset, int32_t originalJumpTarget)`.
    pub fn adjust_jump_target(&self, original_bytecode_offset: u32, original_jump_target: i32) -> i32 {
        self.adjust_jump_target_points(
            InsertionPoint::new(original_bytecode_offset, Position::LabelPoint),
            InsertionPoint { bytecode_offset: original_jump_target, position: Position::LabelPoint },
        )
    }

    // FIXME: unit test the logic in this method
    // https://bugs.webkit.org/show_bug.cgi?id=190950
    /// `adjustJumpTargets()`.
    pub(crate) fn adjust_jump_targets(&self, code_block: &mut UnlinkedCodeBlockGenerator, writer: &mut InstructionStreamWriter) {
        let mut current_insertion = 0usize;
        let out_of_line_jump_targets = code_block.replace_out_of_line_jump_targets();

        let mut offset: i32 = 0;
        let mut i: u32 = 0;
        while (i as usize) < writer.size_in_bytes() {
            let mut before: i32 = 0;
            let mut after: i32 = 0;
            let mut remove: i32 = 0;
            while current_insertion < self.insertions.len()
                && self.insertions[current_insertion].index.bytecode_offset as u32 == i
            {
                let insertion = &self.insertions[current_insertion];
                let size = insertion.length() as i32;
                if insertion.type_ == InsertionType::Remove {
                    remove += size;
                } else if insertion.index.position == Position::Before {
                    before += size;
                } else if insertion.index.position == Position::After {
                    after += size;
                }
                current_insertion += 1;
            }

            offset += before;

            if remove == 0 {
                let mut instruction = writer.ref_at(i);
                let instruction_offset = instruction.offset();
                update_stored_jump_targets_for_instruction(
                    code_block,
                    offset as u32,
                    &mut instruction,
                    |relative_offset| {
                        self.adjust_jump_target(instruction_offset, instruction_offset.wrapping_add(relative_offset as u32) as i32)
                    },
                    &OutOfLineJumpSource::Map(&out_of_line_jump_targets),
                );
                i += instruction.size() as u32;
            } else {
                offset -= remove;
                i += remove as u32;
            }

            offset += after;
        }
    }

    /// `forEachLabelPoint(func)`.
    pub fn for_each_label_point(&self, mut func: impl FnMut(i32)) {
        let mut previous_bytecode_offset: i32 = -1;
        for insertion in &self.insertions {
            let bytecode_offset = insertion.index.bytecode_offset;
            if bytecode_offset == previous_bytecode_offset {
                continue;
            }
            previous_bytecode_offset = bytecode_offset;
            func(bytecode_offset);
        }
    }

    /// `insertImpl(InsertionPoint, IncludeBranch, JSInstructionStreamWriter&&)`.
    fn insert_impl(&mut self, insertion_point: InsertionPoint, include_branch: IncludeBranch, writer: InstructionStreamWriter) {
        debug_assert!(insertion_point.position == Position::Before || insertion_point.position == Position::After);
        self.insertions.push(Insertion {
            index: insertion_point,
            type_: InsertionType::Insert,
            include_branch,
            remove_length: 0,
            instructions: writer,
        });
    }

    /// `applyModification()`.
    pub(crate) fn apply_modification(&mut self, code_block: &mut UnlinkedCodeBlockGenerator, writer: &mut InstructionStreamWriter) {
        for insertion_index in (0..self.insertions.len()).rev() {
            let insertion = &self.insertions[insertion_index];
            if insertion.type_ == InsertionType::Remove {
                writer.remove_at(insertion.index.bytecode_offset as u32, insertion.length());
            } else {
                if insertion.include_branch == IncludeBranch::Yes {
                    let final_offset =
                        insertion.index.bytecode_offset + Self::calculate_difference(&self.insertions[..insertion_index]);
                    self.adjust_jump_targets_in_fragment(code_block, final_offset as u32, insertion_index);
                }
                let insertion = &self.insertions[insertion_index];
                writer.insert_vector(insertion.index.bytecode_offset as u32, &insertion.instructions.instruction_bytes());
            }
        }
        writer.did_mutate_buffer();
        self.insertions.clear();
    }

    /// `adjustJumpTargetsInFragment(finalOffset, insertion)`: a inserção vem pelo índice.
    fn adjust_jump_targets_in_fragment(&self, code_block: &mut UnlinkedCodeBlockGenerator, final_offset: u32, insertion_index: usize) {
        for mut instruction in self.insertions[insertion_index].instructions.iter() {
            if is_branch(instruction.opcode_id_enum()) {
                let bytecode_offset = final_offset.wrapping_add(instruction.offset());
                update_stored_jump_targets_for_instruction_in_block(code_block, final_offset, &mut instruction, |label| {
                    let absolute_offset = self.adjust_absolute_offset(label as u32);
                    absolute_offset - bytecode_offset as i32
                });
            }
        }
    }

    /// `adjustJumpTarget(InsertionPoint startPoint, InsertionPoint jumpTargetPoint)`.
    fn adjust_jump_target_points(&self, start_point: InsertionPoint, jump_target_point: InsertionPoint) -> i32 {
        if start_point < jump_target_point {
            let mut jump_target = jump_target_point.bytecode_offset;
            let start = self.insertions.partition_point(|insertion| insertion.index < start_point);
            if start != self.insertions.len() {
                let end = self.insertions.partition_point(|insertion| insertion.index < jump_target_point);
                jump_target += Self::calculate_difference(&self.insertions[start..end]);
            }
            return jump_target - start_point.bytecode_offset;
        }

        if start_point == jump_target_point {
            return 0;
        }

        -self.adjust_jump_target_points(jump_target_point, start_point)
    }

    /// `calculateDifference(begin, end)`.
    fn calculate_difference(insertions: &[Insertion]) -> i32 {
        let mut result: i32 = 0;
        for insertion in insertions {
            if insertion.type_ == InsertionType::Remove {
                result -= insertion.length() as i32;
            } else {
                result += insertion.length() as i32;
            }
        }
        result
    }
}
