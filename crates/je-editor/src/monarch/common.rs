//! Porte de `src/vs/editor/standalone/common/monarch/monarchCommon.ts`: os tipos e as funções que o compilador
//! (`monarchCompile`) e o tokenizador (`monarchLexer`) dividem.
//!
//! Decisões de tradução:
//! - Os textos que dependem do conteúdo da linha (estados, tokens, trechos casados) são [`JsString`], como o
//!   `string` do JavaScript; as regexes são o YARR do `zyarr` (a mesma semântica das `RegExp` do JavaScript).
//! - Um `throw` é um [`MonarchError`] em `Result`; o tokenizador, cujo contrato não devolve `Result`, converte o
//!   erro em pânico (o `throw` que sobe até quem chamou `tokenize`).
//! - `ILexerMin` e `ILexer` são dois objetos que o original lê por `lexer[attr]`; aqui o que `@atributo` precisa
//!   é o trait [`ILexerMin`]: o `ignoreCase`, o `languageId` e o valor textual de um atributo.
//! - `IAction.transform` não existe: o `compile` nunca o preenche, e o tokenizador só o recusava.
//! - `IRule` é o trait [`IRule`] (a `Rule` do `monarchCompile` o implementa).

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use je_base::JsString;
use je_base::strings::escape_reg_exp_characters;
use zyarr::{Match, RegExp};

/// `MonarchBracket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonarchBracket {
    None = 0,
    Open = 1,
    Close = -1,
}

/// O `Error` que o `monarchCommon.createError` e as `RegExp` inválidas lançam.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonarchError(pub String);

impl fmt::Display for MonarchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MonarchError {}

/// O objeto `lexer` como o `substituteMatches` e o `createError` o enxergam.
pub trait ILexerMin {
    fn language_id(&self) -> &str;
    fn ignore_case(&self) -> bool;
    /// `typeof lexer[attr] === 'string'`: o texto do atributo, ou `None` se não for um texto.
    fn string_attr(&self, name: &str) -> Option<String>;
}

/// `ILexer`.
pub struct Lexer {
    pub language_id: String,
    pub include_lf: bool,
    pub no_throw: bool,
    pub max_stack: usize,
    pub start: Option<JsString>,
    pub ignore_case: bool,
    pub unicode: bool,
    pub token_postfix: String,
    pub default_token: String,
    pub uses_embedded: bool,
    pub tokenizer: HashMap<JsString, Rc<Vec<Rc<dyn IRule>>>>,
    pub brackets: Vec<Bracket>,
}

impl ILexerMin for Lexer {
    fn language_id(&self) -> &str {
        &self.language_id
    }

    fn ignore_case(&self) -> bool {
        self.ignore_case
    }

    fn string_attr(&self, name: &str) -> Option<String> {
        match name {
            "languageId" => Some(self.language_id.clone()),
            "start" => self.start.as_ref().map(JsString::to_string_lossy),
            "tokenPostfix" => Some(self.token_postfix.clone()),
            "defaultToken" => Some(self.default_token.clone()),
            _ => None,
        }
    }
}

/// `IBracket`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bracket {
    pub token: String,
    pub open: String,
    pub close: String,
}

/// `FuzzyAction`.
#[derive(Clone)]
pub enum FuzzyAction {
    Str(String),
    Action(Rc<IAction>),
}

/// O que um `cases` devolve para escolher a ação: `test` do `IAction`.
pub type ActionTest = Rc<dyn Fn(&JsString, &[Option<JsString>], &JsString, bool) -> Result<FuzzyAction, MonarchError>>;

/// `IAction`.
#[derive(Clone, Default)]
pub struct IAction {
    pub group: Option<Vec<FuzzyAction>>,
    pub has_embedded_end_in_cases: bool,
    pub test: Option<ActionTest>,
    pub token: Option<String>,
    pub token_subst: bool,
    pub next: Option<String>,
    pub next_embedded: Option<String>,
    pub bracket: Option<MonarchBracket>,
    pub log: Option<String>,
    pub switch_to: Option<String>,
    pub go_back: Option<i32>,
}

/// `IRule`.
pub trait IRule {
    fn action(&self) -> &FuzzyAction;
    fn match_only_at_line_start(&self) -> bool;
    fn name(&self) -> &str;
    fn resolve_regex(&self, state: &JsString) -> Result<Rc<MonarchRegExp>, MonarchError>;
}

/// O teste de um `case`: `(id, matches, state, eos) => boolean`.
pub type BranchTest = Rc<dyn Fn(&JsString, &[Option<JsString>], &JsString, bool) -> Result<bool, MonarchError>>;

/// `IBranch`.
pub struct Branch {
    pub name: String,
    pub value: FuzzyAction,
    pub test: Option<BranchTest>,
}

/// Uma `RegExp` do JavaScript com o `source` e as flags `i` e `u` que o tokenizador relê.
#[derive(Debug)]
pub struct MonarchRegExp {
    source: JsString,
    ignore_case: bool,
    unicode: bool,
    regexp: RegExp,
}

impl MonarchRegExp {
    /// `new RegExp(source, (ignoreCase ? 'i' : '') + (unicode ? 'u' : ''))`.
    pub fn new(source: &JsString, ignore_case: bool, unicode: bool) -> Result<Self, MonarchError> {
        let flags = format!("{}{}", if ignore_case { "i" } else { "" }, if unicode { "u" } else { "" });
        let flag_units: Vec<u16> = flags.encode_utf16().collect();
        match RegExp::new(source.units(), &flag_units) {
            Ok(regexp) => Ok(Self { source: source.clone(), ignore_case, unicode, regexp }),
            Err(error) => Err(MonarchError(format!("Invalid regular expression: /{source}/{flags}: {error}"))),
        }
    }

    pub fn source(&self) -> &JsString {
        &self.source
    }

    pub fn ignore_case(&self) -> bool {
        self.ignore_case
    }

    pub fn unicode(&self) -> bool {
        self.unicode
    }

    /// `text.match(regex)` de uma regex sem `g`: o primeiro casamento.
    pub fn exec(&self, text: &[u16]) -> Option<Match> {
        self.regexp.exec(text, 0)
    }

    /// `text.search(regex)`: o índice do primeiro casamento.
    pub fn search(&self, text: &[u16]) -> Option<usize> {
        self.exec(text).map(|found| found.start())
    }

    /// `regex.test(text)`.
    pub fn test(&self, text: &[u16]) -> bool {
        self.exec(text).is_some()
    }
}

// Small helper functions

/// `empty(s)`: nulo, `undefined` ou vazio.
pub fn empty(s: &JsString) -> bool {
    s.is_empty()
}

/// `fixCase(lexer, str)`: minúsculas quando `ignoreCase` está ligado.
pub fn fix_case(lexer: &dyn ILexerMin, s: &JsString) -> JsString {
    if lexer.ignore_case() && !s.is_empty() { s.to_lower_case() } else { s.clone() }
}

/// `sanitize(s)`: troca `& < > ' " _` por `-`.
pub fn sanitize(s: &JsString) -> JsString {
    let replaced: Vec<u16> = s
        .units()
        .iter()
        .map(|&unit| if matches!(unit, 0x26 | 0x3C | 0x3E | 0x27 | 0x22 | 0x5F) { u16::from(b'-') } else { unit })
        .collect();
    JsString::from_units(replaced)
}

/// `log(lexer, msg)`.
pub fn log(lexer: &dyn ILexerMin, msg: &str) {
    println!("{}: {}", lexer.language_id(), msg);
}

/// `createError(lexer, msg)`.
pub fn create_error(lexer: &dyn ILexerMin, msg: &str) -> MonarchError {
    MonarchError(format!("{}: {}", lexer.language_id(), msg))
}

/// `state.split('.')` com o próprio estado na frente (`unshift(state)`).
fn state_matches(state: &JsString) -> Vec<JsString> {
    let mut parts = vec![state.clone()];
    parts.extend(state.units().split(|&unit| unit == u16::from(b'.')).map(JsString::from));
    parts
}

fn is_word_unit(unit: u16) -> bool {
    matches!(unit, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A | 0x5F)
}

fn is_digit_unit(unit: u16) -> bool {
    matches!(unit, 0x30..=0x39)
}

/// Lê de um a dois dígitos (`\d\d?`) a partir de `at`: o número e o fim.
pub(crate) fn read_digits(units: &[u16], at: usize) -> Option<(usize, usize)> {
    let first = units.get(at).copied().filter(|&unit| is_digit_unit(unit))?;
    let mut value = usize::from(first - 0x30);
    let mut end = at + 1;
    if let Some(&second) = units.get(end)
        && is_digit_unit(second)
    {
        value = value * 10 + usize::from(second - 0x30);
        end += 1;
    }
    Some((value, end))
}

/// Lê um ou mais caracteres de palavra (`\w+`) a partir de `at`: o fim, se houver ao menos um.
pub(crate) fn read_word(units: &[u16], at: usize) -> Option<usize> {
    let mut end = at;
    while units.get(end).is_some_and(|&unit| is_word_unit(unit)) {
        end += 1;
    }
    (end > at).then_some(end)
}

const DOLLAR: u16 = b'$' as u16;

/// `substituteMatches(lexer, str, id, matches, state)`: troca `$$`, `$#`, `$n`, `$Sn` e `$@atributo`.
pub fn substitute_matches(lexer: &dyn ILexerMin, text: &JsString, id: &JsString, matches: &[Option<JsString>], state: &JsString) -> JsString {
    let units = text.units();
    let mut out = JsString::new();
    let mut state_parts: Option<Vec<JsString>> = None;
    let mut i = 0;
    while i < units.len() {
        if units[i] != DOLLAR {
            out.push_unit(units[i]);
            i += 1;
            continue;
        }
        let next = units.get(i + 1).copied();
        match next {
            // $$
            Some(unit) if unit == DOLLAR => {
                out.push_unit(DOLLAR);
                i += 2;
            }
            // $#
            Some(0x23) => {
                out.push_units(fix_case(lexer, id).units());
                i += 2;
            }
            // $n
            Some(unit) if is_digit_unit(unit) => {
                let (n, end) = read_digits(units, i + 1).expect("há um dígito");
                if n < matches.len() {
                    match &matches[n] {
                        Some(matched) => out.push_units(fix_case(lexer, matched).units()),
                        None => out.push_units(JsString::from("undefined").units()),
                    }
                }
                i = end;
            }
            // $Sn
            Some(unit) if (unit == u16::from(b's') || unit == u16::from(b'S')) && read_digits(units, i + 2).is_some() => {
                let (n, end) = read_digits(units, i + 2).expect("há um dígito");
                let parts = state_parts.get_or_insert_with(|| state_matches(state));
                if n < parts.len() {
                    out.push_units(fix_case(lexer, &parts[n]).units());
                }
                i = end;
            }
            // $@atributo
            Some(0x40) if read_word(units, i + 2).is_some() => {
                let end = read_word(units, i + 2).expect("há uma palavra");
                let attr = String::from_utf16_lossy(&units[i + 2..end]);
                if let Some(value) = lexer.string_attr(&attr) {
                    out.push_units(JsString::from(value).units());
                }
                i = end;
            }
            _ => {
                out.push_unit(DOLLAR);
                i += 1;
            }
        }
    }
    out
}

/// `substituteMatchesRe(lexer, str, state)`: troca `$Sn` pela parte `n` do estado, escapada para regex.
pub fn substitute_matches_re(lexer: &dyn ILexerMin, text: &JsString, state: &JsString) -> JsString {
    let units = text.units();
    let mut out = JsString::new();
    let mut state_parts: Option<Vec<JsString>> = None;
    let mut i = 0;
    while i < units.len() {
        let is_state_ref = units[i] == DOLLAR
            && units.get(i + 1).is_some_and(|&unit| unit == u16::from(b's') || unit == u16::from(b'S'))
            && read_digits(units, i + 2).is_some();
        if !is_state_ref {
            out.push_unit(units[i]);
            i += 1;
            continue;
        }
        let (n, end) = read_digits(units, i + 2).expect("há um dígito");
        let parts = state_parts.get_or_insert_with(|| state_matches(state));
        if n < parts.len() {
            out.push_units(escape_reg_exp_characters(fix_case(lexer, &parts[n]).units()).units());
        }
        i = end;
    }
    out
}

/// O estado pai de `state` (o texto até o último `.`), ou `None` sem pai.
fn parent_state(state: &JsString) -> Option<JsString> {
    let index = state.units().iter().rposition(|&unit| unit == u16::from(b'.'))?;
    Some(state.slice(0, index))
}

/// `findRules(lexer, inState)`: as regras do estado, ou as do estado pai mais próximo.
pub fn find_rules(lexer: &Lexer, in_state: &JsString) -> Option<Rc<Vec<Rc<dyn IRule>>>> {
    let mut state = Some(in_state.clone());
    while let Some(current) = state.filter(|s| !s.is_empty()) {
        if let Some(rules) = lexer.tokenizer.get(&current) {
            return Some(rules.clone());
        }
        state = parent_state(&current);
    }
    None
}

/// `stateExists(lexer, inState)`: o estado, ou algum pai, está entre os nomes declarados.
pub fn state_exists(state_names: &HashSet<JsString>, in_state: &JsString) -> bool {
    let mut state = Some(in_state.clone());
    while let Some(current) = state.filter(|s| !s.is_empty()) {
        if state_names.contains(&current) {
            return true;
        }
        state = parent_state(&current);
    }
    false
}
