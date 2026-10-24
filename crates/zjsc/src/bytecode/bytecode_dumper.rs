//! Porte de `bytecode/BytecodeDumper.h` e `BytecodeDumper.cpp`, mais o `BytecodeDumperGenerated.cpp`
//! (o `dumpBytecode(dumper, location, instruction)` gerado) e o `dump(...)` que o gerador Ruby põe
//! em cada struct `Op*` de `BytecodeStructs.h`.
//!
//! O formato é o do `Options::dumpGeneratedBytecodes` (`BUN_JSC_dumpGeneratedBytecodes=1`):
//! `[   0] enter`, `[   1] mov                dst:loc4, src:...`. O `PrintStream` do C++ vira um
//! `String` que o dumper acumula e entrega a um `fmt::Write` no fim.
//!
//! Mapa do C++ para cá:
//! - `BytecodeDumperBase` e `BytecodeDumper<CodeBlock>`: `BytecodeDumper` (só para `CodeBlock`; o
//!   `UnlinkedCodeBlockGenerator` só usa o dumper no `dumpBytecodesBeforeGeneratorification`, que
//!   não está portado).
//! - `CodeBlockBytecodeDumper<CodeBlock>`: `dump_block` e `dump_graph`, mais os `dump*` do rodapé.
//! - `Op*::dump`: o `impl_op_dump!`, chamado de dentro de `bytecode_op!`; o nome de cada campo
//!   (`m_thisValue`, impresso `thisValue`) vem do campo em snake_case.
//!
//! DIVERGÊNCIAS:
//! - O campo `type` de `is_cell_with_type` é um `u8` no porte (`JSType` ainda vira `u8`); o nome do
//!   `JSType` é impresso por esse campo, os demais `u8` (`IndexingType`) saem em decimal.
//! - Constantes que são células: o dump do C++ termina com `, StructureID: N` e, fora strings,
//!   imprime endereços. Aqui `String`, `SymbolTable`, `Cell Butterfly`, `TemplateObjectDescriptor` e
//!   `JSPropertyNameEnumerator` e objetos (`Object: ptr with butterfly ptr (Structure ...)`) saem com a `Structure::dump` e o `id()` reais da `Structure` do `VM`
//!   (`stringStructure`, `symbolTableStructure`, `rawImmutableButterflyStructure`,
//!   `templateObjectDescriptorStructure`, `propertyNameEnumeratorStructure`); o ponteiro da `Structure` e
//!   o id dependem da ordem de criação e os testes os normalizam. Células sem `Structure` resolvida saem
//!   só como `Cell: 0x...`.
//! - A ordem das entradas de `String Switch Jump Tables` no C++ é a do hash robin hood; aqui é a
//!   ordem de inserção na tabela (`index_in_table`).
//! - O `ICStatusMap` não existe (o dumper o recebe e não o usa).

use std::fmt::{self, Write};
use std::rc::Rc;

use crate::bytecode::bytecode_graph::BytecodeGraph;
use crate::bytecode::bytecode_ops::*;
use crate::bytecode::bytecode_ops_decode::NoGenerator;
use crate::bytecode::code_block::CodeBlock;
use crate::bytecode::handler_info::HandlerType;
use crate::bytecode::instruction_stream::{InstructionStream, JSInstruction, Ref as InstructionRef};
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::put_by_id_flags::PutByIdFlags;
use crate::bytecode::put_kind::PrivateFieldPutKind;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::bytecompiler::label::GenericBoundLabel;
use crate::bytecompiler::profile_type_bytecode_flag::ProfileTypeBytecodeFlag;
use crate::interpreter::interpreter::DebugHookType;
use crate::parser::result_type::{OperandTypes, ResultType};
use crate::runtime::ecma_mode::ECMAMode;
use crate::runtime::error_type::{error_type_name_with_extension, ErrorTypeWithExtension};
use crate::runtime::get_put_info::{print_resolve_type, GetPutInfo, ResolveType};
use crate::runtime::js_cjs_value_types::SourceCodeRepresentation;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_type::js_type_from_u8;
use crate::runtime::js_value::JSValue;
use crate::runtime::symbol_table_or_scope_depth::SymbolTableOrScopeDepth;
use crate::wtf::text::conversion_mode::ConversionMode;

/// `%f` do `printf` do C (`%lf`): seis casas, `nan`/`inf` em minúsculas.
fn c_double(value: f64) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() { "-nan" } else { "nan" }.to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    format!("{value:.6}")
}

/// `m_thisValue` virou `this_value` no porte: devolve `thisValue` (e `type_` vira `type`).
fn camel_case_field(field: &str) -> String {
    let mut result = String::with_capacity(field.len());
    let mut upper_next = false;
    for character in field.trim_end_matches('_').chars() {
        if character == '_' {
            upper_next = true;
        } else if upper_next {
            result.push(character.to_ascii_uppercase());
            upper_next = false;
        } else {
            result.push(character);
        }
    }
    result
}

/// `JSCell::dump` (`Cell: ptr (structure), StructureID: id`) para as células que não são string.
/// Células de outros tipos (objetos, funções) ainda não têm a `Structure` resolvida aqui e saem só com o ponteiro.
fn dump_non_string_cell(pointer: usize) -> String {
    let entry = cell_registry::get(pointer);
    let structure = match &entry {
        Some(CellEntry::SymbolTable(table)) => Rc::clone(table.borrow().structure()),
        Some(CellEntry::CellButterfly(butterfly)) => Rc::clone(butterfly.structure()),
        Some(CellEntry::TemplateObjectDescriptor(descriptor)) => Rc::clone(descriptor.structure()),
        Some(CellEntry::PropertyNameEnumerator(enumerator)) => Rc::clone(enumerator.structure()),
        // Ramo `isSubClassOf(RegExp::info())` do `dumpInContextAssumingStructure` (`RegExp::dumpToStream`:
        // `/padrão/flags`). A célula `RegExp` não tem `Structure` no porte, o id sai 0.
        Some(CellEntry::RegExp(reg_exp)) => {
            let pattern = String::from_utf8_lossy(&reg_exp.pattern().utf8(crate::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned();
            let flags = crate::yarr::yarr_flags::flags_string(reg_exp.flags());
            let flags: String = flags.iter().take_while(|byte| **byte != 0).map(|byte| *byte as char).collect();
            return format!("RegExp: /{pattern}/{flags}, StructureID: 0");
        }
        // Ramo `isHeapBigInt()` do `dumpInContextAssumingStructure`, mais o `StructureID` do chamador.
        Some(CellEntry::BigInt(big_int)) => {
            let id = big_int.structure().map_or(0, |structure| structure.id());
            return format!(
                "BigInt[heap-allocated]: addr={pointer:#x}, length={}, sign={}, StructureID: {id}",
                big_int.length(),
                big_int.sign()
            );
        }
        // Ramo `isSubClassOf(JSObject::info())` do `dumpInContextAssumingStructure`.
        Some(other) => match other.as_js_object() {
            Some(object) => {
                let structure = object.structure();
                // O ponteiro do butterfly não existe no porte; um valor não nulo sintético basta, pois os
                // testes normalizam os endereços. `(base=...)` só sai com butterfly.
                let butterfly = if object.has_butterfly() {
                    format!("{:#x}(base={:#x})", pointer ^ 0x8, pointer ^ 0x8)
                } else {
                    "(nil)".to_string()
                };
                return format!(
                    "Object: {pointer:#x} with butterfly {butterfly} (Structure {}), StructureID: {}",
                    structure.dump(),
                    structure.id()
                );
            }
            None => return format!("Cell: {pointer:#x}"),
        },
        None => return format!("Cell: {pointer:#x}"),
    };
    format!("Cell: {pointer:#x} ({}), StructureID: {}", structure.dump(), structure.id())
}

/// `JSValue::dump` (`dumpInContextAssumingStructure`) como o texto que o C++ imprimiria.
fn dump_js_value(value: JSValue) -> String {
    match value {
        JSValue::Empty => "<JSValue()>".to_string(),
        JSValue::Int32(number) => format!("Int32: {number}"),
        JSValue::Double(number) => format!("Double: {}, {}", number.to_bits() as i64, c_double(number)),
        JSValue::Bool(true) => "True".to_string(),
        JSValue::Bool(false) => "False".to_string(),
        JSValue::Null => "Null".to_string(),
        JSValue::Undefined => "Undefined".to_string(),
        JSValue::Deleted => "INVALID".to_string(),
        JSValue::Cell(pointer) => {
            if !value.is_string() {
                return dump_non_string_cell(pointer);
            }
            let string = value.as_js_string();
            let mut text = String::from("String");
            if string.is_rope() {
                text.push_str(" (rope)");
            }
            let characters = match string.try_get_value_impl() {
                Some(string_impl) => {
                    if string_impl.is_atom() {
                        text.push_str(" (atomic)");
                    }
                    if string_impl.is_symbol() {
                        text.push_str(" (symbol)");
                    }
                    String::from_utf8_lossy(&string_impl.utf8(ConversionMode::LenientConversion)).into_owned()
                }
                None => {
                    text.push_str(" (unresolved)");
                    String::new()
                }
            };
            let eight_bit = if string.is_8bit() { 1 } else { 0 };
            let _ = write!(
                text,
                ",8Bit:({eight_bit}),length:({}): {characters}, StructureID: {}",
                string.length(),
                string.structure().id()
            );
            text
        }
    }
}

/// `BytecodeDumperBase<JSInstructionStream>` e `BytecodeDumper<CodeBlock>`.
pub struct BytecodeDumper<'a> {
    block: &'a CodeBlock,
    out: String,
    current_location: u32,
}

impl<'a> BytecodeDumper<'a> {
    pub fn new(block: &'a CodeBlock) -> BytecodeDumper<'a> {
        BytecodeDumper { block, out: String::new(), current_location: 0 }
    }

    /// O texto acumulado até aqui.
    pub fn text(&self) -> &str {
        &self.out
    }

    /// `printLocationAndOp(location, op)`: `"[%4u] %-18s "`. `op` é o `OPCODE_NAMES` (com `op_`);
    /// `size_shift_amount` põe um `*` por nível de largura (o `&"**nome"[2 - shift]` do C++).
    pub fn print_location_and_op(&mut self, location: u32, opcode_name: &str, size_shift_amount: i32) {
        self.current_location = location;
        let name = opcode_name.strip_prefix("op_").unwrap_or(opcode_name);
        let stars = "*".repeat(size_shift_amount as usize);
        let _ = write!(self.out, "[{location:>4}] {:<18} ", format!("{stars}{name}"));
    }

    /// `dumpOperand(operandName, operand, isFirst)`.
    pub fn dump_operand<T: DumpValue + ?Sized>(&mut self, field: &str, operand: &T, is_first: bool) {
        if !is_first {
            self.out.push_str(", ");
        }
        let name = camel_case_field(field);
        self.out.push_str(&name);
        self.out.push(':');
        operand.dump_value(self, &name);
    }

    /// `constantName(reg)`.
    fn constant_name(&self, register: VirtualRegister) -> String {
        let constants = self.block.constant_registers();
        if register.to_constant_index() as usize >= constants.len() {
            return format!("INVALID_CONSTANT({register})");
        }
        format!("{}({register})", dump_js_value(self.block.get_constant(register)))
    }

    /// `registerName(r)`.
    pub fn register_name(&self, register: VirtualRegister) -> String {
        if register.is_constant() {
            return self.constant_name(register);
        }
        register.to_string()
    }

    /// `outOfLineJumpOffset(offset)`.
    fn out_of_line_jump_offset(&self, offset: u32) -> i32 {
        self.block.unlinked_code_block().borrow().out_of_line_jump_offset(offset)
    }

    /// `BytecodeDumper::dumpBytecode(it, statusMap)`: a instrução em `location` e o `\n`.
    pub fn dump_instruction(&mut self, location: u32, instruction: JSInstruction<'_>) {
        let size_shift_amount = instruction.size_shift_amount();
        dump_bytecode(self, location, size_shift_amount, instruction);
        self.out.push('\n');
    }
}

/// O que `m_out.print(v)` imprime para cada tipo de operando (`dumpValue` do C++).
pub trait DumpValue {
    /// `field` é o nome do campo já em camelCase (só o `type` do `u8` o consulta).
    fn dump_value(&self, dumper: &mut BytecodeDumper<'_>, field: &str);
}

impl DumpValue for VirtualRegister {
    fn dump_value(&self, dumper: &mut BytecodeDumper<'_>, _field: &str) {
        let name = dumper.register_name(*self);
        dumper.out.push_str(&name);
    }
}

impl<Traits> DumpValue for GenericBoundLabel<Traits> {
    fn dump_value(&self, dumper: &mut BytecodeDumper<'_>, _field: &str) {
        let mut target = self.target(&NoGenerator);
        if target == 0 {
            target = dumper.out_of_line_jump_offset(dumper.current_location);
        }
        let target_offset = (target as u32).wrapping_add(dumper.current_location);
        let _ = write!(dumper.out, "{target}(->{target_offset})");
    }
}

macro_rules! dump_value_display {
    ($($ty:ty),* $(,)?) => {$(
        impl DumpValue for $ty {
            fn dump_value(&self, dumper: &mut BytecodeDumper<'_>, _field: &str) {
                let _ = write!(dumper.out, "{}", self);
            }
        }
    )*};
}

dump_value_display!(u32, i32, bool, ProfileTypeBytecodeFlag);

impl DumpValue for u8 {
    fn dump_value(&self, dumper: &mut BytecodeDumper<'_>, field: &str) {
        match (field, js_type_from_u8(*self)) {
            ("type", Some(js_type)) => {
                let _ = write!(dumper.out, "{js_type:?}");
            }
            _ => {
                let _ = write!(dumper.out, "{}", *self as u32);
            }
        }
    }
}

/// Tipos com `dump(PrintStream&)` (o `print(v)` chama `v.dump(out)`).
macro_rules! dump_value_via_dump {
    ($($ty:ty),* $(,)?) => {$(
        impl DumpValue for $ty {
            fn dump_value(&self, dumper: &mut BytecodeDumper<'_>, _field: &str) {
                let _ = self.dump(&mut dumper.out);
            }
        }
    )*};
}

dump_value_via_dump!(ECMAMode, GetPutInfo, ResultType, OperandTypes, SymbolTableOrScopeDepth, PutByIdFlags, PrivateFieldPutKind);

impl DumpValue for ResolveType {
    fn dump_value(&self, dumper: &mut BytecodeDumper<'_>, _field: &str) {
        let _ = print_resolve_type(&mut dumper.out, *self);
    }
}

impl DumpValue for DebugHookType {
    fn dump_value(&self, dumper: &mut BytecodeDumper<'_>, _field: &str) {
        // `printInternal(PrintStream&, DebugHookType)`: o nome do enumerador.
        let _ = write!(dumper.out, "{self:?}");
    }
}

impl DumpValue for ErrorTypeWithExtension {
    fn dump_value(&self, dumper: &mut BytecodeDumper<'_>, _field: &str) {
        dumper.out.push_str(error_type_name_with_extension(*self));
    }
}

/// O `dump(dumper, location, sizeShiftAmount)` que o gerador Ruby põe em cada struct `Op*`.
pub trait DumpOp {
    fn dump(&self, dumper: &mut BytecodeDumper<'_>, location: u32, size_shift_amount: i32);
}

/// `Op*::dump`: o nome do opcode e cada operando, na ordem do `args:`.
macro_rules! impl_op_dump {
    ($name:ident, $id:ident { $($field:ident),* $(,)? }) => {
        impl $crate::bytecode::bytecode_dumper::DumpOp for $name {
            #[allow(unused_mut, unused_variables)]
            fn dump(
                &self,
                dumper: &mut $crate::bytecode::bytecode_dumper::BytecodeDumper<'_>,
                location: u32,
                size_shift_amount: i32,
            ) {
                dumper.print_location_and_op(
                    location,
                    $crate::bytecode::opcode::OPCODE_NAMES[$crate::bytecode::opcode::OpcodeID::$id as usize],
                    size_shift_amount,
                );
                let mut is_first = true;
                $(
                    dumper.dump_operand(stringify!($field), &self.$field, is_first);
                    is_first = false;
                )*
            }
        }
    };
}

pub(crate) use impl_op_dump;

/// `dumpBytecode(dumper, location, instruction)` de `BytecodeDumperGenerated.cpp`: o `switch` pelo
/// opcode.
macro_rules! dispatch_dump {
    ($dumper:expr, $location:expr, $shift:expr, $instruction:expr; $($id:ident => $name:ident),* $(,)?) => {
        match $instruction.opcode_id_enum() {
            $(OpcodeID::$id => $instruction.as_op::<$name>().dump($dumper, $location, $shift),)*
            #[allow(unreachable_patterns)]
            _ => unreachable!("opcode sem dump"),
        }
    };
}

fn dump_bytecode(dumper: &mut BytecodeDumper<'_>, location: u32, shift: i32, instruction: JSInstruction<'_>) {
    dispatch_dump! { dumper, location, shift, instruction;
        op_tail_call_varargs => OpTailCallVarargs,
        op_call_varargs => OpCallVarargs,
        op_iterator_next => OpIteratorNext,
        op_construct_varargs => OpConstructVarargs,
        op_super_construct_varargs => OpSuperConstructVarargs,
        op_iterator_open => OpIteratorOpen,
        op_async_iterator_open => OpAsyncIteratorOpen,
        op_instanceof => OpInstanceof,
        op_set_private_brand => OpSetPrivateBrand,
        op_check_private_brand => OpCheckPrivateBrand,
        op_put_by_id => OpPutById,
        op_construct => OpConstruct,
        op_super_construct => OpSuperConstruct,
        op_tail_call => OpTailCall,
        op_call_direct_eval => OpCallDirectEval,
        op_create_generator => OpCreateGenerator,
        op_create_async_generator => OpCreateAsyncGenerator,
        op_create_promise => OpCreatePromise,
        op_catch => OpCatch,
        op_new_array_with_size => OpNewArrayWithSize,
        op_new_array_buffer => OpNewArrayBuffer,
        op_get_by_id => OpGetById,
        op_get_length => OpGetLength,
        op_profile_type => OpProfileType,
        op_profile_control_flow => OpProfileControlFlow,
        op_new_array_with_species => OpNewArrayWithSpecies,
        op_call => OpCall,
        op_call_ignore_result => OpCallIgnoreResult,
        op_async_iterator_next => OpAsyncIteratorNext,
        op_resolve_scope => OpResolveScope,
        op_get_from_scope => OpGetFromScope,
        op_put_to_scope => OpPutToScope,
        op_create_this => OpCreateThis,
        op_new_object => OpNewObject,
        op_new_array => OpNewArray,
        op_put_private_name => OpPutPrivateName,
        op_get_private_name => OpGetPrivateName,
        op_get_by_val_with_this => OpGetByValWithThis,
        op_get_by_val => OpGetByVal,
        op_put_by_val => OpPutByVal,
        op_put_by_val_direct => OpPutByValDirect,
        op_in_by_val => OpInByVal,
        op_enumerator_next => OpEnumeratorNext,
        op_enumerator_in_by_val => OpEnumeratorInByVal,
        op_enumerator_has_own_property => OpEnumeratorHasOwnProperty,
        op_enumerator_put_by_val => OpEnumeratorPutByVal,
        op_to_this => OpToThis,
        op_enumerator_get_by_val => OpEnumeratorGetByVal,
        op_get_by_id_direct => OpGetByIdDirect,
        op_jneq_ptr => OpJneqPtr,
        op_jeq_ptr => OpJeqPtr,
        op_enter => OpEnter,
        op_get_argument => OpGetArgument,
        op_get_from_arguments => OpGetFromArguments,
        op_get_prototype_of => OpGetPrototypeOf,
        op_get_internal_field => OpGetInternalField,
        op_get_by_id_with_this => OpGetByIdWithThis,
        op_to_object => OpToObject,
        op_in_by_id => OpInById,
        op_has_private_name => OpHasPrivateName,
        op_has_private_brand => OpHasPrivateBrand,
        op_put_by_id_with_this => OpPutByIdWithThis,
        op_del_by_id => OpDelById,
        op_put_by_val_with_this => OpPutByValWithThis,
        op_del_by_val => OpDelByVal,
        op_put_getter_by_id => OpPutGetterById,
        op_put_setter_by_id => OpPutSetterById,
        op_put_getter_setter_by_id => OpPutGetterSetterById,
        op_put_getter_by_val => OpPutGetterByVal,
        op_put_setter_by_val => OpPutSetterByVal,
        op_define_data_property => OpDefineDataProperty,
        op_define_accessor_property => OpDefineAccessorProperty,
        op_set_function_name => OpSetFunctionName,
        op_ret => OpRet,
        op_strcat => OpStrcat,
        op_put_to_arguments => OpPutToArguments,
        op_push_with_scope => OpPushWithScope,
        op_get_parent_scope => OpGetParentScope,
        op_throw => OpThrow,
        op_throw_static_error => OpThrowStaticError,
        op_debug => OpDebug,
        op_get_property_enumerator => OpGetPropertyEnumerator,
        op_create_rest => OpCreateRest,
        op_yield => OpYield,
        op_log_shadow_chicken_prologue => OpLogShadowChickenPrologue,
        op_log_shadow_chicken_tail => OpLogShadowChickenTail,
        op_resolve_scope_for_hoisting_func_decl_in_eval => OpResolveScopeForHoistingFuncDeclInEval,
        op_put_internal_field => OpPutInternalField,
        op_create_scoped_arguments => OpCreateScopedArguments,
        op_check_tdz => OpCheckTdz,
        op_new_array_with_spread => OpNewArrayWithSpread,
        op_spread => OpSpread,
        op_new_reg_exp => OpNewRegExp,
        op_negate => OpNegate,
        op_identity_with_profile => OpIdentityWithProfile,
        op_is_cell_with_type => OpIsCellWithType,
        op_has_structure_with_flags => OpHasStructureWithFlags,
        op_eq => OpEq,
        op_neq => OpNeq,
        op_stricteq => OpStricteq,
        op_nstricteq => OpNstricteq,
        op_less => OpLess,
        op_lesseq => OpLesseq,
        op_greater => OpGreater,
        op_greatereq => OpGreatereq,
        op_below => OpBelow,
        op_beloweq => OpBeloweq,
        op_mod => OpMod,
        op_pow => OpPow,
        op_urshift => OpUrshift,
        op_add => OpAdd,
        op_mul => OpMul,
        op_div => OpDiv,
        op_sub => OpSub,
        op_bitand => OpBitand,
        op_bitor => OpBitor,
        op_bitxor => OpBitxor,
        op_lshift => OpLshift,
        op_rshift => OpRshift,
        op_eq_null => OpEqNull,
        op_neq_null => OpNeqNull,
        op_to_string => OpToString,
        op_is_empty => OpIsEmpty,
        op_typeof_is_undefined => OpTypeofIsUndefined,
        op_typeof_is_object => OpTypeofIsObject,
        op_typeof_is_function => OpTypeofIsFunction,
        op_is_undefined_or_null => OpIsUndefinedOrNull,
        op_is_boolean => OpIsBoolean,
        op_is_number => OpIsNumber,
        op_is_big_int => OpIsBigInt,
        op_is_object => OpIsObject,
        op_is_callable => OpIsCallable,
        op_is_constructor => OpIsConstructor,
        op_to_number => OpToNumber,
        op_to_numeric => OpToNumeric,
        op_bitnot => OpBitnot,
        op_unsigned => OpUnsigned,
        op_inc => OpInc,
        op_dec => OpDec,
        op_jeq => OpJeq,
        op_jstricteq => OpJstricteq,
        op_jneq => OpJneq,
        op_jnstricteq => OpJnstricteq,
        op_jless => OpJless,
        op_jlesseq => OpJlesseq,
        op_jgreater => OpJgreater,
        op_jgreatereq => OpJgreatereq,
        op_jnless => OpJnless,
        op_jnlesseq => OpJnlesseq,
        op_jngreater => OpJngreater,
        op_jngreatereq => OpJngreatereq,
        op_jbelow => OpJbelow,
        op_jbeloweq => OpJbeloweq,
        op_jtrue => OpJtrue,
        op_jfalse => OpJfalse,
        op_jeq_null => OpJeqNull,
        op_jneq_null => OpJneqNull,
        op_jundefined_or_null => OpJundefinedOrNull,
        op_jnundefined_or_null => OpJnundefinedOrNull,
        op_switch_imm => OpSwitchImm,
        op_switch_char => OpSwitchChar,
        op_switch_string => OpSwitchString,
        op_new_func => OpNewFunc,
        op_new_func_exp => OpNewFuncExp,
        op_new_generator_func => OpNewGeneratorFunc,
        op_new_generator_func_exp => OpNewGeneratorFuncExp,
        op_new_async_func => OpNewAsyncFunc,
        op_new_async_func_exp => OpNewAsyncFuncExp,
        op_new_async_generator_func => OpNewAsyncGeneratorFunc,
        op_new_async_generator_func_exp => OpNewAsyncGeneratorFuncExp,
        op_to_primitive => OpToPrimitive,
        op_to_property_key => OpToPropertyKey,
        op_to_property_key_or_number => OpToPropertyKeyOrNumber,
        op_loop_hint => OpLoopHint,
        op_unreachable => OpUnreachable,
        op_check_traps => OpCheckTraps,
        op_nop => OpNop,
        op_super_sampler_begin => OpSuperSamplerBegin,
        op_wide16 => OpWide16,
        op_super_sampler_end => OpSuperSamplerEnd,
        op_wide32 => OpWide32,
        op_get_scope => OpGetScope,
        op_create_direct_arguments => OpCreateDirectArguments,
        op_create_cloned_arguments => OpCreateClonedArguments,
        op_new_promise => OpNewPromise,
        op_new_generator => OpNewGenerator,
        op_new_async_function_generator => OpNewAsyncFunctionGenerator,
        op_argument_count => OpArgumentCount,
        op_create_lexical_environment => OpCreateLexicalEnvironment,
        op_create_generator_frame_environment => OpCreateGeneratorFrameEnvironment,
        op_not => OpNot,
        op_typeof => OpTypeof,
        op_mov => OpMov,
        op_jmp => OpJmp,
    }
}

/// `dumpIdentifiers()`.
fn dump_identifiers(block: &CodeBlock, out: &mut String) {
    let count = block.number_of_identifiers();
    if count == 0 {
        return;
    }
    out.push_str("\nIdentifiers:\n");
    for index in 0..count {
        let identifier = block.identifier(index);
        // `Identifier::dump`: o símbolo privado leva o prefixo `PrivateSymbol.`.
        let prefix = if identifier.is_private_name() { "PrivateSymbol." } else { "" };
        let _ = writeln!(out, "  id{index} = {prefix}{}", String::from_utf8_lossy(&identifier.utf8()));
    }
}

/// `dumpConstants()`.
fn dump_constants(block: &CodeBlock, out: &mut String) {
    let constants = block.constant_registers();
    if constants.is_empty() {
        return;
    }
    out.push_str("\nConstants:\n");
    let unlinked = block.unlinked_code_block().borrow();
    let representations = unlinked.constants_source_code_representation();
    for (index, constant) in constants.iter().enumerate() {
        let representation = representations.get(index).copied().unwrap_or(SourceCodeRepresentation::Other);
        let description = match representation {
            SourceCodeRepresentation::Double => ": in source as double",
            SourceCodeRepresentation::Integer => ": in source as integer",
            SourceCodeRepresentation::Other => "",
            SourceCodeRepresentation::LinkTimeConstant => ": in source as link-time-constant",
        };
        let _ = writeln!(out, "   k{index} = {}{description}", dump_js_value(*constant));
    }
}

/// `dumpExceptionHandlers()`: `handlers` são `(start, end, target, type)` copiados antes, porque
/// `CodeBlock::exception_handler` pede `&mut`.
fn dump_exception_handlers(handlers: &[(u32, u32, u32, HandlerType)], out: &mut String) {
    if handlers.is_empty() {
        return;
    }
    out.push_str("\nException Handlers:\n");
    for (index, (start, end, target, handler_type)) in handlers.iter().enumerate() {
        // `HandlerInfoBase::typeName()`.
        let type_name = match handler_type {
            HandlerType::Catch => "catch",
            HandlerType::Finally => "finally",
            HandlerType::SynthesizedCatch => "synthesized catch",
            HandlerType::SynthesizedFinally => "synthesized finally",
        };
        let _ = writeln!(out, "\t {}: {{ start: [{start:>4}] end: [{end:>4}] target: [{target:>4}] }} {type_name}", index + 1);
    }
}

/// `dumpSwitchJumpTables()`.
fn dump_switch_jump_tables(block: &CodeBlock, out: &mut String) {
    let unlinked = block.unlinked_code_block().borrow();
    let count = unlinked.number_of_unlinked_switch_jump_tables();
    if count == 0 {
        return;
    }
    out.push_str("Switch Jump Tables:\n");
    for index in 0..count {
        let _ = writeln!(out, "  {index} = {{");
        let table = unlinked.unlinked_switch_jump_table(index);
        if table.is_list() {
            for pair in table.branch_offsets.chunks(2) {
                let _ = writeln!(out, "\t\t{:>4} => {:04}", pair[0], pair[1]);
            }
        } else {
            for (entry, offset) in table.branch_offsets.iter().enumerate() {
                if *offset == 0 {
                    continue;
                }
                let _ = writeln!(out, "\t\t{:>4} => {offset:04}", entry as i32 + table.min);
            }
        }
        let _ = writeln!(out, "\t\tdefault => {:04}", table.default_offset);
        out.push_str("      }\n");
    }
}

/// `dumpStringSwitchJumpTables()`.
fn dump_string_switch_jump_tables(block: &CodeBlock, out: &mut String) {
    let unlinked = block.unlinked_code_block().borrow();
    let count = unlinked.number_of_unlinked_string_switch_jump_tables();
    if count == 0 {
        return;
    }
    out.push_str("\nString Switch Jump Tables:\n");
    for index in 0..count {
        let _ = writeln!(out, "  {index} = {{");
        let table = unlinked.unlinked_string_switch_jump_table(index);
        let mut entries: Vec<_> = table.offset_table.iter().collect();
        entries.sort_by_key(|(_, location)| location.index_in_table);
        for (key, location) in entries {
            let text = key.0.utf8(ConversionMode::LenientConversion);
            let _ = writeln!(out, "\t\t\"{}\" => {:04}", String::from_utf8_lossy(&text), location.branch_offset);
        }
        let _ = writeln!(out, "\t\tdefault => {:04}", table.default_offset);
        out.push_str("      }\n");
    }
}

/// `dumpHeader(block, instructions, out)`. `metadata_size_in_bytes` é o `block->metadataSizeInBytes()`.
fn dump_header(
    block: &CodeBlock,
    instructions: &InstructionStream,
    metadata_size_in_bytes: usize,
    out: &mut String,
) -> fmt::Result {
    let mut instruction_count = 0usize;
    let mut wide16_instruction_count = 0usize;
    let mut wide32_instruction_count = 0usize;
    let mut instruction_with_metadata_count = 0usize;
    for instruction in instructions.iter() {
        let (is_wide16, is_wide32, has_metadata) =
            instruction.with_instruction(|ins| (ins.is_wide16(), ins.is_wide32(), ins.has_metadata()));
        if is_wide16 {
            wide16_instruction_count += 1;
        } else if is_wide32 {
            wide32_instruction_count += 1;
        }
        if has_metadata {
            instruction_with_metadata_count += 1;
        }
        instruction_count += 1;
    }
    block.dump(out)?;
    write!(
        out,
        ": {instruction_count} instructions ({wide16_instruction_count} 16-bit instructions, {wide32_instruction_count} 32-bit instructions, {instruction_with_metadata_count} instructions with metadata); {} bytes ({metadata_size_in_bytes} metadata bytes); {} parameter(s); {} callee register(s); {} variable(s)",
        instructions.size_in_bytes() + metadata_size_in_bytes,
        block.num_parameters(),
        block.num_callee_locals(),
        block.num_vars(),
    )?;
    write!(out, "; scope at {}", block.scope_register())?;
    out.push('\n');
    Ok(())
}

/// `dumpFooter(dumper)`.
fn dump_footer(block: &CodeBlock, handlers: &[(u32, u32, u32, HandlerType)], out: &mut String) {
    dump_identifiers(block, out);
    dump_constants(block, out);
    dump_exception_handlers(handlers, out);
    dump_switch_jump_tables(block, out);
    dump_string_switch_jump_tables(block, out);
}

/// O que `CodeBlock` só entrega por `&mut` ou por `borrow_mut` (`exceptionHandler(i)`,
/// `metadata()`), tirado antes de `instructions()` emprestar o `UnlinkedCodeBlock`.
fn collect_mutable_state(block: &mut CodeBlock) -> (Vec<(u32, u32, u32, HandlerType)>, usize) {
    let mut handlers = Vec::new();
    for index in 0..block.number_of_exception_handlers() {
        let handler = block.exception_handler(index);
        handlers.push((handler.base.start, handler.base.end, handler.base.target, handler.base.type_()));
    }
    // `UnlinkedCodeBlock::metadataSizeInBytes()` (`UnlinkedMetadataTable::sizeInBytesForGC()`).
    let metadata_size = {
        let unlinked = block.unlinked_code_block();
        let mut unlinked = unlinked.borrow_mut();
        let metadata = unlinked.metadata();
        if metadata.is_finalized() && !metadata.has_metadata() {
            0
        } else {
            metadata.offset_table_size() as usize
        }
    };
    (handlers, metadata_size)
}

/// `CodeBlockBytecodeDumper<CodeBlock>::dumpBlock`: cabeçalho, as instruções em sequência, rodapé.
pub fn dump_block(block: &mut CodeBlock, out: &mut dyn fmt::Write) -> fmt::Result {
    let (handlers, metadata_size) = collect_mutable_state(block);
    let block: &CodeBlock = block;
    let mut text = String::new();
    {
        let instructions = block.instructions();
        dump_header(block, &instructions, metadata_size, &mut text)?;
        let mut dumper = BytecodeDumper::new(block);
        for instruction in instructions.iter() {
            instruction.with_instruction(|ins| dumper.dump_instruction(instruction.offset(), ins));
        }
        text.push_str(dumper.text());
    }
    dump_footer(block, &handlers, &mut text);
    text.push('\n');
    out.write_str(&text)
}

/// `CodeBlockBytecodeDumper<CodeBlock>::dumpGraph`: as instruções por bloco básico, com
/// `bb#N`, `Predecessors` e `Successors`.
pub fn dump_graph(block: &mut CodeBlock, graph: &BytecodeGraph, out: &mut dyn fmt::Write) -> fmt::Result {
    let (handlers, metadata_size) = collect_mutable_state(block);
    let block: &CodeBlock = block;
    let mut text = String::new();
    {
        let instructions = block.instructions();
        dump_header(block, &instructions, metadata_size, &mut text)?;
        text.push('\n');

        let mut predecessors: Vec<Vec<u32>> = vec![Vec::new(); graph.size()];
        for basic_block in graph.iter() {
            if basic_block.is_entry_block() || basic_block.is_exit_block() {
                continue;
            }
            for successor_index in basic_block.successors() {
                let list = &mut predecessors[*successor_index as usize];
                if !list.contains(&basic_block.index()) {
                    list.push(basic_block.index());
                }
            }
        }

        let mut dumper = BytecodeDumper::new(block);
        for basic_block in graph.iter() {
            if basic_block.is_entry_block() || basic_block.is_exit_block() {
                continue;
            }
            let _ = writeln!(dumper.out, "bb#{}", basic_block.index());
            dumper.out.push_str("Predecessors: [");
            for predecessor in &predecessors[basic_block.index() as usize] {
                if !graph.at(*predecessor as usize).is_entry_block() {
                    let _ = write!(dumper.out, " #{predecessor}");
                }
            }
            dumper.out.push_str(" ]\n");

            let mut i = 0u32;
            while i < basic_block.total_length() {
                let current = instructions.at(i + basic_block.leader_offset());
                let size = current.size() as u32;
                current.with_instruction(|ins| dumper.dump_instruction(current.offset(), ins));
                i += size;
            }

            dumper.out.push_str("Successors: [");
            for successor in basic_block.successors() {
                if !graph.at(*successor as usize).is_exit_block() {
                    let _ = write!(dumper.out, " #{successor}");
                }
            }
            dumper.out.push_str(" ]\n\n");
        }
        text.push_str(dumper.text());
    }
    dump_footer(block, &handlers, &mut text);
    text.push('\n');
    out.write_str(&text)
}

/// `BytecodeDumper<CodeBlock>::dumpBytecode(block, out, it)`: uma instrução e o `\n`.
pub fn dump_bytecode_instruction(block: &CodeBlock, instruction: &InstructionRef, out: &mut dyn fmt::Write) -> fmt::Result {
    let mut dumper = BytecodeDumper::new(block);
    instruction.with_instruction(|ins| dumper.dump_instruction(instruction.offset(), ins));
    out.write_str(dumper.text())
}
