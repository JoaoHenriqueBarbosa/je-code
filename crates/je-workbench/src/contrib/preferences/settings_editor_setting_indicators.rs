//! Porte parcial de `src/vs/workbench/contrib/preferences/browser/settingsEditorSettingIndicators.ts`: o texto
//! acessível dos indicadores de uma configuração (`getIndicatorsLabelAriaLabel`) e o que ele usa. O desenho dos
//! indicadores (`SettingsTreeIndicatorsLabel`, os `hover`s e os links de escopo) depende da árvore de widgets nativa e
//! entra com o porte das telas de preferências.
//!
//! Divergências:
//! - **`getIndicatorsLabelAriaLabel`** não recebe o `userDataProfilesService`: o original o declara e nunca o lê.
//! - **`cachedSyncIgnoredSettingsSet`** (um `let` do módulo que o `SettingsTreeIndicatorsLabel` atualiza) é um estado
//!   por thread, atualizado por [`cache_sync_ignored_settings`]; o `cachedSyncIgnoredSettings !== ignoredSettings` do
//!   original compara a identidade do array, e aqui compara o conteúdo.
//! - **`languageService.getLanguageName(language)`** devolve `string | null`; o `null` entra na frase como o texto
//!   `null`, que é o que o `localize` do original faz com o argumento.
//! - O tipo `string | undefined | string[]` de `getDefaultValueSourceToDisplay` é o enum [`SourceToDisplay`].

use std::cell::RefCell;
use std::collections::HashSet;

use je_base::nls::localize;
use je_editor::language_service::ILanguageService;
use je_platform::configuration::registry::{ConfigurationDefaultSource, ConfigurationDefaultValueSource};

use crate::contrib::preferences::preferences_widgets::SettingsTarget;
use crate::contrib::preferences::settings_tree_models::SettingsTreeSettingElement;
use crate::services::configuration::common::configuration::IWorkbenchConfigurationService;

thread_local! {
    /// `cachedSyncIgnoredSettingsSet`: contains a set of the sync-ignored settings to keep the sync ignored indicator
    /// and the `getIndicatorsLabelAriaLabel()` function in sync.
    static CACHED_SYNC_IGNORED_SETTINGS_SET: RefCell<HashSet<String>> = RefCell::new(HashSet::new());

    /// `cachedSyncIgnoredSettings`: contains a copy of the sync-ignored settings to determine when to update
    /// `cachedSyncIgnoredSettingsSet`.
    static CACHED_SYNC_IGNORED_SETTINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// A atualização do `cachedSyncIgnoredSettingsSet` que o `SettingsTreeIndicatorsLabel#updateSyncIgnored` faz.
pub fn cache_sync_ignored_settings(ignored_settings: &[String]) {
    CACHED_SYNC_IGNORED_SETTINGS.with(|cached| {
        if cached.borrow().as_slice() != ignored_settings {
            *cached.borrow_mut() = ignored_settings.to_vec();
            CACHED_SYNC_IGNORED_SETTINGS_SET.with(|set| *set.borrow_mut() = ignored_settings.iter().cloned().collect());
        }
    });
}

/// O que `getDefaultValueSourceToDisplay` devolve: um nome ou vários.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceToDisplay {
    Single(String),
    Many(Vec<String>),
}

/// `displayName ?? id` de uma origem, ou o próprio texto.
fn source_name(source: &ConfigurationDefaultSource) -> String {
    match source {
        ConfigurationDefaultSource::Text(text) => text.clone(),
        ConfigurationDefaultSource::Extension(extension) => extension.display_name.clone().unwrap_or_else(|| extension.id.clone()),
    }
}

fn get_default_value_source_to_display(element: &SettingsTreeSettingElement) -> Option<SourceToDisplay> {
    match element.default_value_source()? {
        ConfigurationDefaultValueSource::Map(default_value_source) => {
            let mut source_to_display: Vec<String> = Vec::new();
            for (_, value) in default_value_source.borrow().iter() {
                let new_value = source_name(value);
                if !source_to_display.contains(&new_value) {
                    source_to_display.push(new_value);
                }
            }
            Some(SourceToDisplay::Many(source_to_display))
        }
        ConfigurationDefaultValueSource::Source(source) => Some(SourceToDisplay::Single(source_name(&source))),
    }
}

/// O nome do idioma como o `localize` o escreve (`null` quando o serviço não o conhece).
fn language_name(language_service: &dyn ILanguageService, language: &str) -> String {
    language_service.get_language_name(language).unwrap_or_else(|| "null".to_owned())
}

/// `user`, `workspace` ou `remote`, localizado.
fn localized_scope(scope: &str) -> String {
    match scope {
        "user" => localize("user", "User", &[]),
        "workspace" => localize("workspace", "Workspace", &[]),
        _ => localize("remote", "Remote", &[]),
    }
}

/// `completeScope.split(':')` em `[scope, language]` (o idioma ausente ou vazio é `undefined` ou falso).
fn split_scope(complete_scope: &str) -> (&str, Option<&str>) {
    let mut parts = complete_scope.split(':');
    let scope = parts.next().unwrap_or_default();
    (scope, parts.next().filter(|language| !language.is_empty()))
}

#[allow(dead_code)] // usado pelo indicador de escopo da interface, que ainda não foi portado
fn get_accessible_scope_display_text(complete_scope: &str, language_service: &dyn ILanguageService) -> String {
    let (scope, language) = split_scope(complete_scope);
    let localized_scope = localized_scope(scope);
    if let Some(language) = language {
        return localize("modifiedInScopeForLanguage", "The {0} scope for {1}", &[&localized_scope, &language_name(language_service, language)]);
    }
    localized_scope
}

fn get_accessible_scope_display_mid_sentence_text(complete_scope: &str, language_service: &dyn ILanguageService) -> String {
    let (scope, language) = split_scope(complete_scope);
    let localized_scope = localized_scope(scope);
    if let Some(language) = language {
        return localize("modifiedInScopeForLanguageMidSentence", "the {0} scope for {1}", &[&localized_scope.to_lowercase(), &language_name(language_service, language)]);
    }
    localized_scope
}

/// `getIndicatorsLabelAriaLabel`.
pub fn get_indicators_label_aria_label(
    element: &SettingsTreeSettingElement,
    configuration_service: &dyn IWorkbenchConfigurationService,
    language_service: &dyn ILanguageService,
) -> String {
    let mut aria_label_sections: Vec<String> = Vec::new();
    let tags = element.tags();
    let has_tag = |tag: &str| tags.as_ref().is_some_and(|tags| tags.contains(tag));

    // Add preview or experimental indicator text
    if has_tag("preview") {
        aria_label_sections.push(localize("previewLabel", "Preview", &[]));
    } else if has_tag("experimental") {
        aria_label_sections.push(localize("experimentalLabel", "Experimental", &[]));
    }

    if has_tag("advanced") {
        aria_label_sections.push(localize("advancedLabel", "Advanced", &[]));
    }

    // Add workspace trust text
    if element.is_untrusted() {
        aria_label_sections.push(localize("workspaceUntrustedAriaLabel", "Workspace untrusted; setting value not applied", &[]));
    }

    if element.has_policy_value() {
        aria_label_sections
            .push(element.policy_value_description().unwrap_or_else(|| localize("policyDescriptionAccessible", "Managed by organization policy; setting value not applied", &[])));
    } else if element.is_agents_window_read_only() {
        aria_label_sections.push(localize("agentsWindowReadOnlyAccessible", "Cannot be changed in Agents window", &[]));
    } else if element.settings_target == SettingsTarget::UserLocal && configuration_service.is_setting_applied_for_all_profiles(&element.setting.key) {
        aria_label_sections.push(localize("applicationSettingDescriptionAccessible", "Setting value retained when switching profiles", &[]));
    } else {
        // Add other overrides text
        let other_overrides_start =
            if element.is_configured() { localize("alsoConfiguredIn", "Also modified in", &[]) } else { localize("configuredIn", "Modified in", &[]) };
        let overridden_scope_list = element.overridden_scope_list();
        let other_overrides_list =
            overridden_scope_list.iter().map(|scope| get_accessible_scope_display_mid_sentence_text(scope, language_service)).collect::<Vec<_>>().join(", ");
        if !overridden_scope_list.is_empty() {
            aria_label_sections.push(format!("{other_overrides_start} {other_overrides_list}"));
        }
    }

    // Add sync ignored text
    if CACHED_SYNC_IGNORED_SETTINGS_SET.with(|set| set.borrow().contains(&element.setting.key)) {
        aria_label_sections.push(localize("syncIgnoredAriaLabel", "Setting ignored during sync", &[]));
    }

    // Add default override indicator text
    let mut source_to_display = get_default_value_source_to_display(element);
    if let Some(source) = source_to_display.take() {
        let source = match source {
            SourceToDisplay::Many(sources) if sources.len() == 1 => SourceToDisplay::Single(sources[0].clone()),
            other => other,
        };

        let overridden_details_text = match source {
            SourceToDisplay::Single(source) => localize("defaultOverriddenDetailsAriaLabel", "{0} overrides the default value", &[&source]),
            SourceToDisplay::Many(sources) => {
                let (last, rest) = match sources.split_last() {
                    Some((last, rest)) => (last.clone(), rest.join(", ")),
                    None => (String::new(), String::new()),
                };
                localize("multipleDefaultOverriddenDetailsAriaLabel", "{0} override the default value", &[&format!("{rest} & {last}")])
            }
        };
        aria_label_sections.push(overridden_details_text);
    }

    // Add text about default values being overridden in other languages
    let overridden_defaults_language_list = element.overridden_defaults_language_list();
    let other_language_overrides_list =
        overridden_defaults_language_list.iter().map(|language| language_name(language_service, language)).collect::<Vec<_>>().join(", ");
    if !overridden_defaults_language_list.is_empty() {
        let other_language_overrides_text = localize("defaultOverriddenLanguagesList", "Language-specific default values exist for {0}", &[&other_language_overrides_list]);
        aria_label_sections.push(other_language_overrides_text);
    }

    aria_label_sections.join(". ")
}
