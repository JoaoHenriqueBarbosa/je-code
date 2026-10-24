//! Porte de `src/vs/workbench/services/workingCopy/common/abstractFileWorkingCopyManager.ts`: o contrato
//! `IBaseFileWorkingCopyManager` e a classe abstrata `BaseFileWorkingCopyManager`, que guarda as working copies de arquivo
//! por recurso, avisa quando uma é criada e tira do cache a que for descartada.
//!
//! Divergências:
//! - **Classe abstrata.** `BaseFileWorkingCopyManager` é a struct [`BaseFileWorkingCopyManager`], que o gerenciador
//!   concreto guarda num campo (composição no lugar da herança) e expõe por [`IBaseFileWorkingCopyManager::base_manager`];
//!   `onDidCreate`, `workingCopies` e `get` são métodos padrão do trait a partir dela. `IBaseFileWorkingCopyManager<M, W>`
//!   é o trait [`IBaseFileWorkingCopyManager`], genérico só na working copy (`W`): o modelo (`M`) é parâmetro de tipo para
//!   a restrição `W extends IFileWorkingCopy<M>`, que aqui é o limite `W: IFileWorkingCopy`.
//! - **Métodos virtuais.** O `remove` é `protected` e as subclasses o sobrescrevem; como o `onWillDispose` da working copy
//!   chama `this.remove(...)` por despacho virtual, a subclasse instala a sobrescrita por
//!   [`BaseFileWorkingCopyManager::set_remove_override`] (que a classe concreta chama de volta no `super.remove`, que é
//!   [`BaseFileWorkingCopyManager::remove_base`]). O `destroy` termina com `this.dispose()`, também virtual: o fecho
//!   `dispose_manager` de [`BaseFileWorkingCopyManager::destroy`] é o `dispose` da subclasse.
//! - **`onWillDispose` da working copy.** Só o [`IResourceWorkingCopy`](super::resource_working_copy::IResourceWorkingCopy)
//!   o declara (ver `file_working_copy.rs`): o gerenciador recebe como função, no construtor, como ler o evento de cada `W`.
//! - **`dispose(iterable)`** é um laço sobre uma cópia dos valores: descartar uma working copy tira o recurso do mapa pelo
//!   `onWillDispose`.
//! - **Erros do `saveWithFallback`.** `catch (error) { // Ignore }` é `let _ =`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, promises};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::map::ResourceMap;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::WriteFileOptions;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::LogService;

use crate::services::working_copy::file_working_copy::IFileWorkingCopy;
use crate::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyIdentifier};
use crate::services::working_copy::working_copy_backup::IWorkingCopyBackupService;

/// `IBaseFileWorkingCopyManager<M, W>`.
pub trait IBaseFileWorkingCopyManager<W: IFileWorkingCopy + 'static>: IDisposable {
    /// A parte `BaseFileWorkingCopyManager` do gerenciador (o que a classe abstrata do original dá às subclasses).
    fn base_manager(&self) -> &BaseFileWorkingCopyManager<W>;

    /// An event for when a file working copy was created.
    fn on_did_create(&self) -> Event<Rc<W>> {
        self.base_manager().on_did_create()
    }

    /// Access to all known file working copies within the manager.
    fn working_copies(&self) -> Vec<Rc<W>> {
        self.base_manager().working_copies()
    }

    /// Returns the file working copy for the provided resource or `None` if none.
    fn get(&self, resource: &URI) -> Option<Rc<W>> {
        self.base_manager().get(resource)
    }

    /// Disposes all working copies of the manager and disposes the manager. This method is different from `dispose` in
    /// that it will unregister any working copy from the `IWorkingCopyService`. Since this impact things like backups,
    /// the method is `async` because it needs to trigger `save` for any dirty working copy to preserve the data.
    ///
    /// Callers should make sure to e.g. close any editors associated with the working copy.
    fn destroy(&self) -> Promise<()>;
}

/// Como ler o `onWillDispose` de uma working copy `W`.
pub type WillDisposeOf<W> = Rc<dyn Fn(&W) -> Event<()>>;

/// A sobrescrita de `remove(resource)` de uma subclasse.
type RemoveOverride = Rc<dyn Fn(&URI) -> bool>;

/// `BaseFileWorkingCopyManager<M, W>`.
pub struct BaseFileWorkingCopyManager<W: IFileWorkingCopy + 'static> {
    this: Weak<BaseFileWorkingCopyManager<W>>,
    /// O `Disposable` de que a classe herda.
    base: Rc<Disposable>,

    on_did_create: Rc<Emitter<Rc<W>>>,

    map_resource_to_working_copy: RefCell<ResourceMap<Rc<W>>>,
    map_resource_to_dispose_listener: RefCell<ResourceMap<DisposableRef>>,

    will_dispose_of: WillDisposeOf<W>,
    remove_override: RefCell<Option<RemoveOverride>>,

    file_service: Rc<dyn IFileService>,
    log_service: Rc<dyn LogService>,
    working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
}

impl<W: IFileWorkingCopy + 'static> BaseFileWorkingCopyManager<W> {
    /// `constructor(@IFileService fileService, @ILogService logService, @IWorkingCopyBackupService workingCopyBackupService)`.
    pub fn new(
        will_dispose_of: WillDisposeOf<W>,
        file_service: Rc<dyn IFileService>,
        log_service: Rc<dyn LogService>,
        working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_create: Rc<Emitter<Rc<W>>> = Emitter::new();
        base.register(on_did_create.clone());

        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_create,
            map_resource_to_working_copy: RefCell::new(ResourceMap::new()),
            map_resource_to_dispose_listener: RefCell::new(ResourceMap::new()),
            will_dispose_of,
            remove_override: RefCell::new(None),
            file_service,
            log_service,
            working_copy_backup_service,
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o gerenciador está vivo enquanto é usado")
    }

    /// `onDidCreate`.
    pub fn on_did_create(&self) -> Event<Rc<W>> {
        self.on_did_create.event()
    }

    /// Instala a sobrescrita de `remove` da subclasse (ver as divergências do módulo).
    pub fn set_remove_override(&self, remove: impl Fn(&URI) -> bool + 'static) {
        *self.remove_override.borrow_mut() = Some(Rc::new(remove));
    }

    /// `has(resource)`.
    pub fn has(&self, resource: &URI) -> bool {
        self.map_resource_to_working_copy.borrow().has(resource)
    }

    /// `add(resource, workingCopy)`.
    pub fn add(&self, resource: &URI, working_copy: &Rc<W>) {
        let known_working_copy = self.get(resource);
        if known_working_copy.is_some_and(|known| Rc::ptr_eq(&known, working_copy)) {
            return; // already cached
        }

        // Add to our working copy map
        self.map_resource_to_working_copy.borrow_mut().set(resource.clone(), working_copy.clone());

        // Update our dispose listener to remove it on dispose
        let previous_listener = self.map_resource_to_dispose_listener.borrow().get(resource).cloned();
        if let Some(previous_listener) = previous_listener {
            previous_listener.dispose();
        }
        let (this, resource_to_remove) = (self.this.clone(), resource.clone());
        let listener = (self.will_dispose_of)(working_copy).listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.remove(&resource_to_remove);
            }
        });
        self.map_resource_to_dispose_listener.borrow_mut().set(resource.clone(), listener);

        // Signal creation event
        self.on_did_create.fire(working_copy.clone());
    }

    /// `remove(resource)` com o despacho virtual: a sobrescrita da subclasse, se houver.
    pub fn remove(&self, resource: &URI) -> bool {
        let remove_override = self.remove_override.borrow().clone();
        match remove_override {
            Some(remove) => remove(resource),
            None => self.remove_base(resource),
        }
    }

    /// O `remove(resource)` da classe base (o `super.remove` das subclasses).
    pub fn remove_base(&self, resource: &URI) -> bool {
        // Dispose any existing listener
        let dispose_listener = self.map_resource_to_dispose_listener.borrow().get(resource).cloned();
        if let Some(dispose_listener) = dispose_listener {
            dispose_listener.dispose();
            self.map_resource_to_dispose_listener.borrow_mut().delete(resource);
        }

        // Remove from our working copy map
        self.map_resource_to_working_copy.borrow_mut().delete(resource)
    }

    //#region Get / Get all

    /// `workingCopies`.
    pub fn working_copies(&self) -> Vec<Rc<W>> {
        self.map_resource_to_working_copy.borrow().values().cloned().collect()
    }

    /// `get(resource)`.
    pub fn get(&self, resource: &URI) -> Option<Rc<W>> {
        self.map_resource_to_working_copy.borrow().get(resource).cloned()
    }

    //#endregion

    //#region Lifecycle

    /// `dispose()` da classe base.
    pub fn dispose_base(&self) {
        self.base.dispose();

        // Clear working copy caches
        //
        // Note: we are not explicitly disposing the working copies
        // known to the manager because this can have unwanted side
        // effects such as backups getting discarded once the working
        // copy unregisters. We have an explicit `destroy`
        // for that purpose (https://github.com/microsoft/vscode/pull/123555)
        //
        self.map_resource_to_working_copy.borrow_mut().clear();

        // Dispose the dispose listeners
        let dispose_listeners: Vec<DisposableRef> = self.map_resource_to_dispose_listener.borrow().values().cloned().collect();
        for dispose_listener in dispose_listeners {
            dispose_listener.dispose();
        }
        self.map_resource_to_dispose_listener.borrow_mut().clear();
    }

    /// `destroy()`; `dispose_manager` é o `this.dispose()` final, da subclasse.
    pub fn destroy(&self, dispose_manager: impl FnOnce() + 'static) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            // Make sure all dirty working copies are saved to disk
            let saves: Vec<Promise<()>> = this
                .working_copies()
                .into_iter()
                .map(|working_copy| {
                    let this = this.clone();
                    async_fn(async move {
                        if working_copy.is_dirty() {
                            this.save_with_fallback(working_copy).await?;
                        }
                        Ok(())
                    })
                })
                .collect();
            if let Err(error) = promises::settled(saves).await {
                this.log_service.error(&error.to_string());
            }

            // Dispose all working copies
            for working_copy in this.working_copies() {
                working_copy.dispose();
            }

            // Finally dispose manager
            dispose_manager();
            Ok(())
        })
    }

    fn save_with_fallback(&self, working_copy: Rc<W>) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            // First try regular save
            let save_success = working_copy.save(None).await.unwrap_or(false); // Ignore errors

            // Then fallback to backup if that exists
            if !save_success || working_copy.is_dirty() {
                let backup = this.working_copy_backup_service.resolve(&WorkingCopyIdentifier::of(working_copy.as_ref() as &dyn IWorkingCopy)).await?;
                if let Some(backup) = backup {
                    this.file_service
                        .write_file(&working_copy.resource(), FileContentSource::Stream(backup.value), Some(WriteFileOptions { unlock: true, ..WriteFileOptions::default() }))
                        .await?;
                }
            }
            Ok(())
        })
    }

    //#endregion
}
