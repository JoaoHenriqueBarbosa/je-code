//! Parte de `wasm/WasmFunctionParser.h`: as instruções SIMD (`0xFD`), `simd<isReachable>`, nos dois
//! modos (código alcançável e inalcançável). Ver `wasm_function_parser.rs` para as diferenças em
//! relação ao C++.
//!
//! `Context::tierSupportsSIMD()` é verdadeiro no `IPIntGenerator` e no `ConstExprGenerator`, os dois
//! contextos que existem, então o ramo `pushUnreachable` ("Appease generators without SIMD support")
//! não foi portado. O quinto argumento das comparações (`optionalRelation`, um `B3::Air::Arg`) só
//! serve ao BBQ e ao OMG e não existe aqui.

use crate::runtime::options::Options;
use crate::wasm::wasm_format::{TYPE_I32, TYPE_V128, Type};
use crate::wasm::wasm_function_parser::{
    Context, FunctionParser, Hook, PartialResult, parse_or_fail, pfail_if, pop_or_fail, vfail_if,
};
use crate::wasm::wasm_simd_opcodes::{ExtSimdOpType, SimdLaneOperation as Op};

impl<'s, 'i, C: Context> FunctionParser<'s, 'i, C> {
    /// O `case ExtSIMD` de `parseExpression` (`reachable`) e de `parseUnreachableExpression`.
    pub(super) fn parse_ext_simd(&mut self, reachable: bool) -> PartialResult {
        pfail_if!(self, !self.parser.use_wasm_simd, "wasm-simd is not enabled");
        self.context.notify_function_uses_simd();
        self.current_ext_op = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse wasm extended opcode");
        let extended = self.current_ext_op;
        let Some(ext) = ExtSimdOpType::from_value(extended) else {
            return self.pfail(format!("invalid extended simd op {}", extended));
        };
        self.simd(ext, reachable)
    }

    /// `parseImmLaneIdx`: devolve o índice da lane.
    fn parse_imm_lane_idx(&mut self, lane_count: u8) -> Result<u8, String> {
        assert!(matches!(lane_count, 2 | 4 | 8 | 16 | 32));
        let result = parse_or_fail!(self, self.parser.parse_uint8(), "Could not parse the lane index immediate byte.");
        pfail_if!(
            self,
            result >= lane_count,
            "Lane index immediate is too large, saw {}, expected an ImmLaneIdx{}",
            lane_count,
            lane_count
        );
        Ok(result)
    }

    /// A chamada `add*` do contexto e o empilhamento do resultado (`constructAndAppend`).
    fn simd_push(&mut self, ext: ExtSimdOpType, result: Type) -> PartialResult {
        self.hook(Hook::Simd(ext))?;
        self.expression_stack.push(result);
        Ok(())
    }

    /// O lambda `parseMemOp` de `simd`.
    fn simd_mem_op(&mut self, op: Op, reachable: bool) -> PartialResult {
        let max_alignment: u32 = match op {
            Op::LoadLane8 | Op::StoreLane8 | Op::LoadSplat8 => 0,
            Op::LoadLane16 | Op::StoreLane16 | Op::LoadSplat16 => 1,
            Op::LoadLane32 | Op::StoreLane32 | Op::LoadSplat32 | Op::LoadPad32 => 2,
            Op::LoadLane64
            | Op::StoreLane64
            | Op::LoadSplat64
            | Op::LoadPad64
            | Op::LoadExtend8U
            | Op::LoadExtend8S
            | Op::LoadExtend16U
            | Op::LoadExtend16S
            | Op::LoadExtend32U
            | Op::LoadExtend32S => 3,
            Op::Load | Op::Store => 4,
            _ => unreachable!("operação SIMD sem memória"),
        };

        vfail_if!(self, self.info.memory_count() == 0, "simd memory instructions need a memory defined in the module");

        let alignment = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get simd memory op alignment");
        let (alignment, memory_index) = self.parse_memory_index_and_fixup_alignment(alignment)?;
        self.parse_memory_offset(memory_index)?;

        vfail_if!(
            self,
            alignment > max_alignment,
            "alignment: {} can't be larger than max alignment for simd operation: {}",
            alignment,
            max_alignment
        );

        if !reachable {
            return Ok(());
        }

        let pointer = pop_or_fail!(self, "simd memory op pointer");
        let address_kind = self.info.memories[memory_index as usize].address_type.as_wasm_type_kind();
        vfail_if!(self, pointer.kind != address_kind, "pointer type mismatch");
        Ok(())
    }

    /// `simd<isReachable>`.
    fn simd(&mut self, ext: ExtSimdOpType, reachable: bool) -> PartialResult {
        let (op, lane, _sign_mode) = ext.info();
        self.context.notify_function_uses_simd();

        if op.is_relaxed() {
            pfail_if!(
                self,
                !Options::with(|options| options.use_wasm_relaxed_simd),
                "relaxed simd instructions not supported"
            );
        }

        match op {
            Op::Const => {
                parse_or_fail!(self, self.parser.parse_imm_byte_array16(), "can't parse 128-bit vector constant");
                if !reachable {
                    return Ok(());
                }
                self.simd_push(ext, TYPE_V128)
            }
            Op::Splat => {
                if !reachable {
                    return Ok(());
                }
                let scalar = pop_or_fail!(self, "select condition");
                let ok_type = lane.scalar_type().kind == scalar.kind;
                vfail_if!(self, !ok_type, "Wrong type to SIMD splat");
                self.simd_push(ext, TYPE_V128)
            }
            Op::Shr | Op::Shl => {
                if !reachable {
                    return Ok(());
                }
                let shift = pop_or_fail!(self, "shift i32");
                let vector = pop_or_fail!(self, "shift vector");
                vfail_if!(self, !vector.is_v128(), "Shift vector must be v128");
                vfail_if!(self, shift.kind != TYPE_I32.kind, "Shift amount must be i32");
                self.simd_push(ext, TYPE_V128)
            }
            Op::ExtmulLow | Op::ExtmulHigh => {
                if !reachable {
                    return Ok(());
                }
                let rhs = pop_or_fail!(self, "rhs");
                let lhs = pop_or_fail!(self, "lhs");
                vfail_if!(self, !lhs.is_v128(), "extmul lhs vector must be v128");
                vfail_if!(self, !rhs.is_v128(), "extmul rhs vector must be v128");
                self.simd_push(ext, TYPE_V128)
            }
            Op::LoadSplat8 | Op::LoadSplat16 | Op::LoadSplat32 | Op::LoadSplat64 | Op::Load => {
                self.simd_mem_op(op, reachable)?;
                if !reachable {
                    return Ok(());
                }
                self.simd_push(ext, TYPE_V128)
            }
            Op::Store => {
                if reachable {
                    let value = pop_or_fail!(self, "val");
                    vfail_if!(self, !value.is_v128(), "store vector must be v128");
                }
                self.simd_mem_op(op, reachable)?;
                if !reachable {
                    return Ok(());
                }
                self.hook(Hook::Simd(ext))
            }
            Op::LoadLane8 | Op::LoadLane16 | Op::LoadLane32 | Op::LoadLane64 => {
                let lane_count = match op {
                    Op::LoadLane8 => 16,
                    Op::LoadLane16 => 8,
                    Op::LoadLane32 => 4,
                    _ => 2,
                };
                if reachable {
                    let vector = pop_or_fail!(self, "vector");
                    vfail_if!(self, !vector.is_v128(), "load_lane input must be a vector");
                }
                self.simd_mem_op(op, reachable)?;
                self.parse_imm_lane_idx(lane_count)?;
                if !reachable {
                    return Ok(());
                }
                self.simd_push(ext, TYPE_V128)
            }
            Op::StoreLane8 | Op::StoreLane16 | Op::StoreLane32 | Op::StoreLane64 => {
                let lane_count = match op {
                    Op::StoreLane8 => 16,
                    Op::StoreLane16 => 8,
                    Op::StoreLane32 => 4,
                    _ => 2,
                };
                if reachable {
                    let vector = pop_or_fail!(self, "vector");
                    vfail_if!(self, !vector.is_v128(), "store_lane input must be a vector");
                }
                self.simd_mem_op(op, reachable)?;
                self.parse_imm_lane_idx(lane_count)?;
                if !reachable {
                    return Ok(());
                }
                self.hook(Hook::Simd(ext))
            }
            Op::LoadExtend8U
            | Op::LoadExtend8S
            | Op::LoadExtend16U
            | Op::LoadExtend16S
            | Op::LoadExtend32U
            | Op::LoadExtend32S
            | Op::LoadPad32
            | Op::LoadPad64 => {
                self.simd_mem_op(op, reachable)?;
                if !reachable {
                    return Ok(());
                }
                self.simd_push(ext, TYPE_V128)
            }
            Op::Shuffle => {
                let immediate =
                    parse_or_fail!(self, self.parser.parse_imm_byte_array16(), "can't parse 128-bit shuffle immediate");
                for byte in immediate {
                    // `WASM_PARSER_FAIL_IF(condition)`: sem mensagem.
                    pfail_if!(self, byte >= 2 * lane.element_count(), "");
                }
                if !reachable {
                    return Ok(());
                }
                let b = pop_or_fail!(self, "vector argument");
                vfail_if!(self, !b.is_v128(), "shuffle input must be a vector");
                let a = pop_or_fail!(self, "vector argument");
                vfail_if!(self, !a.is_v128(), "shuffle input must be a vector");
                self.simd_push(ext, TYPE_V128)
            }
            Op::ExtractLane => {
                self.parse_imm_lane_idx(lane.element_count())?;
                if !reachable {
                    return Ok(());
                }
                let vector = pop_or_fail!(self, "vector argument");
                vfail_if!(self, vector != TYPE_V128, "type mismatch for argument 0");
                self.simd_push(ext, lane.scalar_type())
            }
            Op::ReplaceLane => {
                self.parse_imm_lane_idx(lane.element_count())?;
                if !reachable {
                    return Ok(());
                }
                let scalar = pop_or_fail!(self, "scalar argument");
                let vector = pop_or_fail!(self, "vector argument");
                vfail_if!(self, vector != TYPE_V128, "type mismatch for argument 1");
                vfail_if!(self, scalar != lane.scalar_type(), "type mismatch for argument 0");
                self.simd_push(ext, TYPE_V128)
            }
            Op::Bitmask | Op::AnyTrue | Op::AllTrue => {
                if !reachable {
                    return Ok(());
                }
                let vector = pop_or_fail!(self, "vector argument");
                vfail_if!(self, vector != TYPE_V128, "type mismatch for argument 0");
                self.simd_push(ext, TYPE_I32)
            }
            Op::ExtaddPairwise
            | Op::Convert
            | Op::ConvertLow
            | Op::ExtendHigh
            | Op::ExtendLow
            | Op::TruncSat
            | Op::RelaxedTruncSat
            | Op::Not
            | Op::Demote
            | Op::Promote
            | Op::Abs
            | Op::Neg
            | Op::Popcnt
            | Op::Ceil
            | Op::Floor
            | Op::Trunc
            | Op::Nearest
            | Op::Sqrt => {
                if !reachable {
                    return Ok(());
                }
                let vector = pop_or_fail!(self, "vector argument");
                vfail_if!(self, vector != TYPE_V128, "type mismatch for argument 0");
                self.simd_push(ext, TYPE_V128)
            }
            Op::BitwiseSelect | Op::RelaxedLaneSelect => {
                if !reachable {
                    return Ok(());
                }
                let c = pop_or_fail!(self, "vector argument");
                let v2 = pop_or_fail!(self, "vector argument");
                let v1 = pop_or_fail!(self, "vector argument");
                vfail_if!(self, v1 != TYPE_V128, "type mismatch for argument 2");
                vfail_if!(self, v2 != TYPE_V128, "type mismatch for argument 1");
                vfail_if!(self, c != TYPE_V128, "type mismatch for argument 0");
                self.simd_push(ext, TYPE_V128)
            }
            Op::GreaterThan
            | Op::GreaterThanOrEqual
            | Op::LessThan
            | Op::LessThanOrEqual
            | Op::Equal
            | Op::NotEqual => {
                if !reachable {
                    return Ok(());
                }
                let rhs = pop_or_fail!(self, "vector argument");
                let lhs = pop_or_fail!(self, "vector argument");
                vfail_if!(self, lhs != TYPE_V128, "type mismatch for argument 1");
                vfail_if!(self, rhs != TYPE_V128, "type mismatch for argument 0");
                self.simd_push(ext, TYPE_V128)
            }
            Op::And
            | Op::Andnot
            | Op::AvgRound
            | Op::DotProduct
            | Op::Add
            | Op::Mul
            | Op::MulSat
            | Op::Sub
            | Op::Div
            | Op::Pmax
            | Op::Pmin
            | Op::Or
            | Op::Swizzle
            | Op::RelaxedSwizzle
            | Op::Xor
            | Op::Narrow
            | Op::AddSat
            | Op::SubSat
            | Op::Max
            | Op::Min
            | Op::RelaxedMin
            | Op::RelaxedMax
            | Op::RelaxedQ15Mulr
            | Op::RelaxedDotI8x16I7x16 => {
                if !reachable {
                    return Ok(());
                }
                let b = pop_or_fail!(self, "vector argument");
                let a = pop_or_fail!(self, "vector argument");
                vfail_if!(self, a != TYPE_V128, "type mismatch for argument 1");
                vfail_if!(self, b != TYPE_V128, "type mismatch for argument 0");
                self.simd_push(ext, TYPE_V128)
            }
            Op::RelaxedMAdd | Op::RelaxedNMAdd | Op::RelaxedDotI8x16I7x16Add => {
                if !reachable {
                    return Ok(());
                }
                let c = pop_or_fail!(self, "vector argument");
                let b = pop_or_fail!(self, "vector argument");
                let a = pop_or_fail!(self, "vector argument");
                vfail_if!(self, a != TYPE_V128, "type mismatch for argument 0");
                vfail_if!(self, b != TYPE_V128, "type mismatch for argument 1");
                vfail_if!(self, c != TYPE_V128, "type mismatch for argument 2");
                self.simd_push(ext, TYPE_V128)
            }
        }
    }
}
