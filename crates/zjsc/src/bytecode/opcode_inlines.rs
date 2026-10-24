//! Parte escrita à mão de `bytecode/Opcode.h`: `isBranch`, `isUnconditionalBranch`, `isTerminal` e
//! `isThrow`. O `opcode.rs` é gerado por `scripts/gen-opcodes.py` a partir de `Bytecodes.h` e não
//! recebe código manual, por isso estas funções vivem neste módulo.

use crate::bytecode::opcode::OpcodeID;

/// `isBranch(OpcodeID)`.
pub fn is_branch(opcode_id: OpcodeID) -> bool {
    matches!(
        opcode_id,
        OpcodeID::op_jmp
            | OpcodeID::op_jtrue
            | OpcodeID::op_jfalse
            | OpcodeID::op_jeq_null
            | OpcodeID::op_jneq_null
            | OpcodeID::op_jundefined_or_null
            | OpcodeID::op_jnundefined_or_null
            | OpcodeID::op_jeq_ptr
            | OpcodeID::op_jneq_ptr
            | OpcodeID::op_jless
            | OpcodeID::op_jlesseq
            | OpcodeID::op_jgreater
            | OpcodeID::op_jgreatereq
            | OpcodeID::op_jnless
            | OpcodeID::op_jnlesseq
            | OpcodeID::op_jngreater
            | OpcodeID::op_jngreatereq
            | OpcodeID::op_jeq
            | OpcodeID::op_jneq
            | OpcodeID::op_jstricteq
            | OpcodeID::op_jnstricteq
            | OpcodeID::op_jbelow
            | OpcodeID::op_jbeloweq
            | OpcodeID::op_switch_imm
            | OpcodeID::op_switch_char
            | OpcodeID::op_switch_string
    )
}

/// `isUnconditionalBranch(OpcodeID)`.
pub fn is_unconditional_branch(opcode_id: OpcodeID) -> bool {
    matches!(opcode_id, OpcodeID::op_jmp)
}

/// `isTerminal(OpcodeID)`.
pub fn is_terminal(opcode_id: OpcodeID) -> bool {
    matches!(opcode_id, OpcodeID::op_ret | OpcodeID::op_unreachable)
}

/// `isThrow(OpcodeID)`.
pub fn is_throw(opcode_id: OpcodeID) -> bool {
    matches!(opcode_id, OpcodeID::op_throw | OpcodeID::op_throw_static_error)
}
