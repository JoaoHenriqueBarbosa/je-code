//! O `Context` que só valida: o papel que `IPIntGenerator` cumpre na validação de um módulo, sem
//! gerar nada. Aceita toda instrução que o `FunctionParser` já conferiu.

use std::collections::HashSet;

use crate::wasm::wasm_function_parser::{
    BlockSignature, BlockType, Context, Env, FunctionParser, Hook, PartialResult, StandardControl,
};
use crate::wasm::wasm_module_information::ModuleInformation;
use crate::wasm::wasm_ops::OpType;

/// O que a validação de um corpo de função descobriu (o que `FunctionData` guarda).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FunctionValidation {
    pub uses_simd: bool,
    pub uses_legacy_exceptions: bool,
    pub uses_modern_exceptions: bool,
}

/// O gerador que não gera.
#[derive(Default)]
pub struct FunctionValidator {
    uses_simd: bool,
    /// Os deslocamentos (depois do opcode e dos imediatos) dos `drop` e `select` cujo operando é `v128`.
    wide_operands: HashSet<usize>,
}

impl Context for FunctionValidator {
    type Control = StandardControl;

    const VALIDATE_FUNCTION_BODY_SIZE: bool = true;
    const REF_FUNC_NEEDS_DECLARATION: bool = true;

    fn make_control(&mut self, block_type: BlockType, signature: BlockSignature) -> StandardControl {
        StandardControl::new(block_type, signature)
    }

    fn add(&mut self, env: &Env<'_>, hook: Hook) -> PartialResult {
        if matches!(hook, Hook::Op(OpType::Drop | OpType::Select)) && env.operand.is_some_and(|ty| ty.is_v128()) {
            self.wide_operands.insert(env.offset);
        }
        Ok(())
    }

    fn notify_function_uses_simd(&mut self) {
        self.uses_simd = true;
    }
}

/// Valida o corpo de uma função (locais e instruções, sem o tamanho) contra o tipo na posição
/// `signature_position` da seção de tipos.
pub fn validate_function(
    info: &ModuleInformation,
    body: &[u8],
    signature_position: u32,
) -> Result<FunctionValidation, String> {
    validate_function_with_wide_operands(info, body, signature_position).map(|(validation, _)| validation)
}

/// Como `validate_function`, e também os deslocamentos dos `drop`/`select` de `v128` (a pilha de tipos
/// do validador responde o que o interpretador precisa saber: quantos slots o operando ocupa).
pub fn validate_function_with_wide_operands(
    info: &ModuleInformation,
    body: &[u8],
    signature_position: u32,
) -> Result<(FunctionValidation, HashSet<usize>), String> {
    let Some(signature) = BlockSignature::from_position(info, signature_position) else {
        return Err("WebAssembly.Module doesn't parse at byte 0: type signature was not a function signature".to_string());
    };
    let mut validator = FunctionValidator::default();
    let (legacy, modern) = {
        let mut parser = FunctionParser::new(&mut validator, body, signature, info);
        parser.parse()?;
        (parser.uses_legacy_exceptions, parser.uses_modern_exceptions)
    };
    let validation = FunctionValidation {
        uses_simd: validator.uses_simd,
        uses_legacy_exceptions: legacy,
        uses_modern_exceptions: modern,
    };
    Ok((validation, validator.wide_operands))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::page_count::PageCount;
    use crate::wasm::wasm_format::{TYPE_I32, TYPE_I64};
    use crate::wasm::wasm_memory_information::MemoryInformation;
    use crate::wasm::wasm_module_information::StructuralType;

    const VALIDATE: &str = "WebAssembly.Module doesn't validate: ";
    const PARSE: &str = "WebAssembly.Module doesn't parse at byte ";

    /// Tipos: 0 = (i32) -> (i32), 1 = () -> (i32), 2 = () -> (), 3 = (i32, i32) -> (i32),
    /// 4 = (i32) -> (i64). A função 0 do módulo tem o tipo 0.
    fn module() -> ModuleInformation {
        let mut info = ModuleInformation::default();
        for (arguments, returns) in [
            (vec![TYPE_I32], vec![TYPE_I32]),
            (vec![], vec![TYPE_I32]),
            (vec![], vec![]),
            (vec![TYPE_I32, TYPE_I32], vec![TYPE_I32]),
            (vec![TYPE_I32], vec![TYPE_I64]),
        ] {
            info.append_recursion_group(vec![(StructuralType::Function { arguments, returns }, None)]);
        }
        info.internal_function_type_signature_indices.push(0);
        info
    }

    fn with_memory(mut info: ModuleInformation) -> ModuleInformation {
        info.memories.push(MemoryInformation::new(PageCount::new(1), PageCount::default(), false, false, false));
        info
    }

    #[test]
    fn straight_line_arithmetic() {
        // local.get 0; local.get 1; i32.add; end
        let body = [0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b];
        assert_eq!(validate_function(&module(), &body, 3), Ok(FunctionValidation::default()));
    }

    #[test]
    fn locals_and_tee() {
        // 1 grupo de 2 locais i32; i32.const 5; local.tee 1; end (o parâmetro é o local 0)
        let body = [0x01, 0x02, 0x7f, 0x41, 0x05, 0x22, 0x01, 0x0b];
        assert!(validate_function(&module(), &body, 0).is_ok());
    }

    #[test]
    fn wrong_result_type() {
        // (i32) -> (i64): local.get 0; end
        let body = [0x00, 0x20, 0x00, 0x0b];
        assert_eq!(
            validate_function(&module(), &body, 4),
            Err(format!("{}control flow returns with unexpected type. I32 is not a I64", VALIDATE))
        );
    }

    #[test]
    fn block_with_unconditional_branch() {
        // block (result i32); i32.const 7; br 0; i32.const 8; end; end
        let body = [0x00, 0x02, 0x7f, 0x41, 0x07, 0x0c, 0x00, 0x41, 0x08, 0x0b, 0x0b];
        assert!(validate_function(&module(), &body, 1).is_ok());
    }

    #[test]
    fn if_else_with_results() {
        // local.get 0; if (result i32); i32.const 1; else; i32.const 2; end; end
        let body = [0x00, 0x20, 0x00, 0x04, 0x7f, 0x41, 0x01, 0x05, 0x41, 0x02, 0x0b, 0x0b];
        assert!(validate_function(&module(), &body, 0).is_ok());
    }

    #[test]
    fn if_without_else_needs_matching_types() {
        // (i32) -> (i32): local.get 0; if (result i32); i32.const 1; end; end
        let body = [0x00, 0x20, 0x00, 0x04, 0x7f, 0x41, 0x01, 0x0b, 0x0b];
        // O `if` sem `else` precisa que os argumentos virem os resultados: (), -> i32 não fecha.
        let error = validate_function(&module(), &body, 0).unwrap_err();
        assert!(error.starts_with(VALIDATE), "{error}");
    }

    #[test]
    fn loop_with_conditional_branch() {
        // (i32) -> (i32): loop; local.get 0; br_if 0; end; local.get 0; end
        let body = [0x00, 0x03, 0x40, 0x20, 0x00, 0x0d, 0x00, 0x0b, 0x20, 0x00, 0x0b];
        assert!(validate_function(&module(), &body, 0).is_ok());
    }

    #[test]
    fn stack_underflow_is_a_parse_error() {
        // () -> (): i32.add sem operandos
        let body = [0x00, 0x6a, 0x0b];
        assert_eq!(
            validate_function(&module(), &body, 2),
            Err(format!("{}2: can't pop empty stack in binary right", PARSE))
        );
    }

    #[test]
    fn invalid_opcode() {
        let body = [0x00, 0x16];
        assert_eq!(validate_function(&module(), &body, 2), Err(format!("{}2: invalid opcode 22", PARSE)));
    }

    #[test]
    fn trailing_bytes_after_the_final_end() {
        let body = [0x00, 0x0b, 0xff];
        assert_eq!(
            validate_function(&module(), &body, 2),
            Err(format!("{}2: function body size doesn't match the expected size", PARSE))
        );
    }

    #[test]
    fn load_needs_a_memory() {
        // i32.const 0; i32.load align=2 offset=0; drop; end
        let body = [0x00, 0x41, 0x00, 0x28, 0x02, 0x00, 0x1a, 0x0b];
        assert_eq!(
            validate_function(&module(), &body, 2),
            Err(format!("{}load instruction without memory", VALIDATE))
        );
        assert!(validate_function(&with_memory(module()), &body, 2).is_ok());
    }

    #[test]
    fn load_alignment_cannot_exceed_the_natural_one() {
        let body = [0x00, 0x41, 0x00, 0x28, 0x03, 0x00, 0x1a, 0x0b];
        assert_eq!(
            validate_function(&with_memory(module()), &body, 2),
            Err(format!("{}5: byte alignment 8 exceeds load's natural alignment 4", PARSE))
        );
    }

    #[test]
    fn store_checks_the_value_type() {
        // i32.const 0; i64.const 1; i32.store; end
        let body = [0x00, 0x41, 0x00, 0x42, 0x01, 0x36, 0x02, 0x00, 0x0b];
        assert_eq!(
            validate_function(&with_memory(module()), &body, 2),
            Err(format!("{}I32Store value type mismatch", VALIDATE))
        );
    }

    #[test]
    fn direct_call_checks_arguments_and_pushes_results() {
        // () -> (i32): i32.const 5; call 0; end
        let body = [0x00, 0x41, 0x05, 0x10, 0x00, 0x0b];
        assert!(validate_function(&module(), &body, 1).is_ok());
        // call sem argumento
        let body = [0x00, 0x10, 0x00, 0x0b];
        assert_eq!(
            validate_function(&module(), &body, 1),
            Err(format!(
                "{}3: call function index 0 has 1 arguments, but the expression stack currently holds 0 values",
                PARSE
            ))
        );
        // índice de função fora do espaço
        let body = [0x00, 0x10, 0x05, 0x0b];
        assert_eq!(
            validate_function(&module(), &body, 2),
            Err(format!("{}3: function index 5 exceeds function index space 1", PARSE))
        );
    }

    #[test]
    fn select_and_drop() {
        // () -> (i32): i32.const 1; i32.const 2; i32.const 0; select; end
        let body = [0x00, 0x41, 0x01, 0x41, 0x02, 0x41, 0x00, 0x1b, 0x0b];
        assert!(validate_function(&module(), &body, 1).is_ok());
    }

    #[test]
    fn unreachable_code_skips_type_checks() {
        // () -> (i32): unreachable; i32.add (sem operandos, mas é código morto); end
        let body = [0x00, 0x00, 0x6a, 0x0b];
        assert!(validate_function(&module(), &body, 1).is_ok());
    }

    #[test]
    fn br_table_targets_must_agree() {
        // () -> (i32): block (result i32); i32.const 1; i32.const 0; br_table [0] default 1; end; end
        let body = [0x00, 0x02, 0x7f, 0x41, 0x01, 0x41, 0x00, 0x0e, 0x01, 0x00, 0x01, 0x0b, 0x0b];
        assert!(validate_function(&module(), &body, 1).is_ok());
    }

    #[test]
    fn bad_block_signature_index() {
        // block com índice de tipo 99
        let body = [0x00, 0x02, 0x63, 0x0b];
        let error = validate_function(&module(), &body, 2).unwrap_err();
        assert!(error.starts_with(PARSE), "{error}");
    }

    #[test]
    fn locals_limit() {
        // um grupo com 60000 locais i32 passa do máximo de 50000
        let body = [0x01, 0xe0, 0xd4, 0x03, 0x7f, 0x0b];
        assert_eq!(
            validate_function(&module(), &body, 2),
            Err(format!("{}4: Function's number of locals is too big 60000 maximum 50000", PARSE))
        );
    }

    /// `v128.const` com os 16 bytes iguais a `fill`, no meio de um corpo.
    fn v128_const(fill: u8) -> Vec<u8> {
        let mut bytes = vec![0xfd, 0x0c];
        bytes.extend_from_slice(&[fill; 16]);
        bytes
    }

    /// Um corpo `() -> ()` sem locais: o prefixo, as instruções, e `end`.
    fn body_of(instructions: &[&[u8]]) -> Vec<u8> {
        let mut body = vec![0x00];
        for instruction in instructions {
            body.extend_from_slice(instruction);
        }
        body.push(0x0b);
        body
    }

    #[test]
    fn simd_constant_and_arithmetic() {
        let constant = v128_const(1);
        // v128.const; drop; end
        let body = body_of(&[&constant, &[0x1a]]);
        assert_eq!(
            validate_function(&module(), &body, 2),
            Ok(FunctionValidation { uses_simd: true, ..FunctionValidation::default() })
        );
        // v128.const; v128.const; i32x4.add (0xae 0x01); drop; end
        let body = body_of(&[&constant, &constant, &[0xfd, 0xae, 0x01], &[0x1a]]);
        assert!(validate_function(&module(), &body, 2).is_ok());
        // i32.const 1; i32x4.splat; i32x4.extract_lane 3; drop; end
        let body = body_of(&[&[0x41, 0x01], &[0xfd, 0x11], &[0xfd, 0x1b, 0x03], &[0x1a]]);
        assert!(validate_function(&module(), &body, 2).is_ok());
    }

    #[test]
    fn simd_scalar_and_lane_checks() {
        // i64.const 1; i32x4.splat
        let body = body_of(&[&[0x42, 0x01], &[0xfd, 0x11], &[0x1a]]);
        assert_eq!(validate_function(&module(), &body, 2), Err(format!("{}Wrong type to SIMD splat", VALIDATE)));
        // v128.const; i32x4.extract_lane 4: o índice é 0..4
        let body = body_of(&[&v128_const(0), &[0xfd, 0x1b, 0x04], &[0x1a]]);
        assert_eq!(
            validate_function(&module(), &body, 2),
            Err(format!("{}22: Lane index immediate is too large, saw 4, expected an ImmLaneIdx4", PARSE))
        );
        // i32.const 0; v128.const; i32x4.add: o operando da esquerda é um i32
        let body = body_of(&[&[0x41, 0x00], &v128_const(0), &[0xfd, 0xae, 0x01], &[0x1a]]);
        assert_eq!(validate_function(&module(), &body, 2), Err(format!("{}type mismatch for argument 1", VALIDATE)));
        // i32x4.replace_lane com o escalar errado: v128.const; i64.const 0; i32x4.replace_lane 0
        let body = body_of(&[&v128_const(0), &[0x42, 0x00], &[0xfd, 0x1c, 0x00], &[0x1a]]);
        assert_eq!(validate_function(&module(), &body, 2), Err(format!("{}type mismatch for argument 0", VALIDATE)));
    }

    #[test]
    fn simd_shuffle_immediate_is_bounded_by_two_vectors() {
        let mut shuffle = vec![0xfd, 0x0d];
        shuffle.extend_from_slice(&[31; 16]);
        let body = body_of(&[&v128_const(0), &v128_const(0), &shuffle, &[0x1a]]);
        assert!(validate_function(&module(), &body, 2).is_ok());
        // 32 não existe em i8x16.shuffle: a falha não leva mensagem.
        let mut bad = vec![0xfd, 0x0d];
        bad.extend_from_slice(&[32; 16]);
        let body = body_of(&[&bad]);
        assert_eq!(validate_function(&module(), &body, 2), Err(format!("{}19: ", PARSE)));
    }

    #[test]
    fn simd_memory_instructions() {
        // i32.const 0; v128.load align=4 offset=0; drop
        let body = body_of(&[&[0x41, 0x00], &[0xfd, 0x00, 0x04, 0x00], &[0x1a]]);
        assert_eq!(
            validate_function(&module(), &body, 2),
            Err(format!("{}simd memory instructions need a memory defined in the module", VALIDATE))
        );
        assert!(validate_function(&with_memory(module()), &body, 2).is_ok());
        // alinhamento 5 passa do máximo de v128.load
        let body = body_of(&[&[0x41, 0x00], &[0xfd, 0x00, 0x05, 0x00], &[0x1a]]);
        assert_eq!(
            validate_function(&with_memory(module()), &body, 2),
            Err(format!("{}alignment: 5 can't be larger than max alignment for simd operation: 4", VALIDATE))
        );
        // v128.store: i32.const 0; v128.const; v128.store
        let body = body_of(&[&[0x41, 0x00], &v128_const(0), &[0xfd, 0x0b, 0x04, 0x00]]);
        assert!(validate_function(&with_memory(module()), &body, 2).is_ok());
        // v128.load8_lane 0 de i32.const 0, v128.const: ordem (pointer, vector), lane 16 inválida
        let body = body_of(&[&[0x41, 0x00], &v128_const(0), &[0xfd, 0x54, 0x00, 0x00, 0x10], &[0x1a]]);
        let error = validate_function(&with_memory(module()), &body, 2).unwrap_err();
        assert!(error.ends_with("Lane index immediate is too large, saw 16, expected an ImmLaneIdx16"), "{error}");
        // v128.load32_splat com ponteiro i64 numa memória i32
        let body = body_of(&[&[0x42, 0x00], &[0xfd, 0x09, 0x02, 0x00], &[0x1a]]);
        assert_eq!(
            validate_function(&with_memory(module()), &body, 2),
            Err(format!("{}pointer type mismatch", VALIDATE))
        );
    }

    #[test]
    fn simd_in_unreachable_code_parses_immediates_without_types() {
        // unreachable; i32x4.extract_lane 3; i8x16.swizzle; end
        let body = body_of(&[&[0x00], &[0xfd, 0x1b, 0x03], &[0xfd, 0x0e]]);
        assert!(validate_function(&module(), &body, 2).is_ok());
        // O índice da lane também é conferido em código morto.
        let body = body_of(&[&[0x00], &[0xfd, 0x1b, 0x09]]);
        assert!(validate_function(&module(), &body, 2).unwrap_err().starts_with(PARSE));
    }

    #[test]
    fn unknown_simd_opcode() {
        // 0x114 é o primeiro valor depois do último opcode da tabela.
        let body = [0x00, 0xfd, 0x94, 0x02];
        assert_eq!(validate_function(&module(), &body, 2), Err(format!("{}4: invalid extended simd op 276", PARSE)));
        // 0x9a e 0xa2 são buracos da tabela do JSC.
        let body = [0x00, 0xfd, 0x9a, 0x01];
        assert_eq!(validate_function(&module(), &body, 2), Err(format!("{}4: invalid extended simd op 154", PARSE)));
    }
}
