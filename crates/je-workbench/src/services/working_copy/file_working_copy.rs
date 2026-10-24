//! Porte de `src/vs/workbench/services/workingCopy/common/fileWorkingCopy.ts`: os contratos das working copies de arquivo
//! (a stored e a untitled): o modelo (`IFileWorkingCopyModel`), a fábrica dele (`IFileWorkingCopyModelFactory`), a
//! configuração do modelo, o contexto do snapshot e a própria working copy (`IFileWorkingCopy`).
//!
//! Divergências:
//! - **Genéricos.** `IFileWorkingCopy<M extends IFileWorkingCopyModel>` não é genérico: o modelo é um objeto de trait
//!   (`Rc<dyn IFileWorkingCopyModel>`), porque o resto do workbench (os participantes de salvar, por exemplo) recebe a
//!   working copy sem saber o tipo do modelo. A classe concreta (`StoredFileWorkingCopy<M>`) continua genérica e dá o
//!   modelo tipado (`Rc<M>`) por um método próprio. A fábrica `IFileWorkingCopyModelFactory<M>` mantém o genérico,
//!   porque devolve o `M` concreto.
//! - **`IResolvedFileWorkingCopy`** (a working copy com `model` garantido) e o `isResolved(): this is ...` são guardas de
//!   tipo do TypeScript: aqui `model()` devolve `Option` e [`IFileWorkingCopy::is_resolved`] diz se há modelo.
//! - **`onDidChangeContent` do modelo** é `Event<unknown>` no original, que a working copy só usa como sinal. Aqui é
//!   `Event<()>`; o `IStoredFileWorkingCopyModel` acrescenta o evento tipado como `on_did_change_stored_content`
//!   (`IStoredFileWorkingCopyModel` sobrescreve o `onDidChangeContent` com um tipo mais estreito, o que Rust não tem).
//! - **`onWillDispose` da working copy** (declarado por `IFileWorkingCopy` e por `IResourceWorkingCopy`) fica só no
//!   [`IResourceWorkingCopy`](super::resource_working_copy::IResourceWorkingCopy): os dois traits na mesma working copy
//!   tornariam a chamada ambígua. A working copy untitled, que não é de recurso, o declara por conta própria.
//! - **`configuration?`** do modelo é um método com implementação padrão que devolve `None` (a propriedade é opcional).

use std::rc::Rc;

use je_base::buffer::VSBufferReadableStream;
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;

use super::working_copy::IWorkingCopy;

/// `IFileWorkingCopyModelFactory<M>`.
pub trait IFileWorkingCopyModelFactory<M: IFileWorkingCopyModel + 'static> {
    /// Create a model for the untitled or stored working copy from the given content under the provided resource.
    ///
    /// - `resource`: the `URI` of the model.
    /// - `contents`: the content of the model to create it.
    /// - `token`: support for cancellation.
    fn create_model(&self, resource: &URI, contents: VSBufferReadableStream, token: CancellationToken) -> Promise<Rc<M>>;
}

/// `IFileWorkingCopyModelConfiguration`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileWorkingCopyModelConfiguration {
    /// The delay in milliseconds to wait before triggering a backup after the content of the model has changed.
    ///
    /// If not configured, a sensible default will be taken based on user settings.
    pub backup_delay: Option<u64>,
}

/// `SnapshotContext`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotContext {
    Save = 1,
    Backup = 2,
}

/// `IFileWorkingCopyModel`: a generic file working copy model to be reused by untitled and stored file working copies.
pub trait IFileWorkingCopyModel: IDisposable {
    /// This event signals ANY changes to the contents, for example:
    /// - through the user typing into the editor
    /// - from API usage (e.g. bulk edits)
    /// - when `IFileWorkingCopyModel#update` is invoked with contents that are different from the current contents
    ///
    /// The file working copy will listen to these changes and may mark the working copy as dirty whenever this event
    /// fires.
    ///
    /// Note: ONLY report changes to the model but not the underlying file. The file working copy is tracking changes to
    /// the file automatically.
    fn on_did_change_content(&self) -> Event<()>;

    /// An event emitted right before disposing the model.
    fn on_will_dispose(&self) -> Event<()>;

    /// Optional additional configuration for the model that drives some of the working copy behaviour.
    fn configuration(&self) -> Option<FileWorkingCopyModelConfiguration> {
        None
    }

    /// Snapshots the model's current content for writing. This must include any changes that were made to the model that
    /// are in memory.
    ///
    /// - `context`: indicates in what context the snapshot is used.
    /// - `token`: support for cancellation.
    fn snapshot(&self, context: SnapshotContext, token: CancellationToken) -> Promise<VSBufferReadableStream>;

    /// Updates the model with the provided contents. The implementation should behave in a similar fashion as
    /// `IFileWorkingCopyModelFactory#createModel` except that here the model already exists and just needs to update to
    /// the provided contents.
    ///
    /// Note: it is expected that the model fires a `onDidChangeContent` event as part of the update.
    ///
    /// - `contents`: the contents to use for the model.
    /// - `token`: support for cancellation.
    fn update(&self, contents: VSBufferReadableStream, token: CancellationToken) -> Promise<()>;
}

/// `IFileWorkingCopy<M>`.
pub trait IFileWorkingCopy: IWorkingCopy + IDisposable {
    /// An event for when the file working copy has been reverted.
    fn on_did_revert(&self) -> Event<()>;

    /// Provides access to the underlying model of this file based working copy. As long as the file working copy has not
    /// been resolved, the model is `None`.
    fn model(&self) -> Option<Rc<dyn IFileWorkingCopyModel>>;

    /// Resolves the file working copy and thus makes the `model` available.
    fn resolve(&self) -> Promise<()>;

    /// Whether we have a resolved model or not.
    fn is_resolved(&self) -> bool;
}
