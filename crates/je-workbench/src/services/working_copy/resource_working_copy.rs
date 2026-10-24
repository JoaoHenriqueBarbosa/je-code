//! Porte de `src/vs/workbench/services/workingCopy/common/resourceWorkingCopy.ts`: a working copy que é apoiada por um
//! `URI` de um provider de arquivos conhecido, com o rastreio de órfão (o arquivo foi apagado do disco) e o descarte.
//!
//! Divergências:
//! - **Classe abstrata.** `ResourceWorkingCopy` é a struct [`ResourceWorkingCopy`], que a working copy concreta guarda
//!   num campo (composição no lugar da herança) e devolve em [`IResourceWorkingCopy::resource_working_copy`]. Os métodos
//!   do contrato `IResourceWorkingCopy` (`isOrphaned`, `isDisposed`, `onDidChangeOrphaned`, `onWillDispose`) são
//!   implementados uma vez, como métodos padrão do trait, a partir dessa struct. Os membros abstratos (`typeId`, `name`,
//!   `capabilities`, os eventos de sujeira e conteúdo, `isDirty`, `backup`, `save` e `revert`) são os do [`IWorkingCopy`],
//!   que a concreta implementa; o `isModified` do original (`isDirty()`) também fica por conta dela.
//! - **Membros protegidos.** `fileService` é [`ResourceWorkingCopy::file_service`] e `setOrphaned` é
//!   [`ResourceWorkingCopy::set_orphaned`], públicos porque o original os expõe às subclasses.
//! - **`dispose()` sobrescrito.** O descarte da concreta chama [`IDisposable::dispose`] da struct (o `super.dispose()`).
//! - **`IFileService`** é o contrato reduzido de `je_platform::files::i_file_service`.
//! - **`onDidFilesChange`** é `async` e ninguém espera a promessa dele no original: aqui é uma promessa que o ouvinte
//!   descarta.

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::{async_fn, timeout};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::files::{FileChangeType, FileChangesEvent};
use je_platform::files::i_file_service::IFileService;

use super::working_copy::IWorkingCopy;

/// `IResourceWorkingCopy`: uma working copy apoiada por um `URI` de um provider de arquivos conhecido.
pub trait IResourceWorkingCopy: IWorkingCopy + IDisposable {
    /// A parte `ResourceWorkingCopy` da working copy (o que a classe abstrata do original dá às subclasses).
    fn resource_working_copy(&self) -> &ResourceWorkingCopy;

    /// An event for when the orphaned state of the resource working copy changes.
    fn on_did_change_orphaned(&self) -> Event<()> {
        self.resource_working_copy().on_did_change_orphaned.event()
    }

    /// Whether the resource working copy is orphaned or not.
    fn is_orphaned(&self) -> bool {
        self.resource_working_copy().orphaned.get()
    }

    /// An event for when the file working copy has been disposed.
    fn on_will_dispose(&self) -> Event<()> {
        self.resource_working_copy().on_will_dispose.event()
    }

    /// Whether the file working copy has been disposed or not.
    fn is_disposed(&self) -> bool {
        self.resource_working_copy().disposed.get()
    }
}

/// `ResourceWorkingCopy`.
pub struct ResourceWorkingCopy {
    base: Rc<Disposable>,
    resource: URI,
    file_service: Rc<dyn IFileService>,
    on_did_change_orphaned: Rc<Emitter<()>>,
    orphaned: Cell<bool>,
    on_will_dispose: Rc<Emitter<()>>,
    disposed: Cell<bool>,
}

impl ResourceWorkingCopy {
    /// `constructor(resource, @IFileService fileService)`.
    pub fn new(resource: URI, file_service: Rc<dyn IFileService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_orphaned = Emitter::new();
        let on_will_dispose = Emitter::new();
        base.register(on_did_change_orphaned.clone());
        base.register(on_will_dispose.clone());

        let this = Rc::new(Self {
            base,
            resource,
            file_service,
            on_did_change_orphaned,
            orphaned: Cell::new(false),
            on_will_dispose,
            disposed: Cell::new(false),
        });

        let weak = Rc::downgrade(&this);
        this.base.register(this.file_service.on_did_files_change().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                let _ = this.on_did_files_change(e);
            }
        }));

        this
    }

    /// `resource`.
    pub fn resource(&self) -> &URI {
        &self.resource
    }

    /// `fileService` (protegido no original).
    pub fn file_service(&self) -> &Rc<dyn IFileService> {
        &self.file_service
    }

    //#region Orphaned Tracking

    fn on_did_files_change(self: &Rc<Self>, e: &Rc<FileChangesEvent>) -> Promise<()> {
        let (this, e) = (self.clone(), e.clone());
        async_fn(async move {
            let mut file_event_impacts_us = false;
            let mut new_in_orphan_mode_guess: Option<bool> = None;

            // If we are currently orphaned, we check if the file was added back
            if this.orphaned.get() {
                let file_working_copy_resource_added = e.contains(&this.resource, &[FileChangeType::Added]);
                if file_working_copy_resource_added {
                    new_in_orphan_mode_guess = Some(false);
                    file_event_impacts_us = true;
                }
            }
            // Otherwise we check if the file was deleted
            else {
                let file_working_copy_resource_deleted = e.contains(&this.resource, &[FileChangeType::Deleted]);
                if file_working_copy_resource_deleted {
                    new_in_orphan_mode_guess = Some(true);
                    file_event_impacts_us = true;
                }
            }

            if file_event_impacts_us && Some(this.orphaned.get()) != new_in_orphan_mode_guess {
                let mut new_in_orphan_mode_validated = false;
                if new_in_orphan_mode_guess == Some(true) {
                    // We have received reports of users seeing delete events even though the file still
                    // exists (network shares issue: https://github.com/microsoft/vscode/issues/13665).
                    // Since we do not want to mark the working copy as orphaned, we have to check if the
                    // file is really gone and not just a faulty file event.
                    timeout(100).promise.await?;

                    if this.disposed.get() {
                        new_in_orphan_mode_validated = true;
                    } else {
                        let exists = this.file_service.exists(&this.resource).await?;
                        new_in_orphan_mode_validated = !exists;
                    }
                }

                if this.orphaned.get() != new_in_orphan_mode_validated && !this.disposed.get() {
                    this.set_orphaned(new_in_orphan_mode_validated);
                }
            }

            Ok(())
        })
    }

    /// `setOrphaned(orphaned)` (protegido no original).
    pub fn set_orphaned(&self, orphaned: bool) {
        if self.orphaned.get() != orphaned {
            self.orphaned.set(orphaned);

            self.on_did_change_orphaned.fire(());
        }
    }

    //#endregion
}

impl IDisposable for ResourceWorkingCopy {
    //#region Dispose

    fn dispose(&self) {
        // State
        self.orphaned.set(false);

        // Event
        self.on_will_dispose.fire(());

        // super.dispose()
        self.disposed.set(true);
        self.base.dispose();
    }

    //#endregion
}
