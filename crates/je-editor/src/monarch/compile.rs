//! Porte de `src/vs/editor/standalone/common/monarch/monarchCompile.ts`: o `compile`, que transforma a descrição
//! de uma linguagem ([`MonarchLanguage`]) num [`Lexer`] tipado e conferido.
//!
//! Decisões de tradução:
//! - O `json` do original é mutado pelo `compile` (`lexerMin = <any>json`): os campos `languageId`, `includeLF`,
//!   `ignoreCase`, `unicode`, `noThrow`, `usesEmbedded`, `stateNames` e `defaultToken` passam a valer como atributos
//!   (`@languageId`, por exemplo). [`LexerMin`] guarda esses campos mais os atributos livres e responde a
//!   `lexer[attr]` com o mesmo resultado.
//! - As funções de apoio do original que usam `RegExp` só sobre o texto da definição (`/^\w*$/`, `/@(\w+)/g` e as
//!   demais) são varreduras de texto com a mesma semântica, sem motor de regex.
//! - Uma `include` que se refere a si mesma estoura a pilha no JavaScript (`RangeError`); aqui vira o erro
//!   `Maximum call stack size exceeded` com o mesmo limite de recursão da ordem de grandeza do motor.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::JsString;

use super::common::{
    Bracket, Branch, BranchTest, ActionTest, FuzzyAction, IAction, ILexerMin, IRule, Lexer, MonarchBracket, MonarchError, MonarchRegExp, create_error, fix_case, read_word,
    state_exists, substitute_matches, substitute_matches_re,
};
use super::types::{MonarchAction, MonarchAttr, MonarchBracketDef, MonarchLanguage, MonarchLanguageBracket, MonarchRule, MonarchTokenAction};

/// O limite de recursão de `include` antes do `RangeError`.
const MAX_INCLUDE_DEPTH: usize = 1000;

/// Os nomes que `lexer[attr]` encontra no protótipo de `Object` (funções, portanto não são texto).
const OBJECT_PROTOTYPE_NAMES: [&str; 12] = [
    "constructor",
    "toString",
    "toLocaleString",
    "valueOf",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "__proto__",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// `ILexerMin`: o objeto (a própria descrição da linguagem, mutada) que as funções de compilação consultam.
pub struct LexerMin {
    language_id: String,
    include_lf: bool,
    no_throw: bool,
    ignore_case: bool,
    unicode: bool,
    uses_embedded: Cell<bool>,
    default_token: String,
    state_names: HashSet<JsString>,
    start: Option<String>,
    token_postfix: Option<String>,
    attributes: HashMap<String, MonarchAttr>,
}

impl LexerMin {
    /// `lexer[attr]`: `None` é o `undefined`.
    pub fn attr(&self, name: &str) -> Option<MonarchAttr> {
        match name {
            "languageId" => Some(MonarchAttr::Str(self.language_id.clone())),
            "includeLF" => Some(MonarchAttr::Bool(self.include_lf)),
            "ignoreCase" => Some(MonarchAttr::Bool(self.ignore_case)),
            "unicode" => Some(MonarchAttr::Bool(self.unicode)),
            "noThrow" => Some(MonarchAttr::Bool(self.no_throw)),
            "usesEmbedded" => Some(MonarchAttr::Bool(self.uses_embedded.get())),
            "stateNames" => Some(MonarchAttr::Other),
            "defaultToken" => Some(MonarchAttr::Str(self.default_token.clone())),
            "start" => self.start.clone().map(MonarchAttr::Str),
            "tokenPostfix" => self.token_postfix.clone().map(MonarchAttr::Str),
            "brackets" | "tokenizer" => Some(MonarchAttr::Other),
            _ => self
                .attributes
                .get(name)
                .cloned()
                .or_else(|| OBJECT_PROTOTYPE_NAMES.contains(&name).then_some(MonarchAttr::Other)),
        }
    }
}

impl ILexerMin for LexerMin {
    fn language_id(&self) -> &str {
        &self.language_id
    }

    fn ignore_case(&self) -> bool {
        self.ignore_case
    }

    fn string_attr(&self, name: &str) -> Option<String> {
        match self.attr(name) {
            Some(MonarchAttr::Str(value)) => Some(value),
            _ => None,
        }
    }
}

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// O `.` de uma regex sem a flag `s` não casa terminadores de linha.
fn has_line_terminator(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
}

/*
 * Type helpers
 *
 * Note: this is just for sanity checks on the JSON description which is
 * helpful for the programmer. No checks are done anymore once the lexer is
 * already 'compiled and checked'.
 */

/// `createKeywordMatcher(arr, caseInsensitive)`.
fn create_keyword_matcher(arr: &[String], case_insensitive: bool) -> Rc<dyn Fn(&JsString) -> bool> {
    let normalize = move |word: &JsString| if case_insensitive { word.to_lower_case() } else { word.clone() };
    let hash: HashSet<JsString> = arr.iter().map(|word| normalize(&js(word))).collect();
    Rc::new(move |word: &JsString| {
        let word = normalize(word);
        // `hash[word] !== undefined && hash.hasOwnProperty(word)`: `__proto__` nunca é propriedade própria.
        word != *"__proto__" && hash.contains(&word)
    })
}

// Lexer helpers

/// A regex de uma regra: fixa, ou dependente do estado (`$Sn`).
pub enum RuleRegex {
    Static(Rc<MonarchRegExp>),
    Dynamic(DynamicRegExp),
}

/// `DynamicRegExp`: `(state) => RegExp`, com o cache do último estado.
pub struct DynamicRegExp {
    lexer: Rc<LexerMin>,
    text: JsString,
    /// `lastState` e `lastRegEx`: o estado é gravado antes de compilar, como no original.
    cache: RefCell<(Option<JsString>, Option<Rc<MonarchRegExp>>)>,
}

impl DynamicRegExp {
    fn resolve(&self, state: &JsString) -> Result<Rc<MonarchRegExp>, MonarchError> {
        {
            let cache = self.cache.borrow();
            if let (Some(regex), Some(last_state)) = (&cache.1, &cache.0)
                && last_state == state
            {
                return Ok(regex.clone());
            }
        }
        self.cache.borrow_mut().0 = Some(state.clone());
        let source = substitute_matches_re(&*self.lexer, &self.text, state);
        let regex = Rc::new(MonarchRegExp::new(&source, self.lexer.ignore_case, self.lexer.unicode)?);
        self.cache.borrow_mut().1 = Some(regex.clone());
        Ok(regex)
    }
}

/// Compila uma regex: troca `@atributo` pelo conteúdo do atributo (`@@` é um `@` literal) e acrescenta as flags
/// `i` e `u`. Com `handle_sn`, uma regex com `$Sn` vira dinâmica.
fn compile_reg_exp(lexer: &Rc<LexerMin>, text: &JsString, handle_sn: bool) -> Result<RuleRegex, MonarchError> {
    // @@ must be interpreted as a literal @, so we replace all occurences of @@ with a placeholder character
    let mut units: Vec<u16> = Vec::with_capacity(text.len());
    let source = text.units();
    let mut i = 0;
    while i < source.len() {
        if source[i] == u16::from(b'@') && source.get(i + 1) == Some(&u16::from(b'@')) {
            units.push(1);
            i += 2;
        } else {
            units.push(source[i]);
            i += 1;
        }
    }
    let mut current = JsString::from_units(units);

    let mut n = 0;
    loop {
        let before = current.clone();
        let units = before.units();
        let mut had_expansion = false;
        let mut out = JsString::new();
        let mut i = 0;
        while i < units.len() {
            if units[i] == u16::from(b'@')
                && let Some(end) = read_word(units, i + 1)
            {
                had_expansion = true;
                let attr = String::from_utf16_lossy(&units[i + 1..end]);
                let sub = match lexer.attr(&attr) {
                    Some(MonarchAttr::Str(value)) | Some(MonarchAttr::Re(value)) => value,
                    None => {
                        return Err(create_error(&**lexer, &format!("language definition does not contain attribute '{attr}', used at: {before}")));
                    }
                    Some(_) => {
                        return Err(create_error(&**lexer, &format!("attribute reference '{attr}' must be a string, used at: {before}")));
                    }
                };
                if !sub.is_empty() {
                    out.push_units(js(&format!("(?:{sub})")).units());
                }
                i = end;
            } else {
                out.push_unit(units[i]);
                i += 1;
            }
        }
        current = out;
        n += 1;
        if !(had_expansion && n < 5) {
            break;
        }
    }

    // handle escaped @@
    let restored: Vec<u16> = current.units().iter().map(|&unit| if unit == 1 { u16::from(b'@') } else { unit }).collect();
    let current = JsString::from_units(restored);

    // handle $Sn
    if handle_sn && has_state_reference(current.units()) {
        return Ok(RuleRegex::Dynamic(DynamicRegExp { lexer: lexer.clone(), text: current, cache: RefCell::new((None, None)) }));
    }

    Ok(RuleRegex::Static(Rc::new(MonarchRegExp::new(&current, lexer.ignore_case, lexer.unicode)?)))
}

/// `/\$[sS](\d\d?)/` casa em algum ponto do texto.
fn has_state_reference(units: &[u16]) -> bool {
    (0..units.len()).any(|i| {
        units[i] == u16::from(b'$')
            && units.get(i + 1).is_some_and(|&unit| unit == u16::from(b's') || unit == u16::from(b'S'))
            && units.get(i + 2).is_some_and(|&unit| (0x30..=0x39).contains(&unit))
    })
}

/// `compileRegExp(lexer, str, false)`: a regex fixa.
fn compile_static_reg_exp(lexer: &Rc<LexerMin>, text: &JsString) -> Result<Rc<MonarchRegExp>, MonarchError> {
    match compile_reg_exp(lexer, text, false)? {
        RuleRegex::Static(regex) => Ok(regex),
        RuleRegex::Dynamic(_) => unreachable!("sem handleSn a regex é sempre fixa"),
    }
}

/// Compiles guard functions for case matches.
/// This compiles 'cases' attributes into efficient match functions.
fn select_scrutinee(id: &JsString, matches: &[Option<JsString>], state: &JsString, num: i32) -> Option<JsString> {
    if num < 0 {
        return Some(id.clone());
    }
    let mut num = num as usize;
    if num < matches.len() {
        return matches[num].clone();
    }
    if num >= 100 {
        num -= 100;
        let mut parts = vec![state.clone()];
        parts.extend(state.units().split(|&unit| unit == u16::from(b'.')).map(JsString::from));
        if num < parts.len() {
            return Some(parts[num].clone());
        }
    }
    None
}

/// O `tester` de um guarda: `(s, id, matches, state, eos) => boolean`.
type Tester = Rc<dyn Fn(&JsString, &JsString, &[Option<JsString>], &JsString, bool) -> Result<bool, MonarchError>>;

/// Fixa a assinatura de um [`Tester`] (a inferência dos parâmetros por referência pede uma função genérica).
fn make_tester<F>(tester: F) -> Tester
where
    F: Fn(&JsString, &JsString, &[Option<JsString>], &JsString, bool) -> Result<bool, MonarchError> + 'static,
{
    Rc::new(tester)
}

/// Fixa a assinatura de um [`BranchTest`].
fn make_branch_test<F>(test: F) -> BranchTest
where
    F: Fn(&JsString, &[Option<JsString>], &JsString, bool) -> Result<bool, MonarchError> + 'static,
{
    Rc::new(test)
}

/// Fixa a assinatura de um [`ActionTest`].
fn make_action_test<F>(test: F) -> ActionTest
where
    F: Fn(&JsString, &[Option<JsString>], &JsString, bool) -> Result<FuzzyAction, MonarchError> + 'static,
{
    Rc::new(test)
}

/// A divisão de uma chave de `case` em escrutínio (`$n`, `$Sn`, `$#`) e o resto: `/^\$(([sS]?)(\d\d?)|#)(.*)$/`.
fn split_scrutinee(tkey: &str) -> Option<(i32, &str)> {
    let rest = tkey.strip_prefix('$')?;
    let bytes = rest.as_bytes();
    let has_s = matches!(bytes.first(), Some(b's' | b'S'));
    let digits_at = usize::from(has_s);
    let digits = bytes[digits_at.min(bytes.len())..].iter().take(2).take_while(|b| b.is_ascii_digit()).count();
    let (scrut, after) = if digits > 0 {
        let value: i32 = rest[digits_at..digits_at + digits].parse().ok()?;
        (if has_s { value + 100 } else { value }, digits_at + digits)
    } else if bytes.first() == Some(&b'#') {
        (-1, 1)
    } else {
        return None;
    };
    let oppat = &rest[after..];
    (!has_line_terminator(oppat)).then_some((scrut, oppat))
}

/// A divisão do `operador` e do padrão: `/^(@|!@|~|!~|==|!=)(.*)$/`.
fn split_operator(oppat: &str) -> Option<(&'static str, &str)> {
    let found = ["@", "!@", "~", "!~", "==", "!="].into_iter().find(|op| oppat.starts_with(op))?;
    let pat = &oppat[found.len()..];
    (!has_line_terminator(pat)).then_some((found, pat))
}

fn create_guard(lexer: &Rc<LexerMin>, rule_name: &str, tkey: &str, val: FuzzyAction) -> Result<Branch, MonarchError> {
    // get the scrutinee and pattern
    let mut scrut: i32 = -1; // -1: $!, 0-99: $n, 100+n: $Sn
    let mut oppat = tkey;
    if let Some((found_scrut, found_oppat)) = split_scrutinee(tkey) {
        scrut = found_scrut;
        oppat = found_oppat;
    }
    // get operator
    let mut op = "~";
    let mut pat = oppat;
    if oppat.is_empty() {
        op = "!=";
        pat = "";
    } else if oppat.chars().all(word_char) {
        // just a word
        op = "==";
    } else if let Some((found_op, found_pat)) = split_operator(oppat) {
        op = found_op;
        pat = found_pat;
    }

    // set the tester function
    let tester: Tester;

    // special case a regexp that matches just words
    if (op == "~" || op == "!~") && pat.chars().all(|c| word_char(c) || c == '|') {
        let words: Vec<String> = pat.split('|').map(str::to_owned).collect();
        let in_words = create_keyword_matcher(&words, lexer.ignore_case);
        let negate = op == "!~";
        tester = make_tester(move |s, _, _, _, _| Ok(in_words(s) != negate));
    } else if op == "@" || op == "!@" {
        let words = match lexer.attr(pat) {
            None | Some(MonarchAttr::Bool(false)) => None,
            Some(MonarchAttr::Str(text)) if text.is_empty() => None,
            Some(other) => Some(other),
        };
        let Some(words) = words else {
            return Err(create_error(&**lexer, &format!("the @ match target '{pat}' is not defined, in rule: {rule_name}")));
        };
        let MonarchAttr::Strs(words) = words else {
            return Err(create_error(&**lexer, &format!("the @ match target '{pat}' must be an array of strings, in rule: {rule_name}")));
        };
        let in_words = create_keyword_matcher(&words, lexer.ignore_case);
        let negate = op == "!@";
        tester = make_tester(move |s, _, _, _, _| Ok(in_words(s) != negate));
    } else if op == "~" || op == "!~" {
        if !pat.contains('$') {
            // precompile regular expression
            let re = compile_static_reg_exp(lexer, &js(&format!("^{pat}$")))?;
            let negate = op == "!~";
            tester = make_tester(move |s, _, _, _, _| Ok(re.test(s.units()) != negate));
        } else {
            // O original devolve `re.test(s)` sem aplicar o `!~` neste caminho dinâmico.
            let (lexer, pat) = (lexer.clone(), js(pat));
            tester = make_tester(move |s, id, matches, state, _| {
                let substituted = substitute_matches(&*lexer, &pat, id, matches, state);
                let mut source = js("^");
                source.push_units(substituted.units());
                source.push_unit(u16::from(b'$'));
                let re = compile_static_reg_exp(&lexer, &source)?;
                Ok(re.test(s.units()))
            });
        }
    } else {
        // if (op==='==' || op==='!=') {
        let patx = fix_case(&**lexer, &js(pat));
        let equals = op == "==";
        if !pat.contains('$') {
            tester = make_tester(move |s, _, _, _, _| Ok((*s == patx) == equals));
        } else {
            let lexer = lexer.clone();
            tester = make_tester(move |s, id, matches, state, _| {
                let patexp = substitute_matches(&*lexer, &patx, id, matches, state);
                Ok((*s == patexp) == equals)
            });
        }
    }

    // return the branch object
    let test: BranchTest = if scrut == -1 {
        make_branch_test(move |id, matches, state, eos| tester(id, id, matches, state, eos))
    } else {
        make_branch_test(move |id, matches, state, eos| {
            let scrutinee = select_scrutinee(id, matches, state, scrut);
            tester(&scrutinee.unwrap_or_default(), id, matches, state, eos)
        })
    };
    Ok(Branch { name: tkey.to_owned(), value: val, test: Some(test) })
}

/// Compiles an action: i.e. optimize regular expressions and case matches
/// and do many sanity checks.
fn compile_action(lexer: &Rc<LexerMin>, rule_name: &str, action: &MonarchAction) -> Result<FuzzyAction, MonarchError> {
    match action {
        MonarchAction::None => Ok(FuzzyAction::Action(Rc::new(IAction { token: Some(String::new()), ..IAction::default() }))),
        MonarchAction::Str(text) if text.is_empty() => Ok(FuzzyAction::Action(Rc::new(IAction { token: Some(String::new()), ..IAction::default() }))),
        MonarchAction::Str(text) => Ok(FuzzyAction::Str(text.clone())), // { token: action };
        MonarchAction::Token(token_action) => compile_token_action(lexer, rule_name, token_action),
        MonarchAction::Group(actions) => {
            let results = actions.iter().map(|action| compile_action(lexer, rule_name, action)).collect::<Result<Vec<_>, _>>()?;
            Ok(FuzzyAction::Action(Rc::new(IAction { group: Some(results), ..IAction::default() })))
        }
        MonarchAction::Cases(entries) => compile_cases(lexer, rule_name, entries),
    }
}

fn compile_token_action(lexer: &Rc<LexerMin>, rule_name: &str, action: &MonarchTokenAction) -> Result<FuzzyAction, MonarchError> {
    let Some(token) = &action.token else {
        return Err(create_error(
            &**lexer,
            &format!("an action must be a string, an object with a 'token' or 'cases' attribute, or an array of actions; in rule: {rule_name}"),
        ));
    };
    // only copy specific typed fields (only happens once during compile Lexer)
    let mut new_action = IAction { token: Some(token.clone()), ..IAction::default() };
    if token.contains('$') {
        new_action.token_subst = true;
    }
    if let Some(bracket) = &action.bracket {
        new_action.bracket = Some(match bracket.as_str() {
            "@open" => MonarchBracket::Open,
            "@close" => MonarchBracket::Close,
            _ => return Err(create_error(&**lexer, &format!("a 'bracket' attribute must be either '@open' or '@close', in rule: {rule_name}"))),
        });
    }
    if let Some(original) = action.next.as_ref().filter(|next| !next.is_empty()) {
        let mut next = original.clone();
        if !matches!(next.as_str(), "@pop" | "@push" | "@popall") {
            if let Some(stripped) = next.strip_prefix('@') {
                next = stripped.to_owned(); // peel off starting @ sign
            }
            if !next.contains('$') {
                // no dollar substitution, we can check if the state exists
                let resolved = substitute_matches(&**lexer, &js(&next), &JsString::new(), &[], &JsString::new());
                if !state_exists(&lexer.state_names, &resolved) {
                    return Err(create_error(&**lexer, &format!("the next state '{original}' is not defined in rule: {rule_name}")));
                }
            }
        }
        new_action.next = Some(next);
    }
    new_action.go_back = action.go_back;
    new_action.switch_to = action.switch_to.clone();
    new_action.log = action.log.clone();
    if let Some(next_embedded) = &action.next_embedded {
        new_action.next_embedded = Some(next_embedded.clone());
        lexer.uses_embedded.set(true);
    }
    Ok(FuzzyAction::Action(Rc::new(new_action)))
}

fn compile_cases(lexer: &Rc<LexerMin>, rule_name: &str, entries: &[(String, MonarchAction)]) -> Result<FuzzyAction, MonarchError> {
    // build an array of test cases
    let mut cases: Vec<Branch> = Vec::new();

    let mut has_embedded_end_in_cases = false;
    // for each case, push a test function and result value
    for (tkey, action) in entries {
        let val = compile_action(lexer, rule_name, action)?;

        // what kind of case
        if tkey == "@default" || tkey == "@" || tkey.is_empty() {
            cases.push(Branch { test: None, value: val.clone(), name: tkey.clone() });
        } else if tkey == "@eos" {
            cases.push(Branch { test: Some(make_branch_test(|_, _, _, eos| Ok(eos))), value: val.clone(), name: tkey.clone() });
        } else {
            cases.push(create_guard(lexer, rule_name, tkey, val.clone())?); // call separate function to avoid local variable capture
        }

        if !has_embedded_end_in_cases {
            has_embedded_end_in_cases = match &val {
                FuzzyAction::Str(_) => false,
                FuzzyAction::Action(inner) => {
                    inner.has_embedded_end_in_cases || matches!(inner.next_embedded.as_deref().unwrap_or(""), "@pop" | "@popall")
                }
            };
        }
    }

    // create a matching function
    let def = lexer.default_token.clone();
    let test = make_action_test(move |id, matches, state, eos| {
        for case in &cases {
            let did_match = match &case.test {
                Some(test) => test(id, matches, state, eos)?,
                None => true,
            };
            if did_match {
                return Ok(case.value.clone());
            }
        }
        Ok(FuzzyAction::Str(def.clone()))
    });
    Ok(FuzzyAction::Action(Rc::new(IAction { has_embedded_end_in_cases, test: Some(test), ..IAction::default() })))
}

/// Helper class for creating matching rules.
struct Rule {
    regex: RuleRegex,
    action: FuzzyAction,
    match_only_at_line_start: bool,
    name: String,
}

impl Rule {
    fn new(name: &str) -> Self {
        Self {
            regex: RuleRegex::Static(Rc::new(MonarchRegExp::new(&JsString::new(), false, false).expect("a regex vazia é válida"))),
            action: FuzzyAction::Action(Rc::new(IAction { token: Some(String::new()), ..IAction::default() })),
            match_only_at_line_start: false,
            name: name.to_owned(),
        }
    }

    fn set_regex(&mut self, lexer: &Rc<LexerMin>, re: &str) -> Result<(), MonarchError> {
        self.match_only_at_line_start = re.starts_with('^');
        self.name = format!("{}: {}", self.name, re);
        let body = if self.match_only_at_line_start { &re[1..] } else { re };
        self.regex = compile_reg_exp(lexer, &js(&format!("^(?:{body})")), true)?;
        Ok(())
    }

    fn set_action(&mut self, lexer: &Rc<LexerMin>, act: &MonarchAction) -> Result<(), MonarchError> {
        self.action = compile_action(lexer, &self.name, act)?;
        Ok(())
    }
}

impl IRule for Rule {
    fn action(&self) -> &FuzzyAction {
        &self.action
    }

    fn match_only_at_line_start(&self) -> bool {
        self.match_only_at_line_start
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn resolve_regex(&self, state: &JsString) -> Result<Rc<MonarchRegExp>, MonarchError> {
        match &self.regex {
            RuleRegex::Static(regex) => Ok(regex.clone()),
            RuleRegex::Dynamic(dynamic) => dynamic.resolve(state),
        }
    }
}

/// Compila uma lista de regras em `newrules`, expandindo os `include`.
fn add_rules(
    lexer: &Rc<LexerMin>,
    json: &MonarchLanguage,
    state: &str,
    newrules: &mut Vec<Rc<dyn IRule>>,
    rules: &[MonarchRule],
    depth: usize,
) -> Result<(), MonarchError> {
    if depth > MAX_INCLUDE_DEPTH {
        return Err(MonarchError("Maximum call stack size exceeded".to_owned()));
    }
    for rule in rules {
        if let MonarchRule::Include(include) = rule
            && !include.is_empty()
        {
            let include = include.strip_prefix('@').unwrap_or(include); // peel off starting @
            let Some((_, included)) = json.tokenizer.iter().find(|(name, _)| name == include) else {
                return Err(create_error(&**lexer, &format!("include target '{include}' is not defined at: {state}")));
            };
            add_rules(lexer, json, &format!("{state}.{include}"), newrules, included, depth + 1)?;
            continue;
        }

        let mut new_rule = Rule::new(state);

        // Set up new rule attributes
        match rule {
            MonarchRule::Tuple { regex, action, next } => {
                new_rule.set_regex(lexer, regex)?;
                match next {
                    Some(next) => match action {
                        MonarchAction::Str(token) => {
                            let token_action = MonarchTokenAction { token: Some(token.clone()), next: Some(next.clone()), ..MonarchTokenAction::default() };
                            new_rule.set_action(lexer, &MonarchAction::Token(token_action))?;
                        }
                        MonarchAction::Token(token_action) => {
                            let mut token_action = token_action.clone();
                            token_action.next = Some(next.clone());
                            new_rule.set_action(lexer, &MonarchAction::Token(token_action))?;
                        }
                        MonarchAction::Group(_) | MonarchAction::Cases(_) => new_rule.set_action(lexer, action)?,
                        MonarchAction::None => {
                            return Err(create_error(
                                &**lexer,
                                &format!("a next state as the last element of a rule can only be given if the action is either an object or a string, at: {state}"),
                            ));
                        }
                    },
                    None => new_rule.set_action(lexer, action)?,
                }
            }
            MonarchRule::Object { regex, action, name } => {
                let Some(regex) = regex.as_ref().filter(|regex| !regex.is_empty()) else {
                    return Err(create_error(&**lexer, &format!("a rule must either be an array, or an object with a 'regex' or 'include' field at: {state}")));
                };
                if let Some(name) = name {
                    new_rule.name = name.clone();
                }
                new_rule.set_regex(lexer, regex)?;
                new_rule.set_action(lexer, action)?;
            }
            MonarchRule::Include(_) => {
                return Err(create_error(&**lexer, &format!("a rule must either be an array, or an object with a 'regex' or 'include' field at: {state}")));
            }
        }

        newrules.push(Rc::new(new_rule));
    }
    Ok(())
}

fn default_brackets() -> Vec<MonarchBracketDef> {
    let bracket = |open: &str, close: &str, token: &str| MonarchBracketDef::Object {
        open: Some(open.to_owned()),
        close: Some(close.to_owned()),
        token: Some(token.to_owned()),
    };
    vec![
        bracket("{", "}", "delimiter.curly"),
        bracket("[", "]", "delimiter.square"),
        bracket("(", ")", "delimiter.parenthesis"),
        bracket("<", ">", "delimiter.angle"),
    ]
}

/// `compile(languageId, json)`: compila a descrição, expande os `include`, compila as definições de colchetes,
/// põe os padrões e faz as conferências de sanidade.
pub fn compile(language_id: &str, json: &MonarchLanguage) -> Result<Lexer, MonarchError> {
    // For calling compileAction later on
    let lexer_min = Rc::new(LexerMin {
        language_id: language_id.to_owned(),
        include_lf: json.include_lf.unwrap_or(false),
        no_throw: false, // raise exceptions during compilation
        ignore_case: json.ignore_case.unwrap_or(false),
        unicode: json.unicode.unwrap_or(false),
        uses_embedded: Cell::new(false), // becomes true if we find a nextEmbedded action
        default_token: json.default_token.clone().unwrap_or_else(|| "source".to_owned()),
        state_names: json.tokenizer.iter().map(|(name, _)| js(name)).collect(),
        start: json.start.clone(),
        token_postfix: json.token_postfix.clone(),
        attributes: json.attributes.iter().cloned().collect(),
    });

    // Create our lexer
    let mut lexer = Lexer {
        language_id: language_id.to_owned(),
        include_lf: lexer_min.include_lf,
        no_throw: false,
        max_stack: 100,
        start: json.start.as_ref().map(|start| js(start)),
        ignore_case: lexer_min.ignore_case,
        unicode: lexer_min.unicode,
        token_postfix: json.token_postfix.clone().unwrap_or_else(|| format!(".{language_id}")),
        default_token: lexer_min.default_token.clone(),
        uses_embedded: false,
        tokenizer: HashMap::new(),
        brackets: Vec::new(),
    };

    // compile the tokenizer rules
    for (key, rules) in &json.tokenizer {
        if lexer.start.as_ref().is_none_or(|start| start.is_empty()) {
            lexer.start = Some(js(key));
        }

        let mut compiled: Vec<Rc<dyn IRule>> = Vec::new();
        add_rules(&lexer_min, json, &format!("tokenizer.{key}"), &mut compiled, rules, 0)?;
        lexer.tokenizer.insert(js(key), Rc::new(compiled));
    }
    lexer.uses_embedded = lexer_min.uses_embedded.get(); // can be set during compileAction

    // Set simple brackets
    let bracket_defs = json.brackets.clone().unwrap_or_else(default_brackets);
    let mut brackets: Vec<MonarchLanguageBracket> = Vec::new();
    for el in &bracket_defs {
        let (open, close, token) = match el {
            MonarchBracketDef::Triple(open, close, token) => (Some(open.clone()), Some(close.clone()), Some(token.clone())),
            MonarchBracketDef::Object { open, close, token } => (open.clone(), close.clone(), token.clone()),
        };
        if open == close {
            return Err(create_error(
                &*lexer_min,
                &format!(
                    "open and close brackets in a 'brackets' attribute must be different: {}\n hint: use the 'bracket' attribute if matching on equal brackets is required.",
                    open.as_deref().unwrap_or("undefined")
                ),
            ));
        }
        match (open, close, token) {
            (Some(open), Some(close), Some(token)) => brackets.push(MonarchLanguageBracket { token: format!("{token}{}", lexer.token_postfix), open, close }),
            _ => {
                return Err(create_error(&*lexer_min, "every element in the 'brackets' array must be a '{open,close,token}' object or array"));
            }
        }
    }
    lexer.brackets = brackets
        .into_iter()
        .map(|bracket| Bracket {
            token: bracket.token,
            open: fix_case(&*lexer_min, &js(&bracket.open)).to_string_lossy(),
            close: fix_case(&*lexer_min, &js(&bracket.close)).to_string_lossy(),
        })
        .collect();

    // Disable throw so the syntax highlighter goes, no matter what
    lexer.no_throw = true;
    Ok(lexer)
}
