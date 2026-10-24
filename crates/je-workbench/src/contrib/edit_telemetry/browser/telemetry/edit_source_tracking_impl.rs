//! Porte de `src/vs/workbench/contrib/editTelemetry/browser/telemetry/editSourceTrackingImpl.ts`.
//!
//! Representação:
//! - `EditTelemetryCategory` (união de textos) é o enum [`EditTelemetryCategory`], que serve de chave do
//!   `sumByCategory`;
//! - o `Map<TextDocument, TrackedDocumentInfo>` de `docsState` é o [`DocsState`] (a lista de pares na ordem do mapa
//!   em cache, com busca pela identidade do documento); o `mapObservableArrayCached` usa a identidade do
//!   `AnnotatedDocument` como chave;
//! - `this._scm = createInstance(ScmAdapter)` é o serviço [`I_SCM_ADAPTER`] (ver `scm_adapter`);
//! - o construtor do `TrackedDocumentInfo` junto das funções que leem os serviços injetados (`sendTelemetry`,
//!   `getTelemetryData`, `_sendTelemetryAndLog`) é dividido: o estado que o `sendTelemetry` lê vive no
//!   [`TrackedDocumentSender`], compartilhado por `Rc` com os fechos dos observáveis (que o `TrackedDocumentInfo`
//!   guarda), para o objeto não formar ciclo de `Rc` consigo mesmo;
//! - os três `derived` de rastreador (longo prazo, janela de 10 minutos e janela de 20 minutos) diferem só no modo, na
//!   política de correlação, na exigência de visibilidade e no reset por tempo de foco, e são montados por
//!   [`create_tracker`];
//! - `longtermReason` (variável do construtor mutada pelos fechos) é um `Rc<Cell<EditTelemetryTrigger>>`;
//! - `async sendTelemetry` é um `async_fn`; o `catch` de `_sendTelemetryAndLog` registra o erro e o `finally` solta as
//!   correlações e descarta o rastreador;
//! - `isTrackedByGit: Promise<boolean> | undefined` é `Option<Promise<bool>>`.

use std::cell::Cell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::async_::{IntervalTimer, async_fn};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::observable::{
    IObservable, IReader, Observable, ObservableExt, ObservableSignal, autorun, derived, map_observable_array_cached_by, observable_signal,
    recompute_initially_and_on_change, run_on_change,
};
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_base::uri::URI;
use je_editor::text_model_edit_source::TextModelEditSource;
use je_instantiation::{Injectable, InstantiationService};
use je_platform::log::{LogService, Logger};
use je_platform::telemetry::edit_telemetry::{
    AgentHostAttributionCoverage, AgentHostCoverageStats, EditSourcesDetailsTelemetryData, EditSourcesStatsTelemetryData, EditTelemetryMode, EditTelemetryTrigger,
    optional_string, send_edit_sources_details_telemetry, send_edit_sources_stats_telemetry,
};
use je_platform::telemetry::telemetry::ITelemetryService;

use super::agent_host_edit_marker_service::{
    IAgentHostEditMarkerService, IExternalEditCorrelation, IPreparedAgentHostEditAttributionFlush, is_agent_host_edit_attribution_deferred_error,
    is_agent_host_edit_attribution_unknown_outcome_error,
};
use super::arc_telemetry_sender::{CreateSuggestionIdForChatOrInlineChatCaller, EditTelemetryReportEditArcForChatOrInlineChatSender, EditTelemetryReportInlineEditArcSender};
use super::edit_tracker::{DocumentEditSourceTracker, ExternalEditCorrelationPolicy, TrackedEdit};
use super::scm_adapter::{IScmAdapter, IScmRepoAdapter};
use crate::contrib::edit_telemetry::browser::helpers::annotated_documents::{AnnotatedDocument, IAnnotatedDocuments};
use crate::contrib::edit_telemetry::browser::helpers::document_with_annotated_edits::{
    EditKeySourceData, EditSource, IDocumentWithAnnotatedEdits, InlineSuggestKind, create_doc_with_just_reason,
};
use crate::contrib::edit_telemetry::browser::helpers::observable_workspace::IObservableDocument;
use crate::contrib::edit_telemetry::browser::helpers::utils::sum_by_category;
use crate::contrib::edit_telemetry::browser::random_service::IRandomService;
use crate::services::textfile::textfiles::ITextFileService;
use crate::services::user_attention::user_attention_service::IUserAttentionService;

const FOCUS_CORRELATION_DRAIN_TIMEOUT: u64 = 1_000;

/// `getDetailsGroupingKey(source, sourceKey = source.toKey(1))`.
fn get_details_grouping_key(source: &TextModelEditSource, source_key: Option<String>) -> String {
    let source_key = source_key.unwrap_or_else(|| source.to_key(1, &[]));
    let agent_session_id = source.metadata_value("$$agentSessionId");
    let chat_session_id = source.metadata_value("$$chatSessionId");
    if source.metadata_value("$origin") == Some("agentHost") && (agent_session_id.is_some() || chat_session_id.is_some()) {
        let key = Dynamic::array([
            Dynamic::string(&source_key),
            optional_string(agent_session_id),
            optional_string(source.metadata_value("$$sessionId")),
            optional_string(chat_session_id),
        ]);
        json_stringify(&key).unwrap_or_default()
    } else {
        source_key
    }
}

/// `EditTelemetryCategory`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditTelemetryCategory {
    Nes,
    InlineCompletionsCopilot,
    InlineCompletionsNes,
    InlineCompletionsOther,
    OtherAi,
    AgentHost,
    User,
    Ide,
    External,
    Unknown,
}

/// `getEditTelemetryCategory(source)`.
pub fn get_edit_telemetry_category(source: &EditSource) -> EditTelemetryCategory {
    if let EditSource::InlineSuggest(inline) = source {
        return match (inline.kind, inline.extension_id.as_str(), inline.provider_id.as_str()) {
            (InlineSuggestKind::Nes, _, _) => EditTelemetryCategory::Nes,
            (InlineSuggestKind::Completion, "github.copilot", _) => EditTelemetryCategory::InlineCompletionsCopilot,
            (InlineSuggestKind::Completion, "github.copilot-chat", "nes") => EditTelemetryCategory::InlineCompletionsNes,
            (InlineSuggestKind::Completion, "github.copilot-chat", "completions") => EditTelemetryCategory::InlineCompletionsCopilot,
            (InlineSuggestKind::Completion, _, _) => EditTelemetryCategory::InlineCompletionsOther,
        };
    }

    match source.category() {
        "ai" => EditTelemetryCategory::OtherAi,
        "agentHost" => EditTelemetryCategory::AgentHost,
        "user" => EditTelemetryCategory::User,
        "ide" => EditTelemetryCategory::Ide,
        "external" => EditTelemetryCategory::External,
        _ => EditTelemetryCategory::Unknown,
    }
}

/// O par `[doc.document, TrackedDocumentInfo]` do mapa em cache.
pub struct TrackedEntry {
    pub document: Rc<dyn IObservableDocument>,
    pub info: Rc<TrackedDocumentInfo>,
}

/// O `Map<IObservableDocument, TrackedDocumentInfo>` de `docsState`.
pub struct DocsState {
    entries: Vec<Rc<TrackedEntry>>,
}

impl DocsState {
    /// `size`.
    pub fn size(&self) -> usize {
        self.entries.len()
    }

    /// `get(document)`: a busca é pela identidade do documento.
    pub fn get(&self, document: &Rc<dyn IObservableDocument>) -> Option<Rc<TrackedDocumentInfo>> {
        let target = Rc::as_ptr(document).cast::<()>();
        self.entries.iter().find(|entry| Rc::as_ptr(&entry.document).cast::<()>() == target).map(|entry| entry.info.clone())
    }
}

/// `EditSourceTrackingImpl`.
pub struct EditSourceTrackingImpl {
    base: Rc<Disposable>,
    pub docs_state: IObservable<Rc<DocsState>>,
}

impl Injectable for EditSourceTrackingImpl {
    /// `_statsEnabled`, `_annotatedDocuments` e `_agentHostEditMarkerService`.
    type Args = (IObservable<bool>, Rc<dyn IAnnotatedDocuments>, Option<Rc<dyn IAgentHostEditMarkerService>>);
    type Deps = (Rc<InstantiationService>,);

    fn create((stats_enabled, annotated_documents, agent_host_edit_marker_service): Self::Args, (instantiation_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();

        let scm_bridge: Rc<dyn IScmAdapter> = instantiation_service.invoke_function(|accessor| accessor.get(super::scm_adapter::I_SCM_ADAPTER));
        let states = map_observable_array_cached_by(
            None,
            annotated_documents.documents(),
            move |doc: &Rc<AnnotatedDocument>, store| {
                let info = store.add_rc(instantiation_service.create_instance::<TrackedDocumentInfo>((
                    doc.clone(),
                    scm_bridge.clone(),
                    stats_enabled.clone(),
                    agent_host_edit_marker_service.clone(),
                )));
                Rc::new(TrackedEntry { document: doc.document.clone(), info })
            },
            |doc| Rc::as_ptr(doc).cast::<()>() as usize,
        );
        let docs_state: IObservable<Rc<DocsState>> = states.map(|entries, _| Rc::new(DocsState { entries: entries.iter().cloned().collect() }));

        base.register(recompute_initially_and_on_change(&docs_state, None));

        Rc::new(Self { base, docs_state })
    }
}

impl IDisposable for EditSourceTrackingImpl {
    fn dispose(&self) {
        self.base.dispose();
    }
}

type TrackerObservable = IObservable<Option<Rc<DocumentEditSourceTracker<()>>>>;

/// O estado que o envio da telemetria (`sendTelemetry`, `getTelemetryData`, `_sendTelemetryAndLog`) lê.
pub struct TrackedDocumentSender {
    doc: Rc<AnnotatedDocument>,
    repo: IObservable<Option<Rc<dyn IScmRepoAdapter>>>,
    agent_host_edit_marker_service: Option<Rc<dyn IAgentHostEditMarkerService>>,
    telemetry_service: Rc<dyn ITelemetryService>,
    random_service: Rc<dyn IRandomService>,
    user_attention_service: Rc<dyn IUserAttentionService>,
    text_file_service: Rc<dyn ITextFileService>,
    log_service: Rc<dyn LogService>,
}

/// `getTelemetryData(ranges)`.
pub struct TelemetryData {
    pub nes_modified_count: usize,
    pub inline_completions_copilot_modified_count: usize,
    pub inline_completions_nes_modified_count: usize,
    pub other_ai_modified_count: usize,
    pub agent_host_modified_count: usize,
    pub user_modified_count: usize,
    pub ide_modified_count: usize,
    pub unknown_modified_count: usize,
    pub external_modified_count: usize,
    pub total_modified_characters_in_final_state: usize,
    pub language_id: String,
    pub is_tracked_by_git: Option<Promise<bool>>,
}

/// O valor de `telemetryKeys` de `sendTelemetry`.
struct TelemetryKeyEntry {
    representative: Rc<TextModelEditSource>,
    modified_count: usize,
    delta_modified_count: usize,
}

impl TrackedDocumentSender {
    /// `_sendTelemetryAndLog(mode, trigger, tracker, focusTime, actualTime)`.
    fn send_telemetry_and_log(
        self: &Rc<Self>,
        mode: EditTelemetryMode,
        trigger: EditTelemetryTrigger,
        tracker: Rc<DocumentEditSourceTracker<()>>,
        focus_time: u64,
        actual_time: u64,
    ) {
        let log_service = self.log_service.clone();
        let finally_tracker = tracker.clone();
        let _ = self
            .send_telemetry(mode, trigger, tracker, focus_time, actual_time)
            .catch(move |error| {
                log_service.error(&format!("[EditSourceTrackingImpl] Failed to send {} edit telemetry: {error}", mode.as_str()));
            })
            .finally(move || {
                finally_tracker.release_external_edit_correlations();
                finally_tracker.dispose();
            });
    }

    /// `sendTelemetry(mode, trigger, t, focusTime, actualTime)`.
    pub fn send_telemetry(
        self: &Rc<Self>,
        mode: EditTelemetryMode,
        trigger: EditTelemetryTrigger,
        t: Rc<DocumentEditSourceTracker<()>>,
        focus_time: u64,
        actual_time: u64,
    ) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if mode != EditTelemetryMode::Longterm {
                t.wait_for_external_edit_correlations(FOCUS_CORRELATION_DRAIN_TIMEOUT).await?;
            }
            t.apply_pending_external_edits();
            let mut ranges = t.get_tracked_ranges(None, false);
            let mut internal_keys = t.get_all_keys(false);
            let mut data = this.get_telemetry_data(&ranges);
            let stats_uuid = this.random_service.generate_uuid();
            let mut prepared_agent_flush: Option<IPreparedAgentHostEditAttributionFlush> = None;
            let mut defer_suppressed_external = false;
            let uri = this.doc.document.uri().clone();
            let is_dirty = this.text_file_service.is_dirty(&uri);
            if mode == EditTelemetryMode::Longterm
                && let Some(marker_service) = &this.agent_host_edit_marker_service
            {
                let language_id = this.doc.document.language_id().get();
                match marker_service.prepare_flush(&uri, trigger, &stats_uuid, is_dirty, Some(&language_id)).await {
                    Ok(prepared) => prepared_agent_flush = prepared,
                    Err(error) => {
                        this.log_service.error(&format!("[EditSourceTrackingImpl] Failed to prepare Agent Host edit attribution: {error}"));
                        defer_suppressed_external =
                            is_agent_host_edit_attribution_deferred_error(&error) || is_agent_host_edit_attribution_unknown_outcome_error(&error);
                    }
                }
            }
            if let Some(flush) = prepared_agent_flush.clone() {
                t.apply_pending_external_edits();
                ranges = t.get_tracked_ranges(None, false);
                internal_keys = t.get_all_keys(false);
                data = this.get_telemetry_data(&ranges);
                if let Err(error) = (flush.commit)(data.total_modified_characters_in_final_state + flush.agent_modified_count).await {
                    this.log_service.error(&format!("[EditSourceTrackingImpl] Failed to commit Agent Host edit attribution: {error}"));
                    if !is_agent_host_edit_attribution_unknown_outcome_error(&error) {
                        prepared_agent_flush = None;
                    }
                    defer_suppressed_external =
                        is_agent_host_edit_attribution_deferred_error(&error) || is_agent_host_edit_attribution_unknown_outcome_error(&error);
                }
            }
            let include_suppressed_external = prepared_agent_flush.is_none()
                && !defer_suppressed_external
                && !is_dirty
                && mode == EditTelemetryMode::Longterm
                && this.agent_host_edit_marker_service.is_some();
            if include_suppressed_external {
                ranges = t.get_tracked_ranges(None, true);
                internal_keys = t.get_all_keys(true);
                data = this.get_telemetry_data(&ranges);
            }
            let coverage_gap = if mode == EditTelemetryMode::Longterm
                && !is_dirty
                && !defer_suppressed_external
                && !prepared_agent_flush.as_ref().is_some_and(|flush| flush.defer_coverage_gap)
            {
                let through_sequence = prepared_agent_flush.as_ref().and_then(|flush| flush.coverage_gap_through_sequence.or(flush.last_sequence));
                this.agent_host_edit_marker_service.as_ref().and_then(|marker_service| marker_service.take_coverage_gap(&uri, through_sequence))
            } else {
                None
            };
            let agent_modified_count = if mode == EditTelemetryMode::Longterm {
                prepared_agent_flush.as_ref().map_or(0, |flush| flush.agent_modified_count)
            } else {
                data.agent_host_modified_count
            };
            if internal_keys.is_empty() && agent_modified_count == 0 && coverage_gap.is_none() {
                return Ok(());
            }
            let total_modified_count = data.total_modified_characters_in_final_state + prepared_agent_flush.as_ref().map_or(0, |flush| flush.agent_modified_count);

            let mut telemetry_keys: IndexMap<String, TelemetryKeyEntry> = IndexMap::new();
            for internal_key in &internal_keys {
                let representative = t.get_representative(internal_key).expect("toda chave interna tem um representante");
                let telemetry_key = get_details_grouping_key(&representative, None);
                let entry = telemetry_keys.entry(telemetry_key).or_insert_with(|| TelemetryKeyEntry { representative, modified_count: 0, delta_modified_count: 0 });
                entry.delta_modified_count += t.get_total_inserted_characters_count(internal_key, include_suppressed_external);
            }
            for range in &ranges {
                let representative = t.get_representative(&range.source_key).expect("toda faixa tem um representante");
                if let Some(entry) = telemetry_keys.get_mut(&get_details_grouping_key(&representative, None)) {
                    entry.modified_count += range.range.length() as usize;
                }
            }
            // Apply the source cap before subdividing by Auto tier and HydraFusion model so no selected source loses contributions.
            let mut source_groups: IndexMap<String, Vec<TelemetryKeyEntry>> = IndexMap::new();
            for (_, entry) in telemetry_keys {
                let group_key = get_details_grouping_key(&entry.representative, Some(entry.representative.to_key(1, &[("$autoTier", false), ("$fusionModel", false)])));
                source_groups.entry(group_key).or_default().push(entry);
            }
            let mut groups: Vec<(Vec<TelemetryKeyEntry>, usize)> = source_groups
                .into_values()
                .map(|entries| {
                    let modified_count = entries.iter().map(|entry| entry.modified_count).sum();
                    (entries, modified_count)
                })
                .collect();
            groups.sort_by(|a, b| b.1.cmp(&a.1));
            let entries: Vec<TelemetryKeyEntry> = groups.into_iter().take(if mode == EditTelemetryMode::Longterm { 30 } else { 10 }).flat_map(|(entries, _)| entries).collect();

            let language_id = this.doc.document.language_id().get();
            for telemetry_entry in &entries {
                let repr = &telemetry_entry.representative;
                let origin = repr.metadata_value("$origin");

                send_edit_sources_details_telemetry(
                    &*this.telemetry_service,
                    &EditSourcesDetailsTelemetryData {
                        copilot_sku: None,
                        mode,
                        source_key: repr.to_key(1, &[]),
                        source_key_cleaned: repr.to_key(
                            1,
                            &[("$extensionId", false), ("$extensionVersion", false), ("$modelId", false), ("$autoTier", false), ("$fusionModel", false)],
                        ),
                        extension_id: repr.metadata_value("$extensionId").map(str::to_owned),
                        extension_version: repr.metadata_value("$extensionVersion").map(str::to_owned),
                        model_id: repr.metadata_value("$modelId").map(str::to_owned),
                        auto_tier: repr.metadata_value("$autoTier").map(str::to_owned),
                        fusion_model: repr.metadata_value("$fusionModel").map(str::to_owned),
                        trigger,
                        language_id: Some(language_id.clone()),
                        stats_uuid: stats_uuid.clone(),
                        conversation_id: repr.metadata_value("$$sessionId").map(str::to_owned),
                        agent_session_id: repr.metadata_value("$$agentSessionId").map(str::to_owned),
                        chat_session_id: repr.metadata_value("$$chatSessionId").map(str::to_owned),
                        request_id: repr.metadata_value("$$requestId").map(str::to_owned),
                        origin: origin.map(str::to_owned),
                        provider: if origin == Some("agentHost") {
                            Some(repr.metadata_value("$harness").filter(|harness| !harness.is_empty()).unwrap_or("unknown").to_owned())
                        } else {
                            None
                        },
                        modified_count: telemetry_entry.modified_count,
                        delta_modified_count: telemetry_entry.delta_modified_count,
                        total_modified_count,
                    },
                );
            }

            let is_tracked_by_git = match data.is_tracked_by_git.clone() {
                Some(promise) => promise.await?,
                None => false,
            };
            send_edit_sources_stats_telemetry(
                &*this.telemetry_service,
                &EditSourcesStatsTelemetryData {
                    copilot_sku: None,
                    mode,
                    language_id: Some(language_id),
                    stats_uuid,
                    nes_modified_count: data.nes_modified_count,
                    inline_completions_copilot_modified_count: data.inline_completions_copilot_modified_count,
                    inline_completions_nes_modified_count: data.inline_completions_nes_modified_count,
                    other_ai_modified_count: data.other_ai_modified_count,
                    agent_host_modified_count: agent_modified_count,
                    unknown_modified_count: data.unknown_modified_count,
                    user_modified_count: data.user_modified_count,
                    ide_modified_count: data.ide_modified_count,
                    total_modified_characters: total_modified_count,
                    external_modified_count: data.external_modified_count,
                    is_tracked_by_git: Some(i32::from(is_tracked_by_git)),
                    focus_time: Some(focus_time),
                    actual_time: Some(actual_time),
                    trigger,
                    tracking_scope: None,
                    agent_host_coverage: if mode == EditTelemetryMode::Longterm {
                        Some(AgentHostCoverageStats {
                            attribution_coverage: if coverage_gap.is_some() { AgentHostAttributionCoverage::Partial } else { AgentHostAttributionCoverage::Complete },
                            untracked_edit_count: coverage_gap.map_or(0, |gap| gap.edit_count),
                            untracked_inserted_count: coverage_gap.map_or(0, |gap| gap.inserted_count),
                        })
                    } else {
                        None
                    },
                },
            );
            Ok(())
        })
    }

    /// `getTelemetryData(ranges)`.
    pub fn get_telemetry_data(&self, ranges: &[TrackedEdit]) -> TelemetryData {
        let sums = sum_by_category(ranges, |r| i64::from(r.range.length()), |r| get_edit_telemetry_category(&r.source));
        let sum = |category: EditTelemetryCategory| sums.get(&category).copied().unwrap_or(0) as usize;
        let total_modified_characters_in_final_state = ranges.iter().map(|r| r.range.length() as usize).sum();
        let uri = self.doc.document.uri();

        TelemetryData {
            nes_modified_count: sum(EditTelemetryCategory::Nes),
            inline_completions_copilot_modified_count: sum(EditTelemetryCategory::InlineCompletionsCopilot),
            inline_completions_nes_modified_count: sum(EditTelemetryCategory::InlineCompletionsNes),
            other_ai_modified_count: sum(EditTelemetryCategory::OtherAi),
            agent_host_modified_count: sum(EditTelemetryCategory::AgentHost),
            user_modified_count: sum(EditTelemetryCategory::User),
            ide_modified_count: sum(EditTelemetryCategory::Ide),
            unknown_modified_count: sum(EditTelemetryCategory::Unknown),
            external_modified_count: sum(EditTelemetryCategory::External),
            total_modified_characters_in_final_state,
            language_id: self.doc.document.language_id().get(),
            is_tracked_by_git: self.repo.get().map(|repo| repo.is_ignored(uri)),
        }
    }
}

/// As diferenças entre os três rastreadores de `TrackedDocumentInfo`.
struct TrackerKind {
    mode: EditTelemetryMode,
    policy: ExternalEditCorrelationPolicy,
    /// O rastreador só existe enquanto o documento está visível.
    requires_visible: bool,
    /// O tempo de foco acumulado depois do qual o rastreador é recriado (`fireAfterGivenFocusTimePassed`).
    reset_after_focus_time_ms: Option<u64>,
    /// O `trigger` do envio, lido quando o rastreador é descartado.
    trigger: Rc<dyn Fn() -> EditTelemetryTrigger>,
}

/// Um dos três `derived` de rastreador do construtor do `TrackedDocumentInfo`.
fn create_tracker(
    sender: &Rc<TrackedDocumentSender>,
    kind: TrackerKind,
    stats_enabled: IObservable<bool>,
    doc_with_just_reason: Rc<dyn IDocumentWithAnnotatedEdits<EditKeySourceData>>,
    external_edit_correlation: Option<Rc<dyn IExternalEditCorrelation>>,
    reset_signal: Rc<ObservableSignal<()>>,
) -> TrackerObservable {
    let sender = sender.clone();
    derived(move |reader| {
        if !stats_enabled.read(reader) {
            return None;
        }
        if kind.requires_visible && !sender.doc.is_visible.read(reader) {
            return None;
        }
        reset_signal.read(reader);

        if let Some(focus_time_ms) = kind.reset_after_focus_time_ms {
            let signal = reset_signal.clone();
            reader.store().add(sender.user_attention_service.fire_after_given_focus_time_passed(focus_time_ms, Box::new(move || signal.trigger(None, ()))));
        }

        let t = DocumentEditSourceTracker::new(doc_with_just_reason.clone(), (), external_edit_correlation.clone(), kind.policy);
        let start_focus_time = sender.user_attention_service.total_focus_time_ms();
        let start_time = date_now();
        let sender = sender.clone();
        let tracker = t.clone();
        let mode = kind.mode;
        let trigger = kind.trigger.clone();
        reader.store().add(to_disposable(move || {
            // send telemetry of the tracker that is going away
            tracker.stop_tracking();
            let focus_time = sender.user_attention_service.total_focus_time_ms() - start_focus_time;
            sender.send_telemetry_and_log(mode, trigger(), tracker, focus_time, date_now() - start_time);
        }));
        Some(t)
    })
}

/// `TrackedDocumentInfo`.
pub struct TrackedDocumentInfo {
    base: Rc<Disposable>,
    pub longterm_tracker: TrackerObservable,
    pub windowed_tracker: TrackerObservable,
    pub windowed_focus_tracker: TrackerObservable,
}

impl Injectable for TrackedDocumentInfo {
    /// `_doc`, `_scm`, `_statsEnabled` e `_agentHostEditMarkerService`.
    type Args = (Rc<AnnotatedDocument>, Rc<dyn IScmAdapter>, IObservable<bool>, Option<Rc<dyn IAgentHostEditMarkerService>>);
    type Deps = (Rc<InstantiationService>, Rc<dyn ITelemetryService>, Rc<dyn IRandomService>, Rc<dyn IUserAttentionService>, Rc<dyn ITextFileService>, Rc<dyn LogService>);

    fn create(
        (doc, scm, stats_enabled, agent_host_edit_marker_service): Self::Args,
        (instantiation_service, telemetry_service, random_service, user_attention_service, text_file_service, log_service): Self::Deps,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());

        let uri: URI = doc.document.uri().clone();
        let repo: IObservable<Option<Rc<dyn IScmRepoAdapter>>> = derived(move |reader| {
            let reader: &dyn IReader = reader;
            scm.get_repo(&uri, Some(reader))
        });

        let doc_with_just_reason = create_doc_with_just_reason(&doc.document_with_annotations, &store);
        let external_edit_correlation: Option<Rc<dyn IExternalEditCorrelation>> =
            agent_host_edit_marker_service.as_ref().map(|marker_service| marker_service.create_correlation(doc.document.uri()));

        let sender = Rc::new(TrackedDocumentSender {
            doc: doc.clone(),
            repo: repo.clone(),
            agent_host_edit_marker_service,
            telemetry_service,
            random_service,
            user_attention_service,
            text_file_service,
            log_service,
        });

        let longterm_reset_signal = observable_signal::<()>("resetSignal");

        let longterm_reason = Rc::new(Cell::new(EditTelemetryTrigger::Closed));
        let reason = longterm_reason.clone();
        let longterm_tracker = create_tracker(
            &sender,
            TrackerKind {
                mode: EditTelemetryMode::Longterm,
                policy: ExternalEditCorrelationPolicy::Suppress,
                requires_visible: false,
                reset_after_focus_time_ms: None,
                trigger: Rc::new(move || reason.get()),
            },
            stats_enabled.clone(),
            doc_with_just_reason.clone(),
            external_edit_correlation.clone(),
            longterm_reset_signal.clone(),
        )
        .recompute_initially_and_on_change(&store, None);

        let interval_timer = IntervalTimer::new();
        base.register(interval_timer.clone());
        {
            let reason = longterm_reason.clone();
            let signal = longterm_reset_signal.clone();
            interval_timer.cancel_and_set(
                move || {
                    // Reset after 10 hours
                    reason.set(EditTelemetryTrigger::TenHours);
                    signal.trigger(None, ());
                    reason.set(EditTelemetryTrigger::Closed);
                },
                10 * 60 * 60 * 1000,
            );
        }

        // Reset on branch change or commit
        {
            let repo = repo.clone();
            let reason = longterm_reason.clone();
            let signal = longterm_reset_signal.clone();
            base.register(autorun(move |reader| {
                let Some(repo) = repo.read(reader) else {
                    return;
                };
                let (hash_reason, hash_signal) = (reason.clone(), signal.clone());
                reader.store().add(run_on_change::<Option<String>, ()>(repo.head_commit_hash_obs(), move |_, _, _| {
                    hash_reason.set(EditTelemetryTrigger::HashChange);
                    hash_signal.trigger(None, ());
                    hash_reason.set(EditTelemetryTrigger::Closed);
                }));
                let (branch_reason, branch_signal) = (reason.clone(), signal.clone());
                reader.store().add(run_on_change::<Option<String>, ()>(repo.head_branch_name_obs(), move |_, _, _| {
                    branch_reason.set(EditTelemetryTrigger::BranchChange);
                    branch_signal.trigger(None, ());
                    branch_reason.set(EditTelemetryTrigger::Closed);
                }));
            }));
        }

        base.register(instantiation_service.create_instance::<EditTelemetryReportInlineEditArcSender>((doc.document_with_annotations.clone(), repo.clone())));
        base.register(instantiation_service.create_instance::<EditTelemetryReportEditArcForChatOrInlineChatSender>((doc.document_with_annotations.clone(), repo.clone())));
        base.register(instantiation_service.create_instance::<CreateSuggestionIdForChatOrInlineChatCaller>(doc.document_with_annotations.clone()));

        // Focus time based 10-minute window tracker
        let windowed_tracker = create_tracker(
            &sender,
            TrackerKind {
                mode: EditTelemetryMode::TenMinFocusWindow,
                policy: ExternalEditCorrelationPolicy::Reattribute,
                requires_visible: true,
                reset_after_focus_time_ms: Some(10 * 60 * 1000),
                trigger: Rc::new(|| EditTelemetryTrigger::Time),
            },
            stats_enabled.clone(),
            doc_with_just_reason.clone(),
            external_edit_correlation.clone(),
            observable_signal::<()>("resetSignal"),
        )
        .recompute_initially_and_on_change(&store, None);

        // Focus time based 20-minute window tracker
        let windowed_focus_tracker = create_tracker(
            &sender,
            TrackerKind {
                mode: EditTelemetryMode::TwentyMinFocusWindow,
                policy: ExternalEditCorrelationPolicy::Reattribute,
                requires_visible: true,
                reset_after_focus_time_ms: Some(20 * 60 * 1000),
                trigger: Rc::new(|| EditTelemetryTrigger::Time),
            },
            stats_enabled,
            doc_with_just_reason,
            external_edit_correlation,
            observable_signal::<()>("focusResetSignal"),
        )
        .recompute_initially_and_on_change(&store, None);

        Rc::new(Self { base, longterm_tracker, windowed_tracker, windowed_focus_tracker })
    }
}

impl IDisposable for TrackedDocumentInfo {
    fn dispose(&self) {
        self.base.dispose();
    }
}
