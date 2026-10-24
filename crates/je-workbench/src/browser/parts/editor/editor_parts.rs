//! Porte de `src/vs/workbench/browser/parts/editor/editorParts.ts`: o [`EditorParts`], a coleção de partes de editor e o
//! `IEditorGroupsService` real. Delega a cada parte (a do grupo dado, ou a ativa), junta os eventos das partes, guarda a
//! lista das partes mais recentemente ativas, os conjuntos de trabalho (`saveWorkingSet`, `getWorkingSets`,
//! `applyWorkingSet`, `deleteWorkingSet`), `applyState`, o estado persistido (`editorparts.state` e
//! `editorparts.modalState`) e as chaves de contexto por grupo (`bind`, `registerContextKeyProvider`).
//!
//! Divergências:
//! - **`MultiWindowParts` e `Component`.** Não portados como tipos: as partes são a lista `parts` (ordem de registro, como o
//!   `Set` do original), `registerPart` devolve o disposable que a retira, e o `Component` dá a este tipo o `Memento`
//!   (`workbench.editorParts`) e o ouvinte de `onWillSaveState` que chama [`EditorParts::save_state`] e grava o memento.
//!   `activePart` é a parte mais recentemente ativa (o primeiro de `mostRecentActiveParts`, que o foco de cada parte atualiza):
//!   sem janelas do DOM, `getPartByDocument(getActiveDocument())` é essa lista. `getPart(element)`, `getPartByDocument` e
//!   `createEditorDropTarget` (que usam o DOM) ficam com o padrão que lança do contrato.
//! - **Sem janela auxiliar nem modal.** `AuxiliaryEditorPart`, `ModalEditorPart` e o `IAuxiliaryWindowService` não existem
//!   no porte. `createAuxiliaryEditorPart` e `createModalEditorPart` devolvem uma promessa rejeitada (o `restoreState` do
//!   original usa `Promise.allSettled` sobre a primeira), `activeModalEditorPart` é sempre `None`, e `createState` devolve
//!   `auxiliary` vazio (nenhuma parte tem janela auxiliar). Quando uma parte auxiliar existir ela entra por
//!   [`EditorParts::register_part`], cujo segundo argumento é o `close()` do `IAuxiliaryEditorPart` que o `applyState` chama.
//!   O estado modal (`maximized`, `size`, `position`, `sidebarWidth`, `sidebarHidden`) é lido e gravado no memento do perfil
//!   como no original, mas só o que veio do memento (nenhuma parte modal o atualiza).
//! - **A parte principal.** `createMainEditorPart()` (o gancho que o `TestEditorParts` sobrescreve) não existe: o
//!   `TestEditorParts` embrulha a parte principal que este tipo cria ([`EditorParts::main_editor_part`]).
//! - **Instanciação escopada.** `getScopedInstantiationService` devolve o próprio `InstantiationService` para toda parte: o
//!   filho com o `IEditorService` e o `IStatusbarService` escopados (`createScoped`) não existe.
//! - **Grupos como `Rc<dyn IEditorGroup>`.** O contrato do porte devolve o grupo como `dyn IEditorGroup`, e a identidade de
//!   grupo é o `id`. As chaves de contexto trabalham sobre o `dyn IEditorGroup` (`id`, `scopedContextKeyService`,
//!   `onDidActiveEditorChange`), que é tudo que o original lê do grupo.
//! - **Estado dos conjuntos de trabalho.** Um conjunto salvo com forma inválida no armazenamento é ignorado (o original faria
//!   `JSON.parse` sem checar a forma).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{async_fn, await_maybe};
use je_base::dynamic::{Dynamic, Truthy, json_stringify};
use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::json::Value;
use je_base::lifecycle::{DisposableMap, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::uuid::generate_uuid;
use je_editor::core_2d::dimension::Dimension;
use je_instantiation::InstantiationService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService, RawContextKey};
use je_platform::opener::editor_types::{AuxiliaryEditorOptions, ModalEditorPartOptions};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;
use je_ui::Ui;

use crate::browser::parts::editor::editor::EditorPartsView;
use crate::browser::parts::editor::editor_group_view::EditorGroupView;
use crate::browser::parts::editor::editor_part::{ApplyStateTarget, EditorPart, EditorPartUiState, number_of, prop, state_from_dynamic, state_to_dynamic};
use crate::common::contextkeys::editor_part_modal_visible_context;
use crate::common::memento::Memento;
use crate::common::editor::GroupIdentifier;
use crate::services::editor::editor_groups_service::{
    EditorGroupActivationEvent, EditorGroupContextKeyProvider, EditorGroupLayout, EditorPartOptions, EditorPartOptionsChangeEvent, EditorWorkingSet,
    EditorWorkingSetOptions, FindGroupScope, GroupActivationReason, GroupDirection, GroupLocation, GroupOrientation, GroupRef, GroupsArrangement, GroupsOrder,
    IAuxiliaryEditorPart, IEditorGroup, IEditorGroupsService, IEditorPart, IEditorSideGroup, IModalEditorPart, MergeGroupOptions, WorkingSetTarget,
};
use crate::services::layout::layout_service::MAIN_WINDOW;

/// `Parts.EDITOR_PART`: o id da parte principal (e do memento dela).
const MAIN_EDITOR_PART_ID: &str = "workbench.parts.editor";
/// O id do memento do `EditorParts` (`super('workbench.editorParts', ...)`).
const EDITOR_PARTS_ID: &str = "workbench.editorParts";
const EDITOR_PARTS_UI_STATE_STORAGE_KEY: &str = "editorparts.state";
const MODAL_EDITOR_STATE_STORAGE_KEY: &str = "editorparts.modalState";
const EDITOR_WORKING_SETS_STORAGE_KEY: &str = "editor.workingSets";

/// `IEditorPartsUIState`: o estado das partes auxiliares (`auxiliary`, cada entrada com o estado da parte e as opções da
/// janela, guardadas como o objeto do memento) e a ordem de uso recente (`mru`, índices em `parts`). O estado da parte
/// principal é gerido por ela.
#[derive(Clone)]
pub struct EditorPartsUiState {
    pub auxiliary: Vec<Dynamic>,
    pub mru: Vec<usize>,
}

/// `IEditorPartsUIState | 'empty'`.
#[derive(Clone)]
pub enum PartsStateTarget {
    State(EditorPartsUiState),
    Empty,
}

/// `IEditorWorkingSetState`.
#[derive(Clone)]
struct WorkingSetState {
    id: String,
    name: String,
    main: EditorPartUiState,
    auxiliary: EditorPartsUiState,
}

/// O estado modal (`modalEditorMaximized`, `modalEditorSize`, `modalEditorPosition`, `modalEditorSidebarWidth` e
/// `modalEditorSidebarHidden`).
#[derive(Clone, Default)]
struct ModalEditorState {
    maximized: bool,
    size: Option<(f64, f64)>,
    position: Option<(f64, f64)>,
    sidebar_width: Option<f64>,
    sidebar_hidden: Option<bool>,
}

fn text_of(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn bool_of(value: &Dynamic) -> Option<bool> {
    match value {
        Dynamic::Bool(flag) => Some(*flag),
        _ => None,
    }
}

fn pair_of(value: &Dynamic, first: &str, second: &str) -> Option<(f64, f64)> {
    Some((number_of(&prop(value, first))?, number_of(&prop(value, second))?))
}

fn list_of(value: &Dynamic) -> Option<Vec<Dynamic>> {
    match value {
        Dynamic::Array(items) => Some(items.to_vec()),
        _ => None,
    }
}

fn parts_state_to_dynamic(state: &EditorPartsUiState) -> Dynamic {
    Dynamic::object([
        ("auxiliary", Dynamic::array(state.auxiliary.iter().cloned())),
        ("mru", Dynamic::array(state.mru.iter().map(|index| Dynamic::Number(*index as f64)))),
    ])
}

fn parts_state_from_dynamic(value: &Dynamic) -> Option<EditorPartsUiState> {
    Some(EditorPartsUiState {
        auxiliary: list_of(&prop(value, "auxiliary"))?,
        mru: list_of(&prop(value, "mru"))?
            .iter()
            .filter_map(number_of)
            .filter(|index| *index >= 0.0 && index.fract() == 0.0)
            .map(|index| index as usize)
            .collect(),
    })
}

fn working_set_to_dynamic(working_set: &WorkingSetState) -> Dynamic {
    Dynamic::object([
        ("id", Dynamic::string(&working_set.id)),
        ("name", Dynamic::string(&working_set.name)),
        ("main", state_to_dynamic(&working_set.main)),
        ("auxiliary", parts_state_to_dynamic(&working_set.auxiliary)),
    ])
}

fn working_set_from_dynamic(value: &Dynamic) -> Option<WorkingSetState> {
    Some(WorkingSetState {
        id: text_of(&prop(value, "id"))?,
        name: text_of(&prop(value, "name"))?,
        main: state_from_dynamic(&prop(value, "main"))?,
        auxiliary: parts_state_from_dynamic(&prop(value, "auxiliary"))?,
    })
}

fn modal_state_from_dynamic(value: &Dynamic) -> ModalEditorState {
    ModalEditorState {
        maximized: bool_of(&prop(value, "maximized")).unwrap_or(false),
        size: pair_of(&prop(value, "size"), "width", "height"),
        position: pair_of(&prop(value, "position"), "left", "top"),
        sidebar_width: number_of(&prop(value, "sidebarWidth")),
        sidebar_hidden: bool_of(&prop(value, "sidebarHidden")),
    }
}

fn modal_state_to_dynamic(state: &ModalEditorState) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> = vec![("maximized", Dynamic::Bool(state.maximized))];
    if let Some((width, height)) = state.size {
        entries.push(("size", Dynamic::object([("width", Dynamic::Number(width)), ("height", Dynamic::Number(height))])));
    }
    if let Some((left, top)) = state.position {
        entries.push(("position", Dynamic::object([("left", Dynamic::Number(left)), ("top", Dynamic::Number(top))])));
    }
    if let Some(sidebar_width) = state.sidebar_width {
        entries.push(("sidebarWidth", Dynamic::Number(sidebar_width)));
    }
    if let Some(sidebar_hidden) = state.sidebar_hidden {
        entries.push(("sidebarHidden", Dynamic::Bool(sidebar_hidden)));
    }

    Dynamic::object(entries)
}

/// `getPart` não achou o que o original acharia: a parte auxiliar ainda não existe.
fn unsupported(what: &str) -> Error {
    Error::new(format!("{what} is not supported yet: the auxiliary and modal editor parts are not ported"))
}

/// O `IContextKey` que o `bind` do original devolve: escreve no contexto escopado do grupo e, se o grupo é o ativo, também no
/// global.
struct BoundContextKey {
    parts: Weak<EditorParts>,
    group_id: GroupIdentifier,
    global: Rc<dyn IContextKey>,
    scoped: Rc<dyn IContextKey>,
}

impl BoundContextKey {
    fn is_active_group(&self) -> bool {
        self.parts.upgrade().is_some_and(|parts| parts.active_part().active_group().id() == self.group_id)
    }
}

impl IContextKey for BoundContextKey {
    fn set(&self, value: Dynamic) {
        if self.is_active_group() {
            self.global.set(value.clone());
        }
        self.scoped.set(value);
    }

    fn reset(&self) {
        if self.is_active_group() {
            self.global.reset();
        }
        self.scoped.reset();
    }

    fn get(&self) -> Dynamic {
        self.scoped.get()
    }
}

/// Os campos mutáveis do `EditorParts`. Nenhum método segura o empréstimo ao chamar código externo ou disparar evento.
struct PartsState {
    /// `_parts`: na ordem de registro.
    parts: Vec<Rc<EditorPart>>,
    /// O `close()` das partes auxiliares (a principal não tem).
    part_closers: Vec<(Rc<EditorPart>, Rc<dyn Fn() -> bool>)>,
    most_recent_active_parts: Vec<Rc<EditorPart>>,
    is_ready: bool,
    editor_working_sets: Vec<WorkingSetState>,
    modal: ModalEditorState,
    global_context_keys: HashMap<String, Rc<dyn IContextKey>>,
    scoped_context_keys: HashMap<GroupIdentifier, HashMap<String, Rc<dyn IContextKey>>>,
    /// `contextKeyProviders`, na ordem de registro.
    context_key_providers: Vec<(String, Rc<EditorGroupContextKeyProvider>)>,
    registered_context_keys: HashMap<GroupIdentifier, HashMap<String, Rc<dyn IContextKey>>>,
    disposed: bool,
}

/// `EditorParts`.
pub struct EditorParts {
    this: Weak<EditorParts>,
    instantiation_service: Rc<InstantiationService>,
    storage_service: Rc<dyn StorageService>,
    context_key_service: Rc<dyn IContextKeyService>,
    memento: Memento,
    main_part: Rc<EditorPart>,
    modal_editor_visible_context: Rc<dyn IContextKey>,
    state: RefCell<PartsState>,
    store: Rc<DisposableStore>,
    context_key_provider_disposables: Rc<DisposableMap<GroupIdentifier, dyn IDisposable>>,

    on_did_active_group_change: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_did_add_group: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_did_remove_group: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_did_move_group: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_did_activate_group: Rc<Emitter<EditorGroupActivationEvent>>,
    on_did_change_group_index: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_did_change_group_locked: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_did_change_group_maximized: Rc<Emitter<bool>>,
    on_did_create_auxiliary_editor_part: Rc<Emitter<Rc<dyn IAuxiliaryEditorPart>>>,
    when_ready_promise: DeferredPromise<()>,
    when_restored_promise: DeferredPromise<()>,
}

impl EditorParts {
    /// `new EditorParts(...)`: cria a parte principal (que o chamador ainda tem de criar com [`EditorPart::create`] e pôr em
    /// layout), registra-a e começa a restaurar as partes.
    pub fn new(
        ui: &Ui,
        instantiation_service: Rc<InstantiationService>,
        configuration_service: Rc<dyn IConfigurationService>,
        storage_service: Rc<dyn StorageService>,
        context_key_service: Rc<dyn IContextKeyService>,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let memento = Memento::new(EDITOR_PARTS_ID, storage_service.clone());
        let modal_editor_visible_context = editor_part_modal_visible_context().bind_to(context_key_service.as_ref());

        let editor_working_sets = match storage_service.get_object(EDITOR_WORKING_SETS_STORAGE_KEY, StorageScope::Workspace, None) {
            Some(Value::Array(items)) => items.iter().filter_map(|item| working_set_from_dynamic(&Dynamic::from_json(item))).collect(),
            _ => Vec::new(),
        };

        let profile_memento = memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
        let modal = profile_memento
            .get(&JsString::from(MODAL_EDITOR_STATE_STORAGE_KEY))
            .filter(Truthy::is_truthy)
            .map(|state| modal_state_from_dynamic(&state))
            .unwrap_or_default();

        let main_part = EditorPart::new(
            ui,
            instantiation_service.clone(),
            configuration_service,
            storage_service.clone(),
            context_key_service.as_ref(),
            MAIN_EDITOR_PART_ID,
            "",
            MAIN_WINDOW,
        );
        store.add_rc(main_part.clone());

        let context_key_provider_disposables = store.add_rc(DisposableMap::new());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            instantiation_service,
            storage_service,
            context_key_service,
            memento,
            main_part: main_part.clone(),
            modal_editor_visible_context,
            state: RefCell::new(PartsState {
                parts: Vec::new(),
                part_closers: Vec::new(),
                most_recent_active_parts: Vec::new(),
                is_ready: false,
                editor_working_sets,
                modal,
                global_context_keys: HashMap::new(),
                scoped_context_keys: HashMap::new(),
                context_key_providers: Vec::new(),
                registered_context_keys: HashMap::new(),
                disposed: false,
            }),
            context_key_provider_disposables,
            on_did_active_group_change: store.add_rc(Emitter::new()),
            on_did_add_group: store.add_rc(Emitter::new()),
            on_did_remove_group: store.add_rc(Emitter::new()),
            on_did_move_group: store.add_rc(Emitter::new()),
            on_did_activate_group: store.add_rc(Emitter::new()),
            on_did_change_group_index: store.add_rc(Emitter::new()),
            on_did_change_group_locked: store.add_rc(Emitter::new()),
            on_did_change_group_maximized: store.add_rc(Emitter::new()),
            on_did_create_auxiliary_editor_part: store.add_rc(Emitter::new()),
            when_ready_promise: DeferredPromise::new(),
            when_restored_promise: DeferredPromise::new(),
            store,
        });

        let parts_view_rc: Rc<dyn EditorPartsView> = this.clone();
        let parts_view: Weak<dyn EditorPartsView> = Rc::downgrade(&parts_view_rc);
        main_part.set_parts_view(parts_view);
        this.register_part(main_part.clone(), None);

        this.state.borrow_mut().most_recent_active_parts = vec![main_part];

        this.restore_parts();
        this.register_listeners();

        this
    }

    fn rc(&self) -> Rc<EditorParts> {
        self.this.upgrade().expect("o EditorParts está vivo enquanto é usado")
    }

    fn register_listeners(&self) {
        // `Component`: ask the component to persist state into the memento, then save the memento into storage
        let weak = self.this.clone();
        self.store.add(self.storage_service.on_will_save_state().listen(move |_| {
            if let Some(parts) = weak.upgrade() {
                parts.save_state();
                parts.memento.save_memento();
            }
        }));

        let weak = self.this.clone();
        self.store.add(self.memento.on_did_change_value(StorageScope::Workspace, &self.store).listen(move |event| {
            let Some(parts) = weak.upgrade() else { return };

            // `onDidChangeMementoState`
            if event.external && event.scope == StorageScope::Workspace {
                parts.memento.reload_memento(event.scope);

                if let Some(state) = parts.load_state() {
                    let _ = parts.apply_state(PartsStateTarget::State(state));
                }
            }
        }));

        let weak = self.this.clone();
        let _ = self.when_ready_promise.promise.then(move |()| {
            if let Some(parts) = weak.upgrade() {
                parts.register_groups_context_key_listeners();
            }
        });
    }

    /// A parte principal (`mainPart`), como o tipo concreto.
    pub fn main_editor_part(&self) -> Rc<EditorPart> {
        self.main_part.clone()
    }

    // region: partes

    /// `parts`.
    fn parts(&self) -> Vec<Rc<EditorPart>> {
        self.state.borrow().parts.clone()
    }

    /// `activePart`: a parte mais recentemente ativa (ver o cabeçalho).
    fn active_part(&self) -> Rc<EditorPart> {
        self.state.borrow().most_recent_active_parts.first().cloned().unwrap_or_else(|| self.main_part.clone())
    }

    /// `getPart(group)`: a parte que contém o grupo (a principal quando só há uma parte).
    fn part_of(&self, group: &GroupRef) -> Rc<EditorPart> {
        let parts = self.parts();
        if parts.len() > 1 {
            let id = match group {
                GroupRef::Id(id) => *id,
                GroupRef::Group(group) => group.id(),
            };

            if let Some(part) = parts.iter().find(|part| part.has_group(id)) {
                return part.clone();
            }
        }

        self.main_part.clone()
    }

    /// `registerPart(part)`: registra a parte e os ouvintes dos eventos dela. `close` é o `close()` de uma parte auxiliar (a
    /// principal não tem). O disposable que volta retira a parte.
    pub fn register_part(&self, part: Rc<EditorPart>, close: Option<Rc<dyn Fn() -> bool>>) -> DisposableRef {
        let disposables = self.store.add_rc(DisposableStore::new());

        {
            let mut state = self.state.borrow_mut();
            state.parts.push(part.clone());
            if let Some(close) = close {
                state.part_closers.push((part.clone(), close));
            }
        }
        let (weak, registered_part) = (self.this.clone(), part.clone());
        disposables.add(to_disposable(move || {
            if let Some(parts) = weak.upgrade() {
                parts.unregister_part(&registered_part);
            }
        }));

        self.register_editor_part_listeners(&part, &disposables);

        disposables
    }

    fn unregister_part(&self, part: &Rc<EditorPart>) {
        {
            let mut state = self.state.borrow_mut();
            state.parts.retain(|candidate| !Rc::ptr_eq(candidate, part));
            state.part_closers.retain(|(candidate, _)| !Rc::ptr_eq(candidate, part));
        }

        // Notify all parts about a groups label change given it is computed based on the index
        for (index, part) in self.parts().iter().enumerate() {
            if Rc::ptr_eq(part, &self.main_part) {
                continue;
            }

            part.notify_groups_label_change(&Self::groups_label(index));
        }
    }

    fn register_editor_part_listeners(&self, part: &Rc<EditorPart>, disposables: &Rc<DisposableStore>) {
        let (weak, weak_part) = (self.this.clone(), Rc::downgrade(part));

        disposables.add(part.on_did_focus().listen({
            let (weak, weak_part) = (weak.clone(), weak_part.clone());
            move |_| {
                let (Some(parts), Some(part)) = (weak.upgrade(), weak_part.upgrade()) else { return };

                parts.do_update_most_recent_active(&part, true);

                if parts.parts().len() > 1 {
                    // Either main or auxiliary editor part got focus which we have to treat as a group change event.
                    parts.on_did_active_group_change.fire(parts.active_part().active_group());
                }
            }
        }));
        disposables.add(to_disposable({
            let (weak, weak_part) = (weak.clone(), weak_part.clone());
            move || {
                let (Some(parts), Some(part)) = (weak.upgrade(), weak_part.upgrade()) else { return };

                parts.do_update_most_recent_active(&part, false);

                if part.window_id() != MAIN_WINDOW {
                    // An auxiliary editor part is closing which we have to treat as group change event for the next editor
                    // part that becomes active.
                    parts.on_did_active_group_change.fire(parts.active_part().active_group());
                }
            }
        }));

        macro_rules! forward {
            ($source:ident, $target:ident) => {
                disposables.add(part.$source().listen({
                    let weak = weak.clone();
                    move |value| {
                        if let Some(parts) = weak.upgrade() {
                            parts.$target.fire(value.clone());
                        }
                    }
                }))
            };
        }

        forward!(on_did_change_active_group, on_did_active_group_change);
        forward!(on_did_add_group, on_did_add_group);
        forward!(on_did_remove_group, on_did_remove_group);
        forward!(on_did_move_group, on_did_move_group);
        disposables.add(part.on_did_activate_group().listen({
            let (weak, weak_part) = (weak.clone(), weak_part);
            move |event| {
                let Some(parts) = weak.upgrade() else { return };

                // A part-close activation means a modal or auxiliary editor part is closing and another part is being made the
                // active one. Update our MRU eagerly here so that downstream queries during the close flow see the new active
                // part instead of the closing one which has not yet been unregistered.
                if event.reason == GroupActivationReason::PartClose
                    && let Some(part) = weak_part.upgrade()
                {
                    parts.do_update_most_recent_active(&part, true);
                }

                parts.on_did_activate_group.fire(event.clone());
            }
        }));
        forward!(on_did_change_group_maximized, on_did_change_group_maximized);

        forward!(on_did_change_group_index, on_did_change_group_index);
        forward!(on_did_change_group_locked, on_did_change_group_locked);
    }

    fn do_update_most_recent_active(&self, part: &Rc<EditorPart>, make_most_recently_active: bool) {
        let mut state = self.state.borrow_mut();

        // Remove from MRU list
        state.most_recent_active_parts.retain(|candidate| !Rc::ptr_eq(candidate, part));

        // Add to front as needed
        if make_most_recently_active {
            state.most_recent_active_parts.insert(0, part.clone());
        }
    }

    /// `getGroupsLabel(index)`.
    fn groups_label(index: usize) -> String {
        localize("groupLabel", "Window {0}", &[&(index + 1).to_string()])
    }

    // endregion

    // region: ciclo de vida e estado

    /// `restoreParts()`.
    fn restore_parts(&self) {
        let parts = self.rc();

        let _ = async_fn(async move {
            // Join on the main part being ready to pick the right moment to begin restoring. The main part is automatically
            // being created as part of the overall startup process.
            parts.main_part.when_ready().await?;

            // Only attempt to restore auxiliary editor parts when the main part did restore. It is possible that restoring
            // was not attempted because specific editors were opened.
            if parts.main_part.will_restore_state()
                && let Some(state) = parts.load_state()
            {
                parts.restore_state(state).await?;
            }

            let most_recent_active_part = parts.state.borrow().most_recent_active_parts.first().cloned();
            if let Some(part) = most_recent_active_part {
                IEditorGroup::focus(&*part.active_group());
            }

            parts.state.borrow_mut().is_ready = true;
            parts.when_ready_promise.complete(());

            // Await restored
            Promise::all_settled(parts.parts().iter().map(|part| part.when_restored()).collect()).await?;
            parts.when_restored_promise.complete(());

            Ok(())
        });
    }

    /// `loadState()`.
    fn load_state(&self) -> Option<EditorPartsUiState> {
        let workspace_memento = self.memento.get_memento(StorageScope::Workspace, StorageTarget::User);

        workspace_memento.get(&JsString::from(EDITOR_PARTS_UI_STATE_STORAGE_KEY)).filter(Truthy::is_truthy).and_then(|state| parts_state_from_dynamic(&state))
    }

    /// `saveState()`.
    pub fn save_state(&self) {
        let workspace_memento = self.memento.get_memento(StorageScope::Workspace, StorageTarget::User);

        let state = self.create_state();
        let key = JsString::from(EDITOR_PARTS_UI_STATE_STORAGE_KEY);
        if state.auxiliary.is_empty() {
            workspace_memento.delete(&key);
        } else {
            workspace_memento.set(&key, parts_state_to_dynamic(&state));
        }

        self.save_modal_state();
    }

    /// `saveModalState()`, sem a captura da parte modal aberta (que não existe).
    fn save_modal_state(&self) {
        let profile_memento = self.memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
        let key = JsString::from(MODAL_EDITOR_STATE_STORAGE_KEY);
        let modal = self.state.borrow().modal.clone();

        // Only persist when there is meaningful state to restore. When all values are at their defaults (not maximized, no
        // custom size or position), we delete the key to avoid storing unnecessary data.
        if modal.maximized || modal.size.is_some() || modal.position.is_some() || modal.sidebar_width.is_some_and(|width| width != 0.0) || modal.sidebar_hidden == Some(true) {
            profile_memento.set(&key, modal_state_to_dynamic(&modal));
        } else {
            profile_memento.delete(&key);
        }
    }

    /// `createState()`: sem janelas auxiliares, só a ordem de uso recente das partes.
    fn create_state(&self) -> EditorPartsUiState {
        let state = self.state.borrow();

        EditorPartsUiState {
            auxiliary: Vec::new(),
            mru: state.most_recent_active_parts.iter().filter_map(|part| state.parts.iter().position(|candidate| Rc::ptr_eq(candidate, part))).collect(),
        }
    }

    /// `restoreState(state)`.
    fn restore_state(&self, state: EditorPartsUiState) -> Promise<()> {
        let parts = self.rc();

        async_fn(async move {
            if !state.auxiliary.is_empty() {
                // Create auxiliary editor parts
                let auxiliary_editor_part_promises: Vec<Promise<Rc<dyn IEditorPart>>> =
                    state.auxiliary.iter().map(|_| Promise::rejected(unsupported("createAuxiliaryEditorPart"))).collect();

                // Await creation
                Promise::all_settled(auxiliary_editor_part_promises).await?;

                // Update MRU list
                let current_parts = parts.parts();
                let most_recent_active_parts: Vec<Rc<EditorPart>> = state.mru.iter().filter_map(|index| current_parts.get(*index).cloned()).collect();
                parts.state.borrow_mut().most_recent_active_parts =
                    if most_recent_active_parts.len() == current_parts.len() && state.mru.len() == current_parts.len() { most_recent_active_parts } else { current_parts.clone() };

                // Await ready
                Promise::all_settled(current_parts.iter().map(|part| part.when_ready()).collect()).await?;
            }

            Ok(())
        })
    }

    /// `applyState(state)`: fecha as partes auxiliares (movendo o que sobra para a principal) e restaura o estado das
    /// auxiliares, a menos que o estado seja vazio. Devolve `false` se o fechamento foi vetado.
    fn apply_state(&self, state: PartsStateTarget) -> Promise<bool> {
        let parts = self.rc();

        async_fn(async move {
            // Before closing windows, try to close as many editors as possible, but skip over those that would trigger a
            // dialog (for example when being dirty). This is to be able to have them merge into the main part.
            for part in parts.parts() {
                if Rc::ptr_eq(&part, &parts.main_part) {
                    continue; // main part takes care on its own
                }

                for group in part.get_group_views(GroupsOrder::MostRecentlyActive) {
                    await_maybe(MaybePromise::Value(IEditorGroup::close_all_editors_excluding_confirming(&*group, Some(true)))).await?;
                }

                let close = parts.state.borrow().part_closers.iter().find(|(candidate, _)| Rc::ptr_eq(candidate, &part)).map(|(_, close)| close.clone());
                let closed = match close {
                    Some(close) => close(), // will move remaining editors to main part
                    None => throw(Error::new("The editor part is not an auxiliary editor part")),
                };
                if !closed {
                    return Ok(false); // this indicates that closing was vetoed
                }
            }

            // Restore auxiliary state unless we are in an empty state
            if let PartsStateTarget::State(state) = state {
                parts.restore_state(state).await?;
            }

            Ok(true)
        })
    }

    // endregion

    // region: conjuntos de trabalho

    fn index_of_working_set(&self, working_set: &EditorWorkingSet) -> Option<usize> {
        self.state.borrow().editor_working_sets.iter().position(|candidate| candidate.id == working_set.id)
    }

    fn save_working_sets(&self) {
        let working_sets = Dynamic::array(self.state.borrow().editor_working_sets.iter().map(working_set_to_dynamic));

        if let Some(serialized) = json_stringify(&working_sets) {
            self.storage_service.store(EDITOR_WORKING_SETS_STORAGE_KEY, StorageValue::String(serialized), StorageScope::Workspace, StorageTarget::Machine);
        }
    }

    // endregion

    // region: grupos de todas as partes

    /// `groups` e `getGroups(order)` com as vistas de grupo de todas as partes.
    fn all_group_views(&self, order: GroupsOrder) -> Vec<Rc<EditorGroupView>> {
        let parts = self.parts();
        if parts.len() > 1 {
            let ordered: Vec<Rc<EditorPart>> = match order {
                // we currently do not have a way to compute by appearance over multiple windows
                GroupsOrder::GridAppearance | GroupsOrder::CreationTime => parts,
                GroupsOrder::MostRecentlyActive => {
                    // always ensure all parts are included
                    let mut distinct: Vec<Rc<EditorPart>> = Vec::new();
                    for part in self.state.borrow().most_recent_active_parts.iter().chain(parts.iter()) {
                        if !distinct.iter().any(|candidate| Rc::ptr_eq(candidate, part)) {
                            distinct.push(part.clone());
                        }
                    }

                    distinct
                }
            };

            return ordered.iter().flat_map(|part| part.get_group_views(order)).collect();
        }

        self.main_part.get_group_views(order)
    }

    fn all_groups(&self, order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>> {
        self.all_group_views(order).iter().map(|group| group.as_group()).collect()
    }

    /// `assertGroupView(group)`.
    fn assert_group(&self, group: &GroupRef) -> Rc<dyn IEditorGroup> {
        match group {
            GroupRef::Id(identifier) => IEditorGroupsService::get_group(self, *identifier).unwrap_or_else(|| throw(Error::new("Invalid editor group provided!"))),
            GroupRef::Group(group) => group.clone(),
        }
    }

    /// `restoreGroup(group)`.
    pub fn restore_group(&self, group: GroupRef) -> Rc<dyn IEditorGroup> {
        self.part_of(&group).restore_group(&group).as_group()
    }

    // endregion

    // region: chaves de contexto dos grupos

    fn register_groups_context_key_listeners(&self) {
        let weak = self.this.clone();
        self.store.add(self.on_did_active_group_change.event().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(parts) = weak.upgrade() {
                    parts.update_global_context_keys();
                }
            }
        }));

        for group in self.all_groups(GroupsOrder::CreationTime) {
            self.register_group_context_key_providers_listeners(&group);
        }

        self.store.add(self.on_did_add_group.event().listen({
            let weak = weak.clone();
            move |group| {
                if let Some(parts) = weak.upgrade() {
                    parts.register_group_context_key_providers_listeners(group);
                }
            }
        }));
        self.store.add(self.on_did_remove_group.event().listen(move |group| {
            let Some(parts) = weak.upgrade() else { return };

            {
                let mut state = parts.state.borrow_mut();
                state.scoped_context_keys.remove(&group.id());
                state.registered_context_keys.remove(&group.id());
            }
            parts.context_key_provider_disposables.delete_and_dispose(&group.id());
        }));
    }

    fn update_global_context_keys(&self) {
        let (active_group_scoped_context_keys, global_context_keys) = {
            let state = self.state.borrow();
            let scoped = state.scoped_context_keys.get(&self.active_part().active_group().id()).cloned();

            (scoped, state.global_context_keys.clone())
        };
        let Some(active_group_scoped_context_keys) = active_group_scoped_context_keys else { return };

        for (key, global_context_key) in global_context_keys {
            match active_group_scoped_context_keys.get(&key) {
                Some(scoped_context_key) => global_context_key.set(scoped_context_key.get()),
                None => global_context_key.reset(),
            }
        }
    }

    /// `bind(contextKey, group)`.
    fn bind_group(&self, context_key: &RawContextKey, group: &Rc<dyn IEditorGroup>) -> Rc<dyn IContextKey> {
        let key = context_key.key().to_string();

        // Ensure we only bind to the same context key once globaly
        let existing_global = self.state.borrow().global_context_keys.get(&key).cloned();
        let global_context_key = existing_global.unwrap_or_else(|| {
            let global_context_key = context_key.bind_to(self.context_key_service.as_ref());
            self.state.borrow_mut().global_context_keys.insert(key.clone(), global_context_key.clone());

            global_context_key
        });

        // Ensure we only bind to the same context key once per group
        let existing_scoped = self.state.borrow().scoped_context_keys.get(&group.id()).and_then(|keys| keys.get(&key)).cloned();
        let scoped_context_key = existing_scoped.unwrap_or_else(|| {
            let scoped_context_key = context_key.bind_to(group.scoped_context_key_service().as_ref());
            self.state.borrow_mut().scoped_context_keys.entry(group.id()).or_default().insert(key.clone(), scoped_context_key.clone());

            scoped_context_key
        });

        Rc::new(BoundContextKey { parts: self.this.clone(), group_id: group.id(), global: global_context_key, scoped: scoped_context_key })
    }

    fn register_group_context_key_providers_listeners(&self, group: &Rc<dyn IEditorGroup>) {
        // Update context keys from providers for the group when its active editor changes
        let (weak, weak_group) = (self.this.clone(), Rc::downgrade(group));
        let disposable = group.on_did_active_editor_change().listen(move |_| {
            let (Some(parts), Some(group)) = (weak.upgrade(), weak_group.upgrade()) else { return };

            let providers: Vec<Rc<EditorGroupContextKeyProvider>> = parts.state.borrow().context_key_providers.iter().map(|(_, provider)| provider.clone()).collect();
            for context_key_provider in &providers {
                parts.update_registered_context_key(&group, context_key_provider);
            }
        });

        self.context_key_provider_disposables.set(group.id(), disposable, false);
    }

    fn update_registered_context_key(&self, group: &Rc<dyn IEditorGroup>, provider: &EditorGroupContextKeyProvider) {
        // Get the group scoped context keys for the provider. If the providers context key has not yet been bound to the group,
        // do so now.
        let key = provider.context_key.key().to_string();
        let existing = self.state.borrow().registered_context_keys.get(&group.id()).and_then(|keys| keys.get(&key)).cloned();
        let scoped_registered_context_key = existing.unwrap_or_else(|| {
            let bound = self.bind_group(&provider.context_key, group);
            self.state.borrow_mut().registered_context_keys.entry(group.id()).or_default().insert(key.clone(), bound.clone());

            bound
        });

        // Set the context key value for the group context
        scoped_registered_context_key.set((provider.get_group_context_key_value)(group));
    }

    // endregion
}

impl EditorPartsView for EditorParts {
    fn groups(&self) -> Vec<Rc<EditorGroupView>> {
        self.all_group_views(GroupsOrder::CreationTime)
    }

    fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<EditorGroupView>> {
        self.all_group_views(GroupsOrder::CreationTime).into_iter().find(|group| group.id() == identifier)
    }

    fn bind(&self, context_key: &RawContextKey, group: &Rc<EditorGroupView>) -> Rc<dyn IContextKey> {
        self.bind_group(context_key, &group.as_group())
    }
}

impl IEditorGroupsService for EditorParts {
    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        self.active_part().active_group()
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        self.all_groups(GroupsOrder::CreationTime)
    }

    fn get_groups(&self, order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>> {
        self.all_groups(order)
    }

    fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<dyn IEditorGroup>> {
        self.all_group_views(GroupsOrder::CreationTime).into_iter().find(|group| group.id() == identifier).map(|group| group.as_group())
    }

    fn find_group(&self, scope: &FindGroupScope) -> Option<Rc<dyn IEditorGroup>> {
        self.find_group_from(scope, None, false)
    }

    fn find_group_from(&self, scope: &FindGroupScope, source: Option<GroupRef>, wrap: bool) -> Option<Rc<dyn IEditorGroup>> {
        let source = source.unwrap_or_else(|| GroupRef::Group(IEditorGroupsService::active_group(self)));
        let source_part = self.part_of(&source);

        if self.parts().len() > 1 {
            let groups = self.all_groups(GroupsOrder::GridAppearance);

            // Ensure that FIRST/LAST dispatches globally over all parts
            if scope.location == Some(GroupLocation::First) || scope.location == Some(GroupLocation::Last) {
                return if scope.location == Some(GroupLocation::First) { groups.first().cloned() } else { groups.last().cloned() };
            }

            // Try to find in target part first without wrapping
            if let Some(group) = source_part.find_group_from(scope, Some(source.clone()), false) {
                return Some(group);
            }

            // Ensure that NEXT/PREVIOUS dispatches globally over all parts
            if scope.location == Some(GroupLocation::Next) || scope.location == Some(GroupLocation::Previous) {
                let source_group = self.assert_group(&source);
                let index = groups.iter().position(|group| group.id() == source_group.id()).map_or(-1, |index| index as i64);
                let at = |index: i64| usize::try_from(index).ok().and_then(|index| groups.get(index)).cloned();

                return if scope.location == Some(GroupLocation::Next) {
                    let mut next_group = at(index + 1);
                    if next_group.is_none() && wrap {
                        next_group = groups.first().cloned();
                    }

                    next_group
                } else {
                    let mut previous_group = at(index - 1);
                    if previous_group.is_none() && wrap {
                        previous_group = groups.last().cloned();
                    }

                    previous_group
                };
            }
        }

        source_part.find_group_from(scope, Some(source), wrap)
    }

    fn add_group(&self, location: Option<GroupRef>, direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        let part = location.as_ref().map_or_else(|| self.main_part.clone(), |location| self.part_of(location));

        part.add_group(location, direction)
    }

    fn main_part(&self) -> Rc<dyn IEditorPart> {
        self.main_part.clone()
    }

    fn create_auxiliary_editor_part(&self, _options: Option<AuxiliaryEditorOptions>) -> Promise<Rc<dyn IEditorPart>> {
        Promise::rejected(unsupported("createAuxiliaryEditorPart"))
    }

    fn create_modal_editor_part(&self, _options: Option<ModalEditorPartOptions>) -> Promise<Rc<dyn IModalEditorPart>> {
        // Também sinaliza que nenhuma parte modal ficou visível.
        self.modal_editor_visible_context.set(Dynamic::Bool(false));

        Promise::rejected(unsupported("createModalEditorPart"))
    }

    fn active_modal_editor_part(&self) -> Option<Rc<dyn IModalEditorPart>> {
        None
    }

    fn on_did_change_active_group(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.on_did_active_group_change.event()
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
        self.main_part.on_did_change_editor_part_options()
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
        self.parts().iter().any(|part| IEditorPart::has_restorable_state(&**part))
    }

    fn side_group(&self) -> Rc<dyn IEditorSideGroup> {
        self.active_part().side_group()
    }

    fn orientation(&self) -> GroupOrientation {
        self.active_part().orientation()
    }

    fn part_options(&self) -> EditorPartOptions {
        self.main_part.part_options()
    }

    fn enforce_part_options(&self, options: Dynamic) -> DisposableRef {
        self.main_part.enforce_part_options(options)
    }

    fn activate_group(&self, group: GroupRef) -> Rc<dyn IEditorGroup> {
        self.part_of(&group).activate_group(group)
    }

    fn get_size(&self, group: GroupRef) -> Dimension {
        self.part_of(&group).get_size(group)
    }

    fn set_size(&self, group: GroupRef, size: Dimension) {
        self.part_of(&group).set_size(group, size);
    }

    fn arrange_groups(&self, arrangement: GroupsArrangement, target: Option<GroupRef>) {
        let group = target.unwrap_or_else(|| GroupRef::Group(self.active_part().active_group()));

        self.part_of(&group).arrange_groups(arrangement, Some(group));
    }

    fn toggle_maximize_group(&self, group: Option<GroupRef>) {
        let group = group.unwrap_or_else(|| GroupRef::Group(self.active_part().active_group()));

        self.part_of(&group).toggle_maximize_group(Some(group));
    }

    fn toggle_expand_group(&self, group: Option<GroupRef>) {
        let group = group.unwrap_or_else(|| GroupRef::Group(self.active_part().active_group()));

        self.part_of(&group).toggle_expand_group(Some(group));
    }

    fn apply_layout(&self, layout: &EditorGroupLayout) {
        self.active_part().apply_layout(layout);
    }

    fn get_layout(&self) -> EditorGroupLayout {
        self.active_part().get_layout()
    }

    fn set_group_orientation(&self, orientation: GroupOrientation) {
        self.active_part().set_group_orientation(orientation);
    }

    fn remove_group(&self, group: GroupRef) {
        self.part_of(&group).remove_group(group);
    }

    fn move_group(&self, group: GroupRef, location: GroupRef, direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        self.part_of(&group).move_group(group, location, direction)
    }

    fn merge_group(&self, group: GroupRef, target: GroupRef, options: Option<MergeGroupOptions>) -> bool {
        self.part_of(&group).merge_group(group, target, options)
    }

    fn merge_all_groups(&self, target: GroupRef) -> bool {
        self.active_part().merge_all_groups(target)
    }

    fn copy_group(&self, group: GroupRef, location: GroupRef, direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        self.part_of(&group).copy_group(group, location, direction)
    }

    fn on_did_create_auxiliary_editor_part(&self) -> Event<Rc<dyn IAuxiliaryEditorPart>> {
        self.on_did_create_auxiliary_editor_part.event()
    }

    fn parts(&self) -> Vec<Rc<dyn IEditorPart>> {
        EditorParts::parts(self).into_iter().map(|part| -> Rc<dyn IEditorPart> { part }).collect()
    }

    fn get_part(&self, group: GroupRef) -> Rc<dyn IEditorPart> {
        self.part_of(&group)
    }

    fn get_scoped_instantiation_service(&self, _part: &Rc<dyn IEditorPart>) -> Rc<InstantiationService> {
        self.instantiation_service.clone()
    }

    fn save_working_set(&self, name: &str) -> EditorWorkingSet {
        let working_set = WorkingSetState { id: generate_uuid(), name: name.to_string(), main: self.main_part.create_state(), auxiliary: self.create_state() };
        let saved = EditorWorkingSet { id: working_set.id.clone(), name: working_set.name.clone() };

        self.state.borrow_mut().editor_working_sets.push(working_set);

        self.save_working_sets();

        saved
    }

    fn get_working_sets(&self) -> Vec<EditorWorkingSet> {
        self.state.borrow().editor_working_sets.iter().map(|working_set| EditorWorkingSet { id: working_set.id.clone(), name: working_set.name.clone() }).collect()
    }

    fn delete_working_set(&self, working_set: &EditorWorkingSet) {
        if let Some(index) = self.index_of_working_set(working_set) {
            self.state.borrow_mut().editor_working_sets.remove(index);

            self.save_working_sets();
        }
    }

    fn apply_working_set(&self, working_set: WorkingSetTarget, options: Option<EditorWorkingSetOptions>) -> Promise<bool> {
        let parts = self.rc();

        async_fn(async move {
            let working_set_state: Option<WorkingSetState> = match &working_set {
                WorkingSetTarget::Empty => None,
                WorkingSetTarget::Set(working_set) => {
                    let found = parts.index_of_working_set(working_set).and_then(|index| parts.state.borrow().editor_working_sets.get(index).cloned());
                    if found.is_none() {
                        return Ok(false);
                    }

                    found
                }
            };

            // Apply state: begin with auxiliary windows first because it helps to keep editors around that need confirmation
            // by moving them into the main part. Also, in rare cases, the auxiliary part may not be able to apply the state
            // for certain editors that cannot move to the main part.
            let applied = parts
                .apply_state(working_set_state.as_ref().map_or(PartsStateTarget::Empty, |state| PartsStateTarget::State(state.auxiliary.clone())))
                .await?;
            if !applied {
                return Ok(false);
            }
            parts.main_part.apply_state(working_set_state.map_or(ApplyStateTarget::Empty, |state| ApplyStateTarget::State(state.main))).await?;

            // Restore Focus unless instructed otherwise
            if !options.is_some_and(|options| options.preserve_focus == Some(true)) {
                let most_recent_active_part = parts.state.borrow().most_recent_active_parts.first().cloned();
                if let Some(part) = most_recent_active_part {
                    part.when_ready().await?;
                    IEditorGroup::focus(&*part.active_group());
                }
            }

            Ok(true)
        })
    }

    fn register_context_key_provider(&self, provider: EditorGroupContextKeyProvider) -> DisposableRef {
        let key = provider.context_key.key().to_string();
        {
            let state = self.state.borrow();
            if state.context_key_providers.iter().any(|(candidate, _)| *candidate == key) || state.global_context_keys.contains_key(&key) {
                throw(Error::new(format!("A context key provider for key {key} already exists.")));
            }
        }

        let provider = Rc::new(provider);
        self.state.borrow_mut().context_key_providers.push((key.clone(), provider.clone()));

        let set_context_key_for_groups: Rc<dyn Fn()> = Rc::new({
            let (weak, provider) = (self.this.clone(), provider.clone());
            move || {
                let Some(parts) = weak.upgrade() else { return };

                for group in parts.all_groups(GroupsOrder::CreationTime) {
                    parts.update_registered_context_key(&group, &provider);
                }
            }
        });

        // Run initially and on change
        set_context_key_for_groups();
        let on_did_change = provider.on_did_change.as_ref().map(|event| {
            let set_context_key_for_groups = set_context_key_for_groups.clone();
            event.listen(move |_| set_context_key_for_groups())
        });

        let weak = self.this.clone();
        to_disposable(move || {
            if let Some(on_did_change) = on_did_change {
                on_did_change.dispose();
            }

            if let Some(parts) = weak.upgrade() {
                let mut state = parts.state.borrow_mut();
                state.global_context_keys.remove(&key);
                for scoped_context_keys in state.scoped_context_keys.values_mut() {
                    scoped_context_keys.remove(&key);
                }

                state.context_key_providers.retain(|(candidate, _)| *candidate != key);
                for registered_context_keys in state.registered_context_keys.values_mut() {
                    registered_context_keys.remove(&key);
                }
            }
        })
    }
}

impl IDisposable for EditorParts {
    fn dispose(&self) {
        if std::mem::replace(&mut self.state.borrow_mut().disposed, true) {
            return;
        }

        self.store.dispose();
    }
}
