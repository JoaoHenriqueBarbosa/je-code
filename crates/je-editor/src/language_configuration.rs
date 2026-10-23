//! Porte de `src/vs/editor/common/languages/languageConfiguration.ts`: a configuração de linguagem
//! (`LanguageConfiguration` e as regras que a compõem), `IndentAction`, `EnterAction`,
//! `StandardAutoClosingPairConditional` e `AutoClosingPairs`.
//!
//! Decisões de tradução:
//! - Os `RegExp` do original são `Rc<JsRegExp>`: o objeto é compartilhado e o `lastIndex` é estado dele.
//! - Os textos de configuração (`open`, `close`, `comment`) são `String`; o que é comparado com o texto do
//!   modelo vira UTF-16 nos módulos `supports`.
//! - `ExplicitLanguageConfiguration` (o `OrUndefined<Required<...>>`) tem a mesma forma que
//!   [`LanguageConfiguration`] com todos os campos `Option`, então não ganhou tipo próprio.
//! - As chaves dos mapas de [`AutoClosingPairs`] são o `charAt` do original: uma unidade UTF-16 (vazio
//!   quando o texto é vazio), por isso [`JsString`].
//! - `StandardAutoClosingPairConditional` é compartilhado por `Rc` entre as listas e os mapas, como o
//!   objeto do original.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;

use crate::encoded_token_attributes::standard_token_type;
use je_base::js_regexp::JsRegExp;
use crate::supports::ScopedLineTokens;

/// `regex.test(text)`: com `g` ou `y` mexe no `lastIndex`, como no original.
// TODO(integração): virar método de `JsRegExp`.
pub fn reg_exp_test(regex: &JsRegExp, text: &[u16]) -> bool {
    regex.exec(text).is_some()
}

/// `LineCommentConfig`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineCommentConfig {
    /// O token de comentário de linha, como `//`.
    pub comment: String,
    /// Se o token não deve ser indentado e fica na primeira coluna.
    pub no_indent: Option<bool>,
}

/// `string | LineCommentConfig` de `CommentRule.lineComment`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineComment {
    Token(String),
    Config(LineCommentConfig),
}

/// `CharacterPair`: uma dupla de caracteres, como abre e fecha de um colchete.
pub type CharacterPair = (String, String);

/// `CommentRule`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommentRule {
    pub line_comment: Option<LineComment>,
    pub block_comment: Option<CharacterPair>,
}

/// `IndentationRule`.
#[derive(Clone, Debug)]
pub struct IndentationRule {
    /// Uma linha que casa faz as seguintes saírem uma indentação.
    pub decrease_indent_pattern: Rc<JsRegExp>,
    /// Uma linha que casa faz as seguintes entrarem uma indentação.
    pub increase_indent_pattern: Rc<JsRegExp>,
    /// Uma linha que casa indenta só a linha seguinte.
    pub indent_next_line_pattern: Option<Rc<JsRegExp>>,
    /// Uma linha que casa mantém a indentação e não é avaliada contra as outras regras.
    pub un_indented_line_pattern: Option<Rc<JsRegExp>>,
}

/// `FoldingMarkers`.
#[derive(Clone, Debug)]
pub struct FoldingMarkers {
    pub start: Rc<JsRegExp>,
    pub end: Rc<JsRegExp>,
}

/// `FoldingRules`.
#[derive(Clone, Debug, Default)]
pub struct FoldingRules {
    /// A linguagem segue a regra off-side (blocos pela indentação).
    pub off_side: Option<bool>,
    pub markers: Option<FoldingMarkers>,
}

/// `IndentAction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndentAction {
    /// Nova linha com a indentação da anterior.
    None = 0,
    /// Nova linha, indentada uma vez.
    Indent = 1,
    /// Duas linhas novas: a primeira indentada (com o cursor) e a segunda no mesmo nível.
    IndentOutdent = 2,
    /// Nova linha, desindentada uma vez.
    Outdent = 3,
}

/// `EnterAction`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnterAction {
    pub indent_action: IndentAction,
    /// Texto a acrescentar depois da nova linha e da indentação.
    pub append_text: Option<String>,
    /// Quantos caracteres tirar da indentação da nova linha.
    pub remove_text: Option<i32>,
}

impl EnterAction {
    pub fn new(indent_action: IndentAction) -> Self {
        Self {
            indent_action,
            append_text: None,
            remove_text: None,
        }
    }
}

/// `CompleteEnterAction`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompleteEnterAction {
    pub indent_action: IndentAction,
    pub append_text: String,
    pub remove_text: i32,
    /// A indentação da linha menos `remove_text`.
    pub indentation: String,
}

/// `OnEnterRule`.
#[derive(Clone, Debug)]
pub struct OnEnterRule {
    /// A regra só vale se o texto antes do cursor casa.
    pub before_text: Rc<JsRegExp>,
    /// A regra só vale se o texto depois do cursor casa.
    pub after_text: Option<Rc<JsRegExp>>,
    /// A regra só vale se o texto da linha de cima casa.
    pub previous_line_text: Option<Rc<JsRegExp>>,
    pub action: EnterAction,
}

/// `IDocComment`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocComment {
    /// O que abre um comentário de documentação (`/**`).
    pub open: String,
    /// O que aparece na última linha e o fecha (` */`).
    pub close: Option<String>,
}

/// O `__electricCharacterSupport` (obsoleto) de `LanguageConfiguration`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ElectricCharacterSupportConfig {
    pub doc_comment: Option<DocComment>,
}

/// `IAutoClosingPair`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoClosingPair {
    pub open: String,
    pub close: String,
}

impl AutoClosingPair {
    pub fn new(open: &str, close: &str) -> Self {
        Self {
            open: open.to_owned(),
            close: close.to_owned(),
        }
    }
}

/// `IAutoClosingPairConditional`.
#[derive(Clone, Debug)]
pub struct AutoClosingPairConditional {
    pub open: String,
    pub close: String,
    pub not_in: Option<Vec<String>>,
    pub before_text: Option<Rc<JsRegExp>>,
}

impl AutoClosingPairConditional {
    pub fn new(open: &str, close: &str) -> Self {
        Self {
            open: open.to_owned(),
            close: close.to_owned(),
            not_in: None,
            before_text: None,
        }
    }

    pub fn not_in(mut self, not_in: &[&str]) -> Self {
        self.not_in = Some(not_in.iter().map(|s| (*s).to_owned()).collect());
        self
    }
}

/// `LanguageConfiguration`: o contrato entre as extensões e os recursos do editor (inserção automática
/// de colchetes, indentação automática, etc.).
#[derive(Clone, Debug, Default)]
pub struct LanguageConfiguration {
    /// Os comentários da linguagem.
    pub comments: Option<CommentRule>,
    /// Os colchetes da linguagem (também afetam o Enter em volta deles).
    pub brackets: Option<Vec<CharacterPair>>,
    /// A definição de palavra da linguagem.
    pub word_pattern: Option<Rc<JsRegExp>>,
    pub indentation_rules: Option<IndentationRule>,
    /// As regras avaliadas ao pressionar Enter.
    pub on_enter_rules: Option<Vec<OnEnterRule>>,
    /// Os pares de fechamento automático; sem eles valem os colchetes.
    pub auto_closing_pairs: Option<Vec<AutoClosingPairConditional>>,
    /// Os pares que envolvem a seleção; sem eles valem os de fechamento automático.
    pub surrounding_pairs: Option<Vec<AutoClosingPair>>,
    /// Os pares coloridos pelo nível de aninhamento; sem eles valem os colchetes.
    pub colorized_bracket_pairs: Option<Vec<CharacterPair>>,
    /// Os caracteres que devem vir depois do cursor para o fechamento automático ocorrer.
    pub auto_close_before: Option<String>,
    pub folding: Option<FoldingRules>,
    /// Obsoleto.
    pub electric_character_support: Option<ElectricCharacterSupportConfig>,
}

/// `StandardAutoClosingPairConditional`.
#[derive(Debug)]
pub struct StandardAutoClosingPairConditional {
    pub open: String,
    pub close: String,
    pub before_text: Option<Rc<JsRegExp>>,
    in_string: bool,
    in_comment: bool,
    in_reg_ex: bool,
    neutral_character: RefCell<Option<String>>,
    neutral_character_searched: Cell<bool>,
}

/// O que o `deepStrictEqual` do original compara em dois `RegExp`: `source` e `flags`.
fn same_reg_exp(a: &Option<Rc<JsRegExp>>, b: &Option<Rc<JsRegExp>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.source() == b.source() && a.flags() == b.flags(),
        _ => false,
    }
}

impl PartialEq for StandardAutoClosingPairConditional {
    /// Compara o que define o par (texto, `beforeText` e onde vale); o cache do caractere neutro, que
    /// depende só de `open` e `close`, fica de fora.
    fn eq(&self, other: &Self) -> bool {
        self.open == other.open
            && self.close == other.close
            && same_reg_exp(&self.before_text, &other.before_text)
            && self.in_string == other.in_string
            && self.in_comment == other.in_comment
            && self.in_reg_ex == other.in_reg_ex
    }
}

impl StandardAutoClosingPairConditional {
    pub fn new(source: &AutoClosingPairConditional) -> Self {
        // Inicialmente vale em todos os tokens.
        let (mut in_string, mut in_comment, mut in_reg_ex) = (true, true, true);
        if let Some(not_in) = &source.not_in {
            for not_in in not_in {
                match not_in.as_str() {
                    "string" => in_string = false,
                    "comment" => in_comment = false,
                    "regex" => in_reg_ex = false,
                    _ => {}
                }
            }
        }
        Self {
            open: source.open.clone(),
            close: source.close.clone(),
            before_text: source.before_text.clone(),
            in_string,
            in_comment,
            in_reg_ex,
            neutral_character: RefCell::new(None),
            neutral_character_searched: Cell::new(false),
        }
    }

    /// Os dois textos do par, sem a parte condicional.
    pub fn to_pair(&self) -> AutoClosingPair {
        AutoClosingPair {
            open: self.open.clone(),
            close: self.close.clone(),
        }
    }

    pub fn should_auto_close_before(&self, text: &[u16]) -> bool {
        match &self.before_text {
            None => true,
            Some(before_text) => {
                before_text.set_last_index(0);
                reg_exp_test(before_text, text)
            }
        }
    }

    pub fn is_ok(&self, standard_token: u32) -> bool {
        match standard_token {
            standard_token_type::OTHER => true,
            standard_token_type::COMMENT => self.in_comment,
            standard_token_type::STRING => self.in_string,
            standard_token_type::REG_EX => self.in_reg_ex,
            _ => false,
        }
    }

    pub fn should_auto_close(&self, context: &ScopedLineTokens, column: i32) -> bool {
        // Sempre completa em linha vazia.
        if context.get_token_count() == 0 {
            return true;
        }

        let token_index = context.find_token_index_at_offset(column - 2);
        let standard_token_type = context.get_standard_token_type(token_index);
        self.is_ok(standard_token_type)
    }

    fn find_neutral_character_in_range(&self, from: char, to: char) -> Option<String> {
        (from..=to)
            .find(|character| !self.open.contains(*character) && !self.close.contains(*character))
            .map(String::from)
    }

    /// Um caractere de `[0-9a-zA-Z]` que não aparece em `open` nem em `close`.
    pub fn find_neutral_character(&self) -> Option<String> {
        if !self.neutral_character_searched.get() {
            self.neutral_character_searched.set(true);
            let mut found = self.neutral_character.borrow_mut();
            for (from, to) in [('0', '9'), ('a', 'z'), ('A', 'Z')] {
                if found.is_none() {
                    *found = self.find_neutral_character_in_range(from, to);
                }
            }
        }
        self.neutral_character.borrow().clone()
    }
}

/// Os mapas de [`AutoClosingPairs`]: o caractere (`charAt`) para os pares.
pub type AutoClosingPairsMap = HashMap<JsString, Vec<Rc<StandardAutoClosingPairConditional>>>;

/// `AutoClosingPairs`: os pares indexados por uma ou outra ponta de `open` e `close`.
pub struct AutoClosingPairs {
    /// A chave é o primeiro caractere de `open`.
    pub auto_closing_pairs_open_by_start: AutoClosingPairsMap,
    /// A chave é o último caractere de `open`.
    pub auto_closing_pairs_open_by_end: AutoClosingPairsMap,
    /// A chave é o primeiro caractere de `close`.
    pub auto_closing_pairs_close_by_start: AutoClosingPairsMap,
    /// A chave é o último caractere de `close`.
    pub auto_closing_pairs_close_by_end: AutoClosingPairsMap,
    /// A chave é `close`; só os pares de um caractere.
    pub auto_closing_pairs_close_single_char: AutoClosingPairsMap,
}

/// O primeiro e o último caractere (unidade UTF-16) de `text`; vazio se não houver.
fn first_and_last_unit(text: &[u16]) -> (JsString, JsString) {
    let unit = |u: Option<&u16>| u.map_or_else(JsString::new, |u| JsString::from_units(vec![*u]));
    (unit(text.first()), unit(text.last()))
}

fn append_entry(
    target: &mut AutoClosingPairsMap,
    key: JsString,
    value: &Rc<StandardAutoClosingPairConditional>,
) {
    target.entry(key).or_default().push(value.clone());
}

impl AutoClosingPairs {
    pub fn new(auto_closing_pairs: &[Rc<StandardAutoClosingPairConditional>]) -> Self {
        let mut this = Self {
            auto_closing_pairs_open_by_start: HashMap::new(),
            auto_closing_pairs_open_by_end: HashMap::new(),
            auto_closing_pairs_close_by_start: HashMap::new(),
            auto_closing_pairs_close_by_end: HashMap::new(),
            auto_closing_pairs_close_single_char: HashMap::new(),
        };
        for pair in auto_closing_pairs {
            let open: Vec<u16> = pair.open.encode_utf16().collect();
            let close: Vec<u16> = pair.close.encode_utf16().collect();
            let (open_first, open_last) = first_and_last_unit(&open);
            let (close_first, close_last) = first_and_last_unit(&close);
            append_entry(&mut this.auto_closing_pairs_open_by_start, open_first, pair);
            append_entry(&mut this.auto_closing_pairs_open_by_end, open_last, pair);
            append_entry(
                &mut this.auto_closing_pairs_close_by_start,
                close_first,
                pair,
            );
            append_entry(&mut this.auto_closing_pairs_close_by_end, close_last, pair);
            if close.len() == 1 && open.len() == 1 {
                append_entry(
                    &mut this.auto_closing_pairs_close_single_char,
                    JsString::from_units(close),
                    pair,
                );
            }
        }
        this
    }
}
