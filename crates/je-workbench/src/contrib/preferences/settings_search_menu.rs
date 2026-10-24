//! Porte de `src/vs/workbench/contrib/preferences/browser/settingsSearchMenu.ts`: o menu de filtros ao lado da caixa de
//! busca do editor de configurações. Cada entrada acrescenta (ou tira) um filtro (`@modified`, `@tag:preview`...) da
//! consulta que está na caixa.
//!
//! Divergências:
//! - **`extends DropdownMenuActionViewItem`** vira composição: o [`SettingsSearchFilterDropdownMenuActionViewItem`] guarda
//!   o [`DropdownMenuActionViewItem`] ([`SettingsSearchFilterDropdownMenuActionViewItem::dropdown`]) e o passa onde o
//!   original passaria `this`. O `render` do original só chama o `super.render`, então não existe aqui.
//! - **`{ getActions: () => this.getActions() }`** é um [`IActionProvider`] que guarda uma referência fraca ao item
//!   (`Rc::new_cyclic`), para o menu pedir as ações a cada abertura.
//! - **`SuggestEnabledInput`** ainda não foi portado: o item só usa `getValue`, `setValue`, `focus` e o
//!   `SuggestController.get(inputWidget)`, que são o trait [`ISuggestEnabledInput`].
//! - **`@IContextMenuService`** é o [`IContextMenuProvider`] e o `Ui` é o da árvore de widgets nativa, que o item
//!   precisa para desenhar o rótulo.
//! - `actionRunner` e `options` chegam separados porque o `IActionViewItemOptions` do original os mescla num objeto só.

use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IActionRunner, Separator, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::layout::AnchorAlignment;
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_editor::suggest::suggest_controller::SuggestController;
use je_ui::Ui;
use je_widgets::action_view_items::IActionViewItemOptions;
use je_widgets::context_view_api::IContextMenuProvider;
use je_widgets::dropdown::IActionProvider;
use je_widgets::dropdown_action_view_item::{DropdownMenuActionViewItem, IDropdownMenuActionViewItemOptions, MenuActions};

use crate::contrib::code_editor::suggest_enabled_input::ISuggestEnabledInput;
use crate::contrib::preferences::common::preferences::{
    ADVANCED_SETTING_TAG, EXP_ASSIGNMENT_SETTING_TAG, EXTENSION_SETTING_TAG, FEATURE_SETTING_TAG, GENERAL_TAG_SETTING_TAG, ID_SETTING_TAG, LANGUAGE_SETTING_TAG,
    MODIFIED_SETTING_TAG, POLICY_SETTING_TAG,
};

/// Uma ação do menu, que roda `run` e devolve `undefined` (as `run` do original não devolvem nada).
fn menu_action(id: &str, label: String, tooltip: String, checked: Option<bool>, run: impl Fn() + 'static) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label,
        tooltip: Some(tooltip),
        enabled: Some(true),
        checked,
        class: None,
        alt: None,
        run: Rc::new(move |_| {
            run();
            MaybePromise::Value(Dynamic::Undefined)
        }),
    })
}

/// `query.split(' ').filter(word => keep(word)).join(' ')`.
fn query_words_without(query: &str, keep: impl Fn(&str) -> bool) -> String {
    query.split(' ').filter(|word| keep(word)).collect::<Vec<_>>().join(" ")
}

/// `SettingsSearchFilterDropdownMenuActionViewItem`.
pub struct SettingsSearchFilterDropdownMenuActionViewItem {
    dropdown: Rc<DropdownMenuActionViewItem>,
    search_widget: Rc<dyn ISuggestEnabledInput>,
    suggest_controller: Option<Rc<SuggestController>>,
}

impl SettingsSearchFilterDropdownMenuActionViewItem {
    pub fn new(
        ui: &Ui,
        action: ActionRef,
        options: IActionViewItemOptions,
        action_runner: Option<Rc<dyn IActionRunner>>,
        search_widget: Rc<dyn ISuggestEnabledInput>,
        context_menu_provider: Rc<dyn IContextMenuProvider>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let provider_weak = weak.clone();
            let provider: Rc<dyn IActionProvider> = Rc::new(move || provider_weak.upgrade().map(|this| this.get_actions()).unwrap_or_default());
            let class_names = action.class().map(|class| class.split_whitespace().map(str::to_owned).collect()).unwrap_or_default();
            let dropdown = DropdownMenuActionViewItem::new(
                ui,
                action,
                MenuActions::Provider(provider),
                context_menu_provider,
                IDropdownMenuActionViewItemOptions {
                    base: options,
                    action_runner,
                    class_names,
                    anchor_alignment_provider: Some(Rc::new(|| AnchorAlignment::Right)),
                    menu_as_child: true,
                    ..IDropdownMenuActionViewItemOptions::default()
                },
            );
            let suggest_controller = search_widget.suggest_controller();
            Self { dropdown, search_widget, suggest_controller }
        })
    }

    /// O item de menu que o original estende.
    pub fn dropdown(&self) -> &Rc<DropdownMenuActionViewItem> {
        &self.dropdown
    }

    /// The created action appends a query to the search widget search string. It optionally triggers suggestions.
    fn create_action(&self, id: &str, label: String, tooltip: String, query_to_append: String, trigger_suggest: bool) -> ActionRef {
        let search_widget = self.search_widget.clone();
        let suggest_controller = self.suggest_controller.clone();
        menu_action(id, label, tooltip, None, move || {
            search_widget.set_value(&format!("{} {}", search_widget.get_value().trim_end(), query_to_append));
            search_widget.focus();
            if let Some(suggest_controller) = suggest_controller.as_ref().filter(|_| trigger_suggest) {
                suggest_controller.trigger_suggest(None, None, None);
            }
        })
    }

    /// The created action appends a query to the search widget search string, if the query does not exist.
    /// Otherwise, it removes the query from the search widget search string.
    /// The action does not trigger suggestions after adding or removing the query.
    fn create_toggle_action(&self, id: &str, label: String, tooltip: String, query_to_append: String) -> ActionRef {
        let search_widget = self.search_widget.clone();
        let query_contains_query_to_append = search_widget.get_value().split(' ').any(|word| word == query_to_append);
        menu_action(id, label, tooltip, Some(query_contains_query_to_append), move || {
            if !query_contains_query_to_append {
                let value = search_widget.get_value();
                let trimmed_current_query = value.trim_end();
                let new_query = if trimmed_current_query.is_empty() { query_to_append.clone() } else { format!("{trimmed_current_query} {query_to_append}") };
                search_widget.set_value(&new_query);
            } else {
                search_widget.set_value(&query_words_without(&search_widget.get_value(), |word| word != query_to_append));
            }
            search_widget.focus();
        })
    }

    fn create_mutually_exclusive_toggle_action(&self, id: &str, label: String, tooltip: String, filter: &'static str, exclude_filters: &'static [&'static str]) -> ActionRef {
        let search_widget = self.search_widget.clone();
        let is_filter_enabled = search_widget.get_value().split(' ').any(|word| word == filter);
        menu_action(id, label, tooltip, Some(is_filter_enabled), move || {
            if is_filter_enabled {
                search_widget.set_value(&query_words_without(&search_widget.get_value(), |word| word != filter));
            } else {
                let new_query = query_words_without(&search_widget.get_value(), |word| !exclude_filters.contains(&word) && word != filter);
                let new_query = new_query.trim_end();
                let new_query = if new_query.is_empty() { filter.to_owned() } else { format!("{new_query} {filter}") };
                search_widget.set_value(&new_query);
            }
            search_widget.focus();
        })
    }

    pub fn get_actions(&self) -> Vec<ActionRef> {
        vec![
            self.create_toggle_action(
                "modifiedSettingsSearch",
                localize("modifiedSettingsSearch", "Modified", &[]),
                localize("modifiedSettingsSearchTooltip", "Add or remove modified settings filter", &[]),
                format!("@{MODIFIED_SETTING_TAG}"),
            ),
            Separator::new(),
            self.create_action(
                "extSettingsSearch",
                localize("extSettingsSearch", "Extension ID...", &[]),
                localize("extSettingsSearchTooltip", "Add extension ID filter", &[]),
                format!("@{EXTENSION_SETTING_TAG}"),
                true,
            ),
            self.create_action(
                "featuresSettingsSearch",
                localize("featureSettingsSearch", "Feature...", &[]),
                localize("featureSettingsSearchTooltip", "Add feature filter", &[]),
                format!("@{FEATURE_SETTING_TAG}"),
                true,
            ),
            self.create_action(
                "tagSettingsSearch",
                localize("tagSettingsSearch", "Tag...", &[]),
                localize("tagSettingsSearchTooltip", "Add tag filter", &[]),
                format!("@{GENERAL_TAG_SETTING_TAG}"),
                true,
            ),
            self.create_action(
                "langSettingsSearch",
                localize("langSettingsSearch", "Language...", &[]),
                localize("langSettingsSearchTooltip", "Add language ID filter", &[]),
                format!("@{LANGUAGE_SETTING_TAG}"),
                true,
            ),
            self.create_action(
                "idSettingsSearch",
                localize("idSettingsSearch", "Setting ID...", &[]),
                localize("idSettingsSearchTooltip", "Add Setting ID filter", &[]),
                format!("@{ID_SETTING_TAG}"),
                false,
            ),
            Separator::new(),
            self.create_toggle_action(
                "onlineSettingsSearch",
                localize("onlineSettingsSearch", "Online services", &[]),
                localize("onlineSettingsSearchTooltip", "Show settings for online services", &[]),
                "@tag:usesOnlineServices".to_owned(),
            ),
            self.create_toggle_action(
                "policySettingsSearch",
                localize("policySettingsSearch", "Organization policies", &[]),
                localize("policySettingsSearchTooltip", "Show organization policy settings", &[]),
                format!("@{POLICY_SETTING_TAG}"),
            ),
            Separator::new(),
            self.create_mutually_exclusive_toggle_action(
                "stableSettingsSearch",
                localize("stableSettings", "Stable", &[]),
                localize("stableSettingsSearchTooltip", "Show stable settings", &[]),
                "@stable",
                &["@tag:preview", "@tag:experimental"],
            ),
            self.create_mutually_exclusive_toggle_action(
                "previewSettingsSearch",
                localize("previewSettings", "Preview", &[]),
                localize("previewSettingsSearchTooltip", "Show preview settings", &[]),
                "@tag:preview",
                &["@stable", "@tag:experimental"],
            ),
            self.create_mutually_exclusive_toggle_action(
                "experimentalSettingsSearch",
                localize("experimental", "Experimental", &[]),
                localize("experimentalSettingsSearchTooltip", "Show experimental settings", &[]),
                "@tag:experimental",
                &["@stable", "@tag:preview"],
            ),
            Separator::new(),
            self.create_toggle_action(
                "advancedSettingsSearch",
                localize("advancedSettingsSearch", "Advanced", &[]),
                localize("advancedSettingsSearchTooltip", "Show advanced settings", &[]),
                format!("@tag:{ADVANCED_SETTING_TAG}"),
            ),
            self.create_toggle_action(
                "expAssignmentSettingsSearch",
                localize("expAssignmentSettingsSearch", "ExP Assigned", &[]),
                localize("expAssignmentSettingsSearchTooltip", "Show settings with an ExP treatment assignment", &[]),
                format!("@tag:{EXP_ASSIGNMENT_SETTING_TAG}"),
            ),
        ]
    }
}

impl IDisposable for SettingsSearchFilterDropdownMenuActionViewItem {
    fn dispose(&self) {
        self.dropdown.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "SettingsSearchFilterDropdownMenuActionViewItem"
    }
}
