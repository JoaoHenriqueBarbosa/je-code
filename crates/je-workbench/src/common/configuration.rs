//! Porte parcial de `src/vs/workbench/common/configuration.ts`: o registro de migrações de configuração
//! (`Extensions.ConfigurationMigration`, `ConfigurationMigration`, `ConfigurationKeyValuePairs` e
//! `IConfigurationMigrationRegistry`).
//!
//! Recortes:
//! - `DynamicWorkbenchSecurityConfiguration` e `DynamicWindowConfiguration` ficam de fora: dependem do
//!   `IRemoteAgentService` completo e do `IUserDataProfilesService`, e entram com os serviços de que dependem. Os
//!   nós-base (`workbenchConfigurationNodeBase` e companhia) já moram em `services/editor/editor_resolver_service.rs` e
//!   nos módulos que os usam.
//! - Os `Object.freeze<IConfigurationNode>` dos nós-base não se aplicam: aqui são funções que devolvem um dado novo.
//!
//! Divergências:
//! - **`ConfigurationMigrationWorkbenchContribution`.** `await Promise.all([migrations.map(...)])` do original recebe um
//!   array dentro de outro array, então não espera as migrações (cada uma segue sozinha, numa promessa própria). O porte
//!   faz o mesmo: cada migração roda numa promessa que ninguém espera, e o `await` de [`Migrator::migrate_configurations`]
//!   é só a rodada da microtarefa que o original também cede. `inspectData[dataKey]` é [`InspectKey`], que escolhe o
//!   `IInspectValue` de cada alvo no [`IConfigurationValue`]; `Promise.allSettled` espera todas as atualizações e ignora
//!   as falhas.
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

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::models::{InspectValue, OverrideValue};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue as IConfigurationValue};
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState, WorkspaceFolder};

use crate::common::contributions::IWorkbenchContribution;

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

/// `keyof IConfigurationValue<unknown>` de cada alvo de migração: escolhe o `IInspectValue` da camada.
#[derive(Clone, Copy)]
enum InspectKey {
    Application,
    User,
    UserLocal,
    UserRemote,
    Workspace,
    WorkspaceFolder,
}

impl InspectKey {
    /// `inspectData[dataKey]`.
    fn inspect_value(self, inspect_data: &dyn IConfigurationValue) -> Option<InspectValue> {
        match self {
            Self::Application => inspect_data.application(),
            Self::User => inspect_data.user(),
            Self::UserLocal => inspect_data.user_local(),
            Self::UserRemote => inspect_data.user_remote(),
            Self::Workspace => inspect_data.workspace(),
            Self::WorkspaceFolder => inspect_data.workspace_folder(),
        }
    }
}

/// O que `ConfigurationMigrationWorkbenchContribution` lê dos serviços: os métodos `async` do original, em um valor que as
/// promessas em andamento podem clonar.
#[derive(Clone)]
struct Migrator {
    configuration_service: Rc<dyn IConfigurationService>,
    workspace_service: Rc<dyn IWorkspaceContextService>,
}

impl Migrator {
    /// `this.migrateConfigurations(migrations)` chamado sem `await`.
    fn spawn_migrate_configurations(&self, migrations: Vec<ConfigurationMigration>) {
        let migrator = self.clone();
        let _ = async_fn(async move { migrator.migrate_configurations(&migrations).await });
    }

    async fn migrate_configurations(&self, migrations: &[ConfigurationMigration]) -> Result<(), Error> {
        self.migrate_configurations_for_folder(None, migrations).await?;
        for folder in self.workspace_service.get_workspace().folders() {
            self.migrate_configurations_for_folder(Some(&folder), migrations).await?;
        }
        Ok(())
    }

    async fn migrate_configurations_for_folder(&self, folder: Option<&WorkspaceFolder>, migrations: &[ConfigurationMigration]) -> Result<(), Error> {
        // `await Promise.all([migrations.map(...)])`: o array de promessas vai dentro de outro array, então o original não as espera.
        for migration in migrations {
            let migrator = self.clone();
            let migration = migration.clone();
            let resource = folder.map(|folder| folder.uri.clone());
            let _ = async_fn(async move { migrator.migrate_configurations_for_folder_and_override(&migration, resource.as_ref()).await });
        }
        Promise::resolved(()).await
    }

    async fn migrate_configurations_for_folder_and_override(&self, migration: &ConfigurationMigration, resource: Option<&URI>) -> Result<(), Error> {
        let inspect_data = self.configuration_service.inspect(&migration.key, &ConfigurationOverrides { override_identifier: None, resource: resource.cloned() });

        let mut target_pairs: Vec<(InspectKey, ConfigurationTarget)> = vec![
            (InspectKey::User, ConfigurationTarget::User),
            (InspectKey::UserLocal, ConfigurationTarget::UserLocal),
            (InspectKey::UserRemote, ConfigurationTarget::UserRemote),
            (InspectKey::Workspace, ConfigurationTarget::Workspace),
        ];
        if self.workspace_service.get_workbench_state() == WorkbenchState::Workspace {
            target_pairs.push((InspectKey::WorkspaceFolder, ConfigurationTarget::WorkspaceFolder));
        }
        if migration.include_application == Some(true) {
            target_pairs.insert(0, (InspectKey::Application, ConfigurationTarget::Application));
        }

        for (data_key, target) in target_pairs {
            let Some(inspect_value) = data_key.inspect_value(inspect_data.as_ref()) else {
                continue;
            };

            let mut migration_values: Vec<((String, ConfigurationValue), Vec<JsString>)> = Vec::new();

            let value = inspect_value.value();
            if !value.is_undefined() {
                for key_value_pair in self.run_migration(migration, data_key, value, resource, None).await? {
                    migration_values.push((key_value_pair, Vec::new()));
                }
            }

            for OverrideValue { identifiers, value } in inspect_value.overrides().unwrap_or_default() {
                if !value.is_undefined() {
                    for key_value_pair in self.run_migration(migration, data_key, value, resource, Some(identifiers.as_slice())).await? {
                        migration_values.push((key_value_pair, identifiers.clone()));
                    }
                }
            }

            if !migration_values.is_empty() {
                // apply migrations
                let updates: Vec<Promise<()>> = migration_values
                    .into_iter()
                    .map(|((key, value), override_identifiers)| {
                        self.configuration_service.update_value(
                            &key,
                            value.value,
                            Some(ConfigurationUpdateOverrides { override_identifiers: Some(override_identifiers), resource: resource.cloned() }),
                            Some(target),
                        )
                    })
                    .collect();
                // `Promise.allSettled`: espera todas e ignora as falhas.
                for update in updates {
                    let _ = update.await;
                }
            }
        }

        Ok(())
    }

    async fn run_migration(
        &self,
        migration: &ConfigurationMigration,
        data_key: InspectKey,
        value: Dynamic,
        resource: Option<&URI>,
        override_identifiers: Option<&[JsString]>,
    ) -> Result<ConfigurationKeyValuePairs, Error> {
        let configuration_service = self.configuration_service.clone();
        let accessor_resource = resource.cloned();
        let accessor_identifiers: Option<Vec<JsString>> = override_identifiers.map(<[JsString]>::to_vec);
        let value_accessor = move |key: &str| -> Dynamic {
            let inspect_data = configuration_service.inspect(key, &ConfigurationOverrides { override_identifier: None, resource: accessor_resource.clone() });
            let Some(inspect_value) = data_key.inspect_value(inspect_data.as_ref()) else {
                return Dynamic::Undefined;
            };
            match &accessor_identifiers {
                None => inspect_value.value(),
                Some(identifiers) => inspect_value
                    .overrides()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|candidate| candidate.identifiers == *identifiers)
                    .map_or(Dynamic::Undefined, |candidate| candidate.value),
            }
        };
        let result = (migration.migrate_fn)(value, &value_accessor).await?;
        Ok(match result {
            ConfigurationMigrationResult::Pairs(pairs) => pairs,
            ConfigurationMigrationResult::Value(value) => vec![(migration.key.clone(), value)],
        })
    }
}

/// `ConfigurationMigrationWorkbenchContribution`: roda as migrações registradas sobre os valores de cada alvo e de cada
/// pasta do workspace, na criação, quando uma pasta entra e quando uma migração é registrada.
pub struct ConfigurationMigrationWorkbenchContribution {
    base: Rc<Disposable>,
}

impl ConfigurationMigrationWorkbenchContribution {
    pub const ID: &'static str = "workbench.contrib.configurationMigration";

    pub fn new(configuration_service: Rc<dyn IConfigurationService>, workspace_service: Rc<dyn IWorkspaceContextService>) -> Rc<Self> {
        let base = Disposable::new();
        let migrator = Migrator { configuration_service, workspace_service: workspace_service.clone() };
        let registry = configuration_migration_registry();

        base.register(workspace_service.on_did_change_workspace_folders().listen({
            let migrator = migrator.clone();
            let registry = registry.clone();
            move |event| {
                let migrator = migrator.clone();
                let added = event.added.clone();
                let migrations = registry.migrations();
                let _ = async_fn(async move {
                    for folder in &added {
                        migrator.migrate_configurations_for_folder(Some(folder), &migrations).await?;
                    }
                    Ok(())
                });
            }
        }));
        migrator.spawn_migrate_configurations(registry.migrations());
        base.register(registry.on_did_register_configuration_migration().listen({
            let migrator = migrator.clone();
            move |migrations| migrator.spawn_migrate_configurations(migrations.clone())
        }));

        Rc::new(Self { base })
    }
}

impl IDisposable for ConfigurationMigrationWorkbenchContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkbenchContribution for ConfigurationMigrationWorkbenchContribution {
    fn disposable(self: Rc<Self>) -> Option<DisposableRef> {
        Some(self)
    }
}
