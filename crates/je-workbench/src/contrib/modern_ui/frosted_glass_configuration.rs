//! Porte do trecho de `src/vs/workbench/browser/workbench.contribution.ts` que registra as duas configurações do
//! vidro fosco do Modern UI (`LayoutSettings.MODERN_UI_FROSTED_GLASS` e `LayoutSettings.MODERN_UI_FROSTED_GLASS_OPACITY`)
//! e as migrações das chaves antigas (`workbench.experimental.modernUIFrostedGlass` e
//! `workbench.experimental.modernUIFrostedGlassOpacity`).
//!
//! Recortes:
//! - O resto do `workbench.contribution.ts` (as demais configurações do workbench) fica de fora: cada contribuição
//!   registra o seu pedaço, como as outras do porte. O corpo do módulo do original roda no `import`; aqui é a função
//!   [`register_frosted_glass_configuration`], chamada na inicialização.
//!
//! Divergências:
//! - `included: isNative` e o `if (isNative)` das migrações recebem o `is_native` como argumento (o original lê a
//!   constante `isNative` do `platform.ts`); quem inicializa passa `Platform::detect().is_native`.
//! - `localize({ key, comment }, ...)` é o `localize` de `je_base` (o comentário para tradutores não tem efeito).
//! - O `ConfigurationMigration[]` do `.map(([key, newKey]) => ...)` é montado por [`frosted_glass_migration`].

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationScope, PropertySchema, configuration_registry};

use crate::common::configuration::{
    ConfigurationMigration, ConfigurationMigrationResult, ConfigurationValue, IConfigurationMigrationRegistry,
    configuration_migration_registry,
};
use crate::services::editor::editor_resolver_service::workbench_configuration_node_base;
use crate::services::layout::layout_service::{LayoutSettings, ModernUiFrostedGlassOpacity};

/// As chaves antigas e as novas, na ordem do registro.
fn migrated_keys() -> [(&'static str, &'static str); 2] {
    [
        ("workbench.experimental.modernUIFrostedGlass", LayoutSettings::ModernUiFrostedGlass.as_str()),
        ("workbench.experimental.modernUIFrostedGlassOpacity", LayoutSettings::ModernUiFrostedGlassOpacity.as_str()),
    ]
}

/// Uma migração do `.map(([key, newKey]) => ({ key, includeApplication: true, migrateFn }))`.
fn frosted_glass_migration(key: &'static str, new_key: &'static str) -> ConfigurationMigration {
    ConfigurationMigration {
        key: key.to_owned(),
        include_application: Some(true),
        migrate_fn: Rc::new(move |value: Dynamic, value_accessor: &dyn Fn(&str) -> Dynamic| {
            let mut result = Vec::new();
            if !value.is_undefined() {
                result.push((key.to_owned(), ConfigurationValue { value: Dynamic::Undefined }));
                if value_accessor(new_key).is_undefined() {
                    result.push((new_key.to_owned(), ConfigurationValue { value }));
                }
            }
            Promise::resolved(ConfigurationMigrationResult::Pairs(result))
        }),
    }
}

/// As duas propriedades, na ordem do original.
fn frosted_glass_properties(is_native: bool) -> Vec<(&'static str, PropertySchema)> {
    let enabled = PropertySchema::from_entries([
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(true)),
        ("scope", ConfigurationScope::Application.to_dynamic()),
        ("included", Dynamic::Bool(is_native)),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "modernUIFrostedGlass",
                "Adds a softly blurred background to menus, the Command Palette, and other pop-ups. Available in the Agents window and in editor windows with {0} enabled. Always respects reduced transparency in your operating system's accessibility settings, as well as {1}. Uses solid backgrounds in high contrast themes or when the effect isn't supported. Turn this off if you notice display or performance issues. Your operating system's own menus and dialogs are unchanged.",
                &["`#workbench.experimental.modernUI#`", "`#workbench.reduceTransparency#`"],
            )),
        ),
    ]);
    let opacity = PropertySchema::from_entries([
        ("type", Dynamic::string("number")),
        ("default", Dynamic::Number(ModernUiFrostedGlassOpacity::Default as i32 as f64)),
        ("minimum", Dynamic::Number(ModernUiFrostedGlassOpacity::Minimum as i32 as f64)),
        ("maximum", Dynamic::Number(ModernUiFrostedGlassOpacity::Maximum as i32 as f64)),
        ("scope", ConfigurationScope::Application.to_dynamic()),
        ("included", Dynamic::Bool(is_native)),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "modernUIFrostedGlassOpacity",
                "Controls how see-through frosted glass backgrounds are, from 50% to 100%. Lower values reveal more of the blurred content behind them; higher values make the background more solid. Text and controls stay unchanged. Requires {0} and does not override reduced transparency settings in your operating system or VS Code.",
                &["`#workbench.modernUIFrostedGlass#`"],
            )),
        ),
    ]);
    vec![
        (LayoutSettings::ModernUiFrostedGlass.as_str(), enabled),
        (LayoutSettings::ModernUiFrostedGlassOpacity.as_str(), opacity),
    ]
}

/// Registra as duas configurações (escondidas quando não é nativo, `included: isNative`) e, só no nativo, as
/// migrações das chaves antigas.
pub fn register_frosted_glass_configuration(is_native: bool) {
    configuration_registry().register_configuration(ConfigurationNode::new(
        workbench_configuration_node_base().with_properties(frosted_glass_properties(is_native)),
    ));
    if is_native {
        configuration_migration_registry()
            .register_configuration_migrations(migrated_keys().into_iter().map(|(key, new_key)| frosted_glass_migration(key, new_key)).collect());
    }
}
