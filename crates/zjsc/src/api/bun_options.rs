//! As opções do JavaScriptCore que o bun 1.4.2 liga antes de criar o `VM`. Medido no bun:
//! `SharedArrayBuffer`, `Temporal`, `DisposableStack`, `Iterator` e `ShadowRealm` existem.
//!
//! As opções são por thread (`thread_local!` em `runtime/options.rs`), então a chamada é idempotente e
//! precisa acontecer em cada thread antes do primeiro `VM::new` dela.

use crate::runtime::options_list::Options;

/// Liga o que o bun liga. `useTemporal`, `useExplicitResourceManagement`, `useImportDefer` e as quatro
/// `useIterator*` já têm default `true` no upstream e são reafirmadas; `useSharedArrayBuffer` e
/// `useShadowRealm` têm default `false` no upstream e o bun as liga.
pub fn apply_bun_options() {
    Options::set_use_shared_array_buffer(true);
    Options::set_use_shadow_realm(true);
    Options::set_use_temporal(true);
    Options::set_use_explicit_resource_management(true);
    Options::set_use_import_defer(true);
    Options::set_use_iterator_chunking(true);
    Options::set_use_iterator_includes(true);
    Options::set_use_iterator_join(true);
    Options::set_use_iterator_sequencing(true);
    // O bun recusa módulos com memória 64-bit (`CompileError: Memory64 is not enabled`), o upstream liga por default.
    Options::set_use_wasm_memory64(false);
    // `Error.stackTraceLimit` inicial: o bun mede 10, o `defaultErrorStackTraceLimit` do upstream é 100.
    Options::set_default_error_stack_trace_limit(10);
}
