//! Porte de `src/vs/workbench/services/extensions/common/extensionDescriptionRegistry.ts`.
//!
//! Divergências:
//! - `IExtensionDescription` é o `ExtensionDescription` de `je-extensions`, compartilhado como `Rc` (no JavaScript as
//!   funções devolvem a mesma referência). O `extensionLocation` desse tipo é o texto da URI, lido com `URI.parse` na
//!   ordenação. `ExtensionIdentifier | string` é `&str` (a comparação ignora a caixa, como o `ExtensionIdentifierMap`).
//! - O trait [`IReadOnlyExtensionDescriptionRegistry`] é a interface do original; a classe
//!   [`ExtensionDescriptionRegistry`] o implementa, e quem precisa sobrescrever um método (um teste que estende a classe)
//!   implementa o trait. `isHostExtension` é função associada e recebe os registros concretos, como no original.
//! - A ordenação das extensões compara o último segmento do caminho como `string` do JavaScript (unidades UTF-16), e a
//!   ordenação é estável, como `Array.prototype.sort`.
//! - Os erros de `console.error` e `console.warn` vão para `eprintln!`.
//! - `Lock.acquire` é `async` e devolve `customer.promise`: a promessa devolvida adota a do cliente (duas rodadas a mais,
//!   como no motor JS).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionIdentifierMap, ExtensionIdentifierSet};
use je_extensions::scanner::ExtensionDescription;

/// Uma extensão compartilhada (a referência do JavaScript).
pub type SharedExtensionDescription = Rc<ExtensionDescription>;

/// `extensionDescription.main || extensionDescription.browser`: o `||` do JavaScript também descarta o texto vazio.
pub fn has_main_or_browser(extension_description: &ExtensionDescription) -> bool {
    let manifest = &extension_description.manifest;
    manifest.main.as_deref().is_some_and(|main| !main.is_empty()) || manifest.browser.as_deref().is_some_and(|browser| !browser.is_empty())
}

/// `DeltaExtensionsResult`.
#[derive(Clone, Debug)]
pub struct DeltaExtensionsResult {
    pub version_id: i64,
    pub removed_due_to_looping: Vec<SharedExtensionDescription>,
}

/// `IReadOnlyExtensionDescriptionRegistry`.
pub trait IReadOnlyExtensionDescriptionRegistry {
    fn contains_activation_event(&self, activation_event: &str) -> bool;
    fn contains_extension(&self, extension_id: &str) -> bool;
    fn get_extension_descriptions_for_activation_event(&self, activation_event: &str) -> Vec<SharedExtensionDescription>;
    fn get_all_extension_descriptions(&self) -> Vec<SharedExtensionDescription>;
    fn get_extension_description(&self, extension_id: &str) -> Option<SharedExtensionDescription>;
    fn get_extension_description_by_uuid(&self, uuid: &str) -> Option<SharedExtensionDescription>;

    /// `getExtensionDescriptionByIdOrUUID`: o `??` do original chama os dois métodos acima (que podem ter sido
    /// sobrescritos).
    fn get_extension_description_by_id_or_uuid(&self, extension_id: &str, uuid: Option<&str>) -> Option<SharedExtensionDescription> {
        self.get_extension_description(extension_id).or_else(|| match uuid {
            Some(uuid) if !uuid.is_empty() => self.get_extension_description_by_uuid(uuid),
            _ => None,
        })
    }
}

/// `IActivationEventsReader`.
pub trait IActivationEventsReader {
    fn read_activation_events(&self, extension_description: &ExtensionDescription) -> Vec<String>;
}

/// `ExtensionDescriptionRegistrySnapshot`.
#[derive(Clone, Debug)]
pub struct ExtensionDescriptionRegistrySnapshot {
    pub version_id: i64,
    pub extensions: Vec<SharedExtensionDescription>,
}

struct RegistryState {
    version_id: i64,
    extension_descriptions: Vec<SharedExtensionDescription>,
    extensions_map: ExtensionIdentifierMap<SharedExtensionDescription>,
    extensions_arr: Vec<SharedExtensionDescription>,
    activation_map: HashMap<String, Vec<SharedExtensionDescription>>,
}

/// `ExtensionDescriptionRegistry`.
pub struct ExtensionDescriptionRegistry {
    base: Rc<Disposable>,
    on_did_change: Rc<Emitter<()>>,
    activation_events_reader: Rc<dyn IActivationEventsReader>,
    state: RefCell<RegistryState>,
}

impl ExtensionDescriptionRegistry {
    /// `ExtensionDescriptionRegistry.isHostExtension(extensionId, myRegistry, globalRegistry)`.
    pub fn is_host_extension(extension_id: &str, my_registry: &ExtensionDescriptionRegistry, global_registry: &ExtensionDescriptionRegistry) -> bool {
        if my_registry.get_extension_description(extension_id).is_some() {
            // I have this extension
            return false;
        }
        let Some(extension_description) = global_registry.get_extension_description(extension_id) else {
            // unknown extension
            return false;
        };
        has_main_or_browser(&extension_description) && extension_description.manifest.api.as_deref() == Some("none")
    }

    pub fn new(activation_events_reader: Rc<dyn IActivationEventsReader>, extension_descriptions: Vec<SharedExtensionDescription>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let this = Rc::new(Self {
            base,
            on_did_change,
            activation_events_reader,
            state: RefCell::new(RegistryState {
                version_id: 0,
                extension_descriptions: Vec::new(),
                extensions_map: ExtensionIdentifierMap::new(),
                extensions_arr: Vec::new(),
                activation_map: HashMap::new(),
            }),
        });
        this.initialize(extension_descriptions);
        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn initialize(&self, mut extension_descriptions: Vec<SharedExtensionDescription>) {
        // Ensure extensions are stored in the order: builtin, user, under development
        extension_descriptions.sort_by(|a, b| extension_cmp(a, b));

        let mut extensions_map = ExtensionIdentifierMap::new();
        let mut extensions_arr = Vec::new();
        let mut activation_map: HashMap<String, Vec<SharedExtensionDescription>> = HashMap::new();

        for extension_description in &extension_descriptions {
            if extensions_map.has(&extension_description.identifier.value) {
                // No overwriting allowed!
                eprintln!("Extension `{}` is already registered", extension_description.identifier.value);
                continue;
            }

            extensions_map.set(&extension_description.identifier.value, extension_description.clone());
            extensions_arr.push(extension_description.clone());

            let activation_events = self.activation_events_reader.read_activation_events(extension_description);
            for activation_event in activation_events {
                activation_map.entry(activation_event).or_default().push(extension_description.clone());
            }
        }

        let mut state = self.state.borrow_mut();
        state.extension_descriptions = extension_descriptions;
        state.extensions_map = extensions_map;
        state.extensions_arr = extensions_arr;
        state.activation_map = activation_map;
    }

    /// `set(extensionDescriptions)`: devolve o `versionId`.
    pub fn set(&self, extension_descriptions: Vec<SharedExtensionDescription>) -> i64 {
        self.initialize(extension_descriptions);
        let version_id = {
            let mut state = self.state.borrow_mut();
            state.version_id += 1;
            state.version_id
        };
        self.on_did_change.fire(());
        version_id
    }

    /// `deltaExtensions(toAdd, toRemove)`.
    pub fn delta_extensions(&self, to_add: Vec<SharedExtensionDescription>, to_remove: &[ExtensionIdentifier]) -> DeltaExtensionsResult {
        // It is possible that an extension is removed, only to be added again at a different version
        // so we will first handle removals
        let current = self.state.borrow().extension_descriptions.clone();
        let mut extension_descriptions = remove_extensions(&current, to_remove);

        // Then, handle the extensions to add
        extension_descriptions.extend(to_add);

        // Immediately remove looping extensions!
        let looping = Self::find_looping_extensions(&extension_descriptions);
        let looping_ids: Vec<ExtensionIdentifier> = looping.iter().map(|ext| ext.identifier.clone()).collect();
        let extension_descriptions = remove_extensions(&extension_descriptions, &looping_ids);

        self.initialize(extension_descriptions);
        let version_id = {
            let mut state = self.state.borrow_mut();
            state.version_id += 1;
            state.version_id
        };
        self.on_did_change.fire(());
        DeltaExtensionsResult { version_id, removed_due_to_looping: looping }
    }

    fn find_looping_extensions(extension_descriptions: &[SharedExtensionDescription]) -> Vec<SharedExtensionDescription> {
        #[derive(Default)]
        struct Graph {
            arcs: HashMap<String, Vec<String>>,
            nodes_set: HashSet<String>,
            nodes_arr: Vec<String>,
        }

        impl Graph {
            fn add_node(&mut self, id: &str) {
                if !self.nodes_set.contains(id) {
                    self.nodes_set.insert(id.to_owned());
                    self.nodes_arr.push(id.to_owned());
                }
            }

            fn add_arc(&mut self, from: &str, to: &str) {
                self.add_node(from);
                self.add_node(to);
                self.arcs.entry(from.to_owned()).or_default().push(to.to_owned());
            }

            fn get_arcs(&self, id: &str) -> &[String] {
                self.arcs.get(id).map(Vec::as_slice).unwrap_or(&[])
            }

            fn has_only_good_arcs(&self, id: &str, good: &HashSet<String>) -> bool {
                self.get_arcs(id).iter().all(|dependency| good.contains(dependency))
            }
        }

        let mut graph = Graph::default();
        let mut descs: ExtensionIdentifierMap<SharedExtensionDescription> = ExtensionIdentifierMap::new();
        for extension_description in extension_descriptions {
            descs.set(&extension_description.identifier.value, extension_description.clone());
            if let Some(dependencies) = &extension_description.manifest.extension_dependencies {
                for dep_id in dependencies {
                    graph.add_arc(&ExtensionIdentifier::to_key(&extension_description.identifier.value), &ExtensionIdentifier::to_key(dep_id));
                }
            }
        }

        // initialize with all extensions with no dependencies.
        let mut good: HashSet<String> = HashSet::new();
        for id in graph.nodes_arr.iter().filter(|id| graph.get_arcs(id).is_empty()) {
            good.insert(id.clone());
        }

        // all other extensions will be processed below.
        let mut nodes: Vec<String> = graph.nodes_arr.iter().filter(|id| !good.contains(*id)).cloned().collect();

        loop {
            let mut made_progress = false;

            // find one extension which has only good deps
            let mut i = 0;
            while i < nodes.len() {
                let id = nodes[i].clone();

                if graph.has_only_good_arcs(&id, &good) {
                    nodes.remove(i);
                    good.insert(id);
                    made_progress = true;
                } else {
                    i += 1;
                }
            }

            if !made_progress {
                break;
            }
        }

        // The remaining nodes are bad and have loops
        nodes.iter().filter_map(|id| descs.get(id).cloned()).collect()
    }

    /// `getSnapshot()`.
    pub fn get_snapshot(&self) -> ExtensionDescriptionRegistrySnapshot {
        ExtensionDescriptionRegistrySnapshot { version_id: self.state.borrow().version_id, extensions: self.get_all_extension_descriptions() }
    }
}

impl IReadOnlyExtensionDescriptionRegistry for ExtensionDescriptionRegistry {
    fn contains_activation_event(&self, activation_event: &str) -> bool {
        self.state.borrow().activation_map.contains_key(activation_event)
    }

    fn contains_extension(&self, extension_id: &str) -> bool {
        self.state.borrow().extensions_map.has(extension_id)
    }

    fn get_extension_descriptions_for_activation_event(&self, activation_event: &str) -> Vec<SharedExtensionDescription> {
        self.state.borrow().activation_map.get(activation_event).cloned().unwrap_or_default()
    }

    fn get_all_extension_descriptions(&self) -> Vec<SharedExtensionDescription> {
        self.state.borrow().extensions_arr.clone()
    }

    fn get_extension_description(&self, extension_id: &str) -> Option<SharedExtensionDescription> {
        self.state.borrow().extensions_map.get(extension_id).cloned()
    }

    fn get_extension_description_by_uuid(&self, uuid: &str) -> Option<SharedExtensionDescription> {
        self.state.borrow().extensions_arr.iter().find(|extension_description| extension_description.uuid.as_deref() == Some(uuid)).cloned()
    }
}

impl IDisposable for ExtensionDescriptionRegistry {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `LockableExtensionDescriptionRegistry`.
pub struct LockableExtensionDescriptionRegistry {
    actual: Rc<ExtensionDescriptionRegistry>,
    lock: Rc<Lock>,
}

impl LockableExtensionDescriptionRegistry {
    pub fn new(activation_events_reader: Rc<dyn IActivationEventsReader>) -> Rc<Self> {
        Rc::new(Self { actual: ExtensionDescriptionRegistry::new(activation_events_reader, Vec::new()), lock: Lock::new() })
    }

    /// `acquireLock(customerName)`.
    pub fn acquire_lock(self: &Rc<Self>, customer_name: &str) -> Promise<Rc<ExtensionDescriptionRegistryLock>> {
        let this = self.clone();
        let acquired = self.lock.acquire(customer_name);
        async_fn(async move {
            let lock = acquired.await?;
            Ok(ExtensionDescriptionRegistryLock::new(this, lock))
        })
    }

    /// `deltaExtensions(acquiredLock, toAdd, toRemove)`: o erro do original (`Lock is not held`) é o `Err`.
    pub fn delta_extensions(
        &self,
        acquired_lock: &ExtensionDescriptionRegistryLock,
        to_add: Vec<SharedExtensionDescription>,
        to_remove: &[ExtensionIdentifier],
    ) -> Result<DeltaExtensionsResult, Error> {
        if !acquired_lock.is_acquired_for(self) {
            return Err(Error::new("Lock is not held"));
        }
        Ok(self.actual.delta_extensions(to_add, to_remove))
    }

    /// `getSnapshot()`.
    pub fn get_snapshot(&self) -> ExtensionDescriptionRegistrySnapshot {
        self.actual.get_snapshot()
    }
}

impl IReadOnlyExtensionDescriptionRegistry for LockableExtensionDescriptionRegistry {
    fn contains_activation_event(&self, activation_event: &str) -> bool {
        self.actual.contains_activation_event(activation_event)
    }

    fn contains_extension(&self, extension_id: &str) -> bool {
        self.actual.contains_extension(extension_id)
    }

    fn get_extension_descriptions_for_activation_event(&self, activation_event: &str) -> Vec<SharedExtensionDescription> {
        self.actual.get_extension_descriptions_for_activation_event(activation_event)
    }

    fn get_all_extension_descriptions(&self) -> Vec<SharedExtensionDescription> {
        self.actual.get_all_extension_descriptions()
    }

    fn get_extension_description(&self, extension_id: &str) -> Option<SharedExtensionDescription> {
        self.actual.get_extension_description(extension_id)
    }

    fn get_extension_description_by_uuid(&self, uuid: &str) -> Option<SharedExtensionDescription> {
        self.actual.get_extension_description_by_uuid(uuid)
    }

    fn get_extension_description_by_id_or_uuid(&self, extension_id: &str, uuid: Option<&str>) -> Option<SharedExtensionDescription> {
        self.actual.get_extension_description_by_id_or_uuid(extension_id, uuid)
    }
}

/// `ExtensionDescriptionRegistryLock`. O `_isDisposed` do original nunca vira `true` (nenhum código o escreve); o porte
/// mantém o comportamento.
pub struct ExtensionDescriptionRegistryLock {
    base: Rc<Disposable>,
    registry: Rc<LockableExtensionDescriptionRegistry>,
    is_disposed: Cell<bool>,
}

impl ExtensionDescriptionRegistryLock {
    fn new(registry: Rc<LockableExtensionDescriptionRegistry>, lock: DisposableRef) -> Rc<Self> {
        let base = Disposable::new();
        base.register(lock);
        Rc::new(Self { base, registry, is_disposed: Cell::new(false) })
    }

    /// `isAcquiredFor(registry)`.
    pub fn is_acquired_for(&self, registry: &LockableExtensionDescriptionRegistry) -> bool {
        !self.is_disposed.get() && std::ptr::eq(&*self.registry, registry)
    }
}

impl IDisposable for ExtensionDescriptionRegistryLock {
    fn dispose(&self) {
        self.base.dispose();
    }
}

struct LockCustomer {
    name: String,
    promise: Promise<DisposableRef>,
    resolver: je_base::promise::Resolver<DisposableRef>,
}

impl LockCustomer {
    fn new(name: &str) -> Rc<Self> {
        let (promise, resolver) = Promise::new();
        Rc::new(Self { name: name.to_owned(), promise, resolver })
    }

    fn resolve(&self, value: DisposableRef) {
        self.resolver.resolve(value);
    }
}

struct Lock {
    pending_customers: RefCell<VecDeque<Rc<LockCustomer>>>,
    is_locked: Cell<bool>,
}

impl Lock {
    fn new() -> Rc<Self> {
        Rc::new(Self { pending_customers: RefCell::new(VecDeque::new()), is_locked: Cell::new(false) })
    }

    /// `async acquire(customerName)`: devolve `customer.promise`, que a promessa da função adota.
    fn acquire(self: &Rc<Self>, customer_name: &str) -> Promise<DisposableRef> {
        let customer = LockCustomer::new(customer_name);
        self.pending_customers.borrow_mut().push_back(customer.clone());
        self.advance();
        let (promise, resolver) = Promise::new();
        resolver.resolve_with(&customer.promise);
        promise
    }

    fn advance(self: &Rc<Self>) {
        if self.is_locked.get() {
            // cannot advance yet
            return;
        }
        let next = self.pending_customers.borrow_mut().pop_front();
        let Some(customer) = next else {
            // no more waiting customers
            return;
        };

        self.is_locked.set(true);
        let customer_holds_lock = Rc::new(Cell::new(true));

        let log_long_running_customer_timeout = runtime::set_timeout(
            {
                let customer_holds_lock = customer_holds_lock.clone();
                let name = customer.name.clone();
                move || {
                    if customer_holds_lock.get() {
                        eprintln!("The customer named {name} has been holding on to the lock for 30s. This might be a problem.");
                    }
                }
            },
            30 * 1000, /* 30 seconds */
        );

        let release_lock = {
            let this = self.clone();
            let customer_holds_lock = customer_holds_lock.clone();
            move || {
                if !customer_holds_lock.get() {
                    return;
                }
                runtime::clear_timeout(log_long_running_customer_timeout);
                customer_holds_lock.set(false);
                this.is_locked.set(false);
                this.advance();
            }
        };

        customer.resolve(to_disposable(release_lock));
    }
}

/// `extensionCmp`. Garante que: primeiro vêm as extensões embutidas, depois as do usuário e por fim as em
/// desenvolvimento; em cada grupo, ordem alfabética pelo nome da pasta.
fn extension_cmp(a: &ExtensionDescription, b: &ExtensionDescription) -> std::cmp::Ordering {
    const BUILTIN: u8 = 0;
    const USER: u8 = 1;
    const DEV: u8 = 2;

    fn bucket(extension: &ExtensionDescription) -> u8 {
        if extension.is_builtin {
            BUILTIN
        } else if extension.is_under_development {
            DEV
        } else {
            USER
        }
    }

    fn last_segment(extension: &ExtensionDescription) -> JsString {
        let location = URI::parse(extension.extension_location.as_str(), false);
        JsString::from(PathModule::posix(&ProcessEnvironment).basename(&location.path().to_string_lossy(), None))
    }

    let (a_sort_bucket, b_sort_bucket) = (bucket(a), bucket(b));
    if a_sort_bucket != b_sort_bucket {
        return a_sort_bucket.cmp(&b_sort_bucket);
    }
    last_segment(a).cmp(&last_segment(b))
}

fn remove_extensions(arr: &[SharedExtensionDescription], to_remove: &[ExtensionIdentifier]) -> Vec<SharedExtensionDescription> {
    let to_remove_set = ExtensionIdentifierSet::new(to_remove.iter().map(|id| id.value.as_str()));
    arr.iter().filter(|extension| !to_remove_set.has(&extension.identifier.value)).cloned().collect()
}
