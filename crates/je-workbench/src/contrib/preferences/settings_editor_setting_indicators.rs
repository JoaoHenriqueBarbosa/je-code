//! Porte de `src/vs/workbench/contrib/preferences/browser/settingsEditorSettingIndicators.ts`: o texto acessível dos
//! indicadores de uma configuração (`getIndicatorsLabelAriaLabel`) e o [`SettingsTreeIndicatorsLabel`], que desenha os
//! indicadores ("Também modificado em", "Gerenciado pela organização"...) ao lado de uma configuração.
//!
//! Divergências:
//! - **`SettingsTreeSettingElement`** é lido pelo trait [`SettingIndicatorElement`], que o elemento do modelo implementa:
//!   o original recebe um objeto qualquer com esses campos (o teste dele passa um parcial que muda `policyValueDescription`).
//! - **`getIndicatorsLabelAriaLabel`** não recebe o `userDataProfilesService`: o original o declara e nunca o lê.
//! - **`cachedSyncIgnoredSettingsSet`** (um `let` do módulo que o `SettingsTreeIndicatorsLabel` atualiza) é um estado
//!   por thread, atualizado por [`cache_sync_ignored_settings`]; o `cachedSyncIgnoredSettings !== ignoredSettings` do
//!   original compara a identidade do array, e aqui compara o conteúdo.
//! - **`languageService.getLanguageName(language)`** devolve `string | null`; o `null` entra na frase como o texto
//!   `null`, que é o que o `localize` do original faz com o argumento.
//! - O tipo `string | undefined | string[]` de `getDefaultValueSourceToDisplay` é o enum [`SourceToDisplay`].
//! - **DOM**: `HTMLElement` é [`NodeId`] da árvore do `je-ui`; `$('span.a.b')` é um nó solto com as classes; `style.display`
//!   é `NodeStyle::visible`; `innerText = ''` solta os indicadores (que continuam vivos, como no DOM) e destrói o resto;
//!   `DOM.addDisposableListener` é `Ui::listen`; `role` é o `Role` do nó. O `tabIndex` não existe no `je-ui` (o foco é o
//!   `Ui::focus`), então `tabIndex = 0` e `-1` não têm efeito. O fluxo em linha dos indicadores (`display: inline`)
//!   entra com o layout da árvore de configurações; aqui os nós ficam na ordem do DOM.
//! - Os serviços que o original injeta (`IWorkbenchConfigurationService`, `IUserDataSyncEnablementService`,
//!   `ILanguageService`, `ICommandService`) são `Option`: o teste do original passa `undefined!` e só falha se o caminho
//!   usar o serviço, o que [`required`] reproduz (o `TypeError` do JS é um pânico).
//! - O `IHoverService` é o [`IHoverDelegate2`]; `setupDelayedHover(el, options, ...)` recebe sempre uma função
//!   (o objeto fixo do original é uma função que o devolve).
//! - `focusIndicatorAt`: o `indicators[this.focusedIndex]` do original lança se a lista encolheu desde o último foco; aqui
//!   o indicador anterior ausente é ignorado.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use je_base::event::Emitter;
use je_base::html_content::{IMarkdownString, IsTrusted, create_markdown_link};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::uri::{URI, UriComponents, decode_uri_component, encode_uri_component};
use je_base::JsString;
use je_editor::language_service::ILanguageService;
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::registry::{ConfigurationDefaultSource, ConfigurationDefaultValueSource};
use je_platform::user_data_sync::user_data_sync::IUserDataSyncEnablementService;
use je_ui::{Content, EventKind, NodeId, Role, Tree, Ui};
use je_widgets::dom::{StandardKeyboardEvent, create_detached_el, create_el, clear_node};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::hover_api::{
    HoverContent, HoverPosition, HoverStyle, HoverTarget, IHoverAction, IHoverLifecycleOptions, IHoverOptions, IHoverPositionOptions, IHoverDelegate2,
};
use je_widgets::simple_icon_label::SimpleIconLabel;
use je_widgets::text::{WORKBENCH_LINE_HEIGHT, ui_font};

use crate::contrib::preferences::common::preferences::{
    POLICY_SETTING_TAG, advanced_indicator_description, experimental_indicator_description, preview_indicator_description,
};
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

/// O que os indicadores leem de um `SettingsTreeSettingElement`.
pub trait SettingIndicatorElement {
    fn tags(&self) -> Option<HashSet<String>>;
    fn is_untrusted(&self) -> bool;
    fn has_policy_value(&self) -> bool;
    fn policy_value_description(&self) -> Option<String>;
    fn is_agents_window_read_only(&self) -> bool;
    fn settings_target(&self) -> SettingsTarget;
    /// `element.setting.key`.
    fn setting_key(&self) -> String;
    fn is_configured(&self) -> bool;
    fn overridden_scope_list(&self) -> Vec<String>;
    fn overridden_defaults_language_list(&self) -> Vec<String>;
    fn default_value_source(&self) -> Option<ConfigurationDefaultValueSource>;
}

impl SettingIndicatorElement for SettingsTreeSettingElement {
    fn tags(&self) -> Option<HashSet<String>> {
        SettingsTreeSettingElement::tags(self)
    }

    fn is_untrusted(&self) -> bool {
        SettingsTreeSettingElement::is_untrusted(self)
    }

    fn has_policy_value(&self) -> bool {
        SettingsTreeSettingElement::has_policy_value(self)
    }

    fn policy_value_description(&self) -> Option<String> {
        SettingsTreeSettingElement::policy_value_description(self)
    }

    fn is_agents_window_read_only(&self) -> bool {
        SettingsTreeSettingElement::is_agents_window_read_only(self)
    }

    fn settings_target(&self) -> SettingsTarget {
        self.settings_target
    }

    fn setting_key(&self) -> String {
        self.setting.key.clone()
    }

    fn is_configured(&self) -> bool {
        SettingsTreeSettingElement::is_configured(self)
    }

    fn overridden_scope_list(&self) -> Vec<String> {
        SettingsTreeSettingElement::overridden_scope_list(self)
    }

    fn overridden_defaults_language_list(&self) -> Vec<String> {
        SettingsTreeSettingElement::overridden_defaults_language_list(self)
    }

    fn default_value_source(&self) -> Option<ConfigurationDefaultValueSource> {
        SettingsTreeSettingElement::default_value_source(self)
    }
}

/// `ISettingOverrideClickEvent`; o `scope` é o texto que o original converte com `as ScopeString`
/// (`workspace`, `user`, `remote` ou `default`) e o `language` é `undefined` quando o escopo não tem idioma.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ISettingOverrideClickEvent {
    pub scope: String,
    pub language: Option<String>,
    pub setting_key: String,
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

fn get_default_value_source_to_display(element: &dyn SettingIndicatorElement) -> Option<SourceToDisplay> {
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
    element: &dyn SettingIndicatorElement,
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
    } else if element.settings_target() == SettingsTarget::UserLocal && configuration_service.is_setting_applied_for_all_profiles(&element.setting_key()) {
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
    if CACHED_SYNC_IGNORED_SETTINGS_SET.with(|set| set.borrow().contains(&element.setting_key())) {
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

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `createMarkdownLink(text, href, title)`.
fn markdown_link(text: &str, href: &str, title: Option<&str>) -> String {
    let (text, href, title) = (utf16(text), utf16(href), title.map(utf16));
    create_markdown_link(&text, &href, title.as_deref(), true).to_string()
}

/// `namespace SettingScopeLink`: links internos usados para abrir um escopo no editor de configurações.
mod setting_scope_link {
    use super::*;

    pub fn create(scope: &str) -> String {
        let components = UriComponents {
            scheme: Some(JsString::from(schemas::INTERNAL)),
            path: Some(JsString::from("/")),
            query: Some(JsString::from_units(encode_uri_component(&utf16(scope)))),
            ..UriComponents::default()
        };
        URI::from(&components, false).to_string(false).to_string()
    }

    pub fn parse(link: &str) -> String {
        let uri = URI::parse(link, false);
        JsString::from_units(decode_uri_component(uri.query().units()).expect("URI malformed")).to_string()
    }
}

/// O serviço ausente (`undefined!` no original): usá-lo falha como o `undefined` falharia.
fn required<'a, T: ?Sized>(service: &'a Option<Rc<T>>, name: &str) -> &'a Rc<T> {
    service.as_ref().unwrap_or_else(|| panic!("Cannot read properties of undefined (reading '{name}')"))
}

/// `SettingIndicator`.
struct SettingIndicator {
    element: NodeId,
    /// The element to focus on when navigating with keyboard. When `None`, use [`SettingIndicator::element`] instead.
    focus_element: Cell<Option<NodeId>>,
    label: Rc<SimpleIconLabel>,
    disposables: Rc<DisposableStore>,
}

impl SettingIndicator {
    fn new(ui: &Ui, styles: HighlightedLabelStyles, class_name: &str) -> Rc<Self> {
        let element = create_detached_el(&mut ui.tree_mut(), Role::Generic, class_name);
        let disposables = DisposableStore::new();
        let label = disposables.add_rc(Rc::new(SimpleIconLabel::new(ui, element, styles)));
        Rc::new(Self { element, focus_element: Cell::new(None), label, disposables })
    }

    /// `indicator.focusElement ?? indicator.element`.
    fn element_to_focus(&self) -> NodeId {
        self.focus_element.get().unwrap_or(self.element)
    }
}

/// `focusIndicatorAt`.
fn focus_indicator_at(ui: &Ui, focused_index: &Cell<usize>, indicators: &[Rc<SettingIndicator>], index: usize) {
    if index == focused_index.get() {
        return;
    }
    ui.focus(indicators[index].element_to_focus());
    focused_index.set(index);
}

/// `defaultHoverOptions` com o conteúdo e o alvo.
fn default_hover_options(element: NodeId, content: HoverContent) -> IHoverOptions {
    let mut options = IHoverOptions::new(content, HoverTarget::Node(element));
    options.trap_focus = true;
    options.style = Some(HoverStyle::Pointer);
    options.position = Some(IHoverPositionOptions { hover_position: Some(HoverPosition::Below), ..IHoverPositionOptions::default() });
    options
}

/// Renders the indicators next to a setting, such as "Also Modified In".
pub struct SettingsTreeIndicatorsLabel {
    ui: Ui,
    styles: HighlightedLabelStyles,
    configuration_service: Option<Rc<dyn IWorkbenchConfigurationService>>,
    hover_service: Rc<dyn IHoverDelegate2>,
    user_data_sync_enablement_service: Option<Rc<dyn IUserDataSyncEnablementService>>,
    language_service: Option<Rc<dyn ILanguageService>>,
    command_service: Option<Rc<dyn ICommandService>>,

    indicators_container_element: NodeId,

    preview_indicator: Rc<SettingIndicator>,
    advanced_indicator: Rc<SettingIndicator>,
    workspace_trust_indicator: Rc<SettingIndicator>,
    scope_overrides_indicator: Rc<SettingIndicator>,
    sync_ignored_indicator: Rc<SettingIndicator>,
    default_override_indicator: Rc<SettingIndicator>,

    /// Indicators that each have their own square container at the top-right of the setting.
    isolated_indicators: Vec<Rc<SettingIndicator>>,
    /// Indicators that end up wrapped in a parenthesis at the top-right of the setting.
    parenthesized_indicators: Vec<Rc<SettingIndicator>>,

    keybinding_listeners: Rc<DisposableStore>,
    preview_hover: Rc<MutableDisposable<dyn IDisposable>>,
    focused_index: Rc<Cell<usize>>,
}

impl SettingsTreeIndicatorsLabel {
    pub fn new(
        ui: &Ui,
        styles: HighlightedLabelStyles,
        container: NodeId,
        configuration_service: Option<Rc<dyn IWorkbenchConfigurationService>>,
        hover_service: Rc<dyn IHoverDelegate2>,
        user_data_sync_enablement_service: Option<Rc<dyn IUserDataSyncEnablementService>>,
        language_service: Option<Rc<dyn ILanguageService>>,
        command_service: Option<Rc<dyn ICommandService>>,
    ) -> Self {
        let indicators_container_element = {
            let mut tree = ui.tree_mut();
            let element = create_el(&mut tree, container, Role::Generic, "setting-indicators-container");
            tree.style_mut(element).visible = true;
            element
        };

        let mut this = Self {
            ui: ui.clone(),
            styles,
            configuration_service,
            hover_service,
            user_data_sync_enablement_service,
            language_service,
            command_service,
            indicators_container_element,
            preview_indicator: SettingIndicator::new(ui, styles, "setting-indicator setting-item-preview"),
            advanced_indicator: SettingIndicator::new(ui, styles, "setting-indicator setting-item-preview"),
            workspace_trust_indicator: SettingIndicator::new(ui, styles, "setting-indicator setting-item-workspace-trust"),
            // Don't add .setting-indicator class here, because it gets conditionally added later.
            scope_overrides_indicator: SettingIndicator::new(ui, styles, "setting-item-overrides"),
            sync_ignored_indicator: SettingIndicator::new(ui, styles, "setting-indicator setting-item-ignored"),
            default_override_indicator: SettingIndicator::new(ui, styles, "setting-indicator setting-item-default-overridden"),
            isolated_indicators: Vec::new(),
            parenthesized_indicators: Vec::new(),
            keybinding_listeners: DisposableStore::new(),
            preview_hover: MutableDisposable::new(),
            focused_index: Rc::new(Cell::new(0)),
        };
        this.init_workspace_trust_indicator();
        this.init_sync_ignored_indicator();
        this.init_default_override_indicator();
        this.init_advanced_indicator();
        this.isolated_indicators = vec![this.preview_indicator.clone(), this.advanced_indicator.clone()];
        this.parenthesized_indicators = vec![
            this.workspace_trust_indicator.clone(),
            this.scope_overrides_indicator.clone(),
            this.sync_ignored_indicator.clone(),
            this.default_override_indicator.clone(),
        ];
        this
    }

    /// `hoverService.setupDelayedHover(element, options, { setupKeyboardEvents: true })`.
    fn setup_delayed_hover(&self, element: NodeId, options: impl Fn() -> IHoverOptions + 'static) -> DisposableRef {
        let lifecycle = IHoverLifecycleOptions { group_id: None, reduced_delay: None, setup_keyboard_events: Some(true) };
        self.hover_service.setup_delayed_hover(element, Rc::new(options), Some(lifecycle))
    }

    /// O hover de um texto fixo, com as `defaultHoverOptions`.
    fn setup_text_hover(&self, element: NodeId, content: String) -> DisposableRef {
        self.setup_delayed_hover(element, move || default_hover_options(element, HoverContent::Text(content.clone())))
    }

    fn init_workspace_trust_indicator(&self) {
        let indicator = &self.workspace_trust_indicator;
        indicator.label.set_text(&format!("$(shield) {}", localize("workspaceUntrustedLabel", "Requires workspace trust", &[])));

        let content = localize("trustLabel", "The setting value can only be applied in a trusted workspace.", &[]);
        let command_service = self.command_service.clone();
        let element = indicator.element;
        indicator.disposables.add(self.setup_delayed_hover(element, move || {
            let mut options = default_hover_options(element, HoverContent::Text(content.clone()));
            let command_service = command_service.clone();
            options.actions = vec![IHoverAction {
                label: localize("manageWorkspaceTrust", "Manage Workspace Trust", &[]),
                command_id: "workbench.trust.manage".to_owned(),
                icon_class: None,
                run: Rc::new(move |_target| {
                    let _ = required(&command_service, "executeCommand").execute_command("workbench.trust.manage", Vec::new());
                }),
            }];
            options
        }));
    }

    fn init_sync_ignored_indicator(&self) {
        let indicator = &self.sync_ignored_indicator;
        indicator.label.set_text(&localize("extensionSyncIgnoredLabel", "Not synced", &[]));
        let content = localize("syncIgnoredTitle", "This setting is ignored during sync", &[]);
        indicator.disposables.add(self.setup_text_hover(indicator.element, content));
    }

    fn init_default_override_indicator(&self) {
        self.default_override_indicator.label.set_text(&localize("defaultOverriddenLabel", "Default value changed", &[]));
    }

    fn init_advanced_indicator(&self) {
        let indicator = &self.advanced_indicator;
        indicator.label.set_text(&localize("advancedLabel", "Advanced", &[]));
        indicator.disposables.add(self.setup_text_hover(indicator.element, advanced_indicator_description()));
    }

    fn is_shown(&self, indicator: &SettingIndicator) -> bool {
        self.ui.tree().style(indicator.element).visible
    }

    fn set_shown(&self, indicator: &SettingIndicator, shown: bool) {
        self.ui.tree_mut().style_mut(indicator.element).visible = shown;
    }

    /// Um nó de texto (`$('span', undefined, text)`) como último filho do contêiner de indicadores.
    fn append_text(&self, tree: &mut Tree, class_name: &str, text: &str) {
        let node = create_el(tree, self.indicators_container_element, Role::Generic, class_name);
        tree.style_mut(node).content = Content::Text {
            text: text.to_owned(),
            style: ui_font(self.styles.font_size, self.styles.font_weight),
            color: self.styles.foreground,
            line_height: self.styles.line_height.unwrap_or(WORKBENCH_LINE_HEIGHT),
        };
    }

    fn render(&self) {
        let container = self.indicators_container_element;
        let all_indicators = || self.isolated_indicators.iter().chain(&self.parenthesized_indicators);
        {
            // `innerText = ''`: os indicadores soltam-se e continuam vivos, o resto (parênteses e vírgulas) é descartado.
            let mut tree = self.ui.tree_mut();
            for child in tree.children(container).to_vec() {
                if all_indicators().any(|indicator| indicator.element == child) {
                    tree.detach(child);
                } else {
                    tree.remove(child);
                }
            }
            tree.style_mut(container).visible = false;
        }

        let isolated_indicators_to_show: Vec<Rc<SettingIndicator>> = self.isolated_indicators.iter().filter(|indicator| self.is_shown(indicator)).cloned().collect();
        if !isolated_indicators_to_show.is_empty() {
            let mut tree = self.ui.tree_mut();
            tree.style_mut(container).visible = true;
            for indicator in &isolated_indicators_to_show {
                tree.append_child(container, indicator.element);
            }
        }

        let parenthesized_indicators_to_show: Vec<Rc<SettingIndicator>> = self.parenthesized_indicators.iter().filter(|indicator| self.is_shown(indicator)).cloned().collect();
        if let Some((last, rest)) = parenthesized_indicators_to_show.split_last() {
            let mut tree = self.ui.tree_mut();
            tree.style_mut(container).visible = true;
            self.append_text(&mut tree, "", "(");
            for indicator in rest {
                tree.append_child(container, indicator.element);
                self.append_text(&mut tree, "comma", " \u{2022} ");
            }
            tree.append_child(container, last.element);
            self.append_text(&mut tree, "", ")");
        }
        self.reset_indicator_navigation_key_bindings(isolated_indicators_to_show.into_iter().chain(parenthesized_indicators_to_show).collect());
    }

    fn reset_indicator_navigation_key_bindings(&self, indicators: Vec<Rc<SettingIndicator>>) {
        self.keybinding_listeners.clear();
        self.ui.tree_mut().set_role(self.indicators_container_element, if indicators.is_empty() { Role::Button } else { Role::Toolbar });
        if indicators.is_empty() {
            return;
        }
        let (ui, focused_index) = (self.ui.clone(), self.focused_index.clone());
        self.keybinding_listeners.add(self.ui.listen(self.indicators_container_element, EventKind::KeyDown, false, move |e| {
            let Some(ev) = StandardKeyboardEvent::from_event(e) else { return };
            let focused = focused_index.get();
            let mut handled = true;
            if ev.equals(KeyCode::Home.bits()) {
                focus_indicator_at(&ui, &focused_index, &indicators, 0);
            } else if ev.equals(KeyCode::End.bits()) {
                focus_indicator_at(&ui, &focused_index, &indicators, indicators.len() - 1);
            } else if ev.equals(KeyCode::RightArrow.bits()) {
                focus_indicator_at(&ui, &focused_index, &indicators, (focused + 1) % indicators.len());
            } else if ev.equals(KeyCode::LeftArrow.bits()) {
                focus_indicator_at(&ui, &focused_index, &indicators, if focused != 0 { focused - 1 } else { indicators.len() - 1 });
            } else {
                handled = false;
            }

            if handled {
                e.prevent_default();
                e.stop_propagation();
            }
        }));
    }

    pub fn update_workspace_trust(&self, element: &dyn SettingIndicatorElement) {
        self.set_shown(&self.workspace_trust_indicator, element.is_untrusted());
        self.render();
    }

    pub fn update_sync_ignored(&self, element: &dyn SettingIndicatorElement, ignored_settings: &[String]) {
        let shown = required(&self.user_data_sync_enablement_service, "isEnabled").is_enabled() && ignored_settings.contains(&element.setting_key());
        self.set_shown(&self.sync_ignored_indicator, shown);
        self.render();
        cache_sync_ignored_settings(ignored_settings);
    }

    pub fn update_preview_indicator(&self, element: &dyn SettingIndicatorElement) {
        let tags = element.tags();
        let has_tag = |tag: &str| tags.as_ref().is_some_and(|tags| tags.contains(tag));
        let is_preview_setting = has_tag("preview");
        let is_experimental_setting = has_tag("experimental");
        self.set_shown(&self.preview_indicator, is_preview_setting || is_experimental_setting);
        self.preview_indicator.label.set_text(&if is_preview_setting { localize("previewLabel", "Preview", &[]) } else { localize("experimentalLabel", "Experimental", &[]) });

        let content = if is_preview_setting { preview_indicator_description() } else { experimental_indicator_description() };
        self.preview_hover.set(Some(self.setup_text_hover(self.preview_indicator.element, content)));

        self.render();
    }

    pub fn update_advanced_indicator(&self, element: &dyn SettingIndicatorElement) {
        self.set_shown(&self.advanced_indicator, element.tags().is_some_and(|tags| tags.contains("advanced")));
        self.render();
    }

    fn get_inline_scope_display_text(&self, complete_scope: &str) -> String {
        let (scope, language) = split_scope(complete_scope);
        let localized_scope = localized_scope(scope);
        if let Some(language) = language {
            return format!("{} > {}", language_name(required(&self.language_service, "getLanguageName").as_ref(), language), localized_scope);
        }
        localized_scope
    }

    pub fn update_scope_overrides(
        &self,
        element: &dyn SettingIndicatorElement,
        on_did_click_override_element: &Rc<Emitter<ISettingOverrideClickEvent>>,
        on_apply_filter: &Rc<Emitter<String>>,
    ) {
        let indicator = &self.scope_overrides_indicator;
        indicator.disposables.clear();
        {
            let mut tree = self.ui.tree_mut();
            clear_node(&mut tree, indicator.element);
            tree.style_mut(indicator.element).visible = false;
        }
        indicator.focus_element.set(Some(indicator.element));
        if element.has_policy_value() {
            // If the setting falls under a policy, then no matter what the user sets, the policy value takes effect.
            self.show_scope_overrides_as_indicator();

            indicator.label.set_text(&format!("$(briefcase) {}", localize("policyLabelText", "Managed by organization", &[])));
            let content = element
                .policy_value_description()
                .unwrap_or_else(|| localize("policyDescription", "This setting is managed by your organization and its actual value cannot be changed.", &[]));
            let (on_apply_filter, target) = (on_apply_filter.clone(), indicator.element);
            indicator.disposables.add(self.setup_delayed_hover(target, move || {
                let mut options = default_hover_options(target, HoverContent::Text(content.clone()));
                let on_apply_filter = on_apply_filter.clone();
                options.actions = vec![IHoverAction {
                    label: localize("policyFilterLink", "View policy settings", &[]),
                    command_id: "_settings.action.viewPolicySettings".to_owned(),
                    icon_class: None,
                    run: Rc::new(move |_| on_apply_filter.fire(format!("@{POLICY_SETTING_TAG}"))),
                }];
                options
            }));
        } else if element.is_agents_window_read_only() {
            self.show_scope_overrides_as_indicator();

            indicator.label.set_text(&format!("$(lock) {}", localize("agentsWindowReadOnlyLabelText", "Cannot be changed in Agents window", &[])));
            let content = localize("agentsWindowReadOnlyDescription", "This setting cannot be changed in the Agents window.", &[]);
            indicator.disposables.add(self.setup_text_hover(indicator.element, content));
        } else if element.settings_target() == SettingsTarget::UserLocal
            && required(&self.configuration_service, "isSettingAppliedForAllProfiles").is_setting_applied_for_all_profiles(&element.setting_key())
        {
            self.show_scope_overrides_as_indicator();

            indicator.label.set_text(&localize("applicationSetting", "Applies to all profiles", &[]));

            let content = localize("applicationSettingDescription", "The setting is not specific to the current profile, and will retain its value when switching profiles.", &[]);
            indicator.disposables.add(self.setup_text_hover(indicator.element, content));
        } else {
            let overridden_scope_list = element.overridden_scope_list();
            let overridden_defaults_language_list = element.overridden_defaults_language_list();
            if !overridden_scope_list.is_empty() || !overridden_defaults_language_list.is_empty() {
                if overridden_scope_list.len() == 1 && overridden_defaults_language_list.is_empty() {
                    self.show_inline_scope_override(element, &overridden_scope_list[0], on_did_click_override_element);
                } else {
                    self.show_scope_overrides_hover(element, &overridden_scope_list, &overridden_defaults_language_list, on_did_click_override_element);
                }
            }
        }
        self.render();
    }

    /// `element.style.display = 'inline'` e `classList.add('setting-indicator')` do indicador de escopo.
    fn show_scope_overrides_as_indicator(&self) {
        let indicator = &self.scope_overrides_indicator;
        let mut tree = self.ui.tree_mut();
        tree.style_mut(indicator.element).visible = true;
        tree.add_class(indicator.element, "setting-indicator");
    }

    /// We can inline the override and show all the text in the label so that users don't have to wait for the hover to
    /// load just to click into the one override there is.
    fn show_inline_scope_override(
        &self,
        element: &dyn SettingIndicatorElement,
        overridden_scope: &str,
        on_did_click_override_element: &Rc<Emitter<ISettingOverrideClickEvent>>,
    ) {
        let indicator = &self.scope_overrides_indicator;
        {
            let mut tree = self.ui.tree_mut();
            tree.style_mut(indicator.element).visible = true;
            tree.remove_class(indicator.element, "setting-indicator");
        }

        let preface_text = if element.is_configured() { localize("alsoConfiguredIn", "Also modified in", &[]) } else { localize("configuredIn", "Modified in", &[]) };
        indicator.label.set_text(&format!("{preface_text} "));

        let view = {
            let mut tree = self.ui.tree_mut();
            let view = create_el(&mut tree, indicator.element, Role::Link, "modified-scope");
            tree.style_mut(view).content = Content::Text {
                text: self.get_inline_scope_display_text(overridden_scope),
                style: ui_font(self.styles.font_size, self.styles.font_weight),
                color: self.styles.foreground,
                line_height: self.styles.line_height.unwrap_or(WORKBENCH_LINE_HEIGHT),
            };
            view
        };
        indicator.focus_element.set(Some(view));
        let (setting_key, overridden_scope) = (element.setting_key(), overridden_scope.to_owned());
        let on_click_or_keydown = {
            let on_did_click_override_element = on_did_click_override_element.clone();
            move |e: &mut je_ui::Event| {
                let (scope, language) = split_scope(&overridden_scope);
                on_did_click_override_element.fire(ISettingOverrideClickEvent {
                    setting_key: setting_key.clone(),
                    scope: scope.to_owned(),
                    language: language.map(str::to_owned),
                });
                e.prevent_default();
                e.stop_propagation();
            }
        };
        let on_click_or_keydown = Rc::new(on_click_or_keydown);
        let on_click = on_click_or_keydown.clone();
        indicator.disposables.add(self.ui.listen(view, EventKind::Click, false, move |e| (*on_click)(e)));
        indicator.disposables.add(self.ui.listen(view, EventKind::KeyDown, false, move |e| {
            let Some(ev) = StandardKeyboardEvent::from_event(e) else { return };
            if ev.equals(KeyCode::Space.bits()) || ev.equals(KeyCode::Enter.bits()) {
                (*on_click_or_keydown)(e);
            }
        }));
    }

    /// "Also modified elsewhere": o hover lista os escopos e os idiomas com padrão sobrescrito.
    fn show_scope_overrides_hover(
        &self,
        element: &dyn SettingIndicatorElement,
        overridden_scope_list: &[String],
        overridden_defaults_language_list: &[String],
        on_did_click_override_element: &Rc<Emitter<ISettingOverrideClickEvent>>,
    ) {
        let indicator = &self.scope_overrides_indicator;
        self.show_scope_overrides_as_indicator();
        let scope_overrides_label_text =
            if element.is_configured() { localize("alsoConfiguredElsewhere", "Also modified elsewhere", &[]) } else { localize("configuredElsewhere", "Modified elsewhere", &[]) };
        indicator.label.set_text(&scope_overrides_label_text);

        let language_service = required(&self.language_service, "getLanguageName");
        let mut content_markdown_string = String::new();
        if !overridden_scope_list.is_empty() {
            let preface_text = if element.is_configured() {
                localize("alsoModifiedInScopes", "The setting has also been modified in the following scopes:", &[])
            } else {
                localize("modifiedInScopes", "The setting has been modified in the following scopes:", &[])
            };
            content_markdown_string = preface_text;
            for scope in overridden_scope_list {
                let scope_display_text = self.get_inline_scope_display_text(scope);
                content_markdown_string.push_str("\n- ");
                content_markdown_string
                    .push_str(&markdown_link(&scope_display_text, &setting_scope_link::create(scope), Some(&get_accessible_scope_display_text(scope, language_service.as_ref()))));
            }
        }
        if !overridden_defaults_language_list.is_empty() {
            if !content_markdown_string.is_empty() {
                content_markdown_string.push_str("\n\n");
            }
            content_markdown_string.push_str(&localize("hasDefaultOverridesForLanguages", "The following languages have default overrides:", &[]));
            for language in overridden_defaults_language_list {
                let scope_display_text = language_service.get_language_name(language);
                content_markdown_string.push_str("\n- ");
                content_markdown_string
                    .push_str(&markdown_link(scope_display_text.as_deref().unwrap_or(language), &setting_scope_link::create(&format!("default:{language}")), None));
            }
        }
        let content = IMarkdownString {
            value: JsString::from(content_markdown_string),
            is_trusted: Some(IsTrusted::Bool(false)),
            support_html: Some(false),
            ..IMarkdownString::default()
        };
        let (setting_key, on_did_click_override_element, target) = (element.setting_key(), on_did_click_override_element.clone(), indicator.element);
        indicator.disposables.add(self.setup_delayed_hover(target, move || {
            let mut options = default_hover_options(target, HoverContent::Markdown(content.clone()));
            let (setting_key, on_did_click_override_element) = (setting_key.clone(), on_did_click_override_element.clone());
            options.link_handler = Some(Rc::new(move |url: &str| {
                let link = setting_scope_link::parse(url);
                let (scope, language) = split_scope(&link);
                on_did_click_override_element.fire(ISettingOverrideClickEvent {
                    setting_key: setting_key.clone(),
                    scope: scope.to_owned(),
                    language: language.map(str::to_owned),
                });
            }));
            options
        }));
    }

    pub fn update_default_override_indicator(&self, element: &dyn SettingIndicatorElement) {
        let indicator = &self.default_override_indicator;
        self.set_shown(indicator, false);
        if let Some(source_to_display) = get_default_value_source_to_display(element) {
            self.set_shown(indicator, true);
            indicator.disposables.clear();

            // Show source of default value when hovered
            let source_to_display = match source_to_display {
                SourceToDisplay::Many(sources) if sources.len() == 1 => SourceToDisplay::Single(sources[0].clone()),
                other => other,
            };

            let default_override_hover_content = match source_to_display {
                SourceToDisplay::Single(source) => localize("defaultOverriddenDetails", "Default setting value overridden by `{0}`", &[&source]),
                SourceToDisplay::Many(sources) => {
                    let sources: Vec<String> = sources.iter().map(|source| format!("`{source}`")).collect();
                    let (last, rest) = match sources.split_last() {
                        Some((last, rest)) => (last.clone(), rest.join(", ")),
                        None => (String::new(), String::new()),
                    };
                    localize("multipledefaultOverriddenDetails", "A default values has been set by {0}", &[&format!("{rest} & {last}")])
                }
            };

            let content = IMarkdownString {
                value: JsString::from(default_override_hover_content),
                is_trusted: Some(IsTrusted::Bool(false)),
                support_theme_icons: Some(false),
                support_html: Some(false),
                support_alert_syntax: Some(false),
                ..IMarkdownString::default()
            };
            let target = indicator.element;
            indicator.disposables.add(self.setup_delayed_hover(target, move || {
                let mut options = IHoverOptions::new(HoverContent::Markdown(content.clone()), HoverTarget::Node(target));
                options.style = Some(HoverStyle::Pointer);
                options.position = Some(IHoverPositionOptions { hover_position: Some(HoverPosition::Below), ..IHoverPositionOptions::default() });
                options
            }));
        }
        self.render();
    }
}

impl IDisposable for SettingsTreeIndicatorsLabel {
    fn dispose(&self) {
        self.keybinding_listeners.dispose();
        self.preview_hover.dispose();
        for indicator in self.isolated_indicators.iter().chain(&self.parenthesized_indicators) {
            indicator.disposables.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "SettingsTreeIndicatorsLabel"
    }
}
