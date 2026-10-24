//! `require("assert")` e `require("assert/strict")` do bun 1.4.2: `node_assert.js`, gerado por
//! `scripts/gen-node-assert.js` a partir do `assert.ts` do bun e dos internos dele, com o diff de Myers
//! (`runtime::myers_diff`) e o `partialDeepStrictEqual` (`runtime::partial_deep_equals`) nativos, como no bun.

use crate::api::builtin_modules::{load, resolve, Resolved};
use crate::api::node_js_host::{module_host, run_factory};
use crate::runtime::host_call::HostResult;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::native_class_support::property_key;

const SOURCE: &str = include_str!("node_assert.js");

/// Instalador de `node:assert`.
pub(crate) fn install_assert_module(global_object: &JSGlobalObject) -> HostResult {
    let host = module_host(
        global_object,
        &[
            ("myersDiff", 2, crate::runtime::myers_diff::myers_diff),
            ("printSimpleMyersDiff", 3, crate::runtime::myers_diff::print_simple_myers_diff),
            ("printMyersDiff", 4, crate::runtime::myers_diff::print_myers_diff),
            ("jsFunctionPartialDeepStrictEqual", 2, crate::runtime::partial_deep_equals::partial_deep_strict_equal),
            ("jsFunctionIsDeepStrictEqual", 3, crate::api::node_util::is_deep_strict_equal),
            ("utilInternal", 1, crate::api::node_util::util_internal),
        ],
    )?;
    Ok(run_factory(global_object, SOURCE, "assert", &host))
}

/// Instalador de `node:assert/strict`: `require("node:assert").strict`.
pub(crate) fn install_assert_strict_module(global_object: &JSGlobalObject) -> HostResult {
    let Resolved::Builtin(entry) = resolve("assert") else { unreachable!("o módulo assert consta em BUILTINS") };
    let assert = load(global_object, entry)?;
    let vm = global_object.vm();
    Ok(crate::runtime::host_call::pending_or(global_object, assert.as_object().get(global_object, &property_key(vm, "strict")))?)
}
