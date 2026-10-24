//! Porte parcial de `src/vs/workbench/services/editor/common/editorGroupsService.ts`: o recorte (`IEditorGroup`, `IEditorPart`, `IModalEditorPart`,
//! `IEditorGroupsService`, `GroupsOrder`, `GroupDirection`, `GroupLocation`, `IFindGroupScope`,
//! `preferredSideBySideGroupDirection`), as constantes e o `PreferredGroup` de `editorService.ts` (`ACTIVE_GROUP`,
//! `SIDE_GROUP`, `AUX_WINDOW_GROUP`, `MODAL_GROUP`, `USE_MODAL_EDITOR_SETTING`, `UseModalEditorMode`) e o
//! `EditorInputWithOptionsAndGroup` de `src/vs/workbench/common/editor.ts`.
//!
//! Divergências:
//! - **Propriedades** (`id`, `editors`, `activeEditor`, `isLocked`, `activeGroup`, `groups`, `mainPart`...) viram métodos.
//! - **`closeEditor`** e **`moveEditor`** perdem o parâmetro `options` (`ICloseEditorOptions`, `IEditorOptions`), que o
//!   resolvedor nunca passa. **`findGroup`** perde `source` e `wrap` pelo mesmo motivo. **`addGroup`** recebe
//!   `Option<GroupRef>`: `None` é o `undefined` de `editorGroups[i - 1]` com `i = 0` que o `columnToEditorGroup` passa.
//! - **`PreferredGroup`** (`IEditorGroup | number`) é um enum: os quatro números conhecidos são variantes e os demais
//!   identificadores são [`PreferredGroup::Id`]. `isPreferredGroup` (checagem de forma sobre `unknown`) não tem equivalente.
//! - **`IModalEditorPart.close()`** devolve `bool`, como no original; o resto do `IModalEditorPart` (maximizar, barra
//!   lateral, navegação) entra com o editor modal.
//! - **Contrato completo, com padrões.** Os membros que o porte já lia (o `QueryBuilder`, o `EditorResolverService`, o
//!   `findGroup` e o `columnToEditorGroup`) continuam obrigatórios; todos os demais (eventos, layout, ativação, mesclar e
//!   copiar grupos, conjuntos de trabalho, as operações de editor do `IEditorGroup`) têm implementação padrão: os eventos
//!   nunca disparam, `count` e `isEmpty` saem de `editors`/`groups`, e o resto lança `Method not implemented.` (o erro do
//!   dublê do original). `EditorPart`, `EditorGroupView` e `EditorParts` sobrescrevem todos.
//! - **`IEditorGroupsContainer`** não é um trait: o original o herda em `IEditorPart` e `IEditorGroupsService`, e herdar
//!   aqui obrigaria cada implementador existente a um `impl` novo. Os membros novos do container vêm de uma única macro
//!   (`editor_groups_container_members`) expandida nos dois traits, e `getGroups`, `getGroup`, `findGroup` e `addGroup`
//!   entram no `IEditorPart` com padrão.
//! - **Sobrecargas com `options`.** Onde o método existente perdeu um parâmetro (`closeEditor`, `moveEditor`, `findGroup`,
//!   `IModalEditorPart.close`), o original completo é um método irmão: `close_editor_with_options`,
//!   `move_editor_with_options`, `find_group_from` e `close_with_options`. `closeAllEditors({ excludeConfirming })`, que
//!   devolve `boolean` e não promessa, é `close_all_editors_excluding_confirming`; `getPart(container)` é
//!   `get_part_by_container`.
//! - **`createAuxiliaryEditorPart`** continua devolvendo `IEditorPart` (o original devolve `IAuxiliaryEditorPart`, que
//!   existe aqui como trait mas ainda não é o retorno).
//! - **Tipos de `common/editor.ts`** que o contrato lê e que ainda não existiam (`IEditorPartOptions` e seus textos
//!   fechados, `IEditorPartOptionsChangeEvent`, `IToolbarActions`) ficam nesta região. `enforcePartOptions` recebe
//!   `Dynamic` no lugar de `DeepPartial<IEditorPartOptions>`; `IEditorGroupContextKeyProvider<T>` usa `Dynamic` no lugar de
//!   `T extends ContextKeyValue`. `isEditorReplacement` e `isEditorGroup` não checam forma (a tipagem já garante).
//! - **Identidade de grupo.** Onde o original compara grupos com `===` ou `indexOf`, o porte compara o `id` do grupo, que é
//!   único por grupo.

use std::any::Any;
use std::collections::BTreeSet;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, throw};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::core_2d::dimension::Dimension;
use je_instantiation::instantiation_service::InstantiationService;
use je_instantiation::service_id;
use je_platform::actions::actions::{MenuChangeEvent, MenuId};
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{IContextKeyService, RawContextKey};
use je_platform::opener::editor_types::{AuxiliaryEditorOptions, EditorOptions, EditorPosition, ModalEditorNavigation, ModalEditorPartOptions};

use crate::common::editor::editor_group_model::{EditorOrIndex, GetEditorsOptions, GroupModelChangeEvent};
use crate::common::editor::{
    ActiveEditorChangeEvent, CloseDirection, EditorCloseEvent, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions,
    EditorWillMoveEvent, EditorWillOpenEvent, EditorsOrder, FindEditorOptions, GroupIdentifier, IEditorPane, MatchEditorOptions,
    PreventPinnedEditorClose, TextEditorOptions,
};
use crate::services::layout::layout_service::WindowId;

// region: editorService.ts

/// `ACTIVE_GROUP`: open an editor in the currently active group.
pub const ACTIVE_GROUP: GroupIdentifier = -1;

/// `SIDE_GROUP`: open an editor to the side of the active group.
pub const SIDE_GROUP: GroupIdentifier = -2;

/// `AUX_WINDOW_GROUP`: open an editor in a new auxiliary window.
pub const AUX_WINDOW_GROUP: GroupIdentifier = -3;

/// `MODAL_GROUP`: open an editor in a modal overlay on top of the workbench.
pub const MODAL_GROUP: GroupIdentifier = -4;

/// `USE_MODAL_EDITOR_SETTING`: setting that controls whether editors open in a modal editor part.
pub const USE_MODAL_EDITOR_SETTING: &str = "workbench.editor.useModal";

/// `UseModalEditorMode`: possible values for the `workbench.editor.useModal` setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseModalEditorMode {
    /// `'off'`: never open editors modal (user opt-out, honored over `RequiresModal`).
    Off,
    /// `'some'`: open modal only for editors that request it (e.g. `RequiresModal`).
    Some,
    /// `'all'`: open all editors modal.
    All,
}

/// `configurationService.getValue<UseModalEditorMode>(USE_MODAL_EDITOR_SETTING)`: um valor que não é um dos três textos
/// (inclusive `undefined`) é `None`, e nenhuma das comparações do original (`=== 'off'`, `=== 'all'`) o reconhece.
pub fn use_modal_editor_mode(configuration_service: &dyn IConfigurationService) -> Option<UseModalEditorMode> {
    match configuration_service.get_value(Some(USE_MODAL_EDITOR_SETTING), None) {
        Dynamic::String(mode) if mode == "off" => Some(UseModalEditorMode::Off),
        Dynamic::String(mode) if mode == "some" => Some(UseModalEditorMode::Some),
        Dynamic::String(mode) if mode == "all" => Some(UseModalEditorMode::All),
        _ => None,
    }
}

/// `PreferredGroup`: `IEditorGroup | GroupIdentifier | SIDE_GROUP_TYPE | ACTIVE_GROUP_TYPE | AUX_WINDOW_GROUP_TYPE |
/// MODAL_GROUP_TYPE`.
#[derive(Clone)]
pub enum PreferredGroup {
    /// Uma instância de grupo.
    Group(Rc<dyn IEditorGroup>),
    /// Um identificador de grupo específico (`>= 0` para um grupo existente).
    Id(GroupIdentifier),
    Active,
    Side,
    AuxWindow,
    Modal,
}

impl PreferredGroup {
    /// O `number` do original: os quatro valores conhecidos viram as variantes próprias.
    pub fn from_number(group: GroupIdentifier) -> Self {
        match group {
            ACTIVE_GROUP => PreferredGroup::Active,
            SIDE_GROUP => PreferredGroup::Side,
            AUX_WINDOW_GROUP => PreferredGroup::AuxWindow,
            MODAL_GROUP => PreferredGroup::Modal,
            id => PreferredGroup::Id(id),
        }
    }

    /// `typeof preferredGroup === 'number'`: o valor numérico, ou `None` para uma instância de grupo.
    pub fn as_number(&self) -> Option<GroupIdentifier> {
        match self {
            PreferredGroup::Group(_) => None,
            PreferredGroup::Id(id) => Some(*id),
            PreferredGroup::Active => Some(ACTIVE_GROUP),
            PreferredGroup::Side => Some(SIDE_GROUP),
            PreferredGroup::AuxWindow => Some(AUX_WINDOW_GROUP),
            PreferredGroup::Modal => Some(MODAL_GROUP),
        }
    }
}

impl From<Rc<dyn IEditorGroup>> for PreferredGroup {
    fn from(group: Rc<dyn IEditorGroup>) -> Self {
        PreferredGroup::Group(group)
    }
}

// endregion

// region: common/editor.ts

/// `EditorInputWithOptionsAndGroup`: o editor resolvido com as opções e o grupo em que abre.
#[derive(Clone)]
pub struct EditorInputWithOptionsAndGroup {
    pub editor: EditorInputRef,
    pub options: Option<TextEditorOptions>,
    pub group: Rc<dyn IEditorGroup>,
}

impl EditorInputWithOptionsAndGroup {
    /// `{ ...input, group }`.
    pub fn new(input: EditorInputWithOptions, group: Rc<dyn IEditorGroup>) -> Self {
        Self { editor: input.editor, options: input.options, group }
    }
}

// endregion

// region: editorGroupsService.ts

/// `GroupsOrder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupsOrder {
    /// Groups sorted by creation order (oldest one first).
    CreationTime,
    /// Groups sorted by most recent activity (most recent active first).
    MostRecentlyActive,
    /// Groups sorted by grid widget order.
    GridAppearance,
}

/// `GroupDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupDirection {
    Up,
    Down,
    Left,
    Right,
}

/// `GroupLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupLocation {
    First,
    Last,
    Next,
    Previous,
}

/// `IFindGroupScope`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindGroupScope {
    pub direction: Option<GroupDirection>,
    pub location: Option<GroupLocation>,
}

/// `IEditorGroup | GroupIdentifier`: um grupo dado por instância ou por identificador.
#[derive(Clone)]
pub enum GroupRef {
    Group(Rc<dyn IEditorGroup>),
    Id(GroupIdentifier),
}

/// `GroupActivationReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupActivationReason {
    /// Group was activated explicitly by user or programmatic action.
    Default = 0,
    /// Group was activated because a modal or auxiliary editor part was closing.
    PartClose = 1,
}

/// `IEditorGroupActivationEvent`.
#[derive(Clone)]
pub struct EditorGroupActivationEvent {
    pub group: Rc<dyn IEditorGroup>,
    pub reason: GroupActivationReason,
}

/// `GroupOrientation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupOrientation {
    Horizontal,
    Vertical,
}

/// `GroupsArrangement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupsArrangement {
    /// Make the current active group consume the entire editor area.
    Maximize,
    /// Make the current active group consume the maximum amount of space possible.
    Expand,
    /// Size all groups evenly.
    Even,
}

/// `GroupLayoutArgument`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupLayoutArgument {
    /// Only applies when there are multiple groups arranged next to each other in a row or column. If provided, their sum
    /// must be 1 to be applied per row or column.
    pub size: Option<f64>,
    /// Editor groups will be laid out orthogonal to the parent orientation.
    pub groups: Option<Vec<GroupLayoutArgument>>,
}

/// `EditorGroupLayout`.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorGroupLayout {
    /// The initial orientation of the editor groups at the root.
    pub orientation: GroupOrientation,
    /// The editor groups at the root of the layout.
    pub groups: Vec<GroupLayoutArgument>,
}

/// `MergeGroupMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeGroupMode {
    CopyEditors,
    MoveEditors,
}

/// `IMergeGroupOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MergeGroupOptions {
    pub mode: Option<MergeGroupMode>,
    pub index: Option<usize>,
    /// Set this to prevent editors already present in the target group from moving to a different index as they are in the
    /// source group.
    pub preserve_existing_index: Option<bool>,
}

/// `ICloseEditorOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloseEditorOptions {
    pub preserve_focus: Option<bool>,
    /// Forces editors to close even when they declare `EditorInputCapabilities.CannotClose`.
    pub force: Option<bool>,
}

/// `ICloseEditorsFilter`.
#[derive(Clone, Default)]
pub struct CloseEditorsFilter {
    pub except: Option<EditorInputRef>,
    pub direction: Option<CloseDirection>,
    pub saved_only: Option<bool>,
    pub exclude_sticky: Option<bool>,
}

/// `EditorInput[] | ICloseEditorsFilter`: o primeiro parâmetro de `closeEditors`.
#[derive(Clone)]
pub enum CloseEditorsTarget {
    Editors(Vec<EditorInputRef>),
    Filter(CloseEditorsFilter),
}

/// `ICloseAllEditorsOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloseAllEditorsOptions {
    pub exclude_sticky: Option<bool>,
    pub exclude_confirming: Option<bool>,
    pub force: Option<bool>,
}

/// `IEditorReplacement`.
#[derive(Clone)]
pub struct EditorReplacement {
    pub editor: EditorInputRef,
    pub replacement: EditorInputRef,
    pub options: Option<EditorOptions>,
    /// Skips asking the user for confirmation and doesn't save the document. Only use this if you really need to!
    pub force_replace_dirty: Option<bool>,
}

/// `IEditorSideGroup`.
pub trait IEditorSideGroup {
    /// `openEditor(editor, options)`: open an editor in this group. Resolves around the pane unless the call failed, or the
    /// editor was not opened as active editor.
    fn open_editor(&self, editor: &EditorInputRef, options: Option<EditorOptions>) -> Promise<Option<Rc<dyn IEditorPane>>>;
}

/// `IEditorDropTargetDelegate`.
#[derive(Clone, Default)]
pub struct EditorDropTargetDelegate {
    /// `containsGroup(groupView)`: a helper to figure out if the drop target contains the provided group.
    pub contains_group: Option<Rc<dyn Fn(&Rc<dyn IEditorGroup>) -> bool>>,
    /// `supportsSplitting`: whether the drop target supports creating editor groups.
    pub supports_splitting: Option<bool>,
}

/// `IEditorWorkingSet`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorWorkingSet {
    pub id: String,
    pub name: String,
}

/// `IEditorWorkingSetOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorWorkingSetOptions {
    pub preserve_focus: Option<bool>,
}

/// `IEditorWorkingSet | 'empty'`: o parâmetro de `applyWorkingSet`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkingSetTarget {
    Set(EditorWorkingSet),
    Empty,
}

/// `IEditorGroupContextKeyProvider<T>`. O `T extends ContextKeyValue` é um [`Dynamic`], como o valor da
/// [`RawContextKey`].
#[derive(Clone)]
pub struct EditorGroupContextKeyProvider {
    /// `contextKey`: the context key that needs to be set for each editor group context and the global context.
    pub context_key: RawContextKey,
    /// `getGroupContextKeyValue(group)`: retrieves the context key value for the given editor group.
    pub get_group_context_key_value: Rc<dyn Fn(&Rc<dyn IEditorGroup>) -> Dynamic>,
    /// `onDidChange`: an event that is fired when there was a change leading to the context key value to be re-evaluated.
    pub on_did_change: Option<Event<()>>,
}

/// `OpenEditorContext`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenEditorContext {
    NewEditor = 1,
    MoveEditor = 2,
    CopyEditor = 3,
}

/// `IActiveEditorActions`.
#[derive(Clone)]
pub struct ActiveEditorActions {
    pub actions: ToolbarActions,
    /// `onDidChange`: `Event<IMenuChangeEvent | void>`.
    pub on_did_change: Event<Option<MenuChangeEvent>>,
}

/// `IModalEditorPart.close(options)`: `{ mergeAllEditorsToMainPart?: boolean }`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModalEditorPartCloseOptions {
    pub merge_all_editors_to_main_part: Option<bool>,
}

// endregion

// region: common/editor.ts (o que o contrato do serviço lê)

/// `IToolbarActions`.
#[derive(Clone, Default)]
pub struct ToolbarActions {
    pub primary: Vec<ActionRef>,
    pub secondary: Vec<ActionRef>,
}

/// `IEditorPartLimitOptions`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorPartLimitOptions {
    pub enabled: bool,
    pub exclude_dirty: bool,
    pub value: f64,
    pub per_editor_group: bool,
}

/// `IEditorPartDecorationOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorPartDecorationOptions {
    pub badges: bool,
    pub colors: bool,
}

/// Uma união de textos do original (`'multiple' | 'single' | 'none'`...) como enum com `as_str` e `parse`.
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

string_enum!(
    /// `showTabs`.
    ShowTabs { Multiple = "multiple", Single = "single", None = "none" }
);
string_enum!(
    /// `tabActionLocation`.
    TabActionLocation { Left = "left", Right = "right" }
);
string_enum!(
    /// `tabSizing`.
    TabSizing { Fit = "fit", Shrink = "shrink", Fixed = "fixed" }
);
string_enum!(
    /// `pinnedTabSizing`.
    PinnedTabSizing { Normal = "normal", Compact = "compact", Shrink = "shrink" }
);
string_enum!(
    /// `tabHeight`.
    TabHeight { Default = "default", Compact = "compact" }
);
string_enum!(
    /// `titleScrollbarSizing`.
    TitleScrollbarSizing { Default = "default", Large = "large" }
);
string_enum!(
    /// `titleScrollbarVisibility`.
    TitleScrollbarVisibility { Auto = "auto", Visible = "visible", Hidden = "hidden" }
);
string_enum!(
    /// `openPositioning`.
    OpenPositioning { Left = "left", Right = "right", First = "first", Last = "last" }
);
string_enum!(
    /// `openSideBySideDirection`.
    OpenSideBySideDirection { Right = "right", Down = "down" }
);
string_enum!(
    /// `labelFormat`.
    EditorLabelFormat { Default = "default", Short = "short", Medium = "medium", Long = "long" }
);
string_enum!(
    /// `splitInGroupLayout`.
    SplitInGroupLayout { Vertical = "vertical", Horizontal = "horizontal" }
);
string_enum!(
    /// `splitSizing`.
    SplitSizing { Auto = "auto", Split = "split", Distribute = "distribute" }
);
string_enum!(
    /// `doubleClickTabToToggleEditorGroupSizes`.
    DoubleClickTabAction { Maximize = "maximize", Expand = "expand", Off = "off" }
);
string_enum!(
    /// `editorActionsLocation`.
    EditorActionsLocation { Default = "default", TitleBar = "titleBar", Hidden = "hidden" }
);

/// `IEditorPartOptions`: `DeepRequiredNonNullable<IEditorPartConfiguration>` mais `hasIcons` e `showBreadcrumbs`.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorPartOptions {
    pub show_tabs: ShowTabs,
    pub wrap_tabs: bool,
    pub scroll_to_switch_tabs: bool,
    pub highlight_modified_tabs: bool,
    pub tab_action_location: TabActionLocation,
    pub tab_action_reserve_space: bool,
    pub tab_action_close_visibility: bool,
    pub tab_action_unpin_visibility: bool,
    pub show_tab_index: bool,
    pub always_show_editor_actions: bool,
    pub tab_sizing: TabSizing,
    pub tab_sizing_fixed_min_width: f64,
    pub tab_sizing_fixed_max_width: f64,
    pub pinned_tab_sizing: PinnedTabSizing,
    pub pinned_tabs_on_separate_row: bool,
    pub tab_height: TabHeight,
    pub prevent_pinned_editor_close: PreventPinnedEditorClose,
    pub title_scrollbar_sizing: TitleScrollbarSizing,
    pub title_scrollbar_visibility: TitleScrollbarVisibility,
    pub focus_recent_editor_after_close: bool,
    pub show_icons: bool,
    pub enable_preview: bool,
    pub enable_preview_from_quick_open: bool,
    pub enable_preview_from_code_navigation: bool,
    pub close_on_file_delete: bool,
    pub open_positioning: OpenPositioning,
    pub open_side_by_side_direction: OpenSideBySideDirection,
    pub close_empty_groups: bool,
    pub auto_lock_groups: BTreeSet<String>,
    pub reveal_if_open: bool,
    pub swipe_to_navigate: bool,
    pub mouse_back_forward_to_navigate: bool,
    pub label_format: EditorLabelFormat,
    pub restore_view_state: bool,
    pub split_in_group_layout: SplitInGroupLayout,
    pub split_sizing: SplitSizing,
    pub split_on_drag_and_drop: bool,
    pub allow_drop_into_group: bool,
    pub drag_to_open_window: bool,
    pub centered_layout_fixed_width: bool,
    pub double_click_tab_to_toggle_editor_group_sizes: DoubleClickTabAction,
    pub editor_actions_location: EditorActionsLocation,
    pub limit: EditorPartLimitOptions,
    pub decorations: EditorPartDecorationOptions,
    pub has_icons: bool,
    pub show_breadcrumbs: Option<bool>,
}

/// `IEditorPartOptionsChangeEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorPartOptionsChangeEvent {
    pub old_part_options: EditorPartOptions,
    pub new_part_options: EditorPartOptions,
}

// endregion

// region: editorGroupsService.ts (funções e contratos)

/// O erro que o dublê `TestEditorGroupsService` do original lança nos membros que não implementa.
fn not_implemented() -> ! {
    throw(Error::new("Method not implemented."))
}

/// `preferredSideBySideGroupDirection`: `GroupDirection.DOWN | GroupDirection.RIGHT`.
pub fn preferred_side_by_side_group_direction(configuration_service: &dyn IConfigurationService) -> GroupDirection {
    let open_side_by_side_direction = configuration_service.get_value(Some("workbench.editor.openSideBySideDirection"), None);

    if matches!(&open_side_by_side_direction, Dynamic::String(direction) if direction == "down") {
        return GroupDirection::Down;
    }

    GroupDirection::Right
}

/// `IEditorGroup`, reduzido ao que o resolvedor e o `findGroup` leem.
pub trait IEditorGroup {
    /// `id`: A unique identifier of this group that remains identical even if the group is moved to different locations.
    fn id(&self) -> GroupIdentifier;

    /// `windowId`: the identifier of the window this group is in.
    fn window_id(&self) -> WindowId;

    /// `activeEditor`: the active editor of the group or `None` if none.
    fn active_editor(&self) -> Option<EditorInputRef>;

    /// `editors`: all editors that are currently opened in the group.
    fn editors(&self) -> Vec<EditorInputRef>;

    /// `isLocked`: whether the group is locked.
    fn is_locked(&self) -> bool;

    /// `isActive(editor)`: find out if the provided editor is active in the group.
    fn is_active(&self, editor: &EditorInputOrUntyped) -> bool;

    /// `closeEditor(editor?)`: closes an editor from the group. Resolves to `false` when there was a veto closing the
    /// editor, e.g. when it is dirty. Sem `editor`, fecha o editor ativo.
    /// Tem implementação padrão (lança o erro do dublê) para o `EditorGroupView` da fatia 2, que ainda não abre nem fecha
    /// editores (isso vem com os painéis de editor, na fatia 3).
    fn close_editor(&self, _editor: Option<&EditorInputRef>) -> Promise<bool> {
        not_implemented()
    }

    /// `moveEditor(editor, target)`: move an editor from this group either within this group or to another group.
    /// Returns whether the editor was moved or not. Mesma observação de [`IEditorGroup::close_editor`].
    fn move_editor(&self, _editor: &EditorInputRef, _target: &Rc<dyn IEditorGroup>) -> bool {
        not_implemented()
    }

    // Os membros abaixo completam o `IEditorGroup`. Todos têm implementação padrão para não quebrar quem implementa só o
    // recorte acima: os eventos nunca disparam, `count` e `isEmpty` saem de `editors`, e o resto lança o erro do dublê.

    /// `onDidModelChange`: an event which fires whenever the underlying group model changes.
    fn on_did_model_change(&self) -> Event<GroupModelChangeEvent> {
        Event::none()
    }

    /// `onWillDispose`: an event that is fired when the group gets disposed.
    fn on_will_dispose(&self) -> Event<()> {
        Event::none()
    }

    /// `onDidActiveEditorChange`: an event that is fired when the active editor in the group changed.
    fn on_did_active_editor_change(&self) -> Event<ActiveEditorChangeEvent> {
        Event::none()
    }

    /// `onWillCloseEditor`: an event that is fired when an editor is about to close.
    fn on_will_close_editor(&self) -> Event<EditorCloseEvent> {
        Event::none()
    }

    /// `onDidCloseEditor`: an event that is fired when an editor is closed.
    fn on_did_close_editor(&self) -> Event<EditorCloseEvent> {
        Event::none()
    }

    /// `onWillMoveEditor`: an event that is fired when an editor is about to move to a different group.
    fn on_will_move_editor(&self) -> Event<EditorWillMoveEvent> {
        Event::none()
    }

    /// `onWillOpenEditor`: an event that is fired when an editor is about to open in the group.
    fn on_will_open_editor(&self) -> Event<EditorWillOpenEvent> {
        Event::none()
    }

    /// `index`: the position of this group in the visual order of groups from left to right and top to bottom.
    fn index(&self) -> usize {
        not_implemented()
    }

    /// `label`: a human readable label for the group, that can change depending on the layout of all editor groups.
    fn label(&self) -> String {
        not_implemented()
    }

    /// `ariaLabel`: a human readable label for the group to be used by screen readers.
    fn aria_label(&self) -> String {
        not_implemented()
    }

    /// `activeEditorPane`: the currently visible editor pane of the group.
    fn active_editor_pane(&self) -> Option<Rc<dyn IEditorPane>> {
        not_implemented()
    }

    /// `selectedEditors`: all selected editors in this group in sequential order. The active editor is always part of the
    /// selection.
    fn selected_editors(&self) -> Vec<EditorInputRef> {
        not_implemented()
    }

    /// `previewEditor`: the editor in the group that is in preview mode if any.
    fn preview_editor(&self) -> Option<EditorInputRef> {
        not_implemented()
    }

    /// `count`: the number of opened editors in this group.
    fn count(&self) -> usize {
        self.editors().len()
    }

    /// `isEmpty`: whether the group has editors or not.
    fn is_empty(&self) -> bool {
        self.editors().is_empty()
    }

    /// `stickyCount`: the number of sticky editors in this group.
    fn sticky_count(&self) -> usize {
        not_implemented()
    }

    /// `scopedContextKeyService`: the scoped context key service for this group.
    fn scoped_context_key_service(&self) -> Rc<dyn IContextKeyService> {
        not_implemented()
    }

    /// `getEditors(order, options)`: get all editors that are currently opened in the group.
    fn get_editors(&self, _order: EditorsOrder, _options: Option<GetEditorsOptions>) -> Vec<EditorInputRef> {
        not_implemented()
    }

    /// `findEditors(resource, options)`: finds all editors for the given resource that are currently opened in the group.
    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorInputRef> {
        not_implemented()
    }

    /// `getEditorByIndex(index)`: the editor at a specific index of the group.
    fn get_editor_by_index(&self, _index: usize) -> Option<EditorInputRef> {
        not_implemented()
    }

    /// `getIndexOfEditor(editor)`: the index of the editor in the group or -1 if not opened.
    fn get_index_of_editor(&self, _editor: &EditorInputRef) -> i64 {
        not_implemented()
    }

    /// `isFirst(editor)`: whether the editor is the first in the group.
    fn is_first(&self, _editor: &EditorInputRef) -> bool {
        not_implemented()
    }

    /// `isLast(editor)`: whether the editor is the last in the group.
    fn is_last(&self, _editor: &EditorInputRef) -> bool {
        not_implemented()
    }

    /// `openEditor(editor, options)`: open an editor in this group. Resolves around the pane unless the call failed, or the
    /// editor was not opened as active editor.
    fn open_editor(&self, _editor: &EditorInputRef, _options: Option<EditorOptions>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        not_implemented()
    }

    /// `openEditors(editors)`: opens editors in this group. Since a group can only ever have one active editor, the result
    /// is only one pane.
    fn open_editors(&self, _editors: Vec<EditorInputWithOptions>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        not_implemented()
    }

    /// `isPinned(editorOrIndex)`: find out if the provided editor is pinned in the group.
    fn is_pinned(&self, _editor_or_index: &EditorOrIndex) -> bool {
        not_implemented()
    }

    /// `isSticky(editorOrIndex)`: find out if the provided editor or index of editor is sticky in the group.
    fn is_sticky(&self, _editor_or_index: &EditorOrIndex) -> bool {
        not_implemented()
    }

    /// `isTransient(editorOrIndex)`: find out if the provided editor or index of editor is transient in the group.
    fn is_transient(&self, _editor_or_index: &EditorOrIndex) -> bool {
        not_implemented()
    }

    /// `isSelected(editor)`: whether the editor is selected in the group.
    fn is_selected(&self, _editor: &EditorInputRef) -> bool {
        not_implemented()
    }

    /// `setSelection(activeSelectedEditor, inactiveSelectedEditors)`: replaces the current selection with the new one.
    fn set_selection(&self, _active_selected_editor: &EditorInputRef, _inactive_selected_editors: Vec<EditorInputRef>) -> Promise<()> {
        not_implemented()
    }

    /// `contains(candidate, options)`: find out if a certain editor is included in the group.
    fn contains(&self, _candidate: &EditorInputOrUntyped, _options: Option<MatchEditorOptions>) -> bool {
        not_implemented()
    }

    /// `moveEditor(editor, target, options)`: a variante com `options` do [`IEditorGroup::move_editor`]. Sem opções é o
    /// mesmo método.
    fn move_editor_with_options(&self, editor: &EditorInputRef, target: &Rc<dyn IEditorGroup>, options: Option<EditorOptions>) -> bool {
        if options.is_some() {
            not_implemented()
        }
        self.move_editor(editor, target)
    }

    /// `moveEditors(editors, target)`: move editors from this group either within this group or to another group.
    /// Returns whether all editors were moved or not.
    fn move_editors(&self, _editors: Vec<EditorInputWithOptions>, _target: &Rc<dyn IEditorGroup>) -> bool {
        not_implemented()
    }

    /// `copyEditor(editor, target, options)`: copy an editor from this group to another group.
    fn copy_editor(&self, _editor: &EditorInputRef, _target: &Rc<dyn IEditorGroup>, _options: Option<EditorOptions>) {
        not_implemented()
    }

    /// `copyEditors(editors, target)`: copy editors from this group to another group.
    fn copy_editors(&self, _editors: Vec<EditorInputWithOptions>, _target: &Rc<dyn IEditorGroup>) {
        not_implemented()
    }

    /// `closeEditor(editor, options)`: a variante com `options` do [`IEditorGroup::close_editor`]. Sem opções é o mesmo
    /// método.
    fn close_editor_with_options(&self, editor: Option<&EditorInputRef>, options: Option<CloseEditorOptions>) -> Promise<bool> {
        if options.is_some() {
            not_implemented()
        }
        self.close_editor(editor)
    }

    /// `closeEditors(editors, options)`: closes specific editors in this group. Resolves to `false` when there was a veto.
    fn close_editors(&self, _editors: CloseEditorsTarget, _options: Option<CloseEditorOptions>) -> Promise<bool> {
        not_implemented()
    }

    /// `closeAllEditors(options?)`: closes all editors from the group.
    fn close_all_editors(&self, _options: Option<CloseAllEditorsOptions>) -> Promise<bool> {
        not_implemented()
    }

    /// `closeAllEditors({ excludeConfirming: true, force })`: a sobrecarga que devolve `boolean` direto, sem promessa.
    fn close_all_editors_excluding_confirming(&self, _force: Option<bool>) -> bool {
        not_implemented()
    }

    /// `replaceEditors(editors)`: replaces editors in this group with the provided replacement. Resolves when the replaced
    /// active editor (if any) has finished loading.
    fn replace_editors(&self, _editors: Vec<EditorReplacement>) -> Promise<()> {
        not_implemented()
    }

    /// `pinEditor(editor?)`: set an editor to be pinned. Sem `editor`, o editor ativo.
    fn pin_editor(&self, _editor: Option<&EditorInputRef>) {
        not_implemented()
    }

    /// `stickEditor(editor?)`: set an editor to be sticky. Sem `editor`, o editor ativo.
    fn stick_editor(&self, _editor: Option<&EditorInputRef>) {
        not_implemented()
    }

    /// `unstickEditor(editor?)`: set an editor to be non-sticky. Sem `editor`, o editor ativo.
    fn unstick_editor(&self, _editor: Option<&EditorInputRef>) {
        not_implemented()
    }

    /// `lock(locked)`: whether this editor group should be locked or not.
    fn lock(&self, _locked: bool) {
        not_implemented()
    }

    /// `focus()`: move keyboard focus into the group.
    fn focus(&self) {
        not_implemented()
    }

    /// `createEditorActions(disposables, menuId?)`: create the editor actions for the current active editor.
    fn create_editor_actions(&self, _disposables: &DisposableStore, _menu_id: Option<MenuId>) -> ActiveEditorActions {
        not_implemented()
    }
}

/// `isEditorGroup`: `typeof group.id === 'number' && Array.isArray(group.editors)`. No porte a tipagem já garante as duas
/// formas, então um [`IEditorGroup`] sempre satisfaz o teste.
pub fn is_editor_group(_group: &dyn IEditorGroup) -> bool {
    true
}

/// Os membros do `IEditorGroupsContainer` que o recorte de `IEditorPart` e de `IEditorGroupsService` ainda não tinha.
/// Os dois traits herdam o `IEditorGroupsContainer` no original (o `EditorPart` e o serviço o implementam), e aqui os
/// traits existentes repetem os mesmos membros a partir desta macro, com implementação padrão (os eventos nunca disparam e
/// o resto lança o erro do dublê), para não quebrar quem já os implementa.
macro_rules! editor_groups_container_members {
    () => {
        /// `onDidChangeActiveGroup`: an event for when the active editor group changes.
        fn on_did_change_active_group(&self) -> Event<Rc<dyn IEditorGroup>> {
            Event::none()
        }

        /// `onDidAddGroup`: an event for when a new group was added.
        fn on_did_add_group(&self) -> Event<Rc<dyn IEditorGroup>> {
            Event::none()
        }

        /// `onDidRemoveGroup`: an event for when a group was removed.
        fn on_did_remove_group(&self) -> Event<Rc<dyn IEditorGroup>> {
            Event::none()
        }

        /// `onDidMoveGroup`: an event for when a group was moved.
        fn on_did_move_group(&self) -> Event<Rc<dyn IEditorGroup>> {
            Event::none()
        }

        /// `onDidActivateGroup`: an event for when a group gets activated.
        fn on_did_activate_group(&self) -> Event<EditorGroupActivationEvent> {
            Event::none()
        }

        /// `onDidChangeGroupIndex`: an event for when the index of a group changes.
        fn on_did_change_group_index(&self) -> Event<Rc<dyn IEditorGroup>> {
            Event::none()
        }

        /// `onDidChangeGroupLocked`: an event for when the locked state of a group changes.
        fn on_did_change_group_locked(&self) -> Event<Rc<dyn IEditorGroup>> {
            Event::none()
        }

        /// `onDidChangeGroupMaximized`: an event for when the maximized state of a group changes.
        fn on_did_change_group_maximized(&self) -> Event<bool> {
            Event::none()
        }

        /// `onDidChangeEditorPartOptions`: an event that notifies when container options change.
        fn on_did_change_editor_part_options(&self) -> Event<EditorPartOptionsChangeEvent> {
            Event::none()
        }

        /// `isReady`: whether groups have been created and are ready to be used in the container.
        fn is_ready(&self) -> bool {
            not_implemented()
        }

        /// `whenReady`: resolves when groups have been created and are ready to be used in the container.
        fn when_ready(&self) -> Promise<()> {
            not_implemented()
        }

        /// `whenRestored`: resolves when groups have been restored in the container.
        fn when_restored(&self) -> Promise<()> {
            not_implemented()
        }

        /// `hasRestorableState`: find out if the container has UI state to restore from a previous session.
        fn has_restorable_state(&self) -> bool {
            not_implemented()
        }

        /// `sideGroup`: a subset of methods on a group that is either created to the side or picked if already there.
        fn side_group(&self) -> Rc<dyn IEditorSideGroup> {
            not_implemented()
        }

        /// `count`: the number of editor groups that are currently opened in the container.
        fn count(&self) -> usize {
            self.groups().len()
        }

        /// `orientation`: the current layout orientation of the root group.
        fn orientation(&self) -> GroupOrientation {
            not_implemented()
        }

        /// `partOptions`: access the options of the container.
        fn part_options(&self) -> EditorPartOptions {
            not_implemented()
        }

        /// `enforcePartOptions(options)`: enforce container options temporarily. O `DeepPartial<IEditorPartOptions>` é um
        /// objeto dinâmico (só os campos presentes valem), e o disposable desfaz a imposição.
        fn enforce_part_options(&self, _options: Dynamic) -> DisposableRef {
            not_implemented()
        }

        /// `activateGroup(group)`: set a group as active.
        fn activate_group(&self, _group: GroupRef) -> Rc<dyn IEditorGroup> {
            not_implemented()
        }

        /// `getSize(group)`: returns the size of a group (`width` e `height`).
        fn get_size(&self, _group: GroupRef) -> Dimension {
            not_implemented()
        }

        /// `setSize(group, size)`: sets the size of a group.
        fn set_size(&self, _group: GroupRef, _size: Dimension) {
            not_implemented()
        }

        /// `arrangeGroups(arrangement, target?)`: arrange all groups in the container according to the arrangement.
        fn arrange_groups(&self, _arrangement: GroupsArrangement, _target: Option<GroupRef>) {
            not_implemented()
        }

        /// `toggleMaximizeGroup(group?)`: toggles the target group size to maximize/unmaximize.
        fn toggle_maximize_group(&self, _group: Option<GroupRef>) {
            not_implemented()
        }

        /// `toggleExpandGroup(group?)`: toggles the target group size to expand/distribute even.
        fn toggle_expand_group(&self, _group: Option<GroupRef>) {
            not_implemented()
        }

        /// `applyLayout(layout)`: applies the provided layout by either moving existing groups or creating new groups.
        fn apply_layout(&self, _layout: &EditorGroupLayout) {
            not_implemented()
        }

        /// `getLayout()`: returns an editor layout of the container.
        fn get_layout(&self) -> EditorGroupLayout {
            not_implemented()
        }

        /// `setGroupOrientation(orientation)`: sets the orientation of the root group.
        fn set_group_orientation(&self, _orientation: GroupOrientation) {
            not_implemented()
        }

        /// `findGroup(scope, source, wrap)`: a variante de `find_group` com o grupo de partida e o `wrap`. Sem os dois é
        /// o mesmo método.
        fn find_group_from(&self, scope: &FindGroupScope, source: Option<GroupRef>, wrap: bool) -> Option<Rc<dyn IEditorGroup>> {
            if source.is_some() || wrap {
                not_implemented()
            }
            self.find_group(scope)
        }

        /// `removeGroup(group)`: remove a group from the container.
        fn remove_group(&self, _group: GroupRef) {
            not_implemented()
        }

        /// `moveGroup(group, location, direction)`: move a group to a new group in the container.
        fn move_group(&self, _group: GroupRef, _location: GroupRef, _direction: GroupDirection) -> Rc<dyn IEditorGroup> {
            not_implemented()
        }

        /// `mergeGroup(group, target, options)`: merge the editors of a group into a target group. Returns if merging was
        /// successful.
        fn merge_group(&self, _group: GroupRef, _target: GroupRef, _options: Option<MergeGroupOptions>) -> bool {
            not_implemented()
        }

        /// `mergeAllGroups(target)`: merge all editor groups into the target one. Returns if merging was successful.
        fn merge_all_groups(&self, _target: GroupRef) -> bool {
            not_implemented()
        }

        /// `copyGroup(group, location, direction)`: copy a group to a new group in the container.
        fn copy_group(&self, _group: GroupRef, _location: GroupRef, _direction: GroupDirection) -> Rc<dyn IEditorGroup> {
            not_implemented()
        }

        /// `createEditorDropTarget(container, delegate)`: registers a drag and drop target for editors on the `container`
        /// (o `HTMLElement` do original é opaco aqui).
        fn create_editor_drop_target(&self, _container: Rc<dyn Any>, _delegate: EditorDropTargetDelegate) -> DisposableRef {
            not_implemented()
        }
    };
}

/// `IEditorPart` (via `IEditorGroupsContainer`): os grupos mais o que o `IEditorPart` acrescenta. Os membros do container
/// que o service também declara (`getGroups`, `getGroup`, `findGroup`, `addGroup`) entram aqui com implementação padrão.
pub trait IEditorPart {
    /// `windowId`: the identifier of the window of this part.
    fn window_id(&self) -> WindowId;

    /// `activeGroup`: an active group is the default location for new editors to open.
    fn active_group(&self) -> Rc<dyn IEditorGroup>;

    /// `groups`: all groups that are currently visible in the container.
    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>>;

    /// `getGroups(order)`: get all groups that are currently visible in the container.
    fn get_groups(&self, _order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>> {
        not_implemented()
    }

    /// `getGroup(identifier)`: allows to convert a group identifier to a group.
    fn get_group(&self, _identifier: GroupIdentifier) -> Option<Rc<dyn IEditorGroup>> {
        not_implemented()
    }

    /// `findGroup(scope)`: find a group in a specific scope.
    fn find_group(&self, _scope: &FindGroupScope) -> Option<Rc<dyn IEditorGroup>> {
        not_implemented()
    }

    /// `addGroup(location, direction)`: add a new group to the container by splitting the provided one in the direction.
    fn add_group(&self, _location: Option<GroupRef>, _direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        not_implemented()
    }

    editor_groups_container_members!();

    /// `onDidLayout`: an event for when the editor part is layed out.
    fn on_did_layout(&self) -> Event<Dimension> {
        Event::none()
    }

    /// `onDidScroll`: an event for when the editor part is scrolled.
    fn on_did_scroll(&self) -> Event<()> {
        Event::none()
    }

    /// `onWillDispose`: an event for when the editor part is disposed.
    fn on_will_dispose(&self) -> Event<()> {
        Event::none()
    }

    /// `contentDimension`: the size of the editor part.
    fn content_dimension(&self) -> Dimension {
        not_implemented()
    }

    /// `hasMaximizedGroup()`: find out if an editor group is currently maximized.
    fn has_maximized_group(&self) -> bool {
        not_implemented()
    }

    /// `centerLayout(active)`: enable or disable centered editor layout.
    fn center_layout(&self, _active: bool) {
        not_implemented()
    }

    /// `isLayoutCentered()`: find out if the editor layout is currently centered.
    fn is_layout_centered(&self) -> bool {
        not_implemented()
    }
}

/// `IAuxiliaryEditorPart`.
pub trait IAuxiliaryEditorPart: IEditorPart {
    /// `close()`: close this auxiliary editor part after moving all dirty editors of all groups back to the main editor
    /// part. Returns `false` if an editor could not be moved back.
    fn close(&self) -> bool;
}

/// `IModalEditorPart`.
pub trait IModalEditorPart: IEditorPart {
    /// `close()`: close this modal editor part after having closed all editors, returns whether the part was closed.
    fn close(&self) -> bool;

    /// `close(options)`: a variante com `mergeAllEditorsToMainPart`, que devolve uma promessa. Sem a opção é o mesmo
    /// método.
    fn close_with_options(&self, options: Option<ModalEditorPartCloseOptions>) -> Promise<bool> {
        if options.is_some_and(|options| options.merge_all_editors_to_main_part == Some(true)) {
            not_implemented()
        }
        Promise::resolved(self.close())
    }

    /// `modalElement`: modal container of the editor part (o `HTMLElement` do original é opaco aqui).
    fn modal_element(&self) -> Rc<dyn Any> {
        not_implemented()
    }

    /// `maximized`: whether the modal editor part is currently maximized.
    fn maximized(&self) -> bool {
        not_implemented()
    }

    /// `onDidChangeMaximized`: fired when the maximized state changes.
    fn on_did_change_maximized(&self) -> Event<bool> {
        Event::none()
    }

    /// `toggleMaximized()`: toggle between default and maximized size.
    fn toggle_maximized(&self) {
        not_implemented()
    }

    /// `size`: size set by the user via resizing, if any.
    fn size(&self) -> Option<Dimension> {
        not_implemented()
    }

    /// `position`: position set by the user via dragging, if any.
    fn position(&self) -> Option<EditorPosition> {
        not_implemented()
    }

    /// `hasSidebar`: whether the modal editor part has a sidebar.
    fn has_sidebar(&self) -> bool {
        not_implemented()
    }

    /// `sidebarWidth`: sidebar width set by the user via resizing, if any.
    fn sidebar_width(&self) -> Option<f64> {
        not_implemented()
    }

    /// `sidebarHidden`: whether the sidebar is hidden.
    fn sidebar_hidden(&self) -> bool {
        not_implemented()
    }

    /// `toggleSidebar()`: toggle sidebar visibility.
    fn toggle_sidebar(&self) {
        not_implemented()
    }

    /// `navigation`: the current navigation context, if any.
    fn navigation(&self) -> Option<ModalEditorNavigation> {
        not_implemented()
    }

    /// `updateOptions(options)`: update options for the modal editor part.
    fn update_options(&self, _options: Option<ModalEditorPartOptions>) {
        not_implemented()
    }

    /// `onWillClose`: fired when this modal editor part is about to close.
    fn on_will_close(&self) -> Event<()> {
        Event::none()
    }
}

/// `IEditorGroupsService`, com o que o resolvedor, o `findGroup` e o `columnToEditorGroup` leem e o resto do contrato, com
/// implementação padrão nos membros acrescentados.
pub trait IEditorGroupsService {
    /// `activeGroup`.
    fn active_group(&self) -> Rc<dyn IEditorGroup>;

    /// `groups`: all groups in creation order.
    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>>;

    /// `getGroups(order)`.
    fn get_groups(&self, order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>>;

    /// `getGroup(identifier)`.
    fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<dyn IEditorGroup>>;

    /// `findGroup(scope)`: find a group in a specific scope.
    fn find_group(&self, scope: &FindGroupScope) -> Option<Rc<dyn IEditorGroup>>;

    /// `addGroup(location, direction)`: add a new group to the container by splitting the provided one in the direction.
    fn add_group(&self, location: Option<GroupRef>, direction: GroupDirection) -> Rc<dyn IEditorGroup>;

    /// `mainPart`: the main window part.
    fn main_part(&self) -> Rc<dyn IEditorPart>;

    /// `createAuxiliaryEditorPart(options)`: creates a new auxiliary editor part in a new window.
    fn create_auxiliary_editor_part(&self, options: Option<AuxiliaryEditorOptions>) -> Promise<Rc<dyn IEditorPart>>;

    /// `createModalEditorPart(options)`: creates a modal editor part that shows in a modal overlay.
    fn create_modal_editor_part(&self, options: Option<ModalEditorPartOptions>) -> Promise<Rc<dyn IModalEditorPart>>;

    /// `activeModalEditorPart`: the currently active modal editor part, if any.
    fn active_modal_editor_part(&self) -> Option<Rc<dyn IModalEditorPart>>;

    editor_groups_container_members!();

    /// `onDidCreateAuxiliaryEditorPart`: an event for when a new auxiliary editor part is created.
    fn on_did_create_auxiliary_editor_part(&self) -> Event<Rc<dyn IAuxiliaryEditorPart>> {
        Event::none()
    }

    /// `parts`: provides access to all editor parts.
    fn parts(&self) -> Vec<Rc<dyn IEditorPart>> {
        not_implemented()
    }

    /// `getPart(group)`: get the editor part that contains the group with the provided identifier.
    fn get_part(&self, _group: GroupRef) -> Rc<dyn IEditorPart> {
        not_implemented()
    }

    /// `getPart(container)`: get the editor part that is rooted in the provided container (o `HTMLElement` do original é
    /// opaco aqui).
    fn get_part_by_container(&self, _container: Rc<dyn Any>) -> Rc<dyn IEditorPart> {
        not_implemented()
    }

    /// `getScopedInstantiationService(part)`: the instantiation service that is scoped to the provided editor part.
    fn get_scoped_instantiation_service(&self, _part: &Rc<dyn IEditorPart>) -> Rc<InstantiationService> {
        not_implemented()
    }

    /// `saveWorkingSet(name)`: save a new editor working set from the currently opened editors and group layout.
    fn save_working_set(&self, _name: &str) -> EditorWorkingSet {
        not_implemented()
    }

    /// `getWorkingSets()`: returns all known editor working sets.
    fn get_working_sets(&self) -> Vec<EditorWorkingSet> {
        not_implemented()
    }

    /// `applyWorkingSet(workingSet, options)`: applies the working set. Use [`WorkingSetTarget::Empty`] for an empty one.
    /// Resolves to `true` when the working set was applied.
    fn apply_working_set(&self, _working_set: WorkingSetTarget, _options: Option<EditorWorkingSetOptions>) -> Promise<bool> {
        not_implemented()
    }

    /// `deleteWorkingSet(workingSet)`: deletes a working set.
    fn delete_working_set(&self, _working_set: &EditorWorkingSet) {
        not_implemented()
    }

    /// `registerContextKeyProvider(provider)`: registers a provider that sets a context key for each scoped editor group
    /// context and the global context. The disposable unregisters it.
    fn register_context_key_provider(&self, _provider: EditorGroupContextKeyProvider) -> DisposableRef {
        not_implemented()
    }
}

service_id!(pub I_EDITOR_GROUPS_SERVICE: dyn IEditorGroupsService = "editorGroupsService");

// endregion
