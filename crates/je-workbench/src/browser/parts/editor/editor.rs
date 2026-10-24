//! Porte parcial de `src/vs/workbench/browser/parts/editor/editor.ts`: o que o `EditorPart` lê do arquivo
//! (`DEFAULT_EDITOR_MIN_DIMENSIONS`, `DEFAULT_EDITOR_MAX_DIMENSIONS`, `DEFAULT_EDITOR_PART_OPTIONS`,
//! `impactsEditorPartOptions`, `getEditorPartOptions`, `IEditorPartCreationOptions`).
//!
//! Divergências:
//! - **`themeService.getFileIconTheme().hasFileIcons`** (o `hasIcons`) chega como o parâmetro `has_file_icons` de
//!   [`get_editor_part_options`], porque o `IThemeService` ainda não está portado.
//! - **`Object.assign` e `verifyObject`.** As duas etapas viram uma só, [`assign_part_options`]: parte-se dos padrões e cada
//!   propriedade do objeto de origem só vale se tiver o tipo (ou o texto) que o `Verifier` do original aceita, senão o
//!   padrão fica. O resultado é o mesmo, porque o `verifyObject` devolve o padrão para qualquer valor de tipo errado. O
//!   `DeepPartial<IEditorPartOptions>` de `enforcePartOptions` usa a mesma função, campo a campo (o `Object.assign`
//!   do original trocaria o `limit` inteiro por um objeto parcial).
//! - **`showBreadcrumbs`** não passa pelo `verifyObject` no original (não está na lista de verificadores), então o objeto
//!   resultante nunca o tem: fica `None`.

use std::collections::BTreeSet;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_editor::core_2d::dimension::Dimension;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{IContextKey, RawContextKey};
use je_platform::opener::editor_types::EditorOptions;

use crate::browser::parts::editor::editor_group_view::EditorGroupView;
use crate::common::editor::{EditorInputOrUntyped, EditorInputRef, GroupIdentifier, PreventPinnedEditorClose};
use crate::services::editor::editor_groups_service::{
    DoubleClickTabAction, EditorActionsLocation, EditorLabelFormat, EditorPartDecorationOptions, EditorPartLimitOptions, EditorPartOptions, IEditorGroup,
    OpenPositioning, OpenSideBySideDirection, PinnedTabSizing, ShowTabs, SplitInGroupLayout, SplitSizing, TabActionLocation, TabHeight, TabSizing,
    TitleScrollbarSizing, TitleScrollbarVisibility,
};

/// `IEditorPartCreationOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorPartCreationOptions {
    pub restore_previous_state: bool,
}

/// `DEFAULT_EDITOR_MIN_DIMENSIONS`.
pub const DEFAULT_EDITOR_MIN_DIMENSIONS: Dimension = Dimension { width: 220.0, height: 70.0 };

/// `DEFAULT_EDITOR_MAX_DIMENSIONS`.
pub const DEFAULT_EDITOR_MAX_DIMENSIONS: Dimension = Dimension { width: f64::INFINITY, height: f64::INFINITY };

/// `DEFAULT_EDITOR_PART_OPTIONS`. Os campos de objeto são os getters do original: cada chamada devolve valores novos.
pub fn default_editor_part_options() -> EditorPartOptions {
    EditorPartOptions {
        show_tabs: ShowTabs::Multiple,
        wrap_tabs: false,
        scroll_to_switch_tabs: false,
        highlight_modified_tabs: false,
        tab_action_location: TabActionLocation::Right,
        tab_action_reserve_space: true,
        tab_action_close_visibility: true,
        tab_action_unpin_visibility: true,
        show_tab_index: false,
        always_show_editor_actions: false,
        tab_sizing: TabSizing::Fit,
        tab_sizing_fixed_min_width: 50.0,
        tab_sizing_fixed_max_width: 160.0,
        pinned_tab_sizing: PinnedTabSizing::Normal,
        pinned_tabs_on_separate_row: false,
        tab_height: TabHeight::Default,
        prevent_pinned_editor_close: PreventPinnedEditorClose::KeyboardAndMouse,
        title_scrollbar_sizing: TitleScrollbarSizing::Default,
        title_scrollbar_visibility: TitleScrollbarVisibility::Auto,
        focus_recent_editor_after_close: true,
        show_icons: true,
        enable_preview: true,
        enable_preview_from_quick_open: false,
        enable_preview_from_code_navigation: false,
        close_on_file_delete: false,
        open_positioning: OpenPositioning::Right,
        open_side_by_side_direction: OpenSideBySideDirection::Right,
        close_empty_groups: true,
        auto_lock_groups: BTreeSet::new(),
        reveal_if_open: false,
        swipe_to_navigate: false,
        mouse_back_forward_to_navigate: true,
        label_format: EditorLabelFormat::Default,
        restore_view_state: true,
        split_in_group_layout: SplitInGroupLayout::Horizontal,
        split_sizing: SplitSizing::Auto,
        split_on_drag_and_drop: true,
        allow_drop_into_group: true,
        drag_to_open_window: true,
        centered_layout_fixed_width: false,
        double_click_tab_to_toggle_editor_group_sizes: DoubleClickTabAction::Expand,
        editor_actions_location: EditorActionsLocation::Default,
        limit: EditorPartLimitOptions { enabled: false, exclude_dirty: false, value: 10.0, per_editor_group: false },
        decorations: EditorPartDecorationOptions { badges: true, colors: true },
        has_icons: true, // 'vs-seti' is our default icon theme
        show_breadcrumbs: None,
    }
}

/// `impactsEditorPartOptions`.
pub fn impacts_editor_part_options(event: &ConfigurationChangeEvent) -> bool {
    event.affects_configuration("workbench.editor", None) || event.affects_configuration("workbench.iconTheme", None) || event.affects_configuration("window.density", None)
}

/// As propriedades próprias de um objeto dinâmico (vazio para o que não é objeto), para a leitura por nome.
struct Source(Vec<(String, Dynamic)>);

impl Source {
    fn of(value: &Dynamic) -> Self {
        Self(value.own_entries().into_iter().map(|(key, value)| (key.to_string_lossy(), value)).collect())
    }

    fn get(&self, key: &str) -> Option<&Dynamic> {
        self.0.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    /// `BooleanVerifier`: só um booleano vale.
    fn assign_bool(&self, key: &str, target: &mut bool) {
        if let Some(Dynamic::Bool(value)) = self.get(key) {
            *target = *value;
        }
    }

    /// `NumberVerifier`: só um número vale.
    fn assign_number(&self, key: &str, target: &mut f64) {
        if let Some(Dynamic::Number(value)) = self.get(key) {
            *target = *value;
        }
    }

    /// `EnumVerifier`: só um dos textos permitidos vale.
    fn assign_enum<E>(&self, key: &str, target: &mut E, parse: impl Fn(&str) -> Option<E>) {
        if let Some(Dynamic::String(value)) = self.get(key)
            && let Some(parsed) = parse(&value.to_string_lossy())
        {
            *target = parsed;
        }
    }
}

fn parse_prevent_pinned_editor_close(text: &str) -> Option<PreventPinnedEditorClose> {
    match text {
        "keyboardAndMouse" => Some(PreventPinnedEditorClose::KeyboardAndMouse),
        "keyboard" => Some(PreventPinnedEditorClose::Keyboard),
        "mouse" => Some(PreventPinnedEditorClose::Mouse),
        "never" => Some(PreventPinnedEditorClose::Never),
        _ => None,
    }
}

/// `Object.assign(options, source)` seguido do `verifyObject` de `validateEditorPartOptions`, ver o cabeçalho.
pub fn assign_part_options(options: &mut EditorPartOptions, source: &Dynamic) {
    let source = Source::of(source);

    // Migrate: Show tabs (config migration kicks in very late and can cause flicker otherwise)
    if let Some(Dynamic::Bool(show_tabs)) = source.get("showTabs") {
        options.show_tabs = if *show_tabs { ShowTabs::Multiple } else { ShowTabs::Single };
    }

    source.assign_bool("wrapTabs", &mut options.wrap_tabs);
    source.assign_bool("scrollToSwitchTabs", &mut options.scroll_to_switch_tabs);
    source.assign_bool("highlightModifiedTabs", &mut options.highlight_modified_tabs);
    source.assign_bool("tabActionReserveSpace", &mut options.tab_action_reserve_space);
    source.assign_bool("tabActionCloseVisibility", &mut options.tab_action_close_visibility);
    source.assign_bool("tabActionUnpinVisibility", &mut options.tab_action_unpin_visibility);
    source.assign_bool("showTabIndex", &mut options.show_tab_index);
    source.assign_bool("alwaysShowEditorActions", &mut options.always_show_editor_actions);
    source.assign_bool("pinnedTabsOnSeparateRow", &mut options.pinned_tabs_on_separate_row);
    source.assign_bool("focusRecentEditorAfterClose", &mut options.focus_recent_editor_after_close);
    source.assign_bool("showIcons", &mut options.show_icons);
    source.assign_bool("enablePreview", &mut options.enable_preview);
    source.assign_bool("enablePreviewFromQuickOpen", &mut options.enable_preview_from_quick_open);
    source.assign_bool("enablePreviewFromCodeNavigation", &mut options.enable_preview_from_code_navigation);
    source.assign_bool("closeOnFileDelete", &mut options.close_on_file_delete);
    source.assign_bool("closeEmptyGroups", &mut options.close_empty_groups);
    source.assign_bool("revealIfOpen", &mut options.reveal_if_open);
    source.assign_bool("swipeToNavigate", &mut options.swipe_to_navigate);
    source.assign_bool("mouseBackForwardToNavigate", &mut options.mouse_back_forward_to_navigate);
    source.assign_bool("restoreViewState", &mut options.restore_view_state);
    source.assign_bool("splitOnDragAndDrop", &mut options.split_on_drag_and_drop);
    source.assign_bool("allowDropIntoGroup", &mut options.allow_drop_into_group);
    source.assign_bool("dragToOpenWindow", &mut options.drag_to_open_window);
    source.assign_bool("centeredLayoutFixedWidth", &mut options.centered_layout_fixed_width);
    source.assign_bool("hasIcons", &mut options.has_icons);

    source.assign_number("tabSizingFixedMinWidth", &mut options.tab_sizing_fixed_min_width);
    source.assign_number("tabSizingFixedMaxWidth", &mut options.tab_sizing_fixed_max_width);

    source.assign_enum("showTabs", &mut options.show_tabs, ShowTabs::parse);
    source.assign_enum("tabActionLocation", &mut options.tab_action_location, TabActionLocation::parse);
    source.assign_enum("tabSizing", &mut options.tab_sizing, TabSizing::parse);
    source.assign_enum("pinnedTabSizing", &mut options.pinned_tab_sizing, PinnedTabSizing::parse);
    source.assign_enum("tabHeight", &mut options.tab_height, TabHeight::parse);
    source.assign_enum("preventPinnedEditorClose", &mut options.prevent_pinned_editor_close, parse_prevent_pinned_editor_close);
    source.assign_enum("titleScrollbarSizing", &mut options.title_scrollbar_sizing, TitleScrollbarSizing::parse);
    source.assign_enum("titleScrollbarVisibility", &mut options.title_scrollbar_visibility, TitleScrollbarVisibility::parse);
    source.assign_enum("openPositioning", &mut options.open_positioning, OpenPositioning::parse);
    source.assign_enum("openSideBySideDirection", &mut options.open_side_by_side_direction, OpenSideBySideDirection::parse);
    source.assign_enum("labelFormat", &mut options.label_format, EditorLabelFormat::parse);
    source.assign_enum("splitInGroupLayout", &mut options.split_in_group_layout, SplitInGroupLayout::parse);
    source.assign_enum("splitSizing", &mut options.split_sizing, SplitSizing::parse);
    source.assign_enum("doubleClickTabToToggleEditorGroupSizes", &mut options.double_click_tab_to_toggle_editor_group_sizes, DoubleClickTabAction::parse);
    source.assign_enum("editorActionsLocation", &mut options.editor_actions_location, EditorActionsLocation::parse);

    // Special handle array types and convert to Set
    if let Some(auto_lock_groups @ Dynamic::Object(_)) = source.get("autoLockGroups") {
        options.auto_lock_groups = auto_lock_groups
            .own_entries()
            .into_iter()
            .filter(|(_, enablement)| matches!(enablement, Dynamic::Bool(true)))
            .map(|(editor_id, _)| editor_id.to_string_lossy())
            .collect();
    }

    if let Some(limit @ Dynamic::Object(_)) = source.get("limit") {
        let limit = Source::of(limit);
        limit.assign_bool("enabled", &mut options.limit.enabled);
        limit.assign_number("value", &mut options.limit.value);
        limit.assign_bool("perEditorGroup", &mut options.limit.per_editor_group);
        limit.assign_bool("excludeDirty", &mut options.limit.exclude_dirty);
    }

    if let Some(decorations @ Dynamic::Object(_)) = source.get("decorations") {
        let decorations = Source::of(decorations);
        decorations.assign_bool("badges", &mut options.decorations.badges);
        decorations.assign_bool("colors", &mut options.decorations.colors);
    }
}

/// `getEditorPartOptions(configurationService, themeService)`: `has_file_icons` é o `hasFileIcons` do tema de ícones.
pub fn get_editor_part_options(configuration_service: &dyn IConfigurationService, has_file_icons: bool) -> EditorPartOptions {
    let mut options = EditorPartOptions { has_icons: has_file_icons, ..default_editor_part_options() };

    // Assign all primitive configuration over
    let editor_configuration = configuration_service.get_value(Some("workbench.editor"), None);
    assign_part_options(&mut options, &editor_configuration);

    let editor_tab_height = configuration_service.get_value(Some("window.density.editorTabHeight"), None);
    if let Dynamic::String(editor_tab_height) = &editor_tab_height
        && let Some(tab_height) = TabHeight::parse(&editor_tab_height.to_string_lossy())
    {
        options.tab_height = tab_height;
    }

    options
}

/// `fillActiveEditorViewState(group, expectedActiveEditor, presetOptions)`: when moving/copying an editor, try to preserve as
/// much view state as possible by checking for the editor to be the active one of the group and reading the view state of its
/// pane. O `{ ...presetOptions, viewState }` do original é o `EditorOptions` com o `view_state` trocado.
pub fn fill_active_editor_view_state(group: &dyn IEditorGroup, expected_active_editor: Option<&EditorInputRef>, preset_options: Option<EditorOptions>) -> EditorOptions {
    let matches_active_editor = match (expected_active_editor, group.active_editor()) {
        (Some(expected), Some(active)) => expected.matches(&EditorInputOrUntyped::Typed(active)),
        _ => true,
    };

    let mut options = preset_options.unwrap_or_default();
    if matches_active_editor {
        options.view_state = group.active_editor_pane().and_then(|pane| pane.get_view_state());
    }

    options
}

/// `IEditorPartsView`, no recorte que as partes e os grupos leem: todos os grupos de todas as partes e a ligação de uma chave
/// de contexto a um grupo (`bind`). O `EditorParts` implementa; a parte avulsa (sem `EditorParts`) não tem onde ligar as
/// chaves e usa só os próprios grupos. `registerPart`, `mainPart`, `activeGroup`, `getGroups`, `count` e as criações de parte
/// auxiliar e modal são métodos do próprio `EditorParts`.
pub trait EditorPartsView {
    /// `groups`: os grupos de todas as partes, na ordem de criação.
    fn groups(&self) -> Vec<Rc<EditorGroupView>>;

    /// `getGroup(identifier)`: o grupo de qualquer parte.
    fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<EditorGroupView>>;

    /// `bind(contextKey, group)`: liga a chave ao contexto escopado do grupo e, quando o grupo é o ativo, ao contexto global.
    fn bind(&self, context_key: &RawContextKey, group: &Rc<EditorGroupView>) -> Rc<dyn IContextKey>;
}
