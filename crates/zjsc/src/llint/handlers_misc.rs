//! Handlers do laço que não tinham braço: comparações e desvios por comparação, predicados de célula,
//! conversões (`to_primitive`, `to_property_key`, `to_property_key_or_number`, `to_object`), acesso ao frame
//! (`argument_count`, `get_argument`), `check_tdz`, `jeq_ptr`/`jneq_ptr`, `strcat` e os opcodes de
//! instrumentação (`super_sampler_*`, `identity_with_profile`, `profile_*`, `log_shadow_chicken_*`, `debug`).
//!
//! O ponto de entrada é [`run_misc`], no mesmo formato de `dispatch_ext::run_ext`: `None` é o opcode que não
//! é daqui. Os handlers dos outros dois arquivos desta fatia estão em `handlers_object` e `handlers_scope`.
//!
//! DIVERGÊNCIAS do `LowLevelInterpreter*.asm` e do `CommonSlowPaths.cpp`:
//!
//! - `op_eq`, `op_less` e as demais comparações vão sempre pelo slow path (`slow_path_eq`, `slow_path_less`...),
//!   que tem a semântica completa; o caminho rápido de `int32` do `.asm` dá o mesmo resultado. Cada uma é
//!   seguida de `LLINT_CHECK_EXCEPTION`, porque a conversão de objeto (`valueOf`) pode lançar.
//! - `op_below`, `op_beloweq`, `op_jbelow` e `op_jbeloweq` comparam as palavras baixas de 32 bits sem sinal
//!   (`cib`, `cibeq`, `bib`, `bibeq`); só o gerador os emite, sobre resultado de `urshift` ou constante
//!   inteira. O `urshift` de operando `double` devolve um `double` inteiro sem sinal, que entra pelo valor
//!   (conferido no bun); operando que não é `uint32` é `Unported`.
//! - `op_check_tdz`: o `this` de construtor derivado tem a mensagem fixa do C++. Para as demais variáveis o
//!   C++ lê o texto do fonte pelo `ExpressionInfo` (`provider->getRange(divot - startOffset, divot +
//!   endOffset)`); aqui o nome vem do operando `identifier` (a constante com o nome da variável, que
//!   `emitTDZCheckIfNecessary` grava). Os dois coincidem quando o nó é o próprio identificador; sem o
//!   operando (`undefined`) sai `createTDZError(globalObject)` sem nome.
//! - `op_to_object` de `undefined` ou `null` sem mensagem no operando e de primitivo (`JSValue::toObject`:
//!   `StringObject`, `NumberObject`, `BooleanObject`, `SymbolObject`, `BigIntObject`) é `Unported`; o
//!   `createNotAnObjectError` leva o texto-fonte, que o `SlowPathFrame` ainda não alcança.
//! - `op_profile_type`, `op_profile_control_flow`, `op_log_shadow_chicken_*` e `op_debug` não fazem nada: o
//!   porte não tem `TypeProfiler`, `BasicBlockLocation`, `ShadowChicken` nem depurador ligados (o `.asm`
//!   também não faz nada quando eles estão desligados, e o gerador só emite os dois primeiros com o
//!   perfilador ativo). `op_unreachable` não está aqui: é `crash()` no C++ e fica sem handler.

use crate::bytecode::bytecode_ops::{
    OpArgumentCount, OpBelow, OpBeloweq, OpCheckTdz, OpEq, OpGetArgument, OpGreater, OpGreatereq, OpIsBigInt,
    OpIsCellWithType, OpIsConstructor, OpJbelow, OpJbeloweq, OpJeq, OpJeqPtr, OpJgreater, OpJgreatereq, OpJless,
    OpJlesseq, OpJneq, OpJneqPtr, OpJngreater, OpJngreatereq, OpJnless, OpJnlesseq, OpJnstricteq, OpJstricteq, OpLess,
    OpLesseq, OpNeq, OpNstricteq, OpStrcat, OpStricteq, OpToObject, OpToPrimitive, OpToPropertyKey,
    OpToPropertyKeyOrNumber,
};
use crate::bytecode::instruction_stream::Ref as InstructionRef;
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::llint::dispatch::{DecodedLabel, Step};
use crate::llint::slow_paths::{throw_error_object, throw_out_of_memory_error};
use crate::llint::slow_paths_arith::{
    slow_path_eq, slow_path_greater, slow_path_greatereq, slow_path_less, slow_path_lesseq, slow_path_neq,
    slow_path_nstricteq, slow_path_stricteq, SlowPathFrame,
};
use crate::llint::slow_paths_control::get_js_function;
use crate::llint::slow_paths_jump::{
    slow_path_jeq, slow_path_jgreater, slow_path_jgreatereq, slow_path_jless, slow_path_jlesseq, slow_path_jneq,
    slow_path_jngreater, slow_path_jngreatereq, slow_path_jnless, slow_path_jnlesseq, slow_path_jnstricteq,
    slow_path_jstricteq,
};
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::call_data::get_construct_data;
use crate::runtime::cell_registry;
use crate::runtime::error::{create_reference_error, create_type_error};
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_boolean, js_number_i32, JSValue};
use crate::runtime::object_to_primitive::PreferredPrimitiveType;
use crate::runtime::operations::RopeBuilder;
use crate::wtf::text::wtf_string::String as WtfString;

use crate::llint::dispatch_ext::check_exception;

/// Uma comparação que escreve o booleano em `dst`, seguida de `LLINT_CHECK_EXCEPTION`.
macro_rules! compare_slow_path {
    ($f:ident, $instruction:ident, $op:ty, $path:path) => {{
        let op: $op = $instruction.as_op();
        $path($f, &op);
        check_exception($f)?;
        Step::Next
    }};
}

/// `LLINT_BRANCH(condição)`: o `slow_path_j*` decide e o laço soma o `JUMP_OFFSET`.
macro_rules! branch_slow_path {
    ($f:ident, $instruction:ident, $op:ty, $path:path) => {{
        let op: $op = $instruction.as_op();
        let taken = $path($f, &op);
        check_exception($f)?;
        branch(taken, &op.target_label)
    }};
}

/// Um predicado de célula `dst = predicado(operand)` sem exceção possível.
macro_rules! cell_predicate {
    ($f:ident, $instruction:ident, $op:ty, $predicate:expr) => {{
        let op: $op = $instruction.as_op();
        let value = $f.get(op.operand);
        $f.set(op.dst, js_boolean(($predicate)(&op, value)));
        Step::Next
    }};
}

/// O desfecho de um desvio: salta para o rótulo se `taken`, senão segue.
fn branch(taken: bool, label: &crate::bytecode::bytecode_ops::BoundLabel) -> Step {
    if taken {
        Step::Jump(label.target(&DecodedLabel))
    } else {
        Step::Next
    }
}

/// Um operando de `below`/`beloweq` como o `uint32` que ele representa. O `int32` é a palavra baixa que
/// `cib`/`cibeq` comparam. O `urshift` sem `op_unsigned` (`m_shouldToUnsignedResult = false`) devolve pelo
/// slow path (`jsURShift`, operando `double`) um `double` inteiro em `[2^31, 2^32)`; o bun confere que
/// `(x >>> 0) < (y >>> 0)` com `x` fracionário compara os valores sem sinal (`f(-2.5, 5)` é `false`), então
/// o `double` entra pelo valor numérico, que é o `uint32` exato.
fn unsigned_operand(value: JSValue) -> LLIntResult<u64> {
    if value.is_int32() {
        return Ok(u64::from(value.as_int32() as u32));
    }
    if value.is_double() {
        let number = value.as_double();
        if number >= 0.0 && number <= f64::from(u32::MAX) && number.fract() == 0.0 {
            return Ok(number as u64);
        }
    }
    Err(LLIntFailure::Unported("below/beloweq com operando que não é um uint32 (o gerador só os emite sobre resultado de urshift ou constante inteira)"))
}

/// Os dois operandos de `below`/`beloweq` (`unsigned_operand` em cada lado).
fn unsigned_operands(lhs: JSValue, rhs: JSValue) -> LLIntResult<(u64, u64)> {
    Ok((unsigned_operand(lhs)?, unsigned_operand(rhs)?))
}

/// `slow_path_to_primitive`: `JSValue::toPrimitive(globalObject)`.
fn to_primitive(f: &mut SlowPathFrame, dst: VirtualRegister, src: VirtualRegister) -> LLIntResult<()> {
    let primitive = f.get(src).to_primitive();
    check_exception(f)?;
    f.set(dst, primitive);
    Ok(())
}

/// `JSValue::toPropertyKeyValue(globalObject)`: `String` e `Symbol` ficam; o resto vira `toPrimitive` com
/// dica de string e depois `String` (ou o próprio `Symbol`).
fn to_property_key_value(f: &SlowPathFrame, value: JSValue) -> LLIntResult<JSValue> {
    if value.is_string() || value.is_symbol() {
        return Ok(value);
    }
    let primitive = value.to_primitive_preferred(PreferredPrimitiveType::PreferString);
    check_exception(f)?;
    if primitive.is_symbol() {
        return Ok(primitive);
    }
    let string = primitive.to_string(f.vm);
    check_exception(f)?;
    Ok(JSValue::from_js_string(string))
}

/// `slow_path_to_object`: `undefined` e `null` com mensagem no operando lançam o `TypeError` dela; objeto
/// fica; o resto é `JSValue::toObject` (ver o cabeçalho).
fn to_object(f: &mut SlowPathFrame, op: &OpToObject) -> LLIntResult<()> {
    let value = f.get(op.operand);
    if value.is_undefined_or_null() {
        let ident = f.code_block.identifier(op.message as usize);
        if !ident.is_empty() {
            let global_object = f.code_block.global_object();
            return Err(throw_error_object(global_object, create_type_error(global_object, &ident.string().string())));
        }
    }
    if JSObject::from_value(&value).is_some() || get_js_function(value).is_some() {
        f.set(op.dst, value);
        return Ok(());
    }
    // `argument.toObject(globalObject)`: `undefined`/`null` sem mensagem lançam `createNotAnObjectError` (com o
    // texto-fonte da instrução), o resto vira o invólucro (`StringObject`, `NumberObject`, `BooleanObject`,
    // `SymbolObject`, `BigIntObject`).
    if value.is_undefined_or_null() {
        return Err(crate::llint::slow_paths_object::throw_not_an_object(f, value));
    }
    let global_object = f.code_block.global_object();
    let object = value.to_object(global_object).ok_or(LLIntFailure::Thrown)?;
    f.set(op.dst, object.as_value());
    Ok(())
}

/// `slow_path_check_tdz`: o valor vazio é o `jsTDZValue()` de variável ainda não inicializada.
fn check_tdz(f: &mut SlowPathFrame, op: &OpCheckTdz) -> LLIntResult<()> {
    if !f.get(op.target_virtual_register).is_empty() {
        return Ok(());
    }
    let global_object = f.code_block.global_object();
    let error = if op.target_virtual_register == f.code_block.this_register() {
        create_reference_error(
            global_object,
            &WtfString::from_latin1(
                b"'super()' must be called in derived constructor before accessing |this| or returning non-object.",
            ),
        )
    } else {
        crate::runtime::exception_helpers::create_tdz_error_from_source_range(global_object, &f.error_site())
    };
    Err(throw_error_object(global_object, error))
}

/// `slow_path_strcat`: `jsStringFromRegisterArray(globalObject, &GET(src), count)`. Os `count` valores estão
/// nos registradores `src`, `src - 1`, ... (`strings[-i]`), cada um passa por `toString`.
fn strcat(f: &mut SlowPathFrame, op: &OpStrcat) -> LLIntResult<()> {
    let mut builder = RopeBuilder::new(f.vm);
    for i in 0..op.count {
        let value = f.get(VirtualRegister::new(op.src.offset() - i));
        let piece = value.to_string(f.vm);
        check_exception(f)?;
        if !builder.append(&piece) {
            return Err(throw_out_of_memory_error(f.code_block.global_object()));
        }
    }
    f.set(op.dst, JSValue::from_js_string(builder.release()));
    Ok(())
}

/// Os handlers deste arquivo. `None` é o opcode que não é daqui.
pub(super) fn run_misc(f: &mut SlowPathFrame, instruction: &InstructionRef) -> LLIntResult<Option<Step>> {
    let step = match instruction.opcode_id_enum() {
        // Comparações (`slow_path_less` e companhia).
        OpcodeID::op_eq => compare_slow_path!(f, instruction, OpEq, slow_path_eq),
        OpcodeID::op_neq => compare_slow_path!(f, instruction, OpNeq, slow_path_neq),
        OpcodeID::op_stricteq => compare_slow_path!(f, instruction, OpStricteq, slow_path_stricteq),
        OpcodeID::op_nstricteq => compare_slow_path!(f, instruction, OpNstricteq, slow_path_nstricteq),
        OpcodeID::op_less => compare_slow_path!(f, instruction, OpLess, slow_path_less),
        OpcodeID::op_lesseq => compare_slow_path!(f, instruction, OpLesseq, slow_path_lesseq),
        OpcodeID::op_greater => compare_slow_path!(f, instruction, OpGreater, slow_path_greater),
        OpcodeID::op_greatereq => compare_slow_path!(f, instruction, OpGreatereq, slow_path_greatereq),
        OpcodeID::op_below => {
            let op: OpBelow = instruction.as_op();
            let (lhs, rhs) = unsigned_operands(f.get(op.lhs), f.get(op.rhs))?;
            f.set(op.dst, js_boolean(lhs < rhs));
            Step::Next
        }
        OpcodeID::op_beloweq => {
            let op: OpBeloweq = instruction.as_op();
            let (lhs, rhs) = unsigned_operands(f.get(op.lhs), f.get(op.rhs))?;
            f.set(op.dst, js_boolean(lhs <= rhs));
            Step::Next
        }

        // Desvios por comparação.
        OpcodeID::op_jless => branch_slow_path!(f, instruction, OpJless, slow_path_jless),
        OpcodeID::op_jnless => branch_slow_path!(f, instruction, OpJnless, slow_path_jnless),
        OpcodeID::op_jgreater => branch_slow_path!(f, instruction, OpJgreater, slow_path_jgreater),
        OpcodeID::op_jngreater => branch_slow_path!(f, instruction, OpJngreater, slow_path_jngreater),
        OpcodeID::op_jlesseq => branch_slow_path!(f, instruction, OpJlesseq, slow_path_jlesseq),
        OpcodeID::op_jnlesseq => branch_slow_path!(f, instruction, OpJnlesseq, slow_path_jnlesseq),
        OpcodeID::op_jgreatereq => branch_slow_path!(f, instruction, OpJgreatereq, slow_path_jgreatereq),
        OpcodeID::op_jngreatereq => branch_slow_path!(f, instruction, OpJngreatereq, slow_path_jngreatereq),
        OpcodeID::op_jeq => branch_slow_path!(f, instruction, OpJeq, slow_path_jeq),
        OpcodeID::op_jneq => branch_slow_path!(f, instruction, OpJneq, slow_path_jneq),
        OpcodeID::op_jstricteq => branch_slow_path!(f, instruction, OpJstricteq, slow_path_jstricteq),
        OpcodeID::op_jnstricteq => branch_slow_path!(f, instruction, OpJnstricteq, slow_path_jnstricteq),
        OpcodeID::op_jbelow => {
            let op: OpJbelow = instruction.as_op();
            let (lhs, rhs) = unsigned_operands(f.get(op.lhs), f.get(op.rhs))?;
            branch(lhs < rhs, &op.target_label)
        }
        OpcodeID::op_jbeloweq => {
            let op: OpJbeloweq = instruction.as_op();
            let (lhs, rhs) = unsigned_operands(f.get(op.lhs), f.get(op.rhs))?;
            branch(lhs <= rhs, &op.target_label)
        }
        // `bpeq special, [cfr, value]`: identidade com o ponteiro especial (constante do bloco).
        OpcodeID::op_jeq_ptr => {
            let op: OpJeqPtr = instruction.as_op();
            branch(f.get(op.value) == f.get(op.special_pointer), &op.target_label)
        }
        OpcodeID::op_jneq_ptr => {
            let op: OpJneqPtr = instruction.as_op();
            branch(f.get(op.value) != f.get(op.special_pointer), &op.target_label)
        }

        // Predicados de célula.
        OpcodeID::op_is_big_int => cell_predicate!(f, instruction, OpIsBigInt, |_: &OpIsBigInt, v: JSValue| v.is_big_int()),
        OpcodeID::op_is_constructor => {
            cell_predicate!(f, instruction, OpIsConstructor, |_: &OpIsConstructor, v: JSValue| !get_construct_data(v)
                .is_none())
        }
        // `cbeq JSCell::m_type[value], type`: valor que não é célula dá falso.
        OpcodeID::op_is_cell_with_type => {
            cell_predicate!(f, instruction, OpIsCellWithType, |op: &OpIsCellWithType, v: JSValue| {
                v.is_cell() && cell_registry::cell_type(v.as_cell()).is_some_and(|type_| type_ as u8 == op.type_)
            })
        }

        // Conversões.
        OpcodeID::op_to_primitive => {
            let op: OpToPrimitive = instruction.as_op();
            to_primitive(f, op.dst, op.src)?;
            Step::Next
        }
        OpcodeID::op_to_property_key => {
            let op: OpToPropertyKey = instruction.as_op();
            let key = to_property_key_value(f, f.get(op.src))?;
            f.set(op.dst, key);
            Step::Next
        }
        // `srcValue.isNumber() ? srcValue : srcValue.toPropertyKeyValue(globalObject)`.
        OpcodeID::op_to_property_key_or_number => {
            let op: OpToPropertyKeyOrNumber = instruction.as_op();
            let value = f.get(op.src);
            let key = if value.is_number() { value } else { to_property_key_value(f, value)? };
            f.set(op.dst, key);
            Step::Next
        }
        OpcodeID::op_to_object => {
            to_object(f, &instruction.as_op::<OpToObject>())?;
            Step::Next
        }
        OpcodeID::op_strcat => {
            strcat(f, &instruction.as_op::<OpStrcat>())?;
            Step::Next
        }

        // Frame: `ArgumentCountIncludingThis` e `ThisArgumentOffset[cfr, index, 8]`.
        OpcodeID::op_argument_count => {
            let op: OpArgumentCount = instruction.as_op();
            f.set(op.dst, js_number_i32(f.call_frame.argument_count(f.stack) as i32));
            Step::Next
        }
        // `bilteq argc, index`: fora do intervalo é `undefined`. O índice conta o `this` (índice 0).
        OpcodeID::op_get_argument => {
            let op: OpGetArgument = instruction.as_op();
            let index = op.index as u32 as usize;
            let value = if f.call_frame.argument_count_including_this(f.stack) <= index {
                JSValue::undefined()
            } else if index == 0 {
                f.call_frame.this_value(f.stack)
            } else {
                f.call_frame.get_argument_unsafe(f.stack, index - 1)
            };
            f.set(op.dst, value);
            Step::Next
        }
        OpcodeID::op_check_tdz => {
            check_tdz(f, &instruction.as_op::<OpCheckTdz>())?;
            Step::Next
        }

        // Instrumentação sem efeito observável (ver o cabeçalho).
        OpcodeID::op_super_sampler_begin
        | OpcodeID::op_super_sampler_end
        | OpcodeID::op_identity_with_profile
        | OpcodeID::op_profile_type
        | OpcodeID::op_profile_control_flow
        | OpcodeID::op_log_shadow_chicken_prologue
        | OpcodeID::op_log_shadow_chicken_tail
        | OpcodeID::op_debug => Step::Next,
        _ => return Ok(None),
    };
    Ok(Some(step))
}
