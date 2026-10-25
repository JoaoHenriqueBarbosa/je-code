//! Porte de `src/vs/workbench/test/common/configuration.test.ts` (suíte `ConfigurationMigrationWorkbenchContribution`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`migrate(includeApplication)`.** Registra a migração no registro (por thread, então os dois casos não se enxergam,
//!   como no original só a migração do próprio caso responde ao `inspect`), cria a contribuição e espera `completed`, que
//!   completa quando `updateValue` recebeu o número esperado de chamadas.
//! - **`new class extends TestConfigurationService`.** Não há herança: o [`MigrationConfigurationService`] guarda o
//!   `TestConfigurationService` e delega a ele tudo que a subclasse não sobrescreve; sobrescreve `inspect` (o literal
//!   `{ application: { value: 'application' }, user: { value: 'user' } }` para a chave da migração, e `{}` para as outras) e
//!   `updateValue` (que exige o alvo e o guarda).
//! - **`{ value: 'application' }`.** É o [`InspectValue`] de um modelo de configuração cujo conteúdo é o próprio valor, lido sem
//!   seção (`value()` devolve o conteúdo, e não há `overrides`).
//! - **`new class extends mock<IWorkspaceContextService>()`.** É o `TestContextService` com um workspace sem pastas, que o
//!   `getWorkbenchState` já resolve como `EMPTY` e `getWorkspace` como `{ id: 'test', folders: [] }`.
//! - **`teardown`.** O `LeakGuard` descarta a contribuição e reprova se algo ficou sem descartar.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::{DeferredPromise, Promise};
use je_platform::configuration::models::{ConfigurationChangeEvent, ConfigurationModel, InspectValue};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::{ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue};
use je_platform::configuration::models::ConfigurationKeys;
use je_platform::log::NullLogService;
use je_platform::workspace::{IWorkspaceContextService, new_test_workspace};
use je_workbench::common::configuration::{
    ConfigurationMigration, ConfigurationMigrationFn, ConfigurationMigrationResult, ConfigurationMigrationWorkbenchContribution, ConfigurationValue as MigrationValue,
    IConfigurationMigrationRegistry, configuration_migration_registry,
};
use je_workbench::test::common::workbench_test_services::TestContextService;

/// `{ value }` como `IInspectValue`: o modelo cujo conteúdo é o valor.
fn inspect_value_of(value: &str) -> InspectValue {
    ConfigurationModel::new(Dynamic::string(value), Vec::new(), Vec::new(), None, NullLogService::shared()).inspect(None, None)
}

/// O literal que `inspect` devolve: `{ application: { value: 'application' }, user: { value: 'user' } }` ou `{}`.
struct InspectLiteral {
    with_layers: bool,
}

impl ConfigurationValue for InspectLiteral {
    fn default_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn application_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn user_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn user_local_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn user_remote_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn workspace_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn workspace_folder_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn memory_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn policy_value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn value(&self) -> Dynamic {
        Dynamic::Undefined
    }
    fn override_identifiers(&self) -> Option<Vec<JsString>> {
        None
    }

    fn application(&self) -> Option<InspectValue> {
        self.with_layers.then(|| inspect_value_of("application"))
    }
    fn user(&self) -> Option<InspectValue> {
        self.with_layers.then(|| inspect_value_of("user"))
    }
}

/// `new class extends TestConfigurationService { override inspect(...); override updateValue(...) }`.
struct MigrationConfigurationService {
    inner: Rc<TestConfigurationService>,
    key: String,
    expected_updates: usize,
    targets: Rc<RefCell<Vec<ConfigurationTarget>>>,
    completed: Rc<DeferredPromise<()>>,
}

impl IConfigurationService for MigrationConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.inner.on_did_change_configuration()
    }

    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        self.inner.get_configuration_data()
    }

    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        self.inner.get_value(section, overrides)
    }

    fn update_value(&self, _key: &str, _value: Dynamic, _overrides: Option<ConfigurationUpdateOverrides>, target: Option<ConfigurationTarget>) -> Promise<()> {
        let target = target.expect("expected updateValue to be called with a target");
        let updates = {
            let mut targets = self.targets.borrow_mut();
            targets.push(target);
            targets.len()
        };
        if updates == self.expected_updates {
            let _ = self.completed.complete(());
        }
        Promise::resolved(())
    }

    fn inspect(&self, key: &str, _overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        Rc::new(InspectLiteral { with_layers: key == self.key })
    }

    fn reload_configuration(&self) -> Promise<()> {
        self.inner.reload_configuration()
    }

    fn keys(&self) -> ConfigurationKeys {
        self.inner.keys()
    }
}

/// `migrate(includeApplication)`, e a conferência do resultado esperado pelo caso.
fn migrate_and_expect(include_application: Option<bool>, expected: Vec<ConfigurationTarget>) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let key = if include_application == Some(true) { "configurationMigration.test.application" } else { "configurationMigration.test.default" };
        let migrate_fn: ConfigurationMigrationFn = Rc::new(|value: Dynamic, _value_accessor: &dyn Fn(&str) -> Dynamic| {
            Promise::resolved(ConfigurationMigrationResult::Value(MigrationValue { value }))
        });
        let migration = ConfigurationMigration { key: key.to_owned(), include_application, migrate_fn };
        configuration_migration_registry().register_configuration_migrations(vec![migration]);

        let targets: Rc<RefCell<Vec<ConfigurationTarget>>> = Rc::default();
        let completed = Rc::new(DeferredPromise::new());
        let inner = TestConfigurationService::new(None);
        disposables.add_rc(inner.on_did_change_configuration_emitter.clone());
        let configuration_service: Rc<dyn IConfigurationService> = Rc::new(MigrationConfigurationService {
            inner,
            key: key.to_owned(),
            expected_updates: if include_application == Some(true) { 2 } else { 1 },
            targets: targets.clone(),
            completed: completed.clone(),
        });
        let workspace_service: Rc<dyn IWorkspaceContextService> = TestContextService::new(Some(new_test_workspace("test", Vec::new(), None, None)), None);

        disposables.add_rc(ConfigurationMigrationWorkbenchContribution::new(configuration_service, workspace_service));
        completed.promise.clone().await.unwrap();

        assert_eq!(*targets.borrow(), expected);
    });
}

#[test]
fn excludes_application_configuration_by_default() {
    migrate_and_expect(None, vec![ConfigurationTarget::User]);
}

#[test]
fn includes_application_configuration_when_the_migration_opts_in() {
    migrate_and_expect(Some(true), vec![ConfigurationTarget::Application, ConfigurationTarget::User]);
}
