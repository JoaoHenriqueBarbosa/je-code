//! Porte de `src/vs/workbench/services/textfile/common/textFileSaveParticipant.ts`: o `TextFileSaveParticipant`, que
//! guarda os participantes do salvar de modelos de arquivo de texto e os roda em ordem.
//!
//! Divergências:
//! - **`LinkedList`** é um `Vec` ordenado na hora de participar (a ordem de inserção dentro do mesmo `ordinal` é mantida:
//!   a ordenação é estável, como a do `Array.prototype.sort`); remover um participante é o `toDisposable` que o tira
//!   do `Vec` por identidade (`Rc::ptr_eq`).
//! - **`Disposable`** é o `Disposable` base em `base`.
//! - **`raceCancellation(promise, token)`** é [`race_cancellation`]: `None` quando o token cancelou primeiro (o original
//!   devolve `undefined` e segue).
//! - **`withProgress`** recebe a tarefa como fecho `FnOnce`; o `bubbleCancel` é um `Rc<Cell<bool>>` compartilhado com ela.
//! - **`model.textEditorModel?.pushStackElement()`** lê o modelo de texto por [`ITextFileEditorModel::text_editor_model`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::{async_fn, race_cancellation};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::log::LogService;
use je_platform::notification::notification::NotificationPriority;
use je_platform::notification::notification_telemetry::NotificationTelemetryId;

use crate::services::textfile::collaborators::{IProgress, IProgressService, ProgressCancellable, ProgressLocation, ProgressOptions, ProgressStep};
use crate::services::textfile::textfiles::{ITextFileEditorModel, ITextFileSaveParticipant, TextFileSaveParticipantContext};

/// `TextFileSaveParticipant`.
pub struct TextFileSaveParticipant {
    base: Rc<Disposable>,
    save_participants: Rc<RefCell<Vec<Rc<dyn ITextFileSaveParticipant>>>>,
    log_service: Rc<dyn LogService>,
    progress_service: Rc<dyn IProgressService>,
}

impl TextFileSaveParticipant {
    /// `constructor(@ILogService logService, @IProgressService progressService)`.
    pub fn new(log_service: Rc<dyn LogService>, progress_service: Rc<dyn IProgressService>) -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), save_participants: Rc::new(RefCell::new(Vec::new())), log_service, progress_service })
    }

    /// `addSaveParticipant(participant)`.
    pub fn add_save_participant(&self, participant: Rc<dyn ITextFileSaveParticipant>) -> DisposableRef {
        self.save_participants.borrow_mut().push(participant.clone());

        let save_participants = self.save_participants.clone();
        to_disposable(move || {
            let mut participants = save_participants.borrow_mut();
            if let Some(index) = participants.iter().position(|candidate| Rc::ptr_eq(candidate, &participant)) {
                participants.remove(index);
            }
        })
    }

    /// `participate(model, context, progress, token)`.
    pub fn participate(
        &self,
        model: Rc<dyn ITextFileEditorModel>,
        context: TextFileSaveParticipantContext,
        progress: Rc<dyn IProgress<ProgressStep>>,
        token: CancellationToken,
    ) -> Promise<()> {
        let cts = CancellationTokenSource::new(Some(&token));

        // undoStop before participation
        if let Some(text_editor_model) = model.text_editor_model() {
            text_editor_model.push_stack_element();
        }

        // report to the "outer" progress
        progress.report(ProgressStep { message: Some(localize("saveParticipants1", "Running Code Actions and Formatters...", &[])), ..ProgressStep::default() });

        let bubble_cancel = Rc::new(Cell::new(false));

        let (save_participants, log_service, progress_service) = (self.save_participants.clone(), self.log_service.clone(), self.progress_service.clone());

        async_fn(async move {
            // create an "inner" progress to allow to skip over long running save participants
            let (task_model, task_cts, task_bubble_cancel) = (model.clone(), cts.clone(), bubble_cancel.clone());
            let cancel_cts = cts.clone();
            progress_service
                .with_progress(
                    ProgressOptions {
                        location: ProgressLocation::Notification,
                        title: None,
                        cancellable: Some(ProgressCancellable::Label(localize("skip", "Skip", &[]))),
                        delay: Some(if model.is_dirty() { 5000 } else { 3000 }),
                        priority: Some(NotificationPriority::Urgent),
                        telemetry: Some(NotificationTelemetryId::FileSaveParticipants),
                    },
                    Box::new(move |progress| {
                        async_fn(async move {
                            let mut participants: Vec<Rc<dyn ITextFileSaveParticipant>> = save_participants.borrow().clone();
                            participants.sort_by_key(|participant| participant.ordinal().unwrap_or(0));

                            for save_participant in participants {
                                if task_cts.token().is_cancellation_requested() || task_model.text_editor_model().is_none() /* disposed */ {
                                    break;
                                }

                                let promise = save_participant.participate(task_model.clone(), context.clone(), progress.clone(), task_cts.token());
                                if let Err(err) = race_cancellation(&promise, &task_cts.token()).await {
                                    if !errors::is_cancellation_error(&err) {
                                        log_service.error(&err.to_string());
                                    } else if !task_cts.token().is_cancellation_requested() {
                                        // we see a cancellation error BUT the token didn't signal it
                                        // this means the participant wants the save operation to be cancelled
                                        task_cts.cancel();
                                        task_bubble_cancel.set(true);
                                    }
                                }
                            }
                            Ok(())
                        })
                    }),
                    Some(Rc::new(move || {
                        cancel_cts.cancel();
                    })),
                )
                .await?;

            // undoStop after participation
            if let Some(text_editor_model) = model.text_editor_model() {
                text_editor_model.push_stack_element();
            }

            cts.dispose_with(false);

            if bubble_cancel.get() {
                return Err(Error::cancellation());
            }
            Ok(())
        })
    }
}

impl IDisposable for TextFileSaveParticipant {
    fn dispose(&self) {
        self.save_participants.borrow_mut().clear();

        self.base.dispose();
    }
}
