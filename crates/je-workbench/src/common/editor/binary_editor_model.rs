//! Porte de `src/vs/workbench/common/editor/binaryEditorModel.ts`: o `BinaryEditorModel`, um modelo de editor que só
//! representa um recurso que pode ser carregado (nome, tamanho, mime e etag do arquivo binário).
//!
//! Divergências:
//! - **Herança.** `BinaryEditorModel extends EditorModel` guarda um [`EditorModel`] (campo `base`) e implementa
//!   [`IEditorModel`] sobrescrevendo só o `resolve`, como o original.
//! - **Injeção.** Os argumentos estáticos são `(resource, name)` ([`Injectable::Args`]) e o serviço decorado
//!   (`IFileService`) é a tupla `Deps`.
//! - **`resolve`.** O `typeof stat.size === 'number'` é sempre verdadeiro: o `size` de um stat com metadados é um número.
//!   A continuação guarda o `Rc` do modelo (o `this` do original), que vive até a promessa se resolver.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::mime::mimes;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::files::i_file_service::IFileService;

use crate::common::editor::editor_model::{EditorModel, IEditorModel};

/// `BinaryEditorModel`: an editor model that just represents a resource that can be loaded.
pub struct BinaryEditorModel {
    this: Weak<BinaryEditorModel>,
    base: EditorModel,

    /// `resource`.
    pub resource: URI,
    name: String,
    mime: &'static str,
    size: Cell<Option<u64>>,
    etag: RefCell<Option<String>>,

    file_service: Rc<dyn IFileService>,
}

impl Injectable for BinaryEditorModel {
    /// `(resource, name)`.
    type Args = (URI, String);
    type Deps = (Rc<dyn IFileService>,);

    fn create((resource, name): Self::Args, (file_service,): Self::Deps) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: EditorModel::new_inner(),
            resource,
            name,
            mime: mimes::BINARY,
            size: Cell::new(None),
            etag: RefCell::new(None),
            file_service,
        })
    }
}

impl BinaryEditorModel {
    /// The name of the binary resource.
    pub fn get_name(&self) -> String {
        self.name.clone()
    }

    /// The size of the binary resource if known.
    pub fn get_size(&self) -> Option<u64> {
        self.size.get()
    }

    /// The mime of the binary resource if known.
    pub fn get_mime(&self) -> &'static str {
        self.mime
    }

    /// The etag of the binary resource if known.
    pub fn get_etag(&self) -> Option<String> {
        self.etag.borrow().clone()
    }
}

impl IDisposable for BinaryEditorModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorModel for BinaryEditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.base.on_will_dispose()
    }

    fn resolve(&self) -> Promise<()> {
        let this = self.this.upgrade().expect("o modelo binário já foi liberado");

        async_fn(async move {
            // Make sure to resolve up to date stat for file resources
            if this.file_service.has_provider(&this.resource) {
                let stat = this.file_service.stat(&this.resource).await?;
                *this.etag.borrow_mut() = Some(stat.etag().to_string());
                this.size.set(Some(stat.size()));
            }

            IEditorModel::resolve(&this.base).await
        })
    }

    fn is_resolved(&self) -> bool {
        self.base.is_resolved()
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }
}
