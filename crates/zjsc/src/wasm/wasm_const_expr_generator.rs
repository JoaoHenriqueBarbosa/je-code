//! Tradução de `wasm/WasmConstExprGenerator.cpp`, a parte que valida: o `ConstExprGenerator` (o
//! `Context` do `FunctionParser` para expressões constantes estendidas) e `parseExtendedConstExpr`.
//!
//! A avaliação (`ConstExprInterpreter`) está em `wasm_const_expr_interpreter.rs`.
//!
//! No C++ quase todo `add*` do gerador é um `CONST_EXPR_STUB` que recusa com
//! "Invalid instruction for constant expression"; aqui a lista branca é a de `Context::add`.

use crate::wasm::wasm_format::{Mutability, Type, TypeKind};
use crate::wasm::wasm_function_parser::{
    BlockSignature, BlockType, ControlData, Context, Env, FunctionParser, Hook, PartialResult,
};
use crate::wasm::wasm_module_information::ModuleInformation;
use crate::wasm::wasm_ops::{ExtGCOpType, OpType};
use crate::wasm::wasm_simd_opcodes::ExtSimdOpType;

/// `ConstExprGenerator::ControlData`: só existe o bloco de nível superior, que não é alvo de desvio.
pub struct ConstExprControl {
    signature: BlockSignature,
}

impl ControlData for ConstExprControl {
    fn block_type(&self) -> BlockType {
        BlockType::TopLevel
    }
    fn set_block_type(&mut self, _block_type: BlockType) {}
    fn signature(&self) -> &BlockSignature {
        &self.signature
    }
    fn branch_target_arity(&self) -> usize {
        0
    }
    fn branch_target_type(&self, _index: usize) -> Type {
        Type::new(TypeKind::Void, crate::wasm::wasm_format::TypeIndex::Invalid)
    }
}

/// `ConstExprGenerator`.
pub struct ConstExprGenerator {
    offset_in_source: usize,
    /// `m_shouldError`: opcodes como `nop` não chegam ao contexto, o `didParseOpcode` os marca.
    should_error: bool,
    declared_functions: Vec<u32>,
}

impl ConstExprGenerator {
    pub fn new(offset_in_source: usize) -> ConstExprGenerator {
        ConstExprGenerator { offset_in_source, should_error: false, declared_functions: Vec::new() }
    }

    pub fn declared_functions(&self) -> &[u32] {
        &self.declared_functions
    }

    /// `fail`: o offset é o do módulo (`m_offsetInSource + m_parser->offset()`).
    fn fail(&self, env: &Env<'_>, message: &str) -> PartialResult {
        Err(format!(
            "WebAssembly.Module doesn't parse at byte {}: {}",
            self.offset_in_source + env.offset,
            message
        ))
    }

    fn stub(&self, env: &Env<'_>) -> PartialResult {
        self.fail(env, "Invalid instruction for constant expression")
    }
}

impl Context for ConstExprGenerator {
    type Control = ConstExprControl;

    const VALIDATE_FUNCTION_BODY_SIZE: bool = false;
    const REF_FUNC_NEEDS_DECLARATION: bool = false;

    fn make_control(&mut self, _block_type: BlockType, signature: BlockSignature) -> ConstExprControl {
        ConstExprControl { signature }
    }

    fn add(&mut self, env: &Env<'_>, hook: Hook) -> PartialResult {
        match hook {
            Hook::GetGlobal(index) => {
                // Note that this check works for table initializers too, because no globals are registered when the table section is read and the count is 0.
                let globals = &env.info.globals;
                if index as usize >= globals.len() {
                    return self.fail(
                        env,
                        &format!("get_global's index {} exceeds the number of globals {}", index, globals.len()),
                    );
                }
                if globals[index as usize].mutability != Mutability::Immutable {
                    return self.fail(env, &format!("get_global import kind index {} is mutable ", index));
                }
                Ok(())
            }
            Hook::RefFunc(index) => {
                self.declared_functions.push(index);
                Ok(())
            }
            Hook::EndBlock => Ok(()),
            // `addSIMDConstant`: o único `addSIMD*` que não é `CONST_EXPR_STUB`.
            Hook::Simd(ExtSimdOpType::V128Const) => Ok(()),
            Hook::Op(
                OpType::I32Add | OpType::I64Add | OpType::I32Sub | OpType::I64Sub | OpType::I32Mul | OpType::I64Mul,
            ) => Ok(()),
            Hook::ExtGC(
                ExtGCOpType::RefI31
                | ExtGCOpType::ArrayNew
                | ExtGCOpType::ArrayNewDefault
                | ExtGCOpType::ArrayNewFixed
                | ExtGCOpType::StructNew
                | ExtGCOpType::StructNewDefault
                | ExtGCOpType::AnyConvertExtern
                | ExtGCOpType::ExternConvertAny,
            ) => Ok(()),
            _ => self.stub(env),
        }
    }

    fn did_parse_opcode(&mut self, opcode: OpType) {
        if opcode == OpType::Nop {
            self.should_error = true;
        }
    }

    fn end_top_level(&mut self, env: &Env<'_>) -> PartialResult {
        // Some opcodes like "nop" are not detectable by an error stub because the context
        // doesn't get called by the parser. This flag is set by didParseOpcode() to signal
        // such cases.
        if self.should_error {
            return self.fail(env, "Invalid instruction for constant expression");
        }
        Ok(())
    }
}

/// `parseExtendedConstExpr`: valida a expressão que começa em `source[0]` e devolve quantos bytes
/// ela ocupa (o `offset` de saída do C++). As funções que `ref.func` citou passam a declaradas.
pub fn parse_extended_const_expr(
    source: &[u8],
    offset_in_source: usize,
    info: &mut ModuleInformation,
    expected_type: Type,
) -> Result<usize, String> {
    let mut generator = ConstExprGenerator::new(offset_in_source);
    let length = {
        let mut parser = FunctionParser::new(&mut generator, source, BlockSignature::Type(expected_type), &*info);
        parser.parse_constant_expression()?;
        parser.offset()
    };
    for index in generator.declared_functions() {
        info.add_declared_function(*index as usize);
    }
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::wasm_format::{GlobalInformation, TYPE_I32, TYPE_I64};

    fn info_with_globals() -> ModuleInformation {
        let mut info = ModuleInformation::default();
        // global 0: i32 imutável, global 1: i32 mutável, global 2: i64 imutável.
        info.globals.push(GlobalInformation::new(TYPE_I32, Mutability::Immutable));
        info.globals.push(GlobalInformation::new(TYPE_I32, Mutability::Mutable));
        info.globals.push(GlobalInformation::new(TYPE_I64, Mutability::Immutable));
        info
    }

    #[test]
    fn i32_arithmetic_is_a_valid_extended_expression() {
        let mut info = info_with_globals();
        // i32.const 1; i32.const 2; i32.add; end
        let bytes = [0x41, 0x01, 0x41, 0x02, 0x6a, 0x0b, 0xff];
        let length = parse_extended_const_expr(&bytes, 0, &mut info, TYPE_I32).unwrap();
        assert_eq!(length, 6);
    }

    #[test]
    fn get_global_and_i64_mul() {
        let mut info = info_with_globals();
        // global.get 2; i64.const 3; i64.mul; end
        let bytes = [0x23, 0x02, 0x42, 0x03, 0x7e, 0x0b];
        assert_eq!(parse_extended_const_expr(&bytes, 0, &mut info, TYPE_I64), Ok(6));
    }

    #[test]
    fn mutable_global_is_rejected() {
        let mut info = info_with_globals();
        // O C++ rejeita pelo parser? Não: quem confere a mutabilidade é o contexto, com o offset do módulo.
        let bytes = [0x23, 0x01, 0x41, 0x01, 0x6a, 0x0b];
        let error = parse_extended_const_expr(&bytes, 100, &mut info, TYPE_I32).unwrap_err();
        assert_eq!(error, "WebAssembly.Module doesn't parse at byte 102: get_global import kind index 1 is mutable ");
    }

    #[test]
    fn unknown_global_fails_in_the_parser() {
        let mut info = info_with_globals();
        let bytes = [0x23, 0x09, 0x0b];
        let error = parse_extended_const_expr(&bytes, 0, &mut info, TYPE_I32).unwrap_err();
        assert_eq!(error, "WebAssembly.Module doesn't validate: 9 of unknown global, limit is 3");
    }

    #[test]
    fn division_is_not_a_constant_instruction() {
        let mut info = info_with_globals();
        // i32.const 4; i32.const 2; i32.div_s; end
        let bytes = [0x41, 0x04, 0x41, 0x02, 0x6d, 0x0b];
        let error = parse_extended_const_expr(&bytes, 7, &mut info, TYPE_I32).unwrap_err();
        assert_eq!(error, "WebAssembly.Module doesn't parse at byte 12: Invalid instruction for constant expression");
    }

    #[test]
    fn nop_is_caught_at_the_end() {
        let mut info = info_with_globals();
        let bytes = [0x41, 0x01, 0x01, 0x41, 0x01, 0x6a, 0x0b];
        let error = parse_extended_const_expr(&bytes, 0, &mut info, TYPE_I32).unwrap_err();
        assert_eq!(error, "WebAssembly.Module doesn't parse at byte 7: Invalid instruction for constant expression");
    }

    #[test]
    fn result_type_mismatch_is_a_validation_error() {
        let mut info = info_with_globals();
        // i64.const 1; i32.const 1; i32.add; end com resultado esperado i32: o i64 sobra na pilha.
        let bytes = [0x42, 0x01, 0x41, 0x01, 0x6a, 0x0b];
        let error = parse_extended_const_expr(&bytes, 0, &mut info, TYPE_I32).unwrap_err();
        assert_eq!(error, "WebAssembly.Module doesn't validate: I32Add left value type mismatch");
    }

    #[test]
    fn wrong_result_type_is_rejected() {
        let mut info = info_with_globals();
        let bytes = [0x41, 0x01, 0x41, 0x01, 0x6a, 0x0b];
        let error = parse_extended_const_expr(&bytes, 0, &mut info, TYPE_I64).unwrap_err();
        assert_eq!(
            error,
            "WebAssembly.Module doesn't validate: control flow returns with unexpected type. I32 is not a I64"
        );
    }

    #[test]
    fn ref_func_marks_the_function_as_declared() {
        let mut info = ModuleInformation::default();
        info.append_recursion_group(vec![(
            crate::wasm::wasm_module_information::StructuralType::Function { arguments: vec![], returns: vec![] },
            None,
        )]);
        info.internal_function_type_signature_indices.push(0);
        // ref.func 0; end. O tipo é `(ref <tipo da função>)`, subtipo de `funcref`.
        let bytes = [0xd2, 0x00, 0x0b];
        let funcref = crate::wasm::wasm_format::funcref_type();
        assert!(!info.is_declared_function(0));
        assert!(parse_extended_const_expr(&bytes, 0, &mut info, funcref).is_ok());
        assert!(info.is_declared_function(0));
    }
}
