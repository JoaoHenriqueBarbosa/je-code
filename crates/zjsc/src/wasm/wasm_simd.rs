//! Semântica das instruções SIMD de `v128` (prefixo 0xFD) sobre `u128`, como a especificação do WebAssembly
//! as define (e como o `WasmIPIntSIMD` do JSC as executa): aritmética por lane, bitwise, comparação, splat,
//! extract/replace lane, shuffle, swizzle, any_true, all_true, bitmask, load e store.
//!
//! O valor é um `u128` em ordem little-endian: a lane 0 ocupa os bits menos significativos. Uma lane viaja como
//! `u64` com os bits crus (zero-estendida), então um `f32` é `f32::to_bits` e um `i8` negativo vira o byte.
//! Falta ligar ao laço de `wasm_ipint.rs`, cuja pilha é de `u64` e precisa de dois slots por `v128` (locais,
//! globais, aridade de bloco). Ver `wip-notes/wasm-plan.md`.

use crate::wasm::wasm_simd_opcodes::{SimdLane, SimdLaneOperation, SimdSignMode};

/// Largura da lane em bits.
pub fn lane_bits(lane: SimdLane) -> u32 {
    // `v128` (not, and, or, xor, bitselect, any_true, load, store) não tem `elementCount` no C++ (é
    // `RELEASE_ASSERT_NOT_REACHED`), então quem trata o vetor inteiro nunca pergunta; aqui o vetor inteiro
    // conta como uma lane só de 128 bits.
    match lane {
        SimdLane::V128 => 128,
        _ => 128 / u32::from(lane.element_count()),
    }
}

fn lane_mask(lane: SimdLane) -> u128 {
    let bits = lane_bits(lane);
    if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 }
}

/// Lê a lane `index` (bits crus).
pub fn lane_get(lane: SimdLane, value: u128, index: u8) -> u64 {
    ((value >> (lane_bits(lane) * u32::from(index))) & lane_mask(lane)) as u64
}

/// Substitui a lane `index` pelos `bits` (truncados à largura da lane).
pub fn lane_set(lane: SimdLane, value: u128, index: u8, bits: u64) -> u128 {
    let shift = lane_bits(lane) * u32::from(index);
    (value & !(lane_mask(lane) << shift)) | ((u128::from(bits) & lane_mask(lane)) << shift)
}

/// `*.splat`.
pub fn splat(lane: SimdLane, bits: u64) -> u128 {
    (0..lane.element_count()).fold(0, |acc, index| lane_set(lane, acc, index, bits))
}

/// Estende o sinal de uma lane inteira para `i64`.
pub fn sign_extend(lane: SimdLane, bits: u64) -> i64 {
    let shift = 64 - lane_bits(lane);
    ((bits << shift) as i64) >> shift
}

/// `*.extract_lane` (e `_s`/`_u` para i8x16 e i16x8): devolve o valor da lane com o sinal pedido.
pub fn extract_lane(lane: SimdLane, value: u128, index: u8, signed: bool) -> u64 {
    let bits = lane_get(lane, value, index);
    match lane {
        SimdLane::I8x16 | SimdLane::I16x8 if signed => sign_extend(lane, bits) as u32 as u64,
        _ => bits,
    }
}

/// Aplica `operation` a cada par de lanes (bits crus) de `a` e `b`.
pub fn map2(lane: SimdLane, a: u128, b: u128, operation: impl Fn(u64, u64) -> u64) -> u128 {
    (0..lane.element_count()).fold(0, |acc, index| {
        lane_set(lane, acc, index, operation(lane_get(lane, a, index), lane_get(lane, b, index)))
    })
}

/// Aplica `operation` a cada lane de `a`.
pub fn map1(lane: SimdLane, a: u128, operation: impl Fn(u64) -> u64) -> u128 {
    (0..lane.element_count()).fold(0, |acc, index| lane_set(lane, acc, index, operation(lane_get(lane, a, index))))
}

/// Operações aritméticas por lane. `add`, `sub` e `mul` são as que o interpretador precisa primeiro; a
/// aritmética inteira dá a volta (`wrapping`), a de ponto flutuante segue o IEEE 754 do Rust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneArith {
    Add,
    Sub,
    Mul,
}

/// `i8x16|i16x8|i32x4|i64x2|f32x4|f64x2 .add|.sub|.mul`.
pub fn arith(lane: SimdLane, operation: LaneArith, a: u128, b: u128) -> u128 {
    match lane {
        SimdLane::F32x4 => map2(lane, a, b, |x, y| {
            let (x, y) = (f32::from_bits(x as u32), f32::from_bits(y as u32));
            u64::from(
                match operation {
                    LaneArith::Add => x + y,
                    LaneArith::Sub => x - y,
                    LaneArith::Mul => x * y,
                }
                .to_bits(),
            )
        }),
        SimdLane::F64x2 => map2(lane, a, b, |x, y| {
            let (x, y) = (f64::from_bits(x), f64::from_bits(y));
            match operation {
                LaneArith::Add => x + y,
                LaneArith::Sub => x - y,
                LaneArith::Mul => x * y,
            }
            .to_bits()
        }),
        _ => map2(lane, a, b, |x, y| match operation {
            LaneArith::Add => x.wrapping_add(y),
            LaneArith::Sub => x.wrapping_sub(y),
            LaneArith::Mul => x.wrapping_mul(y),
        }),
    }
}

/// Comparações por lane; o resultado é `todos os bits` (verdadeiro) ou zero em cada lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneCompare {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

fn compare_ordering<T: PartialOrd>(operation: LaneCompare, x: T, y: T) -> bool {
    match operation {
        LaneCompare::Eq => x == y,
        LaneCompare::Ne => x != y,
        LaneCompare::Lt => x < y,
        LaneCompare::Gt => x > y,
        LaneCompare::Le => x <= y,
        LaneCompare::Ge => x >= y,
    }
}

/// `*.eq|ne|lt|gt|le|ge` (com `_s`/`_u` nas inteiras). Em `f32x4` e `f64x2` o resultado é a máscara na
/// largura da lane inteira correspondente, mas a máscara é a mesma contagem de bits da lane, então
/// `map2` sobre a própria lane serve.
pub fn compare(lane: SimdLane, operation: LaneCompare, signed: bool, a: u128, b: u128) -> u128 {
    map2(lane, a, b, |x, y| {
        let holds = match lane {
            SimdLane::F32x4 => compare_ordering(operation, f32::from_bits(x as u32), f32::from_bits(y as u32)),
            SimdLane::F64x2 => compare_ordering(operation, f64::from_bits(x), f64::from_bits(y)),
            _ if signed => compare_ordering(operation, sign_extend(lane, x), sign_extend(lane, y)),
            _ => compare_ordering(operation, x, y),
        };
        if holds { u64::MAX } else { 0 }
    })
}

/// `v128.not`.
pub fn not(a: u128) -> u128 {
    !a
}

/// `v128.and`.
pub fn and(a: u128, b: u128) -> u128 {
    a & b
}

/// `v128.andnot`.
pub fn andnot(a: u128, b: u128) -> u128 {
    a & !b
}

/// `v128.or`.
pub fn or(a: u128, b: u128) -> u128 {
    a | b
}

/// `v128.xor`.
pub fn xor(a: u128, b: u128) -> u128 {
    a ^ b
}

/// `v128.bitselect`: os bits de `a` onde `mask` é 1 e os de `b` onde é 0.
pub fn bitselect(a: u128, b: u128, mask: u128) -> u128 {
    (a & mask) | (b & !mask)
}

/// `v128.any_true`.
pub fn any_true(a: u128) -> bool {
    a != 0
}

/// `*.all_true`.
pub fn all_true(lane: SimdLane, a: u128) -> bool {
    (0..lane.element_count()).all(|index| lane_get(lane, a, index) != 0)
}

/// `*.bitmask`: o bit de sinal de cada lane, a lane 0 no bit 0.
pub fn bitmask(lane: SimdLane, a: u128) -> u32 {
    let top = lane_bits(lane) - 1;
    (0..lane.element_count()).fold(0, |acc, index| acc | (((lane_get(lane, a, index) >> top) & 1) as u32) << index)
}

/// `i8x16.shuffle`: o índice `0..16` escolhe um byte de `a`, `16..32` um de `b`.
pub fn shuffle(a: u128, b: u128, indices: [u8; 16]) -> u128 {
    let (a, b) = (a.to_le_bytes(), b.to_le_bytes());
    let mut out = [0u8; 16];
    for (slot, index) in out.iter_mut().zip(indices) {
        *slot = if index < 16 { a[usize::from(index)] } else { b[usize::from(index - 16)] };
    }
    u128::from_le_bytes(out)
}

/// `i8x16.swizzle`: índice fora de `0..16` dá zero.
pub fn swizzle(a: u128, selector: u128) -> u128 {
    let (a, selector) = (a.to_le_bytes(), selector.to_le_bytes());
    let mut out = [0u8; 16];
    for (slot, index) in out.iter_mut().zip(selector) {
        *slot = if index < 16 { a[usize::from(index)] } else { 0 };
    }
    u128::from_le_bytes(out)
}

/// `v128.const`: os 16 bytes do imediato.
pub fn from_bytes(bytes: [u8; 16]) -> u128 {
    u128::from_le_bytes(bytes)
}

/// `v128.store`: os 16 bytes na ordem da memória.
pub fn to_bytes(value: u128) -> [u8; 16] {
    value.to_le_bytes()
}

/// Divide em dois slots `u64` da pilha do interpretador (baixo primeiro).
pub fn split(value: u128) -> (u64, u64) {
    (value as u64, (value >> 64) as u64)
}

/// Junta dois slots `u64` (baixo, alto) num `v128`.
pub fn join(low: u64, high: u64) -> u128 {
    u128::from(low) | (u128::from(high) << 64)
}

fn is_float(lane: SimdLane) -> bool {
    matches!(lane, SimdLane::F32x4 | SimdLane::F64x2)
}

/// A lane com metade da largura (a fonte de um `extend`, o resultado de um `narrow`).
fn narrower(lane: SimdLane) -> SimdLane {
    match lane {
        SimdLane::I64x2 => SimdLane::I32x4,
        SimdLane::I32x4 => SimdLane::I16x8,
        SimdLane::I16x8 => SimdLane::I8x16,
        _ => unreachable!("lane sem metade"),
    }
}

/// A lane com o dobro da largura.
fn wider(lane: SimdLane) -> SimdLane {
    match lane {
        SimdLane::I8x16 => SimdLane::I16x8,
        SimdLane::I16x8 => SimdLane::I32x4,
        SimdLane::I32x4 => SimdLane::I64x2,
        _ => unreachable!("lane sem dobro"),
    }
}

/// O valor inteiro da lane, com ou sem sinal, num `i128` que comporta todas as contas.
fn lane_value(lane: SimdLane, bits: u64, signed: bool) -> i128 {
    if signed { i128::from(sign_extend(lane, bits)) } else { i128::from(bits) }
}

fn lane_float(lane: SimdLane, bits: u64) -> f64 {
    match lane {
        SimdLane::F32x4 => f64::from(f32::from_bits(bits as u32)),
        _ => f64::from_bits(bits),
    }
}

fn float_lane(lane: SimdLane, value: f64) -> u64 {
    match lane {
        SimdLane::F32x4 => u64::from((value as f32).to_bits()),
        _ => value.to_bits(),
    }
}

/// `fmin` do WebAssembly: NaN propaga e `-0` é menor que `+0`. Com um NaN, o JSC (medido no bun, x86) devolve
/// sempre o NaN canônico com o bit de sinal ligado (`0xffc00000` em f32, `0xfff8000000000000` em f64).
fn wasm_min(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        -f64::NAN
    } else if x == y {
        if x.is_sign_negative() { x } else { y }
    } else if x < y {
        x
    } else {
        y
    }
}

/// `fmax` do WebAssembly: NaN propaga e `+0` é maior que `-0`. Com um NaN, o JSC (medido no bun) devolve o NaN
/// canônico com o sinal do primeiro operando NaN (inclusive para sNaN, que não sai com o bit quieto de `f32 -> f64`).
fn wasm_max(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        f64::NAN.copysign(if x.is_nan() { x } else { y })
    } else if x == y {

        if x.is_sign_positive() { x } else { y }
    } else if x > y {
        x
    } else {
        y
    }
}

/// Satura `value` na faixa de uma lane de `bits` bits, com ou sem sinal.
fn saturate(value: i128, bits: u32, signed: bool) -> u64 {
    let (low, high) = if signed { (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1) } else { (0, (1i128 << bits) - 1) };
    value.clamp(low, high) as u64
}

/// Operações de um operando `v128` (resultado `v128`): not, abs, neg, popcnt, sqrt, ceil, floor, trunc, nearest,
/// extend, extadd_pairwise, convert, trunc_sat, demote, promote. `lane` e `sign` vêm de `ExtSimdOpType::info`.
pub fn unary(operation: SimdLaneOperation, lane: SimdLane, sign: SimdSignMode, a: u128) -> Option<u128> {
    use SimdLaneOperation as Op;
    let signed = sign == SimdSignMode::Signed;
    let top = 1u64.checked_shl(lane_bits(lane) - 1).unwrap_or(0);
    Some(match operation {
        Op::Not => not(a),
        Op::Abs if is_float(lane) => map1(lane, a, |x| x & !top),
        Op::Abs => map1(lane, a, |x| sign_extend(lane, x).wrapping_abs() as u64),
        Op::Neg if is_float(lane) => map1(lane, a, |x| x ^ top),
        Op::Neg => map1(lane, a, |x| sign_extend(lane, x).wrapping_neg() as u64),
        Op::Popcnt => map1(lane, a, |x| u64::from(x.count_ones())),
        Op::Sqrt => map1(lane, a, |x| float_lane(lane, lane_float(lane, x).sqrt())),
        Op::Ceil => map1(lane, a, |x| float_lane(lane, lane_float(lane, x).ceil())),
        Op::Floor => map1(lane, a, |x| float_lane(lane, lane_float(lane, x).floor())),
        Op::Trunc => map1(lane, a, |x| float_lane(lane, lane_float(lane, x).trunc())),
        Op::Nearest => map1(lane, a, |x| float_lane(lane, lane_float(lane, x).round_ties_even())),
        Op::ExtendLow | Op::ExtendHigh => {
            let source = narrower(lane);
            let count = lane.element_count();
            let base = if operation == Op::ExtendHigh { count } else { 0 };
            (0..count).fold(0, |acc, index| {
                lane_set(lane, acc, index, lane_value(source, lane_get(source, a, base + index), signed) as u64)
            })
        }
        Op::ExtaddPairwise => {
            let result = wider(lane);
            (0..result.element_count()).fold(0, |acc, index| {
                let sum = lane_value(lane, lane_get(lane, a, 2 * index), signed)
                    + lane_value(lane, lane_get(lane, a, 2 * index + 1), signed);
                lane_set(result, acc, index, sum as u64)
            })
        }
        Op::Convert => map1(SimdLane::F32x4, a, |x| {
            let converted = if signed { (x as u32 as i32) as f32 } else { (x as u32) as f32 };
            u64::from(converted.to_bits())
        }),
        Op::ConvertLow => (0..2u8).fold(0, |acc, index| {
            let x = lane_get(SimdLane::I32x4, a, index) as u32;
            let converted = if signed { f64::from(x as i32) } else { f64::from(x) };
            lane_set(SimdLane::F64x2, acc, index, converted.to_bits())
        }),
        Op::TruncSat if lane == SimdLane::F32x4 => map1(SimdLane::I32x4, a, |x| {
            let value = f32::from_bits(x as u32);
            if signed { u64::from((value as i32) as u32) } else { u64::from(value as u32) }
        }),
        Op::TruncSat => (0..2u8).fold(0, |acc, index| {
            let value = f64::from_bits(lane_get(SimdLane::F64x2, a, index));
            let bits = if signed { u64::from((value as i32) as u32) } else { u64::from(value as u32) };
            lane_set(SimdLane::I32x4, acc, index, bits)
        }),
        Op::RelaxedTruncSat if lane == SimdLane::F32x4 => map1(SimdLane::I32x4, a, |x| {
            let value = f64::from(f32::from_bits(x as u32));
            u64::from(if signed { relaxed_trunc_signed(value) } else { relaxed_trunc_f32_unsigned(value) })
        }),
        Op::RelaxedTruncSat => (0..2u8).fold(0, |acc, index| {
            let value = f64::from_bits(lane_get(SimdLane::F64x2, a, index));
            let bits = if signed { relaxed_trunc_signed(value) } else { relaxed_trunc_f64_unsigned(value) };
            lane_set(SimdLane::I32x4, acc, index, u64::from(bits))
        }),
        Op::Demote => (0..2u8).fold(0, |acc, index| {
            let value = f64::from_bits(lane_get(SimdLane::F64x2, a, index));
            lane_set(SimdLane::F32x4, acc, index, u64::from((value as f32).to_bits()))
        }),
        Op::Promote => (0..2u8).fold(0, |acc, index| {
            let value = f32::from_bits(lane_get(SimdLane::F32x4, a, index) as u32);
            lane_set(SimdLane::F64x2, acc, index, f64::from(value).to_bits())
        }),
        _ => return None,
    })
}

/// Operações de dois operandos `v128`: add, sub, mul, comparações, and/or/xor/andnot, swizzle, min, max, pmin,
/// pmax, add_sat, sub_sat, avgr, div, q15mulr, dot, narrow, extmul.
pub fn binary(operation: SimdLaneOperation, lane: SimdLane, sign: SimdSignMode, a: u128, b: u128) -> Option<u128> {
    use SimdLaneOperation as Op;
    let signed = sign == SimdSignMode::Signed;
    let float = is_float(lane);
    let bits = lane_bits(lane);
    Some(match operation {
        Op::Add => arith(lane, LaneArith::Add, a, b),
        Op::Sub => arith(lane, LaneArith::Sub, a, b),
        Op::Mul => arith(lane, LaneArith::Mul, a, b),
        Op::Equal => compare(lane, LaneCompare::Eq, signed, a, b),
        Op::NotEqual => compare(lane, LaneCompare::Ne, signed, a, b),
        Op::LessThan => compare(lane, LaneCompare::Lt, signed, a, b),
        Op::GreaterThan => compare(lane, LaneCompare::Gt, signed, a, b),
        Op::LessThanOrEqual => compare(lane, LaneCompare::Le, signed, a, b),
        Op::GreaterThanOrEqual => compare(lane, LaneCompare::Ge, signed, a, b),
        Op::And => and(a, b),
        Op::Or => or(a, b),
        Op::Xor => xor(a, b),
        Op::Andnot => andnot(a, b),
        Op::Swizzle => swizzle(a, b),
        // relaxed (medido no bun x86-64): swizzle é o pshufb cru, min/max são minps/maxps (devolvem o segundo
        // operando com NaN ou zeros iguais), q15mulr é o pmulhrsw sem saturação, dot é o pmaddubsw (segundo
        // operando sem sinal, soma do par saturada em i16).
        Op::RelaxedSwizzle => from_bytes(core::array::from_fn(|index| {
            let selector = to_bytes(b)[index];
            if selector & 0x80 != 0 { 0 } else { to_bytes(a)[usize::from(selector & 15)] }
        })),
        Op::RelaxedMin => map2(lane, a, b, |x, y| if lane_float(lane, x) < lane_float(lane, y) { x } else { y }),
        Op::RelaxedMax => map2(lane, a, b, |x, y| if lane_float(lane, x) > lane_float(lane, y) { x } else { y }),
        Op::RelaxedQ15Mulr => map2(lane, a, b, |x, y| {
            ((lane_value(lane, x, true) * lane_value(lane, y, true) + 0x4000) >> 15) as u64
        }),
        Op::RelaxedDotI8x16I7x16 => {
            (0..8u8).fold(0, |acc, index| lane_set(SimdLane::I16x8, acc, index, relaxed_dot_pair(a, b, 2 * index)))
        }
        Op::Min | Op::Max if float => map2(lane, a, b, |x, y| {
            let (fx, fy) = (lane_float(lane, x), lane_float(lane, y));
            float_lane(lane, if operation == Op::Min { wasm_min(fx, fy) } else { wasm_max(fx, fy) })
        }),
        Op::Min | Op::Max => map2(lane, a, b, |x, y| {
            let (vx, vy) = (lane_value(lane, x, signed), lane_value(lane, y, signed));
            (if operation == Op::Min { vx.min(vy) } else { vx.max(vy) }) as u64
        }),
        Op::Pmin => map2(lane, a, b, |x, y| if lane_float(lane, y) < lane_float(lane, x) { y } else { x }),
        Op::Pmax => map2(lane, a, b, |x, y| if lane_float(lane, x) < lane_float(lane, y) { y } else { x }),
        Op::AddSat => map2(lane, a, b, |x, y| saturate(lane_value(lane, x, signed) + lane_value(lane, y, signed), bits, signed)),
        Op::SubSat => map2(lane, a, b, |x, y| saturate(lane_value(lane, x, signed) - lane_value(lane, y, signed), bits, signed)),
        Op::AvgRound => map2(lane, a, b, |x, y| ((u128::from(x) + u128::from(y) + 1) >> 1) as u64),
        Op::Div => map2(lane, a, b, |x, y| float_lane(lane, lane_float(lane, x) / lane_float(lane, y))),
        Op::MulSat => map2(lane, a, b, |x, y| {
            let product = lane_value(lane, x, true) * lane_value(lane, y, true);
            saturate((product + 0x4000) >> 15, 16, true)
        }),
        Op::DotProduct => {
            let source = narrower(lane);
            (0..lane.element_count()).fold(0, |acc, index| {
                let pair = |offset: u8| {
                    lane_value(source, lane_get(source, a, 2 * index + offset), true)
                        * lane_value(source, lane_get(source, b, 2 * index + offset), true)
                };
                lane_set(lane, acc, index, (pair(0) + pair(1)) as u64)
            })
        }
        Op::Narrow => {
            let result = narrower(lane);
            let count = lane.element_count();
            (0..2 * count).fold(0, |acc, index| {
                let (source, position) = if index < count { (a, index) } else { (b, index - count) };
                let value = lane_value(lane, lane_get(lane, source, position), true);
                lane_set(result, acc, index, saturate(value, lane_bits(result), signed))
            })
        }
        Op::ExtmulLow | Op::ExtmulHigh => {
            let source = narrower(lane);
            let count = lane.element_count();
            let base = if operation == Op::ExtmulHigh { count } else { 0 };
            (0..count).fold(0, |acc, index| {
                let product = lane_value(source, lane_get(source, a, base + index), signed)
                    * lane_value(source, lane_get(source, b, base + index), signed);
                lane_set(lane, acc, index, product as u64)
            })
        }
        _ => return None,
    })
}

/// `relaxed_trunc` com sinal, como o `cvttps2dq`: NaN e fora da faixa dão `0x80000000`.
fn relaxed_trunc_signed(value: f64) -> u32 {
    let truncated = value.trunc();
    if truncated.is_nan() || truncated >= 2147483648.0 || truncated < -2147483648.0 {
        0x8000_0000
    } else {
        truncated as i32 as u32
    }
}

/// `relaxed_trunc_f32x4_u` do JSC no x86: `max(x, 0)` (NaN vira 0) e depois o `cvttps2dq`.
fn relaxed_trunc_f32_unsigned(value: f64) -> u32 {
    if value.is_nan() || value <= 0.0 { 0 } else { relaxed_trunc_signed(value) }
}

/// `relaxed_trunc_f64x2_u_zero` do JSC no x86: negativo, NaN e infinito dão 0, o resto trunca em 32 bits.
fn relaxed_trunc_f64_unsigned(value: f64) -> u32 {
    if value.is_nan() || value <= 0.0 || value.is_infinite() { 0 } else { value.trunc() as u64 as u32 }
}

/// Um par de produtos do `pmaddubsw` a partir do byte `index`: `a` com sinal, `b` sem sinal, soma saturada em i16.
fn relaxed_dot_pair(a: u128, b: u128, index: u8) -> u64 {
    let product = |offset: u8| {
        i128::from(lane_get(SimdLane::I8x16, a, index + offset) as u8 as i8)
            * i128::from(lane_get(SimdLane::I8x16, b, index + offset) as u8)
    };
    saturate(product(0) + product(1), 16, true)
}

/// `fma` do relaxed (`vfmadd`): com NaN devolve o primeiro NaN dos operandos, quieto; operação inválida dá o NaN
/// padrão do x86 (sinal ligado). `negate` é o `nmadd`: `-(x * y) + z`.
fn relaxed_madd(lane: SimdLane, negate: bool, x: u64, y: u64, z: u64) -> u64 {
    let quiet = if lane == SimdLane::F32x4 { 1u64 << 22 } else { 1u64 << 51 };
    if let Some(nan) = [x, y, z].into_iter().find(|bits| lane_float(lane, *bits).is_nan()) {
        return nan | quiet;
    }
    if lane == SimdLane::F32x4 {
        let (fx, fy, fz) = (f32::from_bits(x as u32), f32::from_bits(y as u32), f32::from_bits(z as u32));
        let result = (if negate { -fx } else { fx }).mul_add(fy, fz);
        if result.is_nan() { 0xffc0_0000 } else { u64::from(result.to_bits()) }
    } else {
        let (fx, fy, fz) = (f64::from_bits(x), f64::from_bits(y), f64::from_bits(z));
        let result = (if negate { -fx } else { fx }).mul_add(fy, fz);
        if result.is_nan() { 0xfff8_0000_0000_0000 } else { result.to_bits() }
    }
}

/// Operações de três operandos `v128` do relaxed-simd (`a`, `b`, `c` nessa ordem de pilha): madd, nmadd,
/// laneselect (o bit mais significativo da lane de `c` escolhe `a`, senão `b`) e dot_add.
pub fn ternary(operation: SimdLaneOperation, lane: SimdLane, a: u128, b: u128, c: u128) -> Option<u128> {
    use SimdLaneOperation as Op;
    Some(match operation {
        Op::RelaxedMAdd | Op::RelaxedNMAdd => (0..lane.element_count()).fold(0, |acc, index| {
            let bits = relaxed_madd(
                lane,
                operation == Op::RelaxedNMAdd,
                lane_get(lane, a, index),
                lane_get(lane, b, index),
                lane_get(lane, c, index),
            );
            lane_set(lane, acc, index, bits)
        }),
        Op::RelaxedLaneSelect => (0..lane.element_count()).fold(0, |acc, index| {
            let source = if lane_get(lane, c, index) >> (lane_bits(lane) - 1) != 0 { a } else { b };
            lane_set(lane, acc, index, lane_get(lane, source, index))
        }),
        Op::RelaxedDotI8x16I7x16Add => (0..4u8).fold(0, |acc, index| {
            let sum = relaxed_dot_pair(a, b, 4 * index) as u32 as i16 as i32
                + relaxed_dot_pair(a, b, 4 * index + 2) as u32 as i16 as i32;
            let total = (sum as u32).wrapping_add(lane_get(SimdLane::I32x4, c, index) as u32);
            lane_set(SimdLane::I32x4, acc, index, u64::from(total))
        }),
        _ => return None,
    })
}

/// `shl`, `shr_s`, `shr_u`: a contagem é tomada módulo a largura da lane.
pub fn shift(operation: SimdLaneOperation, lane: SimdLane, sign: SimdSignMode, a: u128, count: u32) -> Option<u128> {
    let amount = count % lane_bits(lane);
    Some(match operation {
        SimdLaneOperation::Shl => map1(lane, a, |x| x << amount),
        SimdLaneOperation::Shr if sign == SimdSignMode::Signed => map1(lane, a, |x| (sign_extend(lane, x) >> amount) as u64),
        SimdLaneOperation::Shr => map1(lane, a, |x| x >> amount),
        _ => return None,
    })
}
