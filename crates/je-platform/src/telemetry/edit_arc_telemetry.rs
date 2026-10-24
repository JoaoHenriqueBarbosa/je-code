//! Porte de `src/vs/platform/telemetry/common/editArcTelemetry.ts`: o evento `editTelemetry.reportEditArc`.
//!
//! Divergência: `IEditArcTelemetryClassification` é só tipo do compilador e não existe aqui. O dado do evento é um
//! objeto [`Dynamic`], com todas as propriedades presentes (as de texto com `undefined` quando vazias), na ordem do
//! `IEditArcTelemetryEvent`. `isSubagentSession` é `string | undefined` no original e o porte mantém o tipo.

use je_base::dynamic::Dynamic;

use super::edit_telemetry::optional_string;

pub const EDIT_TELEMETRY_SETTING_ID: &str = "telemetry.editStats.enabled";

/// `IEditArcTelemetryEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct EditArcTelemetryEvent {
    pub source_key_cleaned: String,
    pub extension_id: Option<String>,
    pub extension_version: Option<String>,
    pub opportunity_id: Option<String>,
    pub edit_session_id: Option<String>,
    pub request_id: Option<String>,
    pub model_id: Option<String>,
    pub language_id: Option<String>,
    pub mode: Option<String>,
    pub unique_edit_id: Option<String>,
    pub provider: Option<String>,
    pub agent_session_id: Option<String>,
    pub is_subagent_session: Option<String>,

    pub did_branch_change: i32,
    pub time_delay_ms: u64,
    pub original_char_count: usize,
    pub original_line_count: usize,
    pub original_deleted_line_count: usize,
    pub arc: usize,
    pub current_line_count: usize,
    pub current_deleted_line_count: usize,
}

impl EditArcTelemetryEvent {
    /// O objeto do evento.
    pub fn to_dynamic(&self) -> Dynamic {
        let count = |value: usize| Dynamic::Number(value as f64);
        Dynamic::object([
            ("sourceKeyCleaned", Dynamic::string(&self.source_key_cleaned)),
            ("extensionId", optional_string(self.extension_id.as_deref())),
            ("extensionVersion", optional_string(self.extension_version.as_deref())),
            ("opportunityId", optional_string(self.opportunity_id.as_deref())),
            ("editSessionId", optional_string(self.edit_session_id.as_deref())),
            ("requestId", optional_string(self.request_id.as_deref())),
            ("modelId", optional_string(self.model_id.as_deref())),
            ("languageId", optional_string(self.language_id.as_deref())),
            ("mode", optional_string(self.mode.as_deref())),
            ("uniqueEditId", optional_string(self.unique_edit_id.as_deref())),
            ("provider", optional_string(self.provider.as_deref())),
            ("agentSessionId", optional_string(self.agent_session_id.as_deref())),
            ("isSubagentSession", optional_string(self.is_subagent_session.as_deref())),
            ("didBranchChange", Dynamic::Number(f64::from(self.did_branch_change))),
            ("timeDelayMs", Dynamic::Number(self.time_delay_ms as f64)),
            ("originalCharCount", count(self.original_char_count)),
            ("originalLineCount", count(self.original_line_count)),
            ("originalDeletedLineCount", count(self.original_deleted_line_count)),
            ("arc", count(self.arc)),
            ("currentLineCount", count(self.current_line_count)),
            ("currentDeletedLineCount", count(self.current_deleted_line_count)),
        ])
    }
}
