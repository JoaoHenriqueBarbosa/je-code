//! Porte parcial de `src/vs/workbench/browser/parts/editor/editorGroupView.ts`: o [`EditorGroupView`] mínimo da fatia 2
//! (ver `PORT_PLAN.md`), que guarda o [`EditorGroupModel`], a identidade (`id`, índice, rótulo) e a vista do grid
//! (`ISerializableView`: tamanhos mínimos e máximos, `layout`, `toJSON`), e que o `EditorPart` usa como a vista de cada grupo.
//!
//! Divergências:
//! - **Sem DOM.** O `element` é um nó do `je-ui` (o que o grid precisa para posicionar a vista). O título, o painel de
//!   editor, a barra de progresso, o `Watermark`, a barra de ferramentas e o menu de contexto são pintura e vêm depois,
//!   com a renderização nativa.
//! - **Fatia 3.** Abrir, fechar, mover, copiar e substituir editores precisam dos painéis de editor (`EditorPanes`) e do
//!   tratamento de editor sujo, que entram com a fatia 3. Até lá, `open_editor`, `close_editor`, `move_editor` e o resto
//!   do `IEditorGroup` que depende deles lançam o erro `Method not implemented.` do contrato; `move_editors` e
//!   `copy_editors` só aceitam a lista vazia (que é um no-op no original: `true` e nada, respectivamente).
//! - **Tamanhos.** `minimumWidth` e companhia vêm do `EditorPanes` (`DEFAULT_EDITOR_MIN_DIMENSIONS` e
//!   `DEFAULT_EDITOR_MAX_DIMENSIONS` enquanto não há painel ativo), que é o único caso até a fatia 3. `onDidChange` (as
//!   restrições do painel) nunca dispara por esse mesmo motivo.
//! - **Foco.** O `focus()` do original põe o foco do DOM no painel e o evento de foco do DOM chega ao `EditorPart`
//!   (`onDidFocus`). Aqui `focus()` dispara `onDidFocus` direto, que é o efeito observável.
//! - **`groupsView` e `editorPartsView`** (a parte que contém o grupo e a visão de todas as partes) não são guardados: a
//!   fatia 4 (`EditorParts`) os traz. O `windowId` que o grupo lê da `groupsView` é o parâmetro `window_id`.
//! - **Eventos do modelo.** `onDidModelChange` é o evento do próprio modelo (o grupo o repassa sem nada no meio nesta fatia).
//! - **`createNew`, `createFromSerialized` e `createCopy`** são as variantes de [`EditorGroupSource`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_instantiation::InstantiationService;
use je_platform::contextkey::contextkey::{IContextKeyService, IContextKeyServiceTarget, IScopedContextKeyService};
use je_ui::{NodeId, Role, Ui};
use je_widgets::grid::{GridItem, SerializableView};
use je_widgets::gridview::{GridViewView, ViewDimensions};

use crate::browser::parts::editor::editor::{DEFAULT_EDITOR_MAX_DIMENSIONS, DEFAULT_EDITOR_MIN_DIMENSIONS};
use crate::common::editor::editor_group_model::{
    EditorGroupModel, EditorOrIndex, GetEditorsOptions, GroupModelChangeEvent, IReadonlyEditorGroupModel, SerializedEditorGroupModel,
};
use crate::common::editor::{EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, GroupIdentifier, MatchEditorOptions};
use crate::services::editor::editor_groups_service::IEditorGroup;
use crate::services::layout::layout_service::WindowId;

/// `from: IEditorGroupView | ISerializedEditorGroupModel | null`: de onde nasce o modelo do grupo.
#[derive(Clone)]
pub enum EditorGroupSource {
    /// `createNew`: `null`.
    New,
    /// `createFromSerialized`.
    Serialized(SerializedEditorGroupModel),
    /// `createCopy`: o modelo do grupo copiado é clonado.
    Copy(Rc<EditorGroupView>),
}

/// O `HTMLElement` que o `contextKeyService.createScoped(this.element)` recebe: só atributos, sem pai.
pub struct ContextKeyTarget {
    attributes: RefCell<HashMap<String, String>>,
}

impl ContextKeyTarget {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { attributes: RefCell::new(HashMap::new()) })
    }
}

impl IContextKeyServiceTarget for ContextKeyTarget {
    fn parent_element(&self) -> Option<Rc<dyn IContextKeyServiceTarget>> {
        None
    }

    fn set_attribute(&self, attr: &str, value: &str) {
        self.attributes.borrow_mut().insert(attr.to_string(), value.to_string());
    }

    fn remove_attribute(&self, attr: &str) {
        self.attributes.borrow_mut().remove(attr);
    }

    fn has_attribute(&self, attr: &str) -> bool {
        self.attributes.borrow().contains_key(attr)
    }

    fn get_attribute(&self, attr: &str) -> Option<String> {
        self.attributes.borrow().get(attr).cloned()
    }
}

/// `IDomNodePagePosition`: o último `layout`.
#[derive(Clone, Copy)]
struct LastLayout {
    width: f64,
    height: f64,
}

/// O erro que o contrato lança nos membros que dependem da fatia 3.
fn not_implemented() -> ! {
    throw(Error::new("Method not implemented."))
}

/// `EditorGroupView`.
pub struct EditorGroupView {
    ui: Ui,
    element: NodeId,
    model: Rc<EditorGroupModel>,
    window_id: WindowId,
    groups_label: RefCell<String>,
    index: Cell<usize>,
    active: Cell<Option<bool>>,
    last_layout: Cell<Option<LastLayout>>,
    disposed: Cell<bool>,
    scoped_context_key_service: Rc<dyn IScopedContextKeyService>,
    on_did_focus: Rc<Emitter<()>>,
    on_will_dispose: Rc<Emitter<()>>,
    on_did_change: Rc<Emitter<Option<ViewDimensions>>>,
    disposables: Rc<DisposableStore>,
    this: Weak<EditorGroupView>,
}

impl EditorGroupView {
    /// O construtor do original (`createNew`, `createFromSerialized` e `createCopy` escolhem o `from`).
    pub fn new(
        from: EditorGroupSource,
        ui: &Ui,
        instantiation_service: &InstantiationService,
        context_key_service: &dyn IContextKeyService,
        window_id: WindowId,
        groups_label: &str,
        index: usize,
    ) -> Rc<Self> {
        let disposables = DisposableStore::new();

        let model = match from {
            EditorGroupSource::Copy(source) => source.model.clone_model(),
            EditorGroupSource::Serialized(serialized) => instantiation_service.create_instance::<EditorGroupModel>(Some(serialized)),
            EditorGroupSource::New => instantiation_service.create_instance::<EditorGroupModel>(None),
        };
        disposables.add_rc(model.clone());

        let element = ui.tree_mut().create_detached(Role::Generic);

        // Scoped context key service
        let scoped_context_key_service = context_key_service.create_scoped(ContextKeyTarget::new());
        disposables.add(scoped_context_key_service.clone());

        Rc::new_cyclic(|this| Self {
            ui: ui.clone(),
            element,
            model,
            window_id,
            groups_label: RefCell::new(groups_label.to_string()),
            index: Cell::new(index),
            active: Cell::new(None),
            last_layout: Cell::new(None),
            disposed: Cell::new(false),
            scoped_context_key_service,
            on_did_focus: disposables.add_rc(Emitter::new()),
            on_will_dispose: disposables.add_rc(Emitter::new()),
            on_did_change: disposables.add_rc(Emitter::new()),
            disposables,
            this: this.clone(),
        })
    }

    /// `onDidFocus`.
    pub fn on_did_focus(&self) -> Event<()> {
        self.on_did_focus.event()
    }

    /// `disposed`.
    pub fn disposed(&self) -> bool {
        self.disposed.get()
    }

    /// `notifyIndexChanged(newIndex)`.
    pub fn notify_index_changed(&self, new_index: usize) {
        if self.index.get() != new_index {
            self.index.set(new_index);
            self.model.set_index(new_index as i64);
        }
    }

    /// `notifyLabelChanged(newLabel)`.
    pub fn notify_label_changed(&self, new_label: &str) {
        if *self.groups_label.borrow() != new_label {
            *self.groups_label.borrow_mut() = new_label.to_string();
            self.model.set_label(new_label);
        }
    }

    /// `setActive(isActive)`.
    pub fn set_active(&self, is_active: bool) {
        self.active.set(Some(is_active));

        // Clear selection when group no longer active
        if !is_active
            && let Some(active_editor) = self.model.active_editor()
            && self.model.selected_editors().len() > 1
        {
            self.model.set_selection(&active_editor, &[]);
        }

        // Update model
        self.model.set_active(None /* entire group got active */);
    }

    /// `layout(width, height, top, left)`: guarda o tamanho (o título e o painel de editor, que o repartem, vêm depois).
    fn do_layout(&self, width: f64, height: f64) {
        self.last_layout.set(Some(LastLayout { width, height }));
    }
}

impl GridViewView for EditorGroupView {
    fn element(&self) -> NodeId {
        self.element
    }

    fn minimum_width(&self) -> f64 {
        DEFAULT_EDITOR_MIN_DIMENSIONS.width
    }

    fn maximum_width(&self) -> f64 {
        DEFAULT_EDITOR_MAX_DIMENSIONS.width
    }

    fn minimum_height(&self) -> f64 {
        DEFAULT_EDITOR_MIN_DIMENSIONS.height
    }

    fn maximum_height(&self) -> f64 {
        DEFAULT_EDITOR_MAX_DIMENSIONS.height
    }

    fn proportional_layout(&self) -> Option<bool> {
        let Some(last_layout) = self.last_layout.get() else { return Some(true) };

        Some(!(last_layout.width == self.minimum_width() || last_layout.height == self.minimum_height()))
    }

    fn on_did_change(&self) -> Event<Option<ViewDimensions>> {
        self.on_did_change.event()
    }

    fn layout(&self, width: f64, height: f64, _top: f64, _left: f64) {
        self.do_layout(width, height);
    }
}

impl GridItem for EditorGroupView {}

impl SerializableView for EditorGroupView {
    type Json = SerializedEditorGroupModel;

    /// `toJSON()`.
    fn to_json(&self) -> SerializedEditorGroupModel {
        self.model.serialize()
    }
}

impl IEditorGroup for EditorGroupView {
    fn id(&self) -> GroupIdentifier {
        self.model.id()
    }

    fn window_id(&self) -> WindowId {
        self.window_id
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        self.model.active_editor()
    }

    fn editors(&self) -> Vec<EditorInputRef> {
        self.model.get_editors(EditorsOrder::Sequential, None)
    }

    fn is_locked(&self) -> bool {
        self.model.is_locked()
    }

    fn is_active(&self, editor: &EditorInputOrUntyped) -> bool {
        self.model.is_active(editor)
    }

    fn on_did_model_change(&self) -> Event<GroupModelChangeEvent> {
        self.model.on_did_model_change()
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    fn index(&self) -> usize {
        self.index.get()
    }

    fn label(&self) -> String {
        let groups_label = self.groups_label.borrow();
        if groups_label.is_empty() {
            return format!("Group {}", self.index.get() + 1);
        }

        format!("{}: Group {}", *groups_label, self.index.get() + 1)
    }

    fn aria_label(&self) -> String {
        let groups_label = self.groups_label.borrow();
        if groups_label.is_empty() {
            return format!("Editor Group {}", self.index.get() + 1);
        }

        format!("{}: Editor Group {}", *groups_label, self.index.get() + 1)
    }

    /// Nenhum painel de editor existe até a fatia 3: o grupo nunca tem painel ativo.
    fn active_editor_pane(&self) -> Option<Rc<dyn crate::common::editor::IEditorPane>> {
        None
    }

    fn selected_editors(&self) -> Vec<EditorInputRef> {
        self.model.selected_editors()
    }

    fn preview_editor(&self) -> Option<EditorInputRef> {
        self.model.preview_editor()
    }

    fn sticky_count(&self) -> usize {
        self.model.sticky_count()
    }

    fn scoped_context_key_service(&self) -> Rc<dyn IContextKeyService> {
        self.scoped_context_key_service.clone()
    }

    fn get_editors(&self, order: EditorsOrder, options: Option<GetEditorsOptions>) -> Vec<EditorInputRef> {
        self.model.get_editors(order, options)
    }

    fn get_editor_by_index(&self, index: usize) -> Option<EditorInputRef> {
        self.model.get_editor_by_index(index as i64)
    }

    fn get_index_of_editor(&self, editor: &EditorInputRef) -> i64 {
        self.model.index_of(Some(&EditorInputOrUntyped::Typed(editor.clone())), None, None)
    }

    fn is_first(&self, editor: &EditorInputRef) -> bool {
        self.model.is_first(editor, None)
    }

    fn is_last(&self, editor: &EditorInputRef) -> bool {
        self.model.is_last(editor, None)
    }

    fn is_pinned(&self, editor_or_index: &EditorOrIndex) -> bool {
        self.model.is_pinned(editor_or_index)
    }

    fn is_sticky(&self, editor_or_index: &EditorOrIndex) -> bool {
        self.model.is_sticky(editor_or_index)
    }

    fn is_transient(&self, editor_or_index: &EditorOrIndex) -> bool {
        self.model.is_transient(editor_or_index)
    }

    fn is_selected(&self, editor: &EditorInputRef) -> bool {
        self.model.is_selected(&EditorOrIndex::Editor(editor.clone()))
    }

    fn contains(&self, candidate: &EditorInputOrUntyped, options: Option<MatchEditorOptions>) -> bool {
        self.model.contains(candidate, options)
    }

    fn move_editors(&self, editors: Vec<EditorInputWithOptions>, _target: &Rc<dyn IEditorGroup>) -> bool {
        if !editors.is_empty() {
            not_implemented();
        }

        true
    }

    fn copy_editors(&self, editors: Vec<EditorInputWithOptions>, _target: &Rc<dyn IEditorGroup>) {
        if !editors.is_empty() {
            not_implemented();
        }
    }

    fn lock(&self, locked: bool) {
        self.model.lock(locked);
    }

    fn focus(&self) {
        self.on_did_focus.fire(());
    }
}

impl IDisposable for EditorGroupView {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }

        self.on_will_dispose.fire(());

        self.disposables.dispose();
        self.ui.remove(self.element);
    }
}

impl EditorGroupView {
    /// O grupo como o `Rc<dyn IEditorGroup>` que os contratos do serviço usam.
    pub fn as_group(&self) -> Rc<dyn IEditorGroup> {
        self.this.upgrade().expect("o grupo está vivo enquanto é usado")
    }
}
