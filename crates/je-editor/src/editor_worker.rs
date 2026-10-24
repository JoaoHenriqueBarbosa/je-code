//! Porte parcial de `src/vs/editor/common/services/editorWorker.ts`: `IEditorWorkerService`, só com o que o
//! `WordDistance` do módulo `suggest` lê (`canComputeWordRanges` e `computeWordRanges`).
//!
//! As demais operações do serviço (cálculo de diferenças, edições de espaço em branco, links, cores, `navigateValueSet`,
//! sincronização de modelos) entram junto das contribuições que as usam.
//!
//! Representação:
//! - `{ [word: string]: IRange[] }` é o [`WordRanges`], um mapa de palavra para os intervalos em que ela ocorre, na ordem
//!   do texto.
//! - `Promise<X | null>` é a `Promise<Option<X>>` (`null` é `None`).

use std::collections::HashMap;

use je_base::JsString;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use crate::edits::string_edit::StringEdit;
use crate::range::Range;

/// `DiffAlgorithmName`: `'legacy' | 'advanced' | 'advanced-external' | 'advanced-wasm'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffAlgorithmName {
    Legacy,
    Advanced,
    /// O `getExternalLinesDiffComputer(false)` do original carrega um módulo externo, que o porte não tem: usa o
    /// mesmo algoritmo de `Advanced`.
    AdvancedExternal,
    /// Idem `AdvancedExternal` (`getExternalLinesDiffComputer(true)`, a versão wasm).
    AdvancedWasm,
}

/// `{ [word: string]: IRange[] }`.
pub type WordRanges = HashMap<JsString, Vec<Range>>;

/// `IEditorWorkerService` (ver o topo do módulo).
pub trait IEditorWorkerService {
    /// `canComputeWordRanges(resource)`.
    fn can_compute_word_ranges(&self, resource: &URI) -> bool;

    /// `computeWordRanges(resource, range)`: as palavras de `range` e onde elas ocorrem, ou `None` quando o modelo
    /// não está no worker.
    fn compute_word_ranges(&self, resource: &URI, range: Range) -> Promise<Option<WordRanges>>;

    /// `computeStringEditFromDiff(original, modified, { maxComputationTimeMs }, algorithm)`. O worker roda no mesmo
    /// thread, então o padrão calcula a diferença direto (o `$computeStringDiff` do worker); o `catch` do original,
    /// que cai na substituição do texto todo, só existe para a falha do IPC com o worker.
    fn compute_string_edit_from_diff(&self, original: &JsString, modified: &JsString, max_computation_time_ms: f64, algorithm: DiffAlgorithmName) -> Promise<StringEdit> {
        Promise::resolved(crate::editor_web_worker::compute_string_diff(original, modified, max_computation_time_ms, algorithm))
    }
}

service_id!(pub I_EDITOR_WORKER_SERVICE: dyn IEditorWorkerService = "editorWorkerService");
