//! Porte de `src/vs/workbench/common/editor/editorGroupModel.ts`: o `EditorGroupModel`, o modelo de dados de um grupo de
//! editores (abrir, fechar, mover, fixar, tornar aderente, prévia, transiente, seleção, lista dos mais recentes `MRU`,
//! eventos e serialização), com os tipos de evento e de opção que o acompanham.
//!
//! Divergências:
//! - **Injeção.** O construtor `(labelOrSerializedGroup, @IInstantiationService, @IConfigurationService)` é o [`Injectable`]
//!   do [`EditorGroupModel`]: `Args` é o grupo serializado opcional e `Deps` são o [`InstantiationService`] (o
//!   `IInstantiationService` do porte) e o `IConfigurationService`. `createInstance(EditorGroupModel, x)` é
//!   `create_instance::<EditorGroupModel>(x)`.
//! - **`IGroupModelChangeEvent` e as variantes.** `IGroupEditorChangeEvent`, `IGroupEditorOpenEvent`, `IGroupEditorMoveEvent`
//!   e `IGroupEditorCloseEvent` são formas estruturais da mesma interface: aqui são a struct [`GroupModelChangeEvent`], com
//!   os campos que cada forma acrescenta opcionais (`old_editor_index` do movimento, `context` e `sticky` do fechamento).
//!   As funções `isGroupEditorChangeEvent`, `isGroupEditorOpenEvent`, `isGroupEditorMoveEvent` e `isGroupEditorCloseEvent`
//!   são as mesmas checagens de campos do original.
//! - **Índices.** `number` que pode ser `-1` (`indexOf`) ou negativo (`moveEditor(editor, -1)`) é `i64`. Os `splice` do
//!   original, que aceitam índice negativo ou maior que o tamanho, são [`js_splice`].
//! - **`EditorInput | number`** (`isPinned`, `isSticky`, `isSelected`, `isTransient`) é [`EditorOrIndex`]; `EditorInput |
//!   IUntypedEditorInput` é [`EditorInputOrUntyped`]; `EditorInput | null` é `Option<&EditorInputRef>`.
//! - **`clone()`** é [`EditorGroupModel::clone_model`]: o `Rc` do grupo já responde a `clone()` com a cópia do `Rc`.
//! - **`IEditorGroupModel`** (a interface não exportada que acrescenta os métodos de mudança) não existe: os métodos são os
//!   do próprio [`EditorGroupModel`].
//! - **Eventos de editor.** Os listeners que o grupo registra em cada editor guardam o editor por `Weak`, para a entrada não
//!   ficar presa pelo emissor dela mesma.
//! - **`isSerializedEditorGroupModel`** olha um valor dinâmico (o `unknown` lido do armazenamento), e o modelo serializado
//!   é a struct [`SerializedEditorGroupModel`].
//! - **Estado.** Os campos mutáveis ficam num `RefCell` e nenhum método segura o empréstimo ao chamar código externo ou
//!   disparar evento, porque os listeners voltam a consultar o grupo.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, EmitterOptions, Event};
use je_base::lifecycle::{Disposable, DisposableId, DisposableStore, IDisposable, id_of};
use je_instantiation::{Injectable, InstantiationService};
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;

use crate::common::editor::editor_factory_registry::editor_factory_registry;
use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::{
    EditorCloseContext, EditorInputOrUntyped, EditorInputRef, EditorsOrder, GroupIdentifier, GroupModelChangeKind, MatchEditorOptions, SideBySideEditor,
    is_side_by_side_editor_input,
};

/// `EditorOpenPositioning`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditorOpenPositioning {
    Left,
    Right,
    First,
    Last,
}

impl EditorOpenPositioning {
    /// O valor de `workbench.editor.openPositioning`: um texto desconhecido (ou ausente) não é nenhum dos quatro.
    fn parse(value: &Dynamic) -> Option<Self> {
        let Dynamic::String(text) = value else { return None };

        match text.to_string_lossy().as_str() {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "first" => Some(Self::First),
            "last" => Some(Self::Last),
            _ => None,
        }
    }
}

/// `IEditorOpenOptions`.
#[derive(Clone, Default)]
pub struct EditorOpenOptions {
    pub pinned: Option<bool>,
    pub sticky: Option<bool>,
    pub transient: Option<bool>,
    pub active: Option<bool>,
    pub inactive_selection: Option<Vec<EditorInputRef>>,
    pub index: Option<i64>,
    /// `SideBySideEditor.ANY | SideBySideEditor.BOTH`.
    pub support_side_by_side: Option<SideBySideEditor>,
}

/// `IEditorOpenResult`.
#[derive(Clone)]
pub struct EditorOpenResult {
    pub editor: EditorInputRef,
    pub is_new: bool,
}

/// `ISerializedEditorInput`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedEditorInput {
    pub id: String,
    pub value: String,
}

/// `ISerializedEditorGroupModel`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SerializedEditorGroupModel {
    /// Ausente nos dados antigos (compatibilidade com versões anteriores).
    pub id: Option<GroupIdentifier>,
    pub locked: Option<bool>,
    pub editors: Vec<SerializedEditorInput>,
    pub mru: Vec<usize>,
    pub preview: Option<usize>,
    pub sticky: Option<usize>,
}

/// `isSerializedEditorGroupModel`: um objeto com as listas `editors` e `mru`.
pub fn is_serialized_editor_group_model(group: &Dynamic) -> bool {
    !group.is_nullish() && group.type_of() == "object" && group.get_str("editors").is_array() && group.get_str("mru").is_array()
}

/// `IGroupModelChangeEvent` (e as formas `IGroupEditorChangeEvent`, `IGroupEditorOpenEvent`, `IGroupEditorMoveEvent` e
/// `IGroupEditorCloseEvent`, ver o cabeçalho).
#[derive(Clone)]
pub struct GroupModelChangeEvent {
    /// The kind of change that occurred in the group model.
    pub kind: GroupModelChangeKind,

    /// Only applies when editors change providing access to the editor the event is about.
    pub editor: Option<EditorInputRef>,

    /// Only applies when editors change providing access to the index of the editor the event is about.
    pub editor_index: Option<i64>,

    /// `IGroupEditorMoveEvent.oldEditorIndex`: signifies the index the editor is moving from. `editor_index` will contain
    /// the index the editor is moving to.
    pub old_editor_index: Option<i64>,

    /// `IGroupEditorCloseEvent.context`: signifies the context in which the editor is being closed. This allows for
    /// understanding if a replace or reopen is occurring.
    pub context: Option<EditorCloseContext>,

    /// `IGroupEditorCloseEvent.sticky`: signifies whether or not the closed editor was sticky. This is necessary because
    /// state is lost after closing.
    pub sticky: Option<bool>,
}

impl GroupModelChangeEvent {
    /// `{ kind }`: um evento do grupo, sem editor.
    fn group(kind: GroupModelChangeKind) -> Self {
        Self { kind, editor: None, editor_index: None, old_editor_index: None, context: None, sticky: None }
    }

    /// `IGroupEditorChangeEvent`.
    fn editor_change(kind: GroupModelChangeKind, editor: &EditorInputRef, editor_index: i64) -> Self {
        Self { editor: Some(editor.clone()), editor_index: Some(editor_index), ..Self::group(kind) }
    }

    /// `IGroupEditorMoveEvent`.
    fn editor_move(editor: &EditorInputRef, old_editor_index: i64, editor_index: i64) -> Self {
        Self { old_editor_index: Some(old_editor_index), ..Self::editor_change(GroupModelChangeKind::EditorMove, editor, editor_index) }
    }

    /// `{ kind: EDITOR_CLOSE, ...closeResult }`.
    fn editor_close(close_result: &EditorCloseResult) -> Self {
        Self {
            context: Some(close_result.context),
            sticky: Some(close_result.sticky),
            ..Self::editor_change(GroupModelChangeKind::EditorClose, &close_result.editor, close_result.editor_index)
        }
    }
}

/// `isGroupEditorChangeEvent`.
pub fn is_group_editor_change_event(e: &GroupModelChangeEvent) -> bool {
    e.editor.is_some() && e.editor_index.is_some()
}

/// `isGroupEditorOpenEvent`.
pub fn is_group_editor_open_event(e: &GroupModelChangeEvent) -> bool {
    e.kind == GroupModelChangeKind::EditorOpen && e.editor_index.is_some()
}

/// `isGroupEditorMoveEvent`.
pub fn is_group_editor_move_event(e: &GroupModelChangeEvent) -> bool {
    e.kind == GroupModelChangeKind::EditorMove && e.editor_index.is_some() && e.old_editor_index.is_some()
}

/// `isGroupEditorCloseEvent`.
pub fn is_group_editor_close_event(e: &GroupModelChangeEvent) -> bool {
    e.kind == GroupModelChangeKind::EditorClose && e.editor_index.is_some() && e.context.is_some() && e.sticky.is_some()
}

/// `IEditorCloseResult`.
#[derive(Clone)]
pub struct EditorCloseResult {
    pub editor: EditorInputRef,
    pub context: EditorCloseContext,
    pub editor_index: i64,
    pub sticky: bool,
}

/// `{ excludeSticky?: boolean }`: as opções de `getEditors`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GetEditorsOptions {
    pub exclude_sticky: Option<bool>,
}

/// `EditorInput | number`.
#[derive(Clone)]
pub enum EditorOrIndex {
    Editor(EditorInputRef),
    Index(i64),
}

impl From<&EditorInputRef> for EditorOrIndex {
    fn from(editor: &EditorInputRef) -> Self {
        EditorOrIndex::Editor(editor.clone())
    }
}

impl From<i64> for EditorOrIndex {
    fn from(index: i64) -> Self {
        EditorOrIndex::Index(index)
    }
}

/// `IReadonlyEditorGroupModel`.
pub trait IReadonlyEditorGroupModel {
    fn on_did_model_change(&self) -> Event<GroupModelChangeEvent>;

    fn id(&self) -> GroupIdentifier;
    fn count(&self) -> usize;
    fn sticky_count(&self) -> usize;
    fn is_locked(&self) -> bool;
    fn active_editor(&self) -> Option<EditorInputRef>;
    fn preview_editor(&self) -> Option<EditorInputRef>;
    fn selected_editors(&self) -> Vec<EditorInputRef>;

    fn get_editors(&self, order: EditorsOrder, options: Option<GetEditorsOptions>) -> Vec<EditorInputRef>;
    fn get_editor_by_index(&self, index: i64) -> Option<EditorInputRef>;
    fn index_of(&self, editor: Option<&EditorInputOrUntyped>, editors: Option<&[EditorInputRef]>, options: Option<MatchEditorOptions>) -> i64;
    fn is_active(&self, editor: &EditorInputOrUntyped) -> bool;
    fn is_pinned(&self, editor_or_index: &EditorOrIndex) -> bool;
    fn is_sticky(&self, editor_or_index: &EditorOrIndex) -> bool;
    fn is_selected(&self, editor_or_index: &EditorOrIndex) -> bool;
    fn is_transient(&self, editor_or_index: &EditorOrIndex) -> bool;
    fn is_first(&self, editor: &EditorInputRef, editors: Option<&[EditorInputRef]>) -> bool;
    fn is_last(&self, editor: &EditorInputRef, editors: Option<&[EditorInputRef]>) -> bool;
    /// O editor e o índice dele (`[EditorInput, number]`).
    fn find_editor(&self, editor: Option<&EditorInputRef>, options: Option<MatchEditorOptions>) -> Option<(EditorInputRef, i64)>;
    fn contains(&self, editor: &EditorInputOrUntyped, options: Option<MatchEditorOptions>) -> bool;
}

/// `EditorInput` como `EditorInput | IUntypedEditorInput`.
fn typed(editor: &EditorInputRef) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Typed(editor.clone())
}

/// `a === b` entre entradas.
fn same(a: &EditorInputRef, b: &EditorInputRef) -> bool {
    id_of(a) == id_of(b)
}

/// `a === b` entre entradas que podem faltar (`null` e `undefined` são iguais entre si).
fn same_editor(a: Option<&EditorInputRef>, b: Option<&EditorInputRef>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => same(a, b),
        _ => false,
    }
}

/// `editors.indexOf(editor)` do JavaScript: pela identidade, ou `-1`.
fn identity_index(editors: &[EditorInputRef], editor: &EditorInputRef) -> i64 {
    editors.iter().position(|candidate| same(candidate, editor)).map_or(-1, |index| index as i64)
}

/// `items[index]`: um índice negativo ou fora da lista é `undefined`.
fn element<T>(items: &[T], index: i64) -> Option<&T> {
    usize::try_from(index).ok().and_then(|index| items.get(index))
}

/// `items.splice(start, deleteCount, ...insert)`: um `start` negativo conta a partir do fim e um `start` maior que o tamanho
/// vale o tamanho.
fn js_splice<T>(items: &mut Vec<T>, start: i64, delete_count: usize, insert: impl IntoIterator<Item = T>) {
    let len = items.len();
    let at = match usize::try_from(start) {
        Ok(start) => start.min(len),
        Err(_) => len.saturating_sub(usize::try_from(start.unsigned_abs()).unwrap_or(usize::MAX)),
    };
    let end = at.saturating_add(delete_count).min(len);

    drop(items.splice(at..end, insert));
}

/// `matches(editor, candidate, options)`.
fn matches(editor: Option<&EditorInputRef>, candidate: Option<&EditorInputOrUntyped>, options: Option<MatchEditorOptions>) -> bool {
    let (Some(editor), Some(candidate)) = (editor, candidate) else { return false };

    let support_side_by_side = options.and_then(|options| options.support_side_by_side);
    if let Some(support_side_by_side) = support_side_by_side
        && let Some((primary, secondary)) = editor.side_by_side_editors()
        && !is_side_by_side_editor_input(Some(candidate))
    {
        match support_side_by_side {
            SideBySideEditor::Any => {
                if matches(Some(&primary), Some(candidate), options) || matches(Some(&secondary), Some(candidate), options) {
                    return true;
                }
            }
            SideBySideEditor::Both => {
                if matches(Some(&primary), Some(candidate), options) && matches(Some(&secondary), Some(candidate), options) {
                    return true;
                }
            }
            SideBySideEditor::Primary | SideBySideEditor::Secondary => {}
        }
    }

    let strict_equals = matches!(candidate, EditorInputOrUntyped::Typed(candidate) if same(editor, candidate));

    if options.is_some_and(|options| options.strict_equals == Some(true)) {
        return strict_equals;
    }

    strict_equals || editor.matches(candidate)
}

thread_local! {
    /// `EditorGroupModel.IDS`.
    static IDS: Cell<GroupIdentifier> = const { Cell::new(0) };
}

/// `EditorGroupModel.IDS++`.
fn next_group_id() -> GroupIdentifier {
    IDS.with(|ids| {
        let id = ids.get();
        ids.set(id + 1);
        id
    })
}

/// Os campos mutáveis do grupo.
struct ModelState {
    editors: Vec<EditorInputRef>,
    mru: Vec<EditorInputRef>,
    locked: bool,
    /// Editors in selected state, first one is active.
    selection: Vec<EditorInputRef>,
    /// Editor in preview state.
    preview: Option<EditorInputRef>,
    /// Index of first editor in sticky state.
    sticky: i64,
    /// Editors in transient state (pela identidade).
    transient: HashSet<DisposableId>,
    editor_open_positioning: Option<EditorOpenPositioning>,
    focus_recent_editor_after_close: bool,
}

impl ModelState {
    fn new() -> Self {
        Self {
            editors: Vec::new(),
            mru: Vec::new(),
            locked: false,
            selection: Vec::new(),
            preview: None,
            sticky: -1,
            transient: HashSet::new(),
            editor_open_positioning: None,
            focus_recent_editor_after_close: false,
        }
    }
}

/// `EditorGroupModel`.
pub struct EditorGroupModel {
    this: Weak<EditorGroupModel>,
    base: Rc<Disposable>,
    on_did_model_change: Rc<Emitter<GroupModelChangeEvent>>,
    id: Cell<GroupIdentifier>,
    state: RefCell<ModelState>,
    editor_listeners: RefCell<Vec<Rc<DisposableStore>>>,
    instantiation_service: Rc<InstantiationService>,
    configuration_service: Rc<dyn IConfigurationService>,
}

impl Injectable for EditorGroupModel {
    type Args = Option<SerializedEditorGroupModel>;
    type Deps = (Rc<InstantiationService>, Rc<dyn IConfigurationService>);

    fn create(label_or_serialized_group: Self::Args, (instantiation_service, configuration_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        // increased for users with hundreds of inputs opened
        let on_did_model_change: Rc<Emitter<GroupModelChangeEvent>> = Emitter::with_options(EmitterOptions {
            leak_warning_threshold: Some(500),
            leak_warning_name: Some("EditorGroupModel._onDidModelChange".to_string()),
            ..EmitterOptions::default()
        });
        base.register(on_did_model_change.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_model_change,
            id: Cell::new(0),
            state: RefCell::new(ModelState::new()),
            editor_listeners: RefCell::new(Vec::new()),
            instantiation_service,
            configuration_service,
        });

        match &label_or_serialized_group {
            Some(serialized_group) => this.deserialize(serialized_group),
            None => this.id.set(next_group_id()),
        }

        this.on_configuration_updated(None);
        this.register_listeners();

        this
    }
}

impl EditorGroupModel {
    fn register_listeners(&self) {
        let this = self.this.clone();
        self.base.register(self.configuration_service.on_did_change_configuration().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_configuration_updated(Some(e));
            }
        }));
    }

    fn on_configuration_updated(&self, e: Option<&Rc<ConfigurationChangeEvent>>) {
        if let Some(e) = e
            && !e.affects_configuration("workbench.editor.openPositioning", None)
            && !e.affects_configuration("workbench.editor.focusRecentEditorAfterClose", None)
        {
            return;
        }

        let open_positioning = self.configuration_service.get_value(Some("workbench.editor.openPositioning"), None);
        let focus_recent_editor_after_close = self.configuration_service.get_value(Some("workbench.editor.focusRecentEditorAfterClose"), None);

        let mut state = self.state.borrow_mut();
        state.editor_open_positioning = EditorOpenPositioning::parse(&open_positioning);
        state.focus_recent_editor_after_close = focus_recent_editor_after_close.is_truthy();
    }

    /// `this.sticky`.
    fn sticky(&self) -> i64 {
        self.state.borrow().sticky
    }

    /// Os editores do grupo, ou os `editors` dados.
    fn editors_or_all<'a>(&self, editors: Option<&'a [EditorInputRef]>) -> Cow<'a, [EditorInputRef]> {
        match editors {
            Some(editors) => Cow::Borrowed(editors),
            None => Cow::Owned(self.state.borrow().editors.clone()),
        }
    }

    /// `this.indexOf(editor, editors)` com uma entrada tipada.
    fn index_of_editor(&self, editor: Option<&EditorInputRef>, editors: Option<&[EditorInputRef]>) -> i64 {
        self.index_of(editor.map(typed).as_ref(), editors, None)
    }

    /// `isSticky(index)`.
    fn is_sticky_index(&self, index: i64) -> bool {
        let sticky = self.sticky();
        if sticky < 0 {
            return false; // no sticky editor
        }

        if index < 0 {
            return false;
        }

        index <= sticky
    }

    fn do_is_selected(&self, editor: &EditorInputRef) -> bool {
        self.state.borrow().selection.iter().any(|selected| same(selected, editor))
    }

    pub fn open_editor(&self, candidate: &EditorInputRef, options: Option<&EditorOpenOptions>) -> EditorOpenResult {
        let options = options.cloned().unwrap_or_default();
        let make_sticky = options.sticky == Some(true) || options.index.is_some_and(|index| self.is_sticky_index(index));
        let make_pinned = options.pinned == Some(true) || options.sticky == Some(true);
        let make_transient = options.transient == Some(true);
        let make_active = options.active == Some(true)
            || self.active_editor().is_none()
            || (!make_pinned && same_editor(self.preview_editor().as_ref(), self.active_editor().as_ref()));
        let inactive_selection = options.inactive_selection.as_deref().unwrap_or_default();

        let existing_editor_and_index =
            self.find_editor(Some(candidate), Some(MatchEditorOptions { support_side_by_side: options.support_side_by_side, strict_equals: None }));

        match existing_editor_and_index {
            // New editor
            None => {
                let new_editor = candidate.clone();
                let index_of_active = self.index_of_editor(self.active_editor().as_ref(), None);
                let (editor_open_positioning, editors_count) = {
                    let state = self.state.borrow();
                    (state.editor_open_positioning, state.editors.len() as i64)
                };

                let mut target_index = if let Some(index) = options.index {
                    // Insert into specific position
                    index
                } else if editor_open_positioning == Some(EditorOpenPositioning::First) {
                    // Insert to the BEGINNING
                    // Always make sure targetIndex is after sticky editors unless we are explicitly told to make the editor
                    // sticky
                    if !make_sticky && self.is_sticky_index(0) { self.sticky() + 1 } else { 0 }
                } else if editor_open_positioning == Some(EditorOpenPositioning::Last) {
                    // Insert to the END
                    editors_count
                } else {
                    // Insert to LEFT or RIGHT of active editor
                    let target_index = if editor_open_positioning == Some(EditorOpenPositioning::Left) {
                        // Insert to the LEFT of active editor
                        if index_of_active == 0 || editors_count == 0 {
                            0 // to the left becoming first editor in list
                        } else {
                            index_of_active // to the left of active editor
                        }
                    } else {
                        // Insert to the RIGHT of active editor
                        index_of_active + 1
                    };

                    // Always make sure targetIndex is after sticky editors unless we are explicitly told to make the editor
                    // sticky
                    if !make_sticky && self.is_sticky_index(target_index) { self.sticky() + 1 } else { target_index }
                };

                // If the editor becomes sticky, increment the sticky index and adjust the targetIndex to be at the end of
                // sticky editors unless already.
                if make_sticky {
                    self.state.borrow_mut().sticky += 1;

                    if !self.is_sticky_index(target_index) {
                        target_index = self.sticky();
                    }
                }

                // Insert into our list of editors if pinned or we have no preview editor
                let has_preview = self.state.borrow().preview.is_some();
                if make_pinned || !has_preview {
                    self.splice(target_index, false, Some(new_editor.clone()));
                }

                // Handle transient
                if make_transient {
                    self.do_set_transient(&new_editor, target_index, true);
                }

                // Handle preview
                if !make_pinned {
                    // Replace existing preview with this editor if we have a preview
                    if let Some(preview) = self.preview_editor() {
                        let index_of_preview = self.index_of_editor(Some(&preview), None);
                        if target_index > index_of_preview {
                            target_index -= 1; // accommodate for the fact that the preview editor closes
                        }

                        self.replace_editor(&preview, &new_editor, target_index, !make_active);
                    }

                    self.state.borrow_mut().preview = Some(new_editor.clone());
                }

                // Listeners
                self.register_editor_listeners(&new_editor);

                // Event
                self.on_did_model_change.fire(GroupModelChangeEvent::editor_change(GroupModelChangeKind::EditorOpen, &new_editor, target_index));

                // Handle active editor / selected editors
                let active_selected_editor = if make_active { Some(new_editor.clone()) } else { self.active_editor() };
                if let Some(active_selected_editor) = active_selected_editor {
                    self.set_selection(&active_selected_editor, inactive_selection);
                }

                EditorOpenResult { editor: new_editor, is_new: true }
            }

            // Existing editor
            Some((existing_editor, existing_editor_index)) => {
                // Update transient (existing editors do not turn transient if they were not before)
                let transient = make_transient && self.is_transient(&EditorOrIndex::Editor(existing_editor.clone()));
                self.do_set_transient(&existing_editor, existing_editor_index, transient);

                // Pin it
                if make_pinned {
                    self.do_pin(&existing_editor, existing_editor_index);
                }

                // Handle active editor / selected editors
                let active_selected_editor = if make_active { Some(existing_editor.clone()) } else { self.active_editor() };
                if let Some(active_selected_editor) = active_selected_editor {
                    self.set_selection(&active_selected_editor, inactive_selection);
                }

                // Respect index
                if let Some(index) = options.index {
                    self.move_editor(&existing_editor, index);
                }

                // Stick it (intentionally after the moveEditor call in case the editor was already moved into the sticky
                // range)
                if make_sticky {
                    let index = self.index_of_editor(Some(&existing_editor), None);
                    self.do_stick(&existing_editor, index);
                }

                EditorOpenResult { editor: existing_editor, is_new: false }
            }
        }
    }

    fn register_editor_listeners(&self, editor: &EditorInputRef) {
        let listeners = DisposableStore::new();
        self.editor_listeners.borrow_mut().push(listeners.clone());

        let weak_editor = Rc::downgrade(editor);

        // Re-emit disposal of editor input as our own event
        listeners.add(editor.on_will_dispose().once().listen({
            let (this, weak_editor) = (self.this.clone(), weak_editor.clone());
            move |_| {
                if let (Some(this), Some(editor)) = (this.upgrade(), weak_editor.upgrade()) {
                    let editor_index = identity_index(&this.state.borrow().editors, &editor);
                    if editor_index >= 0 {
                        this.on_did_model_change.fire(GroupModelChangeEvent::editor_change(GroupModelChangeKind::EditorWillDispose, &editor, editor_index));
                    }
                }
            }
        }));

        // Re-Emit dirty state changes, label changes and capability changes
        let re_emitters = [
            (editor.on_did_change_dirty(), GroupModelChangeKind::EditorDirty),
            (editor.on_did_change_label(), GroupModelChangeKind::EditorLabel),
            (editor.on_did_change_capabilities(), GroupModelChangeKind::EditorCapabilities),
        ];
        for (event, kind) in re_emitters {
            let (this, weak_editor) = (self.this.clone(), weak_editor.clone());
            listeners.add(event.listen(move |_| {
                if let (Some(this), Some(editor)) = (this.upgrade(), weak_editor.upgrade()) {
                    let editor_index = identity_index(&this.state.borrow().editors, &editor);
                    this.on_did_model_change.fire(GroupModelChangeEvent::editor_change(kind, &editor, editor_index));
                }
            }));
        }

        // Clean up dispose listeners once the editor gets closed
        let (this, weak_listeners) = (self.this.clone(), Rc::downgrade(&listeners));
        listeners.add(self.on_did_model_change.event().listen(move |event| {
            if event.kind == GroupModelChangeKind::EditorClose
                && let Some(closed_editor) = &event.editor
                && let Some(editor) = weak_editor.upgrade()
                && closed_editor.matches(&typed(&editor))
                && let Some(listeners) = weak_listeners.upgrade()
            {
                listeners.dispose();
                if let Some(this) = this.upgrade() {
                    this.editor_listeners.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, &listeners));
                }
            }
        }));
    }

    fn replace_editor(&self, to_replace: &EditorInputRef, replace_with: &EditorInputRef, replace_index: i64, open_next: bool) {
        let close_result = self.do_close_editor(to_replace, EditorCloseContext::Replace, open_next); // optimization to prevent multiple setActive() in one call

        // We want to first add the new editor into our model before emitting the close event because firing the close event
        // can trigger a dispose on the same editor that is now being added. This can lead into opening a disposed editor
        // which is not what we want.
        self.splice(replace_index, false, Some(replace_with.clone()));

        if let Some(close_result) = close_result {
            self.on_did_model_change.fire(GroupModelChangeEvent::editor_close(&close_result));
        }
    }

    /// `closeEditor(candidate, context = UNKNOWN, openNext = true)`.
    pub fn close_editor(&self, candidate: &EditorInputRef, context: Option<EditorCloseContext>, open_next: Option<bool>) -> Option<EditorCloseResult> {
        let close_result = self.do_close_editor(candidate, context.unwrap_or(EditorCloseContext::Unknown), open_next.unwrap_or(true))?;

        self.on_did_model_change.fire(GroupModelChangeEvent::editor_close(&close_result));

        Some(close_result)
    }

    fn do_close_editor(&self, candidate: &EditorInputRef, context: EditorCloseContext, open_next: bool) -> Option<EditorCloseResult> {
        let index = self.index_of_editor(Some(candidate), None);
        if index == -1 {
            return None; // not found
        }

        let editor = self.get_editor_by_index(index)?;
        let sticky = self.is_sticky_index(index);

        // Active editor closed
        let is_active_editor = same_editor(self.active_editor().as_ref(), Some(&editor));
        if open_next && is_active_editor {
            let (editors, mru, selection, focus_recent_editor_after_close) = {
                let state = self.state.borrow();
                (state.editors.clone(), state.mru.clone(), state.selection.clone(), state.focus_recent_editor_after_close)
            };

            // More than one editor
            if mru.len() > 1 {
                let new_active = if focus_recent_editor_after_close {
                    mru.get(1) // active editor is always first in MRU, so pick second editor after as new active
                } else if index == editors.len() as i64 - 1 {
                    element(&editors, index - 1) // last editor is closed, pick previous as new active
                } else {
                    element(&editors, index + 1) // pick next editor as new active
                };

                // Select editor as active
                let new_inactive_selected_editors: Vec<EditorInputRef> = selection
                    .iter()
                    .filter(|selected| !same(selected, &editor) && !new_active.is_some_and(|new_active| same(selected, new_active)))
                    .cloned()
                    .collect();
                let new_active_index = new_active.map_or(-1, |new_active| identity_index(&editors, new_active));
                self.do_set_selection(new_active, Some(new_active_index), new_inactive_selected_editors);
            }
            // Last editor closed: clear selection
            else {
                self.do_set_selection(None, None, Vec::new());
            }
        }
        // Inactive editor closed
        else if !is_active_editor {
            // Remove editor from inactive selection
            if self.do_is_selected(&editor) {
                let active_editor = self.active_editor();
                let new_inactive_selected_editors: Vec<EditorInputRef> = self
                    .state
                    .borrow()
                    .selection
                    .iter()
                    .filter(|selected| !same(selected, &editor) && !active_editor.as_ref().is_some_and(|active_editor| same(selected, active_editor)))
                    .cloned()
                    .collect();
                let active_editor_index = self.index_of_editor(active_editor.as_ref(), None);
                self.do_set_selection(active_editor.as_ref(), Some(active_editor_index), new_inactive_selected_editors);
            }
        }

        {
            let mut state = self.state.borrow_mut();

            // Preview Editor closed
            if same_editor(state.preview.as_ref(), Some(&editor)) {
                state.preview = None;
            }

            // Remove from transient
            state.transient.remove(&id_of(&editor));
        }

        // Remove from arrays
        self.splice(index, true, None);

        // Event
        Some(EditorCloseResult { editor, context, editor_index: index, sticky })
    }

    pub fn move_editor(&self, candidate: &EditorInputRef, to_index: i64) -> Option<EditorInputRef> {
        // Ensure toIndex is in bounds of our model
        let editors_count = self.state.borrow().editors.len() as i64;
        let to_index = if to_index >= editors_count {
            editors_count - 1
        } else if to_index < 0 {
            0
        } else {
            to_index
        };

        let index = self.index_of_editor(Some(candidate), None);
        if index < 0 || to_index == index {
            return None;
        }

        let editor = self.get_editor_by_index(index)?;
        let sticky = self.sticky();

        // Adjust sticky index: editor moved out of sticky state into unsticky state
        if self.is_sticky_index(index) && to_index > self.sticky() {
            self.state.borrow_mut().sticky -= 1;
        }
        // ...or editor moved into sticky state from unsticky state
        else if !self.is_sticky_index(index) && to_index <= self.sticky() {
            self.state.borrow_mut().sticky += 1;
        }

        // Move
        {
            let mut state = self.state.borrow_mut();
            js_splice(&mut state.editors, index, 1, std::iter::empty());
            js_splice(&mut state.editors, to_index, 0, [editor.clone()]);
        }

        // Move Event
        self.on_did_model_change.fire(GroupModelChangeEvent::editor_move(&editor, index, to_index));

        // Sticky Event (if sticky changed as part of the move)
        if sticky != self.sticky() {
            self.on_did_model_change.fire(GroupModelChangeEvent::editor_change(GroupModelChangeKind::EditorSticky, &editor, to_index));
        }

        Some(editor)
    }

    pub fn set_active(&self, candidate: Option<&EditorInputRef>) -> Option<EditorInputRef> {
        let Some(candidate) = candidate else {
            // We do not really keep the `active` state in our model because it has no special meaning to us here. But for
            // consistency we emit a `onDidModelChange` event so that components can react.
            self.on_did_model_change.fire(GroupModelChangeEvent::group(GroupModelChangeKind::GroupActive));

            return None;
        };

        let (editor, editor_index) = self.find_editor(Some(candidate), None)?; // not found

        self.do_set_selection(Some(&editor), Some(editor_index), Vec::new());

        Some(editor)
    }

    /// `setSelection(activeSelectedEditorCandidate, inactiveSelectedEditorCandidates)`.
    pub fn set_selection(&self, active_selected_editor_candidate: &EditorInputRef, inactive_selected_editor_candidates: &[EditorInputRef]) {
        let Some((active_selected_editor, active_selected_editor_index)) = self.find_editor(Some(active_selected_editor_candidate), None) else {
            return; // not found
        };

        let mut inactive_selected_editors: Vec<EditorInputRef> = Vec::new();
        for inactive_selected_editor_candidate in inactive_selected_editor_candidates {
            let Some((inactive_selected_editor, _)) = self.find_editor(Some(inactive_selected_editor_candidate), None) else {
                return; // not found
            };

            if same(&inactive_selected_editor, &active_selected_editor) {
                continue; // already selected
            }

            if !inactive_selected_editors.iter().any(|selected| same(selected, &inactive_selected_editor)) {
                inactive_selected_editors.push(inactive_selected_editor);
            }
        }

        self.do_set_selection(Some(&active_selected_editor), Some(active_selected_editor_index), inactive_selected_editors);
    }

    fn do_set_selection(&self, active_selected_editor: Option<&EditorInputRef>, active_selected_editor_index: Option<i64>, inactive_selected_editors: Vec<EditorInputRef>) {
        let previous_active_editor = self.active_editor();
        let previous_selection = self.state.borrow().selection.clone();

        let new_selection: Vec<EditorInputRef> = match active_selected_editor {
            Some(active_selected_editor) => std::iter::once(active_selected_editor.clone()).chain(inactive_selected_editors).collect(),
            None => Vec::new(),
        };

        // Update selection
        self.state.borrow_mut().selection = new_selection.clone();

        // Update active editor if it has changed
        let active_editor_changed = match (active_selected_editor, active_selected_editor_index) {
            (Some(active_selected_editor), Some(index)) if !same_editor(previous_active_editor.as_ref(), Some(active_selected_editor)) => {
                Some((active_selected_editor, index))
            }
            _ => None,
        };
        if let Some((active_selected_editor, active_selected_editor_index)) = active_editor_changed {
            // Bring to front in MRU list
            let mru = self.state.borrow().mru.clone();
            let mru_index = self.index_of_editor(Some(active_selected_editor), Some(mru.as_slice()));
            {
                let mut state = self.state.borrow_mut();
                js_splice(&mut state.mru, mru_index, 1, std::iter::empty());
                state.mru.insert(0, active_selected_editor.clone());
            }

            // Event
            self.on_did_model_change.fire(GroupModelChangeEvent::editor_change(
                GroupModelChangeKind::EditorActive,
                active_selected_editor,
                active_selected_editor_index,
            ));
        }

        // Fire event if the selection has changed
        if active_editor_changed.is_some()
            || previous_selection.len() != new_selection.len()
            || previous_selection.iter().any(|editor| !new_selection.iter().any(|selected| same(selected, editor)))
        {
            self.on_did_model_change.fire(GroupModelChangeEvent::group(GroupModelChangeKind::EditorsSelection));
        }
    }

    pub fn set_index(&self, _index: i64) {
        // We do not really keep the `index` in our model because it has no special meaning to us here. But for consistency
        // we emit a `onDidModelChange` event so that components can react.
        self.on_did_model_change.fire(GroupModelChangeEvent::group(GroupModelChangeKind::GroupIndex));
    }

    pub fn set_label(&self, _label: &str) {
        // We do not really keep the `label` in our model because it has no special meaning to us here. But for consistency
        // we emit a `onDidModelChange` event so that components can react.
        self.on_did_model_change.fire(GroupModelChangeEvent::group(GroupModelChangeKind::GroupLabel));
    }

    pub fn pin(&self, candidate: &EditorInputRef) -> Option<EditorInputRef> {
        let (editor, editor_index) = self.find_editor(Some(candidate), None)?; // not found

        self.do_pin(&editor, editor_index);

        Some(editor)
    }

    fn do_pin(&self, editor: &EditorInputRef, editor_index: i64) {
        if self.is_pinned(&EditorOrIndex::Editor(editor.clone())) {
            return; // can only pin a preview editor
        }

        // Clear Transient
        self.set_transient(editor, false);

        // Convert the preview editor to be a pinned editor
        self.state.borrow_mut().preview = None;

        // Event
        self.on_did_model_change.fire(GroupModelChangeEvent::editor_change(GroupModelChangeKind::EditorPin, editor, editor_index));
    }

    pub fn unpin(&self, candidate: &EditorInputRef) -> Option<EditorInputRef> {
        let (editor, editor_index) = self.find_editor(Some(candidate), None)?; // not found

        self.do_unpin(&editor, editor_index);

        Some(editor)
    }

    fn do_unpin(&self, editor: &EditorInputRef, editor_index: i64) {
        if !self.is_pinned(&EditorOrIndex::Editor(editor.clone())) {
            return; // can only unpin a pinned editor
        }

        // Set new
        let old_preview = self.state.borrow_mut().preview.replace(editor.clone());

        // Event
        self.on_did_model_change.fire(GroupModelChangeEvent::editor_change(GroupModelChangeKind::EditorPin, editor, editor_index));

        // Close old preview editor if any
        if let Some(old_preview) = old_preview {
            self.close_editor(&old_preview, Some(EditorCloseContext::Unpin), None);
        }
    }

    pub fn stick(&self, candidate: &EditorInputRef) -> Option<EditorInputRef> {
        let (editor, editor_index) = self.find_editor(Some(candidate), None)?; // not found

        self.do_stick(&editor, editor_index);

        Some(editor)
    }

    fn do_stick(&self, editor: &EditorInputRef, editor_index: i64) {
        if self.is_sticky_index(editor_index) {
            return; // can only stick a non-sticky editor
        }

        // Pin editor
        self.pin(editor);

        // Move editor to be the last sticky editor
        let new_editor_index = self.sticky() + 1;
        self.move_editor(editor, new_editor_index);

        // Adjust sticky index
        self.state.borrow_mut().sticky += 1;

        // Event
        self.on_did_model_change.fire(GroupModelChangeEvent::editor_change(GroupModelChangeKind::EditorSticky, editor, new_editor_index));
    }

    pub fn unstick(&self, candidate: &EditorInputRef) -> Option<EditorInputRef> {
        let (editor, editor_index) = self.find_editor(Some(candidate), None)?; // not found

        self.do_unstick(&editor, editor_index);

        Some(editor)
    }

    fn do_unstick(&self, editor: &EditorInputRef, editor_index: i64) {
        if !self.is_sticky_index(editor_index) {
            return; // can only unstick a sticky editor
        }

        // Move editor to be the first non-sticky editor
        let new_editor_index = self.sticky();
        self.move_editor(editor, new_editor_index);

        // Adjust sticky index
        self.state.borrow_mut().sticky -= 1;

        // Event
        self.on_did_model_change.fire(GroupModelChangeEvent::editor_change(GroupModelChangeKind::EditorSticky, editor, new_editor_index));
    }

    pub fn set_transient(&self, candidate: &EditorInputRef, transient: bool) -> Option<EditorInputRef> {
        if !transient && self.state.borrow().transient.is_empty() {
            return None; // no transient editor
        }

        let (editor, editor_index) = self.find_editor(Some(candidate), None)?; // not found

        self.do_set_transient(&editor, editor_index, transient);

        Some(editor)
    }

    fn do_set_transient(&self, editor: &EditorInputRef, editor_index: i64, transient: bool) {
        {
            let mut state = self.state.borrow_mut();
            let changed = if transient { state.transient.insert(id_of(editor)) } else { state.transient.remove(&id_of(editor)) };
            if !changed {
                return;
            }
        }

        // Event
        self.on_did_model_change.fire(GroupModelChangeEvent::editor_change(GroupModelChangeKind::EditorTransient, editor, editor_index));
    }

    fn splice(&self, index: i64, del: bool, editor: Option<EditorInputRef>) {
        let editor_to_delete_or_replace = self.get_editor_by_index(index);

        // Perform on sticky index
        if del && self.is_sticky_index(index) {
            self.state.borrow_mut().sticky -= 1;
        }

        // Perform on editors array
        js_splice(&mut self.state.borrow_mut().editors, index, usize::from(del), editor.clone());

        // Perform on MRU
        {
            // Add
            if !del && let Some(editor) = editor {
                let mut state = self.state.borrow_mut();
                if state.mru.is_empty() {
                    // the list of most recent editors is empty so this editor can only be the most recent
                    state.mru.push(editor);
                } else {
                    // we have most recent editors. as such we put this newly opened editor right after the current most
                    // recent one because it cannot be the most recently active one unless it becomes active. but it is
                    // still more active then any other editor in the list.
                    state.mru.insert(1, editor);
                }
            }
            // Remove / Replace
            else {
                let mru = self.state.borrow().mru.clone();
                let index_in_mru = self.index_of_editor(editor_to_delete_or_replace.as_ref(), Some(mru.as_slice()));

                // Remove
                if del && editor.is_none() {
                    js_splice(&mut self.state.borrow_mut().mru, index_in_mru, 1, std::iter::empty()); // remove from MRU
                }
                // Replace
                else if del && let Some(editor) = editor {
                    js_splice(&mut self.state.borrow_mut().mru, index_in_mru, 1, [editor]); // replace MRU at location
                }
            }
        }
    }

    pub fn lock(&self, locked: bool) {
        if self.is_locked() != locked {
            self.state.borrow_mut().locked = locked;

            self.on_did_model_change.fire(GroupModelChangeEvent::group(GroupModelChangeKind::GroupLocked));
        }
    }

    /// `clone()`.
    pub fn clone_model(&self) -> Rc<EditorGroupModel> {
        let clone = self.instantiation_service.create_instance::<EditorGroupModel>(None);

        // Copy over group properties
        {
            let state = self.state.borrow();
            let mut clone_state = clone.state.borrow_mut();
            clone_state.editors = state.editors.clone();
            clone_state.mru = state.mru.clone();
            clone_state.preview = state.preview.clone();
            clone_state.selection = state.selection.clone();
            clone_state.sticky = state.sticky;
        }

        // Ensure to register listeners for each editor
        let editors = clone.state.borrow().editors.clone();
        for editor in &editors {
            clone.register_editor_listeners(editor);
        }

        clone
    }

    pub fn serialize(&self) -> SerializedEditorGroupModel {
        let registry = editor_factory_registry();
        let (editors, mru, preview, sticky, locked) = {
            let state = self.state.borrow();
            (state.editors.clone(), state.mru.clone(), state.preview.clone(), state.sticky, state.locked)
        };

        // Serialize all editor inputs so that we can store them. Editors that cannot be serialized need to be ignored from
        // mru, active, preview and sticky if any.
        let mut serializable_editors: Vec<EditorInputRef> = Vec::new();
        let mut serialized_editors: Vec<SerializedEditorInput> = Vec::new();
        let mut serializable_preview_index: Option<usize> = None;
        let mut serializable_sticky = sticky;

        for (index, editor) in editors.iter().enumerate() {
            let mut can_serialize_editor = false;

            if let Some(editor_serializer) = registry.get_editor_serializer(editor) {
                let value = if editor_serializer.can_serialize(editor) { editor_serializer.serialize(editor) } else { None };

                // Editor can be serialized
                if let Some(value) = value {
                    can_serialize_editor = true;

                    serialized_editors.push(SerializedEditorInput { id: editor.editor_type_id(), value });
                    serializable_editors.push(editor.clone());

                    if same_editor(preview.as_ref(), Some(editor)) {
                        serializable_preview_index = Some(serializable_editors.len() - 1);
                    }
                }
            }

            // Adjust index of sticky editors if the editor cannot be serialized and is pinned
            if !can_serialize_editor && self.is_sticky_index(index as i64) {
                serializable_sticky -= 1;
            }
        }

        let serializable_mru: Vec<usize> = mru
            .iter()
            .filter_map(|editor| usize::try_from(self.index_of_editor(Some(editor), Some(serializable_editors.as_slice()))).ok())
            .collect();

        SerializedEditorGroupModel {
            id: Some(self.id()),
            locked: locked.then_some(true),
            editors: serialized_editors,
            mru: serializable_mru,
            preview: serializable_preview_index,
            sticky: usize::try_from(serializable_sticky).ok(),
        }
    }

    fn deserialize(&self, data: &SerializedEditorGroupModel) {
        let registry = editor_factory_registry();

        match data.id {
            Some(id) => {
                self.id.set(id);

                IDS.with(|ids| ids.set((id + 1).max(ids.get()))); // make sure our ID generator is always larger
            }
            None => self.id.set(next_group_id()), // backwards compatibility
        }

        if data.locked == Some(true) {
            self.state.borrow_mut().locked = true;
        }

        let mut sticky = data.sticky.map(|sticky| sticky as i64);
        let mut editors: Vec<EditorInputRef> = Vec::new();
        for (index, serialized_editor) in data.editors.iter().enumerate() {
            let mut editor: Option<EditorInputRef> = None;

            if let Some(editor_serializer) = registry.get_editor_serializer_by_type_id(&serialized_editor.id)
                && let Some(deserialized_editor) = editor_serializer.deserialize(&self.instantiation_service, &serialized_editor.value)
            {
                self.register_editor_listeners(&deserialized_editor);
                editor = Some(deserialized_editor);
            }

            if editor.is_none()
                && let Some(current_sticky) = sticky
                && (index as i64) <= current_sticky
            {
                sticky = Some(current_sticky - 1); // if editor cannot be deserialized but was sticky, we need to decrease sticky index
            }

            editors.extend(editor);
        }

        let mru: Vec<EditorInputRef> = data.mru.iter().filter_map(|index| editors.get(*index).cloned()).collect();
        let selection: Vec<EditorInputRef> = mru.first().cloned().into_iter().collect();
        let preview = data.preview.and_then(|index| editors.get(index).cloned());

        let mut state = self.state.borrow_mut();
        state.editors = editors;
        state.mru = mru;
        state.selection = selection;
        state.preview = preview;

        if let Some(sticky) = sticky {
            state.sticky = sticky;
        }
    }
}

impl IReadonlyEditorGroupModel for EditorGroupModel {
    fn on_did_model_change(&self) -> Event<GroupModelChangeEvent> {
        self.on_did_model_change.event()
    }

    fn id(&self) -> GroupIdentifier {
        self.id.get()
    }

    fn count(&self) -> usize {
        self.state.borrow().editors.len()
    }

    fn sticky_count(&self) -> usize {
        usize::try_from(self.sticky() + 1).unwrap_or_default()
    }

    fn is_locked(&self) -> bool {
        self.state.borrow().locked
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        self.state.borrow().selection.first().cloned()
    }

    fn preview_editor(&self) -> Option<EditorInputRef> {
        self.state.borrow().preview.clone()
    }

    fn selected_editors(&self) -> Vec<EditorInputRef> {
        let editors = self.state.borrow().editors.clone();

        editors.into_iter().filter(|editor| self.do_is_selected(editor)).collect() // return in sequential order
    }

    fn get_editors(&self, order: EditorsOrder, options: Option<GetEditorsOptions>) -> Vec<EditorInputRef> {
        let editors = {
            let state = self.state.borrow();
            if order == EditorsOrder::MostRecentlyActive { state.mru.clone() } else { state.editors.clone() }
        };

        if options.is_some_and(|options| options.exclude_sticky == Some(true)) {
            // MRU: need to check for index on each
            if order == EditorsOrder::MostRecentlyActive {
                return editors.into_iter().filter(|editor| !self.is_sticky(&EditorOrIndex::Editor(editor.clone()))).collect();
            }

            // Sequential: simply start after sticky index
            let sticky_count = self.sticky_count();
            return editors.into_iter().skip(sticky_count).collect();
        }

        editors
    }

    fn get_editor_by_index(&self, index: i64) -> Option<EditorInputRef> {
        element(&self.state.borrow().editors, index).cloned()
    }

    fn index_of(&self, candidate: Option<&EditorInputOrUntyped>, editors: Option<&[EditorInputRef]>, options: Option<MatchEditorOptions>) -> i64 {
        let Some(candidate) = candidate else { return -1 };

        let editors = self.editors_or_all(editors);
        let support_side_by_side = options.is_some_and(|options| options.support_side_by_side.is_some());

        let mut index = -1;
        for (i, editor) in editors.iter().enumerate() {
            if matches(Some(editor), Some(candidate), options) {
                // If we are to support side by side matching, it is possible that a better direct match is found later. As
                // such, we continue finding a matching editor and prefer that match over the side by side one.
                if support_side_by_side && editor.side_by_side_editors().is_some() && !is_side_by_side_editor_input(Some(candidate)) {
                    index = i as i64;
                } else {
                    index = i as i64;
                    break;
                }
            }
        }

        index
    }

    fn is_active(&self, candidate: &EditorInputOrUntyped) -> bool {
        matches(self.active_editor().as_ref(), Some(candidate), None)
    }

    fn is_pinned(&self, editor_candidate_or_index: &EditorOrIndex) -> bool {
        let editor = match editor_candidate_or_index {
            EditorOrIndex::Index(index) => self.get_editor_by_index(*index),
            EditorOrIndex::Editor(editor) => Some(editor.clone()),
        };

        !matches(self.preview_editor().as_ref(), editor.as_ref().map(typed).as_ref(), None)
    }

    fn is_sticky(&self, candidate_or_index: &EditorOrIndex) -> bool {
        if self.sticky() < 0 {
            return false; // no sticky editor
        }

        let index = match candidate_or_index {
            EditorOrIndex::Index(index) => *index,
            EditorOrIndex::Editor(candidate) => self.index_of_editor(Some(candidate), None),
        };

        self.is_sticky_index(index)
    }

    fn is_selected(&self, editor_candidate_or_index: &EditorOrIndex) -> bool {
        let editor = match editor_candidate_or_index {
            EditorOrIndex::Index(index) => self.get_editor_by_index(*index),
            EditorOrIndex::Editor(candidate) => self.find_editor(Some(candidate), None).map(|(editor, _)| editor),
        };

        editor.is_some_and(|editor| self.do_is_selected(&editor))
    }

    fn is_transient(&self, editor_candidate_or_index: &EditorOrIndex) -> bool {
        if self.state.borrow().transient.is_empty() {
            return false; // no transient editor
        }

        let editor = match editor_candidate_or_index {
            EditorOrIndex::Index(index) => self.get_editor_by_index(*index),
            EditorOrIndex::Editor(candidate) => self.find_editor(Some(candidate), None).map(|(editor, _)| editor),
        };

        editor.is_some_and(|editor| self.state.borrow().transient.contains(&id_of(&editor)))
    }

    fn is_first(&self, candidate: &EditorInputRef, editors: Option<&[EditorInputRef]>) -> bool {
        matches(self.editors_or_all(editors).first(), Some(&typed(candidate)), None)
    }

    fn is_last(&self, candidate: &EditorInputRef, editors: Option<&[EditorInputRef]>) -> bool {
        matches(self.editors_or_all(editors).last(), Some(&typed(candidate)), None)
    }

    fn find_editor(&self, candidate: Option<&EditorInputRef>, options: Option<MatchEditorOptions>) -> Option<(EditorInputRef, i64)> {
        let editors = self.state.borrow().editors.clone();
        let index = self.index_of(candidate.map(typed).as_ref(), Some(editors.as_slice()), options);
        if index == -1 {
            return None;
        }

        element(&editors, index).map(|editor| (editor.clone(), index))
    }

    fn contains(&self, candidate: &EditorInputOrUntyped, options: Option<MatchEditorOptions>) -> bool {
        self.index_of(Some(candidate), None, options) != -1
    }
}

impl IDisposable for EditorGroupModel {
    fn dispose(&self) {
        let editor_listeners = std::mem::take(&mut *self.editor_listeners.borrow_mut());
        for listeners in &editor_listeners {
            listeners.dispose();
        }

        self.state.borrow_mut().transient.clear();

        self.base.dispose();
    }
}
