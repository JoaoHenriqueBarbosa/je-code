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
//! - **Contrato reduzido.** Cada trait traz os membros que o porte já lê (o `QueryBuilder`, o `EditorResolverService`, o
//!   `findGroup` e o `columnToEditorGroup`); os demais (layout, ativação, mesclar e copiar grupos, conjuntos de trabalho)
//!   entram com o `EditorGroupsService` inteiro.
//! - **Identidade de grupo.** Onde o original compara grupos com `===` ou `indexOf`, o porte compara o `id` do grupo, que é
//!   único por grupo.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::configuration::service::IConfigurationService;
use je_platform::opener::editor_types::{AuxiliaryEditorOptions, ModalEditorPartOptions};

use crate::services::layout::layout_service::WindowId;
use crate::common::editor::{EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, GroupIdentifier, TextEditorOptions};

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
    fn close_editor(&self, editor: Option<&EditorInputRef>) -> Promise<bool>;

    /// `moveEditor(editor, target)`: move an editor from this group either within this group or to another group.
    /// Returns whether the editor was moved or not.
    fn move_editor(&self, editor: &EditorInputRef, target: &Rc<dyn IEditorGroup>) -> bool;
}

/// `IEditorPart` (via `IEditorGroupsContainer`), reduzido aos grupos.
pub trait IEditorPart {
    /// `windowId`: the identifier of the window of this part.
    fn window_id(&self) -> WindowId;

    /// `activeGroup`: an active group is the default location for new editors to open.
    fn active_group(&self) -> Rc<dyn IEditorGroup>;

    /// `groups`: all groups that are currently visible in the container.
    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>>;
}

/// `IModalEditorPart`, reduzido ao `close`.
pub trait IModalEditorPart: IEditorPart {
    /// `close()`: close this modal editor part after having closed all editors, returns whether the part was closed.
    fn close(&self) -> bool;
}

/// `IEditorGroupsService`, reduzido ao que o resolvedor, o `findGroup` e o `columnToEditorGroup` leem.
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
}

service_id!(pub I_EDITOR_GROUPS_SERVICE: dyn IEditorGroupsService = "editorGroupsService");

// endregion
