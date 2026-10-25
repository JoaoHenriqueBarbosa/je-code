//! Porte parcial de `src/vs/editor/common/services/editorWorker.ts`: `IEditorWorkerService`, só com o que o
//! `WordDistance` do módulo `suggest` lê (`canComputeWordRanges` e `computeWordRanges`).
//!
//! `computeDiff` entra só como contrato (ver o método), para a altura dos editores de diff de notebook. As demais operações do
//! serviço (edições de espaço em branco, links, cores, `navigateValueSet`,
//! sincronização de modelos) entram junto das contribuições que as usam. `computeMoreMinimalEdits` entra só como
//! contrato (ver o método), para as funções de formatação.
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

use crate::code_action::languages::TextEdit;
use crate::diff::document_diff_provider::{DocumentDiff, DocumentDiffProviderOptions};
use crate::edits::string_edit::StringEdit;
use crate::inplace_replace_support::InplaceReplaceSupportResult;
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

    /// `computeDiff(original, modified, options, algorithm)`: o diff de linhas entre dois recursos, ou `None` quando o
    /// serviço não consegue calculá-lo. O `mock<IEditorWorkerService>()` do original deixa o membro ausente (chamá-lo lança
    /// `TypeError`), e é isso que o corpo padrão faz: quem usa o cálculo (a altura dos editores de diff de notebook) o
    /// sobrescreve.
    fn compute_diff(&self, _original: &URI, _modified: &URI, _options: &DocumentDiffProviderOptions, _algorithm: DiffAlgorithmName) -> Promise<Option<DocumentDiff>> {
        panic!("computeDiff is not a function")
    }

    /// `computeMoreMinimalEdits(resource, edits, pretty?)`: o serviço real pede ao worker (que tem o modelo sincronizado) o
    /// diff de cada edição contra o texto atual, e ainda não tem porte. O `mock<IEditorWorkerService>()` do original deixa o
    /// membro ausente (chamá-lo lança `TypeError`), e é isso que o corpo padrão faz: quem usa o serviço nas funções de
    /// formatação sobrescreve este método.
    fn compute_more_minimal_edits(&self, _resource: &URI, _edits: Option<Vec<TextEdit>>) -> Promise<Option<Vec<TextEdit>>> {
        panic!("computeMoreMinimalEdits is not a function")
    }

    /// `canNavigateValueSet(resource)`. O `mock<IEditorWorkerService>()` do original deixa o membro ausente (chamá-lo lança
    /// `TypeError`), e é isso que o corpo padrão faz: quem usa o serviço (o controlador de `inPlaceReplace`) o sobrescreve.
    fn can_navigate_value_set(&self, _resource: &URI) -> bool {
        panic!("canNavigateValueSet is not a function")
    }

    /// `navigateValueSet(resource, range, up)`: o worker (`EditorWorker::navigate_value_set`) tem o modelo sincronizado e
    /// devolve o próximo valor do conjunto. O corpo padrão lança como o `mock<IEditorWorkerService>()`.
    fn navigate_value_set(&self, _resource: &URI, _range: Range, _up: bool) -> Promise<Option<InplaceReplaceSupportResult>> {
        panic!("navigateValueSet is not a function")
    }

    /// `computeStringEditFromDiff(original, modified, { maxComputationTimeMs }, algorithm)`. O worker roda no mesmo
    /// thread, então o padrão calcula a diferença direto (o `$computeStringDiff` do worker); o `catch` do original,
    /// que cai na substituição do texto todo, só existe para a falha do IPC com o worker.
    fn compute_string_edit_from_diff(&self, original: &JsString, modified: &JsString, max_computation_time_ms: f64, algorithm: DiffAlgorithmName) -> Promise<StringEdit> {
        Promise::resolved(crate::editor_web_worker::compute_string_diff(original, modified, max_computation_time_ms, algorithm))
    }
}

service_id!(pub I_EDITOR_WORKER_SERVICE: dyn IEditorWorkerService = "editorWorkerService");
