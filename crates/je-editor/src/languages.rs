//! Porte da parte de tokenização de `src/vs/editor/common/languages.ts`: `ILanguageIdCodec`, `IState`,
//! `Token`, `TokenizationResult`, `EncodedTokenizationResult`, `ITokenizationSupport`,
//! `IBackgroundTokenizer`, `IBackgroundTokenizationStore` e `ILazyTokenizationSupport`. No fim estão os tipos de comentários
//! (`CommentThread`, `Comment`, `CommentingRanges`, `PendingCommentThread`, `CommentThreadChangedEvent` e afins).
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

    /// O suporte como `Any`, para o `instanceof` do original (o `MonarchTokenizer` consulta o suporte aninhado).
    /// `None` quando o suporte não oferece a conversão.
    fn as_any(&self) -> Option<&dyn Any> {
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
// - `CompletionItemProvider` traz `_debugDisplayName`, `triggerCharacters`, `provideCompletionItems` e
//   `resolveCompletionItem`. `provideCompletionItems` devolve a `ProviderResult<Rc<CompletionList>>` (a lista é
//   compartilhada com os itens de sugestão, que guardam o contêiner) e tem um corpo padrão que não sugere nada. O
//   `ProviderResult<CompletionItem>` do `resolveCompletionItem` é uma `Promise<Option<_>>`
//   (`null` e `undefined` viram `None`). O `range` de um `CompletionItem` é obrigatório (o tipo do original também
//   o exige), então o preenchimento do `range` ausente que `provideSuggestionItems` faz para os provedores do host
//   de extensões não tem o que preencher.
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

/// `CompletionTriggerKind`: como a lista de sugestões foi pedida.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionTriggerKind {
    Invoke = 0,
    TriggerCharacter = 1,
    TriggerForIncompleteCompletions = 2,
}

/// `CompletionContext`: o contexto de um pedido de sugestões ao provedor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionContext {
    /// Como o pedido foi disparado.
    pub trigger_kind: CompletionTriggerKind,
    /// O caractere que disparou o pedido, quando `trigger_kind` é [`CompletionTriggerKind::TriggerCharacter`].
    pub trigger_character: Option<String>,
}

impl Default for CompletionContext {
    /// O `{ triggerKind: CompletionTriggerKind.Invoke }` que `provideSuggestionItems` usa sem contexto.
    fn default() -> Self {
        Self { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None }
    }
}

/// `CompletionList`: o resultado de um provedor. O `dispose?()` opcional é o `dispose` com `Option`.
#[derive(Clone)]
pub struct CompletionList {
    pub suggestions: Vec<CompletionItem>,
    pub incomplete: Option<bool>,
    pub dispose: Option<std::rc::Rc<dyn Fn()>>,
    /// `duration?`: o tempo, em milissegundos, que o provedor levou para montar a lista.
    pub duration: Option<f64>,
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
    /// An optional array of additional text edits that are applied when selecting this completion. Edits must not
    /// overlap with the main edit nor with themselves.
    pub additional_text_edits: Option<Vec<crate::edit_operation::SingleEditOperation>>,
    /// A command that should be run upon acceptance of this item.
    pub command: Option<crate::code_action::languages::Command>,
    /// A command that should be run upon acceptance of this item.
    pub action: Option<crate::code_action::languages::Command>,
    /// `extensionId` (internal): a extensão que contribuiu o item.
    pub extension_id: Option<je_extensions::manifest::ExtensionIdentifier>,
    /// `_id` (internal): o identificador do item no cache do host de extensões, que o `resolveCompletionItem` devolve ao host.
    pub internal_id: Option<(u32, u32)>,
}

/// `CompletionItemProvider` (ver as diferenças acima).
pub trait CompletionItemProvider {
    /// `_debugDisplayName`.
    fn debug_display_name(&self) -> &str;

    /// `triggerCharacters`.
    fn trigger_characters(&self) -> Option<&[String]> {
        None
    }

    /// `provideCompletionItems(model, position, context, token)`: a lista de sugestões na posição, ou `None` (o
    /// `undefined`/`null`). O padrão é "nenhuma sugestão", como um provedor que não implementa o método.
    fn provide_completion_items(
        &self,
        _model: &std::rc::Rc<crate::text_model::TextModel>,
        _position: crate::position::Position,
        _context: &CompletionContext,
        _token: &je_base::cancellation::CancellationToken,
    ) -> crate::language_providers::ProviderResult<std::rc::Rc<CompletionList>> {
        je_base::promise::Promise::resolved(None)
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
    /// O `cacheId` do `ILinkDto` (`CachedSessionItem`) que o host de extensões põe no link: `[cacheId, itemIdx]`.
    pub cache_id: Option<(u32, u32)>,
}

// `ExternalUriOpenerPriority` de `src/vs/editor/common/languages.ts` (linhas 2603 a 2611 do original).

/// `ExternalUriOpenerPriority`: o quanto um opener externo de URI quer tratar um endereço. É `@internal` no original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalUriOpenerPriority {
    None = 0,
    Option = 1,
    Default = 2,
    Preferred = 3,
}

impl ExternalUriOpenerPriority {
    /// O valor numérico do membro (`ExternalUriOpenerPriority.X`).
    pub fn value(self) -> i32 {
        self as i32
    }

    /// O membro de um valor numérico, se existir.
    pub fn from_value(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Option),
            2 => Some(Self::Default),
            3 => Some(Self::Preferred),
            _ => None,
        }
    }
}

// Completações em linha de `src/vs/editor/common/languages.ts` (linhas 661 a 1142 do original).
//
// Decisões de tradução:
// - `InlineCompletionsProvider<T>` não é genérico: o provedor devolve [`InlineCompletions`] e cada item é um
//   `Rc<InlineCompletion>`, cuja identidade (`Rc::ptr_eq`) faz o papel da identidade de objeto do original (o
//   `supersededBy` e o `handleEndOfLifetime` recebem o mesmo item que o provedor devolveu).
// - `IdentifiableInlineCompletions` e `IdentifiableInlineCompletion` (`pid` e `idx` do protocolo) são os campos
//   `pid` de [`InlineCompletions`] e `idx` de [`InlineCompletion`].
// - `string | { snippet: string } | undefined` é [`InlineCompletionInsertText`]; `string | IMarkdownString` é
//   [`StringOrMarkdown`]; `ISingleEditOperation` é a `SingleEditOperation`.
// - `ProviderResult<T>` é `Promise<Option<T>>`; um membro opcional vira método com implementação padrão ("ausente"),
//   com um `has_*` quando a presença muda o comportamento do consumidor.

/// `PartialAcceptTriggerKind`: como uma aceitação parcial foi disparada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartialAcceptTriggerKind {
    Word = 0,
    Line = 1,
    Suggest = 2,
}

/// `PartialAcceptInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartialAcceptInfo {
    pub kind: PartialAcceptTriggerKind,
    pub accepted_length: i32,
}

/// `InlineCompletionTriggerKind`: como o provedor foi disparado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineCompletionTriggerKind {
    /// Disparo automático durante a edição; basta devolver um item.
    Automatic = 0,
    /// Disparo explícito por um gesto do usuário; devolver vários itens permite alternar entre eles.
    Explicit = 1,
}

/// `IInlineCompletionChangeHint`: dado arbitrário que o provedor passa ao disparar `onDidChangeInlineCompletions`.
#[derive(Clone, Debug, Default)]
pub struct IInlineCompletionChangeHint {
    /// `data?: unknown` (serializável em JSON).
    pub data: Option<je_base::dynamic::Dynamic>,
}

/// `SelectedSuggestionInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedSuggestionInfo {
    pub range: crate::range::Range,
    pub text: String,
    pub completion_kind: CompletionItemKind,
    pub is_snippet_text: bool,
}

impl SelectedSuggestionInfo {
    pub fn new(range: crate::range::Range, text: String, completion_kind: CompletionItemKind, is_snippet_text: bool) -> Self {
        Self { range, text, completion_kind, is_snippet_text }
    }

    /// `equals(other)`.
    pub fn equals(&self, other: &SelectedSuggestionInfo) -> bool {
        self == other
    }
}

/// `InlineCompletionContext`.
#[derive(Clone, Debug)]
pub struct InlineCompletionContext {
    pub trigger_kind: InlineCompletionTriggerKind,
    pub selected_suggestion_info: Option<SelectedSuggestionInfo>,
    /// `userPrompt?` (experimental, interno).
    pub user_prompt: Option<String>,
    /// `requestUuid` (experimental, interno).
    pub request_uuid: String,
    pub include_inline_edits: bool,
    pub include_inline_completions: bool,
    pub request_issued_date_time: f64,
    pub earliest_shown_date_time: f64,
    /// O `changeHint` passado ao `onDidChangeInlineCompletions`; só existe quando o pedido veio desse evento.
    pub change_hint: Option<IInlineCompletionChangeHint>,
}

/// `IInlineCompletionModel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IInlineCompletionModel {
    pub name: String,
    pub id: String,
}

/// `IInlineCompletionModelInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IInlineCompletionModelInfo {
    pub models: Vec<IInlineCompletionModel>,
    pub current_model_id: String,
}

/// `IInlineCompletionProviderOptionValue`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IInlineCompletionProviderOptionValue {
    pub id: String,
    pub label: String,
}

/// `IInlineCompletionProviderOption`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IInlineCompletionProviderOption {
    pub id: String,
    pub label: String,
    pub values: Vec<IInlineCompletionProviderOptionValue>,
    pub current_value_id: String,
}

/// `string | { snippet: string } | undefined`: o `insertText` de um [`InlineCompletion`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineCompletionInsertText {
    Text(String),
    Snippet(String),
}

/// `InlineCompletionWarning`.
#[derive(Clone, Debug)]
pub struct InlineCompletionWarning {
    pub message: StringOrMarkdown,
    /// `icon?: IconPath` (`ThemeIcon`).
    pub icon: Option<je_base::themables::ThemeIcon>,
}

/// `InlineCompletionHintStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineCompletionHintStyle {
    Code = 1,
    Label = 2,
}

/// `IInlineCompletionHint`.
#[derive(Clone, Debug)]
pub struct IInlineCompletionHint {
    /// Refere-se ao documento atual.
    pub range: crate::range::Range,
    pub style: InlineCompletionHintStyle,
    pub content: String,
}

/// `InlineCompletion`.
#[derive(Clone, Debug, Default)]
pub struct InlineCompletion {
    /// O texto a inserir. Se tiver quebra de linha, o `range` termina no fim de uma linha; se trocar texto existente, o
    /// texto existente é prefixo do inserido. Um snippet mostra a prévia com os parâmetros padrão e é inserido inteiro.
    pub insert_text: Option<InlineCompletionInsertText>,
    /// O trecho a trocar, na mesma linha, no documento atual ou no `uri`.
    pub range: Option<crate::range::Range>,
    /// Edições adicionais aplicadas ao aceitar; sem sobreposição com a principal nem entre si.
    pub additional_text_edits: Option<Vec<crate::edit_operation::SingleEditOperation>>,
    /// O arquivo ao qual a edição se aplica.
    pub uri: Option<je_base::uri::UriComponents>,
    /// Comando executado ao aceitar.
    pub command: Option<crate::code_action::languages::Command>,
    pub gutter_menu_link_action: Option<crate::code_action::languages::Command>,
    /// Chamado na primeira vez que o item aparece (obsoleto: usar o `onDidShow` do provedor).
    pub shown_command: Option<crate::code_action::languages::Command>,
    /// Com `true`, fechamentos sem abertura são removidos e aberturas sem fechamento são fechadas.
    pub complete_bracket_pairs: Option<bool>,
    pub is_inline_edit: Option<bool>,
    pub show_inline_edit_menu: Option<bool>,
    /// Só mostra a sugestão quando o cursor está no `show_range`.
    pub show_range: Option<crate::range::Range>,
    pub warning: Option<InlineCompletionWarning>,
    pub hint: Option<IInlineCompletionHint>,
    pub supports_rename: Option<bool>,
    /// Para telemetria.
    pub correlation_id: Option<String>,
    pub jump_to_position: Option<crate::position::Position>,
    pub do_not_log: Option<bool>,
    /// `idx` do `IdentifiableInlineCompletion`: o índice do item na lista do host de extensões.
    pub idx: Option<u32>,
}

/// `InlineCompletionCommand`: `{ command: Command; icon?: ThemeIcon }`.
#[derive(Clone, Debug)]
pub struct InlineCompletionCommand {
    pub command: crate::code_action::languages::Command,
    pub icon: Option<je_base::themables::ThemeIcon>,
}

/// `InlineCompletions`.
#[derive(Clone, Default)]
pub struct InlineCompletions {
    pub items: Vec<std::rc::Rc<InlineCompletion>>,
    /// Comandos associados às completações em linha desta lista.
    pub commands: Option<Vec<InlineCompletionCommand>>,
    pub suppress_suggestions: Option<bool>,
    /// Com `true`, digitar a sugestão sem divergir dela não a atualiza.
    pub enable_forward_stability: Option<bool>,
    /// `pid` do `IdentifiableInlineCompletions`: o identificador da lista no host de extensões.
    pub pid: Option<u32>,
}

/// `InlineCompletionProviderGroupId`.
pub type InlineCompletionProviderGroupId = String;

/// `InlineCompletionsDisposeReason['kind']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineCompletionsDisposeReasonKind {
    LostRace,
    TokenCancellation,
    Other,
    Empty,
    NotTaken,
}

/// `InlineCompletionsDisposeReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineCompletionsDisposeReason {
    pub kind: InlineCompletionsDisposeReasonKind,
}

/// `InlineCompletionEndOfLifeReasonKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineCompletionEndOfLifeReasonKind {
    Accepted = 0,
    Rejected = 1,
    Ignored = 2,
}

/// `InlineCompletionEndOfLifeReason<TInlineCompletion>`.
#[derive(Clone, Debug)]
pub enum InlineCompletionEndOfLifeReason<T = std::rc::Rc<InlineCompletion>> {
    /// O usuário aceitou com uma ação explícita; `alternative_action` diz se foi uma ação alternativa.
    Accepted { alternative_action: bool },
    /// O usuário rejeitou com uma ação explícita.
    Rejected,
    Ignored { superseded_by: Option<T>, user_typing_disagreed: bool },
}

impl<T> InlineCompletionEndOfLifeReason<T> {
    /// O `kind` da variante.
    pub fn kind(&self) -> InlineCompletionEndOfLifeReasonKind {
        match self {
            Self::Accepted { .. } => InlineCompletionEndOfLifeReasonKind::Accepted,
            Self::Rejected => InlineCompletionEndOfLifeReasonKind::Rejected,
            Self::Ignored { .. } => InlineCompletionEndOfLifeReasonKind::Ignored,
        }
    }

    /// `mapReason` do `mainThreadLanguageFeatures`: troca o `supersededBy` por `map`.
    pub fn map<U>(&self, map: impl FnOnce(&T) -> U) -> InlineCompletionEndOfLifeReason<U> {
        match self {
            Self::Accepted { alternative_action } => InlineCompletionEndOfLifeReason::Accepted { alternative_action: *alternative_action },
            Self::Rejected => InlineCompletionEndOfLifeReason::Rejected,
            Self::Ignored { superseded_by, user_typing_disagreed } => InlineCompletionEndOfLifeReason::Ignored {
                superseded_by: superseded_by.as_ref().map(map),
                user_typing_disagreed: *user_typing_disagreed,
            },
        }
    }
}

/// `LifetimeSummary`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LifetimeSummary {
    pub request_uuid: String,
    pub correlation_id: Option<String>,
    pub partially_accepted: i32,
    pub partially_accepted_count_since_original: i32,
    pub partially_accepted_ratio_since_original: f64,
    pub partially_accepted_characters_since_original: i32,
    pub shown: bool,
    pub shown_duration: f64,
    pub shown_duration_uncollapsed: f64,
    pub time_until_shown: Option<f64>,
    pub time_until_actually_shown: Option<f64>,
    pub time_until_provider_request: f64,
    pub time_until_provider_response: f64,
    pub not_shown_reason: Option<String>,
    pub editor_type: String,
    pub view_kind: Option<String>,
    pub preceeded: bool,
    pub language_id: String,
    pub request_reason: String,
    pub performance_markers: Option<String>,
    pub cursor_column_distance: Option<i32>,
    pub cursor_line_distance: Option<i32>,
    pub line_count_original: Option<i32>,
    pub line_count_modified: Option<i32>,
    pub character_count_original: Option<i32>,
    pub character_count_modified: Option<i32>,
    pub disjoint_replacements: Option<i32>,
    pub same_shape_replacements: Option<bool>,
    pub typing_interval: f64,
    pub typing_interval_character_count: i32,
    pub selected_suggestion_info: bool,
    pub available_providers: String,
    pub sku_plan: Option<String>,
    pub sku_type: Option<String>,
    pub rename_created: Option<bool>,
    pub rename_duration: Option<f64>,
    pub rename_timed_out: Option<bool>,
    pub rename_dropped_other_edits: Option<i32>,
    pub rename_dropped_rename_edits: Option<i32>,
    pub edit_kind: Option<String>,
    pub long_distance_hint_visible: Option<bool>,
    pub long_distance_hint_distance: Option<i32>,
    pub is_for_another_document: Option<bool>,
}

/// `InlineCompletionsProvider`.
pub trait InlineCompletionsProvider {
    /// `provideInlineCompletions(model, position, context, token)`.
    fn provide_inline_completions(
        &self,
        model: &std::rc::Rc<crate::text_model::TextModel>,
        position: crate::position::Position,
        context: &InlineCompletionContext,
        token: &je_base::cancellation::CancellationToken,
    ) -> crate::language_providers::ProviderResult<std::rc::Rc<InlineCompletions>>;

    /// `handleItemDidShow?`: chamado quando um item aparece. `updated_insert_text` ajuda a entender o fechamento de
    /// colchetes.
    fn handle_item_did_show(
        &self,
        _completions: &std::rc::Rc<InlineCompletions>,
        _item: &std::rc::Rc<InlineCompletion>,
        _updated_insert_text: &str,
        _edit_delta_info: crate::text_model_edit_source::EditDeltaInfo,
    ) {
    }

    /// `handlePartialAccept?`: chamado quando um item é aceito em parte.
    fn handle_partial_accept(
        &self,
        _completions: &std::rc::Rc<InlineCompletions>,
        _item: &std::rc::Rc<InlineCompletion>,
        _accepted_characters: i32,
        _info: PartialAcceptInfo,
    ) {
    }

    /// Se o provedor implementa o `handleRejection?` (obsoleto: usar `handleEndOfLifetime`).
    fn has_handle_rejection(&self) -> bool {
        false
    }

    /// `handleRejection?`.
    fn handle_rejection(&self, _completions: &std::rc::Rc<InlineCompletions>, _item: &std::rc::Rc<InlineCompletion>) {}

    /// Se o provedor implementa o `handleEndOfLifetime?`.
    fn has_handle_end_of_lifetime(&self) -> bool {
        false
    }

    /// `handleEndOfLifetime?`: o item deixou de ser usado, com o motivo.
    fn handle_end_of_lifetime(
        &self,
        _completions: &std::rc::Rc<InlineCompletions>,
        _item: &std::rc::Rc<InlineCompletion>,
        _reason: &InlineCompletionEndOfLifeReason,
        _lifetime_summary: &LifetimeSummary,
    ) {
    }

    /// `disposeInlineCompletions`: a lista não está mais em uso.
    fn dispose_inline_completions(&self, completions: &std::rc::Rc<InlineCompletions>, reason: InlineCompletionsDisposeReason);

    /// `onDidChangeInlineCompletions?`: o provedor quer disparar um novo pedido; o `changeHint` vai no contexto dele.
    fn on_did_change_inline_completions(&self) -> Option<je_base::event::Event<Option<IInlineCompletionChangeHint>>> {
        None
    }

    /// `groupId?`: usado só por `yieldsToGroupIds`.
    fn group_id(&self) -> Option<&str> {
        None
    }

    /// `providerId?` (interno).
    fn provider_id(&self) -> Option<&crate::code_action::languages::ProviderId> {
        None
    }

    /// `yieldsToGroupIds?`: o provedor só é consultado se nenhum de grupo preferido devolveu resultado.
    fn yields_to_group_ids(&self) -> Option<&[InlineCompletionProviderGroupId]> {
        None
    }

    /// `excludesGroupIds?`.
    fn excludes_group_ids(&self) -> Option<&[InlineCompletionProviderGroupId]> {
        None
    }

    /// `displayName?`.
    fn display_name(&self) -> Option<&str> {
        None
    }

    /// `debounceDelayMs?`.
    fn debounce_delay_ms(&self) -> Option<f64> {
        None
    }

    /// `modelInfo?`.
    fn model_info(&self) -> Option<IInlineCompletionModelInfo> {
        None
    }

    /// `onDidModelInfoChange?`.
    fn on_did_model_info_change(&self) -> Option<je_base::event::Event<()>> {
        None
    }

    /// `setModelId?`.
    fn set_model_id(&self, _model_id: &str) -> je_base::promise::Promise<()> {
        je_base::promise::Promise::resolved(())
    }

    /// `providerOptions?`.
    fn provider_options(&self) -> Option<Vec<IInlineCompletionProviderOption>> {
        None
    }

    /// `onDidProviderOptionsChange?`.
    fn on_did_provider_options_change(&self) -> Option<je_base::event::Event<()>> {
        None
    }

    /// `setProviderOption?`.
    fn set_provider_option(&self, _option_id: &str, _value_id: &str) -> je_base::promise::Promise<()> {
        je_base::promise::Promise::resolved(())
    }

    /// `toString?()`.
    fn to_display_string(&self) -> Option<String> {
        None
    }
}

// ---- Comentários (`CommentThread` e os tipos que o `contrib/comments` troca com os controladores) ------------------------
//
// Divergências:
// - `CommentThread<T>` é um trait: o original é uma interface implementada pelo `MainThreadCommentThread`. As propriedades
//   viram um método de leitura e, quando a propriedade é atribuída em algum lugar do original, um `set_*` (a implementação
//   guarda o estado atrás de `&self`). `isDocumentCommentThread(): this is CommentThread<IRange>` é `is_document_comment_thread`,
//   e `T` é o tipo do intervalo (`IRange` do editor, que aqui é o [`Range`], ou o `ICellRange` do notebook, que mora no workbench).
// - `ReadonlyArray<Comment> | undefined` é `Option<Vec<Comment>>` (cópia), e os `Event` das propriedades levam o valor novo.
// - `IMarkdownString | string` do `Comment.body` é o enum [`CommentBody`]; `boolean | CommentAuthorInformation` do
//   `canReply` é o enum [`CanReply`]; `AddedCommentThread` (que estende a thread com o `editorId`) é uma struct que guarda a
//   thread e se comporta como ela por `Deref`.
// - `IPosition` é o [`Position`] e `UriComponents` é o de `je-base`.

use je_base::event::Event;
use je_base::html_content::IMarkdownString;
use je_base::uri::{URI, UriComponents};

use crate::code_action::languages::Command;
use crate::position::Position;
use crate::range::Range;

/// `CommentThreadTemplate`.
#[derive(Clone, Debug)]
pub struct CommentThreadTemplate {
    pub controller_handle: i64,
    pub label: String,
    pub accept_input_command: Option<Command>,
    pub additional_commands: Option<Vec<Command>>,
    pub delete_command: Option<Command>,
}

/// `CommentInfo<T>`: o que um controlador devolve para um documento (as threads, as que continuam de uma sessão anterior e os
/// intervalos comentáveis).
#[derive(Clone)]
pub struct CommentInfo<T: Clone + 'static = Range> {
    pub extension_id: Option<String>,
    pub threads: Vec<Rc<dyn CommentThread<T>>>,
    pub pending_comment_threads: Option<Vec<PendingCommentThread>>,
    pub commenting_ranges: CommentingRanges,
}

/// `CommentingRangeResourceHint`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommentingRangeResourceHint {
    pub schemes: Vec<String>,
}

/// `CommentThreadCollapsibleState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommentThreadCollapsibleState {
    /// Determines an item is collapsed
    Collapsed = 0,
    /// Determines an item is expanded
    Expanded = 1,
}

/// `CommentThreadState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommentThreadState {
    Unresolved = 0,
    Resolved = 1,
}

/// `CommentThreadApplicability`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommentThreadApplicability {
    Current = 0,
    Outdated = 1,
}

/// `CommentWidget`: o widget do comentário que o controlador do editor enxerga (a entrada de texto atual dele).
pub trait CommentWidget {
    fn comment_thread(&self) -> Rc<dyn CommentThread<Range>>;
    fn comment(&self) -> Option<Comment>;
    fn input(&self) -> String;
    fn set_input(&self, input: &str);
    fn on_did_change_input(&self) -> Event<String>;
}

/// `CommentInput`.
#[derive(Clone, Debug)]
pub struct CommentInput {
    pub value: String,
    pub uri: URI,
}

/// `CommentThreadRevealOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentThreadRevealOptions {
    pub preserve_focus: bool,
    pub focus_reply: bool,
}

/// `boolean | CommentAuthorInformation`.
#[derive(Clone, Debug)]
pub enum CanReply {
    Bool(bool),
    Author(CommentAuthorInformation),
}

/// `CommentThread<T>`.
pub trait CommentThread<T: Clone + 'static> {
    /// `isDocumentCommentThread()`.
    fn is_document_comment_thread(&self) -> bool;
    fn comment_thread_handle(&self) -> i64;
    fn controller_handle(&self) -> i64;
    fn extension_id(&self) -> Option<String>;
    fn thread_id(&self) -> String;
    /// `resource: string | null`.
    fn resource(&self) -> Option<String>;
    fn range(&self) -> Option<T>;
    fn set_range(&self, range: Option<T>);
    fn label(&self) -> Option<String>;
    fn set_label(&self, label: Option<String>);
    fn context_value(&self) -> Option<String>;
    fn set_context_value(&self, context_value: Option<String>);
    fn comments(&self) -> Option<Vec<Comment>>;
    fn set_comments(&self, comments: Option<Vec<Comment>>);
    fn on_did_change_comments(&self) -> Event<Option<Vec<Comment>>>;
    fn collapsible_state(&self) -> Option<CommentThreadCollapsibleState>;
    fn set_collapsible_state(&self, state: Option<CommentThreadCollapsibleState>);
    fn initial_collapsible_state(&self) -> Option<CommentThreadCollapsibleState>;
    fn set_initial_collapsible_state(&self, state: Option<CommentThreadCollapsibleState>);
    fn on_did_change_initial_collapsible_state(&self) -> Event<Option<CommentThreadCollapsibleState>>;
    fn state(&self) -> Option<CommentThreadState>;
    fn set_state(&self, state: Option<CommentThreadState>);
    fn applicability(&self) -> Option<CommentThreadApplicability>;
    fn set_applicability(&self, applicability: Option<CommentThreadApplicability>);
    fn can_reply(&self) -> CanReply;
    fn set_can_reply(&self, can_reply: CanReply);
    fn input(&self) -> Option<CommentInput>;
    fn set_input(&self, input: Option<CommentInput>);
    fn on_did_change_input(&self) -> Event<Option<CommentInput>>;
    fn on_did_change_label(&self) -> Event<Option<String>>;
    fn on_did_change_collapsible_state(&self) -> Event<Option<CommentThreadCollapsibleState>>;
    fn on_did_change_state(&self) -> Event<Option<CommentThreadState>>;
    fn on_did_change_can_reply(&self) -> Event<bool>;
    fn is_disposed(&self) -> bool;
    fn set_is_disposed(&self, is_disposed: bool);
    fn is_template(&self) -> bool;
    fn set_is_template(&self, is_template: bool);
}

/// `AddedCommentThread<T>`: a thread com o `editorId` do editor que a criou.
#[derive(Clone)]
pub struct AddedCommentThread<T: Clone + 'static = Range> {
    pub thread: Rc<dyn CommentThread<T>>,
    pub editor_id: Option<String>,
}

impl<T: Clone + 'static> std::ops::Deref for AddedCommentThread<T> {
    type Target = dyn CommentThread<T>;

    fn deref(&self) -> &Self::Target {
        &*self.thread
    }
}

/// `CommentingRanges`.
#[derive(Clone, Debug)]
pub struct CommentingRanges {
    pub resource: URI,
    pub ranges: Vec<Range>,
    pub file_comments: bool,
}

/// `CommentAuthorInformation`.
#[derive(Clone, Debug)]
pub struct CommentAuthorInformation {
    pub name: String,
    pub icon_path: Option<UriComponents>,
}

/// `CommentReaction`.
#[derive(Clone, Debug, Default)]
pub struct CommentReaction {
    pub label: Option<String>,
    pub icon_path: Option<UriComponents>,
    pub count: Option<i64>,
    pub has_reacted: Option<bool>,
    pub can_edit: Option<bool>,
    pub reactors: Option<Vec<String>>,
}

/// `CommentOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommentOptions {
    /// An optional string to show on the comment input box when it's collapsed.
    pub prompt: Option<String>,
    /// An optional string to show as placeholder in the comment input box when it's focused.
    pub place_holder: Option<String>,
}

/// `CommentMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommentMode {
    Editing = 0,
    Preview = 1,
}

/// `CommentState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommentState {
    Published = 0,
    Draft = 1,
}

/// `string | IMarkdownString` do `Comment.body`.
#[derive(Clone, Debug)]
pub enum CommentBody {
    Text(String),
    Markdown(IMarkdownString),
}

/// `Comment`.
#[derive(Clone, Debug)]
pub struct Comment {
    pub unique_id_in_thread: i64,
    pub body: CommentBody,
    pub user_name: String,
    pub user_icon_path: Option<UriComponents>,
    pub context_value: Option<String>,
    pub comment_reactions: Option<Vec<CommentReaction>>,
    pub label: Option<String>,
    pub mode: Option<CommentMode>,
    pub state: Option<CommentState>,
    pub timestamp: Option<String>,
}

/// `PendingCommentThread`.
#[derive(Clone, Debug)]
pub struct PendingCommentThread {
    pub range: Option<Range>,
    pub uri: URI,
    pub unique_owner: String,
    pub is_reply: bool,
    pub comment: PendingComment,
}

/// `PendingComment`.
#[derive(Clone, Debug)]
pub struct PendingComment {
    pub body: String,
    pub cursor: Position,
}

/// `CommentThreadChangedEvent<T>`.
#[derive(Clone)]
pub struct CommentThreadChangedEvent<T: Clone + 'static = Range> {
    /// Pending comment threads.
    pub pending: Vec<PendingCommentThread>,
    /// Added comment threads.
    pub added: Vec<AddedCommentThread<T>>,
    /// Removed comment threads.
    pub removed: Vec<Rc<dyn CommentThread<T>>>,
    /// Changed comment threads.
    pub changed: Vec<Rc<dyn CommentThread<T>>>,
}
