//! Porte de `src/vs/workbench/contrib/editTelemetry/browser/telemetry/arcTelemetrySender.ts`.
//!
//! Representação:
//! - `docWithAnnotatedEdits: IDocumentWithAnnotatedEdits<EditSourceData>` e `scmRepoBridge` são os `Args` de cada
//!   classe; `@IInstantiationService`, `@IRandomService` e `@IAiEditTelemetryService` são as dependências;
//! - o reporter criado com `this._store.add(createInstance(ArcTelemetryReporter, ...))` e removido pelo `_dispose`
//!   (`this._store.delete(reporter)`) é [`start_arc_reporter`]: o fecho de descarte guarda `Weak` do store e do
//!   reporter, para o reporter (que guarda o fecho) não formar ciclo de `Rc` com o store que o guarda;
//! - a escolha das edições de chat e de chat inline, repetida pelas duas classes do original, é
//!   [`chat_edit_source`];
//! - `...forwardToChannelIf(...)` é o [`ForwardToChannel`] de [`public_log2_forwarding`] (ver `forwarding_telemetry`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::errors::{Error, on_unexpected_error};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::observable::{IObservable, run_on_change};
use je_editor::code_action::languages::ProviderId;
use je_editor::edits::edit::BaseEdit;
use je_editor::edits::string_edit::{AnnotatedStringEdit, AnnotatedStringReplacement};
use je_editor::text::abstract_text::StringText;
use je_editor::text_model_edit_source::{EditDeltaInfo, EditSuggestionId, TextModelEditSource};
use je_instantiation::{Injectable, InstantiationService};
use je_platform::telemetry::edit_arc_telemetry::EditArcTelemetryEvent;
use je_platform::telemetry::edit_telemetry::optional_string;

use super::ai_edit_telemetry::ai_edit_telemetry_service::{
    EditTelemetryCodeSuggestedData, EditTelemetryBaseData, EditTelemetryFeature, EditTelemetryPresentation, IAiEditTelemetryService,
};
use super::arc_telemetry_reporter::{ArcTelemetryReporter, ArcTelemetryReporterArgs, ArcTelemetryReporterData};
use super::forwarding_telemetry::{forward_to_channel_if, is_copilot_like_extension, public_log2_forwarding};
use super::scm_adapter::IScmRepoAdapter;
use crate::contrib::edit_telemetry::browser::helpers::document_with_annotated_edits::{
    AnnotatedEdit, EditSourceData, IDocumentWithAnnotatedEdits, create_doc_with_just_reason,
};
use crate::contrib::edit_telemetry::browser::random_service::IRandomService;

/// `IObservable<IScmRepoAdapter | undefined>`.
pub type ScmRepoObservable = IObservable<Option<Rc<dyn IScmRepoAdapter>>>;

type SourceAnnotatedDocument = Rc<dyn IDocumentWithAnnotatedEdits<EditSourceData>>;

/// `new Set(['Chat.applyEdits', 'inlineChat.applyEdits'])`.
const SUPPORTED_CHAT_SOURCES: [&str; 2] = ["Chat.applyEdits", "inlineChat.applyEdits"];

/// O corpo comum das duas classes de chat: `None` se nenhuma edição é de chat, ou se nem todas são (o erro vai para o
/// tratador de erros inesperados); senão a origem da primeira edição.
fn chat_edit_source(edit: &AnnotatedStringEdit<EditSourceData>) -> Option<Rc<TextModelEditSource>> {
    let is_supported = |r: &AnnotatedStringReplacement<EditSourceData>| SUPPORTED_CHAT_SOURCES.contains(&r.data.edit_source.source.as_str());
    if !edit.replacements().iter().any(is_supported) {
        return None;
    }
    if !edit.replacements().iter().all(is_supported) {
        on_unexpected_error(&Error::new(format!("ArcTelemetrySender: Not all edits are {}!", edit.replacements()[0].data.edit_source.source)));
        return None;
    }
    Some(edit.replacements()[0].data.edit_source.clone())
}

/// `this._store.add(createInstance(ArcTelemetryReporter, times, _prev, docWithJustReason, scmRepoBridge, edit, send, () =>
/// this._store.delete(reporter)))` com `createDocWithJustReason(docWithAnnotatedEdits, this._store)` antes.
#[allow(clippy::too_many_arguments)]
fn start_arc_reporter(
    store: &Rc<DisposableStore>,
    instantiation_service: &Rc<InstantiationService>,
    doc_with_annotated_edits: &SourceAnnotatedDocument,
    scm_repo_bridge: &ScmRepoObservable,
    times_seconds: &[u64],
    prev: &Rc<StringText>,
    edit: AnnotatedStringEdit<EditSourceData>,
    send_telemetry_event: impl Fn(&ArcTelemetryReporterData) + 'static,
) {
    let doc_with_just_reason = create_doc_with_just_reason(doc_with_annotated_edits, store);
    let reporter_slot: Rc<RefCell<Option<Weak<ArcTelemetryReporter<AnnotatedStringEdit<EditSourceData>>>>>> = Rc::default();
    let slot = reporter_slot.clone();
    let weak_store = Rc::downgrade(store);
    let reporter = store.add_rc(instantiation_service.create_instance::<ArcTelemetryReporter<AnnotatedStringEdit<EditSourceData>>>(ArcTelemetryReporterArgs {
        times_ms: times_seconds.iter().map(|s| s * 1000).collect(),
        document_value_before_tracked_edit: prev.clone(),
        document: doc_with_just_reason,
        git_repo: scm_repo_bridge.clone(),
        tracked_edit: edit,
        send_telemetry_event: Rc::new(send_telemetry_event),
        dispose: Rc::new(move || {
            let reporter = slot.borrow().as_ref().and_then(Weak::upgrade);
            if let (Some(store), Some(reporter)) = (weak_store.upgrade(), reporter) {
                let reporter: DisposableRef = reporter;
                store.delete(&reporter);
            }
        }),
    }));
    *reporter_slot.borrow_mut() = Some(Rc::downgrade(&reporter));
}

/// `didBranchChange ? 1 : 0`.
fn branch_change_flag(did_branch_change: bool) -> i32 {
    i32::from(did_branch_change)
}

/// `EditTelemetryReportInlineEditArcSender`.
pub struct EditTelemetryReportInlineEditArcSender {
    base: Rc<Disposable>,
}

impl Injectable for EditTelemetryReportInlineEditArcSender {
    type Args = (SourceAnnotatedDocument, ScmRepoObservable);
    type Deps = (Rc<InstantiationService>,);

    fn create((doc_with_annotated_edits, scm_repo_bridge): Self::Args, (instantiation_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());

        base.register(run_on_change::<Rc<StringText>, AnnotatedEdit<EditSourceData>>(doc_with_annotated_edits.value(), move |_val, prev, changes| {
            let edit = AnnotatedEdit::compose_all(changes);

            if !edit.replacements().iter().any(|r| r.data.edit_source.source == "inlineCompletionAccept") {
                return;
            }
            if !edit.replacements().iter().all(|r| r.data.edit_source.source == "inlineCompletionAccept") {
                on_unexpected_error(&Error::new("ArcTelemetrySender: Not all edits are inline completion accept edits!"));
                return;
            }
            if edit.replacements()[0].data.edit_source.source != "inlineCompletionAccept" {
                return;
            }
            let data = edit.replacements()[0].data.edit_source.clone();

            start_arc_reporter(
                &store,
                &instantiation_service,
                &doc_with_annotated_edits,
                &scm_repo_bridge,
                &[0, 30, 120, 300, 600, 900],
                prev,
                edit,
                move |res| {
                    let extension_id = data.metadata_value("$extensionId");
                    let count = |value: usize| Dynamic::Number(value as f64);
                    let event = Dynamic::object([
                        ("extensionId", Dynamic::string(extension_id.unwrap_or(""))),
                        ("extensionVersion", Dynamic::string(data.metadata_value("$extensionVersion").unwrap_or(""))),
                        ("opportunityId", Dynamic::string(data.metadata_value("$$requestUuid").unwrap_or("unknown"))),
                        ("languageId", optional_string(data.metadata_value("$$languageId"))),
                        ("correlationId", optional_string(data.metadata_value("$$correlationId"))),
                        ("didBranchChange", Dynamic::Number(f64::from(branch_change_flag(res.did_branch_change)))),
                        ("timeDelayMs", Dynamic::Number(res.time_delay_ms as f64)),
                        ("originalCharCount", count(res.original_char_count)),
                        ("originalLineCount", count(res.original_line_count)),
                        ("originalDeletedLineCount", count(res.original_deleted_line_count)),
                        ("arc", count(res.arc)),
                        ("currentLineCount", count(res.current_line_count)),
                        ("currentDeletedLineCount", count(res.current_deleted_line_count)),
                    ]);
                    public_log2_forwarding(&*res.telemetry_service, "editTelemetry.reportInlineEditArc", event, forward_to_channel_if(is_copilot_like_extension(extension_id)));
                },
            );
        }));

        Rc::new(Self { base })
    }
}

impl IDisposable for EditTelemetryReportInlineEditArcSender {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `CreateSuggestionIdForChatOrInlineChatCaller`.
pub struct CreateSuggestionIdForChatOrInlineChatCaller {
    base: Rc<Disposable>,
}

impl Injectable for CreateSuggestionIdForChatOrInlineChatCaller {
    type Args = SourceAnnotatedDocument;
    type Deps = (Rc<dyn IAiEditTelemetryService>,);

    fn create(doc_with_annotated_edits: Self::Args, (ai_edit_telemetry_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();

        base.register(run_on_change::<Rc<StringText>, AnnotatedEdit<EditSourceData>>(doc_with_annotated_edits.value(), move |_val, prev, changes| {
            let edit = AnnotatedEdit::compose_all(changes);

            let Some(data) = chat_edit_source(&edit) else {
                return;
            };
            let mut apply_code_block_suggestion_id: Option<EditSuggestionId> = None;
            let feature = if data.source == "Chat.applyEdits" {
                if data.metadata_value("$$mode") == Some("applyCodeBlock") {
                    apply_code_block_suggestion_id = data.metadata_value("$$codeBlockSuggestionId").map(EditSuggestionId::from_serialized);
                }
                EditTelemetryFeature::SideBarChat
            } else {
                EditTelemetryFeature::InlineChat
            };

            let provider_id = ProviderId::new(
                data.metadata_value("$extensionId").map(str::to_owned),
                data.metadata_value("$extensionVersion").map(str::to_owned),
                data.metadata_value("$providerId").map(str::to_owned),
            );

            // Pendência do original: amarrar este id de sugestão aos hunks, para a aceitação poder ser correlacionada.
            ai_edit_telemetry_service.create_suggestion_id(&EditTelemetryCodeSuggestedData {
                base: EditTelemetryBaseData {
                    suggestion_id: None,
                    feature: Some(feature),
                    presentation: EditTelemetryPresentation::HighlightedEdit,
                    source: Some(provider_id),
                    language_id: data.metadata_value("$$languageId").map(str::to_owned),
                    edit_delta_info: Some(EditDeltaInfo::from_edit(&edit, prev)),
                    mode_id: data.metadata_value("$$mode").map(str::to_owned),
                    apply_code_block_suggestion_id,
                    model_id: data.metadata_value("$modelId").map(str::to_owned),
                    source_request_id: None,
                    is_agent_host_session: None,
                },
                provider: None,
            });
        }));

        Rc::new(Self { base })
    }
}

impl IDisposable for CreateSuggestionIdForChatOrInlineChatCaller {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `EditTelemetryReportEditArcForChatOrInlineChatSender`.
pub struct EditTelemetryReportEditArcForChatOrInlineChatSender {
    base: Rc<Disposable>,
}

impl Injectable for EditTelemetryReportEditArcForChatOrInlineChatSender {
    type Args = (SourceAnnotatedDocument, ScmRepoObservable);
    type Deps = (Rc<InstantiationService>, Rc<dyn IRandomService>);

    fn create((doc_with_annotated_edits, scm_repo_bridge): Self::Args, (instantiation_service, random_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());

        base.register(run_on_change::<Rc<StringText>, AnnotatedEdit<EditSourceData>>(doc_with_annotated_edits.value(), move |_val, prev, changes| {
            let edit = AnnotatedEdit::compose_all(changes);

            let Some(data) = chat_edit_source(&edit) else {
                return;
            };

            let unique_edit_id = random_service.generate_uuid();

            start_arc_reporter(&store, &instantiation_service, &doc_with_annotated_edits, &scm_repo_bridge, &[0, 60, 300], prev, edit, move |res| {
                let extension_id = data.metadata_value("$extensionId");
                let event = EditArcTelemetryEvent {
                    source_key_cleaned: data.to_key(
                        usize::MAX,
                        &[
                            ("$extensionId", false),
                            ("$extensionVersion", false),
                            ("$$requestUuid", false),
                            ("$$sessionId", false),
                            ("$$requestId", false),
                            ("$$languageId", false),
                            ("$modelId", false),
                            ("$autoTier", false),
                            ("$fusionModel", false),
                        ],
                    ),
                    extension_id: extension_id.map(str::to_owned),
                    extension_version: data.metadata_value("$extensionVersion").map(str::to_owned),
                    opportunity_id: data.metadata_value("$$requestUuid").map(str::to_owned),
                    edit_session_id: data.metadata_value("$$sessionId").map(str::to_owned),
                    request_id: data.metadata_value("$$requestId").map(str::to_owned),
                    model_id: data.metadata_value("$modelId").map(str::to_owned),
                    language_id: data.metadata_value("$$languageId").map(str::to_owned),
                    mode: data.metadata_value("$$mode").map(str::to_owned),
                    unique_edit_id: Some(unique_edit_id.clone()),
                    provider: None,
                    agent_session_id: None,
                    is_subagent_session: None,

                    did_branch_change: branch_change_flag(res.did_branch_change),
                    time_delay_ms: res.time_delay_ms,

                    original_char_count: res.original_char_count,
                    original_line_count: res.original_line_count,
                    original_deleted_line_count: res.original_deleted_line_count,
                    arc: res.arc,
                    current_line_count: res.current_line_count,
                    current_deleted_line_count: res.current_deleted_line_count,
                };
                public_log2_forwarding(&*res.telemetry_service, "editTelemetry.reportEditArc", event.to_dynamic(), forward_to_channel_if(is_copilot_like_extension(extension_id)));
            });
        }));

        Rc::new(Self { base })
    }
}

impl IDisposable for EditTelemetryReportEditArcForChatOrInlineChatSender {
    fn dispose(&self) {
        self.base.dispose();
    }
}
