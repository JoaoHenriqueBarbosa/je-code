//! Porte de `src/vs/workbench/api/common/extHostFileSystem.ts`: `workspace.registerFileSystemProvider` do lado das
//! extensões. O host guarda cada provedor por `handle`, avisa a thread principal do registro e das mudanças de arquivo, e
//! responde às operações `$stat`, `$readdir`, `$readFile`... que a thread principal manda de volta. Também registra, uma só
//! vez, um provedor de links de documento que reconhece `<esquema>:/` no texto de qualquer documento (`FsLinkProvider`).
//!
//! Divergências:
//! - `vscode.FileSystemProvider` é o trait [`ApiFileSystemProvider`] de `ext_host_file_system_consumer` (o consumidor e este
//!   módulo compartilham o contrato). `_validateFileSystemProvider` (`MISSING provider`, `Provider does NOT implement X`) não
//!   existe: o trait obriga `watch`, `stat`, `readDirectory`, `createDirectory`, `readFile`, `writeFile`, `delete` e `rename`,
//!   o que o compilador garante. As operações opcionais `copy` e `open`/`close`/`read`/`write` são perguntadas por `has_copy`
//!   e `has_open_close_read_write`; este último é o `typeof` dos quatro métodos juntos, e `$open`/`$close`/`$read`/`$write`
//!   o usam como o teste do método que cada um chama (um provedor só com parte do quarteto não existe no trait).
//! - `IMainContext` só serve para pedir o proxy `MainThreadFileSystem` no construtor, e `ExtHostLanguageFeatures` é o
//!   `Rc` do host. `registerFileSystemProvider` lança no original (esquema já registrado, proposta `fsChunks` ausente);
//!   aqui devolve [`Result`], e o `vscode.Disposable` é o [`DisposableRef`]. A ordem dos efeitos é a do original: o
//!   `checkProposedApiEnabled` falha depois de o esquema e o provedor já estarem registrados.
//! - `vscode.MarkdownString | boolean` de `isReadonly` é [`FileSystemReadonly`]; o `isMarkdownString(...)` é a variante
//!   [`FileSystemReadonly::Message`]. `FileChangeType` tem os três membros da API e `files.FileChangeType` os três do
//!   protocolo, de modo que o `default: throw new Error('Unknown FileChangeType')` não existe (a correspondência é exaustiva).
//! - `console.error` é `eprintln!`. `Uint8Array` é `Vec<u8>` e `VSBuffer.wrap` é `VSBuffer::wrap`. O `ProviderResult<T>` do
//!   provedor é [`MaybePromise`] e o `Promise.resolve(...)` é [`into_promise`].
//! - Os métodos `$` que lançam de forma síncrona no original (`_getFsProvider` sem provedor: `ENOPRO`, e `copy`, `open`,
//!   `close`, `read`, `write` ausentes) devolvem a promessa rejeitada; `$watch`, que não devolve promessa, lança
//!   ([`errors::throw`]), como o original.
//! - `StateMachine` de `linkComputer` guarda estados em `u8` (o `Uint8Matrix` do original); mais de 255 transições (esquemas
//!   com muitos caracteres distintos) dão a volta (`wrapping_add`), o que o original faria ao gravar na matriz. As colunas
//!   `scheme.toUpperCase().charCodeAt(pos)` ficam fora da aresta quando o índice passa do fim (o `NaN` do original não grava
//!   nada). O `FsLinkProvider` guarda o estado em `RefCell`, porque o provedor é compartilhado por `Rc`.
//! - `typeConverter.DocumentLink.to` devolve [`Result`] (o construtor de `DocumentLink` lança para um intervalo vazio); o
//!   erro vira a promessa rejeitada de `provideDocumentLinks`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::char_code as cc;
use je_base::errors::{self, Error};
use je_base::html_content::IMarkdownString;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::strings::common_prefix_length;
use je_base::uri::URI;
use je_editor::link_computer::{Edge, ILinkComputerTarget, LinkComputer, State, StateMachine, state};
use je_extensions::scanner::ExtensionDescription;
use je_platform::files::files::{
    FileChangeType as ProtocolFileChangeType, FileDeleteOptions, FileOpenOptions, FileOverwriteOptions, FileSystemProviderCapabilities, FileType, FileWriteOptions, Stat, WatchOptions,
};

use super::ext_host_document_data::{LineOrPosition, TextDocument};
use super::ext_host_file_system_consumer::{ApiFileSystemProvider, FileSystemProviderOptions, FileSystemReadonly, into_promise};
use super::ext_host_language_features::ExtHostLanguageFeatures;
use super::ext_host_language_features_api::{ApiDocumentLinkProvider, ApiProviderResult, DocumentLinkRef};
use super::ext_host_protocol::{ExtHostFileSystemShape, FileChangeDto, IMainContext, MainContext, MainThreadFileSystemShape};
use super::ext_host_type_converters::DocumentLink as DocumentLinkConverter;
use super::ext_host_type_converters::language_selector::DocumentSelectorInput;
use super::ext_host_types::FileChangeType;
use crate::services::extensions::extensions::check_proposed_api_enabled;

/// O texto de um esquema como unidades UTF-16 (o `scheme.length` e o `charCodeAt` do original).
fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// O documento como alvo do `LinkComputer` (`getLineContent` e `getLineCount` do objeto literal do original).
struct DocumentTarget<'a>(&'a TextDocument);

impl ILinkComputerTarget for DocumentTarget<'_> {
    fn get_line_count(&self) -> i32 {
        self.0.line_count() as i32
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        match self.0.line_at(LineOrPosition::Line(f64::from(line_number - 1))) {
            Ok(line) => line.text().clone(),
            Err(error) => errors::throw(error),
        }
    }
}

/// `FsLinkProvider`.
#[derive(Default)]
struct FsLinkProvider {
    schemes: RefCell<Vec<String>>,
    state_machine: RefCell<Option<Rc<StateMachine>>>,
}

impl FsLinkProvider {
    fn add(&self, scheme: &str) {
        *self.state_machine.borrow_mut() = None;
        self.schemes.borrow_mut().push(scheme.to_owned());
    }

    fn delete(&self, scheme: &str) {
        let index = self.schemes.borrow().iter().position(|candidate| candidate == scheme);
        if let Some(index) = index {
            self.schemes.borrow_mut().remove(index);
            *self.state_machine.borrow_mut() = None;
        }
    }

    fn init_state_machine(&self) -> Rc<StateMachine> {
        if let Some(machine) = self.state_machine.borrow().as_ref() {
            return machine.clone();
        }

        // sort and compute common prefix with previous scheme
        // then build state transitions based on the data
        self.schemes.borrow_mut().sort();
        let schemes = self.schemes.borrow().clone();
        let mut edges: Vec<Edge> = Vec::new();
        let mut prev_scheme: Option<Vec<u16>> = None;
        let mut last_state: State = state::LAST_KNOWN_STATE;
        let mut next_state: State = state::LAST_KNOWN_STATE;
        for scheme in &schemes {
            let units = utf16(scheme);
            let (upper, lower) = (utf16(&scheme.to_uppercase()), utf16(&scheme.to_lowercase()));

            // skip the common prefix of the prev scheme
            // and continue with its last state
            let mut pos = prev_scheme.as_ref().map_or(0, |prev| common_prefix_length(prev, &units));
            let mut prev_state: State = if pos == 0 { state::START } else { next_state };

            while pos < units.len() {
                // keep creating new (next) states until the
                // end (and the BeforeColon-state) is reached
                if pos + 1 == units.len() {
                    // Save the last state here, because we need to continue for the next scheme
                    last_state = next_state;
                    next_state = state::BEFORE_COLON;
                } else {
                    next_state = next_state.wrapping_add(1);
                }
                for unit in [upper.get(pos), lower.get(pos)].into_iter().flatten() {
                    edges.push((prev_state, i32::from(*unit), next_state));
                }
                prev_state = next_state;
                pos += 1;
            }

            prev_scheme = Some(units);
            // Restore the last state
            next_state = last_state;
        }

        // all link must match this pattern `<scheme>:/<more>`
        edges.push((state::BEFORE_COLON, i32::from(cc::Colon), state::AFTER_COLON));
        edges.push((state::AFTER_COLON, i32::from(cc::Slash), state::END));

        let machine = Rc::new(StateMachine::new(&edges));
        *self.state_machine.borrow_mut() = Some(machine.clone());
        machine
    }
}

impl ApiDocumentLinkProvider for FsLinkProvider {
    fn provide_document_links(&self, document: &TextDocument, _token: CancellationToken) -> ApiProviderResult<Vec<DocumentLinkRef>> {
        let machine = self.init_state_machine();

        let mut result: Vec<DocumentLinkRef> = Vec::new();
        let links = LinkComputer::compute_links(&DocumentTarget(document), &machine);

        for link in &links {
            match DocumentLinkConverter::to(link) {
                Ok(doc_link) => {
                    if doc_link.target.is_some() {
                        result.push(Rc::new(RefCell::new(doc_link)));
                    }
                }
                Err(error) => return MaybePromise::Promise(Promise::rejected(error)),
            }
        }
        MaybePromise::Value(Some(result))
    }
}

/// `ExtHostFileSystem`.
pub struct ExtHostFileSystem {
    proxy: Rc<dyn MainThreadFileSystemShape>,
    link_provider: Rc<FsLinkProvider>,
    fs_provider: RefCell<HashMap<u32, Rc<dyn ApiFileSystemProvider>>>,
    registered_schemes: RefCell<HashSet<String>>,
    watches: RefCell<HashMap<u32, DisposableRef>>,

    link_provider_registration: RefCell<Option<DisposableRef>>,
    handle_pool: Cell<u32>,
    ext_host_language_features: Rc<ExtHostLanguageFeatures>,
}

impl ExtHostFileSystem {
    pub fn new(main_context: &dyn IMainContext, ext_host_language_features: Rc<ExtHostLanguageFeatures>) -> Rc<Self> {
        Rc::new(Self {
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_FILE_SYSTEM),
            link_provider: Rc::new(FsLinkProvider::default()),
            fs_provider: RefCell::new(HashMap::new()),
            registered_schemes: RefCell::new(HashSet::new()),
            watches: RefCell::new(HashMap::new()),
            link_provider_registration: RefCell::new(None),
            handle_pool: Cell::new(0),
            ext_host_language_features,
        })
    }

    pub fn register_file_system_provider(
        self: &Rc<Self>,
        extension: &ExtensionDescription,
        scheme: &str,
        provider: Rc<dyn ApiFileSystemProvider>,
        options: FileSystemProviderOptions,
    ) -> Result<DisposableRef, Error> {
        if self.registered_schemes.borrow().contains(scheme) {
            return Err(Error::new(format!("a provider for the scheme '{scheme}' is already registered")));
        }

        //
        if self.link_provider_registration.borrow().is_none() {
            let registration = self.ext_host_language_features.register_document_link_provider(
                extension,
                &DocumentSelectorInput::Language("*".to_owned()),
                self.link_provider.clone(),
            );
            *self.link_provider_registration.borrow_mut() = Some(registration);
        }

        let handle = self.handle_pool.get();
        self.handle_pool.set(handle + 1);
        self.link_provider.add(scheme);
        self.registered_schemes.borrow_mut().insert(scheme.to_owned());
        self.fs_provider.borrow_mut().insert(handle, provider.clone());

        let mut capabilities = FileSystemProviderCapabilities::FILE_READ_WRITE;
        if options.is_case_sensitive {
            capabilities |= FileSystemProviderCapabilities::PATH_CASE_SENSITIVE;
        }
        if options.is_readonly.as_ref().is_some_and(FileSystemReadonly::is_truthy) {
            capabilities |= FileSystemProviderCapabilities::READONLY;
        }
        if provider.has_copy() {
            capabilities |= FileSystemProviderCapabilities::FILE_FOLDER_COPY;
        }
        if provider.has_open_close_read_write() {
            check_proposed_api_enabled(extension, "fsChunks")?;
            capabilities |= FileSystemProviderCapabilities::FILE_OPEN_READ_WRITE_CLOSE;
        }

        let read_only_message = match &options.is_readonly {
            Some(FileSystemReadonly::Message(message)) if !message.value.is_empty() => Some(IMarkdownString {
                value: message.value.clone(),
                is_trusted: message.is_trusted.clone(),
                support_theme_icons: message.support_theme_icons,
                support_html: message.support_html,
                support_alert_syntax: None,
                base_uri: message.base_uri.clone(),
                uris: message.uris.clone(),
            }),
            _ => None,
        };

        let extension_id = extension.identifier.value.clone();
        let registered_scheme = scheme.to_owned();
        self.proxy.rpc_register_file_system_provider(handle, scheme, capabilities, read_only_message).catch(move |err| {
            eprintln!("FAILED to register filesystem provider of {extension_id}-extension for the scheme {registered_scheme}");
            eprintln!("{err}");
        });

        let (proxy, event_scheme) = (self.proxy.clone(), scheme.to_owned());
        let subscription = provider.on_did_change_file().listen(move |event| {
            let mut mapped: Vec<FileChangeDto> = Vec::new();
            for e in event {
                if e.uri.scheme() != event_scheme.as_str() {
                    // dropping events for wrong scheme
                    continue;
                }
                let change_type = match e.change_type {
                    FileChangeType::Changed => ProtocolFileChangeType::Updated,
                    FileChangeType::Created => ProtocolFileChangeType::Added,
                    FileChangeType::Deleted => ProtocolFileChangeType::Deleted,
                };
                mapped.push(FileChangeDto { resource: e.uri.clone(), change_type });
            }
            proxy.rpc_on_file_system_change(handle, mapped);
        });

        let (this, scheme) = (self.clone(), scheme.to_owned());
        Ok(to_disposable(move || {
            subscription.dispose();
            this.link_provider.delete(&scheme);
            this.registered_schemes.borrow_mut().remove(&scheme);
            this.fs_provider.borrow_mut().remove(&handle);
            this.proxy.rpc_unregister_provider(handle);
        }))
    }

    /// `_getFsProvider(handle)`: o erro `ENOPRO` do original quando não há provedor.
    fn get_fs_provider(&self, handle: u32) -> Result<Rc<dyn ApiFileSystemProvider>, Error> {
        self.fs_provider.borrow().get(&handle).cloned().ok_or_else(|| Error::named("ENOPRO", "no provider"))
    }

    /// Roda `operation` sobre o provedor do `handle`; sem provedor, a promessa sai rejeitada (o `throw` síncrono do original).
    fn with_provider<T: Clone + 'static>(&self, handle: u32, operation: impl FnOnce(Rc<dyn ApiFileSystemProvider>) -> Promise<T>) -> Promise<T> {
        match self.get_fs_provider(handle) {
            Ok(provider) => operation(provider),
            Err(error) => Promise::rejected(error),
        }
    }
}

impl IDisposable for ExtHostFileSystem {
    fn dispose(&self) {
        let registration = self.link_provider_registration.borrow().clone();
        if let Some(registration) = registration {
            registration.dispose();
        }
    }
}

/// A promessa rejeitada de uma operação opcional que o provedor não tem.
fn not_implemented<T: Clone + 'static>(operation: &str) -> Promise<T> {
    Promise::rejected(Error::new(format!("FileSystemProvider does not implement \"{operation}\"")))
}

impl ExtHostFileSystemShape for ExtHostFileSystem {
    fn rpc_stat(&self, handle: u32, resource: &URI) -> Promise<Stat> {
        let resource = resource.clone();
        self.with_provider(handle, move |provider| into_promise(provider.stat(&resource)).then(|stat| stat.to_stat()))
    }

    fn rpc_readdir(&self, handle: u32, resource: &URI) -> Promise<Vec<(String, FileType)>> {
        let resource = resource.clone();
        self.with_provider(handle, move |provider| into_promise(provider.read_directory(&resource)))
    }

    fn rpc_read_file(&self, handle: u32, resource: &URI) -> Promise<VSBuffer> {
        let resource = resource.clone();
        self.with_provider(handle, move |provider| into_promise(provider.read_file(&resource)).then(VSBuffer::wrap))
    }

    fn rpc_write_file(&self, handle: u32, resource: &URI, content: VSBuffer, opts: FileWriteOptions) -> Promise<()> {
        let resource = resource.clone();
        self.with_provider(handle, move |provider| into_promise(content.with_bytes(|bytes| provider.write_file(&resource, bytes, &opts))))
    }

    fn rpc_rename(&self, handle: u32, resource: &URI, target: &URI, opts: FileOverwriteOptions) -> Promise<()> {
        let (old_uri, new_uri) = (resource.clone(), target.clone());
        self.with_provider(handle, move |provider| into_promise(provider.rename(&old_uri, &new_uri, &opts)))
    }

    fn rpc_copy(&self, handle: u32, resource: &URI, target: &URI, opts: FileOverwriteOptions) -> Promise<()> {
        let (old_uri, new_uri) = (resource.clone(), target.clone());
        self.with_provider(handle, move |provider| {
            if !provider.has_copy() {
                return not_implemented("copy");
            }
            into_promise(provider.copy(&old_uri, &new_uri, &opts))
        })
    }

    fn rpc_mkdir(&self, handle: u32, resource: &URI) -> Promise<()> {
        let resource = resource.clone();
        self.with_provider(handle, move |provider| into_promise(provider.create_directory(&resource)))
    }

    fn rpc_delete(&self, handle: u32, resource: &URI, opts: FileDeleteOptions) -> Promise<()> {
        let resource = resource.clone();
        self.with_provider(handle, move |provider| into_promise(provider.delete(&resource, &opts)))
    }

    fn rpc_watch(&self, handle: u32, session: u32, resource: &URI, opts: WatchOptions) {
        let provider = self.get_fs_provider(handle).unwrap_or_else(|error| errors::throw(error));
        let subscription = provider.watch(resource, &opts);
        self.watches.borrow_mut().insert(session, subscription);
    }

    fn rpc_unwatch(&self, _handle: u32, session: u32) {
        let subscription = self.watches.borrow_mut().remove(&session);
        if let Some(subscription) = subscription {
            subscription.dispose();
        }
    }

    fn rpc_open(&self, handle: u32, resource: &URI, opts: FileOpenOptions) -> Promise<u32> {
        let resource = resource.clone();
        self.with_provider(handle, move |provider| {
            if !provider.has_open_close_read_write() {
                return not_implemented("open");
            }
            into_promise(provider.open(&resource, &opts))
        })
    }

    fn rpc_close(&self, handle: u32, fd: u32) -> Promise<()> {
        self.with_provider(handle, move |provider| {
            if !provider.has_open_close_read_write() {
                return not_implemented("close");
            }
            into_promise(provider.close(fd))
        })
    }

    fn rpc_read(&self, handle: u32, fd: u32, pos: u64, length: usize) -> Promise<VSBuffer> {
        self.with_provider(handle, move |provider| {
            if !provider.has_open_close_read_write() {
                return not_implemented("read");
            }
            let data = VSBuffer::alloc(length);
            into_promise(provider.read(fd, pos, &data, 0, length)).then(move |read| {
                data.slice(Some(0), Some(read as i64)) // don't send zeros
            })
        })
    }

    fn rpc_write(&self, handle: u32, fd: u32, pos: u64, data: VSBuffer) -> Promise<usize> {
        self.with_provider(handle, move |provider| {
            if !provider.has_open_close_read_write() {
                return not_implemented("write");
            }
            into_promise(provider.write(fd, pos, &data, 0, data.byte_length()))
        })
    }
}
