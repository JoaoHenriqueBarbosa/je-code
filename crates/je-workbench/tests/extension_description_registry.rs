//! Porte de `src/vs/workbench/services/extensions/test/common/extensionDescriptionRegistry.test.ts` (suíte `ExtensionDescriptionRegistry`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda do começo do caso.
//! - `IActivationEventsReader` é um objeto literal no original: aqui é a struct [`BasicActivationEventsReader`], que devolve uma
//!   cópia de `activationEvents` (ou a lista vazia).
//! - `IExtensionDescription` é o `ExtensionDescription` de `je-extensions` (compartilhado como `Rc`): as chaves que ele não tem
//!   (`name`, `publisher`, `version`, `engines`, `activationEvents`, `main`, `extensionDependencies`, `enabledApiProposals`)
//!   são as do `manifest`, e `URI.parse('nothing://nowhere')` é o texto da URI. O parâmetro `version` de `desc` não é usado
//!   pelo original (a versão do manifesto é sempre `0.0.0`), e o porte mantém isso.
//! - `assert.deepStrictEqual(a, b)` é `assert_eq!` sobre as extensões (a igualdade profunda do `PartialEq`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_extensions::manifest::{Engines, ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_workbench::services::extensions::extension_description_registry::{
    ExtensionDescriptionRegistry, IActivationEventsReader, IReadOnlyExtensionDescriptionRegistry, SharedExtensionDescription,
};

/// `basicActivationEventsReader`.
struct BasicActivationEventsReader;

impl IActivationEventsReader for BasicActivationEventsReader {
    fn read_activation_events(&self, extension_description: &ExtensionDescription) -> Vec<String> {
        extension_description.manifest.activation_events.clone().unwrap_or_default()
    }
}

/// `desc(id, version, activationEvents = ['*'])`.
fn desc(id: &ExtensionIdentifier, _version: &str) -> SharedExtensionDescription {
    Rc::new(ExtensionDescription {
        id: id.value.clone(),
        identifier: id.clone(),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: "nothing://nowhere".to_owned(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest {
            name: id.value.clone(),
            publisher: "test".to_owned(),
            version: "0.0.0".to_owned(),
            engines: Engines { vscode: Some("^1.0.0".to_owned()), ..Engines::default() },
            activation_events: Some(vec!["*".to_owned()]),
            main: Some("index.js".to_owned()),
            extension_dependencies: Some(Vec::new()),
            enabled_api_proposals: None,
            ..ExtensionManifest::default()
        },
    })
}

#[test]
fn allow_removing_and_adding_the_same_extension_at_a_different_version() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let id_a = ExtensionIdentifier::new("a");
    let extension_a1 = desc(&id_a, "1.0.0");
    let extension_a2 = desc(&id_a, "2.0.0");

    let basic_activation_events_reader: Rc<dyn IActivationEventsReader> = Rc::new(BasicActivationEventsReader);

    let registry = ExtensionDescriptionRegistry::new(basic_activation_events_reader, vec![extension_a1]);
    registry.delta_extensions(vec![extension_a2.clone()], &[id_a]);

    assert_eq!(registry.get_all_extension_descriptions(), vec![extension_a2]);

    registry.dispose();
}
