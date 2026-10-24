//! Porte de `bytecode/BytecodeUseDef.h` e `BytecodeUseDef.cpp`.
//!
//! `computeUsesForBytecodeIndex` e `computeDefsForBytecodeIndex` chamam o `functor` com cada
//! `VirtualRegister` que a instrução lê (uso) ou escreve (definição) no checkpoint pedido. A tabela
//! por opcode é escrita à mão no C++ (não sai do `BytecodeList.rb`), e aqui segue a mesma ordem e os
//! mesmos agrupamentos do `.cpp`.
//!
//! Diferenças deliberadas em relação ao C++:
//! - O `template<typename Block>` vira o trait `UseDefCodeBlock` (`wasCompiledWithDebuggingOpcodes`,
//!   `scopeRegister`, `numVars`); o `const JSInstruction*` vira `UseDefInstruction`, que aceita
//!   tanto o `JSInstruction` quanto o `Ref` do fluxo.
//! - O `ScopedLambda<void(VirtualRegister)>` vira `&mut dyn FnMut(VirtualRegister)`.
//! - Os lambdas auxiliares do `.cpp` (`handleNewArrayLike`, `handleOpCallLike`, `useAtEachCheckpoint`,
//!   `useAtEachCheckpointStartingWith`, `useAt`, `defAt`) são funções livres, porque em Rust um
//!   fechamento por auxiliar não pode emprestar o mesmo `functor` mutável ao mesmo tempo.
//! - `USES(Op, campos...)`/`DEFS(Op, campos...)` viram o macro `use_def_match!`: um braço por op,
//!   com o `static_assert` de que o op não tem checkpoints.
//! - `FOR_EACH_LLINT_OPCODE_EXTENSION` (opcodes auxiliares do LLInt) não existem no `OpcodeID` do
//!   porte: nunca aparecem num fluxo de bytecode.

use crate::bytecode::bytecode_index::Checkpoint;
use crate::bytecode::bytecode_operands_for_checkpoint::resume_value_operand_for;
use crate::bytecode::bytecode_ops::{
    OpAsyncIteratorNext, OpAsyncIteratorOpen, OpCallDirectEval, OpCallVarargs, OpConstruct, OpConstructVarargs,
    OpInstanceof, OpIteratorNext, OpIteratorOpen, OpNewArray, OpNewArrayWithSpread, OpStrcat, OpSuperConstruct,
    OpSuperConstructVarargs, OpTailCallVarargs,
};
use crate::bytecode::instruction_stream::{JSInstruction, Ref};
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::unlinked_code_block_generator::UnlinkedCodeBlockGenerator;
use crate::bytecode::virtual_register::{call_frame_slot, virtual_register_for_local, VirtualRegister};

/// O que `computeUsesForBytecodeIndex` e `computeDefsForBytecodeIndex` pedem do `Block`
/// (`UnlinkedCodeBlock`, `CodeBlock` ou `UnlinkedCodeBlockGenerator`).
pub trait UseDefCodeBlock {
    /// `wasCompiledWithDebuggingOpcodes()`.
    fn was_compiled_with_debugging_opcodes(&self) -> bool;
    /// `scopeRegister()`.
    fn scope_register(&self) -> VirtualRegister;
    /// `numVars()`.
    fn num_vars(&self) -> u32;
}

impl UseDefCodeBlock for UnlinkedCodeBlockGenerator {
    fn was_compiled_with_debugging_opcodes(&self) -> bool {
        UnlinkedCodeBlockGenerator::was_compiled_with_debugging_opcodes(self)
    }

    fn scope_register(&self) -> VirtualRegister {
        UnlinkedCodeBlockGenerator::scope_register(self)
    }

    fn num_vars(&self) -> u32 {
        self.code_block().borrow().num_vars()
    }
}

/// O `const JSInstruction*` dos dois templates: a instrução apontada, por fechamento.
pub trait UseDefInstruction {
    fn with_js_instruction<R>(&self, f: impl FnOnce(JSInstruction<'_>) -> R) -> R;
}

impl UseDefInstruction for JSInstruction<'_> {
    fn with_js_instruction<R>(&self, f: impl FnOnce(JSInstruction<'_>) -> R) -> R {
        f(*self)
    }
}

impl UseDefInstruction for Ref {
    fn with_js_instruction<R>(&self, f: impl FnOnce(JSInstruction<'_>) -> R) -> R {
        self.with_instruction(f)
    }
}

/// `computeUsesForBytecodeIndex(Block*, const JSInstruction*, Checkpoint, const Functor&)`.
pub fn compute_uses_for_bytecode_index<B: UseDefCodeBlock>(
    code_block: &B,
    instruction: &impl UseDefInstruction,
    checkpoint: impl Into<u32>,
    mut functor: impl FnMut(VirtualRegister),
) {
    let checkpoint = checkpoint.into() as Checkpoint;
    instruction.with_js_instruction(|instruction| {
        let opcode_id = instruction.opcode_id_enum();
        if opcode_id != OpcodeID::op_enter
            && code_block.was_compiled_with_debugging_opcodes()
            && code_block.scope_register().is_valid()
        {
            functor(code_block.scope_register());
        }

        compute_uses_for_bytecode_index_impl(&instruction, checkpoint, &mut functor);
    });
}

/// `computeDefsForBytecodeIndex(Block*, const JSInstruction*, Checkpoint, const Functor&)`.
pub fn compute_defs_for_bytecode_index<B: UseDefCodeBlock>(
    code_block: &B,
    instruction: &impl UseDefInstruction,
    checkpoint: impl Into<u32>,
    mut functor: impl FnMut(VirtualRegister),
) {
    let checkpoint = checkpoint.into() as Checkpoint;
    instruction.with_js_instruction(|instruction| {
        compute_defs_for_bytecode_index_impl(code_block.num_vars(), &instruction, checkpoint, &mut functor);
    });
}

/// Um braço por op sem checkpoints (`USES`/`DEFS` do `.cpp`): chama o `functor` com cada campo, na
/// ordem dada, e sai. Os ops sem registradores vão em `none`, e o que tem lógica própria em `rest`
/// (os braços de `rest` são o resto do `match`, como os `case` escritos à mão no C++).
macro_rules! use_def_match {
    (
        $opcode_id:expr, $instruction:ident, $functor:ident,
        none: [$($none:ident),+ $(,)?],
        simple: [$($op:ident($($field:ident),+)),* $(,)?],
        rest: { $($rest:tt)* }
    ) => {
        match $opcode_id {
            $(OpcodeID::$none)|+ => return,
            $(
                <$crate::bytecode::bytecode_ops::$op as $crate::bytecode::bytecode_ops::BytecodeOp>::OPCODE_ID => {
                    const _: () = assert!(
                        <$crate::bytecode::bytecode_ops::$op as $crate::bytecode::bytecode_ops::BytecodeOp>::OPCODE_ID as u16
                            >= $crate::bytecode::instruction_stream::NUMBER_OF_BYTECODE_WITH_CHECKPOINTS,
                        "Don't use this macro for bytecodes that have checkpoints."
                    );
                    let bytecode = $instruction.as_op::<$crate::bytecode::bytecode_ops::$op>();
                    $($functor(bytecode.$field);)+
                    return;
                }
            )*
            $($rest)*
        }
    };
}

/// `handleNewArrayLike`: `argc` registradores a partir de `argv`, descendo.
fn handle_new_array_like(argv: VirtualRegister, argc: u32, functor: &mut dyn FnMut(VirtualRegister)) {
    let base = argv.offset();
    for i in 0..argc as i32 {
        functor(VirtualRegister::new(base.wrapping_sub(i)));
    }
}

/// `handleOpCallLike`: o callee e os `argc` argumentos (contando o `this`) a partir de `argv`.
fn handle_op_call_like(callee: VirtualRegister, argc: u32, argv: u32, functor: &mut dyn FnMut(VirtualRegister)) {
    functor(callee);
    let last_arg = (argv as i32).wrapping_neg().wrapping_add(call_frame_slot::THIS_ARGUMENT);
    for i in 0..argc as i32 {
        functor(VirtualRegister::new(last_arg.wrapping_add(i)));
    }
}

/// `useAtEachCheckpoint`.
fn use_at_each_checkpoint(virtual_registers: &[VirtualRegister], functor: &mut dyn FnMut(VirtualRegister)) {
    for virtual_register in virtual_registers {
        functor(*virtual_register);
    }
}

/// `useAtEachCheckpointStartingWith`.
fn use_at_each_checkpoint_starting_with(
    checkpoint: Checkpoint,
    first_use: Checkpoint,
    virtual_registers: &[VirtualRegister],
    functor: &mut dyn FnMut(VirtualRegister),
) {
    if checkpoint >= first_use {
        use_at_each_checkpoint(virtual_registers, functor);
    }
}

/// `useAt` e `defAt`: só no checkpoint `target`.
fn use_at(
    checkpoint: Checkpoint,
    target: Checkpoint,
    virtual_registers: &[VirtualRegister],
    functor: &mut dyn FnMut(VirtualRegister),
) {
    if target == checkpoint {
        use_at_each_checkpoint(virtual_registers, functor);
    }
}

/// `computeUsesForBytecodeIndexImpl`.
pub fn compute_uses_for_bytecode_index_impl(
    instruction: &JSInstruction<'_>,
    checkpoint: Checkpoint,
    functor: &mut dyn FnMut(VirtualRegister),
) {
    use_def_match! {
        instruction.opcode_id_enum(), instruction, functor,
        // No uses.
        none: [
            op_new_reg_exp, op_loop_hint, op_jmp, op_new_object, op_new_promise, op_new_generator,
            op_new_async_function_generator, op_enter, op_argument_count, op_catch, op_profile_control_flow,
            op_create_direct_arguments, op_create_cloned_arguments, op_create_rest, op_check_traps,
            op_get_argument, op_nop, op_unreachable, op_super_sampler_begin, op_super_sampler_end, op_get_scope,
        ],
        simple: [
            OpToThis(src_dst),
            OpCheckTdz(target_virtual_register),
            OpIdentityWithProfile(src_dst),
            OpProfileType(target_virtual_register),
            OpThrow(value),
            OpThrowStaticError(message),
            OpDebug(data),
            OpRet(value),
            OpJtrue(condition),
            OpJfalse(condition),
            OpJeqNull(value),
            OpJneqNull(value),
            OpJundefinedOrNull(value),
            OpJnundefinedOrNull(value),
            OpDec(src_dst),
            OpInc(src_dst),
            OpLogShadowChickenPrologue(scope),

            OpJless(lhs, rhs),
            OpJlesseq(lhs, rhs),
            OpJgreater(lhs, rhs),
            OpJgreatereq(lhs, rhs),
            OpJnless(lhs, rhs),
            OpJnlesseq(lhs, rhs),
            OpJngreater(lhs, rhs),
            OpJngreatereq(lhs, rhs),
            OpJeq(lhs, rhs),
            OpJneq(lhs, rhs),
            OpJstricteq(lhs, rhs),
            OpJnstricteq(lhs, rhs),
            OpJbelow(lhs, rhs),
            OpJbeloweq(lhs, rhs),
            OpJeqPtr(value, special_pointer),
            OpJneqPtr(value, special_pointer),

            OpSetFunctionName(function, name),
            OpLogShadowChickenTail(this_value, scope),

            OpPutByVal(base, property, value),
            OpPutByValDirect(base, property, value),

            OpPutById(base, value),
            OpPutToScope(scope, value),
            OpPutToArguments(arguments, value),

            OpPutByIdWithThis(base, this_value, value),

            OpPutByValWithThis(base, this_value, property, value),

            OpPutGetterById(base, accessor),
            OpPutSetterById(base, accessor),

            OpPutGetterSetterById(base, getter, setter),

            OpPutGetterByVal(base, property, accessor),
            OpPutSetterByVal(base, property, accessor),

            OpDefineDataProperty(base, property, value, attributes),

            OpDefineAccessorProperty(base, property, getter, setter, attributes),

            OpSpread(argument),
            OpGetPropertyEnumerator(base),
            OpNewFuncExp(scope),
            OpNewGeneratorFuncExp(scope),
            OpNewAsyncFuncExp(scope),
            OpCreateLexicalEnvironment(scope, symbol_table, initial_value),
            OpCreateGeneratorFrameEnvironment(scope, symbol_table, initial_value),
            OpResolveScope(scope),
            OpResolveScopeForHoistingFuncDeclInEval(scope),
            OpGetFromScope(scope),
            OpToPrimitive(src),
            OpToPropertyKey(src),
            OpToPropertyKeyOrNumber(src),
            OpGetById(base),
            OpGetLength(base),
            OpGetByIdDirect(base),
            OpGetPrototypeOf(value),
            OpInById(base),
            OpTypeof(value),
            OpIsEmpty(operand),
            OpTypeofIsUndefined(operand),
            OpTypeofIsObject(operand),
            OpTypeofIsFunction(operand),
            OpIsUndefinedOrNull(operand),
            OpIsBoolean(operand),
            OpIsNumber(operand),
            OpIsBigInt(operand),
            OpIsObject(operand),
            OpIsCellWithType(operand),
            OpIsCallable(operand),
            OpIsConstructor(operand),
            OpToNumber(operand),
            OpToNumeric(operand),
            OpToString(operand),
            OpToObject(operand),
            OpNegate(operand),
            OpBitnot(operand),
            OpEqNull(operand),
            OpNeqNull(operand),
            OpNot(operand),
            OpUnsigned(operand),
            OpMov(src),
            OpNewArrayWithSize(length),
            OpNewArrayWithSpecies(length, array),
            OpCreateThis(callee),
            OpCreatePromise(callee),
            OpCreateGenerator(callee),
            OpCreateAsyncGenerator(callee),
            OpDelById(base),
            OpNewFunc(scope),
            OpNewAsyncGeneratorFunc(scope),
            OpNewAsyncGeneratorFuncExp(scope),
            OpNewGeneratorFunc(scope),
            OpNewAsyncFunc(scope),
            OpGetParentScope(scope),
            OpCreateScopedArguments(scope),
            OpGetFromArguments(arguments),
            OpNewArrayBuffer(immutable_butterfly),

            OpGetByVal(base, property),
            OpGetPrivateName(base, property),
            OpPutPrivateName(base, property, value),
            OpSetPrivateBrand(base, brand),
            OpCheckPrivateBrand(base, brand),
            OpInByVal(base, property),
            OpHasPrivateName(base, property),
            OpHasPrivateBrand(base, brand),
            OpHasStructureWithFlags(operand),
            OpAdd(lhs, rhs),
            OpMul(lhs, rhs),
            OpDiv(lhs, rhs),
            OpMod(lhs, rhs),
            OpSub(lhs, rhs),
            OpPow(lhs, rhs),
            OpLshift(lhs, rhs),
            OpRshift(lhs, rhs),
            OpUrshift(lhs, rhs),
            OpBitand(lhs, rhs),
            OpBitxor(lhs, rhs),
            OpBitor(lhs, rhs),
            OpLess(lhs, rhs),
            OpLesseq(lhs, rhs),
            OpGreater(lhs, rhs),
            OpGreatereq(lhs, rhs),
            OpBelow(lhs, rhs),
            OpBeloweq(lhs, rhs),
            OpNstricteq(lhs, rhs),
            OpStricteq(lhs, rhs),
            OpNeq(lhs, rhs),
            OpEq(lhs, rhs),
            OpPushWithScope(current_scope, new_scope),
            OpGetByIdWithThis(base, this_value),
            OpDelByVal(base, property),

            OpGetByValWithThis(base, this_value, property),

            OpSwitchString(scrutinee),
            OpSwitchChar(scrutinee),
            OpSwitchImm(scrutinee),

            OpGetInternalField(base),
            OpPutInternalField(base, value),

            OpYield(argument),

            OpEnumeratorNext(mode, index, base, enumerator),
            OpEnumeratorGetByVal(base, mode, property_name, index, enumerator),
            OpEnumeratorInByVal(base, mode, property_name, index, enumerator),
            OpEnumeratorPutByVal(base, mode, property_name, index, enumerator, value),
            OpEnumeratorHasOwnProperty(base, mode, property_name, index, enumerator),
        ],
        rest: {
            OpcodeID::op_wide16 | OpcodeID::op_wide32 => unreachable!("RELEASE_ASSERT_NOT_REACHED"),

            OpcodeID::op_call_varargs => {
                let bytecode = instruction.as_op::<OpCallVarargs>();
                use_at_each_checkpoint(&[bytecode.callee, bytecode.this_value, bytecode.arguments], functor);
            }
            OpcodeID::op_tail_call_varargs => {
                let bytecode = instruction.as_op::<OpTailCallVarargs>();
                use_at_each_checkpoint(&[bytecode.callee, bytecode.this_value, bytecode.arguments], functor);
            }
            OpcodeID::op_construct_varargs => {
                let bytecode = instruction.as_op::<OpConstructVarargs>();
                use_at_each_checkpoint(&[bytecode.callee, bytecode.this_value, bytecode.arguments], functor);
            }
            OpcodeID::op_super_construct_varargs => {
                let bytecode = instruction.as_op::<OpSuperConstructVarargs>();
                use_at_each_checkpoint(&[bytecode.callee, bytecode.this_value, bytecode.arguments], functor);
            }

            OpcodeID::op_iterator_open => {
                let bytecode = instruction.as_op::<OpIteratorOpen>();
                use_at_each_checkpoint_starting_with(
                    checkpoint,
                    OpIteratorOpen::SYMBOL_CALL,
                    &[bytecode.symbol_iterator, bytecode.iterable],
                    functor,
                );
                use_at_each_checkpoint_starting_with(
                    checkpoint,
                    OpIteratorOpen::GET_NEXT,
                    &[bytecode.iterator],
                    functor,
                );
            }

            OpcodeID::op_async_iterator_open => {
                let bytecode = instruction.as_op::<OpAsyncIteratorOpen>();
                use_at_each_checkpoint_starting_with(
                    checkpoint,
                    OpAsyncIteratorOpen::SYMBOL_CALL,
                    &[bytecode.symbol_iterator, bytecode.iterable],
                    functor,
                );
                use_at_each_checkpoint_starting_with(
                    checkpoint,
                    OpAsyncIteratorOpen::GET_NEXT,
                    &[bytecode.iterator],
                    functor,
                );
            }

            OpcodeID::op_iterator_next => {
                let bytecode = instruction.as_op::<OpIteratorNext>();
                use_at_each_checkpoint(&[bytecode.iterator, bytecode.next], functor);
                use_at_each_checkpoint_starting_with(
                    checkpoint,
                    OpIteratorNext::COMPUTE_NEXT,
                    &[bytecode.iterable],
                    functor,
                );
            }

            OpcodeID::op_async_iterator_next => {
                let bytecode = instruction.as_op::<OpAsyncIteratorNext>();
                functor(bytecode.next);
                functor(bytecode.iterator);
                functor(bytecode.driver);
                // The resume value isn't a stored field (see BytecodeList.rb); it's call argument index 1,
                // derived from m_stackOffset, and only present when m_hasValue.
                if bytecode.has_value {
                    functor(resume_value_operand_for(&bytecode));
                }
            }

            OpcodeID::op_new_array_with_spread => {
                let bytecode = instruction.as_op::<OpNewArrayWithSpread>();
                handle_new_array_like(bytecode.argv, bytecode.argc, functor);
            }
            OpcodeID::op_new_array => {
                let bytecode = instruction.as_op::<OpNewArray>();
                handle_new_array_like(bytecode.argv, bytecode.argc, functor);
            }

            OpcodeID::op_strcat => {
                let bytecode = instruction.as_op::<OpStrcat>();
                let base = bytecode.src.offset();
                for i in 0..bytecode.count {
                    functor(VirtualRegister::new(base.wrapping_sub(i)));
                }
            }

            OpcodeID::op_construct => {
                let bytecode = instruction.as_op::<OpConstruct>();
                handle_op_call_like(bytecode.callee, bytecode.argc, bytecode.argv, functor);
            }

            OpcodeID::op_super_construct => {
                let bytecode = instruction.as_op::<OpSuperConstruct>();
                handle_op_call_like(bytecode.callee, bytecode.argc, bytecode.argv, functor);
            }

            OpcodeID::op_call_direct_eval => {
                let bytecode = instruction.as_op::<OpCallDirectEval>();
                handle_op_call_like(bytecode.callee, bytecode.argc, bytecode.argv, functor);
                functor(bytecode.this_value);
                functor(bytecode.scope);
            }
            OpcodeID::op_call => {
                let bytecode = instruction.as_op::<crate::bytecode::bytecode_ops::OpCall>();
                handle_op_call_like(bytecode.callee, bytecode.argc, bytecode.argv, functor);
            }
            OpcodeID::op_tail_call => {
                let bytecode = instruction.as_op::<crate::bytecode::bytecode_ops::OpTailCall>();
                handle_op_call_like(bytecode.callee, bytecode.argc, bytecode.argv, functor);
            }
            OpcodeID::op_call_ignore_result => {
                let bytecode = instruction.as_op::<crate::bytecode::bytecode_ops::OpCallIgnoreResult>();
                handle_op_call_like(bytecode.callee, bytecode.argc, bytecode.argv, functor);
            }

            OpcodeID::op_instanceof => {
                let bytecode = instruction.as_op::<OpInstanceof>();
                use_at(checkpoint, OpInstanceof::GET_HAS_INSTANCE, &[bytecode.constructor], functor);
                use_at(
                    checkpoint,
                    OpInstanceof::GET_PROTOTYPE,
                    &[bytecode.value, bytecode.constructor, bytecode.has_instance_or_prototype],
                    functor,
                );
                use_at(
                    checkpoint,
                    OpInstanceof::INSTANCEOF,
                    &[bytecode.value, bytecode.has_instance_or_prototype],
                    functor,
                );
            }
            // `default: RELEASE_ASSERT_NOT_REACHED()` do C++ some: o match cobre todos os opcodes.
        }
    }
}

/// `computeDefsForBytecodeIndexImpl`.
pub fn compute_defs_for_bytecode_index_impl(
    num_vars: u32,
    instruction: &JSInstruction<'_>,
    checkpoint: Checkpoint,
    functor: &mut dyn FnMut(VirtualRegister),
) {
    use_def_match! {
        instruction.opcode_id_enum(), instruction, functor,
        // These don't define anything.
        none: [
            op_put_to_scope, op_throw, op_throw_static_error, op_check_tdz, op_debug, op_ret, op_jmp, op_jtrue,
            op_jfalse, op_jeq_null, op_jneq_null, op_jundefined_or_null, op_jnundefined_or_null, op_jeq_ptr,
            op_jneq_ptr, op_jless, op_jlesseq, op_jgreater, op_jgreatereq, op_jnless, op_jnlesseq, op_jngreater,
            op_jngreatereq, op_jeq, op_jneq, op_jstricteq, op_jnstricteq, op_jbelow, op_jbeloweq, op_loop_hint,
            op_switch_imm, op_switch_char, op_switch_string, op_put_by_id, op_put_by_id_with_this,
            op_put_by_val_with_this, op_put_getter_by_id, op_put_setter_by_id, op_put_getter_setter_by_id,
            op_put_getter_by_val, op_put_setter_by_val, op_put_by_val, op_put_by_val_direct,
            op_enumerator_put_by_val, op_put_private_name, op_set_private_brand, op_check_private_brand,
            op_put_internal_field, op_define_data_property, op_define_accessor_property, op_profile_type,
            op_profile_control_flow, op_put_to_arguments, op_call_ignore_result, op_set_function_name,
            op_check_traps, op_log_shadow_chicken_prologue, op_log_shadow_chicken_tail, op_yield, op_nop,
            op_unreachable, op_super_sampler_begin, op_super_sampler_end,
        ],
        // These all have a single destination for the first argument.
        simple: [
            OpArgumentCount(dst),
            OpGetPropertyEnumerator(dst),
            OpGetParentScope(dst),
            OpPushWithScope(dst),
            OpCreateLexicalEnvironment(dst),
            OpCreateGeneratorFrameEnvironment(dst),
            OpResolveScope(dst),
            OpResolveScopeForHoistingFuncDeclInEval(dst),
            OpStrcat(dst),
            OpToPrimitive(dst),
            OpToPropertyKey(dst),
            OpToPropertyKeyOrNumber(dst),
            OpCreateThis(dst),
            OpCreatePromise(dst),
            OpCreateGenerator(dst),
            OpCreateAsyncGenerator(dst),
            OpNewArray(dst),
            OpNewArrayWithSpread(dst),
            OpSpread(dst),
            OpNewArrayBuffer(dst),
            OpNewArrayWithSize(dst),
            OpNewArrayWithSpecies(dst),
            OpNewRegExp(dst),
            OpNewFunc(dst),
            OpNewFuncExp(dst),
            OpNewGeneratorFunc(dst),
            OpNewGeneratorFuncExp(dst),
            OpNewAsyncGeneratorFunc(dst),
            OpNewAsyncGeneratorFuncExp(dst),
            OpNewAsyncFunc(dst),
            OpNewAsyncFuncExp(dst),

            OpGetFromScope(dst),
            OpCall(dst),
            OpTailCall(dst),
            OpCallDirectEval(dst),
            OpConstruct(dst),
            OpSuperConstruct(dst),
            OpGetById(dst),
            OpAsyncIteratorNext(dst),
            OpGetLength(dst),
            OpGetByIdDirect(dst),
            OpGetByIdWithThis(dst),
            OpGetByValWithThis(dst),
            OpGetPrototypeOf(dst),
            OpGetByVal(dst),
            OpGetPrivateName(dst),
            OpTypeof(dst),
            OpIdentityWithProfile(src_dst),
            OpIsEmpty(dst),
            OpTypeofIsUndefined(dst),
            OpTypeofIsObject(dst),
            OpTypeofIsFunction(dst),
            OpIsUndefinedOrNull(dst),
            OpIsBoolean(dst),
            OpIsNumber(dst),
            OpIsBigInt(dst),
            OpIsObject(dst),
            OpIsCellWithType(dst),
            OpIsCallable(dst),
            OpIsConstructor(dst),
            OpInById(dst),
            OpInByVal(dst),
            OpHasPrivateName(dst),
            OpHasPrivateBrand(dst),
            OpHasStructureWithFlags(dst),
            OpToNumber(dst),
            OpToNumeric(dst),
            OpToString(dst),
            OpToObject(dst),
            OpNegate(dst),
            OpAdd(dst),
            OpMul(dst),
            OpDiv(dst),
            OpMod(dst),
            OpSub(dst),
            OpPow(dst),
            OpLshift(dst),
            OpRshift(dst),
            OpUrshift(dst),
            OpBitand(dst),
            OpBitxor(dst),
            OpBitor(dst),
            OpBitnot(dst),
            OpInc(src_dst),
            OpDec(src_dst),
            OpEq(dst),
            OpNeq(dst),
            OpStricteq(dst),
            OpNstricteq(dst),
            OpLess(dst),
            OpLesseq(dst),
            OpGreater(dst),
            OpGreatereq(dst),
            OpBelow(dst),
            OpBeloweq(dst),
            OpNeqNull(dst),
            OpEqNull(dst),
            OpNot(dst),
            OpMov(dst),
            OpNewObject(dst),
            OpNewPromise(dst),
            OpNewGenerator(dst),
            OpNewAsyncFunctionGenerator(dst),
            OpToThis(src_dst),
            OpGetScope(dst),
            OpCreateDirectArguments(dst),
            OpCreateScopedArguments(dst),
            OpCreateClonedArguments(dst),
            OpDelById(dst),
            OpDelByVal(dst),
            OpUnsigned(dst),
            OpGetFromArguments(dst),
            OpGetArgument(dst),
            OpCreateRest(dst),
            OpGetInternalField(dst),

            OpCatch(exception, thrown_value),

            OpEnumeratorNext(property_name, mode, index),
            OpEnumeratorGetByVal(dst),
            OpEnumeratorInByVal(dst),
            OpEnumeratorHasOwnProperty(dst),
        ],
        rest: {
            OpcodeID::op_wide16 | OpcodeID::op_wide32 => unreachable!("RELEASE_ASSERT_NOT_REACHED"),

            OpcodeID::op_call_varargs => {
                let bytecode = instruction.as_op::<OpCallVarargs>();
                use_at(checkpoint, OpCallVarargs::MAKE_CALL, &[bytecode.dst], functor);
            }
            OpcodeID::op_tail_call_varargs => {
                let bytecode = instruction.as_op::<OpTailCallVarargs>();
                use_at(checkpoint, OpTailCallVarargs::MAKE_CALL, &[bytecode.dst], functor);
            }
            OpcodeID::op_construct_varargs => {
                let bytecode = instruction.as_op::<OpConstructVarargs>();
                use_at(checkpoint, OpConstructVarargs::MAKE_CALL, &[bytecode.dst], functor);
            }
            OpcodeID::op_super_construct_varargs => {
                let bytecode = instruction.as_op::<OpSuperConstructVarargs>();
                use_at(checkpoint, OpSuperConstructVarargs::MAKE_CALL, &[bytecode.dst], functor);
            }

            OpcodeID::op_iterator_open => {
                let bytecode = instruction.as_op::<OpIteratorOpen>();

                use_at(checkpoint, OpIteratorOpen::SYMBOL_CALL, &[bytecode.iterator], functor);
                use_at(checkpoint, OpIteratorOpen::GET_NEXT, &[bytecode.next], functor);
            }

            OpcodeID::op_async_iterator_open => {
                let bytecode = instruction.as_op::<OpAsyncIteratorOpen>();

                use_at(checkpoint, OpAsyncIteratorOpen::SYMBOL_CALL, &[bytecode.iterator], functor);
                use_at(checkpoint, OpAsyncIteratorOpen::GET_NEXT, &[bytecode.next], functor);
            }

            OpcodeID::op_iterator_next => {
                let bytecode = instruction.as_op::<OpIteratorNext>();

                use_at(checkpoint, OpIteratorNext::GET_DONE, &[bytecode.done], functor);
                // We need to claim we set m_value here because we could early exit from the bytecode if we are done.
                use_at(checkpoint, OpIteratorNext::GET_DONE, &[bytecode.value], functor);

                use_at(checkpoint, OpIteratorNext::GET_VALUE, &[bytecode.value], functor);
            }

            OpcodeID::op_enter => {
                for i in (0..num_vars).rev() {
                    functor(virtual_register_for_local(i as i32));
                }
            }

            OpcodeID::op_instanceof => {
                let bytecode = instruction.as_op::<OpInstanceof>();
                use_at(checkpoint, OpInstanceof::GET_HAS_INSTANCE, &[bytecode.has_instance_or_prototype], functor);
                use_at(checkpoint, OpInstanceof::GET_PROTOTYPE, &[bytecode.has_instance_or_prototype], functor);
                use_at(checkpoint, OpInstanceof::INSTANCEOF, &[bytecode.dst], functor);
            }
        }
    }
}
