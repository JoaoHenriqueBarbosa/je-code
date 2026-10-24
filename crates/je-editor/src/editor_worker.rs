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

use crate::range::Range;

/// `{ [word: string]: IRange[] }`.
pub type WordRanges = HashMap<JsString, Vec<Range>>;

/// `IEditorWorkerService` (ver o topo do módulo).
pub trait IEditorWorkerService {
    /// `canComputeWordRanges(resource)`.
    fn can_compute_word_ranges(&self, resource: &URI) -> bool;

    /// `computeWordRanges(resource, range)`: as palavras de `range` e onde elas ocorrem, ou `None` quando o modelo
    /// não está no worker.
    fn compute_word_ranges(&self, resource: &URI, range: Range) -> Promise<Option<WordRanges>>;
}
