//! Porte de `src/vs/workbench/contrib/testing/common/testExclusions.ts`: o conjunto de testes excluídos do workspace,
//! guardado no `IStorageService` (escopo do workspace, alvo da máquina).
//!
//! Divergências:
//! - **`ReadonlySet<string>`** é `Rc<IndexSet<String>>`: o `MutableObservableValue` compara por identidade (`!==`), e
//!   cada `toggle` e `clear` cria um conjunto novo, como o original. A ordem de inserção do `Set` é a do `IndexSet`.
//! - **`onTestExclusionsChanged: Event<unknown>`** é o `Event` do valor observável (o conjunto novo).
//! - **`all: Iterable<string>`** é o conjunto (`Rc<IndexSet<String>>`).
//! - **`TestExclusions extends Disposable`:** o `Disposable` fica no campo `base`.

use std::rc::Rc;

use indexmap::IndexSet;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};

use super::observable_value::MutableObservableValue;
use super::stored_value::{IStoredValueSerialization, StoredValue, StoredValueOptions};
use super::test_types::InternalTestItem;

pub struct TestExclusions {
    base: Rc<Disposable>,
    excluded: Rc<MutableObservableValue<Rc<IndexSet<String>>>>,
}

impl TestExclusions {
    pub fn new(storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let base = Disposable::new();
        let serialization = IStoredValueSerialization::<Rc<IndexSet<String>>> {
            deserialize: Rc::new(|v| {
                let parsed: Vec<String> = match serde_json::from_str(v) {
                    Ok(parsed) => parsed,
                    Err(error) => je_base::errors::throw(je_base::errors::Error::named("SyntaxError", error.to_string())),
                };
                Rc::new(parsed.into_iter().collect())
            }),
            serialize: Rc::new(|v| serde_json::to_string(&v.iter().collect::<Vec<_>>()).unwrap_or_default()),
        };
        let excluded = MutableObservableValue::stored(
            StoredValue::with_serialization(
                StoredValueOptions { key: "excludedTestItems".to_string(), scope: StorageScope::Workspace, target: StorageTarget::Machine },
                serialization,
                storage_service,
            ),
            Rc::new(IndexSet::new()),
        );
        base.register(excluded.clone());
        Rc::new(Self { base, excluded })
    }

    /// Event that fires when the excluded tests change.
    pub fn on_test_exclusions_changed(&self) -> Event<Rc<IndexSet<String>>> {
        self.excluded.on_did_change()
    }

    /// Gets whether there's any excluded tests.
    pub fn has_any(&self) -> bool {
        !self.excluded.value().is_empty()
    }

    /// Gets all excluded tests.
    pub fn all(&self) -> Rc<IndexSet<String>> {
        self.excluded.value()
    }

    /// Sets whether a test is excluded.
    pub fn toggle(&self, test: &InternalTestItem, exclude: Option<bool>) {
        let current = self.excluded.value();
        let ext_id = &test.item.ext_id;
        if exclude != Some(true) && current.contains(ext_id) {
            self.excluded.set_value(Rc::new(current.iter().filter(|e| *e != ext_id).cloned().collect()));
        } else if exclude != Some(false) && !current.contains(ext_id) {
            let mut next: IndexSet<String> = current.iter().cloned().collect();
            next.insert(ext_id.clone());
            self.excluded.set_value(Rc::new(next));
        }
    }

    /// Gets whether a test is excluded.
    pub fn contains(&self, test: &InternalTestItem) -> bool {
        self.excluded.value().contains(&test.item.ext_id)
    }

    /// Removes all test exclusions.
    pub fn clear(&self) {
        self.excluded.set_value(Rc::new(IndexSet::new()));
    }
}

impl IDisposable for TestExclusions {
    fn dispose(&self) {
        self.base.dispose();
    }
}
