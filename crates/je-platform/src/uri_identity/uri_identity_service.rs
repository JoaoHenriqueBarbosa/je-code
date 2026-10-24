//! Porte de `src/vs/platform/uriIdentity/common/uriIdentityService.ts`: o `UriIdentityService`, que devolve a forma
//! canônica de uma URI (as URIs que só diferem por caixa ou por falta de normalização viram a mesma) e expõe o
//! [`ExtUri`] que conhece a caixa dos caminhos de cada esquema.
//!
//! Notas de tradução:
//! - **Injeção.** O parâmetro decorado `@IFileService` é a tupla `Deps` do [`Injectable`], e `create` é o construtor
//!   (os testes o chamam direto). `registerSingleton(IUriIdentityService, UriIdentityService, InstantiationType.Delayed)`
//!   é [`register`]. A classe herda de nada além do `dispose`, que aqui é o [`IDisposable`].
//! - **`extUri`.** A propriedade é [`IUriIdentityService::ext_uri`], que devolve uma cópia do [`ExtUri`]: ele guarda
//!   o predicado de caixa num `Rc`, então todas as cópias leem o mesmo cache por esquema.
//! - **Identidade da URI.** O `Entry` guarda a URI canônica e o `===` do original (a mesma instância) é
//!   [`URI::ptr_eq`]: `asCanonicalUri` devolve a própria instância guardada quando o fragmento não muda, porque
//!   [`URI::with`] sem mudança devolve `self`.
//! - **`Entry._clock`.** É um campo estático do original (compartilhado por todas as instâncias do serviço); aqui é
//!   um `thread_local`, o mesmo escopo que o resto do porte usa para estado de módulo.
//! - **Mudança de provider.** O ouvinte de `onDidChangeFileSystemProviderRegistrations` e
//!   `onDidChangeFileSystemProviderCapabilities` do original compara `newIgnorePathCasingValue` com ele mesmo
//!   (`x === x`, sempre verdadeiro) e volta antes do laço que limparia as URIs canônicas do esquema. O efeito
//!   observável é preservado: o ouvinte só esquece o valor em cache do esquema, recalcula-o e nunca descarta URIs
//!   canônicas.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::arrays::quick_select;
use je_base::event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::resources::ExtUri;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_instantiation::{Injectable, InstantiationType, register_singleton, service_impl};

use super::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use crate::files::files::FileSystemProviderCapabilities;
use crate::files::i_file_service::IFileService;
use crate::resources::{ext_uri, host_environment};

thread_local! {
    /// `Entry._clock`.
    static CLOCK: Cell<i64> = const { Cell::new(0) };
}

/// `Entry._clock++`.
fn next_tick() -> i64 {
    CLOCK.with(|clock| {
        let tick = clock.get();
        clock.set(tick + 1);
        tick
    })
}

/// `Entry`: a URI canônica e o instante do último acesso.
struct Entry {
    uri: URI,
    time: Cell<i64>,
}

impl Entry {
    fn new(uri: URI) -> Self {
        Self { uri, time: Cell::new(next_tick()) }
    }

    fn touch(&self) -> &Self {
        self.time.set(next_tick());
        self
    }
}

/// O cache de "este esquema ignora a caixa do caminho?" e a pergunta ao `IFileService` que o preenche.
struct PathCasing {
    file_service: Rc<dyn IFileService>,
    scheme_ignores_path_casing_cache: RefCell<HashMap<String, bool>>,
}

impl PathCasing {
    /// `ignorePathCasing(uri)`: assume path casing matters unless the file system provider spec'ed the opposite.
    /// For all other cases path casing matters, e.g for
    /// * virtual documents
    /// * in-memory uris
    /// * all kind of "private" schemes
    fn ignore_path_casing(&self, uri: &URI) -> bool {
        let scheme = uri.scheme().to_string_lossy();
        let cached = self.scheme_ignores_path_casing_cache.borrow().get(&scheme).copied();
        if let Some(ignore_path_casing) = cached {
            return ignore_path_casing;
        }
        // retrieve once and then case per scheme until a change happens
        let ignore_path_casing =
            self.file_service.has_provider(uri) && !self.file_service.has_capability(uri, FileSystemProviderCapabilities::PATH_CASE_SENSITIVE);
        self.scheme_ignores_path_casing_cache.borrow_mut().insert(scheme, ignore_path_casing);
        ignore_path_casing
    }

    /// O corpo do ouvinte de mudança de registro e de capacidades de providers.
    fn did_change_provider(&self, scheme: &str) {
        if !self.scheme_ignores_path_casing_cache.borrow().contains_key(scheme) {
            return;
        }
        self.scheme_ignores_path_casing_cache.borrow_mut().remove(scheme);
        let scheme_only = URI::from(&UriComponents { scheme: Some(JsString::from(scheme)), ..UriComponents::default() }, false);
        self.ignore_path_casing(&scheme_only);
    }
}

/// `UriIdentityService`.
pub struct UriIdentityService {
    file_service: Rc<dyn IFileService>,
    ext_uri: ExtUri,
    disposables: Rc<DisposableStore>,
    canonical_uris: RefCell<HashMap<JsString, Entry>>,
}

/// `_limit = 2 ** 16`.
const LIMIT: usize = 1 << 16;

impl Injectable for UriIdentityService {
    type Args = ();
    type Deps = (Rc<dyn IFileService>,);

    fn create(_: (), (file_service,): Self::Deps) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let path_casing = Rc::new(PathCasing { file_service: file_service.clone(), scheme_ignores_path_casing_cache: RefCell::new(HashMap::new()) });

        let registrations = file_service.on_did_change_file_system_provider_registrations().map(|e| e.scheme.clone(), None);
        let capabilities = file_service.on_did_change_file_system_provider_capabilities().map(|e| e.scheme.clone(), None);
        let listener_path_casing = path_casing.clone();
        disposables.add(event::any(vec![registrations, capabilities]).listen(move |scheme| listener_path_casing.did_change_provider(scheme)));

        let ext_uri = ExtUri::new(host_environment(), move |uri| path_casing.ignore_path_casing(uri));
        Rc::new(Self { file_service, ext_uri, disposables, canonical_uris: RefCell::new(HashMap::new()) })
    }
}

service_impl!(UriIdentityService => dyn IUriIdentityService, disposable);

/// `registerSingleton(IUriIdentityService, UriIdentityService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<UriIdentityService>(I_URI_IDENTITY_SERVICE, InstantiationType::Delayed);
}

impl IDisposable for UriIdentityService {
    fn dispose(&self) {
        self.disposables.dispose();
        self.canonical_uris.borrow_mut().clear();
    }
}

impl IUriIdentityService for UriIdentityService {
    fn ext_uri(&self) -> ExtUri {
        self.ext_uri.clone()
    }

    fn as_canonical_uri(&self, uri: &URI) -> URI {
        // (1) normalize URI
        let uri = if self.file_service.has_provider(uri) { ext_uri().normalize_path(uri) } else { uri.clone() };

        // (2) find the uri in its canonical form or use this uri to define it
        let uri_key = self.ext_uri.get_comparison_key(&uri, true);
        let canonical = self.canonical_uris.borrow().get(&uri_key).map(|item| item.touch().uri.clone());
        if let Some(canonical) = canonical {
            return canonical.with(&UriChange { fragment: Field::Set(uri.fragment().clone()), ..UriChange::default() });
        }

        // this uri is first and defines the canonical form
        self.canonical_uris.borrow_mut().insert(uri_key, Entry::new(uri.clone()));
        self.check_trim();

        uri
    }
}

impl UriIdentityService {
    fn check_trim(&self) {
        if self.canonical_uris.borrow().len() < LIMIT {
            return;
        }

        CLOCK.with(|clock| clock.set(1));
        let times: Vec<i64> = self.canonical_uris.borrow().values().map(|entry| entry.time.get()).collect();
        let median = quick_select(times.len() / 2, &times, |a, b| (a - b) as f64);
        // Its important to remove the median value here (<= not <).
        // If we have not touched any items since the last trim, the
        // median will be 0 and no items will be removed otherwise.
        self.canonical_uris.borrow_mut().retain(|_, entry| {
            if entry.time.get() <= median {
                false
            } else {
                entry.time.set(0);
                true
            }
        });
    }
}
