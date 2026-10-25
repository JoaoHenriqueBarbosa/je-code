//! Porte de `src/vs/workbench/contrib/preferences/browser/settingsTree.ts`: o que o editor de configurações usa da árvore. A
//! resolução do sumário em árvore de configurações (`resolveSettingsTree`, `getMatchingSettings`, `sortSettings`,
//! `getFlatSettings`), as configurações restritas do espaço não confiável (`resolveConfiguredUntrustedSettings`), o sumário das
//! extensões (`createTocTreeForExtensionSettings`), o filtro da árvore (`SettingsTreeFilter`) e, da linha 2540 em diante, o
//! delegado de alturas (`SettingsTreeDelegate`), o modelo sem colapso (`NonCollapsibleObjectTreeModel`), o provedor de
//! acessibilidade (`SettingsTreeAccessibilityProvider`) e o `SettingsTree` sobre a `WorkbenchObjectTree`. Os renderizadores, o
//! `SettingTreeRenderers` e as ações de copiar, sincronizar e aplicar a todos os perfis estão em `settings_tree_renderers.rs`.
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
//! - **`SettingsTreeDelegate`** compõe o [`CachedListVirtualDelegate`] (que o original estende) e acrescenta o `hasDynamicHeight`.
//!   O `throw new Error('unknown element type')` não existe: o [`SettingsTreeGroupChild`] só tem os três elementos.
//! - **`NonCollapsibleObjectTreeModel`** é uma envoltória do `ObjectTreeModel` que implementa o `ITreeModel` repassando tudo e
//!   respondendo `false` em `isCollapsible` e `setCollapsed`, e dá o `ObjectTreeModel` por `Deref` e por `Borrow` (o
//!   `setChildren`). O `createModel` protegido que o `SettingsTree` sobrescreve é o parâmetro `create_model` do
//!   `WorkbenchObjectTree::with_model`: o modelo é o terceiro parâmetro de tipo da árvore ([`SettingsObjectTreeModel`]).
//! - **`SettingsTreeAccessibilityProvider`** implementa o [`IListAccessibilityProvider`] sobre o elemento da árvore de objetos
//!   (`T | null`, a raiz não tem rótulo) e vai nas opções da árvore (`accessibilityProvider`), de onde a lista aplica o
//!   `aria-label` aos nós e o da árvore. O `IUserDataProfilesService` do construtor não é passado, porque o original nunca o lê.
//! - **`SettingsTree`**: o `styleController` (`DefaultStyleController` sobre uma folha de estilo) é o estilo da lista resolvido do
//!   tema, entregue em `overrideStyles` (o `this.style(getListStyles({...}))` do original). `horizontalScrolling`,
//!   `scrollToActiveElement` e `transformOptimization` não existem nas opções do `je-widgets`. O `IContextKeyService`, o
//!   `IListService` e a `IInstantiationService` são o [`WorkbenchTreeServices`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::cmp::Ordering;
use std::hash::Hash;
use std::ops::Deref;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::collator::locale_compare;
use je_base::dynamic::Truthy;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::markdown_plaintext::render_as_plaintext;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_editor::language_service::ILanguageService;
use je_theme::ColorTheme;
use je_ui::{NodeId, Ui};
use je_widgets::list::list_styles::IListStyles;
use je_widgets::list::list_types::{CachedListVirtualDelegate, IIdentityProvider, IListAccessibilityProvider, IListVirtualDelegate};
use je_widgets::tree::abstract_tree::{AbstractTreeOptions, AbstractTreeOptionsUpdate};
use je_widgets::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelOptions};
use je_widgets::tree::tree_renderer::RenderIndentGuides;
use je_widgets::tree::tree_types::{
    CollapseStateChangeEvent, ITreeFilter, ITreeModel, ITreeRenderer, TreeFilterResult, TreeListSpliceData, TreeModelSpliceEvent, TreeVisibility,
};
use zyarr::RegExp;

use crate::browser::workbench_tree::{IWorkbenchTreeOptions, WorkbenchObjectTree, WorkbenchTreeServices};
use crate::contrib::preferences::common::preferences::compare_two_nullable_numbers;
use crate::contrib::preferences::preferences_widgets::SettingsTarget;
use crate::contrib::preferences::settings_editor_setting_indicators::get_indicators_label_aria_label;
use crate::contrib::preferences::settings_layout::{ITOCEntry, ITOCFilter, ITOCFilterPatterns};
use crate::contrib::preferences::settings_tree_models::{
    SettingsTreeElement, SettingsTreeGroupChild, SettingsTreeGroupElement, ViewStateRef, create_setting_match_reg_exp, inspect_setting,
};
use crate::contrib::preferences::settings_tree_renderers::{
    SETTINGS_ARRAY_TEMPLATE_ID, SETTINGS_BOOL_OBJECT_TEMPLATE_ID, SETTINGS_BOOL_TEMPLATE_ID, SETTINGS_COMPLEX_OBJECT_TEMPLATE_ID, SETTINGS_COMPLEX_TEMPLATE_ID,
    SETTINGS_ELEMENT_TEMPLATE_ID, SETTINGS_ENUM_TEMPLATE_ID, SETTINGS_EXCLUDE_TEMPLATE_ID, SETTINGS_EXTENSION_TOGGLE_TEMPLATE_ID, SETTINGS_INCLUDE_TEMPLATE_ID,
    SETTINGS_MULTILINE_TEXT_TEMPLATE_ID, SETTINGS_NEW_EXTENSIONS_TEMPLATE_ID, SETTINGS_NUMBER_TEMPLATE_ID, SETTINGS_OBJECT_TEMPLATE_ID, SETTINGS_TEXT_TEMPLATE_ID,
    SettingsObjectTreeModel, child_identity, fix_setting_links, invalid_type_error,
};
use crate::services::configuration::common::configuration::IWorkbenchConfigurationService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::extensions::extensions::IExtensionService;
use crate::services::preferences::preferences::{ISetting, ISettingsGroup, SettingValueType};

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

/// O [`SettingsTreeFilter`] sobre o elemento da árvore de objetos (`T | null`): a raiz nunca é filtrada.
struct OptionElementFilter(SettingsTreeFilter);

impl ITreeFilter<Option<SettingsTreeGroupChild>> for OptionElementFilter {
    fn filter(&self, element: &Option<SettingsTreeGroupChild>, parent_visibility: TreeVisibility) -> TreeFilterResult<()> {
        match element {
            Some(element) => self.0.filter(element, parent_visibility),
            None => TreeFilterResult::Bool(true),
        }
    }
}

/// `identityProvider: { getId(e) { return e.id; } }`.
struct SettingsTreeIdentityProvider;

impl IIdentityProvider<Option<SettingsTreeGroupChild>> for SettingsTreeIdentityProvider {
    fn get_id(&self, element: &Option<SettingsTreeGroupChild>) -> String {
        element.as_ref().map(SettingsTreeGroupChild::id).unwrap_or_default()
    }
}

/// O identificador de template de um elemento (`SettingsTreeDelegate.getTemplateId`).
fn get_template_id(element: &SettingsTreeGroupChild) -> String {
    let template_id = match element {
        SettingsTreeGroupChild::Group(_) => SETTINGS_ELEMENT_TEMPLATE_ID,
        SettingsTreeGroupChild::NewExtensions(_) => SETTINGS_NEW_EXTENSIONS_TEMPLATE_ID,
        SettingsTreeGroupChild::Setting(element) => {
            if element.value_type() == SettingValueType::ExtensionToggle {
                SETTINGS_EXTENSION_TOGGLE_TEMPLATE_ID
            } else if invalid_type_error(element).is_some() {
                SETTINGS_COMPLEX_TEMPLATE_ID
            } else {
                match element.value_type() {
                    SettingValueType::Boolean => SETTINGS_BOOL_TEMPLATE_ID,
                    SettingValueType::Integer | SettingValueType::Number | SettingValueType::NullableInteger | SettingValueType::NullableNumber => SETTINGS_NUMBER_TEMPLATE_ID,
                    SettingValueType::MultilineString => SETTINGS_MULTILINE_TEXT_TEMPLATE_ID,
                    SettingValueType::String => SETTINGS_TEXT_TEMPLATE_ID,
                    SettingValueType::Enum => SETTINGS_ENUM_TEMPLATE_ID,
                    SettingValueType::Array => SETTINGS_ARRAY_TEMPLATE_ID,
                    SettingValueType::Exclude => SETTINGS_EXCLUDE_TEMPLATE_ID,
                    SettingValueType::Include => SETTINGS_INCLUDE_TEMPLATE_ID,
                    SettingValueType::Object => SETTINGS_OBJECT_TEMPLATE_ID,
                    SettingValueType::BooleanObject => SETTINGS_BOOL_OBJECT_TEMPLATE_ID,
                    SettingValueType::ComplexObject => SETTINGS_COMPLEX_OBJECT_TEMPLATE_ID,
                    // `LanguageTag` e todo outro tipo sem controle próprio.
                    _ => SETTINGS_COMPLEX_TEMPLATE_ID,
                }
            }
        }
    };
    template_id.to_owned()
}

/// `SettingsTreeDelegate.estimateHeight`.
fn estimate_height(element: &SettingsTreeGroupChild) -> f64 {
    match element {
        SettingsTreeGroupChild::Group(_) => 42.0,
        SettingsTreeGroupChild::Setting(element) if element.value_type() == SettingValueType::Boolean => 78.0,
        _ => 104.0,
    }
}

/// `SettingsTreeDelegate`.
pub struct SettingsTreeDelegate {
    cached: CachedListVirtualDelegate<SettingsTreeGroupChild, (u8, usize)>,
}

impl SettingsTreeDelegate {
    pub fn new() -> Self {
        Self { cached: CachedListVirtualDelegate::new(child_identity, estimate_height, get_template_id) }
    }
}

impl Default for SettingsTreeDelegate {
    fn default() -> Self {
        Self::new()
    }
}

impl IListVirtualDelegate<SettingsTreeGroupChild> for SettingsTreeDelegate {
    fn get_height(&self, element: &SettingsTreeGroupChild) -> f64 {
        self.cached.get_height(element)
    }

    fn get_template_id(&self, element: &SettingsTreeGroupChild) -> String {
        self.cached.get_template_id(element)
    }

    fn has_dynamic_height(&self, element: &SettingsTreeGroupChild) -> Option<bool> {
        Some(!matches!(element, SettingsTreeGroupChild::Group(_)))
    }

    fn set_dynamic_height(&self, element: &SettingsTreeGroupChild, height: f64) {
        self.cached.set_dynamic_height(element, height);
    }
}

/// `NonCollapsibleObjectTreeModel`.
pub struct NonCollapsibleObjectTreeModel<T: Clone + Eq + Hash + 'static, F: Clone + 'static = ()> {
    inner: ObjectTreeModel<T, F>,
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> NonCollapsibleObjectTreeModel<T, F> {
    pub fn new(user: &str, options: ObjectTreeModelOptions<T, F>) -> Self {
        Self { inner: ObjectTreeModel::new(user, options) }
    }
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> Deref for NonCollapsibleObjectTreeModel<T, F> {
    type Target = ObjectTreeModel<T, F>;

    fn deref(&self) -> &ObjectTreeModel<T, F> {
        &self.inner
    }
}

/// O `ObjectTreeModel` por baixo, que a [`ObjectTree`](je_widgets::tree::object_tree::ObjectTree) usa para o `setChildren` e o
/// `resort`. (O caminho completo do `Borrow` evita que o `.borrow()` dos `RefCell` do módulo passe a resolver para ele.)
impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> std::borrow::Borrow<ObjectTreeModel<T, F>> for NonCollapsibleObjectTreeModel<T, F> {
    fn borrow(&self) -> &ObjectTreeModel<T, F> {
        &self.inner
    }
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> ITreeModel for NonCollapsibleObjectTreeModel<T, F> {
    type Element = Option<T>;
    type FilterData = F;
    type Ref = Option<T>;
    type Node = <ObjectTreeModel<T, F> as ITreeModel>::Node;

    fn root_ref(&self) -> Option<T> {
        self.inner.root_ref()
    }

    fn ref_is_falsy(&self, location: &Option<T>) -> bool {
        self.inner.ref_is_falsy(location)
    }

    fn on_did_splice_model(&self) -> Event<TreeModelSpliceEvent<Self::Node>> {
        self.inner.on_did_splice_model()
    }

    fn on_did_splice_rendered_nodes(&self) -> Event<TreeListSpliceData<Self::Node>> {
        self.inner.on_did_splice_rendered_nodes()
    }

    fn on_did_change_collapse_state(&self) -> Event<CollapseStateChangeEvent<Self::Node>> {
        self.inner.on_did_change_collapse_state()
    }

    fn on_did_change_render_node_count(&self) -> Event<Rc<Self::Node>> {
        self.inner.on_did_change_render_node_count()
    }

    fn has(&self, location: &Option<T>) -> bool {
        self.inner.has(location)
    }

    fn get_list_index(&self, location: &Option<T>) -> i64 {
        self.inner.get_list_index(location)
    }

    fn get_list_render_count(&self, location: &Option<T>) -> usize {
        self.inner.get_list_render_count(location)
    }

    fn get_node(&self, location: &Option<T>) -> Rc<Self::Node> {
        self.inner.get_node(location)
    }

    fn get_node_location(&self, node: &Rc<Self::Node>) -> Option<T> {
        self.inner.get_node_location(node)
    }

    fn get_parent_node_location(&self, location: &Option<T>) -> Option<Option<T>> {
        self.inner.get_parent_node_location(location)
    }

    fn get_first_element_child(&self, location: &Option<T>) -> Option<Option<T>> {
        self.inner.get_first_element_child(location)
    }

    fn get_last_element_ancestor(&self, location: &Option<T>) -> Option<Option<T>> {
        self.inner.get_last_element_ancestor(location)
    }

    fn is_collapsible(&self, _element: &Option<T>) -> bool {
        false
    }

    fn set_collapsible(&self, location: &Option<T>, collapsible: Option<bool>) -> bool {
        self.inner.set_collapsible(location, collapsible)
    }

    fn is_collapsed(&self, location: &Option<T>) -> bool {
        self.inner.is_collapsed(location)
    }

    fn set_collapsed(&self, _element: &Option<T>, _collapsed: Option<bool>, _recursive: Option<bool>) -> bool {
        false
    }

    fn expand_to(&self, location: &Option<T>) {
        self.inner.expand_to(location);
    }

    fn rerender(&self, location: &Option<T>) {
        self.inner.rerender(location);
    }

    fn refilter(&self) {
        self.inner.refilter();
    }
}

/// `SettingsTreeAccessibilityProvider`: o `accessibilityProvider` das opções do `SettingsTree`, sobre o elemento da árvore de
/// objetos (`T | null`).
pub struct SettingsTreeAccessibilityProvider {
    configuration_service: Rc<dyn IWorkbenchConfigurationService>,
    language_service: Rc<dyn ILanguageService>,
}

impl SettingsTreeAccessibilityProvider {
    pub fn new(configuration_service: Rc<dyn IWorkbenchConfigurationService>, language_service: Rc<dyn ILanguageService>) -> Self {
        Self { configuration_service, language_service }
    }
}

impl IListAccessibilityProvider<Option<SettingsTreeGroupChild>> for SettingsTreeAccessibilityProvider {
    /// `getAriaLabel(element)`: a raiz (`null`) não tem rótulo.
    fn get_aria_label(&self, element: &Option<SettingsTreeGroupChild>) -> Option<String> {
        Some(match element.as_ref()? {
            SettingsTreeGroupChild::Setting(element) => {
                let mut aria_label_sections: Vec<String> = Vec::new();
                aria_label_sections.push(format!("{} {}.", element.display_category(), element.display_label()));

                if element.is_configured() {
                    aria_label_sections.push(localize("settings.Modified", "Modified.", &[]));
                }

                let indicators_label_aria_label = get_indicators_label_aria_label(&**element, &*self.configuration_service, &*self.language_service);
                if !indicators_label_aria_label.is_empty() {
                    aria_label_sections.push(format!("{indicators_label_aria_label}."));
                }

                let description_without_setting_links = render_as_plaintext(&fix_setting_links(element.description(), false));
                if !description_without_setting_links.is_empty() {
                    aria_label_sections.push(description_without_setting_links);
                }
                aria_label_sections.join(" ")
            }
            SettingsTreeGroupChild::Group(element) => element.label(),
            SettingsTreeGroupChild::NewExtensions(element) => element.id(),
        })
    }

    /// `getWidgetAriaLabel()`.
    fn get_widget_aria_label(&self) -> String {
        localize("settings", "Settings", &[])
    }
}

/// Os serviços do construtor do `SettingsTree` (`IConfigurationService`, `ILanguageService`, e o ambiente que o
/// `SettingsTreeFilter` recebe por injeção) e os da árvore do workbench.
#[derive(Clone)]
pub struct SettingsTreeWidgetServices {
    pub tree: WorkbenchTreeServices,
    pub configuration_service: Rc<dyn IWorkbenchConfigurationService>,
    pub language_service: Rc<dyn ILanguageService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
}

/// `workbench.list.smoothScrolling`.
const SMOOTH_SCROLLING_SETTING: &str = "workbench.list.smoothScrolling";

/// `getListStyles({ listBackground: editorBackground, ... })` do construtor do `SettingsTree`: o fundo do editor em todos os
/// estados e o texto em `foreground`.
fn settings_tree_list_styles(theme: &ColorTheme) -> IListStyles {
    let editor_background = theme.get_color("editor.background");
    let foreground = theme.get_color("foreground");
    IListStyles {
        list_background: editor_background,
        list_active_selection_background: editor_background,
        list_active_selection_foreground: foreground,
        list_focus_and_selection_background: editor_background,
        list_focus_and_selection_foreground: foreground,
        list_focus_background: editor_background,
        list_focus_foreground: foreground,
        list_hover_foreground: foreground,
        list_hover_background: editor_background,
        list_hover_outline: editor_background,
        list_focus_outline: editor_background,
        list_inactive_selection_background: editor_background,
        list_inactive_selection_foreground: foreground,
        list_inactive_focus_background: editor_background,
        list_inactive_focus_outline: editor_background,
        tree_indent_guides_stroke: None,
        tree_inactive_indent_guides_stroke: None,
        foreground,
        ..IListStyles::default()
    }
}

/// `SettingsTree`.
pub struct SettingsTree {
    base: Rc<Disposable>,
    tree: Rc<WorkbenchObjectTree<SettingsTreeGroupChild, (), SettingsObjectTreeModel>>,
}

impl SettingsTree {
    pub fn new(
        ui: &Ui,
        container: NodeId,
        view_state: ViewStateRef,
        renderers: Vec<Rc<dyn ITreeRenderer<SettingsObjectTreeModel>>>,
        theme: &ColorTheme,
        services: &SettingsTreeWidgetServices,
    ) -> Rc<Self> {
        let mut tree_options = AbstractTreeOptions::<SettingsObjectTreeModel>::default();
        tree_options.view.support_dynamic_heights = true;
        tree_options.identity_provider = Some(Rc::new(SettingsTreeIdentityProvider));
        tree_options.accessibility_provider = Some(Rc::new(SettingsTreeAccessibilityProvider::new(services.configuration_service.clone(), services.language_service.clone())));
        tree_options.filter = Some(Rc::new(OptionElementFilter(SettingsTreeFilter::new(view_state, true, services.environment_service.clone()))));
        tree_options.update.smooth_scrolling = Some(services.configuration_service.get_value(Some(SMOOTH_SCROLLING_SETTING), None).is_truthy());
        tree_options.update.multiple_selection_support = Some(false);
        tree_options.find_widget_enabled = Some(false);
        tree_options.update.render_indent_guides = Some(RenderIndentGuides::None);

        // O `createModel` sobrescrito: o `NonCollapsibleObjectTreeModel`.
        let tree = WorkbenchObjectTree::<SettingsTreeGroupChild, (), SettingsObjectTreeModel>::with_model(
            "SettingsTree",
            ui,
            container,
            Rc::new(SettingsTreeDelegate::new()),
            renderers,
            tree_options,
            None,
            NonCollapsibleObjectTreeModel::new,
            IWorkbenchTreeOptions { override_styles: Some(settings_tree_list_styles(theme)), ..IWorkbenchTreeOptions::default() },
            &services.tree,
        );

        ui.tree_mut().add_class(tree.get_html_element(), "settings-editor-tree");

        let base = Disposable::new();
        let (weak_tree, weak_configuration) = (Rc::downgrade(&tree), Rc::downgrade(&services.configuration_service));
        base.register(services.configuration_service.on_did_change_configuration().listen(move |e| {
            if e.affects_configuration(SMOOTH_SCROLLING_SETTING, None)
                && let (Some(tree), Some(configuration_service)) = (weak_tree.upgrade(), weak_configuration.upgrade())
            {
                let update = AbstractTreeOptionsUpdate { smooth_scrolling: Some(configuration_service.get_value(Some(SMOOTH_SCROLLING_SETTING), None).is_truthy()), ..AbstractTreeOptionsUpdate::default() };
                tree.update_options(&update, None);
            }
        }));

        Rc::new(Self { base, tree })
    }
}

impl Deref for SettingsTree {
    type Target = WorkbenchObjectTree<SettingsTreeGroupChild, (), SettingsObjectTreeModel>;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

impl IDisposable for SettingsTree {
    fn dispose(&self) {
        self.base.dispose();
        self.tree.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "SettingsTree"
    }
}

