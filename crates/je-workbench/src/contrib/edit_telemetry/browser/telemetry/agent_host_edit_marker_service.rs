//! O trecho de `src/vs/workbench/contrib/editTelemetry/browser/telemetry/agentHostEditMarkerService.ts` que o
//! `editTracker.ts` lê: `IExternalEditCorrelation` e `IExternalEditCorrelationResolution`. O serviço de marcadores do
//! agent host (`AgentHostEditMarkerService`) depende das conexões do agent host e entra quando elas forem portadas.
//!
//! Representação: os membros opcionais do original (`onDidResolve?`, `getResolution?`, `waitForResolution?`) são
//! métodos com implementação padrão que representam a ausência (`None` e uma promessa já resolvida).

use std::rc::Rc;

use je_base::JsString;
use je_base::event::Event;
use je_base::promise::Promise;
use je_editor::text_model_edit_source::TextModelEditSource;

/// `IExternalEditCorrelationResolution`.
#[derive(Clone, Debug)]
pub struct IExternalEditCorrelationResolution {
    pub id: String,
    pub source: Option<Rc<TextModelEditSource>>,
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
