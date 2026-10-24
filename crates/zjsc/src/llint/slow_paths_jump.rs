//! Slow paths de desvio por comparação de `llint/LLIntSlowPaths.cpp`: `slow_path_jless`, `jnless`,
//! `jgreater`, `jngreater`, `jlesseq`, `jnlesseq`, `jgreatereq`, `jngreatereq`, `jeq`, `jneq`,
//! `jstricteq` e `jnstricteq`. Mais os handlers de `LowLevelInterpreter64.asm` sem slow path no C++
//! (`op_jeq_null`, `op_jneq_null`, `op_jundefined_or_null`, `op_jnundefined_or_null`), que são
//! offlineasm puro e entram aqui porque o laço precisa deles com o mesmo formato.
//!
//! Cada função devolve o que o `LLINT_BRANCH` decide: `true` quando o desvio é tomado (o laço soma o
//! `JUMP_OFFSET`). Os `jsLess`/`jsLessEq`/`equal`/`strictEqual` do porte não lançam (a conversão
//! `ToPrimitive` de objeto ainda não existe), então não há `LLINT_CHECK_EXCEPTION`.
//!
//! DIVERGÊNCIA: `jeq_null`/`jneq_null` no `.asm` consultam o bit `MasqueradesAsUndefined` da `Structure`
//! para células; nenhum objeto do porte o liga (ver `runtime/js_typeof.rs`), então a decisão é só
//! `isUndefinedOrNull`.

use crate::bytecode::bytecode_ops::{
    OpJeq, OpJeqNull, OpJgreater, OpJgreatereq, OpJless, OpJlesseq, OpJneq, OpJneqNull, OpJngreater,
    OpJngreatereq, OpJnless, OpJnlesseq, OpJnstricteq, OpJnundefinedOrNull, OpJstricteq, OpJundefinedOrNull,
};
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::runtime::operations::{js_less, js_less_eq, loose_equal, strict_equal};

/// O molde dos `slow_path_j*` de dois operandos: `LLINT_BRANCH(condição(lhs, rhs))`.
macro_rules! binary_branch {
    ($($(#[$doc:meta])* $name:ident($op:ty): |$lhs:ident, $rhs:ident| $condition:expr;)*) => {
        $(
            $(#[$doc])*
            pub fn $name(f: &mut SlowPathFrame, op: &$op) -> bool {
                let ($lhs, $rhs) = (f.get(op.lhs), f.get(op.rhs));
                $condition
            }
        )*
    };
}

/// O molde dos handlers de um operando (`jeq_null` e companhia).
macro_rules! unary_branch {
    ($($(#[$doc:meta])* $name:ident($op:ty): |$value:ident| $condition:expr;)*) => {
        $(
            $(#[$doc])*
            pub fn $name(f: &mut SlowPathFrame, op: &$op) -> bool {
                let $value = f.get(op.value);
                $condition
            }
        )*
    };
}

binary_branch! {
    /// `slow_path_jless`: `jsLess<true>(lhs, rhs)`.
    slow_path_jless(OpJless): |l, r| js_less::<true>(l, r);
    /// `slow_path_jnless`: `!jsLess<true>(lhs, rhs)`.
    slow_path_jnless(OpJnless): |l, r| !js_less::<true>(l, r);
    /// `slow_path_jgreater`: `jsLess<false>(rhs, lhs)`.
    slow_path_jgreater(OpJgreater): |l, r| js_less::<false>(r, l);
    /// `slow_path_jngreater`: `!jsLess<false>(rhs, lhs)`.
    slow_path_jngreater(OpJngreater): |l, r| !js_less::<false>(r, l);
    /// `slow_path_jlesseq`: `jsLessEq<true>(lhs, rhs)`.
    slow_path_jlesseq(OpJlesseq): |l, r| js_less_eq::<true>(l, r);
    /// `slow_path_jnlesseq`: `!jsLessEq<true>(lhs, rhs)`.
    slow_path_jnlesseq(OpJnlesseq): |l, r| !js_less_eq::<true>(l, r);
    /// `slow_path_jgreatereq`: `jsLessEq<false>(rhs, lhs)`.
    slow_path_jgreatereq(OpJgreatereq): |l, r| js_less_eq::<false>(r, l);
    /// `slow_path_jngreatereq`: `!jsLessEq<false>(rhs, lhs)`.
    slow_path_jngreatereq(OpJngreatereq): |l, r| !js_less_eq::<false>(r, l);
    /// `slow_path_jeq`: `JSValue::equal(lhs, rhs)`.
    slow_path_jeq(OpJeq): |l, r| loose_equal(l, r);
    /// `slow_path_jneq`: `!JSValue::equal(lhs, rhs)`.
    slow_path_jneq(OpJneq): |l, r| !loose_equal(l, r);
    /// `slow_path_jstricteq`: `JSValue::strictEqual(lhs, rhs)`.
    slow_path_jstricteq(OpJstricteq): |l, r| strict_equal(l, r);
    /// `slow_path_jnstricteq`: `!JSValue::strictEqual(lhs, rhs)`.
    slow_path_jnstricteq(OpJnstricteq): |l, r| !strict_equal(l, r);
}

unary_branch! {
    /// `op_jeq_null`: desvia se o valor é `undefined` ou `null` (ou mascara-se de `undefined`).
    handle_jeq_null(OpJeqNull): |v| v.is_undefined_or_null();
    /// `op_jneq_null`: desvia se o valor não é `undefined` nem `null`.
    handle_jneq_null(OpJneqNull): |v| !v.is_undefined_or_null();
    /// `op_jundefined_or_null`: desvia se o valor é `undefined` ou `null` (sem o caso de máscara).
    handle_jundefined_or_null(OpJundefinedOrNull): |v| v.is_undefined_or_null();
    /// `op_jnundefined_or_null`: desvia se o valor não é `undefined` nem `null`.
    handle_jnundefined_or_null(OpJnundefinedOrNull): |v| !v.is_undefined_or_null();
}
