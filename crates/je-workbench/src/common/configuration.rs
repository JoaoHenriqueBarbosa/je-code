//! Porte parcial de `src/vs/workbench/common/configuration.ts`: o registro de migrações de configuração
//! (`Extensions.ConfigurationMigration`, `ConfigurationMigration`, `ConfigurationKeyValuePairs` e
//! `IConfigurationMigrationRegistry`).
//!
//! Recortes:
//! - `ConfigurationMigrationWorkbenchContribution` (que lê o `inspect` de cada chave por alvo e escopo, e roda as
//!   migrações) fica de fora: depende de `IWorkspaceContextService` e de `IConfigurationValue`, e entra com o
//!   serviço de configuração do workbench. Os nós-base (`workbenchConfigurationNodeBase` e companhia) já moram em
//!   `services/editor/editor_resolver_service.rs` e nos módulos que os usam.
//! - Os `Object.freeze<IConfigurationNode>` dos nós-base não se aplicam: aqui são funções que devolvem um dado novo.
//!
//! Divergências:
//! - `Registry.add(Extensions.ConfigurationMigration, registry)` é o singleton por thread
//!   [`configuration_migration_registry`], como o `configuration_registry` do `je-platform`.
//! - `ConfigurationValue | ConfigurationKeyValuePairs | Promise<...>` é um [`Promise`] de
//!   [`ConfigurationMigrationResult`]; quem migra de forma síncrona devolve `Promise::resolved`. O `value: unknown`
//!   é um [`Dynamic`], e `undefined` (remover a chave) é `Dynamic::Undefined`.
//! - `valueAccessor: (key: string) => any` é `&dyn Fn(&str) -> Dynamic`.
//! - `onDidRegisterConfigurationMigration` existe, mas, como no original, `registerConfigurationMigrations` só
//!   acrescenta à lista e nunca dispara o evento.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::promise::Promise;

/// `Extensions.ConfigurationMigration`.
pub const CONFIGURATION_MIGRATION_EXTENSION_ID: &str = "base.contributions.configuration.migration";

/// `ConfigurationValue`: `{ value: unknown | undefined /* Remove */ }`.
#[derive(Clone)]
pub struct ConfigurationValue {
    pub value: Dynamic,
}

/// `ConfigurationKeyValuePairs`.
pub type ConfigurationKeyValuePairs = Vec<(String, ConfigurationValue)>;

/// `ConfigurationValue | ConfigurationKeyValuePairs`.
#[derive(Clone)]
pub enum ConfigurationMigrationResult {
    Value(ConfigurationValue),
    Pairs(ConfigurationKeyValuePairs),
}

/// `ConfigurationMigrationFn`.
pub type ConfigurationMigrationFn = Rc<dyn Fn(Dynamic, &dyn Fn(&str) -> Dynamic) -> Promise<ConfigurationMigrationResult>>;

/// `ConfigurationMigration`.
#[derive(Clone)]
pub struct ConfigurationMigration {
    pub key: String,
    pub migrate_fn: ConfigurationMigrationFn,
    pub include_application: Option<bool>,
}

/// `IConfigurationMigrationRegistry`.
pub trait IConfigurationMigrationRegistry {
    fn register_configuration_migrations(&self, configuration_migrations: Vec<ConfigurationMigration>);
}

/// `ConfigurationMigrationRegistry`.
pub struct ConfigurationMigrationRegistry {
    migrations: RefCell<Vec<ConfigurationMigration>>,
    on_did_register_configuration_migrations: Rc<Emitter<Vec<ConfigurationMigration>>>,
}

impl ConfigurationMigrationRegistry {
    fn new() -> Self {
        Self { migrations: RefCell::new(Vec::new()), on_did_register_configuration_migrations: Emitter::new() }
    }

    /// `migrations`: as migrações registradas, na ordem do registro.
    pub fn migrations(&self) -> Vec<ConfigurationMigration> {
        self.migrations.borrow().clone()
    }

    /// `onDidRegisterConfigurationMigration`.
    pub fn on_did_register_configuration_migration(&self) -> Event<Vec<ConfigurationMigration>> {
        self.on_did_register_configuration_migrations.event()
    }
}

impl IConfigurationMigrationRegistry for ConfigurationMigrationRegistry {
    fn register_configuration_migrations(&self, configuration_migrations: Vec<ConfigurationMigration>) {
        self.migrations.borrow_mut().extend(configuration_migrations);
    }
}

thread_local! {
    static CONFIGURATION_MIGRATION_REGISTRY: Rc<ConfigurationMigrationRegistry> = Rc::new(ConfigurationMigrationRegistry::new());
}

/// `Registry.as<IConfigurationMigrationRegistry>(Extensions.ConfigurationMigration)`.
pub fn configuration_migration_registry() -> Rc<ConfigurationMigrationRegistry> {
    CONFIGURATION_MIGRATION_REGISTRY.with(Rc::clone)
}
