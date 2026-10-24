//! Porte de `src/vs/workbench/contrib/editTelemetry/browser/aiContributionFeature.ts`.
//!
//! Representação:
//! - `AiContributionLevel` (união de textos) é o enum [`AiContributionLevel`], que lê e escreve o texto do argumento do
//!   comando (`'chatAndAgent' | 'all'`);
//! - os argumentos e o retorno dos comandos `_aiEdits.*` são [`Dynamic`] (ver `commands`); `UriComponents[]` é um array
//!   cujos itens são o objeto serializado do `URI` ou o próprio `URI` (um objeto com o `URI` como carga nativa), e o
//!   `URI.revive` de cada item é [`revive_uri`];
//! - o `ResourceMap<TrackerEntry>` e o `ResourceMap<AnnotatedDocument>` ficam num estado compartilhado por `Rc` com o fecho
//!   do `autorun` e com os fechos dos comandos, que não guardam o `Disposable` da feature (não há ciclo de `Rc`);
//! - `for (const [uri, entry] of this._trackers)` com remoção dentro do laço itera sobre as chaves copiadas antes, e o
//!   descarte da loja do rastreador acontece fora do empréstimo do mapa;
//! - `new DocumentEditSourceTracker(docWithJustReason, undefined)` usa os valores padrão do original: sem correlação de
//!   edição externa e política `suppress`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::map::ResourceMap;
use je_base::observable::{IObservable, Observable, autorun};
use je_base::uri::{URI, UriComponents};
use je_instantiation::Injectable;
use je_platform::commands::commands::{CommandHandler, commands_registry};

use super::helpers::annotated_documents::{AnnotatedDocument, IAnnotatedDocuments};
use super::helpers::document_with_annotated_edits::create_doc_with_just_reason;
use super::telemetry::edit_tracker::{DocumentEditSourceTracker, ExternalEditCorrelationPolicy};

/// `AiContributionLevel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiContributionLevel {
    ChatAndAgent,
    All,
}

impl AiContributionLevel {
    /// O texto do argumento do comando.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChatAndAgent => "chatAndAgent",
            Self::All => "all",
        }
    }

    /// `level === 'all'` sobre o argumento do comando; qualquer outro valor é `chatAndAgent`.
    fn from_dynamic(value: Option<&Dynamic>) -> Self {
        match value {
            Some(Dynamic::String(text)) if text.to_string_lossy() == Self::All.as_str() => Self::All,
            _ => Self::ChatAndAgent,
        }
    }
}

/// `TrackerEntry`.
struct TrackerEntry {
    tracker_store: Rc<DisposableStore>,
    tracker: Rc<DocumentEditSourceTracker<()>>,
}

/// `_trackers` e `_documentsByUri`.
#[derive(Default)]
struct State {
    trackers: RefCell<ResourceMap<TrackerEntry>>,
    documents_by_uri: RefCell<ResourceMap<Rc<AnnotatedDocument>>>,
}

impl State {
    fn create_tracker_entry(doc: &AnnotatedDocument) -> TrackerEntry {
        let tracker_store = DisposableStore::new();
        let doc_with_just_reason = create_doc_with_just_reason(&doc.document_with_annotations, &tracker_store);
        let tracker = tracker_store.add_rc(DocumentEditSourceTracker::new(doc_with_just_reason, (), None, ExternalEditCorrelationPolicy::Suppress));
        TrackerEntry { tracker_store, tracker }
    }

    /// O corpo do `autorun` do construtor.
    fn sync_documents(&self, docs: &[Rc<AnnotatedDocument>]) {
        let mut active_uris: ResourceMap<bool> = ResourceMap::new();

        for doc in docs {
            let uri = doc.document.uri();
            active_uris.set(uri.clone(), true);
            self.documents_by_uri.borrow_mut().set(uri.clone(), doc.clone());

            if !self.trackers.borrow().has(uri) {
                let entry = Self::create_tracker_entry(doc);
                self.trackers.borrow_mut().set(uri.clone(), entry);
            }
        }

        let stale: Vec<URI> = self.trackers.borrow().keys().filter(|uri| !active_uris.has(uri)).cloned().collect();
        for uri in stale {
            let store = self.trackers.borrow().get(&uri).map(|entry| entry.tracker_store.clone());
            if let Some(store) = store {
                store.dispose();
            }
            self.trackers.borrow_mut().delete(&uri);
            self.documents_by_uri.borrow_mut().delete(&uri);
        }
    }

    fn dispose_trackers(&self) {
        let stores: Vec<Rc<DisposableStore>> = self.trackers.borrow().values().map(|entry| entry.tracker_store.clone()).collect();
        for store in stores {
            store.dispose();
        }
    }

    fn has_ai_contributions(&self, resources: &[URI], level: AiContributionLevel) -> bool {
        for resource in resources {
            let tracker = self.trackers.borrow().get(resource).map(|entry| entry.tracker.clone());
            if let Some(tracker) = tracker {
                for edit in tracker.get_tracked_ranges(None, false) {
                    if edit.source.category() == "ai" && (level == AiContributionLevel::All || edit.source.feature() == Some("chat")) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn clear_ai_contributions(&self, resources: Option<&[URI]>) {
        let uris: Vec<URI> = match resources {
            Some(resources) => resources.to_vec(),
            None => self.trackers.borrow().keys().cloned().collect(),
        };
        for uri in uris {
            let store = self.trackers.borrow().get(&uri).map(|entry| entry.tracker_store.clone());
            let Some(store) = store else { continue };
            store.dispose();
            let doc = self.documents_by_uri.borrow().get(&uri).cloned();
            if let Some(doc) = doc {
                let entry = Self::create_tracker_entry(&doc);
                self.trackers.borrow_mut().set(uri, entry);
            } else {
                self.trackers.borrow_mut().delete(&uri);
                self.documents_by_uri.borrow_mut().delete(&uri);
            }
        }
    }
}

/// `URI.revive(resource)` de um item de `UriComponents[]`: o `URI` que veio como carga nativa do objeto, ou o `URI` dos
/// componentes serializados.
pub fn revive_uri(resource: &Dynamic) -> Option<URI> {
    let Dynamic::Object(object) = resource else {
        return None;
    };
    if let Some(uri) = object.host::<URI>() {
        return Some((*uri).clone());
    }
    let part = |key: &str| match resource.get_str(key) {
        Dynamic::String(text) => Some(text),
        _ => None,
    };
    Some(URI::from(
        &UriComponents { scheme: part("scheme"), authority: part("authority"), path: part("path"), query: part("query"), fragment: part("fragment") },
        false,
    ))
}

/// O `URI` na forma do argumento `UriComponents` dos comandos (o inverso de [`revive_uri`]).
pub fn uri_argument(uri: &URI) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(uri.clone())))
}

/// `resources: UriComponents[]` como lista de `URI`.
fn revive_resources(value: Option<&Dynamic>) -> Vec<URI> {
    match value {
        Some(Dynamic::Array(items)) => items.to_vec().iter().filter_map(revive_uri).collect(),
        _ => Vec::new(),
    }
}

/// Tracks AI-generated edits across open documents using the edit telemetry pipeline.
pub struct AiContributionFeature {
    base: Rc<Disposable>,
    state: Rc<State>,
}

impl Injectable for AiContributionFeature {
    /// `annotatedDocuments`.
    type Args = Rc<dyn IAnnotatedDocuments>;
    type Deps = ();

    fn create(annotated_documents: Self::Args, _: Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let state = Rc::new(State::default());

        let documents: IObservable<Rc<Vec<Rc<AnnotatedDocument>>>> = annotated_documents.documents();
        let autorun_state = state.clone();
        base.register(autorun(move |reader| {
            let docs = documents.read(reader);
            autorun_state.sync_documents(&docs);
        }));

        let has_state = state.clone();
        let has_handler: CommandHandler = Rc::new(move |_accessor, args| {
            let resources = revive_resources(args.first());
            let level = AiContributionLevel::from_dynamic(args.get(1));
            Dynamic::Bool(has_state.has_ai_contributions(&resources, level))
        });
        base.register(commands_registry().register_command_handler("_aiEdits.hasAiContributions", Some(has_handler)));

        let clear_state = state.clone();
        let clear_handler: CommandHandler = Rc::new(move |_accessor, args| {
            clear_state.clear_ai_contributions(Some(&revive_resources(args.first())));
            Dynamic::Undefined
        });
        base.register(commands_registry().register_command_handler("_aiEdits.clearAiContributions", Some(clear_handler)));

        let clear_all_state = state.clone();
        let clear_all_handler: CommandHandler = Rc::new(move |_accessor, _args| {
            clear_all_state.clear_ai_contributions(None);
            Dynamic::Undefined
        });
        base.register(commands_registry().register_command_handler("_aiEdits.clearAllAiContributions", Some(clear_all_handler)));

        Rc::new(Self { base, state })
    }
}

impl IDisposable for AiContributionFeature {
    fn dispose(&self) {
        self.state.dispose_trackers();
        self.base.dispose();
    }
}
