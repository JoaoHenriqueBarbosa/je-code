//! Porte de `src/vs/workbench/api/test/browser/extHost.api.impl.test.ts` (suíte `ExtHost API`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - O caso 1 só afirma algo no Windows (`if (isWindows)`); em outras plataformas o corpo é vazio, como no original.
//!   `URI.file('C:\\test')` é `URI::file` com o sabor de caminho do host; `URI.revive(JSON.parse(JSON.stringify(uri)))` é
//!   `URI::revive(&uri.to_json(), plataforma)` (o `to_json` é o estado que o `JSON.stringify` serializaria).
//! - `assert.throws(fn, /regex/)` é a mensagem do `Err` conter o texto; `deepStrictEqual` do resultado compara o
//!   [`TerminalInternalOptions`] com `Some(...)`.
//! - `{ ...nullExtensionDescription, enabledApiProposals: ['terminalRemoteResolver'] }` é o `null_extension_description()` com
//!   `manifest.enabled_api_proposals` trocado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::resources::original_fs_path;
use je_base::platform::Platform;
use je_base::uri::URI;
use je_workbench::api::common::ext_host_api_impl::{TerminalInternalOptions, TerminalOptions, get_terminal_internal_options};
use je_workbench::services::extensions::extensions::null_extension_description;

#[test]
fn issue_51387_original_fs_path() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let platform = Platform::detect();
    if platform.is_windows {
        let flavor = platform.path_flavor();
        let char_at_0 = |uri: &URI| original_fs_path(uri, flavor).to_string_lossy().chars().next().unwrap();
        let revive = |uri: &URI| URI::revive(&uri.to_json(), flavor);
        assert_eq!(char_at_0(&URI::file("C:\\test", flavor)), 'C');
        assert_eq!(char_at_0(&URI::file("c:\\test", flavor)), 'c');

        assert_eq!(char_at_0(&revive(&URI::file("C:\\test", flavor))), 'C');
        assert_eq!(char_at_0(&revive(&URI::file("c:\\test", flavor))), 'c');
    }
}

#[test]
fn terminal_options_is_remote_resolver_terminal_requires_terminal_remote_resolver_proposal() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = TerminalOptions { is_remote_resolver_terminal: Some(true), ..TerminalOptions::default() };
    let error = get_terminal_internal_options(&null_extension_description(), &options).unwrap_err();
    assert!(error.to_string().contains("CANNOT use API proposal: terminalRemoteResolver"), "{error}");
    let mut extension = null_extension_description();
    extension.manifest.enabled_api_proposals = Some(vec!["terminalRemoteResolver".to_owned()]);
    assert_eq!(
        get_terminal_internal_options(&extension, &options).unwrap(),
        Some(TerminalInternalOptions { is_remote_resolver_terminal: Some(true) })
    );
}
