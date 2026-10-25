//! Porte de `src/vs/editor/browser/widget/diffEditor/diffProviderFactoryService.ts`: o serviço que cria o provedor de diferenças
//! de um editor de diff (`IDiffProviderFactoryService`), a implementação que pede o cálculo ao serviço de worker
//! (`WorkerBasedDiffProviderFactoryService`) e o provedor que ela cria (`WorkerBasedDocumentDiffProvider`).
//!
//! Divergências:
//! - `registerSingleton(IDiffProviderFactoryService, WorkerBasedDiffProviderFactoryService, InstantiationType.Delayed)` do
//!   carregamento do módulo é a função [`register`], chamada por quem monta o workbench (e pelos testes), como nos demais
//!   serviços do porte. O proxy da instanciação atrasada é [`DiffProviderFactoryServiceProxy`].
//! - `diffAlgorithm?: 'legacy' | ... | IDocumentDiffProvider` é o [`DiffAlgorithmOrProvider`] (o nome é o `DiffAlgorithmName`
//!   de `editor_worker`; o provedor, um `Rc<dyn IDocumentDiffProvider>`). `this.diffAlgorithm !== newOptions.diffAlgorithm`
//!   compara nomes por valor e provedores por identidade.
//! - O `static diffCache` (um `Map` por processo, em ordem de inserção) é uma lista por thread; `set` de uma chave que existe
//!   troca o valor sem mudar a posição, como o `Map`.
//! - `async computeDiff` é o [`async_fn_maybe`] (o `return this.diffAlgorithm.computeDiff(...)` entrega a promessa do provedor
//!   aninhado), e `await` é `.await?`. `JSON.stringify` vira o `json_stringify` de `je_base::dynamic` (as opções sem
//!   `extendToSubwords` não o escrevem, como o `JSON.stringify` descarta `undefined`).
//! - `publicLog2` com a anotação de classificação do original é `public_log2` com um objeto [`Dynamic`]; a anotação só serve ao
//!   pipeline de telemetria do TypeScript.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn_maybe;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::stopwatch::StopWatch;
use je_instantiation::{InstantiationService, InstantiationType, Injectable, register_singleton, service_id, service_impl};
use je_platform::telemetry::telemetry::ITelemetryService;

use crate::cursor_common::CursorSimpleModel;
use crate::diff::document_diff_provider::{DocumentDiff, DocumentDiffProviderOptions, IDocumentDiffProvider};
use crate::diff::range_mapping::{DetailedLineRangeMapping, RangeMapping};
use crate::editor_worker::{DiffAlgorithmName, IEditorWorkerService};
use crate::line_range::LineRange;
use crate::text_model::TextModel;

/// `IDocumentDiffFactoryOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocumentDiffFactoryOptions {
    pub diff_algorithm: Option<DiffAlgorithmName>,
}

/// `IDiffProviderFactoryService`.
pub trait IDiffProviderFactoryService {
    fn create_diff_provider(&self, options: DocumentDiffFactoryOptions) -> Rc<dyn IDocumentDiffProvider>;
}

je_instantiation::delayed_proxy!(pub DiffProviderFactoryServiceProxy for dyn IDiffProviderFactoryService {
    methods {
        fn create_diff_provider(&self, options: DocumentDiffFactoryOptions) -> Rc<dyn IDocumentDiffProvider>;
    }
    events {}
});

service_id!(pub I_DIFF_PROVIDER_FACTORY_SERVICE: dyn IDiffProviderFactoryService = "diffProviderFactoryService", proxy DiffProviderFactoryServiceProxy);

/// `WorkerBasedDiffProviderFactoryService`.
pub struct WorkerBasedDiffProviderFactoryService {
    instantiation_service: Rc<InstantiationService>,
}

impl Injectable for WorkerBasedDiffProviderFactoryService {
    type Args = ();
    type Deps = (Rc<InstantiationService>,);

    fn create(_: (), (instantiation_service,): Self::Deps) -> Rc<Self> {
        Rc::new(Self { instantiation_service })
    }
}

service_impl!(WorkerBasedDiffProviderFactoryService => dyn IDiffProviderFactoryService);

impl IDiffProviderFactoryService for WorkerBasedDiffProviderFactoryService {
    fn create_diff_provider(&self, options: DocumentDiffFactoryOptions) -> Rc<dyn IDocumentDiffProvider> {
        let provider: Rc<WorkerBasedDocumentDiffProvider> = self
            .instantiation_service
            .create_instance::<WorkerBasedDocumentDiffProvider>(WorkerBasedDocumentDiffProviderOptions {
                diff_algorithm: options.diff_algorithm.map(DiffAlgorithmOrProvider::Name),
            });
        provider
    }
}

/// `registerSingleton(IDiffProviderFactoryService, WorkerBasedDiffProviderFactoryService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<WorkerBasedDiffProviderFactoryService>(I_DIFF_PROVIDER_FACTORY_SERVICE, InstantiationType::Delayed);
}

/// `DiffAlgorithmName | IDocumentDiffProvider`.
#[derive(Clone)]
pub enum DiffAlgorithmOrProvider {
    Name(DiffAlgorithmName),
    Provider(Rc<dyn IDocumentDiffProvider>),
}

impl DiffAlgorithmOrProvider {
    /// O `===` do original: nomes por valor, provedores por identidade.
    fn is_same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Name(a), Self::Name(b)) => a == b,
            (Self::Provider(a), Self::Provider(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// `IWorkerBasedDocumentDiffProviderOptions`.
#[derive(Clone, Default)]
pub struct WorkerBasedDocumentDiffProviderOptions {
    pub diff_algorithm: Option<DiffAlgorithmOrProvider>,
}

/// O resultado em cache de `WorkerBasedDocumentDiffProvider.diffCache`.
struct CachedDiff {
    result: DocumentDiff,
    context: String,
}

thread_local! {
    /// `WorkerBasedDocumentDiffProvider.diffCache`, na ordem de inserção.
    static DIFF_CACHE: RefCell<Vec<(String, CachedDiff)>> = const { RefCell::new(Vec::new()) };
}

fn diff_cache_get(key: &str, context: &str) -> Option<DocumentDiff> {
    DIFF_CACHE.with(|cache| cache.borrow().iter().find(|(k, c)| k == key && c.context == context).map(|(_, c)| c.result.clone()))
}

fn diff_cache_set(key: String, value: CachedDiff) {
    DIFF_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        // max 10 items in cache
        if cache.len() > 10 {
            cache.remove(0);
        }
        match cache.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => cache.push((key, value)),
        }
    });
}

/// `JSON.stringify(options)` das opções do cálculo.
fn options_to_json(options: &DocumentDiffProviderOptions) -> String {
    let mut entries: Vec<(&str, Dynamic)> = vec![
        ("ignoreTrimWhitespace", Dynamic::Bool(options.ignore_trim_whitespace)),
        ("maxComputationTimeMs", Dynamic::Number(options.max_computation_time_ms)),
        ("computeMoves", Dynamic::Bool(options.compute_moves)),
    ];
    if let Some(extend_to_subwords) = options.extend_to_subwords {
        entries.push(("extendToSubwords", Dynamic::Bool(extend_to_subwords)));
    }
    json_stringify(&Dynamic::object(entries)).unwrap_or_default()
}

/// `WorkerBasedDocumentDiffProvider`.
pub struct WorkerBasedDocumentDiffProvider {
    on_did_change_event_emitter: Rc<Emitter<()>>,
    diff_algorithm: RefCell<DiffAlgorithmOrProvider>,
    diff_algorithm_on_did_change_subscription: RefCell<Option<DisposableRef>>,
    editor_worker_service: Rc<dyn IEditorWorkerService>,
    telemetry_service: Rc<dyn ITelemetryService>,
}

impl Injectable for WorkerBasedDocumentDiffProvider {
    type Args = WorkerBasedDocumentDiffProviderOptions;
    type Deps = (Rc<dyn IEditorWorkerService>, Rc<dyn ITelemetryService>);

    fn create(options: Self::Args, (editor_worker_service, telemetry_service): Self::Deps) -> Rc<Self> {
        let this = Rc::new(Self {
            on_did_change_event_emitter: Emitter::new(),
            diff_algorithm: RefCell::new(DiffAlgorithmOrProvider::Name(DiffAlgorithmName::Advanced)),
            diff_algorithm_on_did_change_subscription: RefCell::new(None),
            editor_worker_service,
            telemetry_service,
        });
        this.set_options(&options);
        this
    }
}

impl WorkerBasedDocumentDiffProvider {
    pub fn set_options(&self, new_options: &WorkerBasedDocumentDiffProviderOptions) {
        let mut did_change = false;
        if let Some(new_algorithm) = &new_options.diff_algorithm {
            let is_different = !self.diff_algorithm.borrow().is_same(new_algorithm);
            if is_different {
                let previous = self.diff_algorithm_on_did_change_subscription.borrow_mut().take();
                if let Some(previous) = previous {
                    previous.dispose();
                }

                *self.diff_algorithm.borrow_mut() = new_algorithm.clone();
                if let DiffAlgorithmOrProvider::Provider(provider) = new_algorithm {
                    let emitter = self.on_did_change_event_emitter.clone();
                    let subscription = provider.on_did_change().listen(move |_| emitter.fire(()));
                    *self.diff_algorithm_on_did_change_subscription.borrow_mut() = Some(subscription);
                }
                did_change = true;
            }
        }
        if did_change {
            self.on_did_change_event_emitter.fire(());
        }
    }
}

impl IDisposable for WorkerBasedDocumentDiffProvider {
    fn dispose(&self) {
        let subscription = self.diff_algorithm_on_did_change_subscription.borrow_mut().take();
        if let Some(subscription) = subscription {
            subscription.dispose();
        }
        self.on_did_change_event_emitter.dispose();
    }
}

impl IDocumentDiffProvider for WorkerBasedDocumentDiffProvider {
    fn compute_diff(
        &self,
        original: &Rc<TextModel>,
        modified: &Rc<TextModel>,
        options: &DocumentDiffProviderOptions,
        cancellation_token: &CancellationToken,
    ) -> Promise<DocumentDiff> {
        let algorithm = self.diff_algorithm.borrow().clone();
        let editor_worker_service = self.editor_worker_service.clone();
        let telemetry_service = self.telemetry_service.clone();
        let (original, modified, options, cancellation_token) = (original.clone(), modified.clone(), *options, cancellation_token.clone());

        async_fn_maybe(async move {
            let diff_algorithm = match algorithm {
                DiffAlgorithmOrProvider::Provider(provider) => {
                    return Ok(MaybePromise::Promise(provider.compute_diff(&original, &modified, &options, &cancellation_token)));
                }
                DiffAlgorithmOrProvider::Name(name) => name,
            };

            if original.is_disposed() || modified.is_disposed() {
                // TODO@hediet
                return Ok(MaybePromise::Value(DocumentDiff { changes: Vec::new(), identical: true, quit_early: false, moves: Vec::new() }));
            }

            // This significantly speeds up the case when the original file is empty
            if original.get_line_count() == 1 && original.get_line_max_column(1) == 1 {
                if modified.get_line_count() == 1 && modified.get_line_max_column(1) == 1 {
                    return Ok(MaybePromise::Value(DocumentDiff { changes: Vec::new(), identical: true, quit_early: false, moves: Vec::new() }));
                }

                return Ok(MaybePromise::Value(DocumentDiff {
                    changes: vec![DetailedLineRangeMapping::new(
                        LineRange::new(1, 2),
                        LineRange::new(1, modified.get_line_count() + 1),
                        Some(vec![RangeMapping::new(original.get_full_model_range(), modified.get_full_model_range())]),
                    )],
                    identical: false,
                    quit_early: false,
                    moves: Vec::new(),
                }));
            }

            let uri_key = json_stringify(&Dynamic::array([
                Dynamic::string(&original.uri().to_string(false).to_string_lossy()),
                Dynamic::string(&modified.uri().to_string(false).to_string_lossy()),
            ]))
            .unwrap_or_default();
            let context = json_stringify(&Dynamic::array([
                Dynamic::string(original.id()),
                Dynamic::string(modified.id()),
                Dynamic::Number(original.get_alternative_version_id() as f64),
                Dynamic::Number(modified.get_alternative_version_id() as f64),
                Dynamic::string(&options_to_json(&options)),
            ]))
            .unwrap_or_default();
            if let Some(cached) = diff_cache_get(&uri_key, &context) {
                return Ok(MaybePromise::Value(cached));
            }

            let sw = StopWatch::create(None);
            let result = editor_worker_service.compute_diff(&original.uri(), &modified.uri(), &options, diff_algorithm).await?;
            let time_ms = sw.elapsed();

            telemetry_service.public_log2(
                "diffEditor.computeDiff",
                Some(Dynamic::object([
                    ("timeMs", Dynamic::Number(time_ms)),
                    ("timedOut", Dynamic::Bool(result.as_ref().is_none_or(|r| r.quit_early))),
                    (
                        "detectedMoves",
                        Dynamic::Number(if options.compute_moves { result.as_ref().map_or(0, |r| r.moves.len()) as f64 } else { -1.0 }),
                    ),
                ])),
            );

            if cancellation_token.is_cancellation_requested() {
                // Text models might be disposed!
                return Ok(MaybePromise::Value(DocumentDiff { changes: Vec::new(), identical: false, quit_early: true, moves: Vec::new() }));
            }

            let Some(result) = result else {
                return Err(Error::new("no diff result available"));
            };

            diff_cache_set(uri_key, CachedDiff { result: result.clone(), context });
            Ok(MaybePromise::Value(result))
        })
    }

    fn on_did_change(&self) -> Event<()> {
        self.on_did_change_event_emitter.event()
    }
}
