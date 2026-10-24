//! Porte de `src/vs/workbench/contrib/editTelemetry/test/browser/editTracker.test.ts` (suíte `DocumentEditSourceTracker`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no início de cada caso; `disposables.add(x)` é
//!   `add_rc`. `new DocumentEditSourceTracker(document, undefined, correlation, policy)` é
//!   `DocumentEditSourceTracker::new(document, (), correlation, policy)`; o terceiro parâmetro ausente é `None` e o quarto
//!   ausente é [`ExternalEditCorrelationPolicy::Suppress`].
//! - `TestAnnotatedDocument` é a struct de mesmo nome sobre um `ObservableValue` com o delta `AnnotatedEdit`;
//!   `edit.mapData(() => data)` é `map_data(|_| data.clone())`.
//! - `snapshot` devolve [`Snapshot`]; `props.$$requestId` é `metadata_value("$$requestId")`. O `localeCompare` da
//!   ordenação é a ordem de bytes (as chaves comparadas são ASCII e a ordem é a mesma).
//! - `assert.deepStrictEqual` de objetos literais é `assert_eq!` sobre structs ou tuplas locais com os mesmos campos, na
//!   mesma ordem. `undefined` é `None`. `lastObservationId!` é `last_observation_id()`, que falha se ausente.
//! - `TestExternalEditCorrelation` guarda os emissores e as resoluções; o `Emitter` não é descartado, como no original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{IObservable, Observable, Settable, observable_value};
use je_base::promise::Promise;
use je_editor::edits::string_edit::{BaseStringEdit, StringEdit};
use je_editor::offset_range::OffsetRange;
use je_editor::text::abstract_text::StringText;
use je_editor::text_model_edit_source::{AgentHostChatApplyEditsData, ChatApplyEditsData, EditSources, TextModelEditSource};
use je_workbench::contrib::edit_telemetry::browser::helpers::document_with_annotated_edits::{
    AnnotatedEdit, EditKeySourceData, EditSourceData, IDocumentWithAnnotatedEdits,
};
use je_workbench::contrib::edit_telemetry::browser::telemetry::agent_host_edit_marker_service::{IExternalEditCorrelation, IExternalEditCorrelationResolution};
use je_workbench::contrib::edit_telemetry::browser::telemetry::edit_tracker::{DocumentEditSourceTracker, ExternalEditCorrelationPolicy};

// region: infraestrutura do caso

/// `TestAnnotatedDocument`.
struct TestAnnotatedDocument {
    base: Rc<Disposable>,
    value: Rc<ObservableValue<Rc<StringText>, AnnotatedEdit<EditKeySourceData>>>,
}

impl TestAnnotatedDocument {
    fn new(initial_value: &str) -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), value: observable_value("value", Rc::new(StringText::new(initial_value))) })
    }

    /// `apply(edit, source)`.
    fn apply(&self, edit: &StringEdit, source: TextModelEditSource) {
        let data = EditSourceData::new(Rc::new(source)).to_edit_source_data();
        let next = Rc::new(edit.apply_on_text(&self.value.get()));
        self.value.set_with_change(next, None, AnnotatedEdit { edit: edit.map_data(|_| data.clone()) });
    }
}

impl IDocumentWithAnnotatedEdits<EditKeySourceData> for TestAnnotatedDocument {
    fn value(&self) -> IObservable<Rc<StringText>> {
        self.value.to_observable()
    }

    fn wait_for_queue(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

impl IDisposable for TestAnnotatedDocument {
    fn dispose(&self) {
        self.base.dispose();
    }
}

fn chat_edit_source(model_id: &str, request_id: &str) -> TextModelEditSource {
    EditSources::chat_apply_edits(&ChatApplyEditsData {
        model_id: Some(model_id.to_owned()),
        session_id: Some("session-1".to_owned()),
        request_id: Some(request_id.to_owned()),
        language_id: "typescript".to_owned(),
        mode: Some("agent".to_owned()),
        ..Default::default()
    })
}

fn agent_host_edit_source(model_id: &str, request_id: &str, harness: &str) -> TextModelEditSource {
    EditSources::agent_host_chat_apply_edits(&AgentHostChatApplyEditsData {
        model_id: Some(model_id.to_owned()),
        auto_tier: None,
        fusion_model: None,
        session_id: "session-1".to_owned(),
        agent_session_id: None,
        chat_session_id: None,
        request_id: request_id.to_owned(),
        harness: harness.to_owned(),
    })
}

/// O objeto `{ key, delta, retained, requestId }` de `snapshot`.
#[derive(Debug, PartialEq)]
struct Snapshot {
    key: String,
    delta: usize,
    retained: i64,
    request_id: Option<String>,
}

fn snap(key: &str, delta: usize, retained: i64, request_id: Option<&str>) -> Snapshot {
    Snapshot { key: key.to_owned(), delta, retained, request_id: request_id.map(str::to_owned) }
}

type Tracker = Rc<DocumentEditSourceTracker<()>>;

fn snapshot(tracker: &Tracker) -> Vec<Snapshot> {
    snapshot_with(tracker, false)
}

fn snapshot_with(tracker: &Tracker, include_suppressed: bool) -> Vec<Snapshot> {
    let mut retained: HashMap<String, i64> = HashMap::new();
    for range in tracker.get_tracked_ranges(None, include_suppressed) {
        *retained.entry(range.source_key.clone()).or_insert(0) += i64::from(range.range.length());
    }
    let mut result: Vec<Snapshot> = tracker
        .get_all_keys(include_suppressed)
        .into_iter()
        .map(|key| Snapshot {
            delta: tracker.get_total_inserted_characters_count(&key, include_suppressed),
            retained: retained.get(&key).copied().unwrap_or(0),
            request_id: tracker.get_representative(&key).and_then(|r| r.metadata_value("$$requestId").map(str::to_owned)),
            key,
        })
        .collect();
    result.sort_by(|a, b| a.key.cmp(&b.key));
    result
}

/// `TestExternalEditCorrelation`.
struct TestExternalEditCorrelation {
    on_did_suppress: Rc<Emitter<String>>,
    on_did_resolve: Rc<Emitter<IExternalEditCorrelationResolution>>,
    on_did_invalidate: Rc<Emitter<String>>,
    resolutions: RefCell<HashMap<String, IExternalEditCorrelationResolution>>,
    sequence: Cell<u32>,
    last_observation_id: RefCell<Option<String>>,
}

impl TestExternalEditCorrelation {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            on_did_suppress: Emitter::new(),
            on_did_resolve: Emitter::new(),
            on_did_invalidate: Emitter::new(),
            resolutions: RefCell::new(HashMap::new()),
            sequence: Cell::new(0),
            last_observation_id: RefCell::new(None),
        })
    }

    /// `lastObservationId!`.
    fn last_observation_id(&self) -> String {
        self.last_observation_id.borrow().clone().expect("há uma observação registrada")
    }

    fn suppress(&self, id: &str) {
        let resolution = IExternalEditCorrelationResolution { id: id.to_owned(), source: None };
        self.resolutions.borrow_mut().insert(id.to_owned(), resolution.clone());
        self.on_did_suppress.fire(id.to_owned());
        self.on_did_resolve.fire(resolution);
    }

    fn resolve(&self, id: &str, source: TextModelEditSource) {
        let resolution = IExternalEditCorrelationResolution { id: id.to_owned(), source: Some(Rc::new(source)) };
        self.resolutions.borrow_mut().insert(id.to_owned(), resolution.clone());
        self.on_did_suppress.fire(id.to_owned());
        self.on_did_resolve.fire(resolution);
    }

    fn invalidate(&self, id: &str) {
        self.resolutions.borrow_mut().remove(id);
        self.on_did_invalidate.fire(id.to_owned());
    }
}

impl IExternalEditCorrelation for TestExternalEditCorrelation {
    fn on_did_suppress(&self) -> Event<String> {
        self.on_did_suppress.event()
    }

    fn on_did_resolve(&self) -> Option<Event<IExternalEditCorrelationResolution>> {
        Some(self.on_did_resolve.event())
    }

    fn on_did_invalidate(&self) -> Event<String> {
        self.on_did_invalidate.event()
    }

    fn register(&self, _before: &JsString, _after: &JsString) -> String {
        self.sequence.set(self.sequence.get() + 1);
        let id = format!("observation-{}", self.sequence.get());
        *self.last_observation_id.borrow_mut() = Some(id.clone());
        id
    }

    fn is_suppressed(&self, id: &str) -> bool {
        self.resolutions.borrow().contains_key(id)
    }

    fn get_resolution(&self, id: &str) -> Option<IExternalEditCorrelationResolution> {
        self.resolutions.borrow().get(id).cloned()
    }

    fn release(&self, id: &str) {
        self.resolutions.borrow_mut().remove(id);
    }
}

/// `new DocumentEditSourceTracker(document, undefined, correlation?, policy?)`.
fn new_tracker(
    disposables: &common::LeakGuard,
    document: &Rc<TestAnnotatedDocument>,
    correlation: Option<&Rc<TestExternalEditCorrelation>>,
    policy: ExternalEditCorrelationPolicy,
) -> Tracker {
    let document: Rc<dyn IDocumentWithAnnotatedEdits<EditKeySourceData>> = document.clone();
    let correlation = correlation.map(|c| -> Rc<dyn IExternalEditCorrelation> { c.clone() });
    disposables.add_rc(DocumentEditSourceTracker::new(document, (), correlation, policy))
}

fn external_edit() -> StringEdit {
    StringEdit::replace(OffsetRange::of_length(7), "external")
}

// endregion

#[test]
fn ignores_an_initial_external_edit() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = disposables.add_rc(TestAnnotatedDocument::new("initial"));
    let tracker = new_tracker(&disposables, &document, None, ExternalEditCorrelationPolicy::Suppress);

    document.apply(&external_edit(), EditSources::reload_from_disk());

    assert_eq!(snapshot(&tracker), vec![]);
}

#[test]
fn retains_a_matched_initial_external_edit_only_for_fallback() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = disposables.add_rc(TestAnnotatedDocument::new("initial"));
    let correlation = TestExternalEditCorrelation::new();
    let tracker = new_tracker(&disposables, &document, Some(&correlation), ExternalEditCorrelationPolicy::Suppress);

    document.apply(&external_edit(), EditSources::reload_from_disk());
    tracker.apply_pending_external_edits();
    let before_suppression = snapshot_with(&tracker, true);
    correlation.suppress(&correlation.last_observation_id());
    let fallback = snapshot_with(&tracker, true);
    tracker.dispose();

    let observation = || vec![snap("external-observation:observation-1", 8, 8, None)];
    assert_eq!(
        (before_suppression, fallback, snapshot(&tracker), snapshot_with(&tracker, true)),
        (vec![], observation(), vec![], observation())
    );
}

#[test]
fn restores_external_attribution_when_agent_host_suppression_is_invalidated() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = disposables.add_rc(TestAnnotatedDocument::new("initial"));
    let correlation = TestExternalEditCorrelation::new();
    let tracker = new_tracker(&disposables, &document, Some(&correlation), ExternalEditCorrelationPolicy::Suppress);

    document.apply(&external_edit(), EditSources::reload_from_disk());
    tracker.apply_pending_external_edits();
    correlation.suppress(&correlation.last_observation_id());
    correlation.invalidate(&correlation.last_observation_id());

    assert_eq!(snapshot(&tracker), vec![snap("external-observation:observation-1", 8, 8, None)]);
}

#[test]
fn applies_queued_external_edits_before_the_next_attributed_edit() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = disposables.add_rc(TestAnnotatedDocument::new(""));
    let tracker = new_tracker(&disposables, &document, None, ExternalEditCorrelationPolicy::Suppress);
    let ai = chat_edit_source("gpt-5", "request-1");
    let user = EditSources::cursor("type", None);
    let ai_key = ai.to_key(1, &[]);

    document.apply(&StringEdit::insert(0, "abcdef"), ai);
    document.apply(&StringEdit::delete(OffsetRange::new(2, 4)), EditSources::reload_from_disk());

    assert_eq!(snapshot(&tracker), vec![snap(&ai_key, 6, 6, Some("request-1"))]);

    document.apply(&StringEdit::insert(4, "X"), user);

    assert_eq!(
        snapshot(&tracker),
        vec![
            snap(&ai_key, 6, 4, Some("request-1")),
            snap("source:cursor-kind:type", 1, 1, None),
            snap("source:reloadFromDisk", 0, 0, None),
        ]
    );
}

#[test]
fn joins_level_one_keys_but_keeps_distinct_model_ids_separate() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = disposables.add_rc(TestAnnotatedDocument::new(""));
    let tracker = new_tracker(&disposables, &document, None, ExternalEditCorrelationPolicy::Suppress);
    let gpt_first = chat_edit_source("gpt-5", "request-1");
    let gpt_second = chat_edit_source("gpt-5", "request-2");
    let claude = chat_edit_source("claude-sonnet", "request-3");
    let (gpt_key, gpt_second_key, claude_key) = (gpt_first.to_key(1, &[]), gpt_second.to_key(1, &[]), claude.to_key(1, &[]));

    document.apply(&StringEdit::insert(0, "one"), gpt_first);
    document.apply(&StringEdit::insert(3, "two"), gpt_second);
    document.apply(&StringEdit::insert(6, "three"), claude);

    assert_eq!(
        snapshot(&tracker),
        vec![snap(&claude_key, 5, 5, Some("request-3")), snap(&gpt_key, 6, 6, Some("request-1"))]
    );
    assert_eq!(gpt_key, gpt_second_key);
}

#[test]
fn suppresses_matched_external_attribution_while_preserving_range_transforms() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = disposables.add_rc(TestAnnotatedDocument::new(""));
    let correlation = TestExternalEditCorrelation::new();
    let tracker = new_tracker(&disposables, &document, Some(&correlation), ExternalEditCorrelationPolicy::Suppress);
    let ai = chat_edit_source("gpt-5", "request-1");
    let user = EditSources::cursor("type", None);
    let ai_key = ai.to_key(1, &[]);

    document.apply(&StringEdit::insert(0, "abcdef"), ai);
    document.apply(&StringEdit::delete(OffsetRange::new(2, 4)), EditSources::reload_from_disk());
    correlation.suppress(&correlation.last_observation_id());
    document.apply(&StringEdit::insert(4, "X"), user);

    assert_eq!(
        snapshot(&tracker),
        vec![snap(&ai_key, 6, 4, Some("request-1")), snap("source:cursor-kind:type", 1, 1, None)]
    );
}

#[test]
fn reattributes_an_initial_external_edit_and_preserves_retention_composition() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = disposables.add_rc(TestAnnotatedDocument::new("initial"));
    let correlation = TestExternalEditCorrelation::new();
    let tracker = new_tracker(&disposables, &document, Some(&correlation), ExternalEditCorrelationPolicy::Reattribute);
    let agent_host = agent_host_edit_source("gpt-5", "turn-1", "copilotcli");

    document.apply(&external_edit(), EditSources::reload_from_disk());
    tracker.apply_pending_external_edits();
    correlation.resolve(&correlation.last_observation_id(), agent_host);
    document.apply(&StringEdit::replace(OffsetRange::new(0, 3), "X"), EditSources::cursor("type", None));

    let external_key = tracker.get_all_keys(false).into_iter().find(|key| key.starts_with("external-observation:")).expect("há uma chave externa");
    let ranges = tracker.get_tracked_ranges(None, false);
    assert_eq!(
        (
            tracker.get_representative(&external_key).map(|r| r.to_key(1, &[])),
            tracker.get_total_inserted_characters_count(&external_key, false),
            ranges.iter().filter(|range| range.source_key == external_key).map(|range| i64::from(range.range.length())).sum::<i64>(),
            ranges.iter().find(|range| range.source_key == external_key).map(|range| range.source.category()),
        ),
        (
            Some("source:Chat.applyEdits-$modelId:gpt-5-$harness:copilotcli-$origin:agentHost".to_owned()),
            8,
            5,
            Some("agentHost"),
        )
    );
}

#[test]
fn restores_external_attribution_when_reattribution_is_invalidated() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = disposables.add_rc(TestAnnotatedDocument::new("initial"));
    let correlation = TestExternalEditCorrelation::new();
    let tracker = new_tracker(&disposables, &document, Some(&correlation), ExternalEditCorrelationPolicy::Reattribute);

    document.apply(&external_edit(), EditSources::reload_from_disk());
    tracker.apply_pending_external_edits();
    correlation.resolve(&correlation.last_observation_id(), agent_host_edit_source("gpt-5", "turn-1", "copilotcli"));
    correlation.invalidate(&correlation.last_observation_id());

    assert_eq!(snapshot(&tracker), vec![snap("external-observation:observation-1", 8, 8, None)]);
}
