//! Porte de `src/vs/workbench/contrib/extensions/test/browser/extensionEnablementContext.test.ts` (suíte
//! `ExtensionEnablementContextKeysContribution`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no começo do caso; o `disposables` do original é o
//!   próprio guarda.
//! - `TestExtensionService extends NullExtensionService` é o [`TestExtensionService`], que implementa o contrato
//!   [`IExtensionService`] reduzido do porte (os demais métodos têm o padrão do `NullExtensionService`). O
//!   `extensions` (mutado por `push`, `splice` e `length = 0`) é um `RefCell<Vec<_>>`, e o `Emitter` de
//!   `onDidChangeExtensions` é descartado pelo guarda (o `disposables.add({ dispose })` do original).
//! - `aExtension(id)` é `{ ...nullExtensionDescription, identifier: new ExtensionIdentifier(id) }`.
//! - `Record<string, boolean | undefined>` do `snapshot` é um `Vec<(String, Option<bool>)>` na ordem dos ids, e o `undefined`
//!   da chave que nunca existiu é `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::promise::Promise;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_workbench::contrib::extensions::extension_enablement_context::{EXTENSION_ENABLED_CONTEXT_KEY_PREFIX, ExtensionEnablementContextKeysContribution};
use je_workbench::services::extensions::extensions::{
    ActivationKind, IExtensionDescription, IExtensionService, IExtensionsChangeEvent, null_extension_description,
};

fn a_extension(id: &str) -> ExtensionDescription {
    ExtensionDescription { identifier: ExtensionIdentifier::new(id), ..null_extension_description() }
}

struct TestExtensionService {
    extensions: RefCell<Vec<ExtensionDescription>>,
    on_did_change_extensions: Rc<Emitter<IExtensionsChangeEvent>>,
}

impl TestExtensionService {
    fn new(initial: Vec<ExtensionDescription>) -> Rc<Self> {
        Rc::new(Self { extensions: RefCell::new(initial), on_did_change_extensions: Emitter::new() })
    }

    /// Applies a delta to the registry and fires the matching change event.
    fn fire_change(&self, added: Vec<ExtensionDescription>, removed: Vec<ExtensionDescription>) {
        {
            let mut extensions = self.extensions.borrow_mut();
            for to_remove in &removed {
                if let Some(index) = extensions.iter().position(|e| ExtensionIdentifier::equals(Some(&e.identifier.value), Some(&to_remove.identifier.value))) {
                    extensions.remove(index);
                }
            }
            extensions.extend(added.iter().cloned());
        }
        self.on_did_change_extensions.fire(IExtensionsChangeEvent { added, removed });
    }

    /// Overwrites the final registry and fires an event carrying an arbitrary (possibly misleading) delta. Used to
    /// simulate the case where registry validation rejects an `added` extension without reporting it in `removed`.
    fn set_extensions_and_fire(&self, final_extensions: Vec<ExtensionDescription>, event: IExtensionsChangeEvent) {
        *self.extensions.borrow_mut() = final_extensions;
        self.on_did_change_extensions.fire(event);
    }
}

impl IExtensionService for TestExtensionService {
    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        Promise::resolved(())
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        false
    }

    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        Promise::resolved(None)
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        Event::none()
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        self.extensions.borrow().clone()
    }

    fn on_did_change_extensions(&self) -> Event<IExtensionsChangeEvent> {
        self.on_did_change_extensions.event()
    }
}

fn snapshot(context_key_service: &MockContextKeyService, ids: &[&str]) -> Vec<(String, Option<bool>)> {
    ids.iter()
        .map(|id| {
            let value = match context_key_service.get_context_key_value(&format!("{EXTENSION_ENABLED_CONTEXT_KEY_PREFIX}{id}")) {
                Dynamic::Bool(value) => Some(value),
                Dynamic::Undefined => None,
                other => panic!("valor inesperado: {other:?}"),
            };
            ((*id).to_owned(), value)
        })
        .collect()
}

fn expected(entries: &[(&str, Option<bool>)]) -> Vec<(String, Option<bool>)> {
    entries.iter().map(|(id, value)| ((*id).to_owned(), *value)).collect()
}

fn create_contribution(disposables: &LeakGuard, initial: Vec<ExtensionDescription>) -> (Rc<MockContextKeyService>, Rc<TestExtensionService>) {
    let extension_service = TestExtensionService::new(initial);
    disposables.add_rc(extension_service.on_did_change_extensions.clone());
    let context_key_service = disposables.add_rc(MockContextKeyService::new());
    disposables.add_rc(ExtensionEnablementContextKeysContribution::new(context_key_service.clone(), extension_service.clone()));
    (context_key_service, extension_service)
}

#[test]
fn reflects_installed_and_enabled_extensions_and_reacts_to_changes() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (context_key_service, extension_service) = create_contribution(&disposables, vec![a_extension("pub.a"), a_extension("pub.b")]);

    assert_eq!(
        snapshot(&context_key_service, &["pub.a", "pub.b", "pub.c"]),
        expected(&[
            ("pub.a", Some(true)), // seeded from already-registered extensions
            ("pub.b", Some(true)),
            ("pub.c", None), // never present -> key was never set
        ])
    );

    extension_service.fire_change(vec![a_extension("pub.c")], vec![]); // newly enabled
    extension_service.fire_change(vec![], vec![a_extension("pub.b")]); // disabled or uninstalled
    extension_service.fire_change(vec![a_extension("pub.a")], vec![a_extension("pub.a")]); // update -> removed then re-added

    assert_eq!(
        snapshot(&context_key_service, &["pub.a", "pub.b", "pub.c"]),
        expected(&[("pub.a", Some(true)), ("pub.b", Some(false)), ("pub.c", Some(true))])
    );
}

#[test]
fn normalizes_extension_id_to_lower_case() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (context_key_service, _) = create_contribution(&disposables, vec![a_extension("Pub.MixedCase")]);

    assert_eq!(snapshot(&context_key_service, &["pub.mixedcase"]), expected(&[("pub.mixedcase", Some(true))]));
}

#[test]
fn reconciles_against_the_final_registry_not_the_change_delta() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (context_key_service, extension_service) = create_contribution(&disposables, vec![a_extension("pub.a")]);

    // Registry validation drops pub.a from the final set (e.g. a dependency loop) but reports it as `added` and omits it
    // from `removed`. The key must follow the final registry and become false, not trust the delta.
    extension_service.set_extensions_and_fire(vec![], IExtensionsChangeEvent { added: vec![a_extension("pub.a")], removed: vec![] });

    assert_eq!(snapshot(&context_key_service, &["pub.a"]), expected(&[("pub.a", Some(false))]));
}
