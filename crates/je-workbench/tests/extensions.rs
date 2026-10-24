//! Porte de `src/vs/workbench/services/extensions/test/common/extensions.test.ts` (suíte `Proposed API checks`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o [`LeakGuard`] criado no começo de cada caso.
//! - `ApiProposalName` (a união gerada em `extensionsApiProposals.ts`) é o nome da proposta como texto, que é como
//!   `is_proposed_api_enabled` e `check_proposed_api_enabled` o recebem; a tabela das propostas não é usada pelo teste.
//! - `IExtensionDescription` é o `ExtensionDescription` do `je-extensions`, com o `package.json` no manifesto (`name`,
//!   `publisher`, `version`, `engines`, `activationEvents`, `main`, `extensionDependencies` e `enabledApiProposals`) e a
//!   `URI.parse('nothing://nowhere')` como texto.
//! - `assert.throws(fn, /regex/)` é o `Err` cuja mensagem contém o trecho do padrão (o padrão não tem metacaracteres).
//! - `assert.doesNotThrow` é conferir que o resultado é `Ok`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_extensions::manifest::{Engines, ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_workbench::services::extensions::extensions::{check_proposed_api_enabled, is_proposed_api_enabled};

fn desc(id: &str, enabled_api_proposals: Option<&[&str]>) -> ExtensionDescription {
    ExtensionDescription {
        id: id.to_owned(),
        identifier: ExtensionIdentifier::new(id),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: "nothing://nowhere".to_owned(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest {
            name: id.to_owned(),
            publisher: "test".to_owned(),
            version: "0.0.0".to_owned(),
            engines: Engines { vscode: Some("^1.0.0".to_owned()), ..Engines::default() },
            activation_events: Some(vec!["*".to_owned()]),
            main: Some("index.js".to_owned()),
            extension_dependencies: Some(Vec::new()),
            enabled_api_proposals: enabled_api_proposals.map(|proposals| proposals.iter().map(|proposal| (*proposal).to_owned()).collect()),
            ..ExtensionManifest::default()
        },
    }
}

/// `assert.throws(() => checkProposedApiEnabled(extension, proposal), /CANNOT use API proposal: <proposal>/)`.
#[track_caller]
fn assert_check_throws(extension: &ExtensionDescription, proposal: &str) {
    let error = check_proposed_api_enabled(extension, proposal).expect_err("deveria lançar");
    let expected = format!("CANNOT use API proposal: {proposal}");
    assert!(error.to_string().contains(&expected), "a mensagem não casa com o padrão: {error}");
}

#[test]
fn only_enables_explicitly_declared_proposals() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = (
        is_proposed_api_enabled(&desc("test.declared", Some(&["fileSearchProvider"])), "fileSearchProvider"),
        is_proposed_api_enabled(&desc("test.missing", Some(&["textSearchProvider"])), "fileSearchProvider"),
        is_proposed_api_enabled(&desc("test.empty", Some(&[])), "fileSearchProvider"),
        is_proposed_api_enabled(&desc("test.undefined", None), "fileSearchProvider"),
    );
    // `{ declared, missing, empty, undefined }`.
    assert_eq!(actual, (true, false, false, false));
}

#[test]
fn checking_a_declared_proposal_succeeds() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = check_proposed_api_enabled(&desc("test.declared", Some(&["fileSearchProvider"])), "fileSearchProvider");
    assert!(result.is_ok(), "não deveria lançar: {result:?}");
}

#[test]
fn checking_an_undeclared_proposal_throws_even_when_another_proposal_is_declared() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_check_throws(&desc("test.missing", Some(&["textSearchProvider"])), "fileSearchProvider");
}

#[test]
fn checking_a_proposal_throws_when_the_declaration_is_empty() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_check_throws(&desc("test.empty", Some(&[])), "fileSearchProvider");
}

#[test]
fn checking_a_proposal_throws_when_no_proposals_are_declared() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_check_throws(&desc("test.undefined", None), "fileSearchProvider");
}
