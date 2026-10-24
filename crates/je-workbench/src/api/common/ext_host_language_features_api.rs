//! A superfície da API `vscode` que `extHostLanguageFeatures.ts` consome: as interfaces de provedor
//! (`vscode.DocumentSymbolProvider`, `vscode.CodeLensProvider`, `vscode.DefinitionProvider`...) e os tipos soltos que
//! elas usam (`vscode.LocationLink`, `vscode.HoverContext`...).
//!
//! PARTE 1: só os provedores de símbolos do documento, lentes de código, definição, declaração, implementação, tipo da
//! definição, hover, destaque de ocorrências (um e vários documentos) e referências. Os demais entram na parte 2.
//!
//! Divergências:
//! - `ProviderResult<T>` (`T | undefined | null | Thenable<T | undefined | null>`) é [`ApiProviderResult`], um
//!   `MaybePromise<Option<T>>`: o valor síncrono é `MaybePromise::Value`, a promessa é `MaybePromise::Promise`, e `undefined`
//!   e `null` são `None`. Um `throw` síncrono do provedor é uma promessa rejeitada.
//! - Um provedor da API é um trait, e o método opcional do TypeScript (`resolveCodeLens?`, `onDidChangeCodeLenses?`) é um
//!   método com implementação padrão, mais um `has_*` onde o original testa `typeof provider.x === 'function'`.
//! - `vscode.DefinitionProvider`, `DeclarationProvider`, `ImplementationProvider` e `TypeDefinitionProvider` têm a mesma
//!   forma (`provideX(document, position, token)` devolvendo `Definition | DefinitionLink[]`) e são o único trait
//!   [`ApiLocationLinkProvider`]; o adaptador é que sabe de qual das quatro é cada registro.
//! - `vscode.Location | vscode.Location[] | vscode.LocationLink[]` é [`ApiLocationResult`]; o objeto que o original distingue
//!   por duck typing (`targetUri`) é o [`LocationOrLink`].
//! - `vscode.Hover` e `vscode.VerboseHover` são o [`VerboseHover`] (`Hover` mais os dois sinalizadores de verbosidade
//!   opcionais): o hover simples é um [`VerboseHover`] sem sinalizadores.
//! - O `vscode.CodeLens` é compartilhado por referência entre o provedor, o cache e o `resolveCodeLens` (que o altera no
//!   lugar): é o [`CodeLensRef`], `Rc<RefCell<CodeLens>>`.
//! - `vscode.TextDocument` é o [`TextDocument`] do host de documentos.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::promise::MaybePromise;
use je_base::uri::URI;

use super::ext_host_document_data::TextDocument;
use super::ext_host_protocol::ReferenceContext;
use super::ext_host_types::{CodeLens, DocumentHighlight, DocumentSymbol, Location, MultiDocumentHighlight, Position, Range, SymbolInformation, VerboseHover};

/// `ProviderResult<T>` da API `vscode` (ver a nota do módulo).
pub type ApiProviderResult<T> = MaybePromise<Option<T>>;

/// `vscode.DocumentSymbolProviderMetadata`.
#[derive(Clone, Debug, Default)]
pub struct DocumentSymbolProviderMetadata {
    pub label: Option<String>,
}

/// `DocumentSymbol[] | SymbolInformation[]`: o original distingue pelo `instanceof` do primeiro elemento.
#[derive(Clone, Debug)]
pub enum ApiDocumentSymbols {
    Symbols(Vec<DocumentSymbol>),
    Information(Vec<SymbolInformation>),
}

/// `vscode.DocumentSymbolProvider`.
pub trait ApiDocumentSymbolProvider {
    fn provide_document_symbols(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<ApiDocumentSymbols>;
}

/// O `vscode.CodeLens` compartilhado por referência.
pub type CodeLensRef = Rc<RefCell<CodeLens>>;

/// `vscode.CodeLensProvider`.
pub trait ApiCodeLensProvider {
    /// `onDidChangeCodeLenses`, quando o provedor tem o evento.
    fn on_did_change_code_lenses(&self) -> Option<Event<()>> {
        None
    }

    fn provide_code_lenses(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<Vec<CodeLensRef>>;

    /// `typeof provider.resolveCodeLens === 'function'`.
    fn has_resolve_code_lens(&self) -> bool {
        false
    }

    /// `resolveCodeLens(codeLens, token)`; só é chamado quando [`ApiCodeLensProvider::has_resolve_code_lens`] é verdadeiro.
    fn resolve_code_lens(&self, _code_lens: &CodeLensRef, _token: CancellationToken) -> ApiProviderResult<CodeLensRef> {
        MaybePromise::Value(None)
    }
}

/// `vscode.LocationLink`.
#[derive(Clone, Debug, PartialEq)]
pub struct DefinitionLink {
    pub origin_selection_range: Option<Range>,
    pub target_uri: URI,
    pub target_range: Range,
    pub target_selection_range: Option<Range>,
}

/// `vscode.Location | vscode.LocationLink`.
#[derive(Clone, Debug)]
pub enum LocationOrLink {
    Location(Location),
    Link(DefinitionLink),
}

/// `vscode.Location | vscode.Location[] | vscode.LocationLink[]`.
#[derive(Clone, Debug)]
pub enum ApiLocationResult {
    Single(LocationOrLink),
    Many(Vec<LocationOrLink>),
}

/// `vscode.DefinitionProvider`, `vscode.DeclarationProvider`, `vscode.ImplementationProvider` e
/// `vscode.TypeDefinitionProvider` (ver a nota do módulo).
pub trait ApiLocationLinkProvider {
    fn provide_location_links(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<ApiLocationResult>;
}

/// `vscode.HoverContext`.
#[derive(Clone, Debug)]
pub struct ApiHoverContext {
    pub verbosity_delta: f64,
    pub previous_hover: VerboseHover,
}

/// `vscode.HoverProvider`.
pub trait ApiHoverProvider {
    /// `provideHover(document, position, token, context?)`.
    fn provide_hover(
        &self,
        document: &TextDocument,
        position: &Position,
        token: CancellationToken,
        context: Option<&ApiHoverContext>,
    ) -> ApiProviderResult<VerboseHover>;
}

/// `vscode.DocumentHighlightProvider`.
pub trait ApiDocumentHighlightProvider {
    fn provide_document_highlights(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<Vec<DocumentHighlight>>;
}

/// `vscode.MultiDocumentHighlightProvider`.
pub trait ApiMultiDocumentHighlightProvider {
    fn provide_multi_document_highlights(
        &self,
        document: &TextDocument,
        position: &Position,
        other_documents: &[TextDocument],
        token: CancellationToken,
    ) -> ApiProviderResult<Vec<MultiDocumentHighlight>>;
}

/// `vscode.ReferenceProvider`.
pub trait ApiReferenceProvider {
    fn provide_references(
        &self,
        document: &TextDocument,
        position: &Position,
        context: &ReferenceContext,
        token: CancellationToken,
    ) -> ApiProviderResult<Vec<Location>>;
}
