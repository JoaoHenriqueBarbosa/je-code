//! Porte de `src/vs/workbench/contrib/editTelemetry/browser/telemetry/editTracker.ts`.
//!
//! Representação: o `Map`/`Set` do JavaScript, que iteram na ordem de inserção, são `IndexMap`/`IndexSet` onde a
//! ordem é observável (`getAllKeys`, `releaseExternalEditCorrelations`) e `HashMap`/`HashSet` nos demais. Os parâmetros
//! opcionais do construtor e de `getTrackedRanges` são explícitos (`None`, `false`, [`ExternalEditCorrelationPolicy::Suppress`]).
//! Os callbacks guardam um `Weak` do rastreador, que vive no `Rc` devolvido por [`DocumentEditSourceTracker::new`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use indexmap::{IndexMap, IndexSet};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::observable::{IReader, Observable, ObservableSignal, observable_signal, run_on_change};
use je_base::promise::Promise;
use je_editor::edits::edit::{BaseEdit, BaseReplacement};
use je_editor::edits::string_edit::{AnnotatedStringEdit, BaseStringEdit};
use je_editor::offset_range::OffsetRange;
use je_editor::text::abstract_text::StringText;
use je_editor::text_model_edit_source::TextModelEditSource;

use super::agent_host_edit_marker_service::{IExternalEditCorrelation, IExternalEditCorrelationResolution};
use crate::contrib::edit_telemetry::browser::helpers::document_with_annotated_edits::{
    AnnotatedEdit, EditKeySourceData, EditSource, EditSourceBase, IDocumentWithAnnotatedEdits,
};

const EXTERNAL_OBSERVATION_KEY_PREFIX: &str = "external-observation:";

/// `'suppress' | 'reattribute'`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExternalEditCorrelationPolicy {
    #[default]
    Suppress,
    Reattribute,
}

/// O estado mutável do rastreador.
struct TrackerState {
    edits: AnnotatedStringEdit<EditKeySourceData>,
    pending_external_edits: AnnotatedStringEdit<EditKeySourceData>,
    representative_per_key: HashMap<String, Rc<TextModelEditSource>>,
    sum_added_characters_per_key: IndexMap<String, usize>,
    external_observation_ids: IndexSet<String>,
    ignored_initial_external_observation_ids: HashSet<String>,
    suppressed_external_observation_ids: HashSet<String>,
    invalidated_external_observation_ids: HashSet<String>,
    reattributed_external_observations: HashMap<String, Rc<TextModelEditSource>>,
}

impl Default for TrackerState {
    fn default() -> Self {
        Self {
            edits: AnnotatedStringEdit::empty(),
            pending_external_edits: AnnotatedStringEdit::empty(),
            representative_per_key: HashMap::new(),
            sum_added_characters_per_key: IndexMap::new(),
            external_observation_ids: IndexSet::new(),
            ignored_initial_external_observation_ids: HashSet::new(),
            suppressed_external_observation_ids: HashSet::new(),
            invalidated_external_observation_ids: HashSet::new(),
            reattributed_external_observations: HashMap::new(),
        }
    }
}

/// Tracks a single document.
pub struct DocumentEditSourceTracker<T: 'static = ()> {
    base: Rc<Disposable>,
    doc: Rc<dyn IDocumentWithAnnotatedEdits<EditKeySourceData>>,
    pub data: T,
    external_edit_correlation: Option<Rc<dyn IExternalEditCorrelation>>,
    external_edit_correlation_policy: ExternalEditCorrelationPolicy,

    state: RefCell<TrackerState>,
    update: Rc<ObservableSignal<()>>,
    document_store: Rc<DisposableStore>,
}

impl<T: 'static> DocumentEditSourceTracker<T> {
    pub fn new(
        doc: Rc<dyn IDocumentWithAnnotatedEdits<EditKeySourceData>>,
        data: T,
        external_edit_correlation: Option<Rc<dyn IExternalEditCorrelation>>,
        external_edit_correlation_policy: ExternalEditCorrelationPolicy,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let document_store = DisposableStore::new();
        base.register(document_store.clone());

        let this = Rc::new(Self {
            base,
            doc,
            data,
            external_edit_correlation,
            external_edit_correlation_policy,
            state: RefCell::new(TrackerState::default()),
            update: observable_signal::<()>("update"),
            document_store,
        });

        let me = Rc::downgrade(&this);
        let weak = me.clone();
        this.document_store.add(run_on_change::<Rc<StringText>, AnnotatedEdit<EditKeySourceData>>(this.doc.value(), move |val, prev_val, edits| {
            if let Some(this) = weak.upgrade() {
                this.handle_document_change(val, prev_val, edits);
            }
        }));
        if let Some(correlation) = &this.external_edit_correlation {
            if let Some(on_did_resolve) = correlation.on_did_resolve() {
                let weak = me.clone();
                this.base.register(on_did_resolve.listen(move |resolution| {
                    if let Some(this) = weak.upgrade() {
                        this.resolve_external_observation(resolution);
                    }
                }));
            } else {
                let weak = me.clone();
                this.base.register(correlation.on_did_suppress().listen(move |observation_id| {
                    let Some(this) = weak.upgrade() else { return };
                    let is_known = this.state.borrow().external_observation_ids.contains(observation_id);
                    if is_known && this.external_edit_correlation_policy == ExternalEditCorrelationPolicy::Suppress {
                        this.state.borrow_mut().suppressed_external_observation_ids.insert(observation_id.clone());
                        this.update.trigger(None, ());
                    }
                }));
            }
            let weak = me.clone();
            this.base.register(correlation.on_did_invalidate().listen(move |observation_id| {
                let Some(this) = weak.upgrade() else { return };
                if this.state.borrow().external_observation_ids.contains(observation_id) {
                    {
                        let mut state = this.state.borrow_mut();
                        state.suppressed_external_observation_ids.remove(observation_id);
                        state.reattributed_external_observations.remove(observation_id);
                        state.invalidated_external_observation_ids.insert(observation_id.clone());
                    }
                    this.update.trigger(None, ());
                }
            }));
        }
        this
    }

    /// O callback do `runOnChange` do construtor.
    fn handle_document_change(&self, val: &Rc<StringText>, prev_val: &Rc<StringText>, edits: &[AnnotatedEdit<EditKeySourceData>]) {
        let mut e_composed = AnnotatedEdit::compose_all(edits);
        if e_composed.replacements().iter().all(|e| e.data.source.category() == "external") {
            if self.state.borrow().edits.is_empty() && self.external_edit_correlation.is_none() {
                // Ignore initial external edits
            } else {
                if let Some(correlation) = &self.external_edit_correlation {
                    let observation_id = correlation.register(&prev_val.value, &val.value);
                    {
                        let mut state = self.state.borrow_mut();
                        state.external_observation_ids.insert(observation_id.clone());
                        if state.edits.is_empty() {
                            state.ignored_initial_external_observation_ids.insert(observation_id.clone());
                        }
                    }
                    let resolution = correlation.get_resolution(&observation_id);
                    if let Some(resolution) = resolution {
                        self.resolve_external_observation(&resolution);
                    } else if self.external_edit_correlation_policy == ExternalEditCorrelationPolicy::Suppress && correlation.is_suppressed(&observation_id) {
                        self.state.borrow_mut().suppressed_external_observation_ids.insert(observation_id.clone());
                    }
                    let key = format!("{EXTERNAL_OBSERVATION_KEY_PREFIX}{observation_id}");
                    e_composed = e_composed.map_data(|replacement| {
                        EditKeySourceData::new(key.clone(), replacement.data.source.clone(), replacement.data.representative.clone())
                    });
                }
                // queue pending external edits
                let mut state = self.state.borrow_mut();
                let pending = state.pending_external_edits.compose(&e_composed);
                state.pending_external_edits = pending;
            }
        } else {
            let pending = self.state.borrow().pending_external_edits.clone();
            if !pending.is_empty() {
                self.apply_edit(&pending);
                self.state.borrow_mut().pending_external_edits = AnnotatedStringEdit::empty();
            }
            self.apply_edit(&e_composed);
        }

        self.update.trigger(None, ());
    }

    pub fn release_external_edit_correlations(&self) {
        let ids: Vec<String> = self.state.borrow().external_observation_ids.iter().cloned().collect();
        for observation_id in &ids {
            if let Some(correlation) = &self.external_edit_correlation {
                correlation.release(observation_id);
            }
        }
        self.state.borrow_mut().external_observation_ids.clear();
    }

    pub fn stop_tracking(&self) {
        self.document_store.clear();
    }

    pub fn wait_for_external_edit_correlations(&self, timeout_ms: u64) -> Promise<()> {
        let correlation = self.external_edit_correlation.clone();
        let ids: Vec<String> = self.state.borrow().external_observation_ids.iter().cloned().collect();
        async_fn(async move {
            match correlation {
                Some(correlation) => correlation.wait_for_resolution(&ids, timeout_ms).await?,
                None => Promise::resolved(()).await?,
            }
            Ok(())
        })
    }

    fn apply_edit(&self, e: &AnnotatedStringEdit<EditKeySourceData>) {
        let mut state = self.state.borrow_mut();
        for r in e.replacements() {
            let existing = match state.sum_added_characters_per_key.get(&r.data.key).copied() {
                Some(existing) => existing,
                None => {
                    state.representative_per_key.insert(r.data.key.clone(), r.data.representative.clone());
                    0
                }
            };
            let new_count = existing + r.get_new_length() as usize;
            state.sum_added_characters_per_key.insert(r.data.key.clone(), new_count);
        }

        let composed = state.edits.compose(e);
        state.edits = composed;
    }

    pub fn wait_for_queue(&self) -> Promise<()> {
        let doc = self.doc.clone();
        async_fn(async move {
            doc.wait_for_queue().await?;
            Ok(())
        })
    }

    pub fn get_total_inserted_characters_count(&self, key: &str, include_suppressed: bool) -> usize {
        if !self.should_include_key(key, include_suppressed) {
            return 0;
        }
        self.state.borrow().sum_added_characters_per_key.get(key).copied().unwrap_or(0)
    }

    pub fn get_all_keys(&self, include_suppressed: bool) -> Vec<String> {
        let keys: Vec<String> = self.state.borrow().sum_added_characters_per_key.keys().cloned().collect();
        keys.into_iter().filter(|key| self.should_include_key(key, include_suppressed)).collect()
    }

    pub fn get_representative(&self, key: &str) -> Option<Rc<TextModelEditSource>> {
        let state = self.state.borrow();
        if let Some(observation_id) = get_external_observation_id(key) {
            return state.reattributed_external_observations.get(observation_id).or_else(|| state.representative_per_key.get(key)).cloned();
        }
        state.representative_per_key.get(key).cloned()
    }

    pub fn apply_pending_external_edits(&self) {
        let pending = self.state.borrow().pending_external_edits.clone();
        if pending.is_empty() {
            return;
        }
        self.apply_edit(&pending);
        self.state.borrow_mut().pending_external_edits = AnnotatedStringEdit::empty();
        self.update.trigger(None, ());
    }

    pub fn get_tracked_ranges(&self, reader: Option<&dyn IReader>, include_suppressed: bool) -> Vec<TrackedEdit> {
        self.update.read_opt(reader);
        let edits = self.state.borrow().edits.clone();
        let ranges = edits.get_new_ranges();
        ranges
            .into_iter()
            .enumerate()
            .map(|(idx, r)| {
                let e = &edits.replacements()[idx];
                let representative = self.get_representative(&e.data.key).unwrap_or_else(|| e.data.representative.clone());
                let source =
                    if Rc::ptr_eq(&representative, &e.data.representative) { e.data.source.clone() } else { EditSourceBase::create(&representative) };
                TrackedEdit::new(e.replace_range, r, e.data.key.clone(), source, representative)
            })
            .filter(|edit| self.should_include_key(&edit.source_key, include_suppressed))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.get_all_keys(false).is_empty()
    }

    fn should_include_key(&self, key: &str, include_suppressed: bool) -> bool {
        let Some(observation_id) = get_external_observation_id(key) else {
            return true;
        };
        let state = self.state.borrow();
        if state.reattributed_external_observations.contains_key(observation_id) {
            return true;
        }
        if state.invalidated_external_observation_ids.contains(observation_id) {
            return true;
        }
        let is_suppressed = state.suppressed_external_observation_ids.contains(observation_id);
        if state.ignored_initial_external_observation_ids.contains(observation_id) {
            if self.external_edit_correlation_policy == ExternalEditCorrelationPolicy::Reattribute {
                return true;
            }
            return include_suppressed && is_suppressed;
        }
        include_suppressed || !is_suppressed
    }

    fn resolve_external_observation(&self, resolution: &IExternalEditCorrelationResolution) {
        {
            let mut state = self.state.borrow_mut();
            if !state.external_observation_ids.contains(&resolution.id) {
                return;
            }
            state.invalidated_external_observation_ids.remove(&resolution.id);
            if self.external_edit_correlation_policy == ExternalEditCorrelationPolicy::Suppress {
                state.suppressed_external_observation_ids.insert(resolution.id.clone());
            } else if let Some(source) = &resolution.source {
                state.reattributed_external_observations.insert(resolution.id.clone(), source.clone());
            }
        }
        self.update.trigger(None, ());
    }

    /// `_getDebugVisualization()`.
    pub fn get_debug_visualization(&self) -> DebugVisualization {
        let ranges = self.get_tracked_ranges(None, false);
        let txt = self.doc.value().get().value.clone();

        DebugVisualization {
            file_extension: "text.w",
            value: txt,
            decorations: ranges.iter().map(|r| DebugDecoration { range: [r.range.start, r.range.end_exclusive], color: r.source.get_color() }).collect(),
        }
    }
}

impl<T: 'static> IDisposable for DocumentEditSourceTracker<T> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O objeto de `_getDebugVisualization()` (`$fileExtension`, `value`, `decorations`).
pub struct DebugVisualization {
    pub file_extension: &'static str,
    pub value: JsString,
    pub decorations: Vec<DebugDecoration>,
}

/// Uma entrada de `decorations`.
pub struct DebugDecoration {
    pub range: [i32; 2],
    pub color: &'static str,
}

fn get_external_observation_id(key: &str) -> Option<&str> {
    key.strip_prefix(EXTERNAL_OBSERVATION_KEY_PREFIX)
}

#[derive(Clone, Debug)]
pub struct TrackedEdit {
    pub original_range: OffsetRange,
    pub range: OffsetRange,
    pub source_key: String,
    pub source: EditSource,
    pub source_representative: Rc<TextModelEditSource>,
}

impl TrackedEdit {
    pub fn new(original_range: OffsetRange, range: OffsetRange, source_key: String, source: EditSource, source_representative: Rc<TextModelEditSource>) -> Self {
        Self { original_range, range, source_key, source, source_representative }
    }
}
