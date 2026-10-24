//! Porte de `src/vs/workbench/contrib/editTelemetry/browser/telemetry/aiEditTelemetry/aiEditTelemetryServiceImpl.ts`: o
//! [`AiEditTelemetryServiceImpl`], que publica `editTelemetry.codeSuggested`, `editTelemetry.codeAccepted` e
//! `editTelemetry.codeRejected`.
//!
//! Representação:
//! - `@IInstantiationService` e `createInstance(DataChannelForwardingTelemetryService)` deixam de existir: o
//!   `DataChannelForwardingTelemetryService` só repassa ao `ITelemetryService` real (o `IDataChannelService` que ele
//!   consulta para o encaminhamento ainda não foi portado, ver `forwarding_telemetry`), então a dependência é o
//!   próprio [`ITelemetryService`] e a marca `...forwardToChannelIf(...)` acompanha o dado em
//!   [`public_log2_forwarding`];
//! - `publicLog2<E, T>` (a classificação GDPR é só tipo do TypeScript) é `public_log2` com o dado montado em
//!   [`Dynamic`], na mesma ordem das chaves do original; as propriedades `undefined` são [`Dynamic::Undefined`], que a
//!   serialização do evento omite como o `JSON.stringify` do original;
//! - os campos comuns às três mensagens (de `eventId` a `isAgentHostSession`) saem de [`base_entries`], porque o
//!   original os repete literalmente;
//! - `suggestionId as unknown as string` é [`EditSuggestionId::as_str`]; `data.provider || (data.isAgentHostSession ?
//!   'unknown' : undefined)` é [`provider_or_unknown`].

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_editor::text_model_edit_source::EditSuggestionId;
use je_instantiation::{Injectable, service_impl};
use je_platform::telemetry::edit_telemetry::optional_string;
use je_platform::telemetry::telemetry::{ITelemetryService, escape_model_id_for_telemetry};

use super::ai_edit_telemetry_service::{
    EditTelemetryBaseData, EditTelemetryCodeAcceptedData, EditTelemetryCodeRejectedData, EditTelemetryCodeSuggestedData, IAiEditTelemetryService,
};
use crate::contrib::edit_telemetry::browser::random_service::IRandomService;
use crate::contrib::edit_telemetry::browser::telemetry::forwarding_telemetry::{forward_to_channel_if, is_copilot_like_extension, public_log2_forwarding};

/// `AiEditTelemetryServiceImpl`.
pub struct AiEditTelemetryServiceImpl {
    telemetry_service: Rc<dyn ITelemetryService>,
    random_service: Rc<dyn IRandomService>,
}

/// `number | undefined` como valor do evento.
fn optional_number(value: Option<i32>) -> Dynamic {
    value.map_or(Dynamic::Undefined, |n| Dynamic::Number(f64::from(n)))
}

/// `boolean | undefined` como valor do evento.
fn optional_bool(value: Option<bool>) -> Dynamic {
    value.map_or(Dynamic::Undefined, Dynamic::Bool)
}

/// `data.provider || (data.isAgentHostSession ? 'unknown' : undefined)`.
fn provider_or_unknown(provider: Option<&str>, is_agent_host_session: Option<bool>) -> Dynamic {
    match provider.filter(|p| !p.is_empty()) {
        Some(provider) => Dynamic::string(provider),
        None if is_agent_host_session == Some(true) => Dynamic::string("unknown"),
        None => Dynamic::Undefined,
    }
}

/// As propriedades comuns às três mensagens, de `eventId` a `isAgentHostSession`.
fn base_entries(event_id: String, suggestion_id: Option<&EditSuggestionId>, data: &EditTelemetryBaseData) -> Vec<(&'static str, Dynamic)> {
    let source = data.source.as_ref();
    let delta = data.edit_delta_info.as_ref();
    vec![
        ("eventId", Dynamic::string(&event_id)),
        ("suggestionId", optional_string(suggestion_id.map(EditSuggestionId::as_str))),
        ("presentation", Dynamic::string(data.presentation.as_str())),
        ("feature", optional_string(data.feature.map(|f| f.as_str()))),
        ("sourceExtensionId", optional_string(source.and_then(|s| s.extension_id.as_deref()))),
        ("sourceExtensionVersion", optional_string(source.and_then(|s| s.extension_version.as_deref()))),
        ("sourceProviderId", optional_string(source.and_then(|s| s.provider_id.as_deref()))),
        ("languageId", optional_string(data.language_id.as_deref())),
        ("editCharsInserted", optional_number(delta.map(|d| d.chars_added))),
        ("editCharsDeleted", optional_number(delta.map(|d| d.chars_removed))),
        ("editLinesInserted", optional_number(delta.map(|d| d.lines_added))),
        ("editLinesDeleted", optional_number(delta.map(|d| d.lines_removed))),
        ("modeId", optional_string(data.mode_id.as_deref())),
        ("modelId", optional_string(escape_model_id_for_telemetry(data.model_id.as_deref()).as_deref())),
        ("applyCodeBlockSuggestionId", optional_string(data.apply_code_block_suggestion_id.as_ref().map(EditSuggestionId::as_str))),
        ("sourceRequestId", optional_string(data.source_request_id.as_deref())),
        ("isAgentHostSession", optional_bool(data.is_agent_host_session)),
    ]
}

impl AiEditTelemetryServiceImpl {
    /// `this._telemetryService.publicLog2(eventName, { ...entries, ...forwardToChannelIf(isCopilotLikeExtension(...)) })`.
    fn publish(&self, event_name: &str, entries: Vec<(&'static str, Dynamic)>, data: &EditTelemetryBaseData) {
        let extension_id = data.source.as_ref().and_then(|s| s.extension_id.as_deref());
        public_log2_forwarding(&*self.telemetry_service, event_name, Dynamic::object(entries), forward_to_channel_if(is_copilot_like_extension(extension_id)));
    }

    fn new_event_id(&self) -> String {
        self.random_service.generate_prefixed_uuid("evt")
    }
}

impl Injectable for AiEditTelemetryServiceImpl {
    type Args = ();
    type Deps = (Rc<dyn ITelemetryService>, Rc<dyn IRandomService>);

    fn create(_: (), (telemetry_service, random_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { telemetry_service, random_service })
    }
}

impl IAiEditTelemetryService for AiEditTelemetryServiceImpl {
    fn create_suggestion_id(&self, data: &EditTelemetryCodeSuggestedData) -> EditSuggestionId {
        let suggestion_id = EditSuggestionId::new_id(Some(&|ns: &str| self.random_service.generate_prefixed_uuid(ns)));
        let mut entries = base_entries(self.new_event_id(), Some(&suggestion_id), &data.base);
        entries.push(("provider", provider_or_unknown(data.provider.as_deref(), data.base.is_agent_host_session)));
        self.publish("editTelemetry.codeSuggested", entries, &data.base);
        suggestion_id
    }

    fn handle_code_accepted(&self, data: &EditTelemetryCodeAcceptedData) {
        let mut entries = base_entries(self.new_event_id(), data.base.suggestion_id.as_ref(), &data.base);
        entries.push(("chatSessionId", optional_string(data.chat_session_id.as_deref())));
        entries.push(("agentSessionId", optional_string(data.agent_session_id.as_deref())));
        entries.push(("provider", provider_or_unknown(data.provider.as_deref(), data.base.is_agent_host_session)));
        entries.push(("acceptanceMethod", Dynamic::string(data.acceptance_method.as_str())));
        self.publish("editTelemetry.codeAccepted", entries, &data.base);
    }

    fn handle_code_rejected(&self, data: &EditTelemetryCodeRejectedData) {
        let mut entries = base_entries(self.new_event_id(), data.base.suggestion_id.as_ref(), &data.base);
        entries.push(("rejectionMethod", Dynamic::string(data.rejection_method.as_str())));
        entries.push(("chatSessionId", optional_string(data.chat_session_id.as_deref())));
        entries.push(("agentSessionId", optional_string(data.agent_session_id.as_deref())));
        self.publish("editTelemetry.codeRejected", entries, &data.base);
    }
}

service_impl!(AiEditTelemetryServiceImpl => dyn IAiEditTelemetryService);
