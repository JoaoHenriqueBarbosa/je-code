//! Porte de `src/vs/workbench/services/workingCopy/common/workingCopyBackupTracker.ts` (o rastreador abstrato de backups)
//! e de `src/vs/workbench/services/workingCopy/browser/workingCopyBackupTracker.ts` (o rastreador do navegador, que só
//! decide o veto do desligamento). O rastreador restaura os backups que existem, cria backups das working copies
//! modificadas, descarta os backups das salvas e trata os backups no desligamento.
//!
//! Divergências:
//! - **Classe abstrata.** `WorkingCopyBackupTracker` é a struct concreta [`WorkingCopyBackupTracker`], e os membros que as
//!   subclasses sobrescrevem (`onFinalBeforeShutdown`, abstrato, e `getBackupScheduleDelay`) são o trait
//!   [`WorkingCopyBackupTrackerHooks`], que a struct recebe no construtor e chama passando a si mesma. O corpo base do
//!   `getBackupScheduleDelay` é [`WorkingCopyBackupTracker::default_backup_schedule_delay`] (o `super.getBackupScheduleDelay`
//!   das subclasses); o do `suspendBackupOperations` é [`WorkingCopyBackupTracker::default_suspend_backup_operations`], e
//!   [`WorkingCopyBackupTracker::suspend_backup_operations`] passa pelos hooks. `BrowserWorkingCopyBackupTracker` é o [`BrowserWorkingCopyBackupTracker`], um [`Injectable`] que guarda
//!   o rastreador criado com os [`BrowserWorkingCopyBackupTrackerHooks`].
//! - **Membros `protected`** (`pendingBackupOperations`, `unrestoredBackups`, `whenReady`, `isReady`, `restoreBackups`,
//!   `getContentVersion`, `cancelBackupOperations`, `suspendBackupOperations`, os serviços injetados) são métodos públicos:
//!   o Rust não tem acesso de subclasse, e as subclasses de teste e os hooks leem esses membros.
//! - **Identidade.** O `Map<IWorkingCopy, number>` das versões de conteúdo e o `Map<IWorkingCopyIdentifier, ...>` das operações
//!   pendentes usam a identidade do objeto: o primeiro é indexado pelo endereço do `Rc` da working copy, e as operações
//!   pendentes guardam um número de sequência no lugar da identidade do identificador (o mapa é um `Vec` em ordem de
//!   inserção). Os `Set<EditorInput>` do restaurador comparam o endereço do `Rc`. O `Set<IWorkingCopyIdentifier>` dos backups
//!   não restaurados compara o valor (`typeId` e recurso), porque o identificador do porte é um valor.
//! - **Valores `T | Promise<T>`.** `onFinalBeforeShutdown` (`boolean | Promise<boolean>`) devolve [`MaybePromise`], e o `await` de
//!   `handler.handles(...)` e `handler.createEditor(...)` é o `await_maybe`.
//! - **`event as InternalBeforeShutdownEvent`.** O `onBeforeShutdown` do trait entrega o `BeforeShutdownEvent`: o
//!   [`InternalBeforeShutdownEvent::from_event`] é a visão interna do mesmo evento, cujo `finalVeto` fica no evento base.
//! - **Temporizador e `async`.** `setTimeout(async () => ...)` é `runtime::set_timeout` que dispara um `async_fn`; o `try/catch`
//!   que loga o erro e o `return` de dentro do `try` (que pula a limpeza) são um `Result<bool, Error>` interno. As referências
//!   a `this` dentro das funções `async` são `Weak`, para uma operação pendente não manter o rastreador vivo.
//! - **Logs.** Os argumentos `...args` do `logService` são `[resource.toString(), typeId]` como [`LogArg`].
//! - **`IWorkbenchContribution` e `registerWorkbenchContribution2`.** O contrato de contribuição e o registro de fase ainda não
//!   foram portados: a classe traz só o `ID`. O `BrowserWorkingCopyBackupService` (que o mesmo arquivo do original registra) não
//!   faz parte deste porte.
//! - **`isVisible`, `openEditors`, `getEditors`.** O recorte do `IEditorService` do porte ganhou os três métodos que o restaurador
//!   lê (ver `editor_service.rs`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, await_maybe, promises};
use je_base::cancellation::CancellationTokenSource;
use je_base::errors::Error;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::log::{LogArg, LogMessage, LogService, LogServiceRef, Logger};
use je_platform::opener::editor_types::EditorOptions;

use crate::common::editor::{EditorInputRef, EditorInputWithOptions, EditorsOrder, TextEditorOptions};
use crate::services::editor::editor_service::IEditorService;
use crate::services::files_configuration::files_configuration_service::{IFilesConfigurationService, ResourceOrEditor};
use crate::services::lifecycle::lifecycle::{ILifecycleService, InternalBeforeShutdownEvent, LifecyclePhase, ShutdownReason};
use crate::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyCapabilities, WorkingCopyIdentifier};
use crate::services::working_copy::working_copy_backup::IWorkingCopyBackupService;
use crate::services::working_copy::working_copy_editor_service::{IWorkingCopyEditorHandler, IWorkingCopyEditorService};
use crate::services::working_copy::working_copy_service::IWorkingCopyService;

/// `DEFAULT_BACKUP_SCHEDULE_DELAYS['default']`.
const DEFAULT_BACKUP_SCHEDULE_DELAY: u64 = 1000;

/// `DEFAULT_BACKUP_SCHEDULE_DELAYS['delayed']`.
const DELAYED_BACKUP_SCHEDULE_DELAY: u64 = 2000;

/// Os membros que as subclasses de `WorkingCopyBackupTracker` sobrescrevem.
pub trait WorkingCopyBackupTrackerHooks {
    /// `protected abstract onFinalBeforeShutdown(reason)`.
    fn on_final_before_shutdown(&self, tracker: &WorkingCopyBackupTracker, reason: ShutdownReason) -> MaybePromise<bool>;

    /// `protected getBackupScheduleDelay(workingCopy)`: o padrão é o corpo da classe base.
    fn get_backup_schedule_delay(&self, tracker: &WorkingCopyBackupTracker, working_copy: &dyn IWorkingCopy) -> u64 {
        tracker.default_backup_schedule_delay(working_copy)
    }

    /// `protected suspendBackupOperations()`: o padrão é o corpo da classe base.
    fn suspend_backup_operations(&self, tracker: &WorkingCopyBackupTracker) -> BackupOperationsResume {
        tracker.default_suspend_backup_operations()
    }
}

/// O valor do mapa de operações pendentes: `{ disposable: IDisposable; cancel: () => void }`, com o número de sequência que
/// faz o papel da identidade do identificador.
struct PendingBackupOperation {
    id: u64,
    identifier: WorkingCopyIdentifier,
    cancel: Rc<dyn Fn()>,
    disposable: DisposableRef,
}

/// O `{ resume: () => void }` que `suspendBackupOperations` devolve.
pub struct BackupOperationsResume {
    resume: Rc<dyn Fn()>,
}

impl BackupOperationsResume {
    /// O objeto `{ resume }` com a função dada (as subclasses embrulham o `resume` da base).
    pub fn new(resume: impl Fn() + 'static) -> Self {
        Self { resume: Rc::new(resume) }
    }

    /// `resume()`.
    pub fn resume(&self) {
        (self.resume)();
    }
}

/// Os argumentos `workingCopy.resource.toString(), workingCopy.typeId` dos logs.
fn log_args(resource: &URI, type_id: &str) -> Vec<LogArg> {
    vec![LogArg::from(resource.to_string(false).to_string_lossy().as_str()), LogArg::from(type_id)]
}

/// A identidade do objeto da working copy (o endereço do `Rc`).
fn working_copy_key(working_copy: &Rc<dyn IWorkingCopy>) -> usize {
    Rc::as_ptr(working_copy).cast::<()>() as usize
}

/// `a === b` para dois editores.
fn is_same_editor(a: &EditorInputRef, b: &EditorInputRef) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `set.add(editor)` de um `Set<EditorInput>` (identidade).
fn add_editor(set: &mut Vec<EditorInputRef>, editor: &EditorInputRef) {
    if !set.iter().any(|candidate| is_same_editor(candidate, editor)) {
        set.push(editor.clone());
    }
}

/// `WorkingCopyBackupTracker`.
pub struct WorkingCopyBackupTracker {
    this: Weak<WorkingCopyBackupTracker>,
    base: Rc<Disposable>,
    working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
    working_copy_service: Rc<dyn IWorkingCopyService>,
    log_service: LogServiceRef,
    lifecycle_service: Rc<dyn ILifecycleService>,
    files_configuration_service: Rc<dyn IFilesConfigurationService>,
    working_copy_editor_service: Rc<dyn IWorkingCopyEditorService>,
    editor_service: Rc<dyn IEditorService>,
    hooks: Rc<dyn WorkingCopyBackupTrackerHooks>,

    /// A map from working copy to a version ID we compute on each content change. This version ID allows to e.g. ask if a
    /// backup for a specific content has been made before closing.
    map_working_copy_to_content_version: RefCell<HashMap<usize, u64>>,
    /// A map of scheduled pending backup operations for working copies.
    pending_backup_operations: RefCell<Vec<PendingBackupOperation>>,
    next_operation_id: Cell<u64>,
    suspended: Rc<Cell<bool>>,

    unrestored_backups: RefCell<Vec<WorkingCopyIdentifier>>,
    when_ready: RefCell<Option<Promise<()>>>,
    is_ready: Cell<bool>,
}

impl WorkingCopyBackupTracker {
    /// `constructor(workingCopyBackupService, workingCopyService, logService, lifecycleService, filesConfigurationService,
    /// workingCopyEditorService, editorService)` mais os `hooks` da subclasse.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
        working_copy_service: Rc<dyn IWorkingCopyService>,
        log_service: LogServiceRef,
        lifecycle_service: Rc<dyn ILifecycleService>,
        files_configuration_service: Rc<dyn IFilesConfigurationService>,
        working_copy_editor_service: Rc<dyn IWorkingCopyEditorService>,
        editor_service: Rc<dyn IEditorService>,
        hooks: Rc<dyn WorkingCopyBackupTrackerHooks>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: Disposable::new(),
            working_copy_backup_service,
            working_copy_service,
            log_service,
            lifecycle_service,
            files_configuration_service,
            working_copy_editor_service,
            editor_service,
            hooks,
            map_working_copy_to_content_version: RefCell::new(HashMap::new()),
            pending_backup_operations: RefCell::new(Vec::new()),
            next_operation_id: Cell::new(0),
            suspended: Rc::new(Cell::new(false)),
            unrestored_backups: RefCell::new(Vec::new()),
            when_ready: RefCell::new(None),
            is_ready: Cell::new(false),
        });

        *this.when_ready.borrow_mut() = Some(this.resolve_backups_to_restore());

        // Fill in initial modified working copies
        for working_copy in this.working_copy_service.modified_working_copies() {
            this.on_did_register(&working_copy);
        }

        this.register_listeners();

        this
    }

    fn register_listeners(&self) {
        // Working Copy events
        let weak = self.this.clone();
        self.base.register(self.working_copy_service.on_did_register().listen({
            let weak = weak.clone();
            move |working_copy| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_register(working_copy);
                }
            }
        }));
        self.base.register(self.working_copy_service.on_did_unregister().listen({
            let weak = weak.clone();
            move |working_copy| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_unregister(working_copy);
                }
            }
        }));
        self.base.register(self.working_copy_service.on_did_change_dirty().listen({
            let weak = weak.clone();
            move |working_copy| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_dirty(working_copy);
                }
            }
        }));
        self.base.register(self.working_copy_service.on_did_change_content().listen({
            let weak = weak.clone();
            move |working_copy| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_content(working_copy);
                }
            }
        }));

        // Lifecycle
        self.base.register(self.lifecycle_service.on_before_shutdown().listen({
            let weak = weak.clone();
            move |event| {
                let reason = event.reason;
                let final_veto_weak = weak.clone();
                InternalBeforeShutdownEvent::from_event(event.clone()).final_veto(
                    Rc::new(move || match final_veto_weak.upgrade() {
                        Some(this) => this.hooks.on_final_before_shutdown(&this, reason),
                        None => MaybePromise::Value(false),
                    }),
                    "veto.backups",
                );
            }
        }));
        self.base.register(self.lifecycle_service.on_will_shutdown().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_will_shutdown();
                }
            }
        }));

        // Once a handler registers, restore backups
        self.base.register(self.working_copy_editor_service.on_did_register_handler().listen(move |handler| {
            if let Some(this) = weak.upgrade() {
                drop(this.restore_backups(handler.clone()));
            }
        }));
    }

    /// O `this` do rastreador, para as subclasses (os `hooks`) o segurarem durante um `async`.
    pub fn rc(&self) -> Rc<WorkingCopyBackupTracker> {
        self.this.upgrade().expect("o rastreador está vivo enquanto os hooks rodam")
    }

    /// `workingCopyBackupService` (protegido).
    pub fn working_copy_backup_service(&self) -> &Rc<dyn IWorkingCopyBackupService> {
        &self.working_copy_backup_service
    }

    /// `workingCopyService` (protegido).
    pub fn working_copy_service(&self) -> &Rc<dyn IWorkingCopyService> {
        &self.working_copy_service
    }

    /// `logService` (protegido).
    pub fn log_service(&self) -> &LogServiceRef {
        &self.log_service
    }

    /// `filesConfigurationService` (protegido).
    pub fn files_configuration_service(&self) -> &Rc<dyn IFilesConfigurationService> {
        &self.files_configuration_service
    }

    /// `editorService` (protegido).
    pub fn editor_service(&self) -> &Rc<dyn IEditorService> {
        &self.editor_service
    }

    fn on_will_shutdown(&self) {
        // Here we know that we will shutdown. Any backup operation that is
        // already scheduled or being scheduled from this moment on runs
        // at the risk of corrupting a backup because the backup operation
        // might terminate at any given time now. As such, we need to disable
        // this tracker from performing more backups by cancelling pending
        // operations and suspending the tracker without resuming.

        self.cancel_backup_operations();
        self.suspend_backup_operations();
    }

    //#region Backup Creator

    fn warn_suspended(&self, what: &str, working_copy: &dyn IWorkingCopy) {
        self.log_service
            .warn_args(&LogMessage::from(format!("[backup tracker] suspended, ignoring {what} event")), &log_args(&working_copy.resource(), &working_copy.working_copy_type_id()));
    }

    fn trace(&self, message: &str, resource: &URI, type_id: &str) {
        self.log_service.trace_args(message, &log_args(resource, type_id));
    }

    fn on_did_register(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        if self.suspended.get() {
            self.warn_suspended("register", &**working_copy);
            return;
        }

        if working_copy.is_modified() {
            self.schedule_backup(working_copy);
        }
    }

    fn on_did_unregister(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        // Remove from content version map
        self.map_working_copy_to_content_version.borrow_mut().remove(&working_copy_key(working_copy));

        // Check suspended
        if self.suspended.get() {
            self.warn_suspended("unregister", &**working_copy);
            return;
        }

        // Discard backup
        self.discard_backup(working_copy);
    }

    fn on_did_change_dirty(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        if self.suspended.get() {
            self.warn_suspended("dirty change", &**working_copy);
            return;
        }

        if working_copy.is_dirty() {
            self.schedule_backup(working_copy);
        } else {
            self.discard_backup(working_copy);
        }
    }

    fn on_did_change_content(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        // Increment content version ID
        let content_version_id = self.get_content_version(working_copy);
        self.map_working_copy_to_content_version.borrow_mut().insert(working_copy_key(working_copy), content_version_id + 1);

        // Check suspended
        if self.suspended.get() {
            self.warn_suspended("content change", &**working_copy);
            return;
        }

        // Schedule backup for modified working copies
        if working_copy.is_modified() {
            // this listener will make sure that the backup is
            // pushed out for as long as the user is still changing
            // the content of the working copy.
            self.schedule_backup(working_copy);
        }
    }

    fn next_operation_id(&self) -> u64 {
        let id = self.next_operation_id.get();
        self.next_operation_id.set(id + 1);

        id
    }

    fn schedule_backup(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        // Clear any running backup operation
        self.cancel_backup_operation(&**working_copy);

        let resource = working_copy.resource();
        let type_id = working_copy.working_copy_type_id();
        self.trace("[backup tracker] scheduling backup", &resource, &type_id);

        // Schedule new backup
        let identifier = WorkingCopyIdentifier { type_id: type_id.clone(), resource: resource.clone() };
        let id = self.next_operation_id();
        let cts = CancellationTokenSource::new(None);
        let handle = runtime::set_timeout(
            {
                let (weak, working_copy, identifier, cts) = (self.this.clone(), working_copy.clone(), identifier.clone(), cts.clone());
                move || drop(async_fn(Self::run_scheduled_backup(weak, working_copy, identifier, cts, id)))
            },
            self.hooks.get_backup_schedule_delay(self, &**working_copy),
        );

        // Keep in map for disposal as needed
        let log_service = self.log_service.clone();
        let cancel_cts = cts.clone();
        self.pending_backup_operations.borrow_mut().push(PendingBackupOperation {
            id,
            identifier,
            cancel: Rc::new(move || {
                log_service.trace_args("[backup tracker] clearing pending backup creation", &log_args(&resource, &type_id));

                cancel_cts.cancel();
            }),
            disposable: to_disposable(move || {
                cts.dispose_with(false);
                runtime::clear_timeout(handle);
            }),
        });
    }

    /// O corpo do `setTimeout(async () => { ... })` de `scheduleBackup`.
    async fn run_scheduled_backup(
        weak: Weak<WorkingCopyBackupTracker>,
        working_copy: Rc<dyn IWorkingCopy>,
        identifier: WorkingCopyIdentifier,
        cts: CancellationTokenSource,
        id: u64,
    ) -> Result<(), Error> {
        let token = cts.token();
        if token.is_cancellation_requested() {
            return Ok(());
        }

        // Backup if modified
        if working_copy.is_modified() {
            let Some(this) = weak.upgrade() else { return Ok(()) };
            this.trace("[backup tracker] creating backup", &identifier.resource, &identifier.type_id);
            drop(this);

            // `true` quando o `return` de dentro do `try` pulou a limpeza.
            let aborted: Result<bool, Error> = async {
                let backup = working_copy.backup(&token).await?;
                if token.is_cancellation_requested() {
                    return Ok(true);
                }

                if working_copy.is_modified() {
                    let Some(this) = weak.upgrade() else { return Ok(true) };
                    this.trace("[backup tracker] storing backup", &identifier.resource, &identifier.type_id);
                    let version_id = this.get_content_version(&working_copy);
                    let backup_service = this.working_copy_backup_service.clone();
                    drop(this);

                    backup_service.backup(&identifier, backup.content, Some(version_id), backup.meta, Some(token.clone())).await?;
                }

                Ok(false)
            }
            .await;

            match aborted {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(error) => {
                    if let Some(this) = weak.upgrade() {
                        this.log_service.error_args(&LogMessage::from(&error), &[]);
                    }
                }
            }
        }

        // Clear disposable unless we got canceled which would
        // indicate another operation has started meanwhile
        if !token.is_cancellation_requested()
            && let Some(this) = weak.upgrade()
        {
            this.do_clear_pending_backup_operation(id, false);
        }

        Ok(())
    }

    /// `getBackupScheduleDelay(workingCopy)` da classe base.
    pub fn default_backup_schedule_delay(&self, working_copy: &dyn IWorkingCopy) -> u64 {
        if let Some(backup_delay) = working_copy.backup_delay() {
            return backup_delay; // respect working copy override
        }

        // auto-save is never on for untitled working copies
        if working_copy.capabilities().contains(WorkingCopyCapabilities::UNTITLED) {
            return DEFAULT_BACKUP_SCHEDULE_DELAY;
        }

        if self.files_configuration_service.has_short_auto_save_delay(Some(&ResourceOrEditor::Resource(working_copy.resource()))) {
            DELAYED_BACKUP_SCHEDULE_DELAY
        } else {
            DEFAULT_BACKUP_SCHEDULE_DELAY
        }
    }

    /// `getContentVersion(workingCopy)`.
    pub fn get_content_version(&self, working_copy: &Rc<dyn IWorkingCopy>) -> u64 {
        self.map_working_copy_to_content_version.borrow().get(&working_copy_key(working_copy)).copied().unwrap_or(0)
    }

    fn discard_backup(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        // Clear any running backup operation
        self.cancel_backup_operation(&**working_copy);

        // Schedule backup discard asap
        let identifier = WorkingCopyIdentifier::of(&**working_copy);
        let id = self.next_operation_id();
        let cts = CancellationTokenSource::new(None);
        drop(async_fn(Self::do_discard_backup(self.this.clone(), identifier.clone(), cts.clone(), id)));

        // Keep in map for disposal as needed
        let log_service = self.log_service.clone();
        let cancel_cts = cts.clone();
        let (resource, type_id) = (identifier.resource.clone(), identifier.type_id.clone());
        self.pending_backup_operations.borrow_mut().push(PendingBackupOperation {
            id,
            identifier,
            cancel: Rc::new(move || {
                log_service.trace_args("[backup tracker] clearing pending backup discard", &log_args(&resource, &type_id));

                cancel_cts.cancel();
            }),
            disposable: Rc::new(cts),
        });
    }

    async fn do_discard_backup(weak: Weak<WorkingCopyBackupTracker>, identifier: WorkingCopyIdentifier, cts: CancellationTokenSource, id: u64) -> Result<(), Error> {
        let Some(this) = weak.upgrade() else { return Ok(()) };
        this.trace("[backup tracker] discarding backup", &identifier.resource, &identifier.type_id);
        let discard = this.working_copy_backup_service.discard_backup(&identifier, Some(cts.token()));
        drop(this);

        // Discard backup
        if let Err(error) = discard.await
            && let Some(this) = weak.upgrade()
        {
            this.log_service.error_args(&LogMessage::from(&error), &[]);
        }

        // Clear disposable unless we got canceled which would
        // indicate another operation has started meanwhile
        if !cts.token().is_cancellation_requested()
            && let Some(this) = weak.upgrade()
        {
            this.do_clear_pending_backup_operation(id, false);
        }

        Ok(())
    }

    fn cancel_backup_operation(&self, working_copy: &dyn IWorkingCopy) {
        // Given a working copy we want to find the matching
        // identifier in our pending operations map because
        // we cannot use the working copy directly, as the
        // identifier might have different object identity.

        let resource = working_copy.resource().to_string(false);
        let type_id = working_copy.working_copy_type_id();
        let found = self
            .pending_backup_operations
            .borrow()
            .iter()
            .find(|operation| operation.identifier.resource.to_string(false) == resource && operation.identifier.type_id == type_id)
            .map(|operation| operation.id);

        if let Some(id) = found {
            self.do_clear_pending_backup_operation(id, true);
        }
    }

    fn do_clear_pending_backup_operation(&self, id: u64, cancel: bool) {
        let operation = {
            let mut operations = self.pending_backup_operations.borrow_mut();
            let Some(position) = operations.iter().position(|operation| operation.id == id) else { return };

            operations.remove(position)
        };

        if cancel {
            (operation.cancel)();
        }

        operation.disposable.dispose();
    }

    /// `cancelBackupOperations()` (protegido).
    pub fn cancel_backup_operations(&self) {
        let operations = std::mem::take(&mut *self.pending_backup_operations.borrow_mut());
        for operation in operations {
            (operation.cancel)();
            operation.disposable.dispose();
        }
    }

    /// `suspendBackupOperations()` (protegido): passa pelos `hooks`, que podem sobrescrevê-lo.
    pub fn suspend_backup_operations(&self) -> BackupOperationsResume {
        self.hooks.suspend_backup_operations(self)
    }

    /// `suspendBackupOperations()` da classe base (o `super.suspendBackupOperations` das subclasses).
    pub fn default_suspend_backup_operations(&self) -> BackupOperationsResume {
        self.suspended.set(true);

        let suspended = self.suspended.clone();
        BackupOperationsResume::new(move || suspended.set(false))
    }

    /// `pendingBackupOperations.size` (protegido, lido pelas subclasses de teste).
    pub fn pending_backup_operation_count(&self) -> usize {
        self.pending_backup_operations.borrow().len()
    }

    //#endregion

    //#region Backup Restorer

    /// `unrestoredBackups` (protegido): os backups, em ordem de inserção.
    pub fn unrestored_backups(&self) -> Vec<WorkingCopyIdentifier> {
        self.unrestored_backups.borrow().clone()
    }

    /// `whenReady` (protegido).
    pub fn when_ready(&self) -> Promise<()> {
        self.when_ready.borrow().clone().expect("o `whenReady` é atribuído no construtor")
    }

    /// `isReady` (protegido).
    pub fn is_ready(&self) -> bool {
        self.is_ready.get()
    }

    fn resolve_backups_to_restore(&self) -> Promise<()> {
        let weak = self.this.clone();
        let lifecycle_service = self.lifecycle_service.clone();
        let backup_service = self.working_copy_backup_service.clone();

        async_fn(async move {
            // Wait for resolving backups until we are restored to reduce startup pressure
            lifecycle_service.when(LifecyclePhase::Restored).await?;

            // Remember each backup that needs to restore
            let backups = backup_service.get_backups().await?;
            let Some(this) = weak.upgrade() else { return Ok(()) };
            for backup in backups {
                let mut unrestored = this.unrestored_backups.borrow_mut();
                if !unrestored.contains(&backup) {
                    unrestored.push(backup);
                }
            }

            this.is_ready.set(true);

            Ok(())
        })
    }

    /// `restoreBackups(handler)` (protegido).
    pub fn restore_backups(&self, handler: Rc<dyn IWorkingCopyEditorHandler>) -> Promise<()> {
        let weak = self.this.clone();
        let when_ready = self.when_ready();

        async_fn(async move {
            // Wait for backups to be resolved
            when_ready.await?;

            let Some(this) = weak.upgrade() else { return Ok(()) };

            this.do_restore_backups(handler).await
        })
    }

    async fn do_restore_backups(self: Rc<Self>, handler: Rc<dyn IWorkingCopyEditorHandler>) -> Result<(), Error> {
        // Figure out already opened editors for backups vs
        // non-opened.
        let mut opened_editors_for_backups: Vec<EditorInputRef> = Vec::new();
        let mut non_opened_editors_for_backups: Vec<EditorInputRef> = Vec::new();

        // Ensure each backup that can be handled has an
        // associated editor.
        let mut restored_backups: Vec<WorkingCopyIdentifier> = Vec::new();
        for unrestored_backup in self.unrestored_backups() {
            // O `for...of` do `Set` enxerga as remoções que outro restaurador fez no meio do caminho.
            if !self.unrestored_backups.borrow().contains(&unrestored_backup) {
                continue;
            }

            let can_handle_unrestored_backup = await_maybe(handler.handles(&unrestored_backup)).await?;
            if !can_handle_unrestored_backup {
                continue;
            }

            // Collect already opened editors for backup
            let mut has_opened_editor_for_backup = false;
            for identifier in self.editor_service.get_editors(EditorsOrder::MostRecentlyActive) {
                let is_unrestored_backup_opened = handler.is_open(&unrestored_backup, &identifier.editor);
                if is_unrestored_backup_opened {
                    add_editor(&mut opened_editors_for_backups, &identifier.editor);
                    has_opened_editor_for_backup = true;
                }
            }

            // Otherwise, make sure to create at least one editor
            // for the backup to show
            if !has_opened_editor_for_backup {
                let editor = await_maybe(handler.create_editor(&unrestored_backup)).await?;
                add_editor(&mut non_opened_editors_for_backups, &editor);
            }

            // Remember as (potentially) restored
            if !restored_backups.contains(&unrestored_backup) {
                restored_backups.push(unrestored_backup);
            }
        }

        // Ensure editors are opened for each backup without editor
        // in the background without stealing focus
        if !non_opened_editors_for_backups.is_empty() {
            let editors = non_opened_editors_for_backups
                .iter()
                .map(|non_opened_editor_for_backup| EditorInputWithOptions {
                    editor: non_opened_editor_for_backup.clone(),
                    options: Some(TextEditorOptions::from(EditorOptions {
                        pinned: Some(true),
                        preserve_focus: Some(true),
                        inactive: Some(true),
                        ..EditorOptions::default()
                    })),
                })
                .collect();
            self.editor_service.open_editors(editors).await?;

            for non_opened_editor_for_backup in &non_opened_editors_for_backups {
                add_editor(&mut opened_editors_for_backups, non_opened_editor_for_backup);
            }
        }

        // Then, resolve each opened editor to make sure the working copy
        // is loaded and the modified editor appears properly.
        // We only do that for editors that are not active in a group
        // already to prevent calling `resolve` twice!
        let editor_service = self.editor_service.clone();
        promises::settled(
            opened_editors_for_backups
                .into_iter()
                .map(|opened_editor_for_backup| {
                    let editor_service = editor_service.clone();

                    async_fn(async move {
                        if editor_service.is_visible(&opened_editor_for_backup) {
                            return Ok(());
                        }

                        opened_editor_for_backup.resolve().await?;

                        Ok(())
                    })
                })
                .collect(),
        )
        .await?;

        // Finally, remove all handled backups from the list
        self.unrestored_backups.borrow_mut().retain(|backup| !restored_backups.contains(backup));

        Ok(())
    }

    //#endregion
}

impl IDisposable for WorkingCopyBackupTracker {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `BrowserWorkingCopyBackupTracker.onFinalBeforeShutdown`.
pub struct BrowserWorkingCopyBackupTrackerHooks;

impl WorkingCopyBackupTrackerHooks for BrowserWorkingCopyBackupTrackerHooks {
    fn on_final_before_shutdown(&self, tracker: &WorkingCopyBackupTracker, _reason: ShutdownReason) -> MaybePromise<bool> {
        // Web: we cannot perform long running in the shutdown phase
        // As such we need to check sync if there are any modified working
        // copies that have not been backed up yet and then prevent the
        // shutdown if that is the case.

        let modified_working_copies = tracker.working_copy_service().modified_working_copies();
        if modified_working_copies.is_empty() {
            return MaybePromise::Value(false); // nothing modified: no veto
        }

        if !tracker.files_configuration_service().is_hot_exit_enabled() {
            return MaybePromise::Value(true); // modified without backup: veto
        }

        for modified_working_copy in &modified_working_copies {
            let identifier = WorkingCopyIdentifier::of(&**modified_working_copy);
            if !tracker.working_copy_backup_service().has_backup_sync(&identifier, Some(tracker.get_content_version(modified_working_copy)), None) {
                tracker.log_service().warn("Unload veto: pending backups");

                return MaybePromise::Value(true); // modified without backup: veto
            }
        }

        MaybePromise::Value(false) // modified and backed up: no veto
    }
}

/// `BrowserWorkingCopyBackupTracker`.
pub struct BrowserWorkingCopyBackupTracker {
    tracker: Rc<WorkingCopyBackupTracker>,
}

impl BrowserWorkingCopyBackupTracker {
    pub const ID: &'static str = "workbench.contrib.browserWorkingCopyBackupTracker";

    /// O [`WorkingCopyBackupTracker`] que esta classe estende.
    pub fn tracker(&self) -> &Rc<WorkingCopyBackupTracker> {
        &self.tracker
    }
}

impl Injectable for BrowserWorkingCopyBackupTracker {
    type Args = ();
    type Deps = (
        Rc<dyn IWorkingCopyBackupService>,
        Rc<dyn IFilesConfigurationService>,
        Rc<dyn IWorkingCopyService>,
        Rc<dyn ILifecycleService>,
        Rc<dyn LogService>,
        Rc<dyn IWorkingCopyEditorService>,
        Rc<dyn IEditorService>,
    );

    fn create(
        (): Self::Args,
        (working_copy_backup_service, files_configuration_service, working_copy_service, lifecycle_service, log_service, working_copy_editor_service, editor_service): Self::Deps,
    ) -> Rc<Self> {
        Rc::new(Self {
            tracker: WorkingCopyBackupTracker::new(
                working_copy_backup_service,
                working_copy_service,
                log_service,
                lifecycle_service,
                files_configuration_service,
                working_copy_editor_service,
                editor_service,
                Rc::new(BrowserWorkingCopyBackupTrackerHooks),
            ),
        })
    }
}

impl IDisposable for BrowserWorkingCopyBackupTracker {
    fn dispose(&self) {
        self.tracker.dispose();
    }
}
