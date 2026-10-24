//! Porte de `src/vs/workbench/api/browser/mainThreadEditorTabs.ts`: o modelo de abas (`vscode.window.tabGroups`) do lado
//! do workbench, que acompanha o `IEditorService` e o `IEditorGroupsService` e avisa o host de extensões por RPC.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadEditorTabs)` e o `IExtHostContext` do construtor: o registro de clientes
//!   não existe ainda; o construtor recebe o [`IMainContext`] e pede o proxy de `ExtHostEditorTabs`. Os ouvintes (do
//!   `IEditorService`, dos grupos) guardam uma referência fraca ao objeto (o `this` do original).
//! - O objeto `IEditorTabDto` que o `_groupLookup` (via `tabs`) e o `_tabInfoLookup` guardam é um só no original, e o código
//!   o altera no lugar; aqui é um `Rc<RefCell<EditorTabDto>>` compartilhado pelos dois, e o que vai para o proxy é uma cópia
//!   (o RPC real serializa). Idem o grupo (`Rc<RefCell<TabGroupModel>>`, compartilhado entre `_tabGroupModel` e `_groupLookup`).
//! - `try { _updateTabsModel } catch { ... }` é `errors::catch`: o `TypeError` de `tabs[editorIndex]` fora do intervalo
//!   (`_onDidTabActiveChange`) é lançado com `errors::throw`, e o `splice` do JavaScript (que tolera índice fora da lista)
//!   é o par de funções [`splice_insert`] e [`splice_remove`].
//! - O `switch` de `_updateTabsModel` cai de um `case` no seguinte quando a condição do `if` falha (não há `break` fora
//!   do `if`): [`UPDATE_CASES`] lista os `case` na ordem do original e o laço de `update_tabs_model` repete a queda.
//! - `instanceof` sobre as classes de entrada: `AbstractTextResourceEditorInput` é o `downcast_ref` por
//!   [`EditorInput::as_any`] das três subclasses portadas (`TextResourceEditorInput`, `FileEditorInput` e
//!   `UntitledTextEditorInput`); `SideBySideEditorInput` sem `DiffEditorInput` é `side_by_side_editors` presente com
//!   `diff_editors` ausente; `DiffEditorInput`, o `diff_editors`; `WebviewInput`, o `downcast_ref`.
//! - Ainda sem porte, e por isso fora de `_editorInputToDto` (uma entrada dessas classes não existe para ser testada):
//!   `MergeEditorInput`, `NotebookEditorInput` (inclusive o diff de notebooks), `CustomEditorInput`, `TerminalEditorInput`,
//!   `InteractiveEditorInput`, `ChatEditorInput` e `MultiDiffEditorInput`. Pelo último também ficam de fora o mapa
//!   `_multiDiffEditorInputListeners` (o ouvinte de `Event.fromObservableLight(editorInput.resources)` que reenvia o DTO
//!   como `TAB_UPDATE`) e a remoção dele em `_onDidTabClose`. Entram junto do porte dessas classes.
//! - `groupId` do protocolo é `u32` e o `GroupIdentifier` é `i64`; os índices do protocolo são `usize`, então o
//!   `index < 0` de `$moveTab` não existe (a extensão não tem como mandar índice negativo).
//! - `$closeTab` e `$closeGroup` (`async`) são `async_fn`: a parte síncrona (montar o mapa grupo para editores) roda antes
//!   do primeiro `await`, como no original; o `Map<IEditorGroup, EditorInput[]>` é uma lista de pares comparada por
//!   identidade do grupo.
//! - `${resource?.primary?.toString()}-${resource?.secondary?.toString()}` imprime `undefined` para o lado ausente.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::log::LogServiceRef;
use je_platform::opener::editor_types::EditorOptions;
use je_platform::resources::ext_uri;

use crate::api::common::ext_host_protocol::{
    AnyInputDto, EditorGroupColumn, EditorTabDto, EditorTabGroupDto, ExtHostContext, ExtHostEditorTabsShape, IMainContext, MainThreadEditorTabsShape, TabModelOperationKind,
    TabOperation, TextDiffInputDto, TextInputDto,
};
use crate::common::editor::editor_group_model::{is_group_editor_move_event, EditorOrIndex, GroupModelChangeEvent};
use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::text_resource_editor_input::TextResourceEditorInput;
use crate::common::editor::{
    EditorInputOrUntyped, EditorInputRef, EditorResourceAccessor, EditorResourceAccessorOptions, EditorUri, GroupIdentifier, GroupModelChangeKind, SideBySideEditor,
};
use crate::contrib::files::browser::editors::file_editor_input::FileEditorInput;
use crate::contrib::webview_panel::webview_editor_input::WebviewInput;
use crate::services::editor::editor_group_column::{column_to_editor_group, editor_group_to_column};
use crate::services::editor::editor_groups_service::{
    preferred_side_by_side_group_direction, CloseEditorOptions, CloseEditorsTarget, GroupDirection, GroupRef, IEditorGroup, IEditorGroupsService, SIDE_GROUP,
};
use crate::services::editor::editor_service::{EditorsChangeEvent, IEditorService};
use crate::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;

/// `TabInfo`.
struct TabInfo {
    tab: Rc<RefCell<EditorTabDto>>,
    group: Rc<dyn IEditorGroup>,
    editor_input: EditorInputRef,
}

/// `IEditorTabGroupDto`, com as abas compartilhadas com o `_tabInfoLookup`.
struct TabGroupModel {
    group_id: GroupIdentifier,
    is_active: bool,
    view_column: EditorGroupColumn,
    tabs: Vec<Rc<RefCell<EditorTabDto>>>,
}

impl TabGroupModel {
    /// A cópia que o RPC leva ao host.
    fn to_dto(&self) -> EditorTabGroupDto {
        EditorTabGroupDto {
            is_active: self.is_active,
            view_column: self.view_column,
            tabs: self.tabs.iter().map(|tab| tab.borrow().clone()).collect(),
            group_id: self.group_id as u32,
        }
    }
}

type SharedGroup = Rc<RefCell<TabGroupModel>>;

/// O ponto de partida de `Array.prototype.splice(start, ...)` num array de `len` elementos: o índice negativo conta do fim e
/// o que passa do tamanho vale o tamanho.
fn splice_start(len: usize, index: i64) -> usize {
    if index < 0 { (len as i64 + index).max(0) as usize } else { (index as usize).min(len) }
}

/// `tabs.splice(index, 0, item)`.
fn splice_insert<T>(items: &mut Vec<T>, index: i64, item: T) {
    let start = splice_start(items.len(), index);
    items.insert(start, item);
}

/// `tabs.splice(index, 1)`: o elemento removido, se o índice caía dentro da lista.
fn splice_remove<T>(items: &mut Vec<T>, index: i64) -> Option<T> {
    let start = splice_start(items.len(), index);
    (start < items.len()).then(|| items.remove(start))
}

/// `editor instanceof AbstractTextResourceEditorInput`: a base e as três subclasses portadas.
fn is_abstract_text_resource_editor_input(editor: &EditorInputRef) -> bool {
    let any = editor.as_any();
    any.is::<TextResourceEditorInput>() || any.is::<FileEditorInput>() || any.is::<UntitledTextEditorInput>()
}

/// Os `case` do `switch` de `_updateTabsModel`, na ordem em que o original os escreve (a queda de um para o seguinte segue essa ordem).
const UPDATE_CASES: [GroupModelChangeKind; 10] = [
    GroupModelChangeKind::EditorLabel,
    GroupModelChangeKind::EditorOpen,
    GroupModelChangeKind::EditorClose,
    GroupModelChangeKind::EditorActive,
    GroupModelChangeKind::EditorDirty,
    GroupModelChangeKind::EditorSticky,
    GroupModelChangeKind::EditorPin,
    GroupModelChangeKind::EditorTransient,
    GroupModelChangeKind::EditorsSelection,
    GroupModelChangeKind::EditorMove,
];

/// `MainThreadEditorTabs`.
pub struct MainThreadEditorTabs {
    disposables: Rc<DisposableStore>,
    proxy: Rc<dyn ExtHostEditorTabsShape>,
    editor_groups_service: Rc<dyn IEditorGroupsService>,
    configuration_service: Rc<dyn IConfigurationService>,
    log_service: LogServiceRef,
    /// List of all groups and their corresponding tabs, this is **the** model.
    tab_group_model: RefCell<Vec<SharedGroup>>,
    /// Lookup table for finding group by id.
    group_lookup: RefCell<HashMap<GroupIdentifier, SharedGroup>>,
    /// Lookup table for finding tab by id.
    tab_info_lookup: RefCell<HashMap<String, TabInfo>>,
}

impl MainThreadEditorTabs {
    pub fn new(
        ext_host_context: &dyn IMainContext,
        editor_groups_service: Rc<dyn IEditorGroupsService>,
        configuration_service: Rc<dyn IConfigurationService>,
        log_service: LogServiceRef,
        editor_service: Rc<dyn IEditorService>,
    ) -> Rc<Self> {
        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_EDITOR_TABS);
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let disposables = DisposableStore::new();

            // Main listener which responds to events from the editor service
            let this = weak.clone();
            disposables.add(editor_service.on_did_editors_change().listen(move |event| {
                let Some(this) = this.upgrade() else { return };
                if errors::catch(|| this.update_tabs_model(event)).is_err() {
                    this.log_service.error("Failed to update model, rebuilding");
                    this.create_tabs_model();
                }
            }));

            // Structural group changes (add, remove, move, etc) are difficult to patch.
            // Since they happen infrequently we just rebuild the entire model
            let this = weak.clone();
            disposables.add(editor_groups_service.on_did_add_group().listen(move |_| {
                if let Some(this) = this.upgrade() {
                    this.create_tabs_model();
                }
            }));
            let this = weak.clone();
            disposables.add(editor_groups_service.on_did_remove_group().listen(move |_| {
                if let Some(this) = this.upgrade() {
                    this.create_tabs_model();
                }
            }));

            // Once everything is read go ahead and initialize the model
            let this = weak.clone();
            editor_groups_service.when_ready().then(move |_| {
                if let Some(this) = this.upgrade() {
                    this.create_tabs_model();
                }
            });

            Self {
                disposables,
                proxy,
                editor_groups_service,
                configuration_service,
                log_service,
                tab_group_model: RefCell::new(Vec::new()),
                group_lookup: RefCell::new(HashMap::new()),
                tab_info_lookup: RefCell::new(HashMap::new()),
            }
        })
    }

    /// Creates a tab object with the correct properties.
    fn build_tab_object(&self, group: &Rc<dyn IEditorGroup>, editor: &EditorInputRef, editor_index: i64) -> EditorTabDto {
        EditorTabDto {
            id: self.generate_tab_id(editor, group.id()),
            label: editor.get_name(),
            editor_id: editor.editor_id(),
            input: self.editor_input_to_dto(editor),
            is_pinned: group.is_sticky(&EditorOrIndex::Index(editor_index)),
            is_preview: !group.is_pinned(&EditorOrIndex::Index(editor_index)),
            is_active: group.is_active(&EditorInputOrUntyped::from(editor)),
            is_dirty: editor.is_dirty(),
        }
    }

    /// `_editorInputToDto` (ver o cabeçalho para as classes que ainda não existem).
    fn editor_input_to_dto(&self, editor: &EditorInputRef) -> AnyInputDto {
        if is_abstract_text_resource_editor_input(editor) {
            return AnyInputDto::Text(TextInputDto { uri: editor.resource().expect("a entrada de recurso de texto tem recurso") });
        }

        let diff_editors = editor.diff_editors();
        if let (Some((primary, secondary)), None) = (editor.side_by_side_editors(), &diff_editors) {
            let primary_resource = primary.resource();
            let secondary_resource = secondary.resource();
            // If side by side editor with same resource on both sides treat it as a singular tab kind
            if is_abstract_text_resource_editor_input(&primary)
                && is_abstract_text_resource_editor_input(&secondary)
                && ext_uri().is_equal(primary_resource.as_ref(), secondary_resource.as_ref(), false)
                && secondary_resource.is_some()
                && let Some(primary_resource) = primary_resource
            {
                return AnyInputDto::Text(TextInputDto { uri: primary_resource });
            }
            return AnyInputDto::Unknown;
        }

        if let Some(webview) = editor.as_any().downcast_ref::<WebviewInput>() {
            return AnyInputDto::Webview(crate::api::common::ext_host_protocol::WebviewInputDto { view_type: webview.view_type.clone() });
        }

        if let Some((modified, original)) = diff_editors
            && is_abstract_text_resource_editor_input(&modified)
            && is_abstract_text_resource_editor_input(&original)
        {
            return AnyInputDto::TextDiff(TextDiffInputDto {
                modified: modified.resource().expect("a entrada de recurso de texto tem recurso"),
                original: original.resource().expect("a entrada de recurso de texto tem recurso"),
            });
        }

        AnyInputDto::Unknown
    }

    /// Generates a unique id for a tab.
    fn generate_tab_id(&self, editor: &EditorInputRef, group_id: GroupIdentifier) -> String {
        // Properly get the resource and account for side by side editors
        let resource = EditorResourceAccessor::get_canonical_uri_sides(
            Some(&EditorInputOrUntyped::from(editor)),
            Some(&EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Both), filter_by_scheme: None }),
        );
        let to_text = |uri: &Option<URI>| uri.as_ref().map_or_else(|| "undefined".to_owned(), |uri| uri.to_string(false).to_string_lossy());
        let resource_string = match resource {
            Some(EditorUri::Resource(resource)) => resource.to_string(false).to_string_lossy(),
            Some(EditorUri::Sides { primary, secondary }) => format!("{}-{}", to_text(&primary), to_text(&secondary)),
            None => "undefined-undefined".to_owned(),
        };
        format!("{}~{}-{}-{} ", group_id, editor.editor_id().unwrap_or_else(|| "undefined".to_owned()), editor.editor_type_id(), resource_string)
    }

    /// Called whenever a group activates, updates the model by marking the group as active an notifies the extension host.
    fn on_did_group_activate(&self) {
        let active_group_id = self.editor_groups_service.active_group().id();
        let active_group = self.group_lookup.borrow().get(&active_group_id).cloned();
        if let Some(active_group) = active_group {
            // Ok not to loop as exthost accepts last active group
            active_group.borrow_mut().is_active = true;
            let dto = active_group.borrow().to_dto();
            self.proxy.rpc_accept_tab_group_update(dto);
        }
    }

    /// Called when the tab label changes.
    fn on_did_tab_label_change(&self, group_id: GroupIdentifier, editor_input: &EditorInputRef, editor_index: i64) {
        let tab_id = self.generate_tab_id(editor_input, group_id);
        let tab = self.tab_info_lookup.borrow().get(&tab_id).map(|tab_info| tab_info.tab.clone());
        // If tab is found patch, else rebuild
        if let Some(tab) = tab {
            tab.borrow_mut().label = editor_input.get_name();
            self.send_tab_operation(TabModelOperationKind::TabUpdate, group_id, editor_index, &tab, None);
        } else {
            if self.editor_groups_service.active_modal_editor_part().is_some_and(|part| part.groups().iter().any(|group| group.id() == group_id)) {
                return;
            }
            self.log_service.error("Invalid model for label change, rebuilding");
            self.create_tabs_model();
        }
    }

    /// Called when a new tab is opened.
    fn on_did_tab_open(&self, group_id: GroupIdentifier, editor_input: &EditorInputRef, editor_index: i64) {
        let group = self.editor_groups_service.get_group(group_id);
        // Even if the editor service knows about the group the group might not exist yet in our model
        let group_in_model = self.group_lookup.borrow().contains_key(&group_id);
        // Means a new group was likely created so we rebuild the model
        let (Some(group), true) = (group, group_in_model) else {
            self.create_tabs_model();
            return;
        };
        let model = self.group_lookup.borrow().get(&group_id).cloned();
        let Some(model) = model else {
            return;
        };
        // Splice tab into group at index editorIndex
        let tab_object = Rc::new(RefCell::new(self.build_tab_object(&group, editor_input, editor_index)));
        splice_insert(&mut model.borrow_mut().tabs, editor_index, tab_object.clone());
        // Update lookup
        let tab_id = self.generate_tab_id(editor_input, group_id);
        self.tab_info_lookup.borrow_mut().insert(tab_id, TabInfo { group, editor_input: editor_input.clone(), tab: tab_object.clone() });

        self.send_tab_operation(TabModelOperationKind::TabOpen, group_id, editor_index, &tab_object, None);
    }

    /// Called when a tab is closed.
    fn on_did_tab_close(&self, group_id: GroupIdentifier, editor_index: i64) {
        let group = self.editor_groups_service.get_group(group_id);
        let model = self.group_lookup.borrow().get(&group_id).cloned();
        // Something is wrong with the model state so we rebuild
        let (Some(_group), Some(model)) = (group, model) else {
            self.create_tabs_model();
            return;
        };
        // Splice tab into group at index editorIndex
        let removed_tab = splice_remove(&mut model.borrow_mut().tabs, editor_index);

        // Index must no longer be valid so we return prematurely
        let Some(removed_tab) = removed_tab else {
            return;
        };

        // Update lookup
        let removed_id = removed_tab.borrow().id.clone();
        self.tab_info_lookup.borrow_mut().remove(&removed_id);

        self.send_tab_operation(TabModelOperationKind::TabClose, group_id, editor_index, &removed_tab, None);
    }

    /// Called when the active tab changes.
    fn on_did_tab_active_change(&self, group_id: GroupIdentifier, editor_index: i64) {
        let model = self.group_lookup.borrow().get(&group_id).cloned();
        let Some(model) = model else {
            return;
        };
        let tabs = model.borrow().tabs.clone();
        let Some(active_tab) = usize::try_from(editor_index).ok().and_then(|index| tabs.get(index)) else {
            errors::throw(Error::type_error("Cannot set properties of undefined (setting 'isActive')"));
        };
        active_tab.borrow_mut().is_active = true;
        // Clear the flag on the other tabs of the group. Otherwise a later `TAB_UPDATE`
        // re-sending one of those still-cached DTOs (label, dirty, pin or preview change)
        // would repoint the exthost at a tab that is no longer active.
        for tab in &tabs {
            if !Rc::ptr_eq(tab, active_tab) {
                tab.borrow_mut().is_active = false;
            }
        }
        // Send DTO update to the exthost
        self.send_tab_operation(TabModelOperationKind::TabUpdate, group_id, editor_index, active_tab, None);
    }

    /// Called when the dirty indicator on the tab changes.
    fn on_did_tab_dirty(&self, group_id: GroupIdentifier, editor_index: i64, editor: &EditorInputRef) {
        let tab_id = self.generate_tab_id(editor, group_id);
        let tab = self.tab_info_lookup.borrow().get(&tab_id).map(|tab_info| tab_info.tab.clone());
        // Something wrong with the model state so we rebuild
        let Some(tab) = tab else {
            self.log_service.error("Invalid model for dirty change, rebuilding");
            self.create_tabs_model();
            return;
        };
        tab.borrow_mut().is_dirty = editor.is_dirty();
        self.send_tab_operation(TabModelOperationKind::TabUpdate, group_id, editor_index, &tab, None);
    }

    /// Called when the tab is pinned/unpinned.
    fn on_did_tab_pin_change(&self, group_id: GroupIdentifier, editor_index: i64, editor: &EditorInputRef) {
        let tab_id = self.generate_tab_id(editor, group_id);
        let found = self.tab_info_lookup.borrow().get(&tab_id).map(|tab_info| (tab_info.group.clone(), tab_info.tab.clone()));
        // Something wrong with the model state so we rebuild
        let Some((group, tab)) = found else {
            self.log_service.error("Invalid model for sticky change, rebuilding");
            self.create_tabs_model();
            return;
        };
        // Whether or not the tab has the pin icon (internally it's called sticky)
        tab.borrow_mut().is_pinned = group.is_sticky(&EditorOrIndex::Index(editor_index));
        self.send_tab_operation(TabModelOperationKind::TabUpdate, group_id, editor_index, &tab, None);
    }

    /// Called when the tab is preview / unpreviewed.
    fn on_did_tab_preview_change(&self, group_id: GroupIdentifier, editor_index: i64, editor: &EditorInputRef) {
        let tab_id = self.generate_tab_id(editor, group_id);
        let found = self.tab_info_lookup.borrow().get(&tab_id).map(|tab_info| (tab_info.group.clone(), tab_info.tab.clone()));
        // Something wrong with the model state so we rebuild
        let Some((group, tab)) = found else {
            self.log_service.error("Invalid model for sticky change, rebuilding");
            self.create_tabs_model();
            return;
        };
        // Whether or not the tab has the pin icon (internally it's called pinned)
        tab.borrow_mut().is_preview = !group.is_pinned(&EditorOrIndex::Index(editor_index));
        self.send_tab_operation(TabModelOperationKind::TabUpdate, group_id, editor_index, &tab, None);
    }

    fn on_did_tab_move(&self, group_id: GroupIdentifier, editor_index: i64, old_editor_index: i64) {
        let model = self.group_lookup.borrow().get(&group_id).cloned();
        // Something wrong with the model state so we rebuild
        let Some(model) = model else {
            self.log_service.error("Invalid model for move change, rebuilding");
            self.create_tabs_model();
            return;
        };

        // Move tab from old index to new index
        let removed_tab = splice_remove(&mut model.borrow_mut().tabs, old_editor_index);
        let Some(removed_tab) = removed_tab else {
            return;
        };
        splice_insert(&mut model.borrow_mut().tabs, editor_index, removed_tab.clone());

        // Notify exthost of move
        self.send_tab_operation(TabModelOperationKind::TabMove, group_id, editor_index, &removed_tab, Some(old_editor_index));
    }

    /// `this._proxy.$acceptTabOperation({ kind, groupId, tabDto, index, oldIndex? })`, com uma cópia do DTO.
    fn send_tab_operation(&self, kind: TabModelOperationKind, group_id: GroupIdentifier, index: i64, tab: &Rc<RefCell<EditorTabDto>>, old_index: Option<i64>) {
        let tab_dto = tab.borrow().clone();
        self.proxy.rpc_accept_tab_operation(TabOperation {
            kind,
            index: index.max(0) as usize,
            tab_dto,
            group_id: group_id as u32,
            old_index: old_index.map(|old_index| old_index.max(0) as usize),
        });
    }

    /// Builds the model from scratch based on the current state of the editor service.
    fn create_tabs_model(&self) {
        let groups = self.editor_groups_service.groups();
        if groups.is_empty() {
            return; // skip this invalid state, it may happen when the entire editor area is transitioning to other state ("editor working sets")
        }

        let mut tab_group_model: Vec<SharedGroup> = Vec::new();
        self.group_lookup.borrow_mut().clear();
        self.tab_info_lookup.borrow_mut().clear();
        for group in groups {
            let mut tabs: Vec<Rc<RefCell<EditorTabDto>>> = Vec::new();
            for (editor_index, editor) in group.editors().iter().enumerate() {
                let tab = Rc::new(RefCell::new(self.build_tab_object(&group, editor, editor_index as i64)));
                tabs.push(tab.clone());
                // Add information about the tab to the lookup
                self.tab_info_lookup.borrow_mut().insert(self.generate_tab_id(editor, group.id()), TabInfo { group: group.clone(), tab, editor_input: editor.clone() });
            }
            let current_tab_group_model = Rc::new(RefCell::new(TabGroupModel {
                group_id: group.id(),
                is_active: group.id() == self.editor_groups_service.active_group().id(),
                view_column: editor_group_to_column(&*self.editor_groups_service, &GroupRef::Group(group.clone())),
                tabs,
            }));
            tab_group_model.push(current_tab_group_model.clone());
            self.group_lookup.borrow_mut().insert(group.id(), current_tab_group_model);
        }
        let dtos: Vec<EditorTabGroupDto> = tab_group_model.iter().map(|group| group.borrow().to_dto()).collect();
        *self.tab_group_model.borrow_mut() = tab_group_model;
        // notify the ext host of the new model
        self.proxy.rpc_accept_editor_tab_model(dtos);
    }

    /// The main handler for the tab events.
    fn update_tabs_model(&self, change_event: &EditorsChangeEvent) {
        let event: &GroupModelChangeEvent = &change_event.event;
        let group_id = change_event.group_id;
        if event.kind == GroupModelChangeKind::GroupActive {
            if group_id == self.editor_groups_service.active_group().id() {
                self.on_did_group_activate();
            }
            return;
        }
        // `switch` com queda de um `case` no seguinte quando a condição não vale; sem `case` correspondente cai no `default`.
        let start = UPDATE_CASES.iter().position(|kind| *kind == event.kind);
        for kind in start.map_or(&[][..], |start| &UPDATE_CASES[start..]) {
            match kind {
                GroupModelChangeKind::EditorLabel => {
                    if let (Some(editor), Some(editor_index)) = (&event.editor, event.editor_index) {
                        self.on_did_tab_label_change(group_id, editor, editor_index);
                        return;
                    }
                }
                GroupModelChangeKind::EditorOpen => {
                    if let (Some(editor), Some(editor_index)) = (&event.editor, event.editor_index) {
                        self.on_did_tab_open(group_id, editor, editor_index);
                        return;
                    }
                }
                GroupModelChangeKind::EditorClose => {
                    if let Some(editor_index) = event.editor_index {
                        self.on_did_tab_close(group_id, editor_index);
                        return;
                    }
                }
                GroupModelChangeKind::EditorActive => {
                    if let Some(editor_index) = event.editor_index {
                        self.on_did_tab_active_change(group_id, editor_index);
                        return;
                    }
                }
                GroupModelChangeKind::EditorDirty => {
                    if let (Some(editor), Some(editor_index)) = (&event.editor, event.editor_index) {
                        self.on_did_tab_dirty(group_id, editor_index, editor);
                        return;
                    }
                }
                GroupModelChangeKind::EditorSticky => {
                    if let (Some(editor), Some(editor_index)) = (&event.editor, event.editor_index) {
                        self.on_did_tab_pin_change(group_id, editor_index, editor);
                        return;
                    }
                }
                GroupModelChangeKind::EditorPin => {
                    if let (Some(editor), Some(editor_index)) = (&event.editor, event.editor_index) {
                        self.on_did_tab_preview_change(group_id, editor_index, editor);
                        return;
                    }
                }
                // Currently not exposed in the API; and multi-select state of editors is workbench-internal and not exposed in the
                // tabs API. Treat as no-op so we do not rebuild the entire model (which would invalidate any `vscode.Tab`
                // references the extension is currently holding).
                GroupModelChangeKind::EditorTransient | GroupModelChangeKind::EditorsSelection => return,
                GroupModelChangeKind::EditorMove => {
                    if is_group_editor_move_event(event)
                        && event.editor.is_some()
                        && let (Some(editor_index), Some(old_editor_index)) = (event.editor_index, event.old_editor_index)
                    {
                        self.on_did_tab_move(group_id, editor_index, old_editor_index);
                        return;
                    }
                }
                _ => {}
            }
        }
        // If it's not an optimized case we rebuild the tabs model from scratch
        self.create_tabs_model();
    }
}

impl IDisposable for MainThreadEditorTabs {
    fn dispose(&self) {
        self.group_lookup.borrow_mut().clear();
        self.tab_info_lookup.borrow_mut().clear();
        self.disposables.dispose();
    }
}

// #region Messages received from Ext Host

impl MainThreadEditorTabsShape for MainThreadEditorTabs {
    fn rpc_move_tab(&self, tab_id: &str, index: usize, view_column: EditorGroupColumn, preserve_focus: Option<bool>) {
        let group_id = column_to_editor_group(&*self.editor_groups_service, &*self.configuration_service, Some(view_column));
        let found = self.tab_info_lookup.borrow().get(tab_id).map(|tab_info| (tab_info.group.id(), tab_info.editor_input.clone()));
        let Some((source_group_id, editor_input)) = found else {
            errors::throw(Error::new(format!("Attempted to close tab with id {tab_id} which does not exist")));
        };
        let Some(source_group) = self.editor_groups_service.get_group(source_group_id) else {
            return;
        };
        // If group index is out of bounds then we make a new one that's to the right of the last group
        let target_group = if !self.group_lookup.borrow().contains_key(&group_id) {
            let mut direction = GroupDirection::Right;
            // Make sure we respect the user's preferred side direction
            if view_column == SIDE_GROUP as f64 {
                direction = preferred_side_by_side_group_direction(&*self.configuration_service);
            }
            let groups = self.editor_groups_service.groups();
            Some(self.editor_groups_service.add_group(groups.last().cloned().map(GroupRef::Group), direction))
        } else {
            self.editor_groups_service.get_group(group_id)
        };
        let Some(target_group) = target_group else {
            return;
        };

        // Similar logic to if index is out of bounds we place it at the end
        let index = if index > target_group.editors().len() { target_group.editors().len() } else { index };
        // Move the editor to the target group
        source_group.move_editor_with_options(
            &editor_input,
            &target_group,
            Some(EditorOptions { index: Some(index as f64), preserve_focus, ..EditorOptions::default() }),
        );
    }

    fn rpc_close_tab(&self, tab_ids: Vec<String>, preserve_focus: Option<bool>) -> Promise<bool> {
        let mut groups: Vec<(Rc<dyn IEditorGroup>, Vec<EditorInputRef>)> = Vec::new();
        for tab_id in &tab_ids {
            let found = self.tab_info_lookup.borrow().get(tab_id).map(|tab_info| (tab_info.group.clone(), tab_info.editor_input.clone()));
            // If not found skip
            let Some((group, editor_tab)) = found else {
                continue;
            };
            match groups.iter_mut().find(|(candidate, _)| std::ptr::addr_eq(Rc::as_ptr(candidate), Rc::as_ptr(&group))) {
                Some((_, group_editors)) => group_editors.push(editor_tab),
                None => groups.push((group, vec![editor_tab])),
            }
        }
        // Loop over keys of the groups map and call closeEditors
        async_fn(async move {
            let mut results: Vec<bool> = Vec::new();
            for (group, editors) in groups {
                results.push(group.close_editors(CloseEditorsTarget::Editors(editors), Some(CloseEditorOptions { preserve_focus, force: None })).await?);
            }
            // TODO @jrieken This isn't quite right how can we say true for some but not others?
            Ok(results.into_iter().all(|result| result))
        })
    }

    fn rpc_close_group(&self, group_ids: Vec<u32>, _preserve_focus: Option<bool>) -> Promise<bool> {
        let editor_groups_service = self.editor_groups_service.clone();
        async_fn(async move {
            let mut group_close_results: Vec<bool> = Vec::new();
            for group_id in group_ids {
                if let Some(group) = editor_groups_service.get_group(GroupIdentifier::from(group_id)) {
                    group_close_results.push(group.close_all_editors(None).await?);
                    // Make sure group is empty but still there before removing it
                    if group.count() == 0 && editor_groups_service.get_group(group.id()).is_some() {
                        editor_groups_service.remove_group(GroupRef::Group(group));
                    }
                }
            }
            Ok(group_close_results.into_iter().all(|result| result))
        })
    }
}

// #endregion
