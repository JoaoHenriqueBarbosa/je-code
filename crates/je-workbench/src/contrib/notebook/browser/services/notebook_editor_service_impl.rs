//! Porte de `src/vs/workbench/contrib/notebook/browser/services/notebookEditorServiceImpl.ts`: o serviço que empresta os
//! widgets de notebook por grupo, recurso e tipo de editor (`NotebookEditorWidgetService`).
//!
//! Recorte e divergências:
//! - **Sem DOM e sem o widget real.** O `NotebookEditorWidget` do porte é o trait [`NotebookEditorWidget`], e o widget
//!   completo (com a criação por `createInstance` num `createChild(ServiceCollection)`) ainda não existe. O `createWidget`
//!   protegido, que o teste sobrescreve, é a [`NotebookWidgetFactory`] das dependências; a fábrica recebe o serviço de
//!   instanciação, o serviço de chaves de contexto e o de progresso do grupo (que `retrieveWidget` lê do `accessor`), o
//!   `DisposableStore` do widget e a dimensão inicial. `creationOptions` e `codeWindow` (`INotebookEditorCreationOptions` e
//!   `CodeWindow`) pertencem ao widget e não são portados. `getDomNode().remove()` é [`NotebookWidgetDomNode::remove`].
//! - **`IBorrowValue` vivo.** O `value` do original é um acessor (`widget.token === myToken ? widget.widget : undefined`),
//!   e o teste depende de ele mudar depois do empréstimo. O [`IBorrowValue`] do contrato é um valor, então os métodos
//!   inerentes devolvem [`NotebookBorrowValue`], com `value()` calculado a cada leitura; o `INotebookEditorService` entrega o
//!   [`IBorrowValue`] na hora da chamada.
//! - **Editores do serviço.** Como em `notebook_editor_service.rs`, os editores são widgets: `INotebookEditor` é
//!   `Rc<dyn NotebookEditorWidget>`, e `editor.getViewModel()?.notebookDocument.uri` é `text_model()` com `uri()`.
//! - **Entradas de notebook.** `NotebookEditorInput`, `NotebookDiffEditorInput` e `ICompositeNotebookEditorInput` não estão
//!   portados. `instanceof NotebookEditorInput || NotebookDiffEditorInput` e `isNotebookEditorInput` olham o `typeId`
//!   ([`NOTEBOOK_EDITOR_INPUT_ID`] e [`NOTEBOOK_DIFF_EDITOR_INPUT_ID`], o `ID` estático de cada classe), e
//!   `isCompositeNotebookEditorInput(editor) ? editor.editorInputs : []` é o [`CompositeInputsResolver`] das dependências
//!   (`None` quando a entrada não é composta). `{ resource, typeId }` é [`NotebookInputIdentity`].
//! - **Chaves de contexto.** `InteractiveWindowOpen` e `MOST_RECENT_REPL_EDITOR` (`notebookContextKeys.ts`, ainda não
//!   portado) estão aqui, com a mesma chave e o mesmo valor padrão. [`I_EDITOR_PROGRESS_SERVICE`] é o de
//!   `platform/progress/common/progress.ts`, declarado no `je_editor` ao lado do trait [`IEditorProgressService`].
//! - **Fora do recorte:** `getNotebookForPossibleCell` (precisa do `ICodeEditor` das células).
//! - O `Map` do JavaScript mantém a ordem de inserção: os grupos e os editores ficam em listas ordenadas.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::map::ResourceMap;
use je_base::uri::URI;
use je_editor::code_action::platform::{I_EDITOR_PROGRESS_SERVICE, IEditorProgressService};
use je_editor::core_2d::dimension::Dimension;
use je_instantiation::{InstantiationService, ServicesAccessor, service_id};
use je_platform::contextkey::contextkey::{ContextKeyMeta, I_CONTEXT_KEY_SERVICE, IContextKey, IContextKeyService, RawContextKey};

use super::notebook_editor_service::{IBorrowValue, INotebookEditorService};
use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::{EditorCloseEvent, EditorInputRef, EditorWillMoveEvent, GroupIdentifier, GroupModelChangeKind};
use crate::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use crate::contrib::notebook::common::notebook_kernel_service::INotebookTextModelLike;
use crate::services::editor::editor_groups_service::{GroupRef, IEditorGroup, IEditorGroupsService};
use crate::services::editor::editor_service::IEditorService;

/// `NotebookEditorInput.ID`.
pub const NOTEBOOK_EDITOR_INPUT_ID: &str = "workbench.input.notebook";

/// `NotebookDiffEditorInput.ID`.
pub const NOTEBOOK_DIFF_EDITOR_INPUT_ID: &str = "workbench.input.diffNotebookInput";


thread_local! {
    /// `InteractiveWindowOpen`.
    static INTERACTIVE_WINDOW_OPEN: RawContextKey = RawContextKey::new("interactiveWindowOpen", false, ContextKeyMeta::None);
    /// `MOST_RECENT_REPL_EDITOR`.
    static MOST_RECENT_REPL_EDITOR: RawContextKey = RawContextKey::new("mostRecentReplEditor", Dynamic::Undefined, ContextKeyMeta::None);
}

/// `{ resource: URI; typeId: string }`: o que `retrieveWidget` lê da entrada do editor.
#[derive(Clone, Debug, PartialEq)]
pub struct NotebookInputIdentity {
    pub resource: URI,
    pub type_id: String,
}

impl NotebookInputIdentity {
    /// O recurso e o `typeId` de uma entrada; `None` se ela não tem recurso.
    pub fn of(input: &EditorInputRef) -> Option<Self> {
        Some(Self { resource: input.resource()?, type_id: input.editor_type_id() })
    }
}

/// `isCompositeNotebookEditorInput(editor) ? editor.editorInputs : undefined`.
pub type CompositeInputsResolver = Rc<dyn Fn(&EditorInputRef) -> Option<Vec<EditorInputRef>>>;

/// O `createWidget` protegido do original: cria o widget de um grupo.
pub trait NotebookWidgetFactory {
    fn create_widget(
        &self,
        instantiation_service: &InstantiationService,
        editor_group_context_key_service: Rc<dyn IContextKeyService>,
        widget_dispose_store: &DisposableStore,
        editor_group_editor_progress_service: Rc<dyn IEditorProgressService>,
        initial_dimension: Option<Dimension>,
    ) -> Rc<dyn NotebookEditorWidget>;
}

/// O que os `@I...Service` do construtor entregam, mais a fábrica de widgets e o reconhecimento das entradas compostas.
pub struct NotebookEditorWidgetServiceDeps {
    pub editor_group_service: Rc<dyn IEditorGroupsService>,
    pub editor_service: Rc<dyn IEditorService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub instantiation_service: Rc<InstantiationService>,
    pub widget_factory: Rc<dyn NotebookWidgetFactory>,
    pub composite_editor_inputs: CompositeInputsResolver,
}

/// O registro de um widget emprestável: o widget, o tipo de editor, o token do empréstimo atual e o store dele.
struct WidgetInfo {
    widget: RefCell<Option<Rc<dyn NotebookEditorWidget>>>,
    editor_type: String,
    token: Cell<Option<u64>>,
    disposable_store: Rc<DisposableStore>,
}

type WidgetList = Rc<RefCell<Vec<Rc<WidgetInfo>>>>;
type WidgetMap = Rc<RefCell<ResourceMap<WidgetList>>>;

/// `IBorrowValue<NotebookEditorWidget>` com o `value` vivo: só o empréstimo mais recente enxerga o widget.
#[derive(Clone)]
pub struct NotebookBorrowValue {
    my_token: Option<u64>,
    info: Rc<WidgetInfo>,
}

impl NotebookBorrowValue {
    /// `value`: o widget, se este empréstimo ainda é o atual, senão `None`.
    pub fn value(&self) -> Option<Rc<dyn NotebookEditorWidget>> {
        if self.info.token.get() == self.my_token { self.info.widget.borrow().clone() } else { None }
    }
}

pub struct NotebookEditorWidgetService {
    me: Weak<Self>,
    token_pool: Cell<u64>,
    disposables: Rc<DisposableStore>,
    notebook_editors: RefCell<Vec<(String, Rc<dyn NotebookEditorWidget>)>>,
    group_listener: RefCell<HashMap<GroupIdentifier, Vec<DisposableRef>>>,
    on_notebook_editor_add: Rc<Emitter<Rc<dyn NotebookEditorWidget>>>,
    on_notebook_editors_remove: Rc<Emitter<Rc<dyn NotebookEditorWidget>>>,
    most_recent_repl: Rc<dyn IContextKey>,
    borrowable_editors: RefCell<Vec<(GroupIdentifier, WidgetMap)>>,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    instantiation_service: Rc<InstantiationService>,
    widget_factory: Rc<dyn NotebookWidgetFactory>,
    composite_editor_inputs: CompositeInputsResolver,
}

impl NotebookEditorWidgetService {
    pub fn new(deps: NotebookEditorWidgetServiceDeps) -> Rc<Self> {
        let most_recent_repl = MOST_RECENT_REPL_EDITOR.with(|key| key.bind_to(&*deps.context_key_service));
        let interactive_window_open = INTERACTIVE_WINDOW_OPEN.with(|key| key.bind_to(&*deps.context_key_service));
        let editor_group_service = deps.editor_group_service.clone();
        let editor_service = deps.editor_service.clone();
        let composite_editor_inputs = deps.composite_editor_inputs.clone();
        let service = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            token_pool: Cell::new(1),
            disposables: DisposableStore::new(),
            notebook_editors: RefCell::new(Vec::new()),
            group_listener: RefCell::new(HashMap::new()),
            on_notebook_editor_add: Emitter::new(),
            on_notebook_editors_remove: Emitter::new(),
            most_recent_repl,
            borrowable_editors: RefCell::new(Vec::new()),
            editor_group_service: deps.editor_group_service,
            instantiation_service: deps.instantiation_service,
            widget_factory: deps.widget_factory,
            composite_editor_inputs: deps.composite_editor_inputs,
        });

        let on_new_group: Rc<dyn Fn(&Rc<dyn IEditorGroup>)> = {
            let target = service.me.clone();
            Rc::new(move |group| {
                if let Some(service) = target.upgrade() {
                    service.on_new_group(group);
                }
            })
        };
        let added = on_new_group.clone();
        service.disposables.add(editor_group_service.on_did_add_group().listen(move |group| added(group)));
        let ready_groups = editor_group_service.clone();
        editor_group_service.when_ready().then(move |()| {
            for group in ready_groups.groups() {
                on_new_group(&group);
            }
        });

        // group removed -> clean up listeners, clean up widgets
        let target = service.me.clone();
        service.disposables.add(editor_group_service.on_did_remove_group().listen(move |group| {
            if let Some(service) = target.upgrade() {
                service.on_group_removed(group.id());
            }
        }));

        let open_resolver = composite_editor_inputs.clone();
        service.disposables.add(editor_service.clone().on_did_editors_change().listen(move |e| {
            let is_open = matches!(interactive_window_open.get(), Dynamic::Bool(true));
            let has_composite = || editor_service.editors().iter().any(|editor| open_resolver(editor).is_some());
            if e.event.kind == GroupModelChangeKind::EditorOpen && !is_open {
                if has_composite() {
                    interactive_window_open.set(Dynamic::Bool(true));
                }
            } else if e.event.kind == GroupModelChangeKind::EditorClose && is_open && !has_composite() {
                interactive_window_open.set(Dynamic::Bool(false));
            }
        }));
        service
    }

    /// `onNewGroup`.
    fn on_new_group(&self, group: &Rc<dyn IEditorGroup>) {
        let id = group.id();
        let mut listeners: Vec<DisposableRef> = Vec::new();
        let target = self.me.clone();
        listeners.push(group.on_did_close_editor().listen(move |e| {
            if let Some(service) = target.upgrade() {
                service.on_did_close_editor(id, e);
            }
        }));
        let target = self.me.clone();
        listeners.push(group.on_will_move_editor().listen(move |e| {
            if let Some(service) = target.upgrade() {
                service.on_will_move_editor(e);
            }
        }));
        self.group_listener.borrow_mut().insert(id, listeners);
    }

    /// As entradas de notebook de um editor: ele mesmo se é de notebook (ou de diff), ou as da entrada composta.
    fn notebook_inputs(&self, editor: &EditorInputRef) -> Vec<NotebookInputIdentity> {
        let type_id = editor.editor_type_id();
        if type_id == NOTEBOOK_EDITOR_INPUT_ID || type_id == NOTEBOOK_DIFF_EDITOR_INPUT_ID {
            return NotebookInputIdentity::of(editor).into_iter().collect();
        }
        (self.composite_editor_inputs)(editor).unwrap_or_default().iter().filter_map(NotebookInputIdentity::of).collect()
    }

    /// `group.onDidCloseEditor`.
    fn on_did_close_editor(&self, group_id: GroupIdentifier, e: &EditorCloseEvent) {
        let Some(widget_map) = self.group_map(group_id) else { return };

        for input in self.notebook_inputs(&e.editor) {
            let widgets = widget_map.borrow().get(&input.resource).cloned();
            let Some(widgets) = widgets else { continue };
            let index = widgets.borrow().iter().position(|widget| widget.editor_type == input.type_id);
            let Some(index) = index else { continue };
            let value = widgets.borrow_mut().remove(index);
            value.token.set(None);
            let widget = value.widget.borrow().clone();
            if let Some(widget) = widget {
                Self::dispose_widget(&widget);
            }
            value.disposable_store.dispose();
            // unset the widget so that others that still hold a reference don't harm us
            *value.widget.borrow_mut() = None;
        }
    }

    /// `group.onWillMoveEditor`.
    fn on_will_move_editor(&self, e: &EditorWillMoveEvent) {
        if e.editor.editor_type_id() == NOTEBOOK_EDITOR_INPUT_ID {
            if let Some(input) = NotebookInputIdentity::of(&e.editor) {
                self.allow_widget_move(&input, e.group_id, e.target);
            }
        }

        if let Some(editor_inputs) = (self.composite_editor_inputs)(&e.editor) {
            for input in editor_inputs.iter().filter_map(NotebookInputIdentity::of) {
                self.allow_widget_move(&input, e.group_id, e.target);
            }
        }
    }

    /// `editorGroupService.onDidRemoveGroup`.
    fn on_group_removed(&self, group_id: GroupIdentifier) {
        let listeners = self.group_listener.borrow_mut().remove(&group_id);
        for listener in listeners.into_iter().flatten() {
            listener.dispose();
        }
        let widget_map = {
            let mut borrowable = self.borrowable_editors.borrow_mut();
            borrowable.iter().position(|(id, _)| *id == group_id).map(|index| borrowable.remove(index).1)
        };
        if let Some(widget_map) = widget_map {
            let values: Vec<Rc<WidgetInfo>> = widget_map.borrow().values().flat_map(|widgets| widgets.borrow().clone()).collect();
            for value in values {
                value.token.set(None);
                let widget = value.widget.borrow().clone();
                if let Some(widget) = widget {
                    Self::dispose_widget(&widget);
                }
                value.disposable_store.dispose();
            }
        }
    }

    // --- group-based editor borrowing...

    /// `_disposeWidget`.
    fn dispose_widget(widget: &Rc<dyn NotebookEditorWidget>) {
        widget.on_will_hide();
        let dom_node = widget.get_dom_node();
        widget.dispose_widget();
        dom_node.remove();
    }

    fn group_map(&self, group_id: GroupIdentifier) -> Option<WidgetMap> {
        self.borrowable_editors.borrow().iter().find(|(id, _)| *id == group_id).map(|(_, map)| map.clone())
    }

    /// `this._borrowableEditors.get(groupId)?.get(resource)`.
    fn widgets_in(&self, group_id: GroupIdentifier, resource: &URI) -> Option<WidgetList> {
        self.group_map(group_id)?.borrow().get(resource).cloned()
    }

    /// O mapa do grupo, criado e registrado se ainda não existe.
    fn group_map_or_create(&self, group_id: GroupIdentifier) -> WidgetMap {
        if let Some(map) = self.group_map(group_id) {
            return map;
        }
        let map: WidgetMap = Rc::new(RefCell::new(ResourceMap::new()));
        self.borrowable_editors.borrow_mut().push((group_id, map.clone()));
        map
    }

    /// `_allowWidgetMove`.
    fn allow_widget_move(&self, input: &NotebookInputIdentity, source_id: GroupIdentifier, target_id: GroupIdentifier) {
        let source_part = self.editor_group_service.get_part(GroupRef::Id(source_id));
        let target_part = self.editor_group_service.get_part(GroupRef::Id(target_id));

        if source_part.window_id() != target_part.window_id() {
            return;
        }

        let find = |widgets: &WidgetList| widgets.borrow().iter().position(|widget| widget.editor_type == input.type_id);
        if self.widgets_in(target_id, &input.resource).is_some_and(|widgets| find(&widgets).is_some()) {
            // not needed, a separate widget is already there
            return;
        }

        let source_widgets = self.widgets_in(source_id, &input.resource);
        let widget = source_widgets.as_ref().and_then(|widgets| find(widgets).map(|index| widgets.borrow()[index].clone()));
        let Some(widget) = widget else {
            errors::throw(Error::new("no widget at source group"));
        };

        // don't allow the widget to be retrieved at its previous location any more
        if let Some(source_widgets) = &source_widgets {
            if let Some(index_to_remove) = find(source_widgets) {
                source_widgets.borrow_mut().remove(index_to_remove);
            }
        }

        // allow the widget to be retrieved at its new location
        let target_map = self.group_map_or_create(target_id);
        let widgets_at_target = target_map.borrow().get(&input.resource).cloned().unwrap_or_default();
        widgets_at_target.borrow_mut().push(widget);
        target_map.borrow_mut().set(input.resource.clone(), widgets_at_target);
    }

    /// `retrieveExistingWidgetFromURI`, com o empréstimo vivo.
    pub fn retrieve_existing_borrowed_widget_from_uri(&self, resource: &URI) -> Option<NotebookBorrowValue> {
        let maps: Vec<WidgetMap> = self.borrowable_editors.borrow().iter().map(|(_, map)| map.clone()).collect();
        for widget_info in maps {
            let widgets = widget_info.borrow().get(resource).cloned();
            if let Some(widgets) = widgets {
                let first = widgets.borrow().first().cloned();
                if let Some(first) = first {
                    return Some(Self::create_borrow_value(first.token.get(), first));
                }
            }
        }
        None
    }

    /// `retrieveAllExistingWidgets`.
    pub fn retrieve_all_existing_widgets(&self) -> Vec<NotebookBorrowValue> {
        let maps: Vec<WidgetMap> = self.borrowable_editors.borrow().iter().map(|(_, map)| map.clone()).collect();
        maps.iter()
            .flat_map(|widget_info| widget_info.borrow().values().flat_map(|widgets| widgets.borrow().clone()).collect::<Vec<_>>())
            .map(|widget| Self::create_borrow_value(widget.token.get(), widget))
            .collect()
    }

    /// `retrieveWidget(accessor, groupId, input, creationOptions, initialDimension, codeWindow)`.
    pub fn retrieve_widget(&self, accessor: &ServicesAccessor, group_id: GroupIdentifier, input: &NotebookInputIdentity, initial_dimension: Option<Dimension>) -> NotebookBorrowValue {
        let existing = self.widgets_in(group_id, &input.resource).and_then(|widgets| widgets.borrow().iter().find(|widget| widget.editor_type == input.type_id).cloned());

        let value = match existing {
            Some(value) => {
                // reuse a widget which was either free'ed before or which is simply being reused...
                value.token.set(Some(self.next_token()));
                value
            }
            None => {
                // NEW widget
                let editor_group_context_key_service = accessor.get(I_CONTEXT_KEY_SERVICE);
                let editor_group_editor_progress_service = accessor.get(I_EDITOR_PROGRESS_SERVICE);
                let widget_dispose_store = DisposableStore::new();
                let widget = self.widget_factory.create_widget(
                    &self.instantiation_service,
                    editor_group_context_key_service,
                    &widget_dispose_store,
                    editor_group_editor_progress_service,
                    initial_dimension,
                );
                let token = self.next_token();
                let value = Rc::new(WidgetInfo {
                    widget: RefCell::new(Some(widget)),
                    editor_type: input.type_id.clone(),
                    token: Cell::new(Some(token)),
                    disposable_store: widget_dispose_store,
                });

                let map = self.group_map_or_create(group_id);
                let values = map.borrow().get(&input.resource).cloned().unwrap_or_default();
                values.borrow_mut().push(value.clone());
                map.borrow_mut().set(input.resource.clone(), values);
                value
            }
        };

        Self::create_borrow_value(value.token.get(), value)
    }

    /// `this._tokenPool++`.
    fn next_token(&self) -> u64 {
        let token = self.token_pool.get();
        self.token_pool.set(token + 1);
        token
    }

    /// `_createBorrowValue`.
    fn create_borrow_value(my_token: Option<u64>, info: Rc<WidgetInfo>) -> NotebookBorrowValue {
        NotebookBorrowValue { my_token, info }
    }

    // --- editor management

    /// `addNotebookEditor`.
    pub fn add_notebook_editor(&self, editor: Rc<dyn NotebookEditorWidget>) {
        let id = editor.get_id();
        {
            let mut editors = self.notebook_editors.borrow_mut();
            match editors.iter_mut().find(|(key, _)| *key == id) {
                Some(entry) => entry.1 = editor.clone(),
                None => editors.push((id, editor.clone())),
            }
        }
        self.on_notebook_editor_add.fire(editor);
    }

    /// `removeNotebookEditor`.
    pub fn remove_notebook_editor(&self, editor: &Rc<dyn NotebookEditorWidget>) {
        let notebook_uri = editor.text_model().map(|model| model.uri());
        let id = editor.get_id();
        let removed = {
            let mut editors = self.notebook_editors.borrow_mut();
            editors.iter().position(|(key, _)| *key == id).map(|index| editors.remove(index)).is_some()
        };
        if removed {
            self.on_notebook_editors_remove.fire(editor.clone());
        }
        let current = self.most_recent_repl.get();
        let is_most_recent = match &notebook_uri {
            Some(uri) => matches!(&current, Dynamic::String(value) if *value == uri.to_string(false)),
            None => matches!(current, Dynamic::Undefined),
        };
        if is_most_recent {
            self.most_recent_repl.reset();
        }
    }

    /// `updateReplContextKey`.
    pub fn update_repl_context_key(&self, uri: &str) {
        self.most_recent_repl.set(Dynamic::string(uri));
    }
}

impl INotebookEditorService for NotebookEditorWidgetService {
    fn on_did_add_notebook_editor(&self) -> Event<Rc<dyn NotebookEditorWidget>> {
        self.on_notebook_editor_add.event()
    }

    fn retrieve_existing_widget_from_uri(&self, resource: &URI) -> Option<IBorrowValue<dyn NotebookEditorWidget>> {
        self.retrieve_existing_borrowed_widget_from_uri(resource).map(|borrowed| IBorrowValue { value: borrowed.value() })
    }

    fn on_did_remove_notebook_editor(&self) -> Event<Rc<dyn NotebookEditorWidget>> {
        self.on_notebook_editors_remove.event()
    }

    fn list_notebook_editors(&self) -> Vec<Rc<dyn NotebookEditorWidget>> {
        self.notebook_editors.borrow().iter().map(|(_, editor)| editor.clone()).collect()
    }

    fn get_notebook_editor(&self, editor_id: &str) -> Option<Rc<dyn NotebookEditorWidget>> {
        self.notebook_editors.borrow().iter().find(|(id, _)| id == editor_id).map(|(_, editor)| editor.clone())
    }
}

impl IDisposable for NotebookEditorWidgetService {
    fn dispose(&self) {
        self.disposables.dispose();
        self.on_notebook_editor_add.dispose();
        self.on_notebook_editors_remove.dispose();
        let listeners: Vec<DisposableRef> = self.group_listener.borrow_mut().drain().flat_map(|(_, listeners)| listeners).collect();
        for listener in listeners {
            listener.dispose();
        }
        let maps: Vec<WidgetMap> = self.borrowable_editors.borrow().iter().map(|(_, map)| map.clone()).collect();
        for widget_map in maps {
            let values: Vec<Rc<WidgetInfo>> = widget_map.borrow().values().flat_map(|widgets| widgets.borrow().clone()).collect();
            for widget in values {
                widget.disposable_store.dispose();
            }
        }
    }
}
