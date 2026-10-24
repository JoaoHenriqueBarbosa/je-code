//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/test/browser/terminalSuggestConfiguration.test.ts` (suíte `Terminal Suggest Dynamic Configuration`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de cada caso.
//! - **`new Map([...])`.** É um `BTreeMap` de id para [`ITerminalSuggestProviderInfo`].
//! - **`assert.ok(true)`.** Os dois casos só afirmam que a função não entra em pânico (o `assert.ok(true)` do original).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::BTreeMap;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::terminal_contrib::suggest::common::terminal_suggest_configuration::{
    ITerminalSuggestProviderInfo, register_terminal_suggest_providers_configuration,
};

fn provider(id: &str, description: Option<&str>) -> (String, ITerminalSuggestProviderInfo) {
    (id.to_owned(), ITerminalSuggestProviderInfo { id: id.to_owned(), description: description.map(str::to_owned) })
}

#[test]
fn should_update_configuration_when_providers_change() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Test initial state
    register_terminal_suggest_providers_configuration(None);

    // Test with some providers
    let providers: BTreeMap<String, ITerminalSuggestProviderInfo> = [
        provider("terminal-suggest", Some("Provides intelligent completions for terminal commands")),
        provider("builtinPwsh", Some("PowerShell completion provider")),
        provider("lsp", None),
        provider("custom-provider", None),
    ]
    .into_iter()
    .collect();
    register_terminal_suggest_providers_configuration(Some(providers));

    // Test with empty providers
    register_terminal_suggest_providers_configuration(None);

    // The fact that this doesn't throw means the basic logic works
}

#[test]
fn should_include_default_providers_even_when_none_provided() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // This should not throw and should set up default configuration
    register_terminal_suggest_providers_configuration(None);
}
