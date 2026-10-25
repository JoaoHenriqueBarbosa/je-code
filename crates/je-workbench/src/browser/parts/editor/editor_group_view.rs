//! Porte de `src/vs/workbench/browser/parts/editor/editorGroupView.ts`: o [`EditorGroupView`] lógico (sem DOM), que guarda o
//! [`EditorGroupModel`], a identidade (`id`, índice, rótulo), a vista do grid (`ISerializableView`: tamanhos mínimos e
//! máximos, `layout`, `toJSON`) e todas as operações de editor do grupo: abrir e ativar (`openEditor`, `openEditors`),
//! fechar (`closeEditor`, `closeEditors`, `closeAllEditors`, com a confirmação de editor sujo e o `CannotClose`), mover e
//! copiar entre grupos, substituir, aderentes, transientes, seleção e bloqueio. O `EditorPart` usa este tipo como a vista de
//! cada grupo (ver `PORT_PLAN.md`).
//!
//! Divergências:
//! - **Sem DOM.** O `element` é um nó do `je-ui` (o que o grid precisa para posicionar a vista). O título
//!   (`EditorTitleControl`), a barra de progresso, o `Watermark`, a barra de ferramentas e o menu de contexto do grupo vazio,
//!   o telemetria de `editorOpened`, o `trackFocus`, os toques e cliques do contêiner e `setBoundarySashes` são pintura ou
//!   entrada do DOM e vêm com a renderização nativa. Toda chamada ao `titleControl` (`openEditor`, `closeEditor`,
//!   `moveEditor`, `pinEditor`, `stickEditor`...) some, e com ela as opções de atualização do título (`skipTitleUpdate`,
//!   `preserveWindowOrder`).
//! - **Classes e estilos** (`classList` e `style` do `element`, do `titleContainer` e do `editorContainer`) são
//!   [`ElementStyle`], que o desenho nativo lê: `updateContainer` (`empty`, `active`, `inactive`, `max-height-478px`),
//!   `updateTitleContainer` (`tabs`, `show-file-icons`) e `updateStyles` (as cores de `EDITOR_GROUP_EMPTY_BACKGROUND`,
//!   `EDITOR_GROUP_HEADER_*` e `editorBackground`, o `--title-border-bottom-color` e o `--title-border-bottom-width`). O
//!   `Themable` é o `IThemeService` lido do `InstantiationService` (sem ele o grupo não pinta cores) e `getColor` é o texto
//!   CSS da cor do tema vigente.
//! - **O painel de editor** (`EditorPanes`) é o registro lógico do editor visível de [`editor_panes`](super::editor_panes): o
//!   `activeEditorPane` devolve o painel lógico (a entrada atribuída, as opções e o id registrado), `onDidActiveEditorChange`
//!   e `onDidOpenEditorFail` disparam como no original, e as restrições de tamanho do painel (`minimumWidth` e companhia)
//!   valem sempre os padrões (`DEFAULT_EDITOR_MIN_DIMENSIONS` e `DEFAULT_EDITOR_MAX_DIMENSIONS`), então `onDidChange` nunca
//!   dispara.
//! - **Foco.** Sem foco do DOM, `shouldRestoreFocus` é sempre falso e `focus()` só dispara `onDidFocus` (o efeito observável;
//!   o `containerFocusTracker` e o foco do painel chegariam ao mesmo evento). `hostService.focus` (trazer a janela para a
//!   frente antes do diálogo de salvar) não existe no `IHostService` portado e não tem efeito sem janela. O
//!   `IEditorGroupViewOptions.preserveFocus` do construtor só decidia dar foco depois de restaurar: some.
//! - **`groupsView` e `editorPartsView`** são o trait [`EditorGroupsView`] (a parte que contém o grupo e a visão de todas as
//!   partes numa coisa só: o `IEditorGroupsView` e o `IEditorPartsView` do original). `editorPartsView.groups` e
//!   `editorPartsView.getGroup` são os grupos de todas as partes quando a parte tem um `EditorParts` (a parte lê pelo trait
//!   `EditorPartsView`) e os da própria parte quando está avulsa. O `windowId` que o grupo lê da `groupsView` é o parâmetro
//!   `window_id`.
//! - **Serviços** lidos na hora de usar, pelo `InstantiationService` (o original os recebe no construtor):
//!   `IFileDialogService` (a confirmação de salvar), `IDialogService` (o erro de editor que não pode mover),
//!   `IFilesConfigurationService` (o modo de auto salvar), `IUriIdentityService` (`findEditors`) e o `ILogService` (opcional:
//!   sem ele os erros não são registrados). O `IEditorResolverService`, o `IFileService`, o `IMenuService`, o
//!   `ITelemetryService`, o `IKeybindingService` e o `IContextMenuService` só serviam ao título, aos context keys do grupo e
//!   ao menu, e não entram. Os context keys do grupo (`handleGroupContextKeys`, `activeEditorDirty` e companhia) ficam fora: as chaves
//!   existem em `common/contextkeys.rs`, mas ligá-las pede o `ResourceContextKey` (`ILanguageService`, `IModelService`,
//!   `IFileService`), que ainda não foi portado.
//! - **Promessas.** As funções `async` do original são [`async_fn`]: o corpo roda já, de forma síncrona, até o primeiro
//!   `await`, e cada `await` custa uma rodada de microtarefas. `closeAllEditors({ excludeConfirming: true })` devolve a
//!   promessa já resolvida (o original devolve `boolean`; o método irmão [`IEditorGroup::close_all_editors_excluding_confirming`]
//!   devolve o `bool`).
//! - **`createEditorActions`** só tem o ramo sem painel de editor com menus (o `activeEditorPane instanceof EditorPane`): sem
//!   `IMenuService` o grupo devolve nenhuma ação e avisa `onDidChange` a cada troca de editor ativo.
//! - **Eventos do modelo.** O grupo tem o próprio `onDidModelChange`: repassa cada evento do modelo e só então cuida dele
//!   (abrir, fechar, descartar, sujar, transiente), a mesma ordem do original.
//! - **`createNew`, `createFromSerialized` e `createCopy`** são as variantes de [`EditorGroupSource`].
//! - **Alvo de mover e copiar.** O original tipa o alvo como `EditorGroupView`; o contrato [`IEditorGroup`] passa
//!   `Rc<dyn IEditorGroup>`, e o alvo é achado pelo `id` na [`EditorGroupsView`] (identidade de grupo é o `id`).
//! - **Identidade de editor.** Onde o original compara com `===` ou `Map<EditorInput, ...>`, o porte compara o ponteiro
//!   (`id_of`).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::{Rc, Weak};

use je_base::async_::{RunOnceWorker, async_fn, promises};
use je_base::errors::{self, Error, throw};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableId, DisposableStore, IDisposable, id_of};
use je_base::nls::localize;
use je_base::promise::{DeferredPromise, Promise};
use je_base::uri::URI;
use je_editor::services::theme_service::{IThemeService, I_THEME_SERVICE};
use je_instantiation::InstantiationService;
use je_platform::actions::actions::{MenuChangeEvent, MenuId};
use je_platform::contextkey::contextkey::{IContextKeyService, IContextKeyServiceTarget, IScopedContextKeyService};
use je_platform::dialogs::{ConfirmResult, FileNameOrResource, I_DIALOG_SERVICE, I_FILE_DIALOG_SERVICE};
use je_platform::log::I_LOG_SERVICE;
use je_platform::opener::editor_types::{EditorActivation, EditorOptions};
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::I_URI_IDENTITY_SERVICE;
use je_ui::{NodeId, Role, Ui};
use je_widgets::grid::{GridItem, SerializableView};
use je_widgets::gridview::{GridViewView, ViewDimensions};

use crate::browser::parts::editor::editor::{DEFAULT_EDITOR_MAX_DIMENSIONS, DEFAULT_EDITOR_MIN_DIMENSIONS, fill_active_editor_view_state};
use crate::browser::parts::editor::editor_panes::EditorPanes;
use crate::common::editor::editor_group_model::{
    EditorGroupModel, EditorOpenOptions, EditorOpenResult, EditorOrIndex, GetEditorsOptions, GroupModelChangeEvent, IReadonlyEditorGroupModel,
    SerializedEditorGroupModel, is_group_editor_close_event,
};
use crate::common::editor::editor_input::CanMove;
use crate::common::theme::{
    CONTRAST_BORDER, EDITOR_BACKGROUND, EDITOR_GROUP_EMPTY_BACKGROUND, EDITOR_GROUP_HEADER_BORDER, EDITOR_GROUP_HEADER_NO_TABS_BACKGROUND,
    EDITOR_GROUP_HEADER_TABS_BACKGROUND,
};
use crate::common::editor::{
    ActiveEditorChangeEvent, CloseDirection, EditorCloseContext, EditorCloseEvent, EditorIdentifier, EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef,
    EditorInputWithOptions, EditorOpenContext, EditorResourceAccessor, EditorResourceAccessorOptions, EditorWillMoveEvent, EditorWillOpenEvent, EditorsOrder,
    FindEditorOptions, GroupIdentifier, GroupModelChangeKind, IEditorPane, MatchEditorOptions, RevertOptions, SaveOptions, SaveReason, SideBySideEditor, TextEditorOptions,
};
use crate::services::editor::editor_groups_service::{
    ActiveEditorActions, CloseAllEditorsOptions, CloseEditorOptions, CloseEditorsFilter, CloseEditorsTarget, EditorPartOptions, EditorPartOptionsChangeEvent,
    EditorReplacement, GroupsOrder, IEditorGroup, ShowTabs, ToolbarActions,
};
use crate::services::files_configuration::files_configuration_service::{AutoSaveMode, I_FILES_CONFIGURATION_SERVICE, ResourceOrEditor};
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

/// `IEditorGroupsView` e `IEditorPartsView` do que o grupo lê: a parte que o contém (as opções, o grupo ativo, ativar,
/// restaurar e remover grupos) e a lista de todos os grupos (`editorPartsView.groups`).
pub trait EditorGroupsView {
    /// `partOptions`.
    fn part_options(&self) -> EditorPartOptions;

    /// `onDidChangeEditorPartOptions`.
    fn on_did_change_editor_part_options(&self) -> Event<EditorPartOptionsChangeEvent>;

    /// `activeGroup`.
    fn active_group(&self) -> Rc<EditorGroupView>;

    /// `activateGroup(group)`.
    fn activate_group(&self, group: &Rc<EditorGroupView>);

    /// `restoreGroup(group)`.
    fn restore_group(&self, group: &Rc<EditorGroupView>);

    /// `removeGroup(group)`.
    fn remove_group(&self, group: &Rc<EditorGroupView>);

    /// `getGroups(order)`.
    fn get_groups(&self, order: GroupsOrder) -> Vec<Rc<EditorGroupView>>;

    /// `editorPartsView.groups`: todos os grupos.
    fn groups(&self) -> Vec<Rc<EditorGroupView>>;

    /// `editorPartsView.getGroup(identifier)`.
    fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<EditorGroupView>>;
}

/// `IInternalEditorOpenOptions`, sem as opções que só atualizam o título e a ordem das janelas.
#[derive(Clone, Default)]
pub struct InternalEditorOpenOptions {
    /// The inactive selection is the set of editors that are selected alongside the editor that is being opened.
    pub inactive_selection: Option<Vec<EditorInputRef>>,
    /// Allow to match on a side-by-side editor when same editor is opened on both sides.
    pub support_side_by_side: Option<SideBySideEditor>,
    /// As opções do `openEditor` como `ITextEditorOptions` (a seleção do editor de texto, que o painel recebe junto com as
    /// opções base).
    pub text_options: Option<TextEditorOptions>,
}

/// `IInternalEditorCloseOptions`, sem `skipTitleUpdate`.
#[derive(Clone, Copy, Default)]
pub struct InternalEditorCloseOptions {
    /// The context in which the editor is closed (para o modelo).
    pub context: Option<EditorCloseContext>,
    /// Closing because opening the editor failed (`fromError`).
    pub from_error: bool,
    /// Forces to close the editor even when it declares `CannotClose`.
    pub force: bool,
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

/// `EditorInput | IUntypedEditorInput` para uma entrada tipada.
fn typed(editor: &EditorInputRef) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Typed(editor.clone())
}

/// `a === b` entre entradas.
fn same_editor(a: &EditorInputRef, b: &EditorInputRef) -> bool {
    id_of(a) == id_of(b)
}

/// `a === b` entre vistas de grupo (a identidade do grupo é o `id`).
fn same_view(a: &EditorGroupView, b: &EditorGroupView) -> bool {
    a.id() == b.id()
}

/// `array.slice(start, end)` do JavaScript: índices negativos contam do fim e os fora da lista valem as pontas.
fn js_slice<T: Clone>(items: &[T], start: i64, end: Option<i64>) -> Vec<T> {
    let len = items.len() as i64;
    let resolve = |index: i64| if index < 0 { (len + index).max(0) } else { index.min(len) };
    let (from, to) = (resolve(start), end.map_or(len, resolve));
    if from >= to {
        return Vec::new();
    }

    items[from as usize..to as usize].to_vec()
}

/// O que o original escreve no `style` e na `classList` de um elemento do grupo (`element`, `titleContainer`,
/// `editorContainer`), para o desenho nativo ler.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElementStyle {
    /// `classList`.
    pub classes: BTreeSet<String>,
    /// `style.backgroundColor` (vazio é a ausência da declaração).
    pub background_color: String,
    /// `style.setProperty(name, value)`: as variáveis CSS.
    pub properties: BTreeMap<String, String>,
}

impl ElementStyle {
    /// `classList.contains(name)`.
    pub fn has_class(&self, name: &str) -> bool {
        self.classes.contains(name)
    }

    /// `style.getPropertyValue(name)`.
    pub fn property(&self, name: &str) -> Option<&str> {
        self.properties.get(name).map(String::as_str)
    }

    /// `classList.toggle(name, force)`.
    fn toggle_class(&mut self, name: &str, force: bool) {
        if force {
            self.classes.insert(name.to_string());
        } else {
            self.classes.remove(name);
        }
    }
}

/// Os elementos de estilo do grupo.
#[derive(Clone, Debug, Default)]
struct GroupStyle {
    container: ElementStyle,
    title: ElementStyle,
    editor: ElementStyle,
    /// O valor de reserva de `var(--modern-ui-editor-title-background, <reserva>)`.
    title_background_fallback: String,
}

/// `EditorGroupView`.
pub struct EditorGroupView {
    ui: Ui,
    element: NodeId,
    model: Rc<EditorGroupModel>,
    window_id: WindowId,
    instantiation_service: Rc<InstantiationService>,
    groups_view: Weak<dyn EditorGroupsView>,
    editor_panes: Rc<EditorPanes>,
    groups_label: RefCell<String>,
    index: Cell<usize>,
    active: Cell<Option<bool>>,
    last_layout: Cell<Option<LastLayout>>,
    content_right_inset: Cell<f64>,
    style: RefCell<GroupStyle>,
    theme_service: Option<Rc<dyn IThemeService>>,
    disposed: Cell<bool>,
    scoped_context_key_service: Rc<dyn IScopedContextKeyService>,
    map_editor_to_pending_confirmation: RefCell<HashMap<DisposableId, Promise<bool>>>,
    disposed_editors_worker: Rc<RunOnceWorker<EditorInputRef>>,
    when_restored: Rc<DeferredPromise<()>>,
    on_did_focus: Rc<Emitter<()>>,
    on_will_dispose: Rc<Emitter<()>>,
    on_did_model_change: Rc<Emitter<GroupModelChangeEvent>>,
    on_did_active_editor_change: Rc<Emitter<ActiveEditorChangeEvent>>,
    on_did_open_editor_fail: Rc<Emitter<EditorInputRef>>,
    on_will_close_editor: Rc<Emitter<EditorCloseEvent>>,
    on_did_close_editor: Rc<Emitter<EditorCloseEvent>>,
    on_will_move_editor: Rc<Emitter<EditorWillMoveEvent>>,
    on_will_open_editor: Rc<Emitter<EditorWillOpenEvent>>,
    on_did_change: Rc<Emitter<Option<ViewDimensions>>>,
    disposables: Rc<DisposableStore>,
    this: Weak<EditorGroupView>,
}

impl EditorGroupView {
    /// O construtor do original (`createNew`, `createFromSerialized` e `createCopy` escolhem o `from`).
    pub fn new(
        from: EditorGroupSource,
        ui: &Ui,
        instantiation_service: &Rc<InstantiationService>,
        context_key_service: &dyn IContextKeyService,
        groups_view: Weak<dyn EditorGroupsView>,
        window_id: WindowId,
        groups_label: &str,
        index: usize,
    ) -> Rc<Self> {
        let disposables = DisposableStore::new();

        let copy_from = match &from {
            EditorGroupSource::Copy(source) => Some(source.clone()),
            _ => None,
        };
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

        // Editor pane
        let editor_panes = disposables.add_rc(EditorPanes::new(model.id(), instantiation_service.clone()));

        // O `Themable`: o tema vigente é opcional (sem o serviço o grupo não pinta cores)
        let theme_service = instantiation_service.invoke_function(|accessor| accessor.try_get(I_THEME_SERVICE));

        let this = Rc::new_cyclic(|this: &Weak<EditorGroupView>| {
            let worker_group = this.clone();
            Self {
                ui: ui.clone(),
                element,
                model,
                window_id,
                instantiation_service: instantiation_service.clone(),
                groups_view,
                editor_panes,
                groups_label: RefCell::new(groups_label.to_string()),
                index: Cell::new(index),
                active: Cell::new(None),
                last_layout: Cell::new(None),
                content_right_inset: Cell::new(0.0),
                style: RefCell::new(GroupStyle::default()),
                theme_service,
                disposed: Cell::new(false),
                scoped_context_key_service,
                map_editor_to_pending_confirmation: RefCell::new(HashMap::new()),
                disposed_editors_worker: disposables.add_rc(RunOnceWorker::new(
                    move |editors| {
                        if let Some(group) = worker_group.upgrade() {
                            group.handle_disposed_editors(editors);
                        }
                    },
                    0,
                )),
                when_restored: Rc::new(DeferredPromise::new()),
                on_did_focus: disposables.add_rc(Emitter::new()),
                on_will_dispose: disposables.add_rc(Emitter::new()),
                on_did_model_change: disposables.add_rc(Emitter::new()),
                on_did_active_editor_change: disposables.add_rc(Emitter::new()),
                on_did_open_editor_fail: disposables.add_rc(Emitter::new()),
                on_will_close_editor: disposables.add_rc(Emitter::new()),
                on_did_close_editor: disposables.add_rc(Emitter::new()),
                on_will_move_editor: disposables.add_rc(Emitter::new()),
                on_will_open_editor: disposables.add_rc(Emitter::new()),
                on_did_change: disposables.add_rc(Emitter::new()),
                disposables,
                this: this.clone(),
            }
        });

        // Update containers
        this.update_title_container();
        this.update_container();

        // Restore editors if provided, and signal restored once editors have restored
        let restore_editors_promise = this.restore_editors(copy_from.as_ref()).unwrap_or_else(|| Promise::resolved(()));
        let when_restored = this.when_restored.clone();
        restore_editors_promise.finally(move || {
            when_restored.complete(());
        });

        // Register Listeners
        this.register_listeners();

        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o grupo está vivo enquanto é usado")
    }

    fn groups_view(&self) -> Rc<dyn EditorGroupsView> {
        self.groups_view.upgrade().expect("a parte está viva enquanto os grupos dela são usados")
    }

    fn part_options(&self) -> EditorPartOptions {
        self.groups_view().part_options()
    }

    /// `this.count` (`isEmpty` é `count === 0`).
    fn editor_count(&self) -> usize {
        self.model.count()
    }

    /// O serviço de log é opcional: sem ele os erros não são registrados.
    fn log_error(&self, error: &Error) {
        if let Some(log_service) = self.instantiation_service.invoke_function(|accessor| accessor.try_get(I_LOG_SERVICE)) {
            log_service.error(&error.to_string());
        }
    }

    // region: restauração e eventos

    /// `restoreEditors(from, groupViewOptions)`.
    fn restore_editors(&self, from: Option<&Rc<EditorGroupView>>) -> Option<Promise<()>> {
        if self.editor_count() == 0 {
            return None; // nothing to show
        }

        // Determine editor options
        let mut options = match from {
            // if we copy from another group, ensure to copy its active editor viewstate
            Some(from) => fill_active_editor_view_state(&**from, None, None),
            None => EditorOptions::default(),
        };

        let active_editor = self.model.active_editor()?;

        options.pinned = Some(self.model.is_pinned(&EditorOrIndex::Editor(active_editor.clone()))); // preserve pinned state
        options.sticky = Some(self.model.is_sticky(&EditorOrIndex::Editor(active_editor.clone()))); // preserve sticky state
        options.preserve_focus = Some(true); // handle focus after editor is restored

        // Show active editor (intentionally not using async to keep `restoreEditors` from executing in same stack)
        Some(self.do_show_editor(&active_editor, true, false /* restored */, Some(options), None).then(|_| ()))
    }

    fn register_listeners(&self) {
        let weak = self.this.clone();

        // Model Events
        self.disposables.add(self.model.on_did_model_change().listen({
            let weak = weak.clone();
            move |event| {
                if let Some(group) = weak.upgrade() {
                    group.on_did_group_model_change(event);
                }
            }
        }));

        // Option Changes
        self.disposables.add(self.groups_view().on_did_change_editor_part_options().listen({
            let weak = weak.clone();
            move |event| {
                if let Some(group) = weak.upgrade() {
                    group.on_did_change_editor_part_options(event);
                }
            }
        }));

        // Theme Changes (`Themable.onThemeChange`)
        if let Some(theme_service) = &self.theme_service {
            self.disposables.add(theme_service.on_did_color_theme_change().listen({
                let weak = weak.clone();
                move |_| {
                    if let Some(group) = weak.upgrade() {
                        group.update_styles();
                    }
                }
            }));
        }

        // Focus
        self.disposables.add(self.on_did_focus.event().listen(move |_| {
            if let Some(group) = weak.upgrade() {
                group.on_did_gain_focus();
            }
        }));
    }

    fn on_did_group_model_change(&self, e: &GroupModelChangeEvent) {
        // Re-emit to outside
        self.on_did_model_change.fire(e.clone());

        // Handle within
        let Some(editor) = &e.editor else {
            return;
        };

        match e.kind {
            // `onDidOpenEditor` publica telemetria (pintura, fora do porte) e atualiza o contêiner
            GroupModelChangeKind::EditorOpen => self.update_container(),
            GroupModelChangeKind::EditorClose => {
                if is_group_editor_close_event(e)
                    && let (Some(index), Some(context), Some(sticky)) = (e.editor_index, e.context, e.sticky)
                {
                    self.handle_on_did_close_editor(editor, index as usize, context, sticky);
                }
            }
            GroupModelChangeKind::EditorWillDispose => self.on_will_dispose_editor(editor),
            GroupModelChangeKind::EditorDirty => self.on_did_change_editor_dirty(editor),
            GroupModelChangeKind::EditorTransient => self.on_did_change_editor_transient(editor),
            // `EDITOR_LABEL` e `EDITOR_CAPABILITIES` só atualizavam o título
            _ => {}
        }
    }

    fn handle_on_did_close_editor(&self, editor: &EditorInputRef, editor_index: usize, context: EditorCloseContext, sticky: bool) {
        // Before close
        self.on_will_close_editor.fire(EditorCloseEvent { group_id: self.id(), editor: editor.clone(), context, index: editor_index, sticky });

        // Handle event
        let mut editors_to_close = vec![editor.clone()];

        // Include both sides of side by side editors when being closed
        if let Some((primary, secondary)) = editor.side_by_side_editors() {
            editors_to_close.push(primary);
            editors_to_close.push(secondary);
        }

        // For each editor to close, we call dispose() to free up any resources. However, certain editors might be shared
        // across multiple editor groups (including being visible in side by side / diff editors) and as such we only dispose
        // when they are not opened elsewhere.
        for editor in &editors_to_close {
            if self.can_dispose(editor) {
                editor.dispose();
            }
        }

        // Update container
        self.update_container();

        // Event
        self.on_did_close_editor.fire(EditorCloseEvent { group_id: self.id(), editor: editor.clone(), context, index: editor_index, sticky });
    }

    fn can_dispose(&self, editor: &EditorInputRef) -> bool {
        for group_view in self.groups_view().groups() {
            if group_view.model.contains(
                &typed(editor),
                Some(MatchEditorOptions {
                    strict_equals: Some(true),                            // only if this input is not shared across editor groups
                    support_side_by_side: Some(SideBySideEditor::Any), // include any side of an opened side by side editor
                }),
            ) {
                return false;
            }
        }

        true
    }

    fn on_will_dispose_editor(&self, editor: &EditorInputRef) {
        // To prevent race conditions, we handle disposed editors in our worker with a timeout because it can happen that an
        // input is being disposed with the intent to replace it with some other input right after.
        self.disposed_editors_worker.work(editor.clone());
    }

    fn handle_disposed_editors(&self, disposed_editors: Vec<EditorInputRef>) {
        // Split between visible and hidden editors
        let mut active_editor: Option<EditorInputRef> = None;
        let mut inactive_editors: Vec<EditorInputRef> = Vec::new();
        for disposed_editor in &disposed_editors {
            let Some((editor, _)) = self.model.find_editor(Some(disposed_editor), None) else {
                continue; // not part of the model anymore
            };

            if !editor.is_disposed() {
                continue; // editor got reopened meanwhile
            }

            if self.model.is_active(&typed(&editor)) {
                active_editor = Some(editor);
            } else {
                inactive_editors.push(editor);
            }
        }

        // Close all inactive editors first to prevent UI flicker
        for inactive_editor in &inactive_editors {
            self.do_close_editor(inactive_editor, Some(true), InternalEditorCloseOptions::default());
        }

        // Close active one last
        if let Some(active_editor) = &active_editor {
            self.do_close_editor(active_editor, Some(true), InternalEditorCloseOptions::default());
        }
    }

    fn on_did_change_editor_part_options(&self, event: &EditorPartOptionsChangeEvent) {
        // Title container
        self.update_title_container();

        // Styles
        self.update_styles();

        // Pin preview editor once user disables preview
        if event.old_part_options.enable_preview
            && !event.new_part_options.enable_preview
            && let Some(preview_editor) = self.model.preview_editor()
        {
            self.pin_editor_candidate(Some(&preview_editor));
        }
    }

    fn on_did_change_editor_dirty(&self, editor: &EditorInputRef) {
        // Always show dirty editors pinned
        self.pin_editor_candidate(Some(editor));
    }

    fn on_did_change_editor_transient(&self, editor: &EditorInputRef) {
        let transient = self.model.is_transient(&EditorOrIndex::Editor(editor.clone()));

        // Transient state overrides the `enablePreview` setting, so when an editor leaves the transient state, we have to
        // ensure its preview state is also cleared.
        if !transient && !self.part_options().enable_preview {
            self.pin_editor_candidate(Some(editor));
        }
    }

    fn on_did_gain_focus(&self) {
        if let Some(active_editor) = self.model.active_editor() {
            // We aggressively clear the transient state of editors as soon as the group gains focus. This is to ensure that
            // the transient state is not staying around when the user interacts with the editor.
            self.model.set_transient(&active_editor, false);
        }
    }

    // endregion

    // region: IEditorGroupView

    /// `onDidFocus`.
    pub fn on_did_focus(&self) -> Event<()> {
        self.on_did_focus.event()
    }

    /// `onDidOpenEditorFail`.
    pub fn on_did_open_editor_fail(&self) -> Event<EditorInputRef> {
        self.on_did_open_editor_fail.event()
    }

    /// `whenRestored`.
    pub fn when_restored(&self) -> Promise<()> {
        self.when_restored.promise.clone()
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
            let _ = self.set_selection(&active_editor, Vec::new());
        }

        // Update container
        {
            let mut style = self.style.borrow_mut();
            style.container.toggle_class("active", is_active);
            style.container.toggle_class("inactive", !is_active);
        }

        // Update styles
        self.update_styles();

        // Update model
        self.model.set_active(None /* entire group got active */);
    }

    /// `layout(width, height, top, left)`: guarda o tamanho e escreve o que o original escreve no título (a classe
    /// `max-height-478px` e `--title-border-bottom-width`). O título e o painel de editor, que repartem a altura, vêm depois.
    fn do_layout(&self, width: f64, height: f64) {
        self.last_layout.set(Some(LastLayout { width, height }));

        let content_width = (width - self.content_right_inset.get()).max(0.0);
        let mut style = self.style.borrow_mut();
        style.container.toggle_class("max-height-478px", height <= 478.0);
        style.title.properties.insert("--title-border-bottom-width".to_string(), format!("{content_width}px"));

        // The editor pane is inset on the right by `_contentRightInset` (0 = fill the group): `editorContainer.style.width`
        style.editor.properties.insert("width".to_string(), format!("{content_width}px"));
    }

    /// `setContentRightInset(inset)`: reserva à direita do cabeçalho e do painel de editor, com as abas de largura total.
    pub fn set_content_right_inset(&self, inset: f64) {
        let next = inset.round().max(0.0);
        if next == self.content_right_inset.get() {
            return;
        }

        self.content_right_inset.set(next);
        self.relayout();
    }

    /// `relayout()`.
    pub fn relayout(&self) {
        if let Some(last_layout) = self.last_layout.get() {
            self.do_layout(last_layout.width, last_layout.height);
        }
    }

    /// `titleContainer`: as classes e o estilo que o grupo escreve nele.
    pub fn title_container_style(&self) -> ElementStyle {
        self.style.borrow().title.clone()
    }

    /// `element`: as classes e o estilo que o grupo escreve nele.
    pub fn container_style(&self) -> ElementStyle {
        self.style.borrow().container.clone()
    }

    /// `editorContainer`: o estilo que o grupo escreve nele.
    pub fn editor_container_style(&self) -> ElementStyle {
        self.style.borrow().editor.clone()
    }

    /// O fundo computado do título: `var(--modern-ui-editor-title-background, <reserva>)` vale a variável quando a raiz a
    /// define (`tabs.css`, com `.modern-ui-tabs`: `--modern-ui-editor-title-background: var(--vscode-editor-background)`) e
    /// a reserva (a cor do cabeçalho do tema) senão.
    pub fn computed_title_background(&self, modern_ui_title_background: Option<&str>) -> String {
        modern_ui_title_background.map_or_else(|| self.style.borrow().title_background_fallback.clone(), str::to_string)
    }

    /// `Themable.getColor(id)`: a cor do tema vigente em CSS, `None` quando o tema não a define.
    fn get_color(&self, id: &str) -> Option<String> {
        let theme = self.theme_service.as_ref()?.get_color_theme();
        theme.get_color(id, None).map(|color| color.to_string())
    }

    /// `updateTitleContainer()`.
    fn update_title_container(&self) {
        let Some(groups_view) = self.groups_view.upgrade() else { return };
        let options = groups_view.part_options();

        let mut style = self.style.borrow_mut();
        style.title.toggle_class("tabs", matches!(options.show_tabs, ShowTabs::Multiple));
        style.title.toggle_class("show-file-icons", options.show_icons);
    }

    /// `updateContainer()`: o grupo vazio ganha a classe `empty` (o `tabIndex` e o `aria-label` são do DOM).
    fn update_container(&self) {
        let is_empty = self.editor_count() == 0;
        self.style.borrow_mut().container.toggle_class("empty", is_empty);

        // Update styles
        self.update_styles();
    }

    /// `updateStyles()`.
    fn update_styles(&self) {
        let is_empty = self.editor_count() == 0;
        let show_tabs = self.groups_view.upgrade().map(|groups_view| groups_view.part_options().show_tabs);
        let multiple_tabs = matches!(show_tabs, Some(ShowTabs::Multiple));

        let empty_background = self.get_color(EDITOR_GROUP_EMPTY_BACKGROUND);
        let border_color = self.get_color(EDITOR_GROUP_HEADER_BORDER).or_else(|| self.get_color(CONTRAST_BORDER));
        let title_background = self.get_color(if multiple_tabs { EDITOR_GROUP_HEADER_TABS_BACKGROUND } else { EDITOR_GROUP_HEADER_NO_TABS_BACKGROUND });
        let editor_background = self.get_color(EDITOR_BACKGROUND);

        let mut style = self.style.borrow_mut();

        // Container
        style.container.background_color = if is_empty { empty_background.unwrap_or_default() } else { String::new() };

        // Title control
        match border_color {
            Some(border_color) if !is_empty => {
                style.title.toggle_class("title-border-bottom", true);
                style.title.properties.insert("--title-border-bottom-color".to_string(), border_color);
            }
            _ => {
                style.title.toggle_class("title-border-bottom", false);
                style.title.properties.remove("--title-border-bottom-color");
            }
        }

        let title_background = title_background.unwrap_or_else(|| "transparent".to_string());
        style.title.background_color = format!("var(--modern-ui-editor-title-background, {title_background})");
        style.title_background_fallback = title_background;

        // Editor container
        style.editor.background_color = editor_background.unwrap_or_default();
    }

    // endregion

    // region: pin, stick

    /// `pinEditor(candidate = this.activeEditor)`.
    fn pin_editor_candidate(&self, candidate: Option<&EditorInputRef>) {
        let candidate = candidate.cloned().or_else(|| self.model.active_editor());

        if let Some(candidate) = candidate
            && !self.model.is_pinned(&EditorOrIndex::Editor(candidate.clone()))
        {
            // Update model
            self.model.pin(&candidate);
        }
    }

    /// `doStickEditor(candidate, sticky)`, com `candidate = this.activeEditor` quando falta.
    fn do_stick_editor(&self, candidate: Option<&EditorInputRef>, sticky: bool) {
        let candidate = candidate.cloned().or_else(|| self.model.active_editor());

        if let Some(candidate) = candidate
            && self.model.is_sticky(&EditorOrIndex::Editor(candidate.clone())) != sticky
        {
            // Update model
            if sticky {
                self.model.stick(&candidate);
            } else {
                self.model.unstick(&candidate);
            }
        }
    }

    // endregion

    // region: openEditor()

    /// `openEditor(editor, options, internalOptions)`.
    fn open_editor_with_internal_options(&self, editor: &EditorInputRef, options: Option<EditorOptions>, internal_options: &InternalEditorOpenOptions) -> Promise<Option<Rc<dyn IEditorPane>>> {
        self.do_open_editor(
            editor,
            options,
            &InternalEditorOpenOptions {
                // Allow to match on a side-by-side editor when same editor is opened on both sides. In that case we do not
                // want to open a new editor but reuse that one.
                support_side_by_side: Some(SideBySideEditor::Both),
                ..internal_options.clone()
            },
        )
    }

    fn do_open_editor(&self, editor: &EditorInputRef, options: Option<EditorOptions>, internal_options: &InternalEditorOpenOptions) -> Promise<Option<Rc<dyn IEditorPane>>> {
        // Guard against invalid editors. Disposed editors should never open because they emit no events e.g. to indicate
        // dirty changes.
        if editor.is_disposed() {
            return Promise::resolved(None);
        }

        // Fire the event letting everyone know we are about to open an editor
        self.on_will_open_editor.fire(EditorWillOpenEvent { group_id: self.id(), editor: editor.clone() });

        // Determine options
        let groups_view = self.groups_view();
        let index = options.as_ref().and_then(|options| options.index).map(|index| index as i64);
        let sticky_by_index = index.is_some_and(|index| self.model.is_sticky(&EditorOrIndex::Index(index)));
        let option_sticky = options.as_ref().is_some_and(|options| options.sticky == Some(true));
        let option_transient = options.as_ref().is_some_and(|options| options.transient == Some(true));
        let pinned = option_sticky
            || (!groups_view.part_options().enable_preview && !option_transient)
            || editor.is_dirty()
            || options.as_ref().and_then(|options| options.pinned).unwrap_or(index.is_some() /* unless specified, prefer to pin when opening with index */)
            || sticky_by_index
            || editor.has_capability(EditorInputCapabilities::SCRATCHPAD);
        let mut open_editor_options = EditorOpenOptions {
            index,
            pinned: Some(pinned),
            sticky: Some(option_sticky || sticky_by_index),
            transient: Some(option_transient),
            inactive_selection: internal_options.inactive_selection.clone(),
            active: Some(self.editor_count() == 0 || !options.as_ref().is_some_and(|options| options.inactive == Some(true))),
            support_side_by_side: internal_options.support_side_by_side,
        };

        if open_editor_options.active != Some(true)
            && open_editor_options.pinned != Some(true)
            && let Some(active_editor) = self.model.active_editor()
            && !self.model.is_pinned(&EditorOrIndex::Editor(active_editor))
        {
            // Special case: we are to open an editor inactive and not pinned, but the current active editor is also not
            // pinned, which means it will get replaced with this one. As such, the editor can only be active.
            open_editor_options.active = Some(true);
        }

        let mut activate_group = false;
        let mut restore_group = false;

        match options.as_ref().and_then(|options| options.activation) {
            // Respect option to force activate an editor group.
            Some(EditorActivation::Activate) => activate_group = true,
            // Respect option to force restore an editor group.
            Some(EditorActivation::Restore) => restore_group = true,
            // Respect option to preserve active editor group.
            Some(EditorActivation::Preserve) => {}
            // Finally, we only activate/restore an editor which is opening as active editor. If preserveFocus is enabled,
            // we only restore but never activate the group.
            None if open_editor_options.active == Some(true) => {
                activate_group = !options.as_ref().is_some_and(|options| options.preserve_focus == Some(true));
                restore_group = !activate_group;
            }
            None => {}
        }

        // Actually move the editor if a specific index is provided and we figure out that the editor is already opened at a
        // different index. This ensures the right set of events are fired to the outside.
        if let Some(index) = open_editor_options.index {
            let index_of_editor = self.model.index_of(Some(&typed(editor)), None, None);
            if index_of_editor != -1 && index_of_editor != index {
                self.do_move_editor_inside_group(editor, Some(index), open_editor_options.sticky);
            }
        }

        // Update model and make sure to continue to use the editor we get from the model. It is possible that the editor
        // was already opened and we want to ensure that we use the existing instance in that case.
        let EditorOpenResult { editor: opened_editor, is_new } = self.model.open_editor(editor, Some(&open_editor_options));

        // Conditionally lock the group
        if is_new // only if this editor was new for the group
            && self.editor_count() == 1 // only when this editor was the first editor in the group
            && groups_view.groups().len() > 1
        // only allow auto locking if more than 1 group is opened
        {
            // only when the editor identifier is configured as such
            if let Some(editor_id) = opened_editor.editor_id()
                && groups_view.part_options().auto_lock_groups.contains(&editor_id)
            {
                self.model.lock(true);
            }
        }

        // Show editor
        let show_editor_result = self.do_show_editor(&opened_editor, open_editor_options.active == Some(true), is_new, options, internal_options.text_options.clone());

        // Finally make sure the group is active or restored as instructed
        if activate_group {
            groups_view.activate_group(&self.rc());
        } else if restore_group {
            groups_view.restore_group(&self.rc());
        }

        show_editor_result
    }

    fn do_show_editor(&self, editor: &EditorInputRef, active: bool, is_new: bool, options: Option<EditorOptions>, text_options: Option<TextEditorOptions>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        // Show in editor control if the active editor changed. Inactive: return `undefined` as result to signal this
        if !active {
            return Promise::resolved(None);
        }

        let (this, editor) = (self.rc(), editor.clone());
        async_fn(async move {
            let is_explicit = options.as_ref().and_then(|options| options.is_explicit);
            let preserve_focus = options.as_ref().and_then(|options| options.preserve_focus);

            // The editor pane receives the options as `ITextEditorOptions`: the text selection (if any) on top of the base options
            let pane_options = match text_options {
                Some(text_options) => Some(TextEditorOptions { base: options.unwrap_or_default(), ..text_options }),
                None => options.map(TextEditorOptions::from),
            };

            let result = this.editor_panes.open_editor_with_text_options(&editor, pane_options, EditorOpenContext { new_in_group: Some(is_new) }).await?;

            // Return early if the operation was cancelled by another operation
            if result.cancelled {
                return Ok(None);
            }

            // Editor change event
            if result.changed {
                this.on_did_active_editor_change.fire(ActiveEditorChangeEvent { editor: Some(editor.clone()), is_explicit });
            }

            // Indicate error as an event but do not bubble them up
            if result.error.is_some() {
                this.on_did_open_editor_fail.fire(editor.clone());
            }

            // Without an editor pane, recover by closing the active editor (if the input is still the active one)
            if result.pane.is_none() && this.model.active_editor().is_some_and(|active_editor| same_editor(&active_editor, &editor)) {
                this.do_close_editor(&editor, preserve_focus, InternalEditorCloseOptions { from_error: true, ..InternalEditorCloseOptions::default() });
            }

            Ok(result.pane.map(|pane| pane as Rc<dyn IEditorPane>))
        })
    }

    // endregion

    // region: moveEditor() e copyEditor()

    /// O alvo do mover e do copiar como vista (o original o tipa `EditorGroupView`).
    fn target_view(&self, target: &Rc<dyn IEditorGroup>) -> Rc<EditorGroupView> {
        self.groups_view().get_group(target.id()).unwrap_or_else(|| throw(Error::new("Invalid editor group provided!")))
    }

    /// `moveEditor(editor, target, options)`.
    fn do_move_editor(&self, editor: &EditorInputRef, target: &Rc<dyn IEditorGroup>, options: Option<EditorOptions>) -> bool {
        let target_view = self.target_view(target);

        // Move within same group
        if same_view(self, &target_view) {
            self.do_move_editor_inside_group(editor, options.as_ref().and_then(|options| options.index).map(|index| index as i64), options.as_ref().and_then(|options| options.sticky));
            return true;
        }

        // Move across groups
        self.do_move_or_copy_editor_across_groups(editor, &target_view, options, false)
    }

    /// `copyEditor(editor, target, options)`.
    fn do_copy_editor(&self, editor: &EditorInputRef, target: &Rc<dyn IEditorGroup>, options: Option<EditorOptions>) {
        let target_view = self.target_view(target);

        // Move within same group because we do not support to show the same editor multiple times in the same group
        if same_view(self, &target_view) {
            self.do_move_editor_inside_group(editor, options.as_ref().and_then(|options| options.index).map(|index| index as i64), options.as_ref().and_then(|options| options.sticky));
        }
        // Copy across groups
        else {
            self.do_move_or_copy_editor_across_groups(editor, &target_view, options, true);
        }
    }

    fn do_move_editor_inside_group(&self, candidate: &EditorInputRef, move_to_index: Option<i64>, sticky: Option<bool>) {
        let Some(move_to_index) = move_to_index else {
            return; // do nothing if we move into same group without index
        };

        // Update model and make sure to continue to use the editor we get from the model. It is possible that the editor
        // was already opened and we want to ensure that we use the existing instance in that case.
        let current_index = self.model.index_of(Some(&typed(candidate)), None, None);
        let Some(editor) = self.model.get_editor_by_index(current_index) else {
            return;
        };

        // Move when index has actually changed
        if current_index != move_to_index {
            // Update model
            self.model.move_editor(&editor, move_to_index);
            self.model.pin(&editor);
        }

        // Support the option to stick the editor even if it is moved. It is important that we call this method after we have
        // moved the editor because the result of moving the editor could have caused a change in sticky state.
        if sticky == Some(true) {
            self.do_stick_editor(Some(&editor), true);
        }
    }

    fn do_move_or_copy_editor_across_groups(&self, editor: &EditorInputRef, target: &Rc<EditorGroupView>, open_options: Option<EditorOptions>, keep_copy: bool) -> bool {
        // Validate that we can move
        if !keep_copy || editor.has_capability(EditorInputCapabilities::SINGLETON) /* singleton editors will always move */ {
            if let CanMove::No(can_move_veto) = editor.can_move(self.id(), target.id()) {
                let dialog_service = self.instantiation_service.invoke_function(|accessor| accessor.get(I_DIALOG_SERVICE));
                let _ = dialog_service.error(&can_move_veto, Some(&localize("moveErrorDetails", "Try saving or reverting the editor first and then try again.", &[])));

                return false;
            }
        }

        // When moving/copying an editor, try to preserve as much view state as possible by checking for the editor to be a
        // text editor and creating the options accordingly if so
        let sticky = open_options
            .as_ref()
            .and_then(|options| options.sticky)
            .unwrap_or(!keep_copy && self.model.is_sticky(&EditorOrIndex::Editor(editor.clone()))); // preserve sticky state only if editor is moved or explicitly wanted (https://github.com/microsoft/vscode/issues/99035)
        let preset_options = EditorOptions {
            pinned: Some(true), // always pin moved editor
            sticky: Some(sticky),
            ..open_options.unwrap_or_default()
        };
        let options = fill_active_editor_view_state(self, Some(editor), Some(preset_options));

        // Indicate will move event
        if !keep_copy {
            self.on_will_move_editor.fire(EditorWillMoveEvent { group_id: self.id(), editor: editor.clone(), target: target.id() });
        }

        // A move to another group is an open first...
        target.do_open_editor(&if keep_copy { editor.copy() } else { editor.clone() }, Some(options), &InternalEditorOpenOptions::default());

        // ...and a close afterwards (unless we copy)
        if !keep_copy {
            self.do_close_editor(editor, Some(true) /* do not focus next one behind if any */, InternalEditorCloseOptions { context: Some(EditorCloseContext::Move), ..InternalEditorCloseOptions::default() });
        }

        true
    }

    // endregion

    // region: closeEditor()

    fn do_close_editor_with_confirmation_handling(&self, editor: Option<EditorInputRef>, options: Option<CloseEditorOptions>, internal_options: InternalEditorCloseOptions) -> Promise<bool> {
        let this = self.rc();

        async_fn(async move {
            let Some(editor) = editor else {
                return Ok(false);
            };

            if !options.is_some_and(|options| options.force == Some(true)) && !internal_options.force && editor.has_capability(EditorInputCapabilities::CANNOT_CLOSE) {
                return Ok(false);
            }

            // Check for confirmation and veto
            let veto = this.handle_close_confirmation(vec![editor.clone()]).await?;
            if veto {
                return Ok(false);
            }

            // Do close
            this.do_close_editor(&editor, options.and_then(|options| options.preserve_focus), internal_options);

            Ok(true)
        })
    }

    fn do_close_editor(&self, editor: &EditorInputRef, preserve_focus: Option<bool>, internal_options: InternalEditorCloseOptions) {
        // Closing the active editor of the group is a bit more work
        if self.model.is_active(&typed(editor)) {
            self.do_close_active_editor(preserve_focus, internal_options);
        }
        // Closing inactive editor is just a model update
        else {
            self.do_close_inactive_editor(editor, internal_options);
        }
    }

    fn do_close_active_editor(&self, preserve_focus: Option<bool>, internal_options: InternalEditorCloseOptions) {
        let groups_view = self.groups_view();
        let preserve_focus = preserve_focus.unwrap_or_else(|| !same_view(&groups_view.active_group(), self));
        let editor_to_close = self.model.active_editor();

        // Optimization: if we are about to close the last editor in this group and settings are configured to close the group
        // since it will be empty, we first set the last active group as empty before closing the editor. This reduces the
        // amount of editor change events that this operation emits and will reduce flicker. Without this optimization, this
        // group (if active) would first trigger a active editor change event because it became empty, only to then trigger
        // another one when the next group gets active.
        let close_empty_group = self.part_options().close_empty_groups;
        if close_empty_group && self.active.get() == Some(true) && self.editor_count() == 1 {
            let most_recently_active_groups = groups_view.get_groups(GroupsOrder::MostRecentlyActive);
            if let Some(next_active_group) = most_recently_active_groups.get(1) {
                // [0] will be the current one, so take [1]
                groups_view.activate_group(next_active_group);
            }
        }

        // Update model
        if let Some(editor_to_close) = &editor_to_close {
            self.model.close_editor(editor_to_close, internal_options.context, None);
        }

        // Open next active if there are more to show
        if let Some(next_active_editor) = self.model.active_editor() {
            let mut activation = None;
            if preserve_focus && !same_view(&groups_view.active_group(), self) {
                // If we are opening the next editor in an inactive group without focussing it, ensure we preserve the editor
                // group sizes in case that group is minimized. https://github.com/microsoft/vscode/issues/117686
                activation = Some(EditorActivation::Preserve);
            }

            let options = EditorOptions {
                preserve_focus: Some(preserve_focus),
                activation,
                // When closing an editor due to an error we can end up in a loop where we continue closing editors that fail
                // to open (e.g. when the file no longer exists). We do not want to show repeated errors in this case to the
                // user. As such, if we open the next editor and we are in a scope of a previous editor failing, we silence
                // the input errors until the editor is opened by setting ignoreError: true.
                ignore_error: internal_options.from_error.then_some(true),
                ..EditorOptions::default()
            };

            self.do_open_editor(&next_active_editor, Some(options), &InternalEditorOpenOptions::default());
        }
        // Otherwise we are empty, so clear from editor control and send event
        else {
            // Forward to editor pane
            if let Some(editor_to_close) = &editor_to_close {
                self.editor_panes.close_editor(editor_to_close);
            }

            // Events
            self.on_did_active_editor_change.fire(ActiveEditorChangeEvent { editor: None, is_explicit: None });

            // Remove empty group if we should
            if close_empty_group {
                groups_view.remove_group(&self.rc());
            }
        }
    }

    fn do_close_inactive_editor(&self, editor: &EditorInputRef, internal_options: InternalEditorCloseOptions) {
        // Update model
        self.model.close_editor(editor, internal_options.context, None);
    }

    fn handle_close_confirmation(&self, editors: Vec<EditorInputRef>) -> Promise<bool /* veto */> {
        let this = self.rc();

        async_fn(async move {
            for editor in editors {
                // To prevent multiple confirmation dialogs from showing up one after the other we check if a pending
                // confirmation is currently showing and if so, join that
                let key = id_of(&editor);
                let pending = this.map_editor_to_pending_confirmation.borrow().get(&key).cloned();
                let handle_close_confirmation_promise = match pending {
                    Some(pending) => pending,
                    None => {
                        let promise = this.do_handle_close_confirmation(&editor, false);
                        this.map_editor_to_pending_confirmation.borrow_mut().insert(key, promise.clone());

                        promise
                    }
                };

                let veto = handle_close_confirmation_promise.await;
                this.map_editor_to_pending_confirmation.borrow_mut().remove(&key);

                // Return for the first veto we got
                if veto? {
                    return Ok(true);
                }
            }

            // Otherwise we are done with the remainders
            Ok(false)
        })
    }

    fn do_handle_close_confirmation(&self, editor: &EditorInputRef, skip_auto_save: bool) -> Promise<bool /* veto */> {
        let (this, editor) = (self.rc(), editor.clone());

        async_fn(async move {
            if !this.should_confirm_close(&editor) {
                return Ok(false); // no veto
            }

            if let Some((primary, _)) = editor.side_by_side_editors()
                && this.model.contains(&typed(&primary), None)
            {
                return Ok(false); // primary-side of editor is still opened somewhere else
            }

            // Note: we explicitly decide to ask for confirm if closing a normal editor even if it is opened in a side-by-side
            // editor in the group. This decision is made because it may be less obvious that one side of a side by side
            // editor is dirty and can still be changed. The only exception is when the same editor is opened on both sides
            // of a side by side editor (https://github.com/microsoft/vscode/issues/138442)

            let opened_elsewhere = this.groups_view().groups().iter().any(|group_view| {
                if same_view(group_view, &this) {
                    return false; // skip (we already handled our group above)
                }

                let other_group = group_view;
                if other_group.model.contains(&typed(&editor), Some(MatchEditorOptions { support_side_by_side: Some(SideBySideEditor::Both), strict_equals: None })) {
                    return true; // exact editor still opened (either single, or split-in-group)
                }

                if let Some((primary, _)) = editor.side_by_side_editors()
                    && other_group.model.contains(&typed(&primary), None)
                {
                    return true; // primary side of side by side editor still opened
                }

                false
            });
            if opened_elsewhere {
                return Ok(false); // editor is still editable somewhere else
            }

            // In some cases trigger save before opening the dialog depending on auto-save configuration. However, make sure
            // to respect `skipAutoSave` option in case the automated save fails which would result in the editor never
            // closing. Also, we only do this if no custom confirmation handling is implemented.
            let mut confirmation = ConfirmResult::Cancel;
            let mut save_reason = SaveReason::Explicit;
            let mut auto_save = false;
            if !editor.has_capability(EditorInputCapabilities::UNTITLED) && !skip_auto_save && editor.close_handler().is_none() {
                let files_configuration_service = this.instantiation_service.invoke_function(|accessor| accessor.get(I_FILES_CONFIGURATION_SERVICE));
                let auto_save_mode = files_configuration_service.get_auto_save_mode(Some(&ResourceOrEditor::from_editor(&editor)), None).mode();

                // Auto-save on focus change: save, because a dialog would steal focus
                // (see https://github.com/microsoft/vscode/issues/108752)
                if auto_save_mode == AutoSaveMode::OnFocusChange {
                    auto_save = true;
                    confirmation = ConfirmResult::Save;
                    save_reason = SaveReason::FocusChange;
                }
                // Auto-save on window change: save, because on Windows and Linux, a native dialog triggers the window focus
                // change (see https://github.com/microsoft/vscode/issues/134250)
                else if cfg!(any(target_os = "windows", target_os = "linux")) && auto_save_mode == AutoSaveMode::OnWindowChange {
                    auto_save = true;
                    confirmation = ConfirmResult::Save;
                    save_reason = SaveReason::WindowChange;
                }
            }

            // No auto-save on focus change or custom confirmation handler: ask user
            if !auto_save {
                // Switch to editor that we want to handle for confirmation unless showing already
                if !this.model.active_editor().is_some_and(|active_editor| active_editor.matches(&typed(&editor))) {
                    this.do_open_editor(&editor, None, &InternalEditorOpenOptions::default()).await?;
                }

                // Let editor handle confirmation if implemented
                let close_handler = editor.close_handler();
                let mut handler_did_error = false;
                if let Some(close_handler) = &close_handler {
                    match close_handler.confirm(&[EditorIdentifier { editor: editor.clone(), group_id: this.id() }]).await {
                        Ok(result) => confirmation = result,
                        Err(error) => {
                            this.log_error(&error);
                            handler_did_error = true;
                        }
                    }
                }

                // Show a file specific confirmation if there is no handler or it errored
                if close_handler.is_none() || handler_did_error {
                    let name = match editor.side_by_side_editors() {
                        Some((primary, _)) => primary.get_name(), // prefer shorter names by using primary's name in this case
                        None => editor.get_name(),
                    };

                    let file_dialog_service = this.instantiation_service.invoke_function(|accessor| accessor.get(I_FILE_DIALOG_SERVICE));
                    confirmation = file_dialog_service.show_save_confirm(&[FileNameOrResource::Name(name)]).await?;
                }
            }

            // It could be that the editor's choice of confirmation has changed given the check for confirmation is long
            // running, so we check again to see if anything needs to happen before closing for good. This can happen for
            // example if `autoSave: onFocusChange` is configured so that the save happens when the dialog opens. However, we
            // only do this unless a custom confirm handler is installed that may not be fit to be asked a second time right
            // after.
            if editor.close_handler().is_none() && !this.should_confirm_close(&editor) {
                return Ok(confirmation == ConfirmResult::Cancel);
            }

            // Otherwise, handle accordingly
            match confirmation {
                ConfirmResult::Save => {
                    let result = editor.save(this.id(), Some(SaveOptions { reason: Some(save_reason), ..SaveOptions::default() })).await?;
                    if result.is_none() && auto_save {
                        // Save failed and we need to signal this back to the user, so we handle the dirty editor again but
                        // this time ensuring to show the confirm dialog
                        // (see https://github.com/microsoft/vscode/issues/108752)
                        return this.do_handle_close_confirmation(&editor, true).await;
                    }

                    Ok(editor.is_dirty()) // veto if still dirty
                }
                ConfirmResult::DontSave => {
                    // first try a normal revert where the contents of the editor are restored
                    match editor.revert(this.id(), None).await {
                        Ok(()) => Ok(editor.is_dirty()), // veto if still dirty
                        Err(error) => {
                            this.log_error(&error);

                            // if that fails, since we are about to close the editor, we accept that the editor cannot be
                            // reverted and instead do a soft revert that just enables us to close the editor. With this, a
                            // user can always close a dirty editor even when reverting fails.
                            editor.revert(this.id(), Some(RevertOptions { soft: Some(true), ..RevertOptions::default() })).await?;

                            Ok(editor.is_dirty()) // veto if still dirty
                        }
                    }
                }
                ConfirmResult::Cancel => Ok(true), // veto
            }
        })
    }

    fn should_confirm_close(&self, editor: &EditorInputRef) -> bool {
        if let Some(close_handler) = editor.close_handler() {
            match errors::catch(|| close_handler.show_confirm()) {
                Ok(show_confirm) => return show_confirm, // custom handling of confirmation on close
                Err(error) => self.log_error(&error),
            }
        }

        editor.is_dirty() && !editor.is_saving() // editor must be dirty and not saving
    }

    // endregion

    // region: closeEditors()

    fn do_get_editors_to_close(&self, args: CloseEditorsTarget) -> Vec<EditorInputRef> {
        let filter: CloseEditorsFilter = match args {
            CloseEditorsTarget::Editors(editors) => return editors,
            CloseEditorsTarget::Filter(filter) => filter,
        };

        let direction = filter.direction;
        let has_direction = direction.is_some();

        // in MRU order only if direction is not specified
        let mut editors_to_close = self.model.get_editors(
            if has_direction { EditorsOrder::Sequential } else { EditorsOrder::MostRecentlyActive },
            Some(GetEditorsOptions { exclude_sticky: filter.exclude_sticky }),
        );

        // Filter: saved or saving only
        if filter.saved_only == Some(true) {
            editors_to_close.retain(|editor| !editor.is_dirty() || editor.is_saving());
        }
        // Filter: direction (left / right)
        else if has_direction && let Some(except) = &filter.except {
            let index = self.model.index_of(Some(&typed(except)), Some(&editors_to_close), None);
            editors_to_close = if direction == Some(CloseDirection::Left) {
                js_slice(&editors_to_close, 0, Some(index))
            } else {
                js_slice(&editors_to_close, index + 1, None)
            };
        }
        // Filter: except
        else if let Some(except) = &filter.except {
            editors_to_close.retain(|editor| !editor.matches(&typed(except)));
        }

        editors_to_close
    }

    fn do_close_editors(&self, editors: &[EditorInputRef], options: Option<CloseEditorOptions>) {
        // Close all inactive editors first
        let mut close_active_editor = false;
        for editor in editors {
            if !self.model.is_active(&typed(editor)) {
                self.do_close_inactive_editor(editor, InternalEditorCloseOptions::default());
            } else {
                close_active_editor = true;
            }
        }

        // Close active editor last if contained in editors list to close
        if close_active_editor {
            self.do_close_active_editor(options.and_then(|options| options.preserve_focus), InternalEditorCloseOptions::default());
        }
    }

    // endregion

    // region: closeAllEditors()

    /// O trecho de `closeAllEditors` que vale para `isEmpty`: fecha o grupo vazio se a configuração mandar.
    fn close_empty_group_if_configured(&self) {
        // If the group is empty and the request is to close all editors, we still close the editor group is the related
        // setting to close empty groups is enabled for a convenient way of removing empty editor groups for the user.
        if self.part_options().close_empty_groups {
            self.groups_view().remove_group(&self.rc());
        }
    }

    fn do_close_all_editors(&self, options: CloseAllEditorsOptions) {
        let force = options.force == Some(true);
        let mut editors: Vec<EditorInputRef> = self
            .model
            .get_editors(EditorsOrder::Sequential, Some(GetEditorsOptions { exclude_sticky: options.exclude_sticky }))
            .into_iter()
            .filter(|editor| force || !editor.has_capability(EditorInputCapabilities::CANNOT_CLOSE))
            .collect();
        if options.exclude_confirming == Some(true) {
            editors.retain(|editor| !self.should_confirm_close(editor));
        }

        // Close all inactive editors first
        let mut editors_to_close: Vec<EditorInputRef> = Vec::new();
        for editor in editors {
            if !self.model.is_active(&typed(&editor)) {
                self.do_close_inactive_editor(&editor, InternalEditorCloseOptions::default());
            }

            editors_to_close.push(editor);
        }

        // Close active editor last (unless we skip it, e.g. because it is sticky)
        if let Some(active_editor) = self.model.active_editor()
            && editors_to_close.iter().any(|editor| same_editor(editor, &active_editor))
        {
            self.do_close_active_editor(None, InternalEditorCloseOptions::default());
        }
    }

    // endregion

    /// O grupo como o `Rc<dyn IEditorGroup>` que os contratos do serviço usam.
    pub fn as_group(&self) -> Rc<dyn IEditorGroup> {
        self.rc()
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
    fn as_any_rc(self: Rc<Self>) -> Option<Rc<dyn std::any::Any>> {
        Some(self)
    }

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
        self.on_did_model_change.event()
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    fn on_did_active_editor_change(&self) -> Event<ActiveEditorChangeEvent> {
        self.on_did_active_editor_change.event()
    }

    fn on_will_close_editor(&self) -> Event<EditorCloseEvent> {
        self.on_will_close_editor.event()
    }

    fn on_did_close_editor(&self) -> Event<EditorCloseEvent> {
        self.on_did_close_editor.event()
    }

    fn on_will_move_editor(&self) -> Event<EditorWillMoveEvent> {
        self.on_will_move_editor.event()
    }

    fn on_will_open_editor(&self) -> Event<EditorWillOpenEvent> {
        self.on_will_open_editor.event()
    }

    fn on_did_open_editor_fail(&self) -> Event<EditorInputRef> {
        self.on_did_open_editor_fail.event()
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

    fn active_editor_pane(&self) -> Option<Rc<dyn IEditorPane>> {
        self.editor_panes.active_editor_pane().map(|pane| pane as Rc<dyn IEditorPane>)
    }

    fn selected_editors(&self) -> Vec<EditorInputRef> {
        self.model.selected_editors()
    }

    fn preview_editor(&self) -> Option<EditorInputRef> {
        self.model.preview_editor()
    }

    fn count(&self) -> usize {
        self.model.count()
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

    fn find_editors(&self, resource: &URI, options: Option<FindEditorOptions>) -> Vec<EditorInputRef> {
        let uri_identity_service = self.instantiation_service.invoke_function(|accessor| accessor.get(I_URI_IDENTITY_SERVICE));
        let canonical_resource = uri_identity_service.as_canonical_uri(resource);
        let support_side_by_side = options.and_then(|options| options.support_side_by_side);
        let ext_uri = ext_uri();

        let canonical_side = |editor: &EditorInputRef, side: SideBySideEditor| {
            EditorResourceAccessor::get_canonical_uri(Some(&typed(editor)), Some(&EditorResourceAccessorOptions { support_side_by_side: Some(side), filter_by_scheme: None }))
        };

        self.get_editors(options.and_then(|options| options.order).unwrap_or(EditorsOrder::Sequential), None)
            .into_iter()
            .filter(|editor| {
                if editor.resource().is_some_and(|resource| ext_uri.is_equal(Some(&resource), Some(&canonical_resource), false)) {
                    return true;
                }

                // Support side by side editor primary side if specified
                if matches!(support_side_by_side, Some(SideBySideEditor::Primary | SideBySideEditor::Any))
                    && canonical_side(editor, SideBySideEditor::Primary).is_some_and(|primary| ext_uri.is_equal(Some(&primary), Some(&canonical_resource), false))
                {
                    return true;
                }

                // Support side by side editor secondary side if specified
                if matches!(support_side_by_side, Some(SideBySideEditor::Secondary | SideBySideEditor::Any))
                    && canonical_side(editor, SideBySideEditor::Secondary).is_some_and(|secondary| ext_uri.is_equal(Some(&secondary), Some(&canonical_resource), false))
                {
                    return true;
                }

                false
            })
            .collect()
    }

    fn get_editor_by_index(&self, index: usize) -> Option<EditorInputRef> {
        self.model.get_editor_by_index(index as i64)
    }

    fn get_index_of_editor(&self, editor: &EditorInputRef) -> i64 {
        self.model.index_of(Some(&typed(editor)), None, None)
    }

    fn is_first(&self, editor: &EditorInputRef) -> bool {
        self.model.is_first(editor, None)
    }

    fn is_last(&self, editor: &EditorInputRef) -> bool {
        self.model.is_last(editor, None)
    }

    fn open_editor(&self, editor: &EditorInputRef, options: Option<EditorOptions>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        self.open_editor_with_internal_options(editor, options, &InternalEditorOpenOptions::default())
    }

    fn open_editor_with_text_options(&self, editor: &EditorInputRef, options: Option<TextEditorOptions>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        let base_options = options.as_ref().map(|options| options.base.clone());

        self.open_editor_with_internal_options(editor, base_options, &InternalEditorOpenOptions { text_options: options, ..InternalEditorOpenOptions::default() })
    }

    fn open_editors(&self, editors: Vec<EditorInputWithOptions>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        let this = self.rc();

        async_fn(async move {
            // Guard against invalid editors. Disposed editors should never open because they emit no events e.g. to indicate
            // dirty changes.
            let editors_to_open: Vec<EditorInputWithOptions> = editors.into_iter().filter(|editor| !editor.editor.is_disposed()).collect();

            // Use the first editor as active editor
            let Some(first_editor) = editors_to_open.first() else {
                return Ok(None);
            };

            let open_editors_options = InternalEditorOpenOptions {
                // Allow to match on a side-by-side editor when same editor is opened on both sides. In that case we do not
                // want to open a new editor but reuse that one.
                support_side_by_side: Some(SideBySideEditor::Both),
                inactive_selection: None,
                text_options: None,
            };

            this.do_open_editor(&first_editor.editor, first_editor.options.as_ref().map(|options| options.base.clone()), &open_editors_options).await?;

            // Open the other ones inactive
            let starting_index = this.model.index_of(Some(&typed(&first_editor.editor)), None, None) + 1;
            let inactive_opens: Vec<Promise<Option<Rc<dyn IEditorPane>>>> = editors_to_open
                .iter()
                .skip(1)
                .enumerate()
                .map(|(index, EditorInputWithOptions { editor, options })| {
                    let options = EditorOptions {
                        inactive: Some(true),
                        pinned: Some(true),
                        index: Some((starting_index + index as i64) as f64),
                        ..options.as_ref().map(|options| options.base.clone()).unwrap_or_default()
                    };

                    this.do_open_editor(editor, Some(options), &open_editors_options)
                })
                .collect();
            promises::settled(inactive_opens).await?;

            // Opening many editors at once can put any editor to be the active one depending on options. As such, we simply
            // return the active editor pane after this operation.
            Ok(this.active_editor_pane())
        })
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

    fn set_selection(&self, active_selected_editor: &EditorInputRef, inactive_selected_editors: Vec<EditorInputRef>) -> Promise<()> {
        if !self.model.is_active(&typed(active_selected_editor)) {
            // The active selected editor is not yet opened, so we go through `openEditor` to show it. We pass the inactive
            // selection as internal options
            let this = self.rc();
            let active_selected_editor = active_selected_editor.clone();

            return async_fn(async move {
                this.open_editor_with_internal_options(
                    &active_selected_editor,
                    Some(EditorOptions { activation: Some(EditorActivation::Activate), ..EditorOptions::default() }),
                    &InternalEditorOpenOptions { inactive_selection: Some(inactive_selected_editors), support_side_by_side: None, text_options: None },
                )
                .await?;

                Ok(())
            });
        }

        self.model.set_selection(active_selected_editor, &inactive_selected_editors);

        Promise::resolved(())
    }

    fn contains(&self, candidate: &EditorInputOrUntyped, options: Option<MatchEditorOptions>) -> bool {
        self.model.contains(candidate, options)
    }

    fn move_editor(&self, editor: &EditorInputRef, target: &Rc<dyn IEditorGroup>) -> bool {
        self.do_move_editor(editor, target, None)
    }

    fn move_editor_with_options(&self, editor: &EditorInputRef, target: &Rc<dyn IEditorGroup>, options: Option<EditorOptions>) -> bool {
        self.do_move_editor(editor, target, options)
    }

    fn move_editors(&self, editors: Vec<EditorInputWithOptions>, target: &Rc<dyn IEditorGroup>) -> bool {
        let mut move_failed = false;

        for EditorInputWithOptions { editor, options } in editors {
            if !self.do_move_editor(&editor, target, options.map(|options| options.base)) {
                move_failed = true;
            }
        }

        !move_failed
    }

    fn copy_editor(&self, editor: &EditorInputRef, target: &Rc<dyn IEditorGroup>, options: Option<EditorOptions>) {
        self.do_copy_editor(editor, target, options);
    }

    fn copy_editors(&self, editors: Vec<EditorInputWithOptions>, target: &Rc<dyn IEditorGroup>) {
        for EditorInputWithOptions { editor, options } in editors {
            self.do_copy_editor(&editor, target, options.map(|options| options.base));
        }
    }

    fn close_editor(&self, editor: Option<&EditorInputRef>) -> Promise<bool> {
        self.close_editor_with_options(editor, None)
    }

    fn close_editor_with_options(&self, editor: Option<&EditorInputRef>, options: Option<CloseEditorOptions>) -> Promise<bool> {
        let editor = editor.cloned().or_else(|| self.model.active_editor());

        self.do_close_editor_with_confirmation_handling(editor, options, InternalEditorCloseOptions::default())
    }

    fn close_editors(&self, editors: CloseEditorsTarget, options: Option<CloseEditorOptions>) -> Promise<bool> {
        let this = self.rc();

        async_fn(async move {
            if this.editor_count() == 0 {
                return Ok(true);
            }

            let force = options.is_some_and(|options| options.force == Some(true));
            let editors: Vec<EditorInputRef> = this
                .do_get_editors_to_close(editors)
                .into_iter()
                .filter(|editor| force || !editor.has_capability(EditorInputCapabilities::CANNOT_CLOSE))
                .collect();
            if editors.is_empty() {
                return Ok(true);
            }

            // Check for confirmation and veto
            let veto = this.handle_close_confirmation(editors.clone()).await?;
            if veto {
                return Ok(false);
            }

            // Do close
            this.do_close_editors(&editors, options);

            Ok(true)
        })
    }

    fn close_all_editors(&self, options: Option<CloseAllEditorsOptions>) -> Promise<bool> {
        if self.editor_count() == 0 {
            self.close_empty_group_if_configured();

            return Promise::resolved(true);
        }

        let options = options.unwrap_or_default();

        // We can go ahead and close "sync" when we exclude confirming editors
        if options.exclude_confirming == Some(true) {
            self.do_close_all_editors(options);

            return Promise::resolved(true);
        }

        // Otherwise go through potential confirmation "async"
        let force = options.force == Some(true);
        let editors: Vec<EditorInputRef> = self
            .model
            .get_editors(EditorsOrder::MostRecentlyActive, Some(GetEditorsOptions { exclude_sticky: options.exclude_sticky }))
            .into_iter()
            .filter(|editor| force || !editor.has_capability(EditorInputCapabilities::CANNOT_CLOSE))
            .collect();
        if editors.is_empty() {
            return Promise::resolved(true);
        }

        let this = self.rc();
        async_fn(async move {
            let veto = this.handle_close_confirmation(editors).await?;
            if veto {
                return Ok(false);
            }

            this.do_close_all_editors(options);

            Ok(true)
        })
    }

    fn close_all_editors_excluding_confirming(&self, force: Option<bool>) -> bool {
        if self.editor_count() == 0 {
            self.close_empty_group_if_configured();

            return true;
        }

        self.do_close_all_editors(CloseAllEditorsOptions { exclude_sticky: None, exclude_confirming: Some(true), force });

        true
    }

    fn replace_editors(&self, editors: Vec<EditorReplacement>) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            // Extract active vs. inactive replacements
            let mut active_replacement: Option<EditorReplacement> = None;
            let mut inactive_replacements: Vec<EditorReplacement> = Vec::new();
            for EditorReplacement { editor, replacement, force_replace_dirty, options } in editors {
                let index = this.model.index_of(Some(&typed(&editor)), None, None);
                if index >= 0 {
                    let is_active_editor = this.model.is_active(&typed(&editor));

                    // make sure we respect the index of the editor to replace
                    let mut options = options.unwrap_or_default();
                    options.index = Some(index as f64);
                    options.inactive = Some(!is_active_editor);
                    options.pinned = Some(options.pinned.unwrap_or(true)); // unless specified, prefer to pin upon replace

                    let editor_to_replace = EditorReplacement { editor, replacement, force_replace_dirty, options: Some(options) };
                    if is_active_editor {
                        active_replacement = Some(editor_to_replace);
                    } else {
                        inactive_replacements.push(editor_to_replace);
                    }
                }
            }

            let replace_context = InternalEditorCloseOptions { context: Some(EditorCloseContext::Replace), ..InternalEditorCloseOptions::default() };

            // Handle inactive first
            for EditorReplacement { editor, replacement, force_replace_dirty, options } in inactive_replacements {
                // Open inactive editor
                this.do_open_editor(&replacement, options, &InternalEditorOpenOptions::default()).await?;

                // Close replaced inactive editor unless they match
                if !editor.matches(&typed(&replacement)) {
                    let closed = if force_replace_dirty == Some(true) {
                        this.do_close_editor(&editor, Some(true), replace_context);

                        true
                    } else {
                        this.do_close_editor_with_confirmation_handling(
                            Some(editor.clone()),
                            Some(CloseEditorOptions { preserve_focus: Some(true), force: None }),
                            InternalEditorCloseOptions { force: true, ..replace_context },
                        )
                        .await?
                    };

                    if !closed {
                        return Ok(()); // canceled
                    }
                }
            }

            // Handle active last
            if let Some(active_replacement) = active_replacement {
                // Open replacement as active editor
                let open_editor_result = this.do_open_editor(&active_replacement.replacement, active_replacement.options.clone(), &InternalEditorOpenOptions::default());

                // Close replaced active editor unless they match
                if !active_replacement.editor.matches(&typed(&active_replacement.replacement)) {
                    if active_replacement.force_replace_dirty == Some(true) {
                        this.do_close_editor(&active_replacement.editor, Some(true), replace_context);
                    } else {
                        this.do_close_editor_with_confirmation_handling(
                            Some(active_replacement.editor.clone()),
                            Some(CloseEditorOptions { preserve_focus: Some(true), force: None }),
                            InternalEditorCloseOptions { force: true, ..replace_context },
                        )
                        .await?;
                    }
                }

                open_editor_result.await?;
            }

            Ok(())
        })
    }

    fn pin_editor(&self, editor: Option<&EditorInputRef>) {
        self.pin_editor_candidate(editor);
    }

    fn stick_editor(&self, editor: Option<&EditorInputRef>) {
        self.do_stick_editor(editor, true);
    }

    fn unstick_editor(&self, editor: Option<&EditorInputRef>) {
        self.do_stick_editor(editor, false);
    }

    fn lock(&self, locked: bool) {
        self.model.lock(locked);
    }

    fn focus(&self) {
        // Pass focus to editor panes or the container: sem DOM, só o evento
        self.on_did_focus.fire(());
    }

    fn create_editor_actions(&self, disposables: &DisposableStore, _menu_id: Option<MenuId>) -> ActiveEditorActions {
        // If there is no active pane in the group (it's the last group and it's empty) Trigger the change event when the
        // active editor changes
        let on_did_change_emitter: Rc<Emitter<Option<MenuChangeEvent>>> = disposables.add_rc(Emitter::new());
        disposables.add(self.on_did_active_editor_change.event().listen({
            let on_did_change_emitter = on_did_change_emitter.clone();
            move |_| on_did_change_emitter.fire(None)
        }));

        ActiveEditorActions { actions: ToolbarActions::default(), on_did_change: on_did_change_emitter.event() }
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
