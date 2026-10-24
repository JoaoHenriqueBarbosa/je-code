//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/test/browser/lspTerminalModelContentProvider.test.ts` (suíte `LspTerminalModelContentProvider`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo do caso.
//! - **Casos fora do porte.** Os três casos de `setContent` (`should add delimiter when setting content on empty document`,
//!   `should update content with delimiter when document already has content` e `should sanitize content when delimiter is in
//!   the middle of existing content`) espionam o `setValue` e o `getValue` de um `ITextModel` falso (`sinon.stub`), que o
//!   `modelService.getModel` devolve. O `IModelService` do porte devolve o `TextModel` concreto do editor, sem ponto de
//!   substituição para espionar o `setValue`, e um `TextModel` real mais um `ILanguageService` exigem montar o `LanguageService`
//!   e o `TextModel` do editor, que mudariam o que o caso afirma (a contagem de chamadas de `setValue`). Sem `#[test]`.
//! - **`Mac, Linux - createTerminalLanguageVirtualUri should return the correct URI`.** Roda em qualquer plataforma (o nome
//!   do original só reflete a convenção de caminho de `/terminal1.py`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::network::schemas;
use je_base::uri::{URI, UriComponents};
use je_workbench::contrib::terminal_contrib::suggest::lsp_terminal_model_content_provider::create_terminal_language_virtual_uri;

#[test]
fn mac_linux_create_terminal_language_virtual_uri_should_return_the_correct_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected_uri = URI::from(
        &UriComponents {
            scheme: Some(JsString::from(schemas::VSCODE_TERMINAL)),
            authority: None,
            path: Some(JsString::from("/terminal1.py")),
            query: None,
            fragment: None,
        },
        false,
    );
    let actual_uri = create_terminal_language_virtual_uri(1, "py");
    assert_eq!(actual_uri.to_string(false), expected_uri.to_string(false));
}
