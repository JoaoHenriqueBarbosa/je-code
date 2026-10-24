//! Porte de `src/vs/workbench/services/workingCopy/electron-browser/workingCopyBackupService.ts`: o serviço de backup
//! das working copies do desktop, que escolhe a pasta de backup da janela e prolonga o desligamento enquanto houver
//! operações de backup pendentes.
//!
//! Divergências:
//! - **Herança.** `NativeWorkingCopyBackupService extends WorkingCopyBackupService` é a composição: a struct guarda o
//!   [`WorkingCopyBackupService`] e o expõe por `Deref` (e por [`NativeWorkingCopyBackupService::service`], para entregar
//!   o `Rc<dyn IWorkingCopyBackupService>` a quem o consome).
//! - **`INativeWorkbenchEnvironmentService`.** O serviço de ambiente nativo do workbench não foi portado: o construtor
//!   recebe os dois valores que o original lê dele, `backupPath` (`Option<&str>`) e `userRoamingDataHome`.
//! - **`@IFileService` e `@ILogService`.** O [`FileService`] concreto e o [`LogServiceRef`] (ver
//!   `working_copy_backup_service.rs`).
//! - **Registro.** `registerSingleton(IWorkingCopyBackupService, NativeWorkingCopyBackupService, Eager)` e
//!   `registerWorkbenchContribution2(NativeWorkingCopyBackupTracker.ID, ...)` ficam para a montagem do workbench: o
//!   `FileService` concreto não é um serviço injetável (o `IFileService` reduzido não traz o que este serviço usa) e o
//!   `NativeWorkingCopyBackupTracker` (`workingCopyBackupTracker.ts`) não foi portado.
//! - **`localize`.** A chave e a mensagem são as do original; o `localize` do porte devolve a mensagem em inglês.

use std::ops::Deref;
use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::uri::{Field, URI, UriChange};
use je_platform::files::file_service::FileService;
use je_platform::log::LogServiceRef;
use je_platform::resources::uri_file;

use super::working_copy_backup::IWorkingCopyBackupService;
use super::working_copy_backup_service::WorkingCopyBackupService;
use crate::services::lifecycle::lifecycle::{ILifecycleService, WillShutdownEventJoiner};

/// `NativeWorkingCopyBackupService`.
pub struct NativeWorkingCopyBackupService {
    base: Rc<Disposable>,
    service: Rc<WorkingCopyBackupService>,
}

impl NativeWorkingCopyBackupService {
    /// `constructor(environmentService, fileService, logService, lifecycleService)`: `backup_path` é o
    /// `environmentService.backupPath` e `user_roaming_data_home` o `environmentService.userRoamingDataHome`.
    pub fn new(
        backup_path: Option<&str>,
        user_roaming_data_home: &URI,
        file_service: Rc<FileService>,
        log_service: LogServiceRef,
        lifecycle_service: Rc<dyn ILifecycleService>,
    ) -> Rc<Self> {
        let backup_workspace_home = backup_path
            .filter(|backup_path| !backup_path.is_empty())
            .map(|backup_path| uri_file(backup_path).with(&UriChange { scheme: Field::Set(user_roaming_data_home.scheme().clone()), ..UriChange::default() }));
        let service = WorkingCopyBackupService::new(backup_workspace_home, file_service, log_service);

        let base = Disposable::new();
        base.register(service.clone());

        let this = Rc::new(Self { base, service });
        this.register_listeners(&lifecycle_service);

        this
    }

    /// O [`WorkingCopyBackupService`] que esta classe estende, como o contrato `IWorkingCopyBackupService`.
    pub fn service(&self) -> Rc<dyn IWorkingCopyBackupService> {
        self.service.clone()
    }

    fn register_listeners(&self, lifecycle_service: &Rc<dyn ILifecycleService>) {
        // Lifecycle: ensure to prolong the shutdown for as long
        // as pending backup operations have not finished yet.
        // Otherwise, we risk writing partial backups to disk.
        let service = self.service.clone();
        self.base.register(lifecycle_service.on_will_shutdown().listen(move |event| {
            event.join(
                service.join_backups(),
                WillShutdownEventJoiner {
                    id: "join.workingCopyBackups".to_string(),
                    label: localize("join.workingCopyBackups", "Backup working copies", &[]),
                    order: None,
                },
            )
        }));
    }
}

impl Deref for NativeWorkingCopyBackupService {
    type Target = WorkingCopyBackupService;

    fn deref(&self) -> &WorkingCopyBackupService {
        &self.service
    }
}

impl IDisposable for NativeWorkingCopyBackupService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
