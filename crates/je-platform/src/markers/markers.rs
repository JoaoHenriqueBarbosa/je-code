//! Porte de `src/vs/platform/markers/common/markers.ts`: os tipos de marcador e o contrato do serviço.

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::nls::localize;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_instantiation::service_id;

/// `IMarkerReadOptions`.
#[derive(Clone, Debug, Default)]
pub struct MarkerReadOptions {
    pub owner: Option<String>,
    pub resource: Option<URI>,
    /// Máscara de bits de [`MarkerSeverity`]; `None` aceita todas.
    pub severities: Option<u32>,
    pub take: Option<i64>,
    pub ignore_resource_filters: bool,
}

/// `IMarkerChangeOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MarkerChangeOptions {
    pub is_truncated: bool,
}

/// `IMarkerService`.
pub trait IMarkerService {
    fn get_statistics(&self) -> MarkerStatistics;
    fn is_source_truncated(&self, resource: &URI) -> bool;
    fn change_one(&self, owner: &str, resource: &URI, markers: Vec<MarkerData>, options: Option<MarkerChangeOptions>);
    fn change_all(&self, owner: &str, data: Vec<ResourceMarker>);
    fn remove(&self, owner: &str, resources: &[URI]);
    fn read(&self, filter: Option<&MarkerReadOptions>) -> Vec<Marker>;
    fn install_resource_filter(&self, resource: &URI, reason: &str) -> DisposableRef;
    fn on_marker_changed(&self) -> Event<Vec<URI>>;
}

service_id!(pub I_MARKER_SERVICE: dyn IMarkerService = "markerService");

/// `IRelatedInformation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedInformation {
    pub resource: URI,
    pub message: String,
    pub start_line_number: i64,
    pub start_column: i64,
    pub end_line_number: i64,
    pub end_column: i64,
}

/// `MarkerTag`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerTag {
    Unnecessary = 1,
    Deprecated = 2,
}

/// `MarkerSeverity`: um número, para aceitar o `0 as MarkerSeverity` que os dados de extensões trazem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MarkerSeverity(pub u32);

impl MarkerSeverity {
    pub const HINT: MarkerSeverity = MarkerSeverity(1);
    pub const INFO: MarkerSeverity = MarkerSeverity(2);
    pub const WARNING: MarkerSeverity = MarkerSeverity(4);
    pub const ERROR: MarkerSeverity = MarkerSeverity(8);

    /// `MarkerSeverity.compare`: o mais grave primeiro.
    pub fn compare(a: MarkerSeverity, b: MarkerSeverity) -> i64 {
        i64::from(b.0) - i64::from(a.0)
    }

    /// `MarkerSeverity.toString`.
    pub fn to_display_string(self) -> String {
        match self {
            MarkerSeverity::ERROR => localize("sev.error", "Error", &[]),
            MarkerSeverity::WARNING => localize("sev.warning", "Warning", &[]),
            MarkerSeverity::INFO => localize("sev.info", "Info", &[]),
            _ => String::new(),
        }
    }

    /// `MarkerSeverity.toStringPlural`.
    pub fn to_string_plural(self) -> String {
        match self {
            MarkerSeverity::ERROR => localize("sev.errors", "Errors", &[]),
            MarkerSeverity::WARNING => localize("sev.warnings", "Warnings", &[]),
            MarkerSeverity::INFO => localize("sev.infos", "Infos", &[]),
            _ => String::new(),
        }
    }

    /// `MarkerSeverity.fromSeverity`.
    pub fn from_severity(severity: Severity) -> MarkerSeverity {
        match severity {
            Severity::Error => MarkerSeverity::ERROR,
            Severity::Warning => MarkerSeverity::WARNING,
            Severity::Info => MarkerSeverity::INFO,
            Severity::Ignore => MarkerSeverity::HINT,
        }
    }

    /// `MarkerSeverity.toSeverity`: `None` para um valor fora dos quatro (o `switch` do original devolve `undefined`).
    pub fn to_severity(self) -> Option<Severity> {
        match self {
            MarkerSeverity::ERROR => Some(Severity::Error),
            MarkerSeverity::WARNING => Some(Severity::Warning),
            MarkerSeverity::INFO => Some(Severity::Info),
            MarkerSeverity::HINT => Some(Severity::Ignore),
            _ => None,
        }
    }
}

/// `code?: string | { value: string; target: URI }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerCode {
    Text(String),
    Target { value: String, target: URI },
}

impl MarkerCode {
    fn value(&self) -> &str {
        match self {
            MarkerCode::Text(text) => text,
            MarkerCode::Target { value, .. } => value,
        }
    }
}

/// `IMarkerData`.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkerData {
    pub code: Option<MarkerCode>,
    pub severity: MarkerSeverity,
    /// Vazia (ou ausente no original) invalida o marcador.
    pub message: String,
    pub source: Option<String>,
    pub start_line_number: i64,
    pub start_column: i64,
    pub end_line_number: i64,
    pub end_column: i64,
    pub model_version_id: Option<i64>,
    pub related_information: Option<Vec<RelatedInformation>>,
    pub tags: Option<Vec<MarkerTag>>,
    pub origin: Option<String>,
}

impl MarkerData {
    pub fn new(severity: MarkerSeverity, message: impl Into<String>, start_line_number: i64, start_column: i64, end_line_number: i64, end_column: i64) -> Self {
        Self {
            code: None,
            severity,
            message: message.into(),
            source: None,
            start_line_number,
            start_column,
            end_line_number,
            end_column,
            model_version_id: None,
            related_information: None,
            tags: None,
            origin: None,
        }
    }

    /// `IMarkerData.makeKey`.
    pub fn make_key(&self) -> String {
        self.make_key_optional_message(true)
    }

    /// `IMarkerData.makeKeyOptionalMessage`.
    pub fn make_key_optional_message(&self, use_message: bool) -> String {
        let escape = |text: &str| text.replacen('¦', "\\¦", 1);
        let mut result: Vec<String> = vec![String::new()];
        result.push(self.source.as_deref().filter(|s| !s.is_empty()).map_or_else(String::new, escape));
        result.push(self.code.as_ref().filter(|c| !c.value().is_empty() || matches!(c, MarkerCode::Target { .. })).map_or_else(String::new, |c| escape(c.value())));
        result.push(self.severity.to_display_string());
        // Modifed to not include the message as part of the marker key to work around
        // https://github.com/microsoft/vscode/issues/77475
        result.push(if !self.message.is_empty() && use_message { escape(&self.message) } else { String::new() });
        result.push(self.start_line_number.to_string());
        result.push(self.start_column.to_string());
        result.push(self.end_line_number.to_string());
        result.push(self.end_column.to_string());
        result.push(String::new());
        result.join("¦")
    }
}

/// `IResourceMarker`.
#[derive(Clone, Debug)]
pub struct ResourceMarker {
    pub resource: URI,
    pub marker: MarkerData,
}

/// A identidade de instância de um `IMarker`. O original compara marcadores por referência (o `MarkerDecorations#update`
/// confia em que o `MarkerService` recria cada marcador a cada `changeOne`/`changeAll` e mantém os dos outros donos); aqui cada
/// marcador criado pelo serviço recebe um número novo, e as cópias de um mesmo marcador o compartilham. Não faz parte do valor:
/// dois marcadores com os mesmos campos são iguais em `==` e no `Debug`, seja qual for a instância.
#[derive(Clone, Copy, Default)]
pub struct MarkerInstance(u64);

impl MarkerInstance {
    /// Uma instância que nenhum outro marcador tem.
    pub fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    pub fn id(self) -> u64 {
        self.0
    }
}

impl PartialEq for MarkerInstance {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for MarkerInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MarkerInstance")
    }
}

/// `IMarker`.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    /// A identidade do objeto no original (ver `MarkerInstance`).
    pub instance: MarkerInstance,
    pub owner: String,
    pub resource: URI,
    pub severity: MarkerSeverity,
    pub code: Option<MarkerCode>,
    pub message: String,
    pub source: Option<String>,
    pub start_line_number: i64,
    pub start_column: i64,
    pub end_line_number: i64,
    pub end_column: i64,
    pub model_version_id: Option<i64>,
    pub related_information: Option<Vec<RelatedInformation>>,
    pub tags: Option<Vec<MarkerTag>>,
    pub origin: Option<String>,
}

/// `MarkerStatistics`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkerStatistics {
    pub errors: i64,
    pub warnings: i64,
    pub infos: i64,
    pub unknowns: i64,
}

/// Mantém `Rc` em uso para quem precisa de serviço compartilhado.
pub type MarkerServiceRef = Rc<dyn IMarkerService>;
