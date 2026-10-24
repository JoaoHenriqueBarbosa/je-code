//! Porte de `src/vs/workbench/api/common/extHostDiagnostics.ts`: o `vscode.DiagnosticCollection` ([`DiagnosticCollection`]),
//! a lista de coleções do host ([`ExtHostDiagnostics`]) e a ponte com a thread principal.
//!
//! Divergências:
//! - Os campos privados `#proxy`, `#onDidChangeDiagnostics` e `#data` são campos comuns (a privacidade vem do módulo).
//! - `Emitter<readonly vscode.Uri[]>` é o [`UriChangeEmitter`]: o `DiagnosticCollection` só chama `fire`, e o `ExtHostDiagnostics`
//!   passa o seu `DebounceEmitter` (que tem o `fire` com atraso, diferente do `Emitter` base), enquanto os testes passam um `Emitter`.
//!   `modelVersionIdProvider` devolve `Option<i64>` (o `number | undefined` do original).
//! - Os `vscode.Uri` e `UriComponents` são [`URI`], o `IMarkerData` é o [`MarkerData`] da plataforma, e o `IMarkerChange` é o struct de
//!   `ext_host_protocol`.
//! - As sobrecargas de `set` viram três métodos: [`DiagnosticCollection::set`] (`set(uri, diagnostics)`),
//!   [`DiagnosticCollection::set_many`] (`set(entries)`); o `diagnostics?: undefined` é `None`. A chamada `set(undefined)` (que o
//!   original trata como `clear()`) não existe, pois um `URI` nunca é falso. Os elementos são `Into<Option<Diagnostic>>` para que
//!   uma lista esparsa (`Vec<Option<Diagnostic>>`) passe pelo `coalesce` como no original.
//! - `get(uri)` devolve uma cópia (o `Object.freeze(result.slice(0))`): o tipo já impede a mutação do que a coleção guarda.
//!   `forEach` e `[Symbol.iterator]` percorrem um retrato das entradas ([`DiagnosticCollection::entries`]); o `thisArg` não existe.
//! - Os métodos que lançam (`illegal state - object is disposed`) levantam o [`Error`] com [`errors::throw`], como o `throw` do original.
//! - `ExtHostDiagnostics._idPool` é estático no original; aqui é um contador do próprio host (há um só em produção, e cada teste
//!   cria o seu, o que mantém `foo0` do caso `#54359` independente da ordem em que os testes rodam).
//! - `IExtHostFileSystemInfo` é o [`ExtHostFileSystemInfo`] (que tem o `extUri`) e `ExtHostDocumentsAndEditors` é o struct, de onde
//!   só `getDocument(uri)?.version` é lido. `Event.map(..., ExtHostDiagnostics._mapper)` é o `Event::map` com o mesmo mapeamento.
//! - A mensagem de `trace` leva as URIs dos `entries` como texto (o original passa os próprios objetos).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::{DebounceEmitter, Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::map::ResourceMap;
use je_base::nls::localize;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::{LogArg, LogServiceRef};
use je_platform::markers::markers::{MarkerData, MarkerSeverity};

use super::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use super::ext_host_file_system_info::ExtHostFileSystemInfo;
use super::ext_host_protocol::{ExtHostDiagnosticsShape, IMainContext, IMarkerChange, MainContext, MainThreadDiagnosticsShape};
use super::ext_host_type_converters as converter;
use super::ext_host_types::{Diagnostic, DiagnosticSeverity};

/// `Number.MAX_SAFE_INTEGER`, o limite "sem limite" da coleção espelho. Em alvos de 32 bits (wasm32) o valor não cabe em `usize`
/// e satura em `usize::MAX`, que continua maior do que qualquer contagem de diagnósticos que a memória do alvo comporte.
const MAX_SAFE_INTEGER: usize = if usize::BITS >= 64 { 9_007_199_254_740_991_u64 as usize } else { usize::MAX };

/// O `Emitter<readonly vscode.Uri[]>` que a coleção dispara a cada mudança.
pub trait UriChangeEmitter {
    /// `fire(uris)`.
    fn fire_uris(&self, uris: Vec<URI>);
}

impl UriChangeEmitter for Emitter<Vec<URI>> {
    fn fire_uris(&self, uris: Vec<URI>) {
        self.fire(uris);
    }
}

impl UriChangeEmitter for DebounceEmitter<Vec<URI>> {
    fn fire_uris(&self, uris: Vec<URI>) {
        self.fire(uris);
    }
}

/// O `Diagnostic[]` guardado por recurso, compartilhado para a coleção poder mudar o conteúdo no lugar (como o `currentDiagnostics`
/// do original).
type StoredDiagnostics = Rc<RefCell<Vec<Diagnostic>>>;

/// `DiagnosticCollection implements vscode.DiagnosticCollection`.
pub struct DiagnosticCollection {
    proxy: Option<Rc<dyn MainThreadDiagnosticsShape>>,
    on_did_change_diagnostics: Rc<dyn UriChangeEmitter>,
    data: RefCell<ResourceMap<StoredDiagnostics>>,
    is_disposed: Cell<bool>,
    name: String,
    owner: String,
    max_diagnostics_total: usize,
    max_diagnostics_per_file: usize,
    model_version_id_provider: Box<dyn Fn(&URI) -> Option<i64>>,
    dispose_hook: RefCell<Option<Box<dyn Fn()>>>,
}

fn coalesce<D: Into<Option<Diagnostic>>>(diagnostics: impl IntoIterator<Item = D>) -> Vec<Diagnostic> {
    diagnostics.into_iter().filter_map(Into::into).collect()
}

impl DiagnosticCollection {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: &str,
        owner: &str,
        max_diagnostics_total: usize,
        max_diagnostics_per_file: usize,
        model_version_id_provider: impl Fn(&URI) -> Option<i64> + 'static,
        ext_uri: ExtUri,
        proxy: Option<Rc<dyn MainThreadDiagnosticsShape>>,
        on_did_change_diagnostics: Rc<dyn UriChangeEmitter>,
    ) -> DiagnosticCollection {
        DiagnosticCollection {
            proxy,
            on_did_change_diagnostics,
            data: RefCell::new(ResourceMap::with_key_fn(Rc::new(move |uri: &URI| ext_uri.get_comparison_key(uri, false)))),
            is_disposed: Cell::new(false),
            name: name.to_owned(),
            owner: owner.to_owned(),
            max_diagnostics_total: max_diagnostics_total.max(max_diagnostics_per_file),
            max_diagnostics_per_file,
            model_version_id_provider: Box::new(model_version_id_provider),
            dispose_hook: RefCell::new(None),
        }
    }

    /// O que a subclasse anônima de `createDiagnosticCollection` faz no `dispose()` depois do `super.dispose()`.
    pub fn with_dispose_hook(self, hook: impl Fn() + 'static) -> DiagnosticCollection {
        *self.dispose_hook.borrow_mut() = Some(Box::new(hook));
        self
    }

    fn keys(&self) -> Vec<URI> {
        self.data.borrow().keys().cloned().collect()
    }

    pub fn dispose(&self) {
        if !self.is_disposed.get() {
            self.on_did_change_diagnostics.fire_uris(self.keys());
            if let Some(proxy) = &self.proxy {
                proxy.rpc_clear(&self.owner);
            }
            self.data.borrow_mut().clear();
            self.is_disposed.set(true);
        }
        if let Some(hook) = &*self.dispose_hook.borrow() {
            hook();
        }
    }

    pub fn name(&self) -> &str {
        self.check_disposed();
        &self.name
    }

    /// `set(uri, diagnostics)`: com `diagnostics` ausente (`None`) remove a entrada.
    pub fn set<D: Into<Option<Diagnostic>>>(&self, uri: &URI, diagnostics: Option<Vec<D>>) {
        self.check_disposed();

        let Some(diagnostics) = diagnostics else {
            // remove this entry
            self.delete(uri);
            return;
        };

        // update single row
        self.data.borrow_mut().set(uri.clone(), Rc::new(RefCell::new(coalesce(diagnostics))));
        self.sync(vec![uri.clone()]);
    }

    /// `set(entries)`: um `None` no lugar da lista (`[uri, undefined]`) limpa aquele recurso.
    pub fn set_many<D: Into<Option<Diagnostic>>>(&self, entries: Vec<(URI, Option<Vec<D>>)>) {
        self.check_disposed();

        // update many rows
        let mut to_sync: Vec<URI> = Vec::new();
        let mut last_uri: Option<URI> = None;

        // ensure stable-sort
        let mut entries = entries;
        entries.sort_by(|a, b| a.0.to_string(false).units().cmp(b.0.to_string(false).units()));

        for (uri, diagnostics) in entries {
            if last_uri.as_ref().is_none_or(|last| uri.to_string(false) != last.to_string(false)) {
                if let Some(last) = &last_uri {
                    let is_empty = self.data.borrow().get(last).is_some_and(|current| current.borrow().is_empty());
                    if is_empty {
                        self.data.borrow_mut().delete(last);
                    }
                }
                last_uri = Some(uri.clone());
                to_sync.push(uri.clone());
                self.data.borrow_mut().set(uri.clone(), Rc::new(RefCell::new(Vec::new())));
            }

            let current = self.data.borrow().get(&uri).cloned();
            match diagnostics {
                // [Uri, undefined] means clear this
                None => {
                    if let Some(current) = current {
                        current.borrow_mut().clear();
                    }
                }
                Some(diagnostics) => {
                    if let Some(current) = current {
                        current.borrow_mut().extend(coalesce(diagnostics));
                    }
                }
            }
        }

        self.sync(to_sync);
    }

    /// O fim comum de `set`: o evento para as extensões e a mudança para a thread principal.
    fn sync(&self, to_sync: Vec<URI>) {
        // send event for extensions
        self.on_did_change_diagnostics.fire_uris(to_sync.clone());

        // compute change and send to main side
        let Some(proxy) = &self.proxy else {
            return;
        };
        let mut entries: Vec<IMarkerChange> = Vec::new();
        let mut total_marker_count = 0;
        for uri in to_sync {
            let mut markers: Vec<MarkerData> = Vec::new();
            let mut is_truncated = false;
            let diagnostics = self.data.borrow().get(&uri).map(|stored| stored.borrow().clone());
            if let Some(diagnostics) = diagnostics {
                // no more than N diagnostics per file
                if diagnostics.len() > self.max_diagnostics_per_file {
                    is_truncated = true;
                    let order = [DiagnosticSeverity::Error, DiagnosticSeverity::Warning, DiagnosticSeverity::Information, DiagnosticSeverity::Hint];
                    'order_loop: for severity in order {
                        for diagnostic in &diagnostics {
                            if diagnostic.severity == severity {
                                markers.push(self.to_marker(&uri, diagnostic));
                                if markers.len() == self.max_diagnostics_per_file {
                                    break 'order_loop;
                                }
                            }
                        }
                    }

                    // add 'signal' marker for showing omitted errors/warnings
                    let Some(last) = markers.last() else {
                        errors::throw(Error::type_error("Cannot read properties of undefined (reading 'startLineNumber')"));
                    };
                    let omitted = (diagnostics.len() - self.max_diagnostics_per_file).to_string();
                    let signal = MarkerData::new(
                        MarkerSeverity::INFO,
                        localize("limitHit", "Not showing {0} further errors and warnings.", &[omitted.as_str()]),
                        last.start_line_number,
                        last.start_column,
                        last.end_line_number,
                        last.end_column,
                    );
                    markers.push(signal);
                } else {
                    markers = diagnostics.iter().map(|diagnostic| self.to_marker(&uri, diagnostic)).collect();
                }
            }

            total_marker_count += markers.len();
            entries.push(IMarkerChange { resource: uri, markers: Some(markers), is_truncated: Some(is_truncated) });
            if total_marker_count > self.max_diagnostics_total {
                // ignore markers that are above the limit
                break;
            }
        }
        proxy.rpc_change_many(&self.owner, entries);
    }

    /// `{ ...converter.Diagnostic.from(diagnostic), modelVersionId: this._modelVersionIdProvider(uri) }`.
    fn to_marker(&self, uri: &URI, diagnostic: &Diagnostic) -> MarkerData {
        let mut marker = converter::Diagnostic::from(diagnostic).unwrap_or_else(|error| errors::throw(error));
        marker.model_version_id = (self.model_version_id_provider)(uri);
        marker
    }

    pub fn delete(&self, uri: &URI) {
        self.check_disposed();
        self.on_did_change_diagnostics.fire_uris(vec![uri.clone()]);
        self.data.borrow_mut().delete(uri);
        if let Some(proxy) = &self.proxy {
            proxy.rpc_change_many(&self.owner, vec![IMarkerChange { resource: uri.clone(), markers: None, is_truncated: None }]);
        }
    }

    pub fn clear(&self) {
        self.check_disposed();
        self.on_did_change_diagnostics.fire_uris(self.keys());
        self.data.borrow_mut().clear();
        if let Some(proxy) = &self.proxy {
            proxy.rpc_clear(&self.owner);
        }
    }

    /// `forEach(callback)`.
    pub fn for_each(&self, mut callback: impl FnMut(&URI, &[Diagnostic], &DiagnosticCollection)) {
        self.check_disposed();
        for (uri, values) in self.entries() {
            callback(&uri, &values, self);
        }
    }

    /// `[Symbol.iterator]()`: um retrato de `[uri, diagnostics]`.
    pub fn entries(&self) -> Vec<(URI, Vec<Diagnostic>)> {
        self.check_disposed();
        self.keys().into_iter().map(|uri| {
            let diagnostics = self.get(&uri);
            (uri, diagnostics)
        }).collect()
    }

    pub fn get(&self, uri: &URI) -> Vec<Diagnostic> {
        self.check_disposed();
        self.data.borrow().get(uri).map(|stored| stored.borrow().clone()).unwrap_or_default()
    }

    pub fn has(&self, uri: &URI) -> bool {
        self.check_disposed();
        self.data.borrow().has(uri)
    }

    fn check_disposed(&self) {
        if self.is_disposed.get() {
            errors::throw(Error::new("illegal state - object is disposed"));
        }
    }
}

/// `vscode.DiagnosticChangeEvent`.
#[derive(Clone, Debug)]
pub struct DiagnosticChangeEvent {
    pub uris: Vec<URI>,
}

/// O `loggingProxy` de `createDiagnosticCollection`: repassa à thread principal e registra um `trace`.
struct LoggingProxy {
    proxy: Rc<dyn MainThreadDiagnosticsShape>,
    log_service: LogServiceRef,
    extension_id: String,
}

impl IDisposable for LoggingProxy {
    fn dispose(&self) {
        self.proxy.dispose();
    }
}

impl MainThreadDiagnosticsShape for LoggingProxy {
    fn rpc_change_many(&self, owner: &str, entries: Vec<IMarkerChange>) {
        let uris = entries.iter().map(|entry| entry.resource.to_string(false).to_string_lossy()).collect::<Vec<_>>().join(", ");
        let summary = if entries.is_empty() { "CLEARING".to_owned() } else { uris };
        self.proxy.rpc_change_many(owner, entries);
        self.log_service.trace_args(
            "[DiagnosticCollection] change many (extension, owner, uris)",
            &[LogArg::from(je_base::dynamic::Dynamic::string(&self.extension_id)), LogArg::from(je_base::dynamic::Dynamic::string(owner)), LogArg::from(je_base::dynamic::Dynamic::string(&summary))],
        );
    }

    fn rpc_clear(&self, owner: &str) {
        self.proxy.rpc_clear(owner);
        self.log_service.trace_args(
            "[DiagnosticCollection] remove all (extension, owner)",
            &[LogArg::from(je_base::dynamic::Dynamic::string(&self.extension_id)), LogArg::from(je_base::dynamic::Dynamic::string(owner))],
        );
    }
}

type Collections = Rc<RefCell<Vec<(String, Rc<DiagnosticCollection>)>>>;

/// `ExtHostDiagnostics implements ExtHostDiagnosticsShape`.
pub struct ExtHostDiagnostics {
    id_pool: Cell<u32>,
    proxy: Rc<dyn MainThreadDiagnosticsShape>,
    collections: Collections,
    on_did_change: Rc<DebounceEmitter<Vec<URI>>>,
    log_service: LogServiceRef,
    file_system_info_service: Rc<ExtHostFileSystemInfo>,
    ext_host_documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
    mirror_collection: RefCell<Option<Rc<DiagnosticCollection>>>,
    /// `onDidChangeDiagnostics`.
    pub on_did_change_diagnostics: Event<DiagnosticChangeEvent>,
}

impl ExtHostDiagnostics {
    const MAX_DIAGNOSTICS_PER_FILE: usize = 1000;
    /// `1.1 * _maxDiagnosticsPerFile`.
    const MAX_DIAGNOSTICS_TOTAL: usize = Self::MAX_DIAGNOSTICS_PER_FILE + Self::MAX_DIAGNOSTICS_PER_FILE / 10;

    /// `ExtHostDiagnostics._mapper`.
    fn mapper(last: &[URI]) -> DiagnosticChangeEvent {
        let mut map: ResourceMap<URI> = ResourceMap::new();
        for uri in last {
            map.set(uri.clone(), uri.clone());
        }
        DiagnosticChangeEvent { uris: map.values().cloned().collect() }
    }

    pub fn new(
        main_context: &dyn IMainContext,
        log_service: LogServiceRef,
        file_system_info_service: Rc<ExtHostFileSystemInfo>,
        ext_host_documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
    ) -> ExtHostDiagnostics {
        let on_did_change = DebounceEmitter::new(|all: Vec<Vec<URI>>| all.into_iter().flatten().collect(), Some(50));
        let on_did_change_diagnostics = on_did_change.event().map(|uris: &Vec<URI>| Self::mapper(uris), None);
        ExtHostDiagnostics {
            id_pool: Cell::new(0),
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_DIAGNOSTICS),
            collections: Rc::default(),
            on_did_change,
            log_service,
            file_system_info_service,
            ext_host_documents_and_editors,
            mirror_collection: RefCell::new(None),
            on_did_change_diagnostics,
        }
    }

    fn next_id(&self) -> u32 {
        let id = self.id_pool.get();
        self.id_pool.set(id + 1);
        id
    }

    fn has_collection(&self, owner: &str) -> bool {
        self.collections.borrow().iter().any(|(key, _)| key == owner)
    }

    pub fn create_diagnostic_collection(&self, extension_id: &ExtensionIdentifier, name: Option<&str>) -> Rc<DiagnosticCollection> {
        let logging_proxy: Rc<dyn MainThreadDiagnosticsShape> =
            Rc::new(LoggingProxy { proxy: self.proxy.clone(), log_service: self.log_service.clone(), extension_id: extension_id.value.clone() });

        let (name, owner) = match name.filter(|name| !name.is_empty()) {
            None => {
                let name = format!("_generated_diagnostic_collection_name_#{}", self.next_id());
                (name.clone(), name)
            }
            Some(name) if !self.has_collection(name) => (name.to_owned(), name.to_owned()),
            Some(name) => {
                self.log_service.warn(&format!("DiagnosticCollection with name '{name}' does already exist."));
                let mut owner;
                loop {
                    owner = format!("{name}{}", self.next_id());
                    if !self.has_collection(&owner) {
                        break;
                    }
                }
                (name.to_owned(), owner)
            }
        };

        let documents = self.ext_host_documents_and_editors.clone();
        let collections = self.collections.clone();
        let hook_owner = owner.clone();
        let result = Rc::new(
            DiagnosticCollection::new(
                &name,
                &owner,
                Self::MAX_DIAGNOSTICS_TOTAL,
                Self::MAX_DIAGNOSTICS_PER_FILE,
                move |uri| documents.get_document(uri).map(|document| document.document().version() as i64),
                self.file_system_info_service.ext_uri().clone(),
                Some(logging_proxy),
                self.on_did_change.clone(),
            )
            .with_dispose_hook(move || collections.borrow_mut().retain(|(key, _)| *key != hook_owner)),
        );
        self.collections.borrow_mut().push((owner, result.clone()));
        result
    }

    /// `getDiagnostics()` sem recurso: os diagnósticos de todas as coleções, juntos por URI.
    pub fn get_all_diagnostics(&self) -> Vec<(URI, Vec<Diagnostic>)> {
        let mut index: Vec<String> = Vec::new();
        let mut res: Vec<(URI, Vec<Diagnostic>)> = Vec::new();
        let collections: Vec<Rc<DiagnosticCollection>> = self.collections.borrow().iter().map(|(_, collection)| collection.clone()).collect();
        for collection in collections {
            collection.for_each(|uri, diagnostics, _| {
                let key = uri.to_string(false).to_string_lossy();
                let idx = match index.iter().position(|known| *known == key) {
                    Some(idx) => idx,
                    None => {
                        index.push(key);
                        res.push((uri.clone(), Vec::new()));
                        res.len() - 1
                    }
                };
                res[idx].1.extend_from_slice(diagnostics);
            });
        }
        res
    }

    /// `getDiagnostics(resource)`.
    pub fn get_diagnostics(&self, resource: &URI) -> Vec<Diagnostic> {
        let mut res: Vec<Diagnostic> = Vec::new();
        let collections: Vec<Rc<DiagnosticCollection>> = self.collections.borrow().iter().map(|(_, collection)| collection.clone()).collect();
        for collection in collections {
            if collection.has(resource) {
                res.extend(collection.get(resource));
            }
        }
        res
    }

    fn mirror(&self) -> Rc<DiagnosticCollection> {
        if let Some(mirror) = &*self.mirror_collection.borrow() {
            return mirror.clone();
        }
        let name = "_generated_mirror";
        let collection = Rc::new(DiagnosticCollection::new(
            name,
            name,
            MAX_SAFE_INTEGER,
            MAX_SAFE_INTEGER, // no limits because this collection is just a mirror of "sanitized" data
            |_uri| None,
            self.file_system_info_service.ext_uri().clone(),
            None,
            self.on_did_change.clone(),
        ));
        self.collections.borrow_mut().retain(|(key, _)| key != name);
        self.collections.borrow_mut().push((name.to_owned(), collection.clone()));
        *self.mirror_collection.borrow_mut() = Some(collection.clone());
        collection
    }
}

impl ExtHostDiagnosticsShape for ExtHostDiagnostics {
    fn rpc_accept_markers_change(&self, data: Vec<(URI, Vec<MarkerData>)>) {
        let mirror = self.mirror();
        for (uri, markers) in data {
            if markers.is_empty() {
                mirror.delete(&uri);
            } else {
                let diagnostics: Vec<Diagnostic> = markers.iter().map(|marker| converter::Diagnostic::to(marker).unwrap_or_else(|error| errors::throw(error))).collect();
                mirror.set(&uri, Some(diagnostics));
            }
        }
    }
}
