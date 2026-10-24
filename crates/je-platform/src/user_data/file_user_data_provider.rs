//! Porte de `src/vs/platform/userData/common/fileUserDataProvider.ts`: o `FileUserDataProvider`, um provider que
//! embrulha o provider do sistema de arquivos local, convertendo os recursos do esquema de dados do usuário para o
//! esquema do sistema de arquivos e vice-versa, e exigindo leitura e escrita atômicas para os arquivos de perfil.
//!
//! Notas de tradução:
//! - **Interfaces de capacidade.** O original implementa uma interface por capacidade
//!   (`IFileSystemProviderWithFileReadWriteCapability` e as outras); o porte tem um único trait [`FileSystemProvider`]
//!   (ver `files.rs`), e o provider embrulhado é um [`FileSystemProviderRef`]. A máscara de capacidades é a do
//!   embrulhado, lida uma vez no construtor (`readonly capabilities`).
//! - **Opções de leitura atômica.** `readFile(resource, opts?: IFileAtomicReadOptions)` recebe o `atomic` como
//!   booleano, como o resto do trait.
//! - **Exceção síncrona.** `copy` e `cloneFile` do original lançam `Error` de forma síncrona quando o provider não tem
//!   a capacidade; quem chama (o `FileService`) sempre o faz dentro de uma função `async`, onde o lançamento vira
//!   rejeição, então aqui a rejeição da promessa é o mesmo efeito observável.
//! - **`UserDataProfilesService`.** O `IUriIdentityService.extUri` que o `ResourceSet` de recursos atômicos consulta é o
//!   [`ExtUri`](je_base::resources::ExtUri) do serviço de identidade de URIs injetado.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::buffer::{VSBuffer, VSBufferReadableStream};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::map::{ResourceMapKeyFn, ResourceSet};
use je_base::promise::Promise;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::{Field, URI, UriChange};

use crate::files::files::{
    FileAtomicOptions, FileChange, FileDeleteOptions, FileOpenOptions, FileOverwriteOptions, FileReadStreamOptions, FileSystemProvider,
    FileSystemProviderCapabilities, FileSystemProviderRef, FileType, FileWriteOptions, Stat, WatchOptions, has_file_clone_capability,
    has_file_folder_copy_capability,
};
use crate::log::{LogServiceRef, Logger};
use crate::uri_identity::uri_identity::IUriIdentityService;
use crate::user_data_profile::IUserDataProfilesService;

/// `resource.with({ scheme })`.
fn with_scheme(resource: &URI, scheme: &str) -> URI {
    resource.with(&UriChange { scheme: Field::Set(JsString::from(scheme)), ..UriChange::default() })
}

/// `FileUserDataProvider`.
///
/// This is a wrapper on top of the local filesystem provider which will
/// - Convert the user data resources to file system scheme and vice-versa
/// - Enforces atomic reads for user data
pub struct FileUserDataProvider {
    me: Weak<FileUserDataProvider>,
    base: Rc<Disposable>,
    capabilities: FileSystemProviderCapabilities,
    on_did_change_capabilities: Event<()>,
    on_did_change_file: Rc<Emitter<Vec<FileChange>>>,
    watch_resources: RefCell<TernarySearchTree<URI, URI>>,
    atomic_read_write_resources: RefCell<ResourceSet>,
    file_system_scheme: String,
    file_system_provider: FileSystemProviderRef,
    user_data_scheme: String,
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    log_service: LogServiceRef,
}

impl FileUserDataProvider {
    pub fn new(
        file_system_scheme: &str,
        file_system_provider: FileSystemProviderRef,
        user_data_scheme: &str,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let capabilities = file_system_provider.capabilities();
        let on_did_change_file = Emitter::new();
        base.register(on_did_change_file.clone());

        let ignore_path_casing = !capabilities.intersects(FileSystemProviderCapabilities::PATH_CASE_SENSITIVE);
        let watch_resources = TernarySearchTree::for_uris(Rc::new(move |_| ignore_path_casing), Rc::new(|_| false));

        let key_scheme = file_system_scheme.to_string();
        let atomic_key: ResourceMapKeyFn =
            Rc::new(move |uri| uri_identity_service.ext_uri().get_comparison_key(&with_scheme(uri, &key_scheme), false));

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            capabilities,
            on_did_change_capabilities: file_system_provider.on_did_change_capabilities(),
            on_did_change_file,
            watch_resources: RefCell::new(watch_resources),
            atomic_read_write_resources: RefCell::new(ResourceSet::new(Some(atomic_key))),
            file_system_scheme: file_system_scheme.to_string(),
            file_system_provider,
            user_data_scheme: user_data_scheme.to_string(),
            user_data_profiles_service,
            log_service,
        });

        this.update_atomic_read_writes_resources();
        let weak = Rc::downgrade(&this);
        this.base.register(this.user_data_profiles_service.on_did_change_profiles().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_atomic_read_writes_resources();
            }
        }));
        let weak = Rc::downgrade(&this);
        this.base.register(this.file_system_provider.on_did_change_file().listen(move |changes| {
            if let Some(this) = weak.upgrade() {
                this.handle_file_changes(changes);
            }
        }));
        this
    }

    fn update_atomic_read_writes_resources(&self) {
        let mut resources = self.atomic_read_write_resources.borrow_mut();
        resources.clear();
        for profile in self.user_data_profiles_service.profiles() {
            resources.add(profile.settings_resource);
            resources.add(profile.keybindings_resource);
            resources.add(profile.tasks_resource);
            resources.add(profile.extensions_resource);
        }
    }

    fn handle_file_changes(&self, changes: &[FileChange]) {
        let mut user_data_changes: Vec<FileChange> = Vec::new();
        for change in changes {
            if change.resource.scheme() != self.file_system_scheme.as_str() {
                continue; // only interested in file schemes
            }

            let user_data_resource = self.to_user_data_resource(&change.resource);
            if self.watch_resources.borrow().find_substr(&user_data_resource).is_some() {
                user_data_changes.push(FileChange { resource: user_data_resource, change_type: change.change_type, c_id: change.c_id });
            }
        }
        if !user_data_changes.is_empty() {
            self.log_service.debug("User data changed");
            self.on_did_change_file.fire(user_data_changes);
        }
    }

    fn to_file_system_resource(&self, user_data_resource: &URI) -> URI {
        with_scheme(user_data_resource, &self.file_system_scheme)
    }

    fn to_user_data_resource(&self, file_system_resource: &URI) -> URI {
        with_scheme(file_system_resource, &self.user_data_scheme)
    }
}

impl IDisposable for FileUserDataProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl FileSystemProvider for FileUserDataProvider {
    fn capabilities(&self) -> FileSystemProviderCapabilities {
        self.capabilities
    }

    fn on_did_change_capabilities(&self) -> Event<()> {
        self.on_did_change_capabilities.clone()
    }

    fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        self.on_did_change_file.event()
    }

    fn open(&self, resource: &URI, opts: &FileOpenOptions) -> Promise<i64> {
        self.file_system_provider.open(&self.to_file_system_resource(resource), opts)
    }

    fn close(&self, fd: i64) -> Promise<()> {
        self.file_system_provider.close(fd)
    }

    fn read(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
        self.file_system_provider.read(fd, pos, data, offset, length)
    }

    fn write(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
        self.file_system_provider.write(fd, pos, data, offset, length)
    }

    fn watch(&self, resource: &URI, opts: &WatchOptions) -> DisposableRef {
        self.watch_resources.borrow_mut().set(resource.clone(), resource.clone());
        let disposable = self.file_system_provider.watch(&self.to_file_system_resource(resource), opts);
        let (this, resource) = (self.me.clone(), resource.clone());
        to_disposable(move || {
            if let Some(this) = this.upgrade() {
                this.watch_resources.borrow_mut().delete(&resource);
            }
            disposable.dispose();
        })
    }

    fn stat(&self, resource: &URI) -> Promise<Stat> {
        self.file_system_provider.stat(&self.to_file_system_resource(resource))
    }

    fn mkdir(&self, resource: &URI) -> Promise<()> {
        self.file_system_provider.mkdir(&self.to_file_system_resource(resource))
    }

    fn rename(&self, from: &URI, to: &URI, opts: &FileOverwriteOptions) -> Promise<()> {
        self.file_system_provider.rename(&self.to_file_system_resource(from), &self.to_file_system_resource(to), opts)
    }

    fn read_file(&self, resource: &URI, atomic: bool) -> Promise<VSBuffer> {
        self.file_system_provider.read_file(&self.to_file_system_resource(resource), atomic)
    }

    fn read_file_stream(&self, resource: &URI, opts: &FileReadStreamOptions, token: &CancellationToken) -> VSBufferReadableStream {
        self.file_system_provider.read_file_stream(&self.to_file_system_resource(resource), opts, token)
    }

    fn readdir(&self, resource: &URI) -> Promise<Vec<(String, FileType)>> {
        self.file_system_provider.readdir(&self.to_file_system_resource(resource))
    }

    fn enforce_atomic_read_file(&self, resource: &URI) -> bool {
        self.atomic_read_write_resources.borrow().has(resource)
    }

    fn write_file(&self, resource: &URI, content: VSBuffer, opts: &FileWriteOptions) -> Promise<()> {
        self.file_system_provider.write_file(&self.to_file_system_resource(resource), content, opts)
    }

    fn enforce_atomic_write_file(&self, resource: &URI) -> Option<FileAtomicOptions> {
        if self.atomic_read_write_resources.borrow().has(resource) {
            return Some(FileAtomicOptions { postfix: ".vsctmp".to_string() });
        }

        None
    }

    fn delete(&self, resource: &URI, opts: &FileDeleteOptions) -> Promise<()> {
        self.file_system_provider.delete(&self.to_file_system_resource(resource), opts)
    }

    fn copy(&self, from: &URI, to: &URI, opts: &FileOverwriteOptions) -> Promise<()> {
        if has_file_folder_copy_capability(self.file_system_provider.as_ref()) {
            return self.file_system_provider.copy(&self.to_file_system_resource(from), &self.to_file_system_resource(to), opts);
        }
        Promise::rejected(Error::new("copy not supported"))
    }

    fn clone_file(&self, from: &URI, to: &URI) -> Promise<()> {
        if has_file_clone_capability(self.file_system_provider.as_ref()) {
            return self.file_system_provider.clone_file(&self.to_file_system_resource(from), &self.to_file_system_resource(to));
        }
        Promise::rejected(Error::new("clone not supported"))
    }
}
