//! Porte de `src/vs/workbench/services/editor/browser/editorService.ts`: o [`EditorService`], a implementação do
//! [`IEditorService`] que abre, acha, fecha, substitui, salva e reverte editores delegando aos grupos do
//! [`IEditorGroupsService`], agrega os eventos dos grupos, observa os editores abertos por uso mais recente
//! ([`EditorsObserver`]) e reage a arquivos movidos e apagados.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`], na ordem do original; o
//!   `editorGroupsContainer` (`IEditorGroupsContainer | undefined`) é o argumento estático, um `Option<EditorGroupsContainer>`
//!   (o container não é um trait aqui, ver [`editor_groups_container`](super::editor_groups_container)). A classe herda de
//!   `Disposable`, aqui um `Disposable` guardado no campo `base`. `registerSingleton(IEditorService, new
//!   SyncDescriptor(EditorService, [undefined], false))` é [`register`]. O `createInstance(EditorsObserver, ...)` é o
//!   `create_instance` do serviço de instanciação.
//! - **Sobrecargas.** Cada forma do original é um método do trait (ver o cabeçalho de
//!   [`editor_service`](super::editor_service)); `openEditor` e `findEditors`, que sobrecarregam o mesmo corpo, compartilham
//!   [`EditorService::do_open_editor`] e as funções de busca. No `findEditors` o original é uma função só com três formas de
//!   retorno; aqui cada forma é um método, que repete a mesma ordem de checagens (o atalho do `EditorsObserver`, o grupo
//!   dado, a busca por todos os grupos), e a busca por todos os grupos chama a busca do grupo, que no original chama
//!   `findEditors` outra vez (a segunda checagem do `EditorsObserver` dá o mesmo resultado e é omitida).
//! - **Opções.** O `IEditorOptions` do `openEditor` tipado é o [`TextEditorOptions`] (que o inclui). O grupo recebe as
//!   [`EditorOptions`] pelo [`IEditorGroup::open_editor`] e as [`TextEditorOptions`] (com a seleção do editor de texto, o
//!   `ITextEditorOptions`) pelo [`IEditorGroup::open_editor_with_text_options`], que é o que o `openEditor` usa: a seleção
//!   chega ao painel.
//! - **`getValue<IWorkbenchEditorConfiguration>()`.** `configuration.workbench?.editor?.closeOnFileDelete` é a leitura da
//!   seção `workbench.editor.closeOnFileDelete` (booleano ou o padrão `false`).
//! - **`handleWorkspaceTrust`.** As checagens `URI.isUri(editor.input1)` (e `input2`, `base`, `result`) do ramo de merge do
//!   original comparam um objeto de entrada com `URI`, o que nunca é verdade: o ramo só liga `mergeMode`.
//! - **`async`.** As funções `async` do original são [`async_fn`]: o corpo roda já, de forma síncrona, até o primeiro
//!   `await`. O `return promise` de `openEditor` e de `replaceEditors` custa as duas rodadas da adoção ([`async_fn_maybe`]).
//!   As funções `async` do original cuja promessa não é aguardada (`handleMovedFile`, a rotina de `handleDeletedFile`) têm a
//!   promessa descartada.
//! - **Grupos.** Onde o original compara grupos com `===`, o porte compara o `id` (único por grupo).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, async_fn_maybe, await_maybe, promises, timeout};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::extpath::index_of_path;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::map::{ResourceMap, ResourceSet};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationService, SyncDescriptor, register_singleton_descriptor, service_impl};
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FileChangeType, FileChangesEvent, FileOperation, FileOperationEvent};
use je_platform::files::i_file_service::IFileService;
use je_platform::opener::editor_types::EditorOptions;
use je_platform::resources::join_path;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use je_platform::workspace_trust::{IWorkspaceTrustRequestService, WorkspaceTrustUriResponse};

use crate::browser::parts::editor::editors_observer::EditorsObserver;
use crate::common::editor::{
    EditorCloseEvent, EditorIdentifier, EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorResourceAccessor,
    EditorResourceAccessorOptions, EditorUri, EditorWillOpenEvent, EditorsOrder, GroupIdentifier, IEditorPane, SaveReason, SideBySideEditor, TextEditorOptions,
    UntypedEditorInput, side_by_side_input_sides,
};
use crate::common::editor::editor_group_model::GetEditorsOptions;
use crate::services::editor::editor_group_finder::{FindGroupEditor, find_group};
use crate::services::editor::editor_groups_container::EditorGroupsContainer;
use crate::services::editor::editor_groups_service::{
    CloseEditorOptions, CloseEditorsTarget, EditorReplacement, GroupRef, GroupsOrder, IEditorGroup, IEditorGroupsService, PreferredGroup,
};
use crate::services::editor::editor_resolver_service::{IEditorResolverService, ResolvedEditor, ResolvedStatus};
use crate::services::editor::editor_service::{
    ActiveTextEditorControl, BaseSaveRevertAllEditorOptions, EditorControl, EditorReplacementInput, EditorsChangeEvent, FindEditorOptions, I_EDITOR_SERVICE,
    IEditorService, IncludeUntitled, OpenEditorsOptions, ResourceEditorInputIdentifier, RevertAllEditorsOptions, SaveAllEditorsOptions, SaveEditorsOptions,
    SaveEditorsResult, UntypedEditorReplacement, VisibleEditorsChangeEvent,
};
use crate::services::host::host::{IHostService, OpenWindowOptions, WindowOpenable};
use crate::services::textfile::text_editor_service::ITextEditorService;

/// `closeOnFileDelete` do `workbench.editor`.
const CLOSE_ON_FILE_DELETE_SETTING: &str = "workbench.editor.closeOnFileDelete";

/// `URI | FileChangesEvent`: de onde vem o apagamento que `handleDeletedFile` trata.
#[derive(Clone)]
enum DeletedFile {
    Resource(URI),
    Changes(Rc<FileChangesEvent>),
}

/// `EditorService`.
pub struct EditorService {
    this: Weak<EditorService>,
    base: Rc<Disposable>,

    // region: events
    on_did_active_editor_change: Rc<Emitter<()>>,
    on_did_visible_editors_change: Rc<Emitter<VisibleEditorsChangeEvent>>,
    on_did_editors_change: Rc<Emitter<EditorsChangeEvent>>,
    on_will_open_editor: Rc<Emitter<EditorWillOpenEvent>>,
    on_did_close_editor: Rc<Emitter<EditorCloseEvent>>,
    on_did_open_editor_fail: Rc<Emitter<EditorIdentifier>>,
    on_did_most_recently_active_editors_change: Rc<Emitter<()>>,
    // endregion
    editor_groups_container: EditorGroupsContainer,
    is_scoped: bool,
    editors_observer: Rc<EditorsObserver>,

    /// `lastActiveEditor`.
    last_active_editor: RefCell<Option<EditorInputRef>>,
    /// `activeOutOfWorkspaceWatchers`.
    active_out_of_workspace_watchers: RefCell<ResourceMap<DisposableRef>>,
    /// `closeOnFileDelete`.
    close_on_file_delete: Cell<bool>,

    editor_group_service: Rc<dyn IEditorGroupsService>,
    instantiation_service: Rc<InstantiationService>,
    file_service: Rc<dyn IFileService>,
    configuration_service: Rc<dyn IConfigurationService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    editor_resolver_service: Rc<dyn IEditorResolverService>,
    workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService>,
    host_service: Rc<dyn IHostService>,
    text_editor_service: Rc<dyn ITextEditorService>,
}

/// Os serviços injetados no [`EditorService`], na ordem do construtor do original.
type EditorServiceDeps = (
    Rc<dyn IEditorGroupsService>,
    Rc<InstantiationService>,
    Rc<dyn IFileService>,
    Rc<dyn IConfigurationService>,
    Rc<dyn IWorkspaceContextService>,
    Rc<dyn IUriIdentityService>,
    Rc<dyn IEditorResolverService>,
    Rc<dyn IWorkspaceTrustRequestService>,
    Rc<dyn IHostService>,
    Rc<dyn ITextEditorService>,
);

/// `this._register(new Emitter<T>())`.
fn registered_emitter<T: 'static>(base: &Disposable) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    emitter
}

impl Injectable for EditorService {
    type Args = Option<EditorGroupsContainer>;
    type Deps = EditorServiceDeps;

    fn create(
        editor_groups_container: Self::Args,
        (
            editor_group_service,
            instantiation_service,
            file_service,
            configuration_service,
            context_service,
            uri_identity_service,
            editor_resolver_service,
            workspace_trust_request_service,
            host_service,
            text_editor_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_active_editor_change = registered_emitter(&base);
        let on_did_visible_editors_change = registered_emitter(&base);
        let on_did_editors_change = registered_emitter(&base);
        let on_will_open_editor = registered_emitter(&base);
        let on_did_close_editor = registered_emitter(&base);
        let on_did_open_editor_fail = registered_emitter(&base);
        let on_did_most_recently_active_editors_change = registered_emitter(&base);

        let is_scoped = editor_groups_container.is_some();
        let editors_observer = instantiation_service.create_instance::<EditorsObserver>(editor_groups_container.clone());
        base.register(editors_observer.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_active_editor_change,
            on_did_visible_editors_change,
            on_did_editors_change,
            on_will_open_editor,
            on_did_close_editor,
            on_did_open_editor_fail,
            on_did_most_recently_active_editors_change,
            editor_groups_container: editor_groups_container.unwrap_or_else(|| EditorGroupsContainer::Service(editor_group_service.clone())),
            is_scoped,
            editors_observer,
            last_active_editor: RefCell::new(None),
            active_out_of_workspace_watchers: RefCell::new(ResourceMap::new()),
            close_on_file_delete: Cell::new(false),
            editor_group_service,
            instantiation_service,
            file_service,
            configuration_service,
            context_service,
            uri_identity_service,
            editor_resolver_service,
            workspace_trust_request_service,
            host_service,
            text_editor_service,
        });

        this.on_configuration_updated(None);
        this.register_listeners();

        this
    }
}

service_impl!(EditorService => dyn IEditorService, disposable);

/// `registerSingleton(IEditorService, new SyncDescriptor(EditorService, [undefined], false))`.
pub fn register() {
    register_singleton_descriptor(I_EDITOR_SERVICE, SyncDescriptor::<dyn IEditorService>::new::<EditorService>(None));
}

impl IDisposable for EditorService {
    fn dispose(&self) {
        self.base.dispose();

        // Dispose remaining watchers if any
        let watchers: Vec<DisposableRef> = self.active_out_of_workspace_watchers.borrow().values().cloned().collect();
        for watcher in watchers {
            watcher.dispose();
        }
        self.active_out_of_workspace_watchers.borrow_mut().clear();
    }
}

impl EditorService {
    fn rc(&self) -> Rc<EditorService> {
        self.this.upgrade().expect("EditorService vivo")
    }

    /// Um listener que guarda só um `Weak` do serviço: o tratador roda enquanto o serviço vive.
    fn weak_listener<T: 'static>(&self, event: Event<T>, handler: impl Fn(&Rc<EditorService>, &T) + 'static) -> DisposableRef {
        let weak = self.this.clone();
        event.listen(move |value| {
            if let Some(this) = weak.upgrade() {
                handler(&this, value);
            }
        })
    }

    /// [`Self::weak_listener`] registrado no `Disposable` do serviço (`this._register(event(...))`).
    fn register_listener<T: 'static>(&self, event: Event<T>, handler: impl Fn(&Rc<EditorService>, &T) + 'static) {
        self.base.register(self.weak_listener(event, handler));
    }

    fn register_listeners(&self) {
        // Editor & group changes
        if self.editor_groups_container.is_main_part_of(&self.editor_group_service) {
            let weak = self.this.clone();
            drop(self.editor_group_service.when_ready().then(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_editor_groups_ready();
                }
            }));
        } else {
            self.on_editor_groups_ready();
        }
        self.register_listener(self.editor_groups_container.on_did_change_active_group(), |this, group| this.handle_active_editor_change(group));
        self.register_listener(self.editor_groups_container.on_did_add_group(), |this, group| this.register_group_listeners(group));
        self.register_listener(self.editors_observer.on_did_most_recently_active_editors_change(), |this, _| this.on_did_most_recently_active_editors_change.fire(()));

        // Out of workspace file watchers
        self.register_listener(self.on_did_visible_editors_change.event(), |this, _| this.handle_visible_editors_change());

        // File operation events are global; scoped services would process each operation again.
        if !self.is_scoped {
            // Note: there is some duplication with the two file event handlers- Since we cannot always rely on the disk events
            // carrying all necessary data in all environments, we also use the file operation events to make sure operations
            // are handled. In any case there is no guarantee if the local event is fired first or the disk one. Thus, code
            // must handle the case that the event ordering is random as well as might not carry all information needed.
            self.register_listener(self.file_service.on_did_run_operation(), |this, event| this.on_did_run_file_operation(event));
            self.register_listener(self.file_service.on_did_files_change(), |this, event| this.on_did_files_change(event));
        }

        // Configuration
        self.register_listener(self.configuration_service.on_did_change_configuration(), |this, event| this.on_configuration_updated(Some(event)));
    }

    // region: Editor & group event handlers

    fn on_editor_groups_ready(&self) {
        // Register listeners to each opened group
        for group in self.editor_groups_container.groups() {
            self.register_group_listeners(&group);
        }

        // Fire initial set of editor events if there is an active editor
        if self.active_editor_input().is_some() {
            self.do_handle_active_editor_change_event();
            self.on_did_visible_editors_change.fire(VisibleEditorsChangeEvent { is_explicit: false });
        }
    }

    fn handle_active_editor_change(&self, group: &Rc<dyn IEditorGroup>) {
        if group.id() != self.editor_groups_container.active_group().id() {
            return; // ignore if not the active group
        }

        if self.last_active_editor.borrow().is_none() && group.active_editor().is_none() {
            return; // ignore if we still have no active editor
        }

        self.do_handle_active_editor_change_event();
    }

    fn do_handle_active_editor_change_event(&self) {
        // Remember as last active
        *self.last_active_editor.borrow_mut() = self.editor_groups_container.active_group().active_editor();

        // Fire event to outside parties
        self.on_did_active_editor_change.fire(());
    }

    fn register_group_listeners(&self, group: &Rc<dyn IEditorGroup>) {
        let group_disposables = DisposableStore::new();
        let group_id = group.id();

        group_disposables.add(
            self.weak_listener(group.on_did_model_change(), move |this, event| this.on_did_editors_change.fire(EditorsChangeEvent { group_id, event: event.clone() })),
        );

        let group_ref = group.clone();
        group_disposables.add(self.weak_listener(group.on_did_active_editor_change(), move |this, event| {
            this.handle_active_editor_change(&group_ref);
            this.on_did_visible_editors_change.fire(VisibleEditorsChangeEvent { is_explicit: event.is_explicit != Some(false) /* treat undefined as explicit */ });
        }));

        group_disposables.add(self.weak_listener(group.on_will_open_editor(), |this, event| this.on_will_open_editor.fire(event.clone())));

        group_disposables.add(self.weak_listener(group.on_did_close_editor(), |this, event| this.on_did_close_editor.fire(event.clone())));

        group_disposables.add(
            self.weak_listener(group.on_did_open_editor_fail(), move |this, editor| this.on_did_open_editor_fail.fire(EditorIdentifier { editor: editor.clone(), group_id })),
        );

        let group_disposables_to_dispose = group_disposables.clone();
        group.on_will_dispose().once().listen(move |_| group_disposables_to_dispose.dispose());
    }

    // endregion

    // region: Visible Editors Change: Install file watchers for out of workspace resources that became visible

    fn handle_visible_editors_change(&self) {
        let mut visible_out_of_workspace_resources = ResourceSet::new(None);

        for editor in self.visible_editors() {
            let editor = EditorInputOrUntyped::Typed(editor);
            let mut resources: Vec<URI> = Vec::new();
            for side in [SideBySideEditor::Primary, SideBySideEditor::Secondary] {
                let options = EditorResourceAccessorOptions { support_side_by_side: Some(side), ..EditorResourceAccessorOptions::default() };
                if let Some(resource) = EditorResourceAccessor::get_canonical_uri(Some(&editor), Some(&options))
                    && !resources.iter().any(|candidate| candidate.to_string(false) == resource.to_string(false))
                {
                    resources.push(resource);
                }
            }

            for resource in resources {
                if self.file_service.has_provider(&resource) && !self.context_service.is_inside_workspace(&resource) {
                    visible_out_of_workspace_resources.add(resource);
                }
            }
        }

        // Handle no longer visible out of workspace resources
        let watched_resources: Vec<URI> = self.active_out_of_workspace_watchers.borrow().keys().cloned().collect();
        for resource in watched_resources {
            if !visible_out_of_workspace_resources.has(&resource) {
                let watcher = self.active_out_of_workspace_watchers.borrow().get(&resource).cloned();
                if let Some(watcher) = watcher {
                    watcher.dispose();
                }
                self.active_out_of_workspace_watchers.borrow_mut().delete(&resource);
            }
        }

        // Handle newly visible out of workspace resources
        for resource in visible_out_of_workspace_resources.iter() {
            if self.active_out_of_workspace_watchers.borrow().get(resource).is_none() {
                let disposable = self.file_service.watch(resource, None);
                self.active_out_of_workspace_watchers.borrow_mut().set(resource.clone(), disposable);
            }
        }
    }

    // endregion

    // region: File Changes: Move & Deletes to move or close opend editors

    fn on_did_run_file_operation(&self, event: &FileOperationEvent) {
        // Handle moves specially when file is opened
        if event.is_operation(FileOperation::Move)
            && let Some(target) = &event.target
        {
            drop(self.rc().handle_moved_file(event.resource.clone(), target.resource.clone()));
        }

        // Handle deletes
        if event.is_operation(FileOperation::Delete) || event.is_operation(FileOperation::Move) {
            self.handle_deleted_file(DeletedFile::Resource(event.resource.clone()), false, event.target.as_ref().map(|target| target.resource.clone()));
        }
    }

    fn on_did_files_change(&self, event: &Rc<FileChangesEvent>) {
        if event.got_deleted() {
            self.handle_deleted_file(DeletedFile::Changes(event.clone()), true, None);
        }
    }

    /// `handleMovedFile(source, target)`.
    fn handle_moved_file(self: &Rc<Self>, source: URI, target: URI) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let ext_uri = this.uri_identity_service.ext_uri();

            for group in this.editor_groups_container.groups() {
                let mut replacements: Vec<EditorReplacementInput> = Vec::new();

                for editor in group.editors() {
                    let resource = EditorResourceAccessor::get_original_uri(Some(&EditorInputOrUntyped::Typed(editor.clone())), None).or_else(|| editor.resource());
                    let Some(resource) = resource else {
                        continue; // not matching our resource
                    };
                    if !ext_uri.is_equal_or_parent(&resource, &source, false, false) {
                        continue; // not matching our resource
                    }

                    // Determine new resulting target resource
                    let target_resource = if ext_uri.is_equal(Some(&source), Some(&resource), false) {
                        target.clone() // file got moved
                    } else {
                        // parent folder got moved
                        let index = index_of_path(&resource.path().to_string_lossy(), &source.path().to_string_lossy(), ext_uri.ignore_path_casing(&resource)).map_or(-1, |index| index as i64);
                        let start = (index + source.path().len() as i64 + 1).clamp(0, resource.path().len() as i64) as usize;
                        let fragment = resource.path().slice(start, resource.path().len()).to_string_lossy();
                        join_path(&target, &[fragment.as_str()])
                    };

                    // Delegate rename() to editor instance
                    let Some(move_result) = editor.rename(group.id(), &target_resource).await? else {
                        return Ok(()); // not target - ignore
                    };

                    // Construct a replacement with our extra options mixed in
                    replacements.push(match move_result.editor {
                        EditorInputOrUntyped::Typed(replacement) => EditorReplacementInput::Typed(EditorReplacement {
                            editor: editor.clone(),
                            replacement,
                            options: Some(Self::with_option_overrides(move_result.options, &group, &editor).base),
                            force_replace_dirty: None,
                        }),
                        EditorInputOrUntyped::Untyped(mut replacement) => {
                            replacement.options = Some(Self::with_option_overrides(replacement.options.take(), &group, &editor));
                            EditorReplacementInput::Untyped(UntypedEditorReplacement { editor: editor.clone(), replacement, force_replace_dirty: None })
                        }
                    });
                }

                // Apply replacements
                if !replacements.is_empty() {
                    drop(this.replace_editors(replacements, GroupRef::Group(group.clone())));
                }
            }

            Ok(())
        })
    }

    /// `{ ...options, ...optionOverrides }`: as opções do resultado do `rename` com o estado atual do editor no grupo.
    fn with_option_overrides(options: Option<TextEditorOptions>, group: &Rc<dyn IEditorGroup>, editor: &EditorInputRef) -> TextEditorOptions {
        let mut options = options.unwrap_or_default();
        options.preserve_focus = Some(true);
        options.pinned = Some(group.is_pinned(&editor.into()));
        options.sticky = Some(group.is_sticky(&editor.into()));
        options.index = Some(group.get_index_of_editor(editor) as f64);
        options.inactive = Some(!group.is_active(&EditorInputOrUntyped::from(editor)));
        options
    }

    /// `onConfigurationUpdated(e?)`.
    fn on_configuration_updated(&self, event: Option<&Rc<ConfigurationChangeEvent>>) {
        if event.is_some_and(|event| !event.affects_configuration(CLOSE_ON_FILE_DELETE_SETTING, None)) {
            return;
        }

        // `typeof configuration.workbench?.editor?.closeOnFileDelete === 'boolean'`, senão o padrão `false`
        self.close_on_file_delete.set(matches!(self.configuration_service.get_value(Some(CLOSE_ON_FILE_DELETE_SETTING), None), Dynamic::Bool(true)));
    }

    /// `handleDeletedFile(arg1, isExternal, movedTo?)`.
    fn handle_deleted_file(&self, deleted: DeletedFile, is_external: bool, moved_to: Option<URI>) {
        for editor in self.get_all_non_dirty_editors(false, true) {
            let this = self.rc();
            let (deleted, moved_to) = (deleted.clone(), moved_to.clone());
            drop(async_fn(async move {
                let Some(resource) = EditorResourceAccessor::get_original_uri(Some(&EditorInputOrUntyped::Typed(editor.clone())), None).or_else(|| editor.resource()) else {
                    return Ok(());
                };

                // Handle deletes in opened editors depending on:
                // - we close any editor when `closeOnFileDelete: true`
                // - we close any editor when the delete occurred from within VSCode
                if this.close_on_file_delete.get() || !is_external {
                    let ext_uri = this.uri_identity_service.ext_uri();

                    // Do NOT close any opened editor that matches the resource path (either equal or being parent) of the
                    // resource we move to (movedTo). Otherwise we would close a resource that has been renamed to the same
                    // path but different casing.
                    if moved_to.as_ref().is_some_and(|moved_to| ext_uri.is_equal_or_parent(&resource, moved_to, false, false)) {
                        return Ok(());
                    }

                    let matches = match &deleted {
                        DeletedFile::Changes(event) => event.contains(&resource, &[FileChangeType::Deleted]),
                        DeletedFile::Resource(deleted) => ext_uri.is_equal_or_parent(&resource, deleted, false, false),
                    };

                    if !matches {
                        return Ok(());
                    }

                    // We have received reports of users seeing delete events even though the file still exists (network
                    // shares issue: https://github.com/microsoft/vscode/issues/13665). Since we do not want to close an
                    // editor without reason, we have to check if the file is really gone and not just a faulty file event.
                    // This only applies to external file events, so we need to check for the isExternal flag.
                    let mut exists = false;
                    if is_external && this.file_service.has_provider(&resource) {
                        timeout(100).await?;
                        exists = this.file_service.exists(&resource).await?;
                    }

                    if !exists && !editor.is_disposed() {
                        editor.dispose();
                    }
                }

                Ok(())
            }));
        }
    }

    /// `getAllNonDirtyEditors({ includeUntitled, supportSideBySide })`.
    fn get_all_non_dirty_editors(&self, include_untitled: bool, support_side_by_side: bool) -> Vec<EditorInputRef> {
        let mut editors: Vec<EditorInputRef> = Vec::new();

        let mut conditionally_add_editor = |editor: EditorInputRef| {
            if editor.has_capability(EditorInputCapabilities::UNTITLED) && !include_untitled {
                return;
            }

            if editor.is_dirty() {
                return;
            }

            editors.push(editor);
        };

        for editor in self.all_editors() {
            match side_by_side_input_sides(&editor).filter(|_| support_side_by_side) {
                Some((primary, secondary)) => {
                    conditionally_add_editor(primary);
                    conditionally_add_editor(secondary);
                }
                None => conditionally_add_editor(editor),
            }
        }

        editors
    }

    // endregion

    // region: Editor accessors

    /// `activeEditor` como a entrada tipada.
    fn active_editor_input(&self) -> Option<EditorInputRef> {
        self.editor_groups_container.active_group().active_editor()
    }

    /// `editors`.
    fn all_editors(&self) -> Vec<EditorInputRef> {
        self.get_editors_with_options(EditorsOrder::Sequential, None).into_iter().map(|EditorIdentifier { editor, .. }| editor).collect()
    }

    /// `doGetVisibleTextEditorControls(editorPanes)`.
    fn do_get_visible_text_editor_controls(editor_panes: &[Rc<dyn IEditorPane>]) -> Vec<ActiveTextEditorControl> {
        let mut visible_text_editor_controls = Vec::new();
        for editor_pane in editor_panes {
            let controls: Vec<Option<EditorControl>> = match editor_pane.side_by_side_panes() {
                Some((primary, secondary)) => vec![primary.and_then(|pane| pane.get_control()), secondary.and_then(|pane| pane.get_control())],
                None => vec![editor_pane.get_control()],
            };

            for control in controls {
                if let Some(EditorControl::Text(control)) = control {
                    visible_text_editor_controls.push(control);
                }
            }
        }

        visible_text_editor_controls
    }

    // endregion

    // region: openEditor()

    /// `openEditor(editor, optionsOrPreferredGroup, preferredGroup)`: o corpo compartilhado das sobrecargas. `options` só vale
    /// para a entrada tipada (a sem tipo traz as dela em `options`).
    fn do_open_editor(self: &Rc<Self>, editor: EditorInputOrUntyped, options: Option<TextEditorOptions>, preferred_group: Option<PreferredGroup>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        let this = self.clone();
        async_fn_maybe(async move {
            let mut typed_editor: Option<EditorInputRef> = None;
            let mut options = match &editor {
                EditorInputOrUntyped::Typed(_) => options,
                EditorInputOrUntyped::Untyped(untyped) => untyped.options.clone(),
            };
            let mut group: Option<Rc<dyn IEditorGroup>> = None;

            // Resolve override unless disabled
            if let EditorInputOrUntyped::Untyped(untyped) = &editor {
                match this.editor_resolver_service.resolve_editor(untyped, preferred_group.as_ref()).await? {
                    // skip editor if override is aborted
                    ResolvedEditor::Status(ResolvedStatus::Abort) => return Ok(MaybePromise::Value(None)),

                    // We resolved an editor to use
                    ResolvedEditor::Editor(resolved_editor) => {
                        typed_editor = Some(resolved_editor.editor);
                        options = resolved_editor.options;
                        group = Some(resolved_editor.group);
                    }

                    ResolvedEditor::Status(ResolvedStatus::None) => {}
                }
            }

            // Override is disabled or did not apply: fallback to default
            let typed_editor = match (typed_editor, editor) {
                (Some(typed_editor), _) | (None, EditorInputOrUntyped::Typed(typed_editor)) => typed_editor,
                (None, EditorInputOrUntyped::Untyped(untyped)) => this.text_editor_service.resolve_text_editor(untyped).await?,
            };

            // If group still isn't defined because of a disabled override we resolve it
            let group = match group {
                Some(group) => group,
                None => {
                    let find_group_editor = FindGroupEditor::WithOptions(EditorInputWithOptions { editor: typed_editor.clone(), options: options.clone() });
                    let find_group_result = this.instantiation_service.invoke_function(|accessor| find_group(accessor, &find_group_editor, preferred_group.as_ref()));
                    let (group, activation) = await_maybe(find_group_result).await?;

                    // Mixin editor group activation if returned
                    if let Some(activation) = activation {
                        options.get_or_insert_with(TextEditorOptions::default).activation = Some(activation);
                    }

                    group
                }
            };

            // Modal group: override `preserveFocus` to move focus into the modal because there is nothing to preserve if this is
            // the first modal editor
            if let Some(options) = options.as_mut()
                && options.preserve_focus == Some(true)
                && this.is_first_modal_editor_group(&group)
            {
                options.preserve_focus = Some(false);
            }

            Ok(MaybePromise::Promise(group.open_editor_with_text_options(&typed_editor, options)))
        })
    }

    /// `this.editorGroupService.activeModalEditorPart?.groups.some(modalGroup => modalGroup.id === group.id) &&
    /// activeModalEditorPart.count === 1 && activeModalEditorPart.groups[0].isEmpty`: o grupo é o primeiro editor modal (ainda
    /// vazio), onde não há foco a preservar.
    fn is_first_modal_editor_group(&self, group: &Rc<dyn IEditorGroup>) -> bool {
        let Some(modal_editor_part) = self.editor_group_service.active_modal_editor_part() else {
            return false;
        };

        let groups = modal_editor_part.groups();
        groups.iter().any(|modal_group| modal_group.id() == group.id()) && modal_editor_part.count() == 1 && groups[0].is_empty()
    }

    // endregion

    // region: openEditors()

    /// `openEditors(editors, preferredGroup, options)`.
    fn do_open_editors(self: &Rc<Self>, editors: Vec<FindGroupEditor>, preferred_group: Option<PreferredGroup>, options: Option<OpenEditorsOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        let this = self.clone();
        async_fn(async move {
            // Pass all editors to trust service to determine if we should proceed with opening the editors if we are asked to
            // validate trust.
            if options.is_some_and(|options| options.validate_trust == Some(true)) {
                let editors_trusted = this.handle_workspace_trust(&editors).await?;
                if !editors_trusted {
                    return Ok(Vec::new());
                }
            }

            // Find target groups for editors to open
            let mut map_group_to_typed_editors: Vec<(Rc<dyn IEditorGroup>, Vec<EditorInputWithOptions>)> = Vec::new();
            for editor in editors {
                let mut typed_editor: Option<EditorInputWithOptions> = None;
                let mut group: Option<Rc<dyn IEditorGroup>> = None;

                // Resolve override unless disabled
                if let FindGroupEditor::Untyped(untyped) = &editor {
                    match this.editor_resolver_service.resolve_editor(untyped, preferred_group.as_ref()).await? {
                        // skip editor if override is aborted
                        ResolvedEditor::Status(ResolvedStatus::Abort) => continue,

                        // We resolved an editor to use
                        ResolvedEditor::Editor(resolved_editor) => {
                            typed_editor = Some(EditorInputWithOptions { editor: resolved_editor.editor, options: resolved_editor.options });
                            group = Some(resolved_editor.group);
                        }

                        ResolvedEditor::Status(ResolvedStatus::None) => {}
                    }
                }

                // Override is disabled or did not apply: fallback to default
                let mut typed_editor = match (typed_editor, editor) {
                    (Some(typed_editor), _) | (None, FindGroupEditor::WithOptions(typed_editor)) => typed_editor,
                    (None, FindGroupEditor::Untyped(untyped)) => {
                        let options = untyped.options.clone();
                        EditorInputWithOptions { editor: this.text_editor_service.resolve_text_editor(untyped).await?, options }
                    }
                };

                // If group still isn't defined because of a disabled override we resolve it
                let group = match group {
                    Some(group) => group,
                    None => {
                        let find_group_editor = FindGroupEditor::WithOptions(typed_editor.clone());
                        let find_group_result = this.instantiation_service.invoke_function(|accessor| find_group(accessor, &find_group_editor, preferred_group.as_ref()));
                        await_maybe(find_group_result).await?.0
                    }
                };

                // Modal group: override `preserveFocus` to move focus into the modal there is nothing to preserve if this is the
                // first modal editor
                if let Some(options) = typed_editor.options.as_mut()
                    && options.preserve_focus == Some(true)
                    && this.is_first_modal_editor_group(&group)
                {
                    options.preserve_focus = Some(false);
                }

                // Update map of groups to editors
                match map_group_to_typed_editors.iter_mut().find(|(target_group, _)| target_group.id() == group.id()) {
                    Some((_, target_group_editors)) => target_group_editors.push(typed_editor),
                    None => map_group_to_typed_editors.push((group, vec![typed_editor])),
                }
            }

            // Open in target groups
            let result: Vec<Promise<Option<Rc<dyn IEditorPane>>>> = map_group_to_typed_editors.into_iter().map(|(group, editors)| group.open_editors(editors)).collect();

            Ok(promises::settled(result).await?.into_iter().flatten().collect())
        })
    }

    /// `handleWorkspaceTrust(editors)`.
    fn handle_workspace_trust(self: &Rc<Self>, editors: &[FindGroupEditor]) -> Promise<bool> {
        let this = self.clone();
        let (resources, diff_mode, merge_mode) = Self::extract_editor_resources(editors);
        async_fn(async move {
            match this.workspace_trust_request_service.request_open_files_trust(&resources).await? {
                WorkspaceTrustUriResponse::Open => Ok(true),
                WorkspaceTrustUriResponse::OpenInNewWindow => {
                    let to_open = resources.into_iter().map(|file_uri| WindowOpenable::File { file_uri, label: None }).collect();
                    let options = OpenWindowOptions { force_new_window: Some(true), diff_mode: Some(diff_mode), merge_mode: Some(merge_mode), ..OpenWindowOptions::default() };
                    this.host_service.open_window(to_open, Some(options)).await?;
                    Ok(false)
                }
                WorkspaceTrustUriResponse::Cancel => Ok(false),
            }
        })
    }

    /// `extractEditorResources(editors)`: os recursos, `diffMode` e `mergeMode`.
    fn extract_editor_resources(editors: &[FindGroupEditor]) -> (Vec<URI>, bool, bool) {
        let mut resources = ResourceSet::new(None);
        let mut diff_mode = false;
        let mut merge_mode = false;

        for editor in editors {
            match editor {
                // Typed Editor
                FindGroupEditor::WithOptions(editor) => {
                    let options = EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Both), ..EditorResourceAccessorOptions::default() };
                    match EditorResourceAccessor::get_original_uri_sides(Some(&EditorInputOrUntyped::Typed(editor.editor.clone())), Some(&options)) {
                        Some(EditorUri::Resource(resource)) => {
                            resources.add(resource);
                        }
                        Some(EditorUri::Sides { primary, secondary }) => {
                            if let Some(primary) = primary {
                                resources.add(primary);
                            }

                            if let Some(secondary) = secondary {
                                resources.add(secondary);
                            }

                            diff_mode = editor.editor.diff_editors().is_some();
                        }
                        None => {}
                    }
                }

                // Untyped editor
                FindGroupEditor::Untyped(editor) => {
                    if editor.is_resource_merge_editor_input() {
                        merge_mode = true;
                    }

                    if editor.is_resource_diff_editor_input() {
                        for side in [&editor.original, &editor.modified] {
                            if let Some(resource) = side.as_ref().and_then(|side| side.resource.clone()) {
                                resources.add(resource);
                            }
                        }

                        diff_mode = true;
                    } else if let Some(resource) = &editor.resource {
                        resources.add(resource.clone());
                    }
                }
            }
        }

        (resources.iter().cloned().collect(), diff_mode, merge_mode)
    }

    // endregion

    // region: findEditors()

    /// O atalho de `findEditors` pelo `EditorsObserver`: só dá para saber pelo recurso quando não se pede o lado secundário de
    /// um editor lado a lado, porque o observador só indexa os editores primários.
    fn may_have_editors(&self, resource: &URI, options: Option<FindEditorOptions>) -> bool {
        let support_side_by_side = options.and_then(|options| options.support_side_by_side);
        if support_side_by_side != Some(SideBySideEditor::Any) && support_side_by_side != Some(SideBySideEditor::Secondary) {
            return self.editors_observer.has_editors(resource);
        }

        true
    }

    /// O grupo de um `IEditorGroup | GroupIdentifier`.
    fn resolve_group(&self, group: GroupRef) -> Option<Rc<dyn IEditorGroup>> {
        match group {
            GroupRef::Id(id) => self.editor_groups_container.get_group(id),
            GroupRef::Group(group) => Some(group),
        }
    }

    /// Procura em todos os grupos, em ordem MRU (ou sequencial, com `options.order`), o que `find_in_group` devolve de cada um.
    fn find_across_groups(&self, options: Option<FindEditorOptions>, find_in_group: impl Fn(&Rc<dyn IEditorGroup>) -> Vec<EditorInputRef>) -> Vec<EditorIdentifier> {
        let order = if options.and_then(|options| options.order) == Some(EditorsOrder::Sequential) { GroupsOrder::GridAppearance } else { GroupsOrder::MostRecentlyActive };

        let mut result = Vec::new();
        for group in self.editor_groups_container.get_groups(order) {
            result.extend(find_in_group(&group).into_iter().map(|editor| EditorIdentifier { editor, group_id: group.id() }));
        }

        result
    }

    /// `findEditors(editor, options, group)` num grupo só: o editor do grupo com o `typeId` do identificador.
    fn find_editor_of_identifier_in_group(group: &Rc<dyn IEditorGroup>, editor: &ResourceEditorInputIdentifier, options: Option<FindEditorOptions>) -> Option<EditorInputRef> {
        group.find_editors(&editor.resource, options).into_iter().find(|candidate| candidate.editor_type_id() == editor.type_id)
    }

    // endregion

    // region: save/revert

    /// `getAllModifiedEditors(options)`.
    fn get_all_modified_editors(&self, options: Option<BaseSaveRevertAllEditorOptions>) -> Vec<EditorIdentifier> {
        let include_untitled = options.and_then(|options| options.include_untitled);
        let exclude_sticky = options.and_then(|options| options.exclude_sticky) == Some(true);

        let mut editors = Vec::new();

        for group in self.editor_groups_container.get_groups(GroupsOrder::MostRecentlyActive) {
            for editor in group.get_editors(EditorsOrder::MostRecentlyActive, None) {
                if !editor.is_modified() {
                    continue;
                }

                if (matches!(include_untitled, Some(IncludeUntitled::Bool(_))) || !matches!(include_untitled, Some(IncludeUntitled::Options { include_scratchpad: true })))
                    && editor.has_capability(EditorInputCapabilities::SCRATCHPAD)
                {
                    continue;
                }

                // `!options?.includeUntitled`: ausente ou `false`
                if matches!(include_untitled, None | Some(IncludeUntitled::Bool(false))) && editor.has_capability(EditorInputCapabilities::UNTITLED) {
                    continue;
                }

                if exclude_sticky && group.is_sticky(&(&editor).into()) {
                    continue;
                }

                editors.push(EditorIdentifier { group_id: group.id(), editor });
            }
        }

        editors
    }

    /// `getUniqueEditors(editors)`: sem o mesmo editor mais de uma vez (`matches()` acha os repetidos).
    fn get_unique_editors(editors: Vec<EditorIdentifier>) -> Vec<EditorIdentifier> {
        let mut unique_editors: Vec<EditorIdentifier> = Vec::new();
        for EditorIdentifier { editor, group_id } in editors {
            if unique_editors.iter().any(|unique_editor| unique_editor.editor.matches(&EditorInputOrUntyped::Typed(editor.clone()))) {
                continue;
            }

            unique_editors.push(EditorIdentifier { editor, group_id });
        }

        unique_editors
    }

    // endregion
}

impl IEditorService for EditorService {
    fn on_did_active_editor_change(&self) -> Event<()> {
        self.on_did_active_editor_change.event()
    }

    fn on_did_visible_editors_change(&self) -> Event<VisibleEditorsChangeEvent> {
        self.on_did_visible_editors_change.event()
    }

    fn on_did_editors_change(&self) -> Event<EditorsChangeEvent> {
        self.on_did_editors_change.event()
    }

    fn on_will_open_editor(&self) -> Event<EditorWillOpenEvent> {
        self.on_will_open_editor.event()
    }

    fn on_did_close_editor(&self) -> Event<EditorCloseEvent> {
        self.on_did_close_editor.event()
    }

    fn on_did_open_editor_fail(&self) -> Event<EditorIdentifier> {
        self.on_did_open_editor_fail.event()
    }

    fn on_did_most_recently_active_editors_change(&self) -> Event<()> {
        self.on_did_most_recently_active_editors_change.event()
    }

    fn create_scoped(&self, editor_groups_container: EditorGroupsContainer, disposables: &DisposableStore) -> Rc<dyn IEditorService> {
        let deps: EditorServiceDeps = (
            self.editor_group_service.clone(),
            self.instantiation_service.clone(),
            self.file_service.clone(),
            self.configuration_service.clone(),
            self.context_service.clone(),
            self.uri_identity_service.clone(),
            self.editor_resolver_service.clone(),
            self.workspace_trust_request_service.clone(),
            self.host_service.clone(),
            self.text_editor_service.clone(),
        );

        let scoped = disposables.add_rc(Self::create(Some(editor_groups_container), deps));
        scoped
    }

    // region: Editor accessors

    fn active_editor_pane(&self) -> Option<Rc<dyn IEditorPane>> {
        self.editor_groups_container.active_group().active_editor_pane()
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        match self.active_editor_pane()?.get_control()? {
            EditorControl::Text(control) => Some(control),
            EditorControl::Composite { active_code_editor: Some(active_code_editor) } => Some(ActiveTextEditorControl::Code(active_code_editor)),
            EditorControl::Composite { active_code_editor: None } | EditorControl::Other => None,
        }
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        match self.active_text_editor_control()? {
            ActiveTextEditorControl::Diff { modified, .. } => modified.get_language_id(),
            ActiveTextEditorControl::Code(active_code_editor) => active_code_editor.get_language_id(),
        }
    }

    fn count(&self) -> usize {
        self.editors_observer.count()
    }

    fn editors(&self) -> Vec<EditorInputRef> {
        self.all_editors()
    }

    fn get_editors(&self, order: EditorsOrder) -> Vec<EditorIdentifier> {
        self.get_editors_with_options(order, None)
    }

    fn get_editors_with_options(&self, order: EditorsOrder, options: Option<GetEditorsOptions>) -> Vec<EditorIdentifier> {
        match order {
            // MRU
            EditorsOrder::MostRecentlyActive => {
                if options.and_then(|options| options.exclude_sticky) == Some(true) {
                    return self
                        .editors_observer
                        .editors()
                        .into_iter()
                        .filter(|EditorIdentifier { group_id, editor }| !self.editor_groups_container.get_group(*group_id).is_some_and(|group| group.is_sticky(&editor.into())))
                        .collect();
                }

                self.editors_observer.editors()
            }

            // Sequential
            EditorsOrder::Sequential => {
                let mut editors = Vec::new();

                for group in self.editor_groups_container.get_groups(GroupsOrder::GridAppearance) {
                    editors.extend(group.get_editors(EditorsOrder::Sequential, options).into_iter().map(|editor| EditorIdentifier { editor, group_id: group.id() }));
                }

                editors
            }
        }
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        self.active_editor_input().map(EditorInputOrUntyped::Typed)
    }

    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        self.editor_groups_container.groups().iter().filter_map(|group| group.active_editor_pane()).collect()
    }

    fn visible_text_editor_controls(&self) -> Vec<ActiveTextEditorControl> {
        Self::do_get_visible_text_editor_controls(&self.visible_editor_panes())
    }

    fn get_visible_text_editor_controls(&self, order: EditorsOrder) -> Vec<ActiveTextEditorControl> {
        let groups_order = if order == EditorsOrder::Sequential { GroupsOrder::GridAppearance } else { GroupsOrder::MostRecentlyActive };
        let editor_panes: Vec<Rc<dyn IEditorPane>> = self.editor_groups_container.get_groups(groups_order).iter().filter_map(|group| group.active_editor_pane()).collect();

        Self::do_get_visible_text_editor_controls(&editor_panes)
    }

    fn visible_editors(&self) -> Vec<EditorInputRef> {
        self.editor_groups_container.groups().iter().filter_map(|group| group.active_editor()).collect()
    }

    // endregion

    // region: openEditor()

    fn open_editor(&self, editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        self.open_editor_in(editor, None)
    }

    fn open_editor_in(&self, editor: UntypedEditorInput, group: Option<PreferredGroup>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        self.rc().do_open_editor(EditorInputOrUntyped::Untyped(editor), None, group)
    }

    fn open_editor_typed(&self, editor: &EditorInputRef, options: Option<TextEditorOptions>, group: Option<PreferredGroup>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        self.rc().do_open_editor(EditorInputOrUntyped::Typed(editor.clone()), options, group)
    }

    // endregion

    // region: openEditors()

    fn open_editors(&self, editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        self.open_editors_in(editors.into_iter().map(FindGroupEditor::WithOptions).collect(), None, None)
    }

    fn open_editors_in(&self, editors: Vec<FindGroupEditor>, group: Option<PreferredGroup>, options: Option<OpenEditorsOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        self.rc().do_open_editors(editors, group, options)
    }

    // endregion

    // region: isOpened() / isVisible()

    fn is_opened(&self, editor: &ResourceEditorInputIdentifier) -> bool {
        self.editors_observer.has_editor(&ResourceEditorInputIdentifier {
            resource: self.uri_identity_service.as_canonical_uri(&editor.resource),
            type_id: editor.type_id.clone(),
            editor_id: editor.editor_id.clone(),
        })
    }

    fn is_visible(&self, editor: &EditorInputRef) -> bool {
        self.editor_groups_container.groups().iter().any(|group| group.active_editor().is_some_and(|active_editor| active_editor.matches(&EditorInputOrUntyped::Typed(editor.clone()))))
    }

    // endregion

    // region: closeEditor()

    fn close_editor(&self, EditorIdentifier { editor, group_id }: EditorIdentifier, options: Option<CloseEditorOptions>) -> Promise<()> {
        let group = self.editor_groups_container.get_group(group_id);

        async_fn(async move {
            if let Some(group) = group {
                group.close_editor_with_options(Some(&editor), options).await?;
            }

            Ok(())
        })
    }

    // endregion

    // region: closeEditors()

    fn close_editors(&self, editors: Vec<EditorIdentifier>, options: Option<CloseEditorOptions>) -> Promise<()> {
        let mut map_group_to_editors: Vec<(Rc<dyn IEditorGroup>, Vec<EditorInputRef>)> = Vec::new();

        for EditorIdentifier { editor, group_id } in editors {
            let Some(group) = self.editor_groups_container.get_group(group_id) else {
                continue;
            };

            match map_group_to_editors.iter_mut().find(|(target_group, _)| target_group.id() == group.id()) {
                Some((_, group_editors)) => group_editors.push(editor),
                None => map_group_to_editors.push((group, vec![editor])),
            }
        }

        async_fn(async move {
            for (group, editors) in map_group_to_editors {
                group.close_editors(CloseEditorsTarget::Editors(editors), options).await?;
            }

            Ok(())
        })
    }

    // endregion

    // region: findEditors()

    fn find_editors(&self, resource: &URI, options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        // Do a quick check for the resource via the editor observer which is a very efficient way to find an editor by resource.
        // However, we can only do that unless we are asked to find an editor on the secondary side of a side by side editor,
        // because the editor observer provides fast lookups only for primary editors.
        if !self.may_have_editors(resource, options) {
            return Vec::new();
        }

        // Search across all groups in MRU order
        self.find_across_groups(options, |group| group.find_editors(resource, options))
    }

    fn find_editors_by_identifier(&self, editor: &ResourceEditorInputIdentifier, options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        if !self.may_have_editors(&editor.resource, options) {
            return Vec::new();
        }

        // Search across all groups in MRU order, with a single result per group
        self.find_across_groups(options, |group| Self::find_editor_of_identifier_in_group(group, editor, options).into_iter().collect())
    }

    fn find_editors_in_group(&self, resource: &URI, options: Option<FindEditorOptions>, group: GroupRef) -> Vec<EditorInputRef> {
        if !self.may_have_editors(resource, options) {
            return Vec::new();
        }

        // Search only in specific group
        match self.resolve_group(group) {
            Some(target_group) => target_group.find_editors(resource, options),
            None => Vec::new(),
        }
    }

    fn find_editor_in_group(&self, editor: &ResourceEditorInputIdentifier, options: Option<FindEditorOptions>, group: GroupRef) -> Option<EditorInputRef> {
        if !self.may_have_editors(&editor.resource, options) {
            return None;
        }

        // Search only in specific group, the result is single
        Self::find_editor_of_identifier_in_group(&self.resolve_group(group)?, editor, options)
    }

    // endregion

    // region: replaceEditors()

    fn replace_editors(&self, replacements: Vec<EditorReplacementInput>, group: GroupRef) -> Promise<()> {
        let this = self.rc();
        let target_group = self.resolve_group(group);

        async_fn_maybe(async move {
            // Convert all replacements to typed editors unless already typed and handle overrides properly.
            let mut typed_replacements: Vec<EditorReplacement> = Vec::new();
            for replacement in replacements {
                let typed_replacement = match replacement {
                    // Already typed: no override to resolve
                    EditorReplacementInput::Typed(replacement) => replacement,

                    EditorReplacementInput::Untyped(replacement) => {
                        // Resolve override unless disabled
                        let preferred_group = target_group.clone().map(PreferredGroup::Group);
                        match this.editor_resolver_service.resolve_editor(&replacement.replacement, preferred_group.as_ref()).await? {
                            // skip editor if override is aborted
                            ResolvedEditor::Status(ResolvedStatus::Abort) => continue,

                            // We resolved an editor to use
                            ResolvedEditor::Editor(resolved_editor) => EditorReplacement {
                                editor: replacement.editor,
                                replacement: resolved_editor.editor,
                                options: resolved_editor.options.map(|options| options.base),
                                force_replace_dirty: replacement.force_replace_dirty,
                            },

                            // Override is disabled or did not apply: fallback to default
                            ResolvedEditor::Status(ResolvedStatus::None) => {
                                let options = replacement.replacement.options.clone().map(|options| options.base);
                                EditorReplacement {
                                    editor: replacement.editor,
                                    replacement: this.text_editor_service.resolve_text_editor(replacement.replacement).await?,
                                    options,
                                    force_replace_dirty: replacement.force_replace_dirty,
                                }
                            }
                        }
                    }
                };

                typed_replacements.push(typed_replacement);
            }

            Ok(match target_group {
                Some(target_group) => MaybePromise::Promise(target_group.replace_editors(typed_replacements)),
                None => MaybePromise::Value(()),
            })
        })
    }

    // endregion

    // region: save/revert

    fn save(&self, editors: Vec<EditorIdentifier>, options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        let this = self.rc();

        // Make sure to not save the same editor multiple times by using the `matches()` method to find duplicates
        let unique_editors = Self::get_unique_editors(editors);

        // Split editors up into a bucket that is saved in parallel and sequentially. Unless "Save As", all non-untitled editors
        // can be saved in parallel to speed up the operation. Remaining editors are potentially bringing up some UI and thus
        // run sequentially.
        let mut editors_to_save_parallel: Vec<EditorIdentifier> = Vec::new();
        let mut editors_to_save_sequentially: Vec<EditorIdentifier> = Vec::new();
        if options.as_ref().is_some_and(|options| options.save_as == Some(true)) {
            editors_to_save_sequentially.extend(unique_editors);
        } else {
            for EditorIdentifier { group_id, editor } in unique_editors {
                if editor.has_capability(EditorInputCapabilities::UNTITLED) {
                    editors_to_save_sequentially.push(EditorIdentifier { group_id, editor });
                } else {
                    editors_to_save_parallel.push(EditorIdentifier { group_id, editor });
                }
            }
        }

        // Editors to save in parallel
        let save_options = options.as_ref().map(|options| options.base.clone());
        let save_as = options.as_ref().is_some_and(|options| options.save_as == Some(true));
        let save_promises: Vec<Promise<Option<EditorInputOrUntyped>>> = editors_to_save_parallel
            .into_iter()
            .map(|EditorIdentifier { group_id, editor }| {
                // Use save as a hint to pin the editor if used explicitly
                if options.as_ref().is_some_and(|options| options.base.reason == Some(SaveReason::Explicit))
                    && let Some(group) = this.editor_groups_container.get_group(group_id)
                {
                    group.pin_editor(Some(&editor));
                }

                // Save
                editor.save(group_id, save_options.clone())
            })
            .collect();

        async_fn(async move {
            let mut save_results: Vec<Option<EditorInputOrUntyped>> = promises::settled(save_promises).await?;

            // Editors to save sequentially
            for EditorIdentifier { group_id, editor } in editors_to_save_sequentially {
                if editor.is_disposed() {
                    continue; // might have been disposed from the save already
                }

                // Preserve view state by opening the editor first if the editor is untitled or we "Save As". This also allows
                // the user to review the contents of the editor before making a decision.
                let editor_pane = this.do_open_editor(EditorInputOrUntyped::Typed(editor.clone()), None, Some(PreferredGroup::from_number(group_id))).await?;
                let editor_options = EditorOptions { pinned: Some(true), view_state: editor_pane.and_then(|pane| pane.get_view_state()), ..EditorOptions::default() };

                let result = if save_as { editor.save_as(group_id, save_options.clone()).await? } else { editor.save(group_id, save_options.clone()).await? };
                save_results.push(result.clone());

                let Some(result) = result else {
                    break; // failed or cancelled, abort
                };

                // Replace editor preserving viewstate (either across all groups or only selected group) if the resulting editor is
                // different from the current one.
                if !editor.matches(&result) {
                    let target_groups: Vec<GroupIdentifier> = if editor.has_capability(EditorInputCapabilities::UNTITLED) {
                        this.editor_groups_container.groups().iter().map(|group| group.id()).collect() /* untitled replaces across all groups */
                    } else {
                        vec![group_id]
                    };
                    for target_group in target_groups {
                        let replacement = match &result {
                            EditorInputOrUntyped::Typed(result) => {
                                EditorReplacementInput::Typed(EditorReplacement { editor: editor.clone(), replacement: result.clone(), options: Some(editor_options.clone()), force_replace_dirty: None })
                            }
                            EditorInputOrUntyped::Untyped(result) => EditorReplacementInput::Untyped(UntypedEditorReplacement {
                                editor: editor.clone(),
                                replacement: UntypedEditorInput { options: Some(editor_options.clone().into()), ..result.clone() },
                                force_replace_dirty: None,
                            }),
                        };
                        this.replace_editors(vec![replacement], GroupRef::Id(target_group)).await?;
                    }
                }
            }

            Ok(SaveEditorsResult { success: save_results.iter().all(Option::is_some), editors: save_results.into_iter().flatten().collect() })
        })
    }

    fn save_all(&self, options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        self.save(self.get_all_modified_editors(options.as_ref().map(SaveAllEditorsOptions::base)), options.map(|options| options.save))
    }

    fn revert(&self, editors: Vec<EditorIdentifier>, options: Option<crate::common::editor::RevertOptions>) -> Promise<bool> {
        // Make sure to not revert the same editor multiple times by using the `matches()` method to find duplicates
        let unique_editors = Self::get_unique_editors(editors);

        let revert_promises: Vec<Promise<()>> = unique_editors
            .iter()
            .map(|EditorIdentifier { group_id, editor }| {
                // Use revert as a hint to pin the editor
                if let Some(group) = self.editor_groups_container.get_group(*group_id) {
                    group.pin_editor(Some(editor));
                }

                editor.revert(*group_id, options)
            })
            .collect();

        async_fn(async move {
            promises::settled(revert_promises).await?;

            Ok(!unique_editors.iter().any(|EditorIdentifier { editor, .. }| editor.is_dirty()))
        })
    }

    fn revert_all(&self, options: Option<RevertAllEditorsOptions>) -> Promise<bool> {
        self.revert(self.get_all_modified_editors(options.map(|options| options.base)), options.map(|options| options.revert))
    }

    // endregion
}
