//! Porte da parte de tokenização de `src/vs/editor/common/languages.ts`: `ILanguageIdCodec`, `IState`,
//! `Token`, `TokenizationResult`, `EncodedTokenizationResult`, `ITokenizationSupport`,
//! `IBackgroundTokenizer`, `IBackgroundTokenizationStore` e `ILazyTokenizationSupport`.
//!
//! Decisões de tradução:
//! - `IState` vira o trait [`IState`] com `as_any` (o `equals` de cada implementação recorre ao
//!   downcast, como o `instanceof` do original); o estado compartilhado é `Rc<dyn IState>` ([`State`]).
//! - Os tokens codificados são `Vec<u32>` (o `Uint32Array`); o texto da linha é [`JsString`].
//! - `FontTokensUpdate` (`AnnotationsUpdate`, de `annotations.ts`) não foi portado: [`FontTokensUpdate`]
//!   é um marcador sem conteúdo, e nenhum tokenizador do porte produz informação de fonte.

use std::any::Any;
use std::rc::Rc;

use je_base::JsString;
use je_base::lifecycle::IDisposable;

use crate::contiguous_multiline_tokens::ContiguousMultilineTokens;
use crate::text_model::TextModel;

/// `ILanguageIdCodec`.
pub trait ILanguageIdCodec {
    fn encode_language_id(&self, language_id: &str) -> u32;
    fn decode_language_id(&self, language_id: u32) -> String;
}

/// `IState`.
pub trait IState: Any {
    fn clone_state(&self) -> State;
    fn equals(&self, other: &dyn IState) -> bool;
    fn as_any(&self) -> &dyn Any;
}

/// O estado do tokenizador no fim de uma linha.
pub type State = Rc<dyn IState>;

/// `Token`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub offset: usize,
    pub type_: String,
    pub language: String,
}

impl Token {
    pub fn new(offset: usize, type_: &str, language: &str) -> Self {
        Self { offset, type_: type_.to_owned(), language: language.to_owned() }
    }
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.offset, self.type_)
    }
}

/// `TokenizationResult`.
#[derive(Clone)]
pub struct TokenizationResult {
    pub tokens: Vec<Token>,
    pub end_state: State,
}

/// `IFontToken`.
#[derive(Clone, Debug, PartialEq)]
pub struct FontToken {
    pub start_index: usize,
    pub end_index: usize,
    pub font_family: Option<String>,
    pub font_size_multiplier: Option<f64>,
    pub line_height_multiplier: Option<f64>,
}

/// `EncodedTokenizationResult`.
#[derive(Clone)]
pub struct EncodedTokenizationResult {
    pub tokens: Vec<u32>,
    pub font_info: Vec<FontToken>,
    pub end_state: State,
}

/// `FontTokensUpdate` (ver a nota do módulo).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontTokensUpdate;

/// `ITokenizationSupport`.
pub trait ITokenizationSupport {
    /// `backgroundTokenizerShouldOnlyVerifyTokens`.
    fn background_tokenizer_should_only_verify_tokens(&self) -> bool {
        false
    }

    fn get_initial_state(&self) -> State;

    fn tokenize(&self, line: &JsString, has_eol: bool, state: &State) -> TokenizationResult;

    fn tokenize_encoded(&self, line: &JsString, has_eol: bool, state: &State) -> EncodedTokenizationResult;

    /// `createBackgroundTokenizer?`: `None` quando o suporte não tem tokenizador em segundo plano próprio.
    fn create_background_tokenizer(
        &self,
        _text_model: &Rc<TextModel>,
        _store: Rc<dyn IBackgroundTokenizationStore>,
    ) -> Option<Rc<dyn IBackgroundTokenizer>> {
        None
    }
}

/// `IBackgroundTokenizer`.
pub trait IBackgroundTokenizer: IDisposable {
    fn request_tokens(&self, start_line_number: i32, end_line_number_exclusive: i32);

    /// `reportMismatchingTokens?`: devolve `false` quando o tokenizador não tem o método.
    fn report_mismatching_tokens(&self, _line_number: i32) -> bool {
        false
    }

    /// Se o tokenizador implementa `reportMismatchingTokens`.
    fn has_report_mismatching_tokens(&self) -> bool {
        false
    }
}

/// `IBackgroundTokenizationStore`.
pub trait IBackgroundTokenizationStore {
    fn set_tokens(&self, tokens: Vec<ContiguousMultilineTokens>);
    fn set_font_info(&self, changes: FontTokensUpdate);
    fn set_end_state(&self, line_number: i32, state: State);
    fn background_tokenization_finished(&self);
}

/// `ITokenizationSupportChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenizationSupportChangedEvent {
    pub changed_languages: Vec<String>,
    pub changed_color_map: bool,
}

// Completion: `CompletionItemKind`, `CompletionItemLabel`, `CompletionItemTag`,
// `CompletionItemInsertTextRule`, `CompletionItem` e `CompletionItemProvider` de
// `src/vs/editor/common/languages.ts` (linhas 353 a 736 do original).
//
// Diferenças do original:
// - `CompletionItem` tem o `range` ([`CompletionItemRange`]), mas não tem `additionalTextEdits`, `command`,
//   `action`, `extensionId` nem `_id` (dependem de `ISingleEditOperation`, `Command` e `ExtensionIdentifier`,
//   que nenhum consumidor atual lê); os demais campos estão todos.
// - `CompletionItemProvider` só traz `_debugDisplayName`, `triggerCharacters` e `resolveCompletionItem`;
//   `provideCompletionItems` precisa de `ITextModel` e de `ProviderResult`, e fica para quando o consumidor
//   chegar. O `ProviderResult<CompletionItem>` do `resolveCompletionItem` é uma `Promise<Option<_>>`
//   (`null` e `undefined` viram `None`).
// - `resolveCompletionItem?` opcional vira [`CompletionItemProvider::has_resolve_completion_item`].
// - `CompletionItemKinds` só traz o `fromString` ([`CompletionItemKind::from_string`]): o ícone e o rótulo
//   traduzido dependem de `Codicon` e de `localize`, e nenhum consumidor os lê.
// - `string | CompletionItemLabel` é [`CompletionLabel`] e `string | IMarkdownString` é [`StringOrMarkdown`].

/// `CompletionItemKind`. `Snippet` é o maior valor (usado em comparações).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompletionItemKind {
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Struct,
    Interface,
    Module,
    Property,
    Event,
    Operator,
    Unit,
    Value,
    Constant,
    Enum,
    EnumMember,
    Keyword,
    Text,
    Color,
    File,
    Reference,
    Customcolor,
    Folder,
    TypeParameter,
    User,
    Issue,
    Tool,
    Snippet,
}

impl CompletionItemKind {
    /// Os valores do enum na ordem numérica do original (o `CompletionItemKind` de `const enum`).
    const ALL: [CompletionItemKind; 29] = [
        CompletionItemKind::Method,
        CompletionItemKind::Function,
        CompletionItemKind::Constructor,
        CompletionItemKind::Field,
        CompletionItemKind::Variable,
        CompletionItemKind::Class,
        CompletionItemKind::Struct,
        CompletionItemKind::Interface,
        CompletionItemKind::Module,
        CompletionItemKind::Property,
        CompletionItemKind::Event,
        CompletionItemKind::Operator,
        CompletionItemKind::Unit,
        CompletionItemKind::Value,
        CompletionItemKind::Constant,
        CompletionItemKind::Enum,
        CompletionItemKind::EnumMember,
        CompletionItemKind::Keyword,
        CompletionItemKind::Text,
        CompletionItemKind::Color,
        CompletionItemKind::File,
        CompletionItemKind::Reference,
        CompletionItemKind::Customcolor,
        CompletionItemKind::Folder,
        CompletionItemKind::TypeParameter,
        CompletionItemKind::User,
        CompletionItemKind::Issue,
        CompletionItemKind::Tool,
        CompletionItemKind::Snippet,
    ];

    /// O valor numérico do original (`Method` é 0 e `Snippet` é 28).
    pub fn to_index(self) -> usize {
        self as usize
    }

    /// O inverso de [`Self::to_index`]: `None` fora de 0 a 28.
    pub fn from_index(index: usize) -> Option<CompletionItemKind> {
        Self::ALL.get(index).copied()
    }

    /// `CompletionItemKinds.fromString(value, strict)`: o nome (`'enum-member'`, `'enumMember'`, `'account'`...)
    /// para o tipo. Fora do modo estrito, um nome desconhecido vira `Property`.
    pub fn from_string(value: &str, strict: bool) -> Option<CompletionItemKind> {
        let found = match value {
            "method" => Some(CompletionItemKind::Method),
            "function" => Some(CompletionItemKind::Function),
            "constructor" => Some(CompletionItemKind::Constructor),
            "field" => Some(CompletionItemKind::Field),
            "variable" => Some(CompletionItemKind::Variable),
            "class" => Some(CompletionItemKind::Class),
            "struct" => Some(CompletionItemKind::Struct),
            "interface" => Some(CompletionItemKind::Interface),
            "module" => Some(CompletionItemKind::Module),
            "property" => Some(CompletionItemKind::Property),
            "event" => Some(CompletionItemKind::Event),
            "operator" => Some(CompletionItemKind::Operator),
            "unit" => Some(CompletionItemKind::Unit),
            "value" => Some(CompletionItemKind::Value),
            "constant" => Some(CompletionItemKind::Constant),
            "enum" => Some(CompletionItemKind::Enum),
            "enum-member" | "enumMember" => Some(CompletionItemKind::EnumMember),
            "keyword" => Some(CompletionItemKind::Keyword),
            "snippet" => Some(CompletionItemKind::Snippet),
            "text" => Some(CompletionItemKind::Text),
            "color" => Some(CompletionItemKind::Color),
            "file" => Some(CompletionItemKind::File),
            "reference" => Some(CompletionItemKind::Reference),
            "customcolor" => Some(CompletionItemKind::Customcolor),
            "folder" => Some(CompletionItemKind::Folder),
            "type-parameter" | "typeParameter" => Some(CompletionItemKind::TypeParameter),
            "account" => Some(CompletionItemKind::User),
            "issue" => Some(CompletionItemKind::Issue),
            "tool" => Some(CompletionItemKind::Tool),
            _ => None,
        };
        found.or(if strict { None } else { Some(CompletionItemKind::Property) })
    }
}

/// `CompletionItemLabel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItemLabel {
    pub label: String,
    pub detail: Option<String>,
    pub description: Option<String>,
}

/// `string | CompletionItemLabel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionLabel {
    Text(String),
    Item(CompletionItemLabel),
}

/// `CompletionItemTag`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionItemTag {
    Deprecated = 1,
}

/// `CompletionItemInsertTextRule`: máscara de bits para `CompletionItem::insert_text_rules`.
pub mod completion_item_insert_text_rule {
    pub const NONE: u32 = 0;
    /// Ajusta espaços e indentação de textos de várias linhas à indentação da linha atual.
    pub const KEEP_WHITESPACE: u32 = 0b001;
    /// `insertText` é um snippet.
    pub const INSERT_AS_SNIPPET: u32 = 0b100;
}

/// `string | IMarkdownString`.
#[derive(Clone, Debug)]
pub enum StringOrMarkdown {
    String(String),
    Markdown(je_base::html_content::IMarkdownString),
}

impl StringOrMarkdown {
    /// O valor é verdadeiro no sentido do JavaScript: texto não vazio, ou qualquer objeto markdown.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::String(text) => !text.is_empty(),
            Self::Markdown(_) => true,
        }
    }
}

/// `IRange | { insert: IRange; replace: IRange }`: o `range` de um [`CompletionItem`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionItemRange {
    /// Um intervalo só: o trecho trocado pelo texto inserido.
    Single(crate::range::Range),
    /// Intervalos separados para o modo de inserir e o de substituir.
    InsertReplace { insert: crate::range::Range, replace: crate::range::Range },
}

/// `CompletionList`: o resultado de um provedor. O `dispose?()` opcional é o `dispose` com `Option`.
#[derive(Clone)]
pub struct CompletionList {
    pub suggestions: Vec<CompletionItem>,
    pub incomplete: Option<bool>,
    pub dispose: Option<std::rc::Rc<dyn Fn()>>,
}

/// `CompletionItem`.
#[derive(Clone, Debug)]
pub struct CompletionItem {
    /// O rótulo; por padrão é também o texto inserido ao escolher o item.
    pub label: CompletionLabel,
    pub kind: CompletionItemKind,
    /// O trecho que o item troca ao ser inserido.
    pub range: CompletionItemRange,
    /// Modificadores do `kind` que afetam a exibição (por exemplo, `Deprecated` fica riscado).
    pub tags: Option<Vec<CompletionItemTag>>,
    pub detail: Option<String>,
    pub documentation: Option<StringOrMarkdown>,
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
    pub preselect: Option<bool>,
    pub insert_text: String,
    /// Máscara de [`completion_item_insert_text_rule`].
    pub insert_text_rules: Option<u32>,
    pub commit_characters: Option<Vec<String>>,
}

/// `CompletionItemProvider` (ver as diferenças acima).
pub trait CompletionItemProvider {
    /// `_debugDisplayName`.
    fn debug_display_name(&self) -> &str;

    /// `triggerCharacters`.
    fn trigger_characters(&self) -> Option<&[String]> {
        None
    }

    /// Se o provedor implementa o `resolveCompletionItem?` opcional.
    fn has_resolve_completion_item(&self) -> bool {
        false
    }

    /// `resolveCompletionItem`: completa o item (documentação, detalhes). Só é chamado quando
    /// [`Self::has_resolve_completion_item`] é verdadeiro.
    fn resolve_completion_item(
        &self,
        _item: &CompletionItem,
        _token: &je_base::cancellation::CancellationToken,
    ) -> je_base::promise::Promise<Option<CompletionItem>> {
        je_base::promise::Promise::resolved(None)
    }
}

// `ILink` de `src/vs/editor/common/languages.ts` (linhas 1882 a 1889 do original).

/// `URI | string`: o destino de um [`ILink`].
#[derive(Clone)]
pub enum LinkUrl {
    Uri(je_base::uri::URI),
    Text(JsString),
}

/// `ILink`: um link dentro do editor. `url` e `tooltip` são opcionais (`undefined` vira `None`).
#[derive(Clone)]
pub struct ILink {
    pub range: crate::range::Range,
    pub url: Option<LinkUrl>,
    pub tooltip: Option<String>,
}
