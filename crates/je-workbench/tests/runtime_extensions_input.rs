//! Porte de `src/vs/workbench/contrib/extensions/test/common/runtimeExtensionsInput.test.ts` (suíte
//! `RuntimeExtensionsInput`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no começo do caso e conferido quando sai de escopo;
//!   o `store` do original é o próprio guarda (`add_rc` registra a entrada para o descarte).
//! - `RuntimeExtensionsInput.instance` é [`RuntimeExtensionsInput::instance`] (a instância singleton, recriada se a
//!   guardada foi descartada).
//! - O `assert.deepStrictEqual` entre dois objetos de três booleanos é a comparação das tuplas.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::common::editor::EditorInputCapabilities;
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::contrib::extensions::runtime_extensions_input::RuntimeExtensionsInput;

#[test]
fn requires_modal_presentation() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let input = store.add_rc(RuntimeExtensionsInput::instance());

    assert_eq!(
        (
            input.has_capability(EditorInputCapabilities::READONLY),
            input.has_capability(EditorInputCapabilities::SINGLETON),
            input.has_capability(EditorInputCapabilities::REQUIRES_MODAL),
        ),
        (true, true, true)
    );
}
