//! Porte parcial de `src/vs/editor/common/languages.ts`: os tipos de provedor de recurso de linguagem que as
//! contribuições `smartSelect`, `documentSymbols` e `wordHighlighter` consomem (`SelectionRange` e
//! `SelectionRangeProvider` aqui; os demais entram junto da contribuição que os usa).
//!
//! Representação:
//! - `ProviderResult<T>` (`T | undefined | null | Thenable<T | undefined | null>`) é [`ProviderResult`], uma
//!   `Promise<Option<T>>`: o valor síncrono do original é uma promessa já resolvida (o `Promise.resolve(valor)` que
//!   todo consumidor aplica antes do `then`), e `undefined` e `null` viram `None`.
//! - `ITextModel` é o `Rc<TextModel>`: o provedor pode guardá-lo para trabalho que continua depois do retorno (o
//!   `BracketSelectionRangeProvider` cede a vez com `setTimeout`).
//! - `IRange` (a forma que um provedor devolve) é o [`Range`]: o `Range.isIRange` e o `Range.lift` que os
//!   consumidores aplicam à saída do provedor são a identidade num tipo que sempre é um intervalo válido.
//!
//! O arquivo `languages.rs` guarda a parte de tokenização do mesmo `languages.ts`; as duas metades não se
//! misturam para cada contribuição editar só o seu pedaço.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::map::ResourceMap;
use je_base::promise::Promise;

use crate::language_selector::LanguageSelector;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

/// `ProviderResult<T>` (ver a nota do módulo).
pub type ProviderResult<T> = Promise<Option<T>>;

/// `SelectionRange`: um intervalo que o usuário pode alcançar ao expandir a seleção.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionRange {
    pub range: Range,
}

/// `SelectionRangeProvider`.
pub trait SelectionRangeProvider {
    /// `provideSelectionRanges(model, positions, token)`: para cada posição, a lista de intervalos que a contêm, do
    /// menor para o maior.
    fn provide_selection_ranges(
        &self,
        model: &Rc<TextModel>,
        positions: &[Position],
        token: &CancellationToken,
    ) -> ProviderResult<Vec<Vec<SelectionRange>>>;
}

/// `SymbolKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    File = 0,
    Module = 1,
    Namespace = 2,
    Package = 3,
    Class = 4,
    Method = 5,
    Property = 6,
    Field = 7,
    Constructor = 8,
    Enum = 9,
    Interface = 10,
    Function = 11,
    Variable = 12,
    Constant = 13,
    String = 14,
    Number = 15,
    Boolean = 16,
    Array = 17,
    Object = 18,
    Key = 19,
    Null = 20,
    EnumMember = 21,
    Struct = 22,
    Event = 23,
    Operator = 24,
    TypeParameter = 25,
}

/// `SymbolTag`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolTag {
    Deprecated = 1,
}

/// `DocumentSymbol`: `containerName?` e `children?` ausentes são `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentSymbol {
    pub name: String,
    pub detail: String,
    pub kind: SymbolKind,
    pub tags: Vec<SymbolTag>,
    pub container_name: Option<String>,
    pub range: Range,
    pub selection_range: Range,
    pub children: Option<Vec<DocumentSymbol>>,
}

/// `DocumentSymbolProvider`.
pub trait DocumentSymbolProvider {
    /// `displayName?`.
    fn display_name(&self) -> Option<&str> {
        None
    }

    /// `provideDocumentSymbols(model, token)`.
    fn provide_document_symbols(&self, model: &Rc<TextModel>, token: &CancellationToken) -> ProviderResult<Vec<DocumentSymbol>>;
}

/// `DocumentHighlightKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocumentHighlightKind {
    /// A textual occurrence.
    Text,
    /// Read-access of a symbol, like reading a variable.
    Read,
    /// Write-access of a symbol, like writing to a variable.
    Write,
}

/// `DocumentHighlight`: `kind?` ausente (`undefined`, que vale `Text`) é `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentHighlight {
    pub range: Range,
    pub kind: Option<DocumentHighlightKind>,
}

/// `Map<URI, DocumentHighlight[]>`/`ResourceMap<DocumentHighlight[]>`: os destaques por recurso. O mapa é compartilhado
/// por referência depois de pronto (nenhum consumidor o altera), o que um `Rc` guarda e a promessa pode copiar.
pub type HighlightMap = Rc<ResourceMap<Vec<DocumentHighlight>>>;

/// `DocumentHighlightProvider`.
pub trait DocumentHighlightProvider {
    /// `provideDocumentHighlights(model, position, token)`: um valor lançado de forma síncrona pelo provedor do original é
    /// uma promessa rejeitada.
    fn provide_document_highlights(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<DocumentHighlight>>;

    /// O `provider instanceof TextualDocumentHighlightProvider` de `getOccurrencesAtPosition`: só o provedor textual
    /// embutido responde `true`.
    fn is_textual_provider(&self) -> bool {
        false
    }
}

/// `MultiDocumentHighlightProvider`.
pub trait MultiDocumentHighlightProvider {
    /// `selector`.
    fn selector(&self) -> &LanguageSelector;

    /// `provideMultiDocumentHighlights(primaryModel, position, otherModels, token)`.
    fn provide_multi_document_highlights(
        &self,
        primary_model: &Rc<TextModel>,
        position: Position,
        other_models: &[Rc<TextModel>],
        token: &CancellationToken,
    ) -> ProviderResult<HighlightMap>;
}
