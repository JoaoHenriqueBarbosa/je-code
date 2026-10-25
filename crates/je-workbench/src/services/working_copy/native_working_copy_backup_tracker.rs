//! Porte de `src/vs/workbench/services/workingCopy/electron-browser/workingCopyBackupTracker.ts`: o rastreador de backups do
//! desktop (`NativeWorkingCopyBackupTracker`), que decide o veto do desligamento: salva o que o salvamento automático cobre,
//! cria backups conforme o `files.hotExit`, pergunta ao usuário o que sobrou (salvar, descartar, cancelar) e descarta os
//! backups do que foi salvo ou revertido.
//!
//! Divergências:
//! - **Herança.** `NativeWorkingCopyBackupTracker extends WorkingCopyBackupTracker` é a composição que o rastreador do
//!   navegador já usa: [`NativeWorkingCopyBackupTrackerHooks`] guarda os membros que a subclasse acrescenta (o
//!   `onFinalBeforeShutdown` abstrato e os métodos privados) e implementa [`WorkingCopyBackupTrackerHooks`];
//!   [`NativeWorkingCopyBackupTracker`] guarda o [`WorkingCopyBackupTracker`] criado com eles. Uma subclasse de teste cria o
//!   rastreador base com os próprios hooks, que embrulham os nativos.
//! - **`@INativeHostService`.** O serviço de host nativo não foi portado: [`INativeHostService`] é o recorte que o rastreador
//!   lê (`getWindowCount`).
//! - **Valores `async`.** Os métodos `async` do original são `async fn` que o rastreador espera com `.await?`; o que o original
//!   faz num `try`/`catch` que loga e segue é um `match` sobre o `Result`. O `onFinalBeforeShutdown` cancela e suspende as
//!   operações de forma síncrona antes de devolver a promessa (como a função `async` do original, que roda sem pausa até o
//!   primeiro `await`).
//! - **Identidade das working copies.** `!backups.includes(workingCopy)` compara o endereço do `Rc`.
//! - **Sobrecargas de `noVeto` e `discardBackupsBeforeShutdown`.** `IWorkingCopyIdentifier[] | { except }` é o enum
//!   [`BackupsToDiscard`].
//! - **`IWorkbenchContribution`, `registerWorkbenchContribution2` e o contêiner de injeção.** O contrato de contribuição ainda
//!   não foi portado e o construtor recebe cada serviço por argumento, como o do rastreador comum.
//! - **`localize`.** A chave e a mensagem são as do original; o `localize` do porte devolve a mensagem em inglês.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::{async_fn, promises, race_cancellation};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_instantiation::service_id;
use je_platform::dialogs::{
    BaseDialogOptions, ConfirmResult, DialogDetail, DialogType, DialogTypeOrSeverity, FileNameOrResource, IDialogService, IFileDialogService, Prompt, PromptBaseButton,
    PromptButton, get_file_names_message,
};
use je_platform::environment::environment::EnvironmentService;
use je_platform::log::{LogServiceRef, Logger};
use je_platform::progress::progress::{IProgressService, ProgressCancellable, ProgressLocation, ProgressOptions};
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState};

use crate::common::editor::{RevertOptions, SaveOptions, SaveReason};
use crate::services::editor::editor_service::{IEditorService, IncludeUntitled, RevertAllEditorsOptions, SaveAllEditorsOptions, SaveEditorsOptions};
use crate::services::files_configuration::files_configuration_service::{AutoSaveMode, IFilesConfigurationService, ResourceOrEditor};
use je_platform::files::files::HotExitConfiguration;
use crate::services::lifecycle::lifecycle::{ILifecycleService, ShutdownReason};
use crate::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyCapabilities, WorkingCopyIdentifier};
use crate::services::working_copy::working_copy_backup::{DiscardBackupsFilter, IWorkingCopyBackupService};
use crate::services::working_copy::working_copy_backup_tracker::{WorkingCopyBackupTracker, WorkingCopyBackupTrackerHooks};
use crate::services::working_copy::working_copy_editor_service::IWorkingCopyEditorService;
use crate::services::working_copy::working_copy_service::IWorkingCopyService;

/// `INativeHostService`: o recorte que o rastreador lê.
pub trait INativeHostService {
    /// `getWindowCount()`.
    fn get_window_count(&self) -> Promise<usize>;
}

service_id!(pub I_NATIVE_HOST_SERVICE: dyn INativeHostService = "nativeHostService");

/// O `IWorkingCopyIdentifier[] | { except: IWorkingCopyIdentifier[] }` de `noVeto` e `discardBackupsBeforeShutdown`.
pub enum BackupsToDiscard {
    /// `backupsToDiscard`: descarta estes backups.
    Discard(Vec<WorkingCopyIdentifier>),
    /// `backupsToKeep`: descarta todos, menos estes.
    Keep { except: Vec<WorkingCopyIdentifier> },
}

impl BackupsToDiscard {
    /// Uma working copy É um `IWorkingCopyIdentifier` no original: aqui o identificador sai dela.
    fn of_working_copies(working_copies: &[Rc<dyn IWorkingCopy>]) -> Self {
        Self::Discard(working_copies.iter().map(|working_copy| WorkingCopyIdentifier::of(&**working_copy)).collect())
    }
}

/// O `{ backups: IWorkingCopy[]; error?: Error }` de `backupBeforeShutdown`.
struct BackupResult {
    backups: Vec<Rc<dyn IWorkingCopy>>,
    error: Option<Error>,
}

/// A identidade do objeto da working copy (o endereço do `Rc`).
fn is_same_working_copy(a: &Rc<dyn IWorkingCopy>, b: &Rc<dyn IWorkingCopy>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// Os membros que o `NativeWorkingCopyBackupTracker` acrescenta ao rastreador comum.
#[derive(Clone)]
pub struct NativeWorkingCopyBackupTrackerHooks {
    file_dialog_service: Rc<dyn IFileDialogService>,
    dialog_service: Rc<dyn IDialogService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    native_host_service: Rc<dyn INativeHostService>,
    environment_service: Rc<dyn EnvironmentService>,
    progress_service: Rc<dyn IProgressService>,
}

impl NativeWorkingCopyBackupTrackerHooks {
    pub fn new(
        file_dialog_service: Rc<dyn IFileDialogService>,
        dialog_service: Rc<dyn IDialogService>,
        context_service: Rc<dyn IWorkspaceContextService>,
        native_host_service: Rc<dyn INativeHostService>,
        environment_service: Rc<dyn EnvironmentService>,
        progress_service: Rc<dyn IProgressService>,
    ) -> Self {
        Self { file_dialog_service, dialog_service, context_service, native_host_service, environment_service, progress_service }
    }

    /// O corpo do `try` de `onFinalBeforeShutdown`.
    async fn final_before_shutdown(&self, tracker: &Rc<WorkingCopyBackupTracker>, reason: ShutdownReason) -> Result<bool, Error> {
        // Modified working copies need treatment on shutdown
        let modified_working_copies = tracker.working_copy_service().modified_working_copies();
        if !modified_working_copies.is_empty() {
            self.on_before_shutdown_with_modified(tracker, reason, modified_working_copies).await
        }
        // No modified working copies
        else {
            self.on_before_shutdown_without_modified(tracker).await
        }
    }

    async fn on_before_shutdown_with_modified(
        &self,
        tracker: &Rc<WorkingCopyBackupTracker>,
        reason: ShutdownReason,
        modified_working_copies: Vec<Rc<dyn IWorkingCopy>>,
    ) -> Result<bool, Error> {
        // If auto save is enabled, save all non-untitled working copies
        // and then check again for modified copies

        let working_copies_to_auto_save: Vec<Rc<dyn IWorkingCopy>> = modified_working_copies
            .iter()
            .filter(|working_copy| {
                !working_copy.capabilities().contains(WorkingCopyCapabilities::UNTITLED)
                    && tracker.files_configuration_service().get_auto_save_mode(Some(&ResourceOrEditor::Resource(working_copy.resource())), None).mode() != AutoSaveMode::Off
            })
            .cloned()
            .collect();
        if !working_copies_to_auto_save.is_empty() {
            // Save all modified working copies that can be auto-saved
            if let Err(error) = self.do_save_all_before_shutdown(tracker, working_copies_to_auto_save, SaveReason::Auto).await {
                tracker.log_service().error(&format!("[backup tracker] error saving modified working copies: {error}")); // guard against misbehaving saves, we handle remaining modified below
            }

            // If we still have modified working copies, we either have untitled ones or working copies that cannot be saved
            let remaining_modified_working_copies = tracker.working_copy_service().modified_working_copies();
            if !remaining_modified_working_copies.is_empty() {
                return self.handle_modified_before_shutdown(tracker, remaining_modified_working_copies, reason).await;
            }

            return self.no_veto(tracker, BackupsToDiscard::of_working_copies(&modified_working_copies)).await; // no veto (modified auto-saved)
        }

        // Auto save is not enabled
        self.handle_modified_before_shutdown(tracker, modified_working_copies, reason).await
    }

    async fn handle_modified_before_shutdown(
        &self,
        tracker: &Rc<WorkingCopyBackupTracker>,
        modified_working_copies: Vec<Rc<dyn IWorkingCopy>>,
        reason: ShutdownReason,
    ) -> Result<bool, Error> {
        // Trigger backup if configured and enabled for shutdown reason
        let mut backups: Vec<Rc<dyn IWorkingCopy>> = Vec::new();
        let mut backup_error: Option<Error> = None;
        let modified_working_copies_to_backup = self.should_backup_before_shutdown(tracker, reason, &modified_working_copies).await?;
        if !modified_working_copies_to_backup.is_empty() {
            match self.backup_before_shutdown(tracker, modified_working_copies_to_backup).await {
                Ok(backup_result) => {
                    backups = backup_result.backups;
                    backup_error = backup_result.error;

                    if backups.len() == modified_working_copies.len() {
                        return Ok(false); // no veto (backup was successful for all working copies)
                    }
                }
                Err(error) => backup_error = Some(error),
            }
        }

        let remaining_modified_working_copies: Vec<Rc<dyn IWorkingCopy>> =
            modified_working_copies.iter().filter(|working_copy| !backups.iter().any(|backup| is_same_working_copy(backup, working_copy))).cloned().collect();

        // We ran a backup but received an error that we show to the user
        if let Some(backup_error) = backup_error {
            if self.environment_service.is_extension_development() {
                tracker.log_service().error(&format!("[backup tracker] error creating backups: {backup_error}"));

                return Ok(false); // do not block shutdown during extension development (https://github.com/microsoft/vscode/issues/115028)
            }

            return self
                .show_error_dialog(
                    tracker,
                    localize("backupTrackerBackupFailed", "The following editors with unsaved changes could not be saved to the backup location.", &[]),
                    &remaining_modified_working_copies,
                    &backup_error,
                    reason,
                )
                .await;
        }

        // Since a backup did not happen, we have to confirm for
        // the working copies that did not successfully backup

        match self.confirm_before_shutdown(tracker, &remaining_modified_working_copies).await {
            Ok(veto) => Ok(veto),
            Err(error) => {
                if self.environment_service.is_extension_development() {
                    tracker.log_service().error(&format!("[backup tracker] error saving or reverting modified working copies: {error}"));

                    return Ok(false); // do not block shutdown during extension development (https://github.com/microsoft/vscode/issues/115028)
                }

                self.show_error_dialog(
                    tracker,
                    localize("backupTrackerConfirmFailed", "The following editors with unsaved changes could not be saved or reverted.", &[]),
                    &remaining_modified_working_copies,
                    &error,
                    reason,
                )
                .await
            }
        }
    }

    async fn should_backup_before_shutdown(
        &self,
        tracker: &Rc<WorkingCopyBackupTracker>,
        reason: ShutdownReason,
        modified_working_copies: &[Rc<dyn IWorkingCopy>],
    ) -> Result<Vec<Rc<dyn IWorkingCopy>>, Error> {
        let files_configuration_service = tracker.files_configuration_service();
        if !files_configuration_service.is_hot_exit_enabled() {
            return Ok(Vec::new()); // never backup when hot exit is disabled via settings
        }

        if self.environment_service.is_extension_development() {
            return Ok(modified_working_copies.to_vec()); // always backup closing extension development window without asking to speed up debugging
        }

        let scratchpads = || -> Vec<Rc<dyn IWorkingCopy>> {
            modified_working_copies.iter().filter(|modified_working_copy| modified_working_copy.capabilities().contains(WorkingCopyCapabilities::SCRATCHPAD)).cloned().collect()
        };
        let on_exit_and_window_close = files_configuration_service.hot_exit_configuration().as_deref() == Some(HotExitConfiguration::ON_EXIT_AND_WINDOW_CLOSE);

        Ok(match reason {
            // Window Close
            ShutdownReason::Close => {
                if self.context_service.get_workbench_state() != WorkbenchState::Empty && on_exit_and_window_close {
                    return Ok(modified_working_copies.to_vec()); // backup if a workspace/folder is open and onExitAndWindowClose is configured
                }

                if Platform::detect().is_macintosh || self.native_host_service.get_window_count().await? > 1 {
                    if self.context_service.get_workbench_state() != WorkbenchState::Empty {
                        return Ok(scratchpads()); // backup scratchpads automatically to avoid user confirmation
                    }

                    return Ok(Vec::new()); // do not backup if a window is closed that does not cause quitting of the application
                }

                modified_working_copies.to_vec() // backup if last window is closed on win/linux where the application quits right after
            }

            // Application Quit
            ShutdownReason::Quit => modified_working_copies.to_vec(), // backup because next start we restore all backups

            // Window Reload
            ShutdownReason::Reload => modified_working_copies.to_vec(), // backup because after window reload, backups restore

            // Workspace Change
            ShutdownReason::Load => {
                if self.context_service.get_workbench_state() != WorkbenchState::Empty {
                    if on_exit_and_window_close {
                        return Ok(modified_working_copies.to_vec()); // backup if a workspace/folder is open and onExitAndWindowClose is configured
                    }

                    return Ok(scratchpads()); // backup scratchpads automatically to avoid user confirmation
                }

                Vec::new() // do not backup because we are switching contexts with no workspace/folder open
            }
        })
    }

    async fn show_error_dialog(
        &self,
        tracker: &Rc<WorkingCopyBackupTracker>,
        message: String,
        working_copies: &[Rc<dyn IWorkingCopy>],
        error: &Error,
        reason: ShutdownReason,
    ) -> Result<bool, Error> {
        tracker.log_service().error(&format!("[backup tracker] {message}: {error}"));

        let modified_working_copies: Vec<&Rc<dyn IWorkingCopy>> = working_copies.iter().filter(|working_copy| working_copy.is_modified()).collect();

        let advice = localize("backupErrorDetails", "Try saving or reverting the editors with unsaved changes first and then try again.", &[]);
        let detail = if modified_working_copies.is_empty() {
            advice
        } else {
            let names: Vec<FileNameOrResource> = modified_working_copies.iter().map(|working_copy| FileNameOrResource::Name(working_copy.name())).collect();

            format!("{}\n{advice}", get_file_names_message(&names))
        };

        let result = self
            .dialog_service
            .prompt(Prompt {
                base: BaseDialogOptions {
                    dialog_type: Some(DialogTypeOrSeverity::Type(DialogType::Error)),
                    detail: Some(DialogDetail::Text(detail)),
                    ..BaseDialogOptions::new(message)
                },
                buttons: Some(vec![
                    PromptButton {
                        label: localize("ok", "&&OK", &[]),
                        base: PromptBaseButton { run: Rc::new(|_| MaybePromise::Value(Dynamic::from(true))) }, // veto
                    },
                    PromptButton {
                        label: Self::to_force_shutdown_label(reason),
                        base: PromptBaseButton { run: Rc::new(|_| MaybePromise::Value(Dynamic::from(false))) }, // no veto
                    },
                ]),
                cancel_button: None,
            })
            .await?
            .result;

        Ok(match result {
            Some(Dynamic::Bool(veto)) => veto,
            _ => true, // `result ?? true`
        })
    }

    fn to_force_shutdown_label(reason: ShutdownReason) -> String {
        match reason {
            ShutdownReason::Close | ShutdownReason::Load => localize("shutdownForceClose", "Close Anyway", &[]),
            ShutdownReason::Quit => localize("shutdownForceQuit", "Quit Anyway", &[]),
            ShutdownReason::Reload => localize("shutdownForceReload", "Reload Anyway", &[]),
        }
    }

    async fn backup_before_shutdown(&self, tracker: &Rc<WorkingCopyBackupTracker>, modified_working_copies: Vec<Rc<dyn IWorkingCopy>>) -> Result<BackupResult, Error> {
        let backups: Rc<RefCell<Vec<Rc<dyn IWorkingCopy>>>> = Rc::default();
        let error: Rc<RefCell<Option<Error>>> = Rc::default();

        let (task_tracker, task_backups, task_error) = (tracker.clone(), backups.clone(), error.clone());
        self.with_progress_and_cancellation(
            move |token| {
                async_fn(async move {
                    // Perform a backup of all modified working copies unless a backup already exists
                    let backup_operations = modified_working_copies
                        .into_iter()
                        .map(|working_copy| {
                            let (tracker, backups, token) = (task_tracker.clone(), task_backups.clone(), token.clone());

                            async_fn(async move {
                                // Backup exists
                                let content_version = tracker.get_content_version(&working_copy);
                                let identifier = WorkingCopyIdentifier::of(&*working_copy);
                                if tracker.working_copy_backup_service().has_backup_sync(&identifier, Some(content_version), None) {
                                    backups.borrow_mut().push(working_copy);
                                }
                                // Backup does not exist
                                else {
                                    let backup = working_copy.backup(&token).await?;
                                    if token.is_cancellation_requested() {
                                        return Ok(());
                                    }

                                    tracker.working_copy_backup_service().backup(&identifier, backup.content, Some(content_version), backup.meta, Some(token.clone())).await?;
                                    if token.is_cancellation_requested() {
                                        return Ok(());
                                    }

                                    backups.borrow_mut().push(working_copy);
                                }

                                Ok(())
                            })
                        })
                        .collect();

                    if let Err(backup_error) = promises::settled(backup_operations).await {
                        *task_error.borrow_mut() = Some(backup_error);
                    }

                    Ok(())
                })
            },
            localize("backupBeforeShutdownMessage", "Backing up editors with unsaved changes is taking a bit longer...", &[]),
            Some(localize("backupBeforeShutdownDetail", "Click 'Cancel' to stop waiting and to save or revert editors with unsaved changes.", &[])),
            ProgressLocation::Dialog,
        )
        .await?;

        let backups = backups.borrow().clone();
        let error = error.borrow().clone();

        Ok(BackupResult { backups, error })
    }

    async fn confirm_before_shutdown(&self, tracker: &Rc<WorkingCopyBackupTracker>, modified_working_copies: &[Rc<dyn IWorkingCopy>]) -> Result<bool, Error> {
        // Save
        let names: Vec<FileNameOrResource> = modified_working_copies.iter().map(|working_copy| FileNameOrResource::Name(working_copy.name())).collect();
        let confirm = self.file_dialog_service.show_save_confirm(&names).await?;
        match confirm {
            ConfirmResult::Save => {
                let modified_count_before_save = tracker.working_copy_service().modified_count();

                if let Err(error) = self.do_save_all_before_shutdown(tracker, modified_working_copies.to_vec(), SaveReason::Explicit).await {
                    tracker.log_service().error(&format!("[backup tracker] error saving modified working copies: {error}")); // guard against misbehaving saves, we handle remaining modified below
                }

                let saved_working_copies = modified_count_before_save as i64 - tracker.working_copy_service().modified_count() as i64;
                if saved_working_copies < modified_working_copies.len() as i64 {
                    return Ok(true); // veto (save failed or was canceled)
                }

                self.no_veto(tracker, BackupsToDiscard::of_working_copies(modified_working_copies)).await // no veto (modified saved)
            }

            // Don't Save
            ConfirmResult::DontSave => {
                if let Err(error) = self.do_revert_all_before_shutdown(tracker, modified_working_copies.to_vec()).await {
                    tracker.log_service().error(&format!("[backup tracker] error reverting modified working copies: {error}")); // do not block the shutdown on errors from revert
                }

                self.no_veto(tracker, BackupsToDiscard::of_working_copies(modified_working_copies)).await // no veto (modified reverted)
            }

            // Cancel
            ConfirmResult::Cancel => Ok(true), // veto (user canceled)
        }
    }

    fn do_save_all_before_shutdown(&self, tracker: &Rc<WorkingCopyBackupTracker>, working_copies: Vec<Rc<dyn IWorkingCopy>>, reason: SaveReason) -> Promise<()> {
        // Do not pick `Dialog` as location for reporting progress if it is likely
        // that the save operation will itself open a dialog for asking for the
        // location to save to for untitled or scratchpad working copies.
        // https://github.com/microsoft/vscode-internalbacklog/issues/4943
        let location = if working_copies
            .iter()
            .any(|working_copy| working_copy.capabilities().contains(WorkingCopyCapabilities::UNTITLED) || working_copy.capabilities().contains(WorkingCopyCapabilities::SCRATCHPAD))
        {
            ProgressLocation::Window
        } else {
            ProgressLocation::Dialog
        };

        let tracker = tracker.clone();
        self.with_progress_and_cancellation(
            move |_token| {
                async_fn(async move {
                    // Skip save participants on shutdown for performance reasons
                    let save_options = SaveOptions { skip_save_participants: Some(true), reason: Some(reason), ..SaveOptions::default() };

                    // First save through the editor service if we save all to benefit
                    // from some extras like switching to untitled modified editors before saving.
                    let mut result: Option<bool> = None;
                    if working_copies.len() == tracker.working_copy_service().modified_count() {
                        result = Some(
                            tracker
                                .editor_service()
                                .save_all(Some(SaveAllEditorsOptions {
                                    save: SaveEditorsOptions { base: save_options.clone(), save_as: None },
                                    include_untitled: Some(IncludeUntitled::Options { include_scratchpad: true }),
                                    exclude_sticky: None,
                                }))
                                .await?
                                .success,
                        );
                    }

                    // If we still have modified working copies, save those directly
                    // unless the save was not successful (e.g. cancelled)
                    if result != Some(false) {
                        promises::settled(
                            working_copies
                                .iter()
                                .map(|working_copy| if working_copy.is_modified() { working_copy.save(Some(save_options.clone())) } else { Promise::resolved(true) })
                                .collect(),
                        )
                        .await?;
                    }

                    Ok(())
                })
            },
            localize("saveBeforeShutdown", "Saving editors with unsaved changes is taking a bit longer...", &[]),
            None,
            location,
        )
    }

    fn do_revert_all_before_shutdown(&self, tracker: &Rc<WorkingCopyBackupTracker>, modified_working_copies: Vec<Rc<dyn IWorkingCopy>>) -> Promise<()> {
        let tracker = tracker.clone();
        self.with_progress_and_cancellation(
            move |_token| {
                async_fn(async move {
                    // Soft revert is good enough on shutdown
                    let revert_options = RevertOptions { soft: Some(true), ..RevertOptions::default() };

                    // First revert through the editor service if we revert all
                    if modified_working_copies.len() == tracker.working_copy_service().modified_count() {
                        tracker.editor_service().revert_all(Some(RevertAllEditorsOptions { revert: revert_options, ..RevertAllEditorsOptions::default() })).await?;
                    }

                    // If we still have modified working copies, revert those directly
                    promises::settled(
                        modified_working_copies
                            .iter()
                            .map(|working_copy| if working_copy.is_modified() { working_copy.revert(Some(revert_options)) } else { Promise::resolved(()) })
                            .collect(),
                    )
                    .await?;

                    Ok(())
                })
            },
            localize("revertBeforeShutdown", "Reverting editors with unsaved changes is taking a bit longer...", &[]),
            None,
            ProgressLocation::Dialog,
        )
    }

    async fn on_before_shutdown_without_modified(&self, tracker: &Rc<WorkingCopyBackupTracker>) -> Result<bool, Error> {
        // We are about to shutdown without modified editors
        // and will discard any backups that are still
        // around that have not been handled depending
        // on the window state.
        //
        // Empty window: discard even unrestored backups to
        // prevent empty windows from restoring that cannot
        // be closed (workaround for not having implemented
        // https://github.com/microsoft/vscode/issues/127163
        // and a fix for what users have reported in issue
        // https://github.com/microsoft/vscode/issues/126725)
        //
        // Workspace/Folder window: do not discard unrestored
        // backups to give a chance to restore them in the
        // future. Since we do not restore workspace/folder
        // windows with backups, this is fine.

        let except = if self.context_service.get_workbench_state() == WorkbenchState::Empty { Vec::new() } else { tracker.unrestored_backups() };

        self.no_veto(tracker, BackupsToDiscard::Keep { except }).await
    }

    async fn no_veto(&self, tracker: &Rc<WorkingCopyBackupTracker>, backups_to_discard_or_keep: BackupsToDiscard) -> Result<bool, Error> {
        // Discard backups from working copies the
        // user either saved or reverted

        self.discard_backups_before_shutdown(tracker, backups_to_discard_or_keep).await?;

        Ok(false) // no veto (no modified)
    }

    async fn discard_backups_before_shutdown(&self, tracker: &Rc<WorkingCopyBackupTracker>, backups_to_discard_or_keep: BackupsToDiscard) -> Result<(), Error> {
        // We never discard any backups before we are ready
        // and have resolved all backups that exist. This
        // is important to not loose backups that have not
        // been handled.

        if !tracker.is_ready() {
            return Ok(());
        }

        let tracker = tracker.clone();
        self.with_progress_and_cancellation(
            move |_token| {
                async_fn(async move {
                    // When we shutdown either with no modified working copies left
                    // or with some handled, we start to discard these backups
                    // to free them up. This helps to get rid of stale backups
                    // as reported in https://github.com/microsoft/vscode/issues/92962
                    //
                    // However, we never want to discard backups that we know
                    // were not restored in the session.

                    let working_copy_backup_service = tracker.working_copy_backup_service().clone();
                    let result = match backups_to_discard_or_keep {
                        BackupsToDiscard::Discard(backups_to_discard) => {
                            promises::settled(backups_to_discard.iter().map(|working_copy| working_copy_backup_service.discard_backup(working_copy, None)).collect()).await.map(|_| ())
                        }
                        BackupsToDiscard::Keep { except } => working_copy_backup_service.discard_backups(Some(DiscardBackupsFilter { except })).await,
                    };

                    if let Err(error) = result {
                        tracker.log_service().error(&format!("[backup tracker] error discarding backups: {error}"));
                    }

                    Ok(())
                })
            },
            localize("discardBackupsBeforeShutdown", "Discarding backups is taking a bit longer...", &[]),
            None,
            ProgressLocation::Dialog,
        )
        .await
    }

    fn with_progress_and_cancellation(
        &self,
        promise_factory: impl FnOnce(CancellationToken) -> Promise<()> + 'static,
        title: String,
        detail: Option<String>,
        location: ProgressLocation,
    ) -> Promise<()> {
        let cts = CancellationTokenSource::new(None);
        let token = cts.token();
        let cancel_cts = cts.clone();

        self.progress_service.with_progress(
            ProgressOptions {
                title: Some(title.into()),
                detail,
                cancellable: Some(ProgressCancellable::Flag(true)), // allow to cancel (https://github.com/microsoft/vscode/issues/112278)
                delay: Some(800.0),                                  // delay so that it only appears when operation takes a long time
                ..ProgressOptions::new(location) // by default use a dialog to prevent the user from making any more changes now (https://github.com/microsoft/vscode/issues/122774)
            },
            Box::new(move |_progress| {
                let token_for_race = token.clone();
                race_cancellation(&promise_factory(token), &token_for_race).then(|_| ())
            }),
            Some(Rc::new(move |_choice| cancel_cts.dispose_with(true))),
        )
    }
}

impl WorkingCopyBackupTrackerHooks for NativeWorkingCopyBackupTrackerHooks {
    fn on_final_before_shutdown(&self, tracker: &WorkingCopyBackupTracker, reason: ShutdownReason) -> MaybePromise<bool> {
        // Important: we are about to shutdown and handle modified working copies
        // and backups. We do not want any pending backup ops to interfer with
        // this because there is a risk of a backup being scheduled after we have
        // acknowledged to shutdown and then might end up with partial backups
        // written to disk, or even empty backups or deletes after writes.
        // (https://github.com/microsoft/vscode/issues/138055)

        tracker.cancel_backup_operations();

        // For the duration of the shutdown handling, suspend backup operations
        // and only resume after we have handled backups. Similar to above, we
        // do not want to trigger backup tracking during our shutdown handling
        // but we must resume, in case of a veto afterwards.

        let resume = tracker.suspend_backup_operations();

        let this = self.clone();
        let tracker = tracker.rc();

        MaybePromise::Promise(async_fn(async move {
            let result = this.final_before_shutdown(&tracker, reason).await;

            resume.resume(); // `finally`

            result
        }))
    }
}

/// `NativeWorkingCopyBackupTracker`.
pub struct NativeWorkingCopyBackupTracker {
    tracker: Rc<WorkingCopyBackupTracker>,
}

impl NativeWorkingCopyBackupTracker {
    pub const ID: &'static str = "workbench.contrib.nativeWorkingCopyBackupTracker";

    /// `constructor(workingCopyBackupService, filesConfigurationService, workingCopyService, lifecycleService,
    /// fileDialogService, dialogService, contextService, nativeHostService, logService, environmentService, progressService,
    /// workingCopyEditorService, editorService)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
        files_configuration_service: Rc<dyn IFilesConfigurationService>,
        working_copy_service: Rc<dyn IWorkingCopyService>,
        lifecycle_service: Rc<dyn ILifecycleService>,
        file_dialog_service: Rc<dyn IFileDialogService>,
        dialog_service: Rc<dyn IDialogService>,
        context_service: Rc<dyn IWorkspaceContextService>,
        native_host_service: Rc<dyn INativeHostService>,
        log_service: LogServiceRef,
        environment_service: Rc<dyn EnvironmentService>,
        progress_service: Rc<dyn IProgressService>,
        working_copy_editor_service: Rc<dyn IWorkingCopyEditorService>,
        editor_service: Rc<dyn IEditorService>,
    ) -> Rc<Self> {
        let hooks = NativeWorkingCopyBackupTrackerHooks::new(file_dialog_service, dialog_service, context_service, native_host_service, environment_service, progress_service);

        Rc::new(Self {
            tracker: WorkingCopyBackupTracker::new(
                working_copy_backup_service,
                working_copy_service,
                log_service,
                lifecycle_service,
                files_configuration_service,
                working_copy_editor_service,
                editor_service,
                Rc::new(hooks),
            ),
        })
    }

    /// O [`WorkingCopyBackupTracker`] que esta classe estende.
    pub fn tracker(&self) -> &Rc<WorkingCopyBackupTracker> {
        &self.tracker
    }
}

impl IDisposable for NativeWorkingCopyBackupTracker {
    fn dispose(&self) {
        self.tracker.dispose();
    }
}
