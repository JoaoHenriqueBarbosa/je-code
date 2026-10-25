//! Porte de `src/vs/workbench/contrib/modernUI/test/browser/frostedGlassConfiguration.test.ts` (suíte
//! `FrostedGlassConfiguration`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `import '../../../../browser/workbench.contribution.js'` registra as configurações ao importar; aqui o
//!   `setup` de cada caso chama `register_frosted_glass_configuration(is_native)`, o trecho do
//!   `workbench.contribution.ts` que registra as duas propriedades e as duas migrações (os registros são por thread, e
//!   cada caso roda no seu).
//! - `isNative` é `Platform::detect().is_native`. `(isNative ? test : test.skip)` vira a saída antecipada
//!   `if !is_native { return; }`, o mesmo que o caso pulado.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo de cada caso.
//! - O `for (const { oldKey, newKey, values, configuredValue } of [...])` vira um caso por combinação, na ordem do
//!   original: para cada configuração, o registro, um caso por valor, o preservado e o ausente.
//! - `await migration?.migrateFn(value, () => undefined)` é o `Promise` já resolvido da migração (`.result()`), e o
//!   `assert.deepStrictEqual` do resultado compara valores dinâmicos: `[[chave, { value }], ...]`.
//! - `value: undefined` (remover a chave) é `Dynamic::Undefined`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::platform::Platform;
use je_platform::configuration::registry::configuration_registry;
use je_workbench::common::configuration::{
    ConfigurationMigration, ConfigurationMigrationResult, configuration_migration_registry,
};
use je_workbench::contrib::modern_ui::frosted_glass_configuration::register_frosted_glass_configuration;
use je_workbench::services::layout::layout_service::LayoutSettings;

const FROSTED_GLASS_OLD_KEY: &str = "workbench.experimental.modernUIFrostedGlass";
const FROSTED_GLASS_OPACITY_OLD_KEY: &str = "workbench.experimental.modernUIFrostedGlassOpacity";

fn is_native() -> bool {
    Platform::detect().is_native
}

/// O `import` do `workbench.contribution.js` e a busca da migração, como no corpo da suíte.
fn migration_for(old_key: &str) -> Option<ConfigurationMigration> {
    register_frosted_glass_configuration(is_native());
    configuration_migration_registry().migrations().into_iter().find(|migration| migration.key == old_key)
}

fn pairs_as_dynamic(pairs: &[(&str, Dynamic)]) -> Dynamic {
    Dynamic::array(pairs.iter().map(|(key, value)| Dynamic::array([Dynamic::string(key), Dynamic::object([("value", value.clone())])])))
}

/// `await migration?.migrateFn(value, accessor)` como `ConfigurationKeyValuePairs` dinâmico.
fn migrate(migration: &ConfigurationMigration, value: Dynamic, configured: Dynamic) -> Dynamic {
    let result = (migration.migrate_fn)(value, &|_| configured.clone()).result().expect("a migração já resolveu").expect("a migração não rejeita");
    match result {
        ConfigurationMigrationResult::Pairs(pairs) => Dynamic::array(
            pairs.into_iter().map(|(key, value)| Dynamic::array([Dynamic::string(&key), Dynamic::object([("value", value.value)])])),
        ),
        ConfigurationMigrationResult::Value(_) => panic!("a migração devolve pares chave e valor"),
    }
}

fn only_registers_migration_where_setting_is_available(old_key: &str, new_key: LayoutSettings) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let migration = migration_for(old_key);
    let setting_registered = configuration_registry().get_configuration_properties().contains(new_key.as_str());
    assert_eq!(
        (setting_registered, migration.is_some()),
        (is_native(), is_native()),
    );
}

fn migrates_with_value(old_key: &str, new_key: LayoutSettings, value: Dynamic) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !is_native() {
        return;
    }
    let migration = migration_for(old_key).expect("a migração está registrada no nativo");
    assert_eq!(migration.include_application, Some(true));
    assert_deep_strict_equal(
        &migrate(&migration, value.clone(), Dynamic::Undefined),
        &pairs_as_dynamic(&[(old_key, Dynamic::Undefined), (new_key.as_str(), value)]),
    );
}

fn preserves_explicitly_configured(old_key: &str, last_value: Dynamic, configured_value: Dynamic) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !is_native() {
        return;
    }
    let migration = migration_for(old_key).expect("a migração está registrada no nativo");
    assert_deep_strict_equal(&migrate(&migration, last_value, configured_value), &pairs_as_dynamic(&[(old_key, Dynamic::Undefined)]));
}

fn does_not_create_when_old_setting_is_absent(old_key: &str) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !is_native() {
        return;
    }
    let migration = migration_for(old_key).expect("a migração está registrada no nativo");
    assert_deep_strict_equal(&migrate(&migration, Dynamic::Undefined, Dynamic::Undefined), &pairs_as_dynamic(&[]));
}

#[test]
fn only_registers_frosted_glass_migration_where_setting_is_available() {
    only_registers_migration_where_setting_is_available(FROSTED_GLASS_OLD_KEY, LayoutSettings::ModernUiFrostedGlass);
}

#[test]
fn migrates_frosted_glass_with_value_false() {
    migrates_with_value(FROSTED_GLASS_OLD_KEY, LayoutSettings::ModernUiFrostedGlass, Dynamic::Bool(false));
}

#[test]
fn migrates_frosted_glass_with_value_true() {
    migrates_with_value(FROSTED_GLASS_OLD_KEY, LayoutSettings::ModernUiFrostedGlass, Dynamic::Bool(true));
}

#[test]
fn preserves_explicitly_configured_frosted_glass_during_migration() {
    preserves_explicitly_configured(FROSTED_GLASS_OLD_KEY, Dynamic::Bool(true), Dynamic::Bool(false));
}

#[test]
fn does_not_create_frosted_glass_when_the_old_setting_is_absent() {
    does_not_create_when_old_setting_is_absent(FROSTED_GLASS_OLD_KEY);
}

#[test]
fn only_registers_frosted_glass_opacity_migration_where_setting_is_available() {
    only_registers_migration_where_setting_is_available(FROSTED_GLASS_OPACITY_OLD_KEY, LayoutSettings::ModernUiFrostedGlassOpacity);
}

#[test]
fn migrates_frosted_glass_opacity_with_value_50() {
    migrates_with_value(FROSTED_GLASS_OPACITY_OLD_KEY, LayoutSettings::ModernUiFrostedGlassOpacity, Dynamic::Number(50.0));
}

#[test]
fn migrates_frosted_glass_opacity_with_value_75_5() {
    migrates_with_value(FROSTED_GLASS_OPACITY_OLD_KEY, LayoutSettings::ModernUiFrostedGlassOpacity, Dynamic::Number(75.5));
}

#[test]
fn migrates_frosted_glass_opacity_with_value_100() {
    migrates_with_value(FROSTED_GLASS_OPACITY_OLD_KEY, LayoutSettings::ModernUiFrostedGlassOpacity, Dynamic::Number(100.0));
}

#[test]
fn preserves_explicitly_configured_frosted_glass_opacity_during_migration() {
    preserves_explicitly_configured(FROSTED_GLASS_OPACITY_OLD_KEY, Dynamic::Number(100.0), Dynamic::Number(75.0));
}

#[test]
fn does_not_create_frosted_glass_opacity_when_the_old_setting_is_absent() {
    does_not_create_when_old_setting_is_absent(FROSTED_GLASS_OPACITY_OLD_KEY);
}
