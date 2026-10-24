//! O trecho de `src/vs/workbench/contrib/editTelemetry/browser/telemetry/agentHostEditMarkerService.ts` que o
//! `editTracker.ts` e o `editSourceTrackingImpl.ts` leem: `IExternalEditCorrelation`,
//! `IExternalEditCorrelationResolution`, `IAgentHostEditMarkerService`, `IPreparedAgentHostEditAttributionFlush`,
//! `IAgentHostEditAttributionCoverageGap` e os dois erros. O serviço de marcadores do agent host
//! (`AgentHostEditMarkerService`) depende das conexões do agent host e entra quando elas forem portadas.
//!
//! Representação:
//! - os membros opcionais do original (`onDidResolve?`, `getResolution?`, `waitForResolution?`, `takeCoverageGap?`)
//!   são métodos com implementação padrão que representam a ausência (`None` e uma promessa já resolvida);
//! - `IPreparedAgentHostEditAttributionFlush` é uma struct: os campos opcionais são `Option` (`deferCoverageGap?` é
//!   `bool`, falso quando ausente) e `commit(totalModifiedCount)` é um fecho que devolve a promessa;
//! - `AgentHostEditAttributionUnknownOutcomeError` e `AgentHostEditAttributionDeferredError` (subclasses de `Error`
//!   com `cause`) são `Error` com o `name` da classe; `instanceof` é a conferência do `name`
//!   ([`is_agent_host_edit_attribution_deferred_error`], [`is_agent_host_edit_attribution_unknown_outcome_error`]).

use std::rc::Rc;

use je_base::JsString;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::text_model_edit_source::TextModelEditSource;
use je_platform::telemetry::edit_telemetry::EditTelemetryTrigger;

/// `IExternalEditCorrelationResolution`.
#[derive(Clone, Debug)]
pub struct IExternalEditCorrelationResolution {
    pub id: String,
    pub source: Option<Rc<TextModelEditSource>>,
}

/// `IAgentHostEditAttributionCoverageGap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IAgentHostEditAttributionCoverageGap {
    pub edit_count: usize,
    pub inserted_count: usize,
}

/// `IPreparedAgentHostEditAttributionFlush`.
#[derive(Clone)]
pub struct IPreparedAgentHostEditAttributionFlush {
    pub flush_token: String,
    pub agent_modified_count: usize,
    pub last_sequence: Option<u64>,
    pub coverage_gap_through_sequence: Option<u64>,
    pub defer_coverage_gap: bool,
    /// `commit(totalModifiedCount)`.
    pub commit: Rc<dyn Fn(usize) -> Promise<()>>,
}

impl IPreparedAgentHostEditAttributionFlush {
    /// Um flush com só os campos obrigatórios (`flushToken`, `agentModifiedCount` e `commit`).
    pub fn new(flush_token: impl Into<String>, agent_modified_count: usize, commit: impl Fn(usize) -> Promise<()> + 'static) -> Self {
        Self {
            flush_token: flush_token.into(),
            agent_modified_count,
            last_sequence: None,
            coverage_gap_through_sequence: None,
            defer_coverage_gap: false,
            commit: Rc::new(commit),
        }
    }
}

const AGENT_HOST_EDIT_ATTRIBUTION_UNKNOWN_OUTCOME_ERROR: &str = "AgentHostEditAttributionUnknownOutcomeError";
const AGENT_HOST_EDIT_ATTRIBUTION_DEFERRED_ERROR: &str = "AgentHostEditAttributionDeferredError";

/// `new AgentHostEditAttributionUnknownOutcomeError(cause)`.
pub fn agent_host_edit_attribution_unknown_outcome_error(cause: Error) -> Error {
    Error::named(AGENT_HOST_EDIT_ATTRIBUTION_UNKNOWN_OUTCOME_ERROR, "The Agent Host edit attribution outcome is unknown").with_cause(cause)
}

/// `error instanceof AgentHostEditAttributionUnknownOutcomeError`.
pub fn is_agent_host_edit_attribution_unknown_outcome_error(error: &Error) -> bool {
    error.name == AGENT_HOST_EDIT_ATTRIBUTION_UNKNOWN_OUTCOME_ERROR
}

/// `new AgentHostEditAttributionDeferredError(cause)`.
pub fn agent_host_edit_attribution_deferred_error(cause: Error) -> Error {
    Error::named(AGENT_HOST_EDIT_ATTRIBUTION_DEFERRED_ERROR, "The Agent Host edit attribution was deferred").with_cause(cause)
}

/// `error instanceof AgentHostEditAttributionDeferredError`.
pub fn is_agent_host_edit_attribution_deferred_error(error: &Error) -> bool {
    error.name == AGENT_HOST_EDIT_ATTRIBUTION_DEFERRED_ERROR
}

/// `IAgentHostEditMarkerService`.
pub trait IAgentHostEditMarkerService {
    fn create_correlation(&self, resource: &URI) -> Rc<dyn IExternalEditCorrelation>;

    /// `takeCoverageGap?(resource, throughSequence)`: `None` quando o serviço não tem a operação.
    fn take_coverage_gap(&self, _resource: &URI, _through_sequence: Option<u64>) -> Option<IAgentHostEditAttributionCoverageGap> {
        None
    }

    fn prepare_flush(
        &self,
        resource: &URI,
        trigger: EditTelemetryTrigger,
        stats_uuid: &str,
        is_dirty: bool,
        language_id: Option<&str>,
    ) -> Promise<Option<IPreparedAgentHostEditAttributionFlush>>;
}

/// `IExternalEditCorrelation`.
pub trait IExternalEditCorrelation {
    fn on_did_suppress(&self) -> Event<String>;

    /// `onDidResolve?`: `None` quando o correlacionador não resolve por evento.
    fn on_did_resolve(&self) -> Option<Event<IExternalEditCorrelationResolution>> {
        None
    }

    fn on_did_invalidate(&self) -> Event<String>;

    fn register(&self, before: &JsString, after: &JsString) -> String;

    fn is_suppressed(&self, id: &str) -> bool;

    /// `getResolution?(id)`.
    fn get_resolution(&self, _id: &str) -> Option<IExternalEditCorrelationResolution> {
        None
    }

    /// `waitForResolution?(ids, timeoutMs)`.
    fn wait_for_resolution(&self, _ids: &[String], _timeout_ms: u64) -> Promise<()> {
        Promise::resolved(())
    }

    fn release(&self, id: &str);
}
