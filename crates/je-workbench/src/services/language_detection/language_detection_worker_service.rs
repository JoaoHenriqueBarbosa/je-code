//! Porte de `src/vs/workbench/services/languageDetection/common/languageDetectionWorkerService.ts`: o contrato
//! `ILanguageDetectionService` e as constantes e tipos de dados que os consumidores leem dele.
//!
//! Divergências:
//! - Os tipos de classificação de telemetria (`AutomaticLanguageDetectionLikelyWrongClassification` e
//!   `LanguageDetectionStatsClassification`) são só tipagem de TypeScript e não têm forma em tempo de execução, então não
//!   são portados. Os dados que eles descrevem ([`AutomaticLanguageDetectionLikelyWrongData`] e
//!   [`LanguageDetectionStats`]) são.
//! - O serviço de detecção em si (o worker com o modelo de aprendizado) ainda não foi portado.

use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

/// `LanguageDetectionLanguageEventSource`.
pub const LANGUAGE_DETECTION_LANGUAGE_EVENT_SOURCE: &str = "languageDetection";

/// `ILanguageDetectionService`.
pub trait ILanguageDetectionService {
    /// `languageId`: the language id to check if language detection is currently enabled. Devolve se a detecção está
    /// ligada para essa linguagem.
    fn is_enabled_for_language(&self, language_id: &str) -> bool;

    /// `resource`: the resource to detect the language for. `supportedLangs`: when populated, the model will only return
    /// languages from the provided list.
    ///
    /// Devolve o id da linguagem do recurso, ou `None` se o modelo não tem confiança suficiente.
    fn detect_language(&self, resource: &URI, supported_langs: Option<&[String]>) -> Promise<Option<String>>;
}

service_id!(pub I_LANGUAGE_DETECTION_SERVICE: dyn ILanguageDetectionService = "ILanguageDetectionService");

/// `LanguageDetectionHintConfig`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LanguageDetectionHintConfig {
    pub untitled_editors: bool,
    pub notebook_editors: bool,
}

/// `AutomaticLanguageDetectionLikelyWrongId`.
pub const AUTOMATIC_LANGUAGE_DETECTION_LIKELY_WRONG_ID: &str = "automaticlanguagedetection.likelywrong";

/// `IAutomaticLanguageDetectionLikelyWrongData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomaticLanguageDetectionLikelyWrongData {
    pub current_language_id: String,
    pub next_language_id: String,
    pub line_count: usize,
    pub model_preference: String,
}

/// `LanguageDetectionStatsId`.
pub const LANGUAGE_DETECTION_STATS_ID: &str = "automaticlanguagedetection.stats";

/// `ILanguageDetectionStats`.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageDetectionStats {
    pub languages: String,
    pub confidences: String,
    pub time_spent: f64,
}
