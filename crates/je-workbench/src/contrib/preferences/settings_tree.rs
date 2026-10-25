//! Porte parcial de `src/vs/workbench/contrib/preferences/browser/settingsTree.ts`: o que o editor de configurações usa
//! da árvore sem depender de widget. A resolução do sumário em árvore de configurações (`resolveSettingsTree`,
//! `getMatchingSettings`, `sortSettings`, `getFlatSettings`), as configurações restritas do espaço não confiável
//! (`resolveConfiguredUntrustedSettings`), o sumário das extensões (`createTocTreeForExtensionSettings`) e o filtro da
//! árvore (`SettingsTreeFilter`). Os renderizadores da faixa 856 a 1327 (a base, o grupo, as extensões novas e o complexo) estão em
//! `settings_tree_renderers.rs`; o resto dos renderizadores, o delegado de alturas e o `SettingsTree` em si são widgets e entram
//! com as fatias seguintes.
//!
//! Divergências:
//! - **`Set<ISetting>`** (com `delete` durante o `forEach`) é o [`SettingSet`], um `IndexMap` ordenado pela identidade
//!   (o endereço do `Rc`) que o `getMatchingSettings` percorre sobre uma cópia e esvazia por `shift_remove`, e que mantém a
//!   ordem de inserção do `Set`.
//! - **`resolveSettingsTree(..., logService)`**: o `ILogService` é repassado pelo original até o `_resolveSettingsTree`, que
//!   nunca o lê; aqui o parâmetro não existe.
//! - **`createSettingMatchRegExp`** vive em `settings_tree_models.rs` (o original o define aqui e o modelo o importa, um ciclo
//!   de módulos que o TypeScript tolera), e o `settingPatternCache` é um `thread_local`.
//! - **`SettingsTreeFilter`** implementa o `ITreeFilter` sobre o [`SettingsTreeGroupChild`] (a união dos três elementos), e o
//!   `@IWorkbenchEnvironmentService` do construtor é o `Rc<dyn ...>` passado em [`SettingsTreeFilter::new`].
//! - **`createTocTreeForExtensionSettings`** devolve uma `Promise`; o trabalho síncrono de cada grupo (filtro e ordenação)
//!   roda na hora, como no `processGroupEntry`, e só a consulta da extensão espera.

use std::cell::RefCell;
use std::collections::HashMap;
use std::cmp::Ordering;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::collator::locale_compare;
use je_base::errors::{self, Error};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_widgets::tree::tree_types::{ITreeFilter, TreeFilterResult, TreeVisibility};
use zyarr::RegExp;

use crate::contrib::preferences::common::preferences::compare_two_nullable_numbers;
use crate::contrib::preferences::preferences_widgets::SettingsTarget;
use crate::contrib::preferences::settings_layout::{ITOCEntry, ITOCFilter, ITOCFilterPatterns};
use crate::contrib::preferences::settings_tree_models::{
    SettingsTreeElement, SettingsTreeGroupChild, SettingsTreeGroupElement, ViewStateRef, create_setting_match_reg_exp, inspect_setting,
};
use crate::services::configuration::common::configuration::IWorkbenchConfigurationService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::extensions::extensions::IExtensionService;
use crate::services::preferences::preferences::{ISetting, ISettingsGroup};

const MULTI_GROUP_TOC_SETTINGS: [&str; 2] = ["accessibility.signals.chatUserActionRequired", "accessibility.signals.chatResponseReceived"];

/// O `Set<ISetting>` do original: as configurações por identidade, na ordem de inserção.
pub type SettingSet = IndexMap<usize, Rc<ISetting>>;

/// A identidade de uma configuração (o `===` do JavaScript).
fn setting_id(setting: &Rc<ISetting>) -> usize {
    Rc::as_ptr(setting) as usize
}

/// `setting.tags?.includes(tag)`.
fn has_tag(setting: &ISetting, tag: &str) -> bool {
    setting.tags.as_ref().is_some_and(|tags| tags.iter().any(|candidate| candidate == tag))
}

/// `resolveSettingsTree`: o resultado, com a árvore e as configurações que o sumário não incluiu.
pub struct ResolvedSettingsTree {
    pub tree: ITOCEntry<Rc<ISetting>>,
    pub leftover_settings: SettingSet,
}

pub fn resolve_settings_tree(toc_data: &ITOCEntry<String>, core_settings_groups: &[ISettingsGroup], filter: Option<&ITOCFilter>) -> ResolvedSettingsTree {
    let mut all_settings = get_flat_settings(core_settings_groups);
    let tree = resolve_settings_tree_node(toc_data, &mut all_settings, filter);
    ResolvedSettingsTree { tree, leftover_settings: all_settings }
}

/// `resolveConfiguredUntrustedSettings`.
pub fn resolve_configured_untrusted_settings(
    groups: &[ISettingsGroup],
    target: &SettingsTarget,
    language_filter: Option<&str>,
    configuration_service: &dyn IWorkbenchConfigurationService,
) -> Vec<Rc<ISetting>> {
    get_flat_settings(groups)
        .into_values()
        .filter(|setting| setting.restricted.unwrap_or(false) && inspect_setting(&setting.key, target, language_filter, configuration_service).is_configured)
        .collect()
}

/// `createTocTreeForExtensionSettings`.
pub fn create_toc_tree_for_extension_settings(
    extension_service: Rc<dyn IExtensionService>,
    groups: &[ISettingsGroup],
    filter: Option<&ITOCFilter>,
) -> Promise<ITOCEntry<Rc<ISetting>>> {
    let ext_group_tree: Rc<RefCell<IndexMap<String, ITOCEntry<Rc<ISetting>>>>> = Rc::default();

    let mut process_promises: Vec<Promise<()>> = Vec::new();
    for group in groups {
        // `processGroupEntry`: a parte síncrona.
        let flat_settings: Vec<Rc<ISetting>> = group.sections.iter().flat_map(|section| section.settings.iter().cloned()).collect();
        let mut settings = match filter {
            Some(filter) => {
                let mut set: SettingSet = flat_settings.iter().map(|setting| (setting_id(setting), setting.clone())).collect();
                get_matching_settings(&mut set, filter)
            }
            None => flat_settings,
        };
        sort_settings(&mut settings);

        let extension_id = group.extension_info.as_ref().expect("o grupo de extensão tem extensionInfo").id.clone();

        // There could be multiple groups with the same extension id that all belong to the same extension.
        // To avoid highlighting all groups upon expanding the extension's ToC entry,
        // use the group ID only if it is non-empty and isn't the extension ID.
        // Ref https://github.com/microsoft/vscode/issues/241521.
        let setting_group_id = if !group.id.is_empty() && group.id != extension_id { group.id.clone() } else { group.title.clone() };
        let child_entry = ITOCEntry { id: setting_group_id, label: group.title.clone(), order: group.order, children: None, settings: Some(settings), hide: None };

        let ext_group_tree = ext_group_tree.clone();
        process_promises.push(extension_service.get_extension(&extension_id).then(move |extension| {
            let extension_name = extension.and_then(|extension| extension.display_name.or(extension.name)).unwrap_or_else(|| extension_id.clone());
            // `addEntryToTree`
            let mut ext_group_tree = ext_group_tree.borrow_mut();
            let root_entry = ext_group_tree.entry(extension_id.clone()).or_insert_with(|| ITOCEntry {
                id: extension_id.clone(),
                label: extension_name,
                order: None,
                children: Some(Vec::new()),
                settings: None,
                hide: None,
            });
            root_entry.children.get_or_insert_with(Vec::new).push(child_entry);
        }));
    }

    Promise::all(process_promises).then(move |_| {
        let mut ext_groups: Vec<ITOCEntry<Rc<ISetting>>> = Vec::new();
        let roots: Vec<ITOCEntry<Rc<ISetting>>> = ext_group_tree.borrow_mut().drain(..).map(|(_, root)| root).collect();
        for mut extension_root_entry in roots {
            let mut children = extension_root_entry.children.take().unwrap_or_default();
            if children.len() == 1 {
                // There is a single category for this extension.
                // Push a flattened setting.
                let only_child = children.remove(0);
                ext_groups.push(ITOCEntry {
                    id: extension_root_entry.id,
                    label: only_child.label,
                    order: None,
                    children: None,
                    settings: only_child.settings,
                    hide: None,
                });
            } else {
                // Sort the categories.
                // Leave the undefined order categories untouched.
                children.sort_by(|a, b| compare_to_ordering(compare_two_nullable_numbers(a.order, b.order)));

                // If there is a category that matches the setting name,
                // add the settings in manually as "ungrouped" settings.
                // https://github.com/microsoft/vscode/issues/137259
                let ungrouped_index = children.iter().position(|child| child.label == extension_root_entry.label);
                match ungrouped_index {
                    Some(index) if children[index].children.is_none() => {
                        let ungrouped_child = children.remove(index);
                        ext_groups.push(ITOCEntry {
                            id: extension_root_entry.id,
                            label: extension_root_entry.label,
                            order: None,
                            children: Some(children),
                            settings: ungrouped_child.settings,
                            hide: None,
                        });
                    }
                    _ => {
                        // Push all the groups as-is.
                        extension_root_entry.children = Some(children);
                        ext_groups.push(extension_root_entry);
                    }
                }
            }
        }

        // Sort the outermost settings.
        ext_groups.sort_by(|a, b| locale_compare(&a.label, &b.label));

        ITOCEntry { id: "extensions".to_owned(), label: localize("extensions", "Extensions", &[]), order: None, children: Some(ext_groups), settings: None, hide: None }
    })
}

/// O resultado de `compareTwoNullableNumbers` (-1, 0 ou 1) como [`Ordering`].
fn compare_to_ordering(result: f64) -> Ordering {
    if result < 0.0 {
        Ordering::Less
    } else if result > 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

fn resolve_settings_tree_node(toc_data: &ITOCEntry<String>, all_settings: &mut SettingSet, filter: Option<&ITOCFilter>) -> ITOCEntry<Rc<ISetting>> {
    let children: Option<Vec<ITOCEntry<Rc<ISetting>>>> = toc_data.children.as_ref().map(|toc_children| {
        toc_children
            .iter()
            .filter(|child| child.hide != Some(true))
            .map(|child| resolve_settings_tree_node(child, all_settings, filter))
            .filter(|child| child.children.as_ref().is_some_and(|children| !children.is_empty()) || child.settings.as_ref().is_some_and(|settings| !settings.is_empty()))
            .collect()
    });

    let settings: Option<Vec<Rc<ISetting>>> = if filter.is_some() || toc_data.settings.is_some() {
        let include = filter.and_then(|filter| filter.include.as_ref());
        let mut key_patterns: Vec<String> = include.and_then(|include| include.key_patterns.clone()).unwrap_or_default();
        key_patterns.extend(toc_data.settings.iter().flatten().cloned());
        let matching_filter = ITOCFilter {
            include: Some(ITOCFilterPatterns { key_patterns: Some(key_patterns), tags: Some(include.and_then(|include| include.tags.clone()).unwrap_or_default()) }),
            exclude: Some(filter.and_then(|filter| filter.exclude.clone()).unwrap_or_default()),
        };
        let mut settings = get_matching_settings(all_settings, &matching_filter);
        sort_settings(&mut settings);
        Some(settings)
    } else {
        None
    };

    if children.is_none() && settings.is_none() {
        errors::throw(Error::new(format!("TOC node has no child groups or settings: {}", toc_data.id)));
    }

    ITOCEntry { id: toc_data.id.clone(), label: toc_data.label.clone(), order: None, children, settings, hide: None }
}

/// Sort settings so that preview and experimental settings are deprioritized.
/// Within each tier, sort the settings by order, then alphabetically.
fn sort_settings(settings: &mut [Rc<ISetting>]) {
    const SETTING_STATUS_NORMAL: u8 = 0;
    const SETTING_STATUS_PREVIEW: u8 = 1;
    const SETTING_STATUS_EXPERIMENTAL: u8 = 2;

    let experimental_status = |setting: &ISetting| {
        if has_tag(setting, "experimental") {
            SETTING_STATUS_EXPERIMENTAL
        } else if has_tag(setting, "preview") {
            SETTING_STATUS_PREVIEW
        } else {
            SETTING_STATUS_NORMAL
        }
    };

    settings.sort_by(|a, b| {
        let (experimental_status_a, experimental_status_b) = (experimental_status(a), experimental_status(b));
        if experimental_status_a != experimental_status_b {
            return experimental_status_a.cmp(&experimental_status_b);
        }

        compare_to_ordering(compare_two_nullable_numbers(a.order, b.order)).then_with(|| locale_compare(&a.key, &b.key))
    });
}

/// `getMatchingSettings`: as configurações do conjunto que o filtro inclui (e não exclui), tiradas do conjunto, menos as
/// que o sumário repete em mais de um grupo.
fn get_matching_settings(all_settings: &mut SettingSet, filter: &ITOCFilter) -> Vec<Rc<ISetting>> {
    let mut result: Vec<Rc<ISetting>> = Vec::new();

    let snapshot: Vec<Rc<ISetting>> = all_settings.values().cloned().collect();
    for setting in snapshot {
        let matches_pattern = |pattern: &String| match pattern.strip_prefix("@tag:") {
            Some(tag_name) => has_tag(&setting, tag_name),
            None => setting_matches(&setting, pattern),
        };

        // Check include filters
        let mut should_include = match filter.include.as_ref().and_then(|include| include.key_patterns.as_ref()) {
            Some(key_patterns) => key_patterns.iter().any(matches_pattern),
            None => true,
        };

        if should_include && let Some(tags) = filter.include.as_ref().and_then(|include| include.tags.as_ref()).filter(|tags| !tags.is_empty()) {
            should_include = tags.iter().any(|tag| has_tag(&setting, tag));
        }

        // Check exclude filters (takes precedence)
        let mut should_exclude = match filter.exclude.as_ref().and_then(|exclude| exclude.key_patterns.as_ref()) {
            Some(key_patterns) => key_patterns.iter().any(matches_pattern),
            None => false,
        };

        if !should_exclude && let Some(tags) = filter.exclude.as_ref().and_then(|exclude| exclude.tags.as_ref()).filter(|tags| !tags.is_empty()) {
            should_exclude = tags.iter().any(|tag| has_tag(&setting, tag));
        }

        // Include if matches include filter and doesn't match exclude filter
        if should_include && !should_exclude {
            result.push(setting.clone());
            if !MULTI_GROUP_TOC_SETTINGS.contains(&setting.key.as_str()) {
                all_settings.shift_remove(&setting_id(&setting));
            }
        }
    }

    result
}

thread_local! {
    /// `settingPatternCache`.
    static SETTING_PATTERN_CACHE: RefCell<HashMap<String, Rc<RegExp>>> = RefCell::new(HashMap::new());
}

/// `settingMatches`.
fn setting_matches(setting: &ISetting, pattern: &str) -> bool {
    let reg_exp = SETTING_PATTERN_CACHE.with(|cache| cache.borrow_mut().entry(pattern.to_owned()).or_insert_with(|| Rc::new(create_setting_match_reg_exp(pattern))).clone());
    reg_exp.exec(&setting.key.encode_utf16().collect::<Vec<u16>>(), 0).is_some()
}

/// `getFlatSettings`.
fn get_flat_settings(settings_groups: &[ISettingsGroup]) -> SettingSet {
    let mut result = SettingSet::new();

    for group in settings_groups {
        for section in &group.sections {
            for setting in &section.settings {
                if setting.overrides.as_ref().is_none_or(Vec::is_empty) {
                    result.insert(setting_id(setting), setting.clone());
                }
            }
        }
    }

    result
}

/// `SettingsTreeFilter`.
pub struct SettingsTreeFilter {
    view_state: ViewStateRef,
    is_filtering_groups: bool,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
}

impl SettingsTreeFilter {
    pub fn new(view_state: ViewStateRef, is_filtering_groups: bool, environment_service: Rc<dyn IWorkbenchEnvironmentService>) -> Self {
        Self { view_state, is_filtering_groups, environment_service }
    }

    fn setting_contained_in_group(setting: &ISetting, group: &Rc<SettingsTreeGroupElement>) -> bool {
        group.children().iter().any(|child| match child {
            SettingsTreeGroupChild::Group(child) => Self::setting_contained_in_group(setting, child),
            SettingsTreeGroupChild::Setting(child) => child.setting.key == setting.key,
            SettingsTreeGroupChild::NewExtensions(_) => false,
        })
    }

    /// Checks if a group is related to the filtered category.
    /// A group is related if it's the category itself, a descendant of it, or an ancestor of it.
    fn group_is_related_to_category(group: &Rc<SettingsTreeGroupElement>, category: &Rc<SettingsTreeGroupElement>) -> bool {
        // Check if this group is the category itself
        if group.id() == category.id() {
            return true;
        }

        // Check if this group is a descendant of the category
        let mut parent = group.parent();
        while let Some(current) = parent {
            if current.id() == category.id() {
                return true;
            }
            parent = current.parent();
        }

        // Check if this group is an ancestor of the category
        let mut category_parent = category.parent();
        while let Some(current) = category_parent {
            if current.id() == group.id() {
                return true;
            }
            category_parent = current.parent();
        }

        false
    }
}

impl ITreeFilter<SettingsTreeGroupChild> for SettingsTreeFilter {
    fn filter(&self, element: &SettingsTreeGroupChild, _parent_visibility: TreeVisibility) -> TreeFilterResult<()> {
        let view_state = self.view_state.borrow();

        // Filter during search
        if let (Some(category_filter), SettingsTreeGroupChild::Setting(setting)) = (&view_state.category_filter, element)
            && !Self::setting_contained_in_group(&setting.setting, category_filter)
        {
            return TreeFilterResult::Bool(false);
        }

        // Non-user scope selected
        if let SettingsTreeGroupChild::Setting(setting) = element
            && view_state.settings_target != SettingsTarget::UserLocal
        {
            let is_remote = self.environment_service.remote_authority().is_some_and(|authority| !authority.is_empty());
            if !setting.matches_scope(&view_state.settings_target, is_remote) {
                return TreeFilterResult::Bool(false);
            }
        }

        // Group with no visible children
        if let SettingsTreeGroupChild::Group(group) = element {
            // When filtering to a specific category, only show that category and its descendants
            if self.is_filtering_groups
                && let Some(category_filter) = &view_state.category_filter
            {
                if !Self::group_is_related_to_category(group, category_filter) {
                    return TreeFilterResult::Bool(false);
                }
                // For groups related to the category, skip the count check and recurse
                // to let child settings be filtered
                return TreeFilterResult::Visibility(TreeVisibility::Recurse);
            }

            if let Some(count) = group.count() {
                return TreeFilterResult::Bool(count > 0);
            }

            return TreeFilterResult::Visibility(TreeVisibility::Recurse);
        }

        // Filtered "new extensions" button
        if matches!(element, SettingsTreeGroupChild::NewExtensions(_)) && (view_state.tag_filters.as_ref().is_some_and(|tag_filters| !tag_filters.is_empty()) || view_state.category_filter.is_some()) {
            return TreeFilterResult::Bool(false);
        }

        TreeFilterResult::Bool(true)
    }
}
