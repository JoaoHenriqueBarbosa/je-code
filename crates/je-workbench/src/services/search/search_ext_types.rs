//! Porte de `src/vs/workbench/services/search/common/searchExtTypes.ts`: os tipos de busca da API de extensões.
//!
//! Portado o que o código do workbench constrói e lê: `Position`, `Range`, os dois enums
//! (`ExcludeSettingOptions`, `TextSearchCompleteMessageType`), as consultas e opções de busca de texto
//! (`TextSearchQuery2`, `TextSearchProviderFolderOptions`, `TextSearchProviderOptions`, `GlobPattern`,
//! `RelativePattern`, `TextSearchComplete2`, `TextSearchCompleteMessage2`) e os resultados (`TextSearchMatch2`,
//! `TextSearchContext2`, `TextSearchResult2`, `AISearchKeyword`, `AISearchResult`). Os métodos de `Position` e
//! `Range` que o original declara só como assinatura do tipo da API (devolvem constantes, a implementação real
//! vive no host de extensões) não existem aqui. As opções da busca de arquivo (`FileSearchProviderFolderOptions`
//! e `FileSearchProviderOptions`) também estão aqui, e no fim do arquivo as interfaces dos provedores
//! (`TextSearchProvider2`, `FileSearchProvider2`, `AITextSearchProvider`) com o `ProviderResult`.
//!
//! Linhas e colunas são `i64`, como em [`SearchRange`](super::search::SearchRange); o texto é [`JsString`]
//! (comprimentos contam unidades UTF-16, como no JS). Os campos opcionais (`?:`) viram `Option`, e `type` de
//! `TextSearchCompleteMessage2` é `message_type`.

use std::any::Any;
use std::rc::Rc;

use je_base::js_string::JsString;
use je_base::uri::URI;

use super::search::ITextSearchPreviewOptions;

/// `Position`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: i64,
    pub character: i64,
}

impl Position {
    pub fn new(line: i64, character: i64) -> Self {
        Self { line, character }
    }
}

/// `Range`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start_line: i64, start_col: i64, end_line: i64, end_col: i64) -> Self {
        Self { start: Position::new(start_line, start_col), end: Position::new(end_line, end_col) }
    }
}

/// `ExcludeSettingOptions`: opções para seguir os settings `search.exclude` e `files.exclude`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExcludeSettingOptions {
    /// Não usa nenhum setting de exclusão.
    None = 1,
    /// Usa o setting `files.exclude`.
    FilesExclude = 2,
    /// Usa os settings `files.exclude` e `search.exclude`.
    SearchAndFilesExclude = 3,
}

/// `TextSearchCompleteMessageType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSearchCompleteMessageType {
    Information = 1,
    Warning = 2,
}

/// `RelativePattern`: um glob casado relativamente a um caminho base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelativePattern {
    /// O caminho base absoluto, sem separador no fim nem segmentos relativos (`.` ou `..`).
    pub base_uri: URI,
    /// Um glob como `*.{ts,js}`, casado com o caminho relativo à base.
    pub pattern: String,
}

/// `GlobPattern = string | RelativePattern`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobPattern {
    Pattern(String),
    Relative(RelativePattern),
}

/// `TextSearchQuery2`: os parâmetros de uma consulta de busca de texto.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextSearchQuery2 {
    /// O texto a procurar.
    pub pattern: JsString,
    /// Se `pattern` casa várias linhas de texto.
    pub is_multiline: Option<bool>,
    /// Se `pattern` é uma expressão regular.
    pub is_reg_exp: Option<bool>,
    /// Se a busca diferencia maiúsculas de minúsculas.
    pub is_case_sensitive: Option<bool>,
    /// Se só casam palavras inteiras.
    pub is_word_match: Option<bool>,
}

impl TextSearchQuery2 {
    /// Um `{ pattern }` sem nenhuma opção.
    pub fn new(pattern: impl Into<JsString>) -> Self {
        Self { pattern: pattern.into(), ..Self::default() }
    }
}

/// `useIgnoreFiles` das opções de pasta: onde procurar arquivos de ignore (`.gitignore` ou `.ignore`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UseIgnoreFiles {
    /// Os arquivos de ignore da raiz do workspace.
    pub local: bool,
    /// Os do diretório pai (se ligado, `local` também deve estar).
    pub parent: bool,
    /// Os globais (se ligado, `local` também deve estar).
    pub global: bool,
}

/// `TextSearchProviderFolderOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSearchProviderFolderOptions {
    /// A pasta raiz onde buscar.
    pub folder: URI,
    /// Arquivos que casam um glob de `includes` entram na busca.
    pub includes: Vec<String>,
    /// Arquivos que casam um glob de `excludes` ficam de fora da busca.
    pub excludes: Vec<GlobPattern>,
    /// Se a caixa é ignorada nos globs.
    pub ignore_glob_case: Option<bool>,
    /// Se os links simbólicos são seguidos (setting `search.followSymlinks`).
    pub follow_symlinks: bool,
    pub use_ignore_files: UseIgnoreFiles,
    /// A codificação dos arquivos (setting `files.encoding`).
    pub encoding: String,
}

/// `FileSearchProviderFolderOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSearchProviderFolderOptions {
    /// A pasta raiz onde buscar.
    pub folder: URI,
    /// Arquivos que casam um glob de `includes` entram na busca.
    pub includes: Vec<String>,
    /// Arquivos que casam um glob de `excludes` ficam de fora da busca.
    pub excludes: Vec<GlobPattern>,
    /// Se os links simbólicos são seguidos (setting `search.followSymlinks`).
    pub follow_symlinks: bool,
    pub use_ignore_files: UseIgnoreFiles,
}

/// `FileSearchProviderOptions`: as opções da busca de arquivo.
#[derive(Clone)]
pub struct FileSearchProviderOptions {
    pub folder_options: Vec<FileSearchProviderFolderOptions>,
    /// Um objeto com o tempo de vida da sessão (`session: unknown`): o provedor pode usá-lo como chave de cache.
    pub session: Rc<dyn Any>,
    /// O número máximo de resultados.
    pub max_results: i64,
}

/// `TextSearchProviderOptions`: as opções da busca de texto. O `previewOptions` do original é um tipo
/// literal de mesma forma que [`ITextSearchPreviewOptions`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSearchProviderOptions {
    pub folder_options: Vec<TextSearchProviderFolderOptions>,
    /// O número máximo de resultados.
    pub max_results: i64,
    /// O tamanho da prévia do texto do resultado.
    pub preview_options: ITextSearchPreviewOptions,
    /// Ignora arquivos maiores que `maxFileSize`, em bytes (`number | undefined`, propriedade obrigatória).
    pub max_file_size: Option<i64>,
    /// Quantas linhas de contexto antes e depois de cada casamento.
    pub surrounding_context: i64,
}

/// `TextSearchCompleteMessage2`: uma mensagem sobre uma busca concluída.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSearchCompleteMessage2 {
    /// O texto da mensagem, em Markdown.
    pub text: String,
    /// Se a origem é confiável (links de comando ficam desligados para origens não confiáveis).
    pub trusted: Option<bool>,
    /// O tipo da mensagem, que afeta como ela é exibida.
    pub message_type: TextSearchCompleteMessageType,
}

/// `TextSearchComplete2`: o que se sabe quando a busca de texto termina.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextSearchComplete2 {
    /// Se a busca atingiu o limite de resultados.
    pub limit_hit: Option<bool>,
    /// Informações adicionais sobre o estado da busca concluída.
    pub message: Option<Vec<TextSearchCompleteMessage2>>,
}

/// Um par de intervalos de um casamento: o da fonte e o da prévia (`{ sourceRange, previewRange }`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSearchMatchRanges {
    pub source_range: Range,
    pub preview_range: Range,
}

/// `TextSearchMatch2`: a informação principal de um [`TextSearchResult2`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSearchMatch2 {
    /// O documento que casou.
    pub uri: URI,
    /// Os intervalos associados ao casamento.
    pub ranges: Vec<TextSearchMatchRanges>,
    /// O texto da prévia; o trecho destacado é dado por `ranges`.
    pub preview_text: JsString,
}

impl TextSearchMatch2 {
    pub fn new(uri: URI, ranges: Vec<TextSearchMatchRanges>, preview_text: impl Into<JsString>) -> Self {
        Self { uri, ranges, preview_text: preview_text.into() }
    }
}

/// `TextSearchContext2`: a informação de contexto de um [`TextSearchResult2`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSearchContext2 {
    /// O documento que casou.
    pub uri: URI,
    /// A linha de texto de contexto.
    pub text: JsString,
    /// O número da linha de contexto.
    pub line_number: i64,
}

impl TextSearchContext2 {
    pub fn new(uri: URI, text: impl Into<JsString>, line_number: i64) -> Self {
        Self { uri, text: text.into(), line_number }
    }
}

/// `AISearchKeyword`: sugestão de palavra-chave da busca com IA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AISearchKeyword {
    pub keyword: String,
}

/// `TextSearchResult2 = TextSearchMatch2 | TextSearchContext2`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextSearchResult2 {
    Match(TextSearchMatch2),
    Context(TextSearchContext2),
}

/// `AISearchResult = TextSearchResult2 | AISearchKeyword`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AISearchResult {
    Text(TextSearchResult2),
    Keyword(AISearchKeyword),
}

// ---------------------------------------------------------------------------------------------------------
// As interfaces dos provedores (`FileSearchProvider2`, `TextSearchProvider2`, `AITextSearchProvider`).
//
// Divergências:
// - `ProviderResult<T> = T | undefined | null | Thenable<T | undefined | null>` é sempre a promessa de um `Option`:
//   um resultado síncrono é `Promise::resolved`, e `undefined`/`null` são `None`.
// - `progress: IProgress<TextSearchResult2>` do `provideAITextSearchResults` é `IProgress<AISearchResult>`: o
//   `TextSearchManager` entrega ao provedor um progresso que também aceita o `AISearchKeyword`, e o tipo declarado
//   no original só o admite por bivariância dos métodos do TypeScript.
// - `readonly name?: string` do `AITextSearchProvider` é o método `name`.
// ---------------------------------------------------------------------------------------------------------

use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;
use je_platform::progress::progress::IProgress;

/// `ProviderResult<T>`.
pub type ProviderResult<T> = Promise<Option<T>>;

/// `FileSearchProvider2`: provides search results for files in the given folder that match a query string.
pub trait FileSearchProvider2 {
    /// Provide the set of files that match a certain file path pattern.
    fn provide_file_search_results(&self, pattern: &str, options: &FileSearchProviderOptions, token: &CancellationToken) -> ProviderResult<Vec<URI>>;

    /// `this.provider instanceof OldFileSearchProviderConverter`: o `FileSearchManager` entrega a um provedor antigo o token da
    /// sessão (no `session` das opções, como um [`CancellationToken`]) e a um provedor novo o objeto da sessão. Só o
    /// `OldFileSearchProviderConverter` (`searchExtConversionTypes`) devolve `true`.
    fn is_old_file_search_provider_converter(&self) -> bool {
        false
    }
}

/// `TextSearchProvider2`: provides search results for text results inside files in the workspace.
pub trait TextSearchProvider2 {
    /// Provide results that match the given text pattern.
    fn provide_text_search_results(
        &self,
        query: &TextSearchQuery2,
        options: &TextSearchProviderOptions,
        progress: Rc<dyn IProgress<TextSearchResult2>>,
        token: &CancellationToken,
    ) -> ProviderResult<TextSearchComplete2>;
}

/// `AITextSearchProvider`: provides additional AI text search results in the workspace.
pub trait AITextSearchProvider {
    /// The name of the AI searcher. Will be displayed as `{name} Results` in the Search View.
    fn name(&self) -> Option<String> {
        None
    }

    /// WARNING: VERY EXPERIMENTAL. Provide results that match the given text pattern.
    fn provide_ai_text_search_results(
        &self,
        query: &JsString,
        options: &TextSearchProviderOptions,
        progress: Rc<dyn IProgress<AISearchResult>>,
        token: &CancellationToken,
    ) -> ProviderResult<TextSearchComplete2>;
}
