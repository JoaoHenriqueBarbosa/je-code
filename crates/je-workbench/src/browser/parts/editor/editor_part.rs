//! Porte de `src/vs/workbench/browser/parts/editor/editorPart.ts`: o [`EditorPart`] lógico (a grade de grupos de editor, a
//! ativação e a ordem de uso recente, os eventos, o layout, o estado persistido), sobre o `SerializableGrid` de
//! `je-widgets` e com o [`EditorGroupView`] mínimo da fatia 2 (ver `PORT_PLAN.md`).
//!
//! Divergências:
//! - **Sem DOM.** A renderização é nativa e vem depois: o `container` é um nó do `je-ui`, o `GridWidgetView` é um
//!   [`GridViewView`] que guarda o grid e o `CenteredViewLayout` o posiciona. Não portados por serem pintura: `updateStyles`,
//!   o drag e drop (`setupDragAndDropSupport`, `createEditorDropTarget`), `setBoundarySashes`, o inset à direita
//!   (`setContentRightInset`), o `layout` flutuante e as classes CSS (`updateContainer`).
//! - **`Part` e `Component`.** O `Part` (título, tamanhos mínimos e máximos, prioridade) e o `IThemeService` não estão portados.
//!   O que o `Component` dá ao `EditorPart` vem direto: o `Memento` e o ouvinte de `onWillSaveState` (que chama
//!   [`EditorPart::save_state`] e grava o memento). O `hasIcons` das opções vem do campo `has_file_icons`
//!   ([`EditorPart::set_has_file_icons`] faz o papel de `onDidFileIconThemeChange`).
//! - **Foco.** Sem foco do DOM, `shouldRestoreFocus` é sempre falso (reparentar nós não rouba foco) e os `preserveFocus` somem
//!   dos métodos. `focus()` do grupo dispara `onDidFocus`, que ativa o grupo (ver `editor_group_view.rs`).
//! - **Uma parte só.** O `IEditorPartsView` (`editorPartsView`) e a `groupsView` de cada grupo chegam com a fatia 4: aqui
//!   `assertGroupView` procura o grupo nesta parte, e `addGroup`, `moveGroup` e `copyGroup` trabalham só no grid desta parte.
//!   `hostService.moveTop` (trazer a janela para a frente) não existe. O `scopedInstantiationService` (filho com o
//!   `IContextKeyService` escopado) é o próprio `InstantiationService`, porque o modelo do grupo só lê a configuração.
//! - **Context keys.** `handleContextKeys` (`editorAreaFocus`, `multipleEditorGroups`, `editorPartMaximizedEditorGroup`,
//!   `editorTabsVisible`, `isTopRightEditorGroup`) depende de `common/contextkeys.ts`, ainda não portado. O provider de
//!   context key e os conjuntos de trabalho (`applyState`; `createState` entra) ficam com o padrão que lança `Method not
//!   implemented.` do contrato.
//! - **`groupsView` dos grupos.** Cada [`EditorGroupView`] recebe o [`EditorGroupsView`] da parte (`PartGroupsView`, que
//!   delega aos métodos privados daqui), que faz o papel do `IEditorGroupsView` e do `IEditorPartsView` do original.
//! - **`sideGroup`.** `findGroup(accessor, { editor, options }, SIDE_GROUP)` roda no `InstantiationService` da parte, que
//!   precisa ter o `IEditorGroupsService` (no original o `scopedInstantiationService` tem o serviço de grupos).
//! - **`whenRestored`.** O original espera o `whenRestored` de cada grupo (a restauração dos editores); aqui `whenRestored`
//!   completa junto com `whenReady` (a restauração dos editores de cada grupo é a promessa [`EditorGroupView::when_restored`]).
//! - **Eventos de tamanho.** `onDidChange`, `onDidChangeSizeConstraints` e `minimumWidth` (o `Part`) não existem; `onDidScroll`
//!   existe. `onDidChangeEditorPartOptions` e a comparação `assert.strictEqual(oldOptions, currentOptions)` do original são
//!   por valor ([`EditorPartOptions`] é `PartialEq`).
//! - **`applyLayout`.** O fallback de orientação (`toGridViewOrientation(layout.orientation, fallback)`) nunca vale, porque
//!   `EditorGroupLayout.orientation` é obrigatório; a orientação do layout é usada direto.
//! - **`contentDimension`** antes do primeiro `layout` é `undefined` no original e `0x0` aqui.
//! - **Estado.** O estado do grid (`IEditorPartUIState`) vai ao memento como objeto dinâmico com a forma do JSON do original
//!   (`serializedGrid`, `activeGroup`, `mostRecentActiveGroups`), e o dado de cada folha vem do [`EditorGroupView::to_json`].
//!   A folha sem dado (`null`/`undefined` no original) é `None`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy};
use je_base::errors::{self, Error, throw};
use je_base::event::{Emitter, Event, PauseableEmitter, Relay, any};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_editor::core_2d::dimension::Dimension;
use je_instantiation::InstantiationService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{IContextKeyService, IScopedContextKeyService};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_ui::{NodeId, Role, Ui};
use je_widgets::centered_view_layout::{CenteredViewLayout, CenteredViewState};
use je_widgets::grid::{
    Direction, GridDescriptor, GridNodeDescriptor, GridOptions, Orientation, SerializableGrid, SerializedGrid, Sizing, create_serialized_grid,
};
use je_widgets::gridview::{GridNode, GridViewRef, GridViewView, SerializedNode, ViewDimensions};
use je_widgets::sash::BoundarySashes;

use crate::browser::parts::editor::editor::{EditorPartCreationOptions, assign_part_options, get_editor_part_options, impacts_editor_part_options};
use crate::browser::parts::editor::editor_group_view::{ContextKeyTarget, EditorGroupSource, EditorGroupView, EditorGroupsView};
use crate::common::editor::editor_group_model::{
    EditorOrIndex, SerializedEditorGroupModel, SerializedEditorInput, is_serialized_editor_group_model,
};
use crate::common::editor::{EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, GroupIdentifier, GroupModelChangeKind, IEditorPane, TextEditorOptions};
use crate::common::memento::Memento;
use crate::services::editor::editor_group_finder::{FindGroupEditor, find_group};
use crate::services::editor::editor_groups_service::{
    EditorGroupActivationEvent, EditorGroupLayout, EditorPartOptions, EditorPartOptionsChangeEvent, FindGroupScope, GroupActivationReason, GroupDirection,
    GroupLayoutArgument, GroupLocation, GroupOrientation, GroupRef, GroupsArrangement, GroupsOrder, IEditorGroup, IEditorPart, IEditorSideGroup, MergeGroupMode,
    MergeGroupOptions, PreferredGroup, SplitSizing,
};
use crate::services::layout::layout_service::WindowId;
use je_platform::opener::editor_types::EditorOptions;

const EDITOR_PART_UI_STATE_STORAGE_KEY: &str = "editorpart.state";
const EDITOR_PART_CENTERED_VIEW_STORAGE_KEY: &str = "editorpart.centeredview";

/// O dado de cada folha do grid serializado: o grupo serializado, ou nada (a folha de um layout ainda sem grupo).
pub type SerializedEditorGrid = SerializedGrid<Option<SerializedEditorGroupModel>>;

/// `IEditorPartUIState`.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorPartUiState {
    pub serialized_grid: SerializedEditorGrid,
    pub active_group: GroupIdentifier,
    pub most_recent_active_groups: Vec<GroupIdentifier>,
}

/// `GridWidgetView<T>`: a vista que guarda o grid do `EditorPart` (o `CenteredViewLayout` a posiciona).
struct GridWidgetView {
    ui: Ui,
    element: NodeId,
    grid_widget: RefCell<Option<Rc<SerializableGrid<EditorGroupView>>>>,
    on_did_change: Rc<Relay<Option<ViewDimensions>>>,
}

impl GridWidgetView {
    fn new(ui: &Ui) -> Rc<Self> {
        let element = ui.tree_mut().create_detached(Role::Generic);

        Rc::new(Self { ui: ui.clone(), element, grid_widget: RefCell::new(None), on_did_change: Relay::new() })
    }

    /// `set gridWidget(grid)`.
    fn set_grid_widget(&self, grid: Option<Rc<SerializableGrid<EditorGroupView>>>) {
        {
            let mut tree = self.ui.tree_mut();
            for child in tree.children(self.element).to_vec() {
                tree.detach(child);
            }

            if let Some(grid) = &grid {
                tree.append_child(self.element, grid.element());
            }
        }

        self.on_did_change.set_input(grid.as_ref().map_or_else(Event::none, |grid| grid.on_did_change()));
        *self.grid_widget.borrow_mut() = grid;
    }

    fn grid(&self) -> Option<Rc<SerializableGrid<EditorGroupView>>> {
        self.grid_widget.borrow().clone()
    }
}

impl GridViewView for GridWidgetView {
    fn element(&self) -> NodeId {
        self.element
    }

    fn minimum_width(&self) -> f64 {
        self.grid().map_or(0.0, |grid| grid.minimum_width())
    }

    fn maximum_width(&self) -> f64 {
        self.grid().map_or(f64::INFINITY, |grid| grid.maximum_width())
    }

    fn minimum_height(&self) -> f64 {
        self.grid().map_or(0.0, |grid| grid.minimum_height())
    }

    fn maximum_height(&self) -> f64 {
        self.grid().map_or(f64::INFINITY, |grid| grid.maximum_height())
    }

    fn on_did_change(&self) -> Event<Option<ViewDimensions>> {
        self.on_did_change.event()
    }

    fn layout(&self, width: f64, height: f64, top: f64, left: f64) {
        if let Some(grid) = self.grid() {
            grid.layout(width, height, top, left);
        }
    }
}

impl IDisposable for GridWidgetView {
    fn dispose(&self) {
        self.on_did_change.dispose();
    }
}

/// Os campos mutáveis da parte. Nenhum método segura o empréstimo ao chamar código externo ou disparar evento.
struct PartState {
    /// `groupViews`: na ordem de criação (o `Map` do JavaScript guarda a ordem de inserção).
    group_views: Vec<Rc<EditorGroupView>>,
    most_recent_active_groups: Vec<GroupIdentifier>,
    active_group: Option<Rc<EditorGroupView>>,
    grid_widget: Option<Rc<SerializableGrid<EditorGroupView>>>,
    centered_layout_widget: Option<Rc<CenteredViewLayout>>,
    /// `enforcedPartOptions`, com um número que identifica cada um (o `indexOf` do original compara o objeto).
    enforced_part_options: Vec<(u64, Dynamic)>,
    next_enforcement_id: u64,
    part_options: EditorPartOptions,
    top: f64,
    left: f64,
    content_dimension: Option<Dimension>,
    is_ready: bool,
    will_restore_state: bool,
    disposed: bool,
}

/// `EditorPart`.
pub struct EditorPart {
    this: Weak<EditorPart>,
    ui: Ui,
    id: String,
    groups_label: String,
    window_id: WindowId,
    instantiation_service: Rc<InstantiationService>,
    configuration_service: Rc<dyn IConfigurationService>,
    storage_service: Rc<dyn StorageService>,
    memento: Memento,
    container: NodeId,
    scoped_context_key_service: Rc<dyn IScopedContextKeyService>,
    grid_widget_view: Rc<GridWidgetView>,
    /// O [`EditorGroupsView`] que cada grupo recebe (em `Weak`): a parte vista pelos grupos.
    groups_view: Rc<PartGroupsView>,
    has_file_icons: Cell<bool>,
    state: RefCell<PartState>,
    store: Rc<DisposableStore>,
    grid_widget_disposables: Rc<DisposableStore>,

    on_did_focus: Rc<Emitter<()>>,
    on_did_layout: Rc<Emitter<Dimension>>,
    on_did_change_active_group: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_did_change_group_index: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_did_change_group_label: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_did_change_group_locked: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_did_change_group_maximized: Rc<Emitter<bool>>,
    on_did_activate_group: Rc<Emitter<EditorGroupActivationEvent>>,
    on_did_add_group: Rc<PauseableEmitter<Rc<dyn IEditorGroup>>>,
    on_did_remove_group: Rc<PauseableEmitter<Rc<dyn IEditorGroup>>>,
    on_did_move_group: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_did_set_grid_widget: Rc<Emitter<()>>,
    on_did_scroll: Rc<Relay<()>>,
    on_did_change_editor_part_options: Rc<Emitter<EditorPartOptionsChangeEvent>>,
    on_will_dispose: Rc<Emitter<()>>,
    when_ready_promise: DeferredPromise<()>,
    when_restored_promise: DeferredPromise<()>,
}

fn invalid_group() -> ! {
    throw(Error::new("Invalid editor group provided!"))
}

/// O [`EditorGroupsView`] da parte: delega aos métodos privados do [`EditorPart`] (o `IEditorGroupsView` e o
/// `IEditorPartsView` do original, que o `EditorPart` implementa direto).
struct PartGroupsView {
    part: Weak<EditorPart>,
}

impl PartGroupsView {
    fn part(&self) -> Rc<EditorPart> {
        self.part.upgrade().expect("a parte está viva enquanto os grupos dela são usados")
    }
}

impl EditorGroupsView for PartGroupsView {
    fn part_options(&self) -> EditorPartOptions {
        self.part().state.borrow().part_options.clone()
    }

    fn on_did_change_editor_part_options(&self) -> Event<EditorPartOptionsChangeEvent> {
        self.part().on_did_change_editor_part_options.event()
    }

    fn active_group(&self) -> Rc<EditorGroupView> {
        self.part().active_view()
    }

    fn activate_group(&self, group: &Rc<EditorGroupView>) {
        self.part().activate_group_with_reason(&GroupRef::Group(group.as_group()), GroupActivationReason::Default);
    }

    fn restore_group(&self, group: &Rc<EditorGroupView>) {
        self.part().restore_group(&GroupRef::Group(group.as_group()));
    }

    fn remove_group(&self, group: &Rc<EditorGroupView>) {
        self.part().remove_group_view(group);
    }

    fn get_groups(&self, order: GroupsOrder) -> Vec<Rc<EditorGroupView>> {
        self.part().get_group_views(order)
    }

    fn groups(&self) -> Vec<Rc<EditorGroupView>> {
        self.part().group_views()
    }

    fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<EditorGroupView>> {
        self.part().get_group_view(identifier)
    }
}

/// `sideGroup: IEditorSideGroup`.
struct PartSideGroup {
    part: Weak<EditorPart>,
}

impl IEditorSideGroup for PartSideGroup {
    fn open_editor(&self, editor: &EditorInputRef, options: Option<EditorOptions>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        let part = self.part.upgrade().expect("a parte está viva enquanto o grupo ao lado é usado");
        let editor = editor.clone();

        async_fn(async move {
            let find_group_editor = FindGroupEditor::WithOptions(EditorInputWithOptions { editor: editor.clone(), options: options.clone().map(TextEditorOptions::from) });
            let find_group_result = part.instantiation_service.invoke_function(|accessor| find_group(accessor, &find_group_editor, Some(&PreferredGroup::Side)));
            let group = match find_group_result {
                MaybePromise::Value((group, _)) => group,
                MaybePromise::Promise(promise) => promise.await?.0,
            };

            group.open_editor(&editor, options).await
        })
    }
}

fn to_grid_direction(direction: GroupDirection) -> Direction {
    match direction {
        GroupDirection::Up => Direction::Up,
        GroupDirection::Down => Direction::Down,
        GroupDirection::Left => Direction::Left,
        GroupDirection::Right => Direction::Right,
    }
}

fn to_grid_orientation(orientation: GroupOrientation) -> Orientation {
    match orientation {
        GroupOrientation::Horizontal => Orientation::Horizontal,
        GroupOrientation::Vertical => Orientation::Vertical,
    }
}

fn to_group_orientation(orientation: Orientation) -> GroupOrientation {
    match orientation {
        Orientation::Horizontal => GroupOrientation::Horizontal,
        Orientation::Vertical => GroupOrientation::Vertical,
    }
}

/// `groups[index]` do JavaScript: um índice negativo ou fora da lista é `undefined`.
fn group_at(groups: &[Rc<EditorGroupView>], index: i64) -> Option<Rc<EditorGroupView>> {
    usize::try_from(index).ok().and_then(|index| groups.get(index)).cloned()
}

fn same_group(a: &Rc<EditorGroupView>, b: &Rc<EditorGroupView>) -> bool {
    a.id() == b.id()
}

/// `fillGridNodes`.
fn fill_grid_nodes(target: &mut Vec<Rc<EditorGroupView>>, node: &GridNode<Rc<EditorGroupView>>) {
    match node {
        GridNode::Branch(branch) => {
            for child in &branch.children {
                fill_grid_nodes(target, child);
            }
        }
        GridNode::Leaf(leaf) => target.push(leaf.view.clone()),
    }
}

/// `GroupLayoutArgument` como o nó do descritor que o `createSerializedGrid` lê (`Array.isArray(groups)` é um ramo).
fn layout_argument_to_descriptor(argument: &GroupLayoutArgument) -> GridNodeDescriptor<Option<SerializedEditorGroupModel>> {
    match &argument.groups {
        Some(groups) => GridNodeDescriptor::Branch { size: argument.size, groups: groups.iter().map(layout_argument_to_descriptor).collect() },
        None => GridNodeDescriptor::Leaf { size: argument.size, data: None },
    }
}

/// `serializedNodeToGroupLayoutArgument`.
fn serialized_node_to_layout_argument<D>(node: &SerializedNode<D>) -> GroupLayoutArgument {
    match node {
        SerializedNode::Branch { data, size, .. } => GroupLayoutArgument { size: *size, groups: Some(data.iter().map(serialized_node_to_layout_argument).collect()) },
        SerializedNode::Leaf { size, .. } => GroupLayoutArgument { size: *size, groups: None },
    }
}

/// `countGroups` do `applyLayout`: as folhas do layout.
fn count_layout_groups(groups: &[GroupLayoutArgument]) -> usize {
    groups.iter().map(|group| group.groups.as_deref().map_or(1, count_layout_groups)).sum()
}

// region: o estado como objeto dinâmico (o JSON que o original grava no memento)

/// `value[key]` de um objeto, `undefined` para o que não é objeto.
fn prop(value: &Dynamic, key: &str) -> Dynamic {
    match value {
        Dynamic::Object(_) => value.get_str(key),
        _ => Dynamic::Undefined,
    }
}

fn number_of(value: &Dynamic) -> Option<f64> {
    match value {
        Dynamic::Number(number) => Some(*number),
        _ => None,
    }
}

fn bool_of(value: &Dynamic) -> Option<bool> {
    match value {
        Dynamic::Bool(flag) => Some(*flag),
        _ => None,
    }
}

fn text_of(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn index_of_number(value: &Dynamic) -> Option<usize> {
    number_of(value).filter(|number| *number >= 0.0 && number.fract() == 0.0).map(|number| number as usize)
}

fn push_if<'a>(entries: &mut Vec<(&'a str, Dynamic)>, key: &'a str, value: Option<Dynamic>) {
    if let Some(value) = value {
        entries.push((key, value));
    }
}

fn group_model_to_dynamic(model: &SerializedEditorGroupModel) -> Dynamic {
    let mut entries = Vec::new();
    push_if(&mut entries, "id", model.id.map(|id| Dynamic::Number(id as f64)));
    push_if(&mut entries, "locked", model.locked.map(Dynamic::Bool));
    entries.push((
        "editors",
        Dynamic::array(model.editors.iter().map(|editor| Dynamic::object([("id", Dynamic::string(&editor.id)), ("value", Dynamic::string(&editor.value))]))),
    ));
    entries.push(("mru", Dynamic::array(model.mru.iter().map(|index| Dynamic::Number(*index as f64)))));
    push_if(&mut entries, "preview", model.preview.map(|index| Dynamic::Number(index as f64)));
    push_if(&mut entries, "sticky", model.sticky.map(|index| Dynamic::Number(index as f64)));

    Dynamic::object(entries)
}

fn group_model_from_dynamic(value: &Dynamic) -> SerializedEditorGroupModel {
    let list = |key: &str| match prop(value, key) {
        Dynamic::Array(items) => items.to_vec(),
        _ => Vec::new(),
    };

    SerializedEditorGroupModel {
        id: number_of(&prop(value, "id")).map(|id| id as GroupIdentifier),
        locked: bool_of(&prop(value, "locked")),
        editors: list("editors")
            .iter()
            .filter_map(|editor| Some(SerializedEditorInput { id: text_of(&prop(editor, "id"))?, value: text_of(&prop(editor, "value"))? }))
            .collect(),
        mru: list("mru").iter().filter_map(index_of_number).collect(),
        preview: index_of_number(&prop(value, "preview")),
        sticky: index_of_number(&prop(value, "sticky")),
    }
}

fn node_to_dynamic(node: &SerializedNode<Option<SerializedEditorGroupModel>>) -> Dynamic {
    let mut entries = Vec::new();

    match node {
        SerializedNode::Leaf { data, size, visible, maximized } => {
            entries.push(("type", Dynamic::string("leaf")));
            entries.push(("data", data.as_ref().map_or(Dynamic::Null, group_model_to_dynamic)));
            push_if(&mut entries, "size", size.map(Dynamic::Number));
            push_if(&mut entries, "visible", visible.map(Dynamic::Bool));
            push_if(&mut entries, "maximized", maximized.map(Dynamic::Bool));
        }
        SerializedNode::Branch { data, size, visible } => {
            entries.push(("type", Dynamic::string("branch")));
            entries.push(("data", Dynamic::array(data.iter().map(node_to_dynamic))));
            push_if(&mut entries, "size", size.map(Dynamic::Number));
            push_if(&mut entries, "visible", visible.map(Dynamic::Bool));
        }
    }

    Dynamic::object(entries)
}

fn node_from_dynamic(value: &Dynamic) -> Option<SerializedNode<Option<SerializedEditorGroupModel>>> {
    let size = number_of(&prop(value, "size"));
    let visible = bool_of(&prop(value, "visible"));

    match text_of(&prop(value, "type"))?.as_str() {
        "branch" => {
            let Dynamic::Array(children) = prop(value, "data") else { return None };
            let data = children.to_vec().iter().map(node_from_dynamic).collect::<Option<Vec<_>>>()?;

            Some(SerializedNode::Branch { data, size, visible })
        }
        "leaf" => {
            let data = prop(value, "data");
            let group = is_serialized_editor_group_model(&data).then(|| group_model_from_dynamic(&data));

            Some(SerializedNode::Leaf { data: group, size, visible, maximized: bool_of(&prop(value, "maximized")) })
        }
        _ => None,
    }
}

/// `ISerializedGrid`: a orientação é o número do enum do original (`VERTICAL` é 0 e `HORIZONTAL` é 1).
fn grid_to_dynamic(grid: &SerializedEditorGrid) -> Dynamic {
    Dynamic::object([
        ("root", node_to_dynamic(&grid.root)),
        ("orientation", Dynamic::Number(if grid.orientation == Orientation::Vertical { 0.0 } else { 1.0 })),
        ("width", Dynamic::Number(grid.width)),
        ("height", Dynamic::Number(grid.height)),
    ])
}

fn grid_from_dynamic(value: &Dynamic) -> Option<SerializedEditorGrid> {
    let orientation = if number_of(&prop(value, "orientation"))? == 0.0 { Orientation::Vertical } else { Orientation::Horizontal };

    Some(SerializedGrid {
        root: node_from_dynamic(&prop(value, "root"))?,
        orientation,
        width: number_of(&prop(value, "width"))?,
        height: number_of(&prop(value, "height"))?,
    })
}

fn state_to_dynamic(state: &EditorPartUiState) -> Dynamic {
    Dynamic::object([
        ("serializedGrid", grid_to_dynamic(&state.serialized_grid)),
        ("activeGroup", Dynamic::Number(state.active_group as f64)),
        ("mostRecentActiveGroups", Dynamic::array(state.most_recent_active_groups.iter().map(|id| Dynamic::Number(*id as f64)))),
    ])
}

fn state_from_dynamic(value: &Dynamic) -> Option<EditorPartUiState> {
    let Dynamic::Array(most_recent_active_groups) = prop(value, "mostRecentActiveGroups") else { return None };

    Some(EditorPartUiState {
        serialized_grid: grid_from_dynamic(&prop(value, "serializedGrid"))?,
        active_group: number_of(&prop(value, "activeGroup"))? as GroupIdentifier,
        most_recent_active_groups: most_recent_active_groups.to_vec().iter().filter_map(number_of).map(|id| id as GroupIdentifier).collect(),
    })
}

fn centered_state_to_dynamic(state: &CenteredViewState) -> Dynamic {
    Dynamic::object([
        ("targetWidth", Dynamic::Number(state.target_width)),
        ("leftMarginRatio", Dynamic::Number(state.left_margin_ratio)),
        ("rightMarginRatio", Dynamic::Number(state.right_margin_ratio)),
    ])
}

fn centered_state_from_dynamic(value: &Dynamic) -> Option<CenteredViewState> {
    Some(CenteredViewState {
        target_width: number_of(&prop(value, "targetWidth"))?,
        left_margin_ratio: number_of(&prop(value, "leftMarginRatio"))?,
        right_margin_ratio: number_of(&prop(value, "rightMarginRatio"))?,
    })
}

// endregion

impl EditorPart {
    /// `new EditorPart(editorPartsView, id, groupsLabel, windowId, ...)`: `id` é o do memento (`Parts.EDITOR_PART`,
    /// `workbench.parts.editor`, na parte principal).
    pub fn new(
        ui: &Ui,
        instantiation_service: Rc<InstantiationService>,
        configuration_service: Rc<dyn IConfigurationService>,
        storage_service: Rc<dyn StorageService>,
        context_key_service: &dyn IContextKeyService,
        id: &str,
        groups_label: &str,
        window_id: WindowId,
    ) -> Rc<Self> {
        let store = DisposableStore::new();

        let container = ui.tree_mut().create_detached(Role::Generic);
        let scoped_context_key_service = context_key_service.create_scoped(ContextKeyTarget::new());
        store.add(scoped_context_key_service.clone());

        let grid_widget_view = GridWidgetView::new(ui);
        store.add_rc(grid_widget_view.clone());

        let part_options = get_editor_part_options(configuration_service.as_ref(), true);

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            ui: ui.clone(),
            id: id.to_string(),
            groups_label: groups_label.to_string(),
            window_id,
            instantiation_service,
            configuration_service,
            memento: Memento::new(id, storage_service.clone()),
            storage_service,
            container,
            scoped_context_key_service,
            grid_widget_view,
            groups_view: Rc::new(PartGroupsView { part: this.clone() }),
            has_file_icons: Cell::new(true),
            state: RefCell::new(PartState {
                group_views: Vec::new(),
                most_recent_active_groups: Vec::new(),
                active_group: None,
                grid_widget: None,
                centered_layout_widget: None,
                enforced_part_options: Vec::new(),
                next_enforcement_id: 0,
                part_options,
                top: 0.0,
                left: 0.0,
                content_dimension: None,
                is_ready: false,
                will_restore_state: false,
                disposed: false,
            }),
            on_did_focus: store.add_rc(Emitter::new()),
            on_did_layout: store.add_rc(Emitter::new()),
            on_did_change_active_group: store.add_rc(Emitter::new()),
            on_did_change_group_index: store.add_rc(Emitter::new()),
            on_did_change_group_label: store.add_rc(Emitter::new()),
            on_did_change_group_locked: store.add_rc(Emitter::new()),
            on_did_change_group_maximized: store.add_rc(Emitter::new()),
            on_did_activate_group: store.add_rc(Emitter::new()),
            on_did_add_group: store.add_rc(PauseableEmitter::new()),
            on_did_remove_group: store.add_rc(PauseableEmitter::new()),
            on_did_move_group: store.add_rc(Emitter::new()),
            on_did_set_grid_widget: store.add_rc(Emitter::new()),
            on_did_scroll: store.add_rc(Relay::new()),
            on_did_change_editor_part_options: store.add_rc(Emitter::new()),
            on_will_dispose: store.add_rc(Emitter::new()),
            when_ready_promise: DeferredPromise::new(),
            when_restored_promise: DeferredPromise::new(),
            store,
            grid_widget_disposables: DisposableStore::new(),
        });

        this.register_listeners();

        this
    }

    fn register_listeners(&self) {
        let weak = self.this.clone();
        self.store.add(self.configuration_service.on_did_change_configuration().listen(move |event| {
            if let Some(part) = weak.upgrade()
                && impacts_editor_part_options(event)
            {
                part.handle_changed_part_options();
            }
        }));

        // `Component`: ask the part to persist state into the memento, then save the memento into storage
        let weak = self.this.clone();
        self.store.add(self.storage_service.on_will_save_state().listen(move |_| {
            if let Some(part) = weak.upgrade() {
                part.save_state();
                part.memento.save_memento();
            }
        }));
    }

    /// `getId()`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// `themeService.getFileIconTheme().hasFileIcons` mudou (`onDidFileIconThemeChange`).
    pub fn set_has_file_icons(&self, has_file_icons: bool) {
        self.has_file_icons.set(has_file_icons);
        self.handle_changed_part_options();
    }

    fn handle_changed_part_options(&self) {
        let old_part_options = self.state.borrow().part_options.clone();
        let mut new_part_options = get_editor_part_options(self.configuration_service.as_ref(), self.has_file_icons.get());

        let enforced_part_options: Vec<Dynamic> = self.state.borrow().enforced_part_options.iter().map(|(_, options)| options.clone()).collect();
        for enforced in &enforced_part_options {
            assign_part_options(&mut new_part_options, enforced); // check for overrides
        }

        self.state.borrow_mut().part_options = new_part_options.clone();

        self.on_did_change_editor_part_options.fire(EditorPartOptionsChangeEvent { old_part_options, new_part_options });
    }

    // region: acesso a grupos e à grade

    fn group_views(&self) -> Vec<Rc<EditorGroupView>> {
        self.state.borrow().group_views.clone()
    }

    fn group_count(&self) -> usize {
        self.state.borrow().group_views.len()
    }

    fn grid_widget(&self) -> Option<Rc<SerializableGrid<EditorGroupView>>> {
        self.state.borrow().grid_widget.clone()
    }

    /// `this.gridWidget` onde o original o assume criado.
    fn grid(&self) -> Rc<SerializableGrid<EditorGroupView>> {
        self.grid_widget().unwrap_or_else(|| throw(Error::new("The editor part has no grid widget yet")))
    }

    fn centered_layout_widget(&self) -> Rc<CenteredViewLayout> {
        self.state.borrow().centered_layout_widget.clone().unwrap_or_else(|| throw(Error::new("The editor part has not been created yet")))
    }

    fn active_view(&self) -> Rc<EditorGroupView> {
        self.state.borrow().active_group.clone().unwrap_or_else(|| throw(Error::new("The editor part has no active group yet")))
    }

    fn get_group_view(&self, identifier: GroupIdentifier) -> Option<Rc<EditorGroupView>> {
        self.state.borrow().group_views.iter().find(|group| group.id() == identifier).cloned()
    }

    /// `assertGroupView(group)`.
    fn assert_group_view(&self, group: &GroupRef) -> Rc<EditorGroupView> {
        match group {
            GroupRef::Id(identifier) => self.get_group_view(*identifier).unwrap_or_else(|| invalid_group()),
            // O original devolve o próprio objeto (mesmo um grupo já removido); um dublê sem vista concreta é buscado pelo id.
            GroupRef::Group(group) => match group.clone().as_any_rc().and_then(|any| any.downcast::<EditorGroupView>().ok()) {
                Some(view) => view,
                None => self.get_group_view(group.id()).unwrap_or_else(|| invalid_group()),
            },
        }
    }

    /// `hasGroup(identifier)`.
    pub fn has_group(&self, identifier: GroupIdentifier) -> bool {
        self.get_group_view(identifier).is_some()
    }

    /// `getGroups(order)` com as vistas de grupo.
    pub fn get_group_views(&self, order: GroupsOrder) -> Vec<Rc<EditorGroupView>> {
        match order {
            GroupsOrder::CreationTime => self.group_views(),
            GroupsOrder::MostRecentlyActive => {
                let most_recent_active: Vec<GroupIdentifier> = self.state.borrow().most_recent_active_groups.clone();
                let mut groups: Vec<Rc<EditorGroupView>> = most_recent_active.iter().filter_map(|id| self.get_group_view(*id)).collect();

                // there can be groups that got never active, even though they exist. in this case
                // make sure to just append them at the end so that all groups are returned properly
                for group in self.group_views() {
                    if !groups.iter().any(|candidate| same_group(candidate, &group)) {
                        groups.push(group);
                    }
                }

                groups
            }
            GroupsOrder::GridAppearance => {
                let mut views = Vec::new();
                if let Some(grid) = self.grid_widget() {
                    for child in &grid.get_views().children {
                        fill_grid_nodes(&mut views, child);
                    }
                }

                views
            }
        }
    }

    // endregion

    // region: busca

    /// `findGroup(scope, source = this.activeGroup, wrap)`.
    fn do_find_group(&self, scope: &FindGroupScope, source: Option<GroupRef>, wrap: bool) -> Option<Rc<EditorGroupView>> {
        let source = match source {
            Some(source) => self.assert_group_view(&source),
            None => self.active_view(),
        };

        // by direction
        if let Some(direction) = scope.direction {
            return self.do_find_group_by_direction(direction, &source, wrap);
        }

        // by location
        if let Some(location) = scope.location {
            return self.do_find_group_by_location(location, &source, wrap);
        }

        throw(Error::new("invalid arguments"))
    }

    fn do_find_group_by_direction(&self, direction: GroupDirection, source: &Rc<EditorGroupView>, wrap: bool) -> Option<Rc<EditorGroupView>> {
        // Find neighbours and sort by our MRU list
        let mut neighbours = self.grid().get_neighbor_views(source, to_grid_direction(direction), wrap);
        let most_recent_active = self.state.borrow().most_recent_active_groups.clone();
        neighbours.sort_by_key(|neighbour| most_recent_active.iter().position(|id| *id == neighbour.id()).map_or(-1, |index| index as i64));

        neighbours.into_iter().next()
    }

    fn do_find_group_by_location(&self, location: GroupLocation, source: &Rc<EditorGroupView>, wrap: bool) -> Option<Rc<EditorGroupView>> {
        let groups = self.get_group_views(GroupsOrder::GridAppearance);
        let index = groups.iter().position(|group| same_group(group, source)).map_or(-1, |index| index as i64);

        match location {
            GroupLocation::First => groups.first().cloned(),
            GroupLocation::Last => groups.last().cloned(),
            GroupLocation::Next => {
                let mut next_group = group_at(&groups, index + 1);
                if next_group.is_none() && wrap {
                    next_group = self.do_find_group_by_location(GroupLocation::First, source, false);
                }

                next_group
            }
            GroupLocation::Previous => {
                let mut previous_group = group_at(&groups, index - 1);
                if previous_group.is_none() && wrap {
                    previous_group = self.do_find_group_by_location(GroupLocation::Last, source, false);
                }

                previous_group
            }
        }
    }

    // endregion

    // region: ativação

    /// `activateGroup(group, preserveWindowOrder, reason)`: sem janela do DOM, não há ordem de janelas para preservar.
    pub fn activate_group_with_reason(&self, group: &GroupRef, reason: GroupActivationReason) -> Rc<EditorGroupView> {
        let group_view = self.assert_group_view(group);
        self.do_set_group_active(&group_view, reason);

        group_view
    }

    /// `restoreGroup(group)`.
    pub fn restore_group(&self, group: &GroupRef) -> Rc<EditorGroupView> {
        let group_view = self.assert_group_view(group);
        self.do_restore_group(&group_view);

        group_view
    }

    fn do_set_group_active(&self, group: &Rc<EditorGroupView>, reason: GroupActivationReason) {
        let previous_active_group = self.state.borrow().active_group.clone();

        if previous_active_group.as_ref().is_none_or(|previous| !same_group(previous, group)) {
            self.state.borrow_mut().active_group = Some(group.clone());

            // Update list of most recently active groups
            self.do_update_most_recent_active(group.id(), true);

            // Mark previous one as inactive
            if let Some(previous) = &previous_active_group
                && !previous.disposed()
            {
                previous.set_active(false);
            }

            // Mark group as new active
            group.set_active(true);

            // Expand the group if it is currently minimized
            self.do_restore_group(group);

            // Event
            self.on_did_change_active_group.fire(group.as_group());
        }

        // Always fire the event that a group has been activated even if its the same group that is already active to
        // signal the intent even when nothing has changed.
        self.on_did_activate_group.fire(EditorGroupActivationEvent { group: group.as_group(), reason });
    }

    fn do_restore_group(&self, group: &Rc<EditorGroupView>) {
        let Some(grid) = self.grid_widget() else {
            return; // method is called as part of state restore very early
        };

        // ignore: method might be called too early before view is known to grid
        let _ = errors::catch(|| {
            if grid.has_maximized_view() && !grid.is_view_maximized(group) {
                self.unmaximize_group();
            }

            let view_size = grid.get_view_size(Some(group));
            if view_size.width == group.minimum_width() || view_size.height == group.minimum_height() {
                self.arrange_groups_of(GroupsArrangement::Expand, Some(group.clone()));
            }
        });
    }

    fn do_update_most_recent_active(&self, group_id: GroupIdentifier, make_most_recently_active: bool) {
        let mut state = self.state.borrow_mut();

        // Remove from MRU list
        state.most_recent_active_groups.retain(|id| *id != group_id);

        // Add to front as needed
        if make_most_recently_active {
            state.most_recent_active_groups.insert(0, group_id);
        }
    }

    // endregion

    // region: tamanho e arranjo

    fn get_split_sizing_style(&self) -> Sizing {
        match self.state.borrow().part_options.split_sizing {
            SplitSizing::Distribute => Sizing::Distribute,
            SplitSizing::Split => Sizing::Split,
            SplitSizing::Auto => Sizing::Auto,
        }
    }

    /// `arrangeGroups(arrangement, target = this.activeGroup)`.
    fn arrange_groups_of(&self, arrangement: GroupsArrangement, target: Option<Rc<EditorGroupView>>) {
        if self.group_count() < 2 {
            return; // require at least 2 groups to show
        }

        let Some(grid) = self.grid_widget() else {
            return; // we have not been created yet
        };

        let group_view = target.unwrap_or_else(|| self.active_view());

        match arrangement {
            GroupsArrangement::Even => grid.distribute_view_sizes(),
            GroupsArrangement::Maximize => {
                grid.maximize_view(&group_view, &[]);
                group_view.focus();
            }
            GroupsArrangement::Expand => grid.expand_view(&group_view),
        }
    }

    fn unmaximize_group(&self) {
        self.grid().exit_maximized_view();
        self.active_view().focus(); // When making views visible the focus can be affected, so restore it
    }

    /// `isGroupExpanded(targetGroup)`.
    pub fn is_group_expanded(&self, target_group: &GroupRef) -> bool {
        self.grid().is_view_expanded(&self.assert_group_view(target_group))
    }

    // endregion

    // region: criação de grupos

    /// `doCreateGroupView(from)`.
    fn do_create_group_view(&self, from: EditorGroupSource) -> Rc<EditorGroupView> {
        // Create group view
        let groups_view_rc: Rc<dyn EditorGroupsView> = self.groups_view.clone();
        let groups_view: Weak<dyn EditorGroupsView> = Rc::downgrade(&groups_view_rc);
        let group_view = EditorGroupView::new(
            from,
            &self.ui,
            &self.instantiation_service,
            self.scoped_context_key_service.as_ref(),
            groups_view,
            self.window_id,
            &self.groups_label,
            self.group_count(),
        );

        // Keep in map
        self.state.borrow_mut().group_views.push(group_view.clone());

        // Track focus
        let group_disposables = DisposableStore::new();
        let (weak_part, weak_group) = (self.this.clone(), Rc::downgrade(&group_view));
        group_disposables.add(group_view.on_did_focus().listen({
            let (weak_part, weak_group) = (weak_part.clone(), weak_group.clone());
            move |_| {
                if let (Some(part), Some(group)) = (weak_part.upgrade(), weak_group.upgrade()) {
                    part.do_set_group_active(&group, GroupActivationReason::Default);

                    part.on_did_focus.fire(());
                }
            }
        }));

        // Track group changes
        group_disposables.add(group_view.on_did_model_change().listen({
            let (weak_part, weak_group) = (weak_part.clone(), weak_group.clone());
            move |event| {
                let (Some(part), Some(group)) = (weak_part.upgrade(), weak_group.upgrade()) else { return };

                match event.kind {
                    GroupModelChangeKind::GroupLocked => part.on_did_change_group_locked.fire(group.as_group()),
                    GroupModelChangeKind::GroupIndex => part.on_did_change_group_index.fire(group.as_group()),
                    GroupModelChangeKind::GroupLabel => part.on_did_change_group_label.fire(group.as_group()),
                    _ => {}
                }
            }
        }));

        // Track dispose
        let group_id = group_view.id();
        group_view.on_will_dispose().once().listen(move |_| {
            group_disposables.dispose();

            if let Some(part) = weak_part.upgrade() {
                part.state.borrow_mut().group_views.retain(|group| group.id() != group_id);
                part.do_update_most_recent_active(group_id, false);
            }
        });

        group_view
    }

    /// `addGroup(location, direction, groupToCopy)`.
    pub fn add_group_with_copy(&self, location: Option<GroupRef>, direction: GroupDirection, group_to_copy: Option<Rc<EditorGroupView>>) -> Rc<EditorGroupView> {
        let Some(location) = location else { invalid_group() };
        let location_view = self.assert_group_view(&location);

        let should_expand = self.group_count() > 1 && self.grid().is_view_expanded(&location_view);
        let new_group_view = self.do_create_group_view(group_to_copy.map_or(EditorGroupSource::New, EditorGroupSource::Copy));

        // Add to grid widget
        self.grid().add_view(new_group_view.clone(), self.get_split_sizing_style(), &location_view, to_grid_direction(direction));

        // Event
        self.on_did_add_group.fire(new_group_view.as_group());

        // Notify group index change given a new group was added
        self.notify_group_index_change();

        // Expand new group, if the reference view was previously expanded
        if should_expand {
            self.arrange_groups_of(GroupsArrangement::Expand, Some(new_group_view.clone()));
        }

        new_group_view
    }

    // endregion

    // region: remover, mover, copiar e mesclar

    /// `removeGroup(group)`.
    fn remove_group_view(&self, group_view: &Rc<EditorGroupView>) {
        if self.group_count() == 1 {
            return; // Cannot remove the last root group
        }

        // Remove empty group
        if IEditorGroup::is_empty(&**group_view) {
            self.do_remove_empty_group(group_view);
        }
        // Remove group with editors
        else {
            self.do_remove_group_with_editors(group_view);
        }
    }

    fn do_remove_group_with_editors(&self, group_view: &Rc<EditorGroupView>) {
        let most_recently_active_groups = self.get_group_views(GroupsOrder::MostRecentlyActive);

        let last_active_group = if same_group(&self.active_view(), group_view) { most_recently_active_groups.get(1) } else { most_recently_active_groups.first() };

        // Removing a group with editors should merge these editors into the last active group and then remove this group.
        let last_active_group = last_active_group.cloned().unwrap_or_else(|| invalid_group());
        self.merge_group_views(group_view, &last_active_group, None);
    }

    fn do_remove_empty_group(&self, group_view: &Rc<EditorGroupView>) {
        // Activate next group if the removed one was active
        if same_group(&self.active_view(), group_view) {
            let most_recently_active_groups = self.get_group_views(GroupsOrder::MostRecentlyActive);
            let next_active_group = most_recently_active_groups.get(1).cloned().unwrap_or_else(|| invalid_group()); // [0] will be the current group we are about to dispose
            self.do_set_group_active(&next_active_group, GroupActivationReason::Default);
        }

        // Remove from grid widget & dispose
        self.grid().remove_view(group_view, Some(self.get_split_sizing_style()));
        group_view.dispose();

        // Notify group index change given a group was removed
        self.notify_group_index_change();

        // Event
        self.on_did_remove_group.fire(group_view.as_group());
    }

    /// `mergeGroup(group, target, options)` entre vistas.
    fn merge_group_views(&self, source_view: &Rc<EditorGroupView>, target_view: &Rc<EditorGroupView>, options: Option<MergeGroupOptions>) -> bool {
        let active_group = self.active_view();

        // Collect editors to move/copy
        let mut editors: Vec<EditorInputWithOptions> = Vec::new();
        let mut index = options.and_then(|options| options.index).unwrap_or_else(|| target_view.count());
        for editor in source_view.editors() {
            let inactive = !source_view.is_active(&EditorInputOrUntyped::Typed(editor.clone())) || !same_group(&active_group, source_view);

            let mut actual_index: Option<usize> = None;
            if target_view.contains(&EditorInputOrUntyped::Typed(editor.clone()), None)
                && (
                    // Do not configure an `index` for editors that are sticky in the target, otherwise there is a chance of losing
                    // that state when the editor is moved. See https://github.com/microsoft/vscode/issues/239549
                    target_view.is_sticky(&EditorOrIndex::Editor(editor.clone())) ||
                    // Do not configure an `index` when we are explicitly instructed
                    options.and_then(|options| options.preserve_existing_index) == Some(true)
                )
            {
                // leave `index` as `undefined`
            } else {
                actual_index = Some(index);
                index += 1;
            }

            editors.push(EditorInputWithOptions {
                editor,
                options: Some(TextEditorOptions::from(EditorOptions {
                    index: actual_index.map(|index| index as f64),
                    inactive: Some(inactive),
                    preserve_focus: Some(inactive),
                    ..EditorOptions::default()
                })),
            });
        }

        // Move/Copy editors over into target
        let mut result = true;
        let target_group = target_view.as_group();
        if options.and_then(|options| options.mode) == Some(MergeGroupMode::CopyEditors) {
            source_view.copy_editors(editors, &target_group);
        } else {
            result = source_view.move_editors(editors, &target_group);
        }

        // Remove source if the view is now empty and not already removed
        if IEditorGroup::is_empty(&**source_view) && !source_view.disposed() {
            // could have been disposed already via workbench.editor.closeEmptyGroups setting
            self.remove_group_view(source_view);
        }

        result
    }

    /// `mergeAllGroups(target, options)`.
    pub fn merge_all_groups_with_options(&self, target: &GroupRef, options: Option<MergeGroupOptions>) -> bool {
        let target_view = self.assert_group_view(target);

        let mut result = true;
        for group in self.get_group_views(GroupsOrder::MostRecentlyActive) {
            if same_group(&group, &target_view) {
                continue; // keep target
            }

            let merged = self.merge_group_views(&group, &target_view, options);
            if !merged {
                result = false;
            }
        }

        result
    }

    // endregion

    // region: layout

    /// `applyLayout(layout)`.
    fn do_apply_layout(&self, layout: &EditorGroupLayout) {
        // Determine how many groups we need overall
        let layout_groups_count = count_layout_groups(&layout.groups);

        // If we currently have too many groups, merge them into the last one
        let mut current_group_views = self.get_group_views(GroupsOrder::GridAppearance);
        if layout_groups_count < current_group_views.len() {
            let last_group_in_layout = layout_groups_count.checked_sub(1).and_then(|index| current_group_views.get(index)).cloned().unwrap_or_else(|| invalid_group());
            for (index, group) in current_group_views.iter().enumerate() {
                if index >= layout_groups_count {
                    self.merge_group_views(group, &last_group_in_layout, None);
                }
            }

            current_group_views = self.get_group_views(GroupsOrder::GridAppearance);
        }

        let active_group = self.active_view();

        // Prepare grid descriptor to create new grid from
        let grid_descriptor = create_serialized_grid(GridDescriptor {
            orientation: to_grid_orientation(layout.orientation),
            size: None,
            groups: layout.groups.iter().map(layout_argument_to_descriptor).collect(),
        });

        // Recreate gridwidget with descriptor
        self.do_apply_grid_state(&grid_descriptor, active_group.id(), Some(current_group_views));
    }

    fn do_create_grid_control(&self) {
        // Grid Widget (with previous UI state)
        let mut restore_error = false;
        if self.state.borrow().will_restore_state {
            restore_error = !self.do_create_grid_control_with_previous_state();
        }

        // Grid Widget (no previous UI state or failed to restore)
        if self.grid_widget().is_none() || restore_error {
            let initial_group = self.do_create_group_view(EditorGroupSource::New);
            self.do_set_grid_widget(SerializableGrid::new(&self.ui, initial_group.clone(), GridOptions::default()));

            // Ensure a group is active
            self.do_set_group_active(&initial_group, GroupActivationReason::Default);
        }

        // Notify group index change we created the entire grid
        self.notify_group_index_change();
    }

    /// `loadState()`: o estado do memento do espaço de trabalho (`None` se ausente).
    fn load_state(&self) -> Option<Dynamic> {
        let workspace_memento = self.memento.get_memento(StorageScope::Workspace, StorageTarget::User);

        workspace_memento.get(&JsString::from(EDITOR_PART_UI_STATE_STORAGE_KEY)).filter(Truthy::is_truthy)
    }

    fn do_create_grid_control_with_previous_state(&self) -> bool {
        let Some(raw_state) = self.load_state() else { return true };
        if !prop(&raw_state, "serializedGrid").is_truthy() {
            return true;
        }

        let Some(state) = state_from_dynamic(&raw_state) else {
            // Log error
            errors::on_unexpected_error(&Error::new("Error restoring editor grid widget: the stored state is not valid"));

            // Clear any state we have from the failing restore
            self.dispose_groups();

            return false; // failure
        };

        // MRU
        self.state.borrow_mut().most_recent_active_groups = state.most_recent_active_groups.clone();

        // Grid Widget
        if let Err(error) = errors::catch(|| self.do_create_grid_control_with_state(&state.serialized_grid, state.active_group, None)) {
            // Log error
            errors::on_unexpected_error(&Error::new(format!("Error restoring editor grid widget: {error}")));

            // Clear any state we have from the failing restore
            self.dispose_groups();

            return false; // failure
        }

        true // success
    }

    fn do_create_grid_control_with_state(&self, serialized_grid: &SerializedEditorGrid, active_group_id: GroupIdentifier, editor_group_views_to_reuse: Option<Vec<Rc<EditorGroupView>>>) {
        // Determine group views to reuse if any
        let reuse_group_views = RefCell::new(editor_group_views_to_reuse.unwrap_or_default()); // do not modify original array

        // Create new
        let group_views: RefCell<Vec<Rc<EditorGroupView>>> = RefCell::new(Vec::new());
        let from_json = |serialized_editor_group: &Option<SerializedEditorGroupModel>| -> Rc<EditorGroupView> {
            let reused = {
                let mut reuse = reuse_group_views.borrow_mut();
                if reuse.is_empty() { None } else { Some(reuse.remove(0)) }
            };

            let group_view = reused.unwrap_or_else(|| {
                self.do_create_group_view(serialized_editor_group.clone().map_or(EditorGroupSource::New, EditorGroupSource::Serialized))
            });

            group_views.borrow_mut().push(group_view.clone());

            if group_view.id() == active_group_id {
                self.do_set_group_active(&group_view, GroupActivationReason::Default);
            }

            group_view
        };
        let grid_widget = SerializableGrid::deserialize(&self.ui, serialized_grid, &from_json, GridOptions::default());
        let group_views = group_views.into_inner();

        // If the active group was not found when restoring the grid make sure to make at least one group active. We always
        // need an active group.
        if self.state.borrow().active_group.is_none() {
            let first_group = group_views.first().cloned().unwrap_or_else(|| invalid_group());
            self.do_set_group_active(&first_group, GroupActivationReason::Default);
        }

        // Validate MRU group views matches grid widget state
        let mru_is_stale = self.state.borrow().most_recent_active_groups.iter().any(|group_id| self.get_group_view(*group_id).is_none());
        if mru_is_stale {
            self.state.borrow_mut().most_recent_active_groups = group_views.iter().map(|group| group.id()).collect();
        }

        // Set it
        self.do_set_grid_widget(grid_widget);
    }

    fn do_set_grid_widget(&self, grid_widget: Rc<SerializableGrid<EditorGroupView>>) {
        let mut boundary_sashes = BoundarySashes::default();

        let previous_grid_widget = self.grid_widget();
        if let Some(previous_grid_widget) = &previous_grid_widget {
            boundary_sashes = previous_grid_widget.boundary_sashes();
            previous_grid_widget.dispose();
        }

        self.state.borrow_mut().grid_widget = Some(grid_widget.clone());
        grid_widget.set_boundary_sashes(boundary_sashes);
        self.grid_widget_view.set_grid_widget(Some(grid_widget.clone()));

        self.on_did_scroll.set_input(grid_widget.on_did_scroll());
        self.grid_widget_disposables.clear();
        let weak = self.this.clone();
        self.grid_widget_disposables.add(grid_widget.on_did_change_view_maximized().listen(move |maximized| {
            if let Some(part) = weak.upgrade() {
                part.on_did_change_group_maximized.fire(*maximized);
            }
        }));

        self.on_did_set_grid_widget.fire(());
    }

    fn do_apply_grid_state(&self, grid_state: &SerializedEditorGrid, active_group_id: GroupIdentifier, editor_group_views_to_reuse: Option<Vec<Rc<EditorGroupView>>>) {
        // Recreate grid widget from state
        self.do_create_grid_control_with_state(grid_state, active_group_id, editor_group_views_to_reuse.clone());

        // Layout, but only if the part has already been laid out at least once. When restoring a working set into an editor
        // part that has never been shown, the content dimension is still undefined; the grid is laid out later when the part
        // is first shown.
        let content_dimension = self.state.borrow().content_dimension;
        if let Some(content_dimension) = content_dimension {
            let (top, left) = (self.state.borrow().top, self.state.borrow().left);
            self.do_layout(content_dimension, top, left);
        }

        // Events for groups that got added
        let reused = editor_group_views_to_reuse.unwrap_or_default();
        for group_view in self.get_group_views(GroupsOrder::GridAppearance) {
            if !reused.iter().any(|candidate| same_group(candidate, &group_view)) {
                self.on_did_add_group.fire(group_view.as_group());
            }
        }

        // Notify group index change given layout has changed
        self.notify_group_index_change();
    }

    fn notify_group_index_change(&self) {
        for (index, group) in self.get_group_views(GroupsOrder::GridAppearance).iter().enumerate() {
            group.notify_index_changed(index);
        }
    }

    /// `notifyGroupsLabelChange(newLabel)`.
    pub fn notify_groups_label_change(&self, new_label: &str) {
        for group in self.group_views() {
            group.notify_label_changed(new_label);
        }
    }

    /// `isEmpty`: um grupo só, e vazio.
    fn is_empty(&self) -> bool {
        self.group_count() == 1 && IEditorGroup::is_empty(&*self.active_view())
    }

    fn do_layout(&self, dimension: Dimension, top: f64, left: f64) {
        self.state.borrow_mut().content_dimension = Some(dimension);

        // Layout Grid
        self.centered_layout_widget().layout(dimension.width, dimension.height, top, left);

        // Event
        self.on_did_layout.fire(dimension);
    }

    /// `layout(width, height, top, left)`: sem título, a área de conteúdo é a área inteira (`layoutContents`).
    pub fn layout(&self, width: f64, height: f64, top: f64, left: f64) {
        {
            let mut state = self.state.borrow_mut();
            state.top = top;
            state.left = left;
        }

        self.do_layout(Dimension { width, height }, top, left);
    }

    // endregion

    // region: criação da parte e estado

    /// `createContentArea(parent, options)`: cria a grade (restaurando o estado anterior se pedido) e o layout centralizado.
    pub fn create(&self, options: Option<EditorPartCreationOptions>) {
        // Grid control
        self.state.borrow_mut().will_restore_state = options.is_none_or(|options| options.restore_previous_state);
        self.do_create_grid_control();

        // Centered layout widget
        let profile_memento = self.memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
        let centered_state = profile_memento.get(&JsString::from(EDITOR_PART_CENTERED_VIEW_STORAGE_KEY)).and_then(|state| centered_state_from_dynamic(&state));
        let centered_layout_fixed_width = self.state.borrow().part_options.centered_layout_fixed_width;
        let grid_widget_view: GridViewRef = self.grid_widget_view.clone();
        let centered_layout_widget = CenteredViewLayout::new(&self.ui, self.container, grid_widget_view, centered_state, centered_layout_fixed_width);
        self.store.add_rc(centered_layout_widget.clone());
        self.state.borrow_mut().centered_layout_widget = Some(centered_layout_widget.clone());
        self.store.add(self.on_did_change_editor_part_options.event().listen(move |event| {
            centered_layout_widget.set_fixed_width(event.new_part_options.centered_layout_fixed_width);
        }));

        // Signal ready
        self.when_ready_promise.complete(());
        self.state.borrow_mut().is_ready = true;

        // Signal restored
        self.when_restored_promise.complete(());
    }

    /// `createState()`.
    pub fn create_state(&self) -> EditorPartUiState {
        let serialized_grid = self.grid().serialize();

        EditorPartUiState {
            serialized_grid: SerializedGrid {
                root: map_serialized_node(&serialized_grid.root),
                orientation: serialized_grid.orientation,
                width: serialized_grid.width,
                height: serialized_grid.height,
            },
            active_group: self.active_view().id(),
            most_recent_active_groups: self.state.borrow().most_recent_active_groups.clone(),
        }
    }

    /// `saveState()`: grava o estado do grid (espaço de trabalho) e o do layout centralizado (perfil) nos mementos.
    pub fn save_state(&self) {
        let workspace_memento = self.memento.get_memento(StorageScope::Workspace, StorageTarget::User);
        let profile_memento = self.memento.get_memento(StorageScope::Profile, StorageTarget::Machine);

        // Persist grid UI state
        if self.grid_widget().is_some() {
            let key = JsString::from(EDITOR_PART_UI_STATE_STORAGE_KEY);
            if self.is_empty() {
                workspace_memento.delete(&key);
            } else {
                workspace_memento.set(&key, state_to_dynamic(&self.create_state()));
            }
        }

        // Persist centered view state
        let centered_layout_widget = self.state.borrow().centered_layout_widget.clone();
        if let Some(centered_layout_widget) = centered_layout_widget {
            let key = JsString::from(EDITOR_PART_CENTERED_VIEW_STORAGE_KEY);
            let centered_layout_state = centered_layout_widget.state();
            if centered_layout_widget.is_default(&centered_layout_state) {
                profile_memento.delete(&key);
            } else {
                profile_memento.set(&key, centered_state_to_dynamic(&centered_layout_state));
            }
        }
    }

    /// `workspaceMemento`, para quem precisa ler ou limpar o estado persistido (o `clearState` do `TestEditorPart`).
    pub fn workspace_memento(&self) -> ObjectRef {
        self.memento.get_memento(StorageScope::Workspace, StorageTarget::Machine)
    }

    /// `profileMemento`.
    pub fn profile_memento(&self) -> ObjectRef {
        self.memento.get_memento(StorageScope::Profile, StorageTarget::Machine)
    }

    fn dispose_groups(&self) {
        for group in self.group_views() {
            group.dispose();

            self.on_did_remove_group.fire(group.as_group());
        }

        let mut state = self.state.borrow_mut();
        state.group_views.clear();
        state.most_recent_active_groups.clear();
    }

    // endregion

    // region: eventos fora do contrato

    /// `onDidChangeGroupLabel`.
    pub fn on_did_change_group_label(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.on_did_change_group_label.event()
    }

    /// `onDidFocus`.
    pub fn on_did_focus(&self) -> Event<()> {
        self.on_did_focus.event()
    }

    // endregion
}

/// `Some` em cada folha do grid serializado dos grupos (o dado do grupo existe).
fn map_serialized_node(node: &SerializedNode<SerializedEditorGroupModel>) -> SerializedNode<Option<SerializedEditorGroupModel>> {
    match node {
        SerializedNode::Leaf { data, size, visible, maximized } => SerializedNode::Leaf { data: Some(data.clone()), size: *size, visible: *visible, maximized: *maximized },
        SerializedNode::Branch { data, size, visible } => SerializedNode::Branch { data: data.iter().map(map_serialized_node).collect(), size: *size, visible: *visible },
    }
}

impl IEditorPart for EditorPart {
    fn window_id(&self) -> WindowId {
        self.window_id
    }

    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        self.active_view().as_group()
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        self.group_views().iter().map(|group| group.as_group()).collect()
    }

    fn get_groups(&self, order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>> {
        self.get_group_views(order).iter().map(|group| group.as_group()).collect()
    }

    fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<dyn IEditorGroup>> {
        self.get_group_view(identifier).map(|group| group.as_group())
    }

    fn find_group(&self, scope: &FindGroupScope) -> Option<Rc<dyn IEditorGroup>> {
        self.find_group_from(scope, None, false)
    }

    fn add_group(&self, location: Option<GroupRef>, direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        self.add_group_with_copy(location, direction, None).as_group()
    }

    fn on_did_change_active_group(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.on_did_change_active_group.event()
    }

    fn on_did_add_group(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.on_did_add_group.event()
    }

    fn on_did_remove_group(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.on_did_remove_group.event()
    }

    fn on_did_move_group(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.on_did_move_group.event()
    }

    fn on_did_activate_group(&self) -> Event<EditorGroupActivationEvent> {
        self.on_did_activate_group.event()
    }

    fn on_did_change_group_index(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.on_did_change_group_index.event()
    }

    fn on_did_change_group_locked(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.on_did_change_group_locked.event()
    }

    fn on_did_change_group_maximized(&self) -> Event<bool> {
        self.on_did_change_group_maximized.event()
    }

    fn on_did_change_editor_part_options(&self) -> Event<EditorPartOptionsChangeEvent> {
        self.on_did_change_editor_part_options.event()
    }

    fn is_ready(&self) -> bool {
        self.state.borrow().is_ready
    }

    fn when_ready(&self) -> Promise<()> {
        self.when_ready_promise.promise.clone()
    }

    fn when_restored(&self) -> Promise<()> {
        self.when_restored_promise.promise.clone()
    }

    fn has_restorable_state(&self) -> bool {
        self.load_state().is_some()
    }

    fn side_group(&self) -> Rc<dyn IEditorSideGroup> {
        Rc::new(PartSideGroup { part: self.this.clone() })
    }

    fn orientation(&self) -> GroupOrientation {
        match self.grid_widget() {
            Some(grid) if grid.orientation() == Orientation::Vertical => GroupOrientation::Vertical,
            _ => GroupOrientation::Horizontal,
        }
    }

    fn part_options(&self) -> EditorPartOptions {
        self.state.borrow().part_options.clone()
    }

    fn enforce_part_options(&self, options: Dynamic) -> DisposableRef {
        let enforcement_id = {
            let mut state = self.state.borrow_mut();
            let enforcement_id = state.next_enforcement_id;
            state.next_enforcement_id += 1;
            state.enforced_part_options.push((enforcement_id, options));

            enforcement_id
        };
        self.handle_changed_part_options();

        let weak = self.this.clone();
        to_disposable(move || {
            if let Some(part) = weak.upgrade() {
                part.state.borrow_mut().enforced_part_options.retain(|(id, _)| *id != enforcement_id);
                part.handle_changed_part_options();
            }
        })
    }

    fn activate_group(&self, group: GroupRef) -> Rc<dyn IEditorGroup> {
        self.activate_group_with_reason(&group, GroupActivationReason::Default).as_group()
    }

    fn get_size(&self, group: GroupRef) -> Dimension {
        let size = self.grid().get_view_size(Some(&self.assert_group_view(&group)));

        Dimension { width: size.width, height: size.height }
    }

    fn set_size(&self, group: GroupRef, size: Dimension) {
        let group_view = self.assert_group_view(&group);

        self.grid().resize_view(&group_view, ViewDimensions { width: size.width, height: size.height });
    }

    fn arrange_groups(&self, arrangement: GroupsArrangement, target: Option<GroupRef>) {
        let target = target.map(|target| self.assert_group_view(&target));

        self.arrange_groups_of(arrangement, target);
    }

    fn toggle_maximize_group(&self, group: Option<GroupRef>) {
        if self.has_maximized_group() {
            self.unmaximize_group();
        } else {
            self.arrange_groups(GroupsArrangement::Maximize, group);
        }
    }

    fn toggle_expand_group(&self, group: Option<GroupRef>) {
        if self.grid().is_view_expanded(&self.active_view()) {
            self.arrange_groups(GroupsArrangement::Even, None);
        } else {
            self.arrange_groups(GroupsArrangement::Expand, group);
        }
    }

    fn apply_layout(&self, layout: &EditorGroupLayout) {
        self.do_apply_layout(layout);
    }

    fn get_layout(&self) -> EditorGroupLayout {
        // Example return value:
        // { orientation: 0, groups: [ { groups: [ { size: 0.4 }, { size: 0.6 } ], size: 0.5 }, { groups: [ {}, {} ], size: 0.5 } ] }
        let serialized_grid = self.grid().serialize();
        let root = serialized_node_to_layout_argument(&serialized_grid.root);

        EditorGroupLayout { orientation: to_group_orientation(serialized_grid.orientation), groups: root.groups.unwrap_or_default() }
    }

    fn set_group_orientation(&self, orientation: GroupOrientation) {
        let Some(grid) = self.grid_widget() else {
            return; // we have not been created yet
        };

        let new_orientation = to_grid_orientation(orientation);
        if grid.orientation() != new_orientation {
            grid.set_orientation(new_orientation);
        }
    }

    fn find_group_from(&self, scope: &FindGroupScope, source: Option<GroupRef>, wrap: bool) -> Option<Rc<dyn IEditorGroup>> {
        self.do_find_group(scope, source, wrap).map(|group| group.as_group())
    }

    fn remove_group(&self, group: GroupRef) {
        // O `assertGroupView` do original devolve a própria instância sem conferir se a parte ainda a tem, e `removeGroup` sai
        // sem fazer nada quando só resta um grupo (`count === 1`): um grupo que a parte já removeu (por exemplo, pela
        // configuração `closeEmptyGroups`) passa por esse caminho. Aqui, sem como converter o `Rc<dyn IEditorGroup>` na
        // vista, só esse caso é aceito.
        let group_view = match &group {
            GroupRef::Group(candidate) if !self.has_group(candidate.id()) && self.group_count() == 1 => return,
            _ => self.assert_group_view(&group),
        };

        self.remove_group_view(&group_view);
    }

    fn move_group(&self, group: GroupRef, location: GroupRef, direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        let source_view = self.assert_group_view(&group);
        let target_view = self.assert_group_view(&location);

        if same_group(&source_view, &target_view) {
            throw(Error::new("Cannot move group into its own"));
        }

        // Move via grid widget API
        self.grid().move_view(&source_view, self.get_split_sizing_style(), &target_view, to_grid_direction(direction));

        // Event
        self.on_did_move_group.fire(source_view.as_group());

        // Notify group index change given a group was moved
        self.notify_group_index_change();

        source_view.as_group()
    }

    fn merge_group(&self, group: GroupRef, target: GroupRef, options: Option<MergeGroupOptions>) -> bool {
        let source_view = self.assert_group_view(&group);
        let target_view = self.assert_group_view(&target);

        self.merge_group_views(&source_view, &target_view, options)
    }

    fn merge_all_groups(&self, target: GroupRef) -> bool {
        self.merge_all_groups_with_options(&target, None)
    }

    fn copy_group(&self, group: GroupRef, location: GroupRef, direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        let group_view = self.assert_group_view(&group);
        let location_view = self.assert_group_view(&location);

        // Copy the group view
        self.add_group_with_copy(Some(GroupRef::Group(location_view.as_group())), direction, Some(group_view)).as_group()
    }

    fn on_did_layout(&self) -> Event<Dimension> {
        self.on_did_layout.event()
    }

    fn on_did_scroll(&self) -> Event<()> {
        any(vec![self.on_did_set_grid_widget.event(), self.on_did_scroll.event()])
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    fn content_dimension(&self) -> Dimension {
        self.state.borrow().content_dimension.unwrap_or(Dimension { width: 0.0, height: 0.0 })
    }

    fn has_maximized_group(&self) -> bool {
        self.grid().has_maximized_view()
    }

    fn center_layout(&self, active: bool) {
        self.centered_layout_widget().activate(active);
    }

    fn is_layout_centered(&self) -> bool {
        let centered_layout_widget = self.state.borrow().centered_layout_widget.clone();

        centered_layout_widget.is_some_and(|widget| widget.is_active())
    }
}

impl IDisposable for EditorPart {
    fn dispose(&self) {
        if std::mem::replace(&mut self.state.borrow_mut().disposed, true) {
            return;
        }

        // Event
        self.on_will_dispose.fire(());

        // Forward to all groups
        self.dispose_groups();

        // Grid widget
        if let Some(grid_widget) = self.grid_widget() {
            grid_widget.dispose();
        }

        self.grid_widget_disposables.dispose();
        self.store.dispose();
        self.ui.remove(self.container);
    }
}
