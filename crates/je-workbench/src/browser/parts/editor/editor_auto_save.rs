//! Porte de `src/vs/workbench/browser/parts/editor/editorAutoSave.ts`: o [`EditorAutoSave`], que salva automaticamente as
//! working copies e os editores sujos conforme a configuração `files.autoSave` (depois de um atraso, na mudança de foco do
//! editor, na mudança de janela) e retoma o salvamento quando uma condição que o impedia (erros nos marcadores, salvamento
//! desligado para o recurso) deixa de valer.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`], na ordem do original. A classe
//!   herda de `Disposable`, aqui um `Disposable` guardado no campo `base`. `IWorkbenchContribution` ainda não foi portado: a
//!   classe traz só o `ID`.
//! - **Identidade das working copies.** O `Map<IWorkingCopy, IDisposable>` do salvamento depois do atraso usa a identidade do
//!   objeto: a chave é o endereço do `Rc` da working copy.
//! - **`ResourceMap` com a chave de comparação.** O `ResourceMap` das condições usa
//!   `uriIdentityService.extUri.getComparisonKey(resource)`, como o original, e os valores são as structs
//!   [`WaitingWorkingCopy`] e [`WaitingEditor`].
//! - **`IAutoSaveMode`.** O `autoSaveMode.reason` só existe na variante `Disabled` do enum: o teste de `ERRORS` ou `DISABLED` é
//!   [`waiting_condition`].
//! - **`onDidChangeActiveWindow`.** O `IHostService` do porte ganhou o evento (`Event<u32>`, o identificador da janela), que o
//!   original lê aqui.
//! - **`onDidBlur`.** O `IEditorPane` do porte ganhou `on_did_blur` (padrão: nunca dispara), que o original lê aqui.
//! - **`setTimeout`.** O atraso (`number`) é `f64` no porte do serviço de configuração e vira milissegundos inteiros (um valor
//!   negativo vale zero, como no `setTimeout`). A promessa devolvida por `save` é solta sem aguardar, como o original.
//! - **Logs.** Os argumentos `...args` do `logService` são `[resource.toString(), typeId]` como [`LogArg`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::map::ResourceMap;
use je_base::runtime;
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::log::{LogArg, LogServiceRef, Logger};
use je_platform::markers::IMarkerService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::{EditorIdentifier, EditorInputCapabilities, EditorInputRef, GroupIdentifier, SaveOptions, SaveReason};
use crate::services::editor::editor_groups_service::IEditorGroupsService;
use crate::services::editor::editor_service::{IEditorService, SaveEditorsOptions};
use crate::services::files_configuration::files_configuration_service::{
    AutoSaveDisabledReason, AutoSaveMode, IAutoSaveMode, IFilesConfigurationService, ResourceOrEditor,
};
use crate::services::host::host::IHostService;
use crate::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyCapabilities};
use crate::services::working_copy::working_copy_service::IWorkingCopyService;

/// O valor de `waitingOnConditionAutoSaveWorkingCopies`: `{ workingCopy, reason, condition }`.
#[derive(Clone)]
struct WaitingWorkingCopy {
    working_copy: Rc<dyn IWorkingCopy>,
    reason: SaveReason,
    condition: AutoSaveDisabledReason,
}

/// O valor de `waitingOnConditionAutoSaveEditors`: `{ editor, reason, condition }`.
#[derive(Clone)]
struct WaitingEditor {
    editor: EditorIdentifier,
    reason: SaveReason,
    condition: AutoSaveDisabledReason,
}

/// `autoSaveMode.reason === AutoSaveDisabledReason.ERRORS || autoSaveMode.reason === AutoSaveDisabledReason.DISABLED`: o motivo
/// quando é uma das duas condições que podem passar.
fn waiting_condition(mode: IAutoSaveMode) -> Option<AutoSaveDisabledReason> {
    match mode {
        IAutoSaveMode::Disabled(reason @ (AutoSaveDisabledReason::Errors | AutoSaveDisabledReason::Disabled)) => Some(reason),
        IAutoSaveMode::Disabled(_) | IAutoSaveMode::Enabled(_) => None,
    }
}

/// `{ reason }`.
fn save_options(reason: SaveReason) -> SaveOptions {
    SaveOptions { reason: Some(reason), ..SaveOptions::default() }
}

/// Os argumentos `workingCopy.resource.toString(), workingCopy.typeId` dos logs.
fn log_args(resource: &URI, type_id: &str) -> Vec<LogArg> {
    vec![LogArg::from(resource.to_string(false).to_string_lossy().as_str()), LogArg::from(type_id)]
}

/// A identidade do objeto da working copy (o endereço do `Rc`).
fn working_copy_key(working_copy: &Rc<dyn IWorkingCopy>) -> usize {
    Rc::as_ptr(working_copy).cast::<()>() as usize
}

/// `EditorAutoSave`.
pub struct EditorAutoSave {
    this: Weak<EditorAutoSave>,
    base: Rc<Disposable>,

    /// Auto save: after delay.
    scheduled_auto_saves_after_delay: RefCell<HashMap<usize, DisposableRef>>,

    /// Auto save: focus change & window change.
    last_active_editor: RefCell<Option<EditorInputRef>>,
    last_active_group_id: Cell<Option<GroupIdentifier>>,
    last_active_editor_control_disposable: Rc<DisposableStore>,

    /// Auto save: waiting on specific condition.
    waiting_on_condition_auto_save_working_copies: RefCell<ResourceMap<WaitingWorkingCopy>>,
    waiting_on_condition_auto_save_editors: RefCell<ResourceMap<WaitingEditor>>,

    files_configuration_service: Rc<dyn IFilesConfigurationService>,
    host_service: Rc<dyn IHostService>,
    editor_service: Rc<dyn IEditorService>,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    working_copy_service: Rc<dyn IWorkingCopyService>,
    log_service: LogServiceRef,
    marker_service: Rc<dyn IMarkerService>,
}

impl Injectable for EditorAutoSave {
    type Args = ();
    type Deps = (
        Rc<dyn IFilesConfigurationService>,
        Rc<dyn IHostService>,
        Rc<dyn IEditorService>,
        Rc<dyn IEditorGroupsService>,
        Rc<dyn IWorkingCopyService>,
        LogServiceRef,
        Rc<dyn IMarkerService>,
        Rc<dyn IUriIdentityService>,
    );

    fn create(
        _: (),
        (
            files_configuration_service,
            host_service,
            editor_service,
            editor_group_service,
            working_copy_service,
            log_service,
            marker_service,
            uri_identity_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let last_active_editor_control_disposable = DisposableStore::new();
        base.register(last_active_editor_control_disposable.clone());

        let comparison_key = {
            let uri_identity_service = uri_identity_service.clone();
            move |resource: &URI| uri_identity_service.ext_uri().get_comparison_key(resource, false)
        };

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            scheduled_auto_saves_after_delay: RefCell::new(HashMap::new()),
            last_active_editor: RefCell::new(None),
            last_active_group_id: Cell::new(None),
            last_active_editor_control_disposable,
            waiting_on_condition_auto_save_working_copies: RefCell::new(ResourceMap::with_key_fn(Rc::new(comparison_key.clone()))),
            waiting_on_condition_auto_save_editors: RefCell::new(ResourceMap::with_key_fn(Rc::new(comparison_key))),
            files_configuration_service,
            host_service,
            editor_service,
            editor_group_service,
            working_copy_service,
            log_service,
            marker_service,
        });

        // Fill in initial dirty working copies
        for dirty_working_copy in this.working_copy_service.dirty_working_copies() {
            this.on_did_register(&dirty_working_copy);
        }

        this.register_listeners();

        this
    }
}

impl IDisposable for EditorAutoSave {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorAutoSave {
    /// `EditorAutoSave.ID`.
    pub const ID: &'static str = "workbench.contrib.editorAutoSave";

    /// `this._register(event(listener))`, com o `this` seguro por `Weak` para o ouvinte não manter a classe viva.
    fn listen<T: 'static>(&self, event: Event<T>, handler: impl Fn(&EditorAutoSave, &T) + 'static) {
        let weak = self.this.clone();
        self.base.register(event.listen(move |value| {
            if let Some(this) = weak.upgrade() {
                handler(&this, value);
            }
        }));
    }

    fn trace(&self, message: &str, resource: &URI, type_id: &str) {
        self.log_service.trace_args(message, &log_args(resource, type_id));
    }

    fn register_listeners(&self) {
        self.listen(self.host_service.on_did_change_focus(), |this, focused| this.on_window_focus_change(*focused));
        self.listen(self.host_service.on_did_change_active_window(), |this, _| this.on_active_window_change());
        self.listen(self.editor_service.on_did_active_editor_change(), |this, _| this.on_did_active_editor_change());
        self.listen(self.files_configuration_service.on_did_change_auto_save_configuration(), |this, _| this.on_did_change_auto_save_configuration());

        // Working Copy events
        self.listen(self.working_copy_service.on_did_register(), |this, working_copy| this.on_did_register(working_copy));
        self.listen(self.working_copy_service.on_did_unregister(), |this, working_copy| this.on_did_unregister(working_copy));
        self.listen(self.working_copy_service.on_did_change_dirty(), |this, working_copy| this.on_did_change_dirty(working_copy));
        self.listen(self.working_copy_service.on_did_change_content(), |this, working_copy| this.on_did_change_content(working_copy));

        // Condition changes
        self.listen(self.marker_service.on_marker_changed(), |this, resources| this.on_condition_changed(resources, AutoSaveDisabledReason::Errors));
        self.listen(self.files_configuration_service.on_did_change_auto_save_disabled(), |this, resource| {
            this.on_condition_changed(std::slice::from_ref(resource), AutoSaveDisabledReason::Disabled)
        });
    }

    /// `editorService.save(editorIdentifier, { reason })`.
    fn save_editor(&self, editor: EditorIdentifier, reason: SaveReason) {
        drop(self.editor_service.save(vec![editor], Some(SaveEditorsOptions { base: save_options(reason), ..SaveEditorsOptions::default() })));
    }

    fn on_condition_changed(&self, resources: &[URI], condition: AutoSaveDisabledReason) {
        for resource in resources {
            // Waiting working copies
            let working_copy_result = self.waiting_on_condition_auto_save_working_copies.borrow().get(resource).cloned();
            match working_copy_result {
                Some(result) if result.condition == condition => {
                    let working_copy_resource = ResourceOrEditor::Resource(result.working_copy.resource());
                    if result.working_copy.is_dirty()
                        && self.files_configuration_service.get_auto_save_mode(Some(&working_copy_resource), Some(result.reason)).mode() != AutoSaveMode::Off
                    {
                        self.discard_auto_save(&result.working_copy);

                        self.trace(
                            "[editor auto save] running auto save from condition change event",
                            &result.working_copy.resource(),
                            &result.working_copy.working_copy_type_id(),
                        );
                        drop(result.working_copy.save(Some(save_options(result.reason))));
                    }
                }

                // Waiting editors
                _ => {
                    let editor_result = self.waiting_on_condition_auto_save_editors.borrow().get(resource).cloned();
                    let Some(editor_result) = editor_result else { continue };
                    let editor = &editor_result.editor.editor;
                    if editor_result.condition == condition
                        && !editor.is_disposed()
                        && editor.is_dirty()
                        && self.files_configuration_service.get_auto_save_mode(Some(&ResourceOrEditor::from_editor(editor)), Some(editor_result.reason)).mode()
                            != AutoSaveMode::Off
                    {
                        self.waiting_on_condition_auto_save_editors.borrow_mut().delete(resource);

                        self.log_service.trace(&format!("[editor auto save] running auto save from condition change event with reason {}", editor_result.reason as i32));
                        self.save_editor(editor_result.editor.clone(), editor_result.reason);
                    }
                }
            }
        }
    }

    fn on_window_focus_change(&self, focused: bool) {
        if !focused {
            self.maybe_trigger_auto_save(SaveReason::WindowChange, None);
        }
    }

    fn on_active_window_change(&self) {
        self.maybe_trigger_auto_save(SaveReason::WindowChange, None);
    }

    fn on_did_active_editor_change(&self) {
        // Treat editor change like a focus change for our last active editor if any
        let last_active_editor = self.last_active_editor.borrow().clone();
        if let (Some(editor), Some(group_id)) = (last_active_editor, self.last_active_group_id.get()) {
            self.maybe_trigger_auto_save(SaveReason::FocusChange, Some(EditorIdentifier { group_id, editor }));
        }

        // Remember as last active
        let active_group = self.editor_group_service.active_group();
        let active_editor = active_group.active_editor();
        *self.last_active_editor.borrow_mut() = active_editor.clone();
        self.last_active_group_id.set(Some(active_group.id()));

        // Dispose previous active control listeners
        self.last_active_editor_control_disposable.clear();

        // Listen to focus changes on control for auto save
        let active_editor_pane = self.editor_service.active_editor_pane();
        if let (Some(active_editor), Some(active_editor_pane)) = (active_editor, active_editor_pane) {
            let group_id = active_group.id();
            let weak = self.this.clone();
            self.last_active_editor_control_disposable.add(active_editor_pane.on_did_blur().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.maybe_trigger_auto_save(SaveReason::FocusChange, Some(EditorIdentifier { group_id, editor: active_editor.clone() }));
                }
            }));
        }
    }

    fn maybe_trigger_auto_save(&self, reason: SaveReason, editor_identifier: Option<EditorIdentifier>) {
        let Some(editor_identifier) = editor_identifier else {
            self.save_all_dirty_auto_saveables(reason);
            return;
        };

        let editor = &editor_identifier.editor;
        if !editor.is_dirty() || editor.is_readonly().is_readonly() || editor.has_capability(EditorInputCapabilities::UNTITLED) {
            return; // no auto save for non-dirty, readonly or untitled editors
        }

        let auto_save_mode = self.files_configuration_service.get_auto_save_mode(Some(&ResourceOrEditor::from_editor(editor)), Some(reason));
        if auto_save_mode.mode() != AutoSaveMode::Off {
            // Determine if we need to save all. In case of a window focus change we also save if
            // auto save mode is configured to be ON_FOCUS_CHANGE (editor focus change)
            let mode = auto_save_mode.mode();
            if (reason == SaveReason::WindowChange && (mode == AutoSaveMode::OnFocusChange || mode == AutoSaveMode::OnWindowChange))
                || (reason == SaveReason::FocusChange && mode == AutoSaveMode::OnFocusChange)
            {
                self.log_service.trace(&format!("[editor auto save] triggering auto save with reason {}", reason as i32));
                self.save_editor(editor_identifier.clone(), reason);
            }
        } else if let (Some(resource), Some(condition)) = (editor.resource(), waiting_condition(auto_save_mode)) {
            self.waiting_on_condition_auto_save_editors.borrow_mut().set(resource, WaitingEditor { editor: editor_identifier.clone(), reason, condition });
        }
    }

    fn on_did_change_auto_save_configuration(&self) {
        // Trigger a save-all when auto save is enabled
        let reason = match self.files_configuration_service.get_auto_save_mode(None, None).mode() {
            AutoSaveMode::OnFocusChange => Some(SaveReason::FocusChange),
            AutoSaveMode::OnWindowChange => Some(SaveReason::WindowChange),
            AutoSaveMode::AfterShortDelay | AutoSaveMode::AfterLongDelay => Some(SaveReason::Auto),
            AutoSaveMode::Off => None,
        };

        if let Some(reason) = reason {
            self.save_all_dirty_auto_saveables(reason);
        }
    }

    fn save_all_dirty_auto_saveables(&self, reason: SaveReason) {
        for working_copy in self.working_copy_service.dirty_working_copies() {
            if working_copy.capabilities().contains(WorkingCopyCapabilities::UNTITLED) {
                continue; // we never auto save untitled working copies
            }

            let auto_save_mode = self.files_configuration_service.get_auto_save_mode(Some(&ResourceOrEditor::Resource(working_copy.resource())), Some(reason));
            if auto_save_mode.mode() != AutoSaveMode::Off {
                drop(working_copy.save(Some(save_options(reason))));
            } else if let Some(condition) = waiting_condition(auto_save_mode) {
                self.waiting_on_condition_auto_save_working_copies.borrow_mut().set(working_copy.resource(), WaitingWorkingCopy { working_copy: working_copy.clone(), reason, condition });
            }
        }
    }

    fn on_did_register(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        if working_copy.is_dirty() {
            self.schedule_auto_save(working_copy);
        }
    }

    fn on_did_unregister(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        self.discard_auto_save(working_copy);
    }

    fn on_did_change_dirty(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        if working_copy.is_dirty() {
            self.schedule_auto_save(working_copy);
        } else {
            self.discard_auto_save(working_copy);
        }
    }

    fn on_did_change_content(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        if working_copy.is_dirty() {
            // this listener will make sure that the auto save is
            // pushed out for as long as the user is still changing
            // the content of the working copy.
            self.schedule_auto_save(working_copy);
        }
    }

    fn schedule_auto_save(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        if working_copy.capabilities().contains(WorkingCopyCapabilities::UNTITLED) {
            return; // we never auto save untitled working copies
        }

        let resource = working_copy.resource();
        let type_id = working_copy.working_copy_type_id();

        let Some(auto_save_after_delay) = self.files_configuration_service.get_auto_save_configuration(Some(&ResourceOrEditor::Resource(resource.clone()))).auto_save_delay
        else {
            return; // auto save after delay must be enabled
        };

        // Clear any running auto save operation
        self.discard_auto_save(working_copy);

        self.trace(&format!("[editor auto save] scheduling auto save after {auto_save_after_delay}ms"), &resource, &type_id);

        // Schedule new auto save
        let handle = runtime::set_timeout(
            {
                let (weak, working_copy) = (self.this.clone(), working_copy.clone());
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.run_scheduled_auto_save(&working_copy);
                    }
                }
            },
            auto_save_after_delay as u64,
        );

        // Keep in map for disposal as needed
        let log_service = self.log_service.clone();
        self.scheduled_auto_saves_after_delay.borrow_mut().insert(
            working_copy_key(working_copy),
            to_disposable(move || {
                log_service.trace_args("[editor auto save] clearing pending auto save", &log_args(&resource, &type_id));

                runtime::clear_timeout(handle);
            }),
        );
    }

    /// O corpo do `setTimeout` de `scheduleAutoSave`.
    fn run_scheduled_auto_save(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        // Clear pending
        self.discard_auto_save(working_copy);

        // Save if dirty and unless prevented by other conditions such as error markers
        if working_copy.is_dirty() {
            let reason = SaveReason::Auto;
            let auto_save_mode = self.files_configuration_service.get_auto_save_mode(Some(&ResourceOrEditor::Resource(working_copy.resource())), Some(reason));
            if auto_save_mode.mode() != AutoSaveMode::Off {
                self.trace("[editor auto save] running auto save", &working_copy.resource(), &working_copy.working_copy_type_id());
                drop(working_copy.save(Some(save_options(reason))));
            } else if let Some(condition) = waiting_condition(auto_save_mode) {
                self.waiting_on_condition_auto_save_working_copies.borrow_mut().set(working_copy.resource(), WaitingWorkingCopy { working_copy: working_copy.clone(), reason, condition });
            }
        }
    }

    fn discard_auto_save(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        let scheduled = self.scheduled_auto_saves_after_delay.borrow_mut().remove(&working_copy_key(working_copy));
        if let Some(scheduled) = scheduled {
            scheduled.dispose();
        }

        let resource = working_copy.resource();
        self.waiting_on_condition_auto_save_working_copies.borrow_mut().delete(&resource);
        self.waiting_on_condition_auto_save_editors.borrow_mut().delete(&resource);
    }
}
