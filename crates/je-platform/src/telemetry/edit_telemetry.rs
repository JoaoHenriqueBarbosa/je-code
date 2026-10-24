//! Porte de `src/vs/platform/telemetry/common/editTelemetry.ts`: os eventos `editTelemetry.editSources.details` e
//! `editTelemetry.editSources.stats`.
//!
//! Divergências: as classificações GDPR (`EditSourcesDetailsTelemetryClassification`,
//! `EditSourcesStatsTelemetryClassification`) são só tipos do compilador e não existem aqui. O dado do evento é um
//! objeto [`Dynamic`] (o `publicLog2` do porte), montado na ordem dos campos do original. Os campos opcionais do
//! TypeScript (`agentSessionId?`, `chatSessionId?`, `copilotSku?`, `trackingScope?` e os três de cobertura) só viram
//! propriedade do objeto quando têm valor, como o `...(x !== undefined ? { x } : {})` do original; os demais campos
//! (`extensionId: string | undefined` e semelhantes) são sempre propriedade, com `undefined` quando vazios.

use je_base::dynamic::Dynamic;

use super::telemetry::ITelemetryService;

/// `EditTelemetryMode`: `'longterm' | '10minFocusWindow' | '20minFocusWindow'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditTelemetryMode {
    Longterm,
    TenMinFocusWindow,
    TwentyMinFocusWindow,
}

impl EditTelemetryMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Longterm => "longterm",
            Self::TenMinFocusWindow => "10minFocusWindow",
            Self::TwentyMinFocusWindow => "20minFocusWindow",
        }
    }
}

/// `EditTelemetryTrigger`: `'10hours' | 'hashChange' | 'branchChange' | 'closed' | 'time'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditTelemetryTrigger {
    TenHours,
    HashChange,
    BranchChange,
    Closed,
    Time,
}

impl EditTelemetryTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TenHours => "10hours",
            Self::HashChange => "hashChange",
            Self::BranchChange => "branchChange",
            Self::Closed => "closed",
            Self::Time => "time",
        }
    }
}

/// `string | undefined` como valor do evento.
pub fn optional_string(value: Option<&str>) -> Dynamic {
    match value {
        Some(text) => Dynamic::string(text),
        None => Dynamic::Undefined,
    }
}

/// `IEditSourcesDetailsTelemetryData`.
#[derive(Clone, Debug, PartialEq)]
pub struct EditSourcesDetailsTelemetryData {
    pub copilot_sku: Option<String>,
    pub mode: EditTelemetryMode,
    pub source_key: String,
    pub source_key_cleaned: String,
    pub extension_id: Option<String>,
    pub extension_version: Option<String>,
    pub model_id: Option<String>,
    pub auto_tier: Option<String>,
    /// The HydraFusion model (`hydrafusion` or `hydrafusion-max`) that routed the edit, when known.
    pub fusion_model: Option<String>,
    pub trigger: EditTelemetryTrigger,
    pub language_id: Option<String>,
    pub stats_uuid: String,
    pub conversation_id: Option<String>,
    /// Owning Agent Host session ID; unlike conversationId, never the chat's persistence-resource ID.
    pub agent_session_id: Option<String>,
    /// Hashed originating Agent Host chat ID, scoped under its session rather than globally; omitted when provenance is unknown.
    pub chat_session_id: Option<String>,
    pub request_id: Option<String>,
    pub origin: Option<String>,
    pub provider: Option<String>,
    pub modified_count: usize,
    pub delta_modified_count: usize,
    pub total_modified_count: usize,
}

impl EditSourcesDetailsTelemetryData {
    /// O objeto do evento.
    pub fn to_dynamic(&self) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = Vec::new();
        if let Some(sku) = &self.copilot_sku {
            entries.push(("copilotSku", Dynamic::string(sku)));
        }
        entries.push(("mode", Dynamic::string(self.mode.as_str())));
        entries.push(("sourceKey", Dynamic::string(&self.source_key)));
        entries.push(("sourceKeyCleaned", Dynamic::string(&self.source_key_cleaned)));
        entries.push(("extensionId", optional_string(self.extension_id.as_deref())));
        entries.push(("extensionVersion", optional_string(self.extension_version.as_deref())));
        entries.push(("modelId", optional_string(self.model_id.as_deref())));
        entries.push(("autoTier", optional_string(self.auto_tier.as_deref())));
        entries.push(("fusionModel", optional_string(self.fusion_model.as_deref())));
        entries.push(("trigger", Dynamic::string(self.trigger.as_str())));
        entries.push(("languageId", optional_string(self.language_id.as_deref())));
        entries.push(("statsUuid", Dynamic::string(&self.stats_uuid)));
        entries.push(("conversationId", optional_string(self.conversation_id.as_deref())));
        if let Some(id) = &self.agent_session_id {
            entries.push(("agentSessionId", Dynamic::string(id)));
        }
        if let Some(id) = &self.chat_session_id {
            entries.push(("chatSessionId", Dynamic::string(id)));
        }
        entries.push(("requestId", optional_string(self.request_id.as_deref())));
        entries.push(("origin", optional_string(self.origin.as_deref())));
        entries.push(("provider", optional_string(self.provider.as_deref())));
        entries.push(("modifiedCount", Dynamic::Number(self.modified_count as f64)));
        entries.push(("deltaModifiedCount", Dynamic::Number(self.delta_modified_count as f64)));
        entries.push(("totalModifiedCount", Dynamic::Number(self.total_modified_count as f64)));
        Dynamic::object(entries)
    }
}

/// `sendEditSourcesDetailsTelemetry(telemetryService, data)`.
pub fn send_edit_sources_details_telemetry(telemetry_service: &dyn ITelemetryService, data: &EditSourcesDetailsTelemetryData) {
    telemetry_service.public_log2("editTelemetry.editSources.details", Some(data.to_dynamic()));
}

/// `'complete' | 'partial'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentHostAttributionCoverage {
    Complete,
    Partial,
}

impl AgentHostAttributionCoverage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
        }
    }
}

/// Os três campos de cobertura que só o modo `longterm` envia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentHostCoverageStats {
    pub attribution_coverage: AgentHostAttributionCoverage,
    pub untracked_edit_count: usize,
    pub untracked_inserted_count: usize,
}

/// `IEditSourcesStatsTelemetryData`. O `attributionSchemaVersion` é sempre `2`.
#[derive(Clone, Debug, PartialEq)]
pub struct EditSourcesStatsTelemetryData {
    pub copilot_sku: Option<String>,
    pub mode: EditTelemetryMode,
    pub language_id: Option<String>,
    pub stats_uuid: String,
    pub nes_modified_count: usize,
    pub inline_completions_copilot_modified_count: usize,
    pub inline_completions_nes_modified_count: usize,
    pub other_ai_modified_count: usize,
    pub agent_host_modified_count: usize,
    pub unknown_modified_count: usize,
    pub user_modified_count: usize,
    pub ide_modified_count: usize,
    pub total_modified_characters: usize,
    pub external_modified_count: usize,
    pub is_tracked_by_git: Option<i32>,
    pub focus_time: Option<u64>,
    pub actual_time: Option<u64>,
    pub trigger: EditTelemetryTrigger,
    /// `trackingScope?: 'agentHostStandalone'`.
    pub tracking_scope: Option<String>,
    pub agent_host_coverage: Option<AgentHostCoverageStats>,
}

impl EditSourcesStatsTelemetryData {
    /// O objeto do evento.
    pub fn to_dynamic(&self) -> Dynamic {
        let count = |value: usize| Dynamic::Number(value as f64);
        let mut entries: Vec<(&str, Dynamic)> = Vec::new();
        if let Some(sku) = &self.copilot_sku {
            entries.push(("copilotSku", Dynamic::string(sku)));
        }
        entries.push(("attributionSchemaVersion", Dynamic::Number(2.0)));
        entries.push(("mode", Dynamic::string(self.mode.as_str())));
        entries.push(("languageId", optional_string(self.language_id.as_deref())));
        entries.push(("statsUuid", Dynamic::string(&self.stats_uuid)));
        entries.push(("nesModifiedCount", count(self.nes_modified_count)));
        entries.push(("inlineCompletionsCopilotModifiedCount", count(self.inline_completions_copilot_modified_count)));
        entries.push(("inlineCompletionsNESModifiedCount", count(self.inline_completions_nes_modified_count)));
        entries.push(("otherAIModifiedCount", count(self.other_ai_modified_count)));
        entries.push(("agentHostModifiedCount", count(self.agent_host_modified_count)));
        entries.push(("unknownModifiedCount", count(self.unknown_modified_count)));
        entries.push(("userModifiedCount", count(self.user_modified_count)));
        entries.push(("ideModifiedCount", count(self.ide_modified_count)));
        entries.push(("totalModifiedCharacters", count(self.total_modified_characters)));
        entries.push(("externalModifiedCount", count(self.external_modified_count)));
        entries.push(("isTrackedByGit", self.is_tracked_by_git.map_or(Dynamic::Undefined, |v| Dynamic::Number(f64::from(v)))));
        entries.push(("focusTime", self.focus_time.map_or(Dynamic::Undefined, |v| Dynamic::Number(v as f64))));
        entries.push(("actualTime", self.actual_time.map_or(Dynamic::Undefined, |v| Dynamic::Number(v as f64))));
        entries.push(("trigger", Dynamic::string(self.trigger.as_str())));
        if let Some(scope) = &self.tracking_scope {
            entries.push(("trackingScope", Dynamic::string(scope)));
        }
        if let Some(coverage) = &self.agent_host_coverage {
            entries.push(("agentHostAttributionCoverage", Dynamic::string(coverage.attribution_coverage.as_str())));
            entries.push(("agentHostUntrackedEditCount", count(coverage.untracked_edit_count)));
            entries.push(("agentHostUntrackedInsertedCount", count(coverage.untracked_inserted_count)));
        }
        Dynamic::object(entries)
    }
}

/// `sendEditSourcesStatsTelemetry(telemetryService, data)`.
pub fn send_edit_sources_stats_telemetry(telemetry_service: &dyn ITelemetryService, data: &EditSourcesStatsTelemetryData) {
    telemetry_service.public_log2("editTelemetry.editSources.stats", Some(data.to_dynamic()));
}
