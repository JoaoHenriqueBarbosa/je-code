//! Porte de `src/vs/workbench/services/workingCopy/common/workingCopyFileOperationParticipant.ts`: o
//! `WorkingCopyFileOperationParticipant`, que guarda os participantes das operações de arquivo de working copies e os
//! roda em ordem de registro. O contrato `IWorkingCopyFileOperationParticipant` é o de `textfile/collaborators.rs`, junto
//! com o resto do contrato do `IWorkingCopyFileService` (declarado no original em `workingCopyFileService.ts`).
//!
//! Divergências:
//! - **`LinkedList`** é um `Vec` percorrido por índice (um participante registrado durante a participação ainda é
//!   visitado, como na lista ligada do original); remover um participante é o `toDisposable` que o tira do `Vec` por
//!   identidade (`Rc::ptr_eq`).
//! - **`Disposable`** é o `Disposable` base em `base`.
//! - **`timeout`** é um `f64`: o `getValue<number>('files.participants.timeout')` do original é um `number` de JavaScript.
//!   Um valor que não é número, ou que é zero ou negativo, desliga os participantes.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::FileOperation;
use je_platform::log::LogServiceRef;

use crate::services::textfile::collaborators::{FileOperationUndoRedoInfo, IWorkingCopyFileOperationParticipant, WorkingCopyFileEventFile};

/// `WorkingCopyFileOperationParticipant`.
pub struct WorkingCopyFileOperationParticipant {
    base: Rc<Disposable>,
    participants: Rc<RefCell<Vec<Rc<dyn IWorkingCopyFileOperationParticipant>>>>,
    log_service: LogServiceRef,
    configuration_service: Rc<dyn IConfigurationService>,
}

impl WorkingCopyFileOperationParticipant {
    /// `constructor(@ILogService logService, @IConfigurationService configurationService)`.
    pub fn new(log_service: LogServiceRef, configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), participants: Rc::new(RefCell::new(Vec::new())), log_service, configuration_service })
    }

    /// `addFileOperationParticipant(participant)`.
    pub fn add_file_operation_participant(&self, participant: Rc<dyn IWorkingCopyFileOperationParticipant>) -> DisposableRef {
        self.participants.borrow_mut().push(participant.clone());

        let participants = self.participants.clone();
        to_disposable(move || {
            let mut participants = participants.borrow_mut();
            if let Some(index) = participants.iter().position(|candidate| Rc::ptr_eq(candidate, &participant)) {
                participants.remove(index);
            }
        })
    }

    /// `participate(files, operation, undoInfo, token)`.
    pub fn participate(&self, files: Vec<WorkingCopyFileEventFile>, operation: FileOperation, undo_info: Option<FileOperationUndoRedoInfo>, token: CancellationToken) -> Promise<()> {
        let timeout = self.configuration_service.get_value(Some("files.participants.timeout"), None);
        let (participants, log_service) = (self.participants.clone(), self.log_service.clone());

        async_fn(async move {
            let timeout = match timeout {
                Dynamic::Number(timeout) if !(timeout <= 0.0) => timeout,
                _ => return Ok(()), // disabled
            };

            // For each participant
            let mut index = 0;
            loop {
                let participant = participants.borrow().get(index).cloned();
                let Some(participant) = participant else {
                    break;
                };
                index += 1;

                if let Err(err) = participant.participate(files.clone(), operation, undo_info, timeout, token.clone()).await {
                    log_service.warn(&err.to_string());
                }
            }

            Ok(())
        })
    }
}

impl IDisposable for WorkingCopyFileOperationParticipant {
    fn dispose(&self) {
        self.participants.borrow_mut().clear();

        self.base.dispose();
    }
}
