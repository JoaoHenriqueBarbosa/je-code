//! Porte de `src/vs/base/common/glob.ts`: o glob simplificado do VS Code (`files.exclude`,
//! `search.exclude`, associações de arquivo), que compila cada padrão para um `RegExp` ECMAScript
//! (aqui o `zyarr`, com a mesma semântica) ou, nos casos triviais (`**/*.ext`, `**/nome`, `a/b`...),
//! para uma comparação direta de texto.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `string | IRelativePattern | IExpression` viram tipos distintos: [`parse_pattern`] (texto ou
//!   [`RelativePattern`]), [`parse_expression`] ([`Expression`]) e a união [`parse`] sobre [`Glob`].
//!   `match` (palavra reservada) vira [`match_pattern`] e [`match_expression`]. A checagem de `null` e
//!   `undefined` do original é garantida pelo sistema de tipos.
//! - `isWindows`, `isLinux` e `sep` vêm das [`GlobOptions`] (`platform` e `is_linux`), não de estado
//!   global.
//! - A expressão devolve [`ExprMatch`]: o `string | null | Promise<string | null>` do original. O
//!   `hasSibling` devolve [`Sibling`], o `boolean | Promise<boolean>`.
//! - O cache de padrões (LRU de 10 000 entradas) é por thread.

use std::cell::RefCell;
use std::rc::Rc;

use crate::arrays;
use crate::extpath;
use crate::js_string::{self, is_js_whitespace};
use crate::map::LRUCache;
use crate::path::{Environment, PathModule, Platform, ProcessEnvironment};
use crate::promise::Promise;
use crate::strings;
use zyarr::RegExp;

/// `IRelativePattern`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelativePattern {
    /// Caminho base contra o qual o padrão é casado de forma relativa.
    pub base: String,
    /// Glob como `*.{ts,js}`, casado com o caminho relativo à base. Com base `/home/work/folder` e
    /// arquivo `/home/work/folder/index.js`, o padrão casa com `index.js`.
    pub pattern: String,
}

/// O valor de uma entrada de `IExpression` (`boolean | SiblingClause`, mais o que o JavaScript deixa
/// passar sem checar).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExprValue {
    /// `true` liga o padrão, `false` o desliga.
    Bool(bool),
    /// `{ when: '$(basename).ts' }`: casa só se o arquivo irmão existir. O `when` não string do
    /// original (`{}`, `{ bananas: true }`) é [`ExprValue::Other`].
    When(String),
    /// Qualquer outro valor que não seja `false` (`0`, `{}`...): o padrão vale como se fosse `true`.
    Other,
}

impl From<bool> for ExprValue {
    fn from(value: bool) -> Self {
        ExprValue::Bool(value)
    }
}

/// `IExpression`: padrão para valor, na ordem em que o JavaScript enumera as propriedades de um
/// objeto (`Object.getOwnPropertyNames`): chaves que são índice de array primeiro, em ordem numérica,
/// depois as demais na ordem de inserção.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Expression {
    entries: Vec<(String, ExprValue)>,
}

impl Expression {
    /// `getEmptyExpression()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Define uma propriedade; repetir a chave troca o valor sem mudar a posição, como num objeto.
    pub fn insert(&mut self, pattern: impl Into<String>, value: impl Into<ExprValue>) -> &mut Self {
        let (pattern, value) = (pattern.into(), value.into());
        match self.entries.iter_mut().find(|(key, _)| *key == pattern) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((pattern, value)),
        }
        self
    }

    pub fn from_entries<P: Into<String>, V: Into<ExprValue>>(entries: impl IntoIterator<Item = (P, V)>) -> Self {
        let mut expression = Self::new();
        for (pattern, value) in entries {
            expression.insert(pattern, value);
        }
        expression
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// As entradas na ordem de `Object.getOwnPropertyNames`.
    fn own_property_names(&self) -> Vec<&(String, ExprValue)> {
        let mut indexed: Vec<(u32, &(String, ExprValue))> = Vec::new();
        let mut named = Vec::new();
        for entry in &self.entries {
            match array_index(&entry.0) {
                Some(index) => indexed.push((index, entry)),
                None => named.push(entry),
            }
        }
        indexed.sort_by_key(|(index, _)| *index);
        indexed.into_iter().map(|(_, entry)| entry).chain(named).collect()
    }
}

/// O valor se `key` é um índice de array canônico (`"0"`, `"17"`, sem zero à esquerda, abaixo de 2^32 - 1).
fn array_index(key: &str) -> Option<u32> {
    let canonical = key == "0" || (!key.starts_with('0') && !key.is_empty());
    if canonical && key.bytes().all(|b| b.is_ascii_digit()) {
        key.parse::<u32>().ok().filter(|&index| index != u32::MAX)
    } else {
        None
    }
}

pub const GLOBSTAR: &str = "**";
pub const GLOB_SPLIT: char = '/';

const PATH_REGEX: &str = "[/\\\\]"; // any slash or backslash
const NO_PATH_REGEX: &str = "[^/\\\\]"; // any non-slash and non-backslash

/// `IGlobOptions`, mais a plataforma que o original lê de `platform.ts` e `path.ts`.
#[derive(Clone, Copy, Debug)]
pub struct GlobOptions {
    /// Simplifica padrões para uso como filtro de exclusão durante a travessia de árvore, para pular
    /// subárvores inteiras. Não vale fora de uma travessia.
    pub trim_for_exclusions: bool,
    /// Se o casamento ignora maiúsculas e minúsculas. Sem valor, o glob não ignora, mas o casamento de
    /// padrão relativo segue o sistema de arquivos da plataforma (`!is_linux`).
    pub ignore_case: Option<bool>,
    /// `process.platform === 'win32'` decide o `sep` e a semântica de `path`.
    pub platform: Platform,
    /// `platform.isLinux`.
    pub is_linux: bool,
}

impl Default for GlobOptions {
    fn default() -> Self {
        Self {
            trim_for_exclusions: false,
            ignore_case: None,
            platform: ProcessEnvironment.platform(),
            is_linux: cfg!(target_os = "linux"),
        }
    }
}

impl GlobOptions {
    fn ignore_case(&self) -> bool {
        self.ignore_case.unwrap_or(false)
    }

    fn equals(&self, a: &str, b: &str) -> bool {
        if self.ignore_case() { strings::equals_ignore_case(&units(a), &units(b)) } else { a == b }
    }

    fn ends_with(&self, text: &str, candidate: &str) -> bool {
        if self.ignore_case() { strings::ends_with_ignore_case(&units(text), &units(candidate)) } else { text.ends_with(candidate) }
    }

    fn sep(&self) -> &'static str {
        PathModule::native(&PlatformEnv(self.platform)).sep()
    }

    /// O trecho de chave do cache que depende das opções.
    fn cache_suffix(&self) -> String {
        let platform = if self.platform == Platform::Win32 { 'w' } else { 'p' };
        format!("_{}_{}_{platform}", self.trim_for_exclusions, self.ignore_case())
    }
}

/// O ambiente mínimo que `path` e `extpath` pedem: só a plataforma conta, o resto nunca é lido
/// pelas funções que o glob usa.
struct PlatformEnv(Platform);

impl Environment for PlatformEnv {
    fn platform(&self) -> Platform {
        self.0
    }

    fn cwd(&self) -> String {
        "/".to_string()
    }

    fn env_var(&self, _name: &str) -> Option<String> {
        None
    }
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `String#trim` do JavaScript (o conjunto de espaços dele difere do `str::trim` do Rust).
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.len_utf16() == 1 && is_js_whitespace(c as u16))
}

fn starts_to_reg_exp(star_count: usize, is_last_pattern: bool) -> String {
    match star_count {
        0 => String::new(),
        // 1 star matches any number of characters except path separator (/ and \) - non greedy (?)
        1 => format!("{NO_PATH_REGEX}*?"),
        // Matches: (Path Sep OR Path Val followed by Path Sep) 0-many times except when it's the last
        // pattern in which case also matches (Path Sep followed by Path Val). Group is non capturing,
        // and non-greedy matching is used because it could be that we match too much.
        _ => {
            let last = if is_last_pattern { format!("|{PATH_REGEX}{NO_PATH_REGEX}+") } else { String::new() };
            format!("(?:{PATH_REGEX}|{NO_PATH_REGEX}+{PATH_REGEX}{last})*?")
        }
    }
}

pub fn split_glob_aware(pattern: &str, split_char: char) -> Vec<String> {
    if pattern.is_empty() {
        return Vec::new();
    }

    let mut segments = Vec::new();
    let mut in_braces = false;
    let mut in_brackets = false;
    let mut cur_val = String::new();

    for ch in pattern.chars() {
        if ch == split_char {
            if !in_braces && !in_brackets {
                segments.push(std::mem::take(&mut cur_val));
                continue;
            }
        } else {
            match ch {
                '{' => in_braces = true,
                '}' => in_braces = false,
                '[' => in_brackets = true,
                ']' => in_brackets = false,
                _ => {}
            }
        }
        cur_val.push(ch);
    }

    // Tail
    if !cur_val.is_empty() {
        segments.push(cur_val);
    }

    segments
}

/// `escapeRegExpCharacters` de um caractere.
fn escape_char(ch: char) -> String {
    let mut buffer = [0u16; 2];
    strings::escape_reg_exp_characters(ch.encode_utf16(&mut buffer)).to_string_lossy()
}

fn parse_reg_exp(pattern: &str) -> String {
    if pattern.is_empty() {
        return String::new();
    }

    let mut reg_ex = String::new();

    // Split up into segments for each slash found
    let segments = split_glob_aware(pattern, GLOB_SPLIT);

    // Special case where we only have globstars
    if segments.iter().all(|segment| segment == GLOBSTAR) {
        reg_ex.push_str(".*");
    }
    // Build regex over segments
    else {
        let mut previous_segment_was_glob_star = false;
        for (index, segment) in segments.iter().enumerate() {
            let is_glob_star = segment == GLOBSTAR;

            // Treat globstar specially
            if is_glob_star {
                // if we have more than one globstar after another, just ignore it
                if !previous_segment_was_glob_star {
                    reg_ex.push_str(&starts_to_reg_exp(2, index == segments.len() - 1));
                }
            }
            // Anything else, not globstar
            else {
                parse_segment(segment, &mut reg_ex);

                // Tail: Add the slash we had split on if there is more to come and the remaining
                // pattern is not a globstar. For example if pattern: some/**/*.js we want the "/"
                // after some to be included in the RegEx to prevent a folder called "something" to
                // match as well.
                if index < segments.len() - 1 // more segments to come after this
                    && (segments[index + 1] != GLOBSTAR // next segment is not **, or...
                        || index + 2 < segments.len())
                // ...next segment is ** but there is more segments after that
                {
                    reg_ex.push_str(PATH_REGEX);
                }
            }

            // update globstar state
            previous_segment_was_glob_star = is_glob_star;
        }
    }

    reg_ex
}

/// O ramo "qualquer coisa que não seja globstar" do `parseRegExp`: converte um segmento, com
/// chaves e colchetes.
fn parse_segment(segment: &str, reg_ex: &mut String) {
    // States
    let mut in_braces = false;
    let mut brace_val = String::new();

    let mut in_brackets = false;
    let mut bracket_val = String::new();

    for ch in segment.chars() {
        // Support brace expansion
        if ch != '}' && in_braces {
            brace_val.push(ch);
            continue;
        }

        // Support brackets. `]` is literally only allowed as first character in brackets to match it.
        if in_brackets && (ch != ']' || bracket_val.is_empty()) {
            let res = match ch {
                // range operator
                '-' => "-".to_string(),
                // negation operator (only valid on first index in bracket)
                '^' | '!' if bracket_val.is_empty() => "^".to_string(),
                // glob split matching is not allowed within character ranges
                // see http://man7.org/linux/man-pages/man7/glob.7.html
                GLOB_SPLIT => String::new(),
                // anything else gets escaped
                _ => escape_char(ch),
            };

            bracket_val.push_str(&res);
            continue;
        }

        match ch {
            '{' => in_braces = true,
            '[' => in_brackets = true,
            '}' => {
                let choices = split_glob_aware(&brace_val, ',');

                // Converts {foo,bar} => [foo|bar]
                let alternatives: Vec<String> = choices.iter().map(|choice| parse_reg_exp(choice)).collect();
                reg_ex.push_str(&format!("(?:{})", alternatives.join("|")));

                in_braces = false;
                brace_val.clear();
            }
            ']' => {
                reg_ex.push_str(&format!("[{bracket_val}]"));

                in_brackets = false;
                bracket_val.clear();
            }
            // 1 ? matches any single character except path separator (/ and \)
            '?' => reg_ex.push_str(NO_PATH_REGEX),
            '*' => reg_ex.push_str(&starts_to_reg_exp(1, false)),
            _ => reg_ex.push_str(&escape_char(ch)),
        }
    }
}

/// Os regex dos padrões triviais, que só precisam de `endsWith` e igualdade.
struct Trivia {
    t1: RegExp,  // **/*.something
    t2: RegExp,  // **/something
    t3: RegExp,  // {**/*.something,**/*.else} or {**/package.json,**/project.json}
    t3_2: RegExp, // Like T3, with optional trailing /**
    t4: RegExp,  // **/something/else
    t5: RegExp,  // something/else
}

impl Trivia {
    fn new() -> Self {
        let compile = |pattern: &str| RegExp::from_str(pattern, "").expect("regex trivial válido");
        Self {
            t1: compile(r"^\*\*\/\*\.[\w\.-]+$"),
            t2: compile(r"^\*\*\/([\w\.-]+)\/?$"),
            t3: compile(r"^{\*\*\/\*?[\w\.-]+\/?(,\*\*\/\*?[\w\.-]+\/?)*}$"),
            t3_2: compile(r"^{\*\*\/\*?[\w\.-]+(\/(\*\*)?)?(,\*\*\/\*?[\w\.-]+(\/(\*\*)?)?)*}$"),
            t4: compile(r"^\*\*((\/[\w\.-]+)+)\/?$"),
            t5: compile(r"^([\w\.-]+(\/[\w\.-]+)*)\/?$"),
        }
    }
}

thread_local! {
    static TRIVIA: Trivia = Trivia::new();
    static CACHE: LRUCache<String, Node> = LRUCache::new(10000, 1.0); // bounded to 10000 elements
    /// Buffer reaproveitado para o texto UTF-16 que o regex consome.
    static SCRATCH: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
}

fn test_regex(regex: &RegExp, text: &str) -> bool {
    regex.exec(&units(text), 0).is_some()
}

/// O grupo 1 de `regex.exec(text)`.
fn first_group(regex: &RegExp, text: &str) -> Option<String> {
    let text_units = units(text);
    let found = regex.exec(&text_units, 0)?;
    found.captures.get(1)?.as_ref().map(|range| String::from_utf16_lossy(&text_units[range.clone()]))
}

/// O que um glob devolve: o `string | null | Promise<string | null>` do original (o texto é o
/// padrão que casou).
#[derive(Clone)]
pub enum ExprMatch {
    None,
    Matched(Rc<str>),
    /// Só quando o `hasSibling` devolveu uma promessa.
    Pending(Promise<Option<Rc<str>>>),
}

impl ExprMatch {
    /// O padrão que casou, se o resultado já é conhecido.
    pub fn matched(&self) -> Option<&str> {
        match self {
            ExprMatch::Matched(pattern) => Some(pattern),
            _ => None,
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, ExprMatch::None)
    }

    /// O resultado como promessa (o `await` do original, que aceita valor ou promessa).
    pub fn into_promise(self) -> Promise<Option<Rc<str>>> {
        match self {
            ExprMatch::None => Promise::resolved(None),
            ExprMatch::Matched(pattern) => Promise::resolved(Some(pattern)),
            ExprMatch::Pending(promise) => promise,
        }
    }
}

impl From<Option<Rc<str>>> for ExprMatch {
    fn from(value: Option<Rc<str>>) -> Self {
        value.map_or(ExprMatch::None, ExprMatch::Matched)
    }
}

impl std::fmt::Debug for ExprMatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExprMatch::None => f.write_str("None"),
            ExprMatch::Matched(pattern) => write!(f, "Matched({pattern:?})"),
            ExprMatch::Pending(_) => f.write_str("Pending"),
        }
    }
}

/// O que o `hasSibling` devolve: `boolean | Promise<boolean>`.
#[derive(Clone)]
pub enum Sibling {
    Ready(bool),
    Pending(Promise<bool>),
}

impl From<bool> for Sibling {
    fn from(value: bool) -> Self {
        Sibling::Ready(value)
    }
}

impl From<Promise<bool>> for Sibling {
    fn from(value: Promise<bool>) -> Self {
        Sibling::Pending(value)
    }
}

/// O `hasSibling` de `ParsedExpression`.
pub type HasSibling<'a> = &'a dyn Fn(&str) -> Sibling;

type NodeFn = dyn Fn(&str, Option<&str>, Option<&str>, Option<&dyn Fn(&str) -> Sibling>) -> ExprMatch;
type Terms = Rc<[String]>;

/// Um padrão compilado, com os metadados que o original pendura na função (`basenames`, `patterns`,
/// `allBasenames`, `allPaths`, `requiresSiblings`). O `NULL` do original é `is_null`.
#[derive(Clone)]
struct Node {
    f: Rc<NodeFn>,
    is_null: bool,
    requires_siblings: bool,
    basenames: Option<Terms>,
    patterns: Option<Rc<[Rc<str>]>>,
    all_basenames: Option<Terms>,
    all_paths: Option<Terms>,
}

impl Node {
    fn with_fn(f: Rc<NodeFn>) -> Self {
        Self { f, is_null: false, requires_siblings: false, basenames: None, patterns: None, all_basenames: None, all_paths: None }
    }

    /// Um `ParsedStringPattern`: só vê o caminho e o basename.
    fn simple(f: impl Fn(&str, Option<&str>) -> Option<Rc<str>> + 'static) -> Self {
        Self::with_fn(Rc::new(move |path, basename, _, _| f(path, basename).into()))
    }

    fn null() -> Self {
        Self { is_null: true, ..Self::simple(|_, _| None) }
    }

    fn call(&self, path: &str, basename: Option<&str>) -> ExprMatch {
        (self.f)(path, basename, None, None)
    }

    /// Metadados que o padrão agregado herda: o primeiro `allBasenames` e todos os `allPaths`.
    fn with_merged_terms(mut self, nodes: &[Node]) -> Self {
        self.all_basenames = nodes.iter().find_map(|node| node.all_basenames.clone());
        let all_paths: Vec<String> = nodes.iter().filter_map(|node| node.all_paths.as_deref()).flatten().cloned().collect();
        self.all_paths = (!all_paths.is_empty()).then(|| all_paths.into());
        self
    }
}

/// `ParsedPattern`: `(path, basename?) => boolean`.
#[derive(Clone)]
pub struct ParsedPattern(Node);

impl ParsedPattern {
    pub fn matches(&self, path: &str, basename: Option<&str>) -> bool {
        !self.0.call(path, basename).is_none()
    }

    /// `isEmptyPattern`: um padrão vazio nunca casa com nada.
    pub fn is_empty(&self) -> bool {
        self.0.is_null
    }
}

/// `ParsedExpression`: `(path, basename?, hasSibling?) => string | null | Promise<string | null>`.
#[derive(Clone)]
pub struct ParsedExpression(Node);

impl ParsedExpression {
    pub fn matches(&self, path: &str, basename: Option<&str>, has_sibling: Option<HasSibling<'_>>) -> ExprMatch {
        (self.0.f)(path, basename, None, has_sibling)
    }

    /// `isEmptyPattern`.
    pub fn is_empty(&self) -> bool {
        self.0.is_null
    }
}

/// Um glob de qualquer das três formas (`string | IRelativePattern | IExpression`).
#[derive(Clone, Copy)]
pub enum Glob<'a> {
    Pattern(&'a str),
    Relative(&'a RelativePattern),
    Expression(&'a Expression),
}

/// O que [`parse`] devolve.
#[derive(Clone)]
pub enum ParsedGlob {
    Pattern(ParsedPattern),
    Expression(ParsedExpression),
}

/// Um glob de texto: `string | IRelativePattern`.
#[derive(Clone, Copy)]
pub enum PatternArg<'a> {
    Str(&'a str),
    Relative(&'a RelativePattern),
}

impl<'a> From<&'a str> for PatternArg<'a> {
    fn from(value: &'a str) -> Self {
        PatternArg::Str(value)
    }
}

impl<'a> From<&'a String> for PatternArg<'a> {
    fn from(value: &'a String) -> Self {
        PatternArg::Str(value)
    }
}

impl<'a> From<&'a RelativePattern> for PatternArg<'a> {
    fn from(value: &'a RelativePattern) -> Self {
        PatternArg::Relative(value)
    }
}

/// Os metadados de termos, para `getBasenameTerms` e `getPathTerms`.
pub trait GlobTerms {
    fn all_basenames(&self) -> Option<&[String]>;
    fn all_paths(&self) -> Option<&[String]>;
}

macro_rules! impl_terms {
    ($($ty:ty => |$this:ident| $node:expr),* $(,)?) => {$(
        impl GlobTerms for $ty {
            fn all_basenames(&self) -> Option<&[String]> {
                let $this = self;
                let node: &Node = $node;
                node.all_basenames.as_deref()
            }

            fn all_paths(&self) -> Option<&[String]> {
                let $this = self;
                let node: &Node = $node;
                node.all_paths.as_deref()
            }
        }
    )*};
}

impl_terms! {
    ParsedPattern => |this| &this.0,
    ParsedExpression => |this| &this.0,
    ParsedGlob => |this| match this {
        ParsedGlob::Pattern(pattern) => &pattern.0,
        ParsedGlob::Expression(expression) => &expression.0,
    },
}

/// Se o padrão ou a expressão é vazio e por isso nunca casa com nada.
pub fn is_empty_pattern(glob: &ParsedGlob) -> bool {
    match glob {
        ParsedGlob::Pattern(pattern) => pattern.is_empty(),
        ParsedGlob::Expression(expression) => expression.is_empty(),
    }
}

fn trim_for_exclusions<'a>(pattern: &'a str, options: &GlobOptions) -> &'a str {
    match pattern.strip_suffix("**") {
        Some(rest) if options.trim_for_exclusions && rest.ends_with('/') => rest, // dropping **, tailing / is dropped later
        _ => pattern,
    }
}

fn parse_string_pattern(arg: PatternArg<'_>, options: &GlobOptions, cache_key: Option<&str>) -> Node {
    // Handle relative patterns
    let raw = match arg {
        PatternArg::Str("") => return Node::null(),
        PatternArg::Str(pattern) => pattern,
        PatternArg::Relative(relative) => &relative.pattern,
    };

    // Whitespace trimming
    let pattern = js_trim(raw);

    let ignore_case = options.ignore_case();

    // Check cache
    let key_body = match cache_key {
        None => {
            let pattern_key = if ignore_case { js_string::JsString::from(pattern).to_lower_case().to_string_lossy() } else { pattern.to_string() };
            format!("default:{pattern_key}")
        }
        Some(custom) => format!("custom:{custom}"),
    };
    let key = format!("{key_body}{}", options.cache_suffix());
    if let Some(cached) = CACHE.with(|cache| cache.get(&key)) {
        return wrap_relative_pattern(cached, arg, options);
    }

    // Check for Trivials
    let parsed = TRIVIA.with(|trivia| {
        let trimmed = trim_for_exclusions(pattern, options);
        if test_regex(&trivia.t1, pattern) {
            trivia1(&pattern[4..], pattern, options) // common pattern: **/*.txt just need endsWith check
        } else if let Some(base) = first_group(&trivia.t2, trimmed) {
            trivia2(&base, pattern, options) // common pattern: **/some.txt just need basename check
        } else if test_regex(if options.trim_for_exclusions { &trivia.t3_2 } else { &trivia.t3 }, pattern) {
            trivia3(pattern, options) // repetition of common patterns (see above) {**/*.txt,**/*.png}
        } else if let Some(path) = first_group(&trivia.t4, trimmed) {
            trivia4and5(&path[1..], pattern, true, options) // common pattern: **/something/else just need endsWith check
        } else if let Some(path) = first_group(&trivia.t5, trimmed) {
            trivia4and5(&path, pattern, false, options) // common pattern: something/else just need equals check
        }
        // Otherwise convert to pattern
        else {
            to_reg_exp(pattern, options)
        }
    });

    // Cache
    CACHE.with(|cache| {
        cache.set(key, parsed.clone());
    });

    wrap_relative_pattern(parsed, arg, options)
}

fn wrap_relative_pattern(parsed: Node, arg: PatternArg<'_>, options: &GlobOptions) -> Node {
    let PatternArg::Relative(relative) = arg else {
        return parsed;
    };

    let base = relative.base.clone();
    let (platform, ignore_case) = (options.platform, options.ignore_case.unwrap_or(!options.is_linux));
    let sep = units(options.sep());
    let inner = parsed.clone();
    let wrapped = Node::simple(move |path, basename| {
        if !extpath::is_equal_or_parent(path, &base, ignore_case, false, &PlatformEnv(platform)) {
            // skip glob matching if `base` is not a parent of `path`
            return None;
        }

        // Given we have checked `base` being a parent of `path`, we can now remove the `base` portion
        // of the `path` and only match on the remaining path components. We have to account for the
        // fact that `base` might end in a path separator (https://github.com/microsoft/vscode/issues/162498)
        let path_units = units(path);
        let rest = strings::ltrim(js_string::substring(&path_units, units(&base).len() as i64, path_units.len() as i64), &sep);
        match inner.call(&String::from_utf16_lossy(rest), basename) {
            ExprMatch::Matched(pattern) => Some(pattern),
            _ => None,
        }
    });

    // Make sure to preserve associated metadata
    Node { all_basenames: parsed.all_basenames, all_paths: parsed.all_paths, basenames: parsed.basenames, patterns: parsed.patterns, ..wrapped }
}

/// A forma com que o padrão volta quando casa: o texto do padrão.
fn matched(pattern: &Rc<str>) -> Option<Rc<str>> {
    Some(pattern.clone())
}

// common pattern: **/*.txt just need endsWith check
fn trivia1(base: &str, pattern: &str, options: &GlobOptions) -> Node {
    let (base, pattern, options) = (base.to_string(), Rc::<str>::from(pattern), *options);
    Node::simple(move |path, _| if options.ends_with(path, &base) { matched(&pattern) } else { None })
}

// common pattern: **/some.txt just need basename check
fn trivia2(base: &str, pattern: &str, options: &GlobOptions) -> Node {
    let slash_base = format!("/{base}");
    let backslash_base = format!("\\{base}");
    let pattern_rc = Rc::<str>::from(pattern);
    let (base_owned, options) = (base.to_string(), *options);

    let basenames: Terms = vec![base.to_string()].into();
    Node {
        basenames: Some(basenames.clone()),
        patterns: Some(vec![pattern_rc.clone()].into()),
        all_basenames: Some(basenames),
        ..Node::simple(move |path, basename| {
            let found = match basename {
                Some(basename) if !basename.is_empty() => options.equals(basename, &base_owned),
                _ => options.equals(path, &base_owned) || options.ends_with(path, &slash_base) || options.ends_with(path, &backslash_base),
            };
            if found { matched(&pattern_rc) } else { None }
        })
    }
}

// repetition of common patterns (see above) {**/*.txt,**/*.png}
fn trivia3(pattern: &str, options: &GlobOptions) -> Node {
    let parsed: Vec<Node> = pattern[1..pattern.len() - 1]
        .split(',')
        .map(|sub| parse_string_pattern(PatternArg::Str(sub), options, None))
        .filter(|node| !node.is_null)
        .collect();
    let mut parsed = aggregate_basename_matches(parsed, Some(pattern), options.ignore_case());

    match parsed.len() {
        0 => Node::null(),
        1 => parsed.remove(0),
        _ => {
            let pattern_rc = Rc::<str>::from(pattern);
            let nodes = parsed.clone();
            Node::simple(move |path, basename| {
                if nodes.iter().any(|node| !node.call(path, basename).is_none()) { matched(&pattern_rc) } else { None }
            })
            .with_merged_terms(&parsed)
        }
    }
}

// common patterns: **/something/else just need endsWith check, something/else just needs and equals check
fn trivia4and5(target_path: &str, pattern: &str, match_path_ends: bool, options: &GlobOptions) -> Node {
    let using_posix_sep = options.sep() == "/";
    let native_path = if using_posix_sep { target_path.to_string() } else { target_path.replace('/', options.sep()) };
    let native_path_end = format!("{}{native_path}", options.sep());
    let target_path_end = format!("/{target_path}");
    let (target_path_owned, pattern_rc, options) = (target_path.to_string(), Rc::<str>::from(pattern), *options);

    let node = if match_path_ends {
        Node::simple(move |path, _| {
            let found = (options.equals(path, &native_path) || options.ends_with(path, &native_path_end))
                || !using_posix_sep && (options.equals(path, &target_path_owned) || options.ends_with(path, &target_path_end));
            if found { matched(&pattern_rc) } else { None }
        })
    } else {
        Node::simple(move |path, _| {
            let found = options.equals(path, &native_path) || (!using_posix_sep && options.equals(path, &target_path_owned));
            if found { matched(&pattern_rc) } else { None }
        })
    };

    Node { all_paths: Some(vec![format!("{}{target_path}", if match_path_ends { "*/" } else { "./" })].into()), ..node }
}

fn to_reg_exp(pattern: &str, options: &GlobOptions) -> Node {
    let source = format!("^{}$", parse_reg_exp(pattern));
    let Ok(regex) = RegExp::from_str(&source, if options.ignore_case() { "i" } else { "" }) else {
        return Node::null();
    };
    let pattern = Rc::<str>::from(pattern);
    Node::simple(move |path, _| {
        let found = SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            scratch.clear();
            scratch.extend(path.encode_utf16());
            regex.exec(&scratch, 0).is_some()
        });
        if found { matched(&pattern) } else { None }
    })
}

/// Parse de um glob de texto (`string | IRelativePattern`).
///
/// Glob simplificado. Suporta um subconjunto:
/// * `*` casa zero ou mais caracteres num segmento de caminho
/// * `?` casa um caractere num segmento de caminho
/// * `**` casa qualquer quantidade de segmentos, inclusive nenhum
/// * `{}` agrupa condições (`*.{ts,js}` casa arquivos TypeScript e JavaScript)
/// * `[]` declara um intervalo de caracteres num segmento (`example.[0-9]` casa `example.0`, `example.1`...)
/// * `[!...]` nega o intervalo (`example.[!0-9]` casa `example.a`, não `example.0`)
pub fn parse_pattern<'a>(pattern: impl Into<PatternArg<'a>>, options: &GlobOptions) -> ParsedPattern {
    let arg = pattern.into();
    let parsed = parse_string_pattern(arg, options, None);
    if parsed.is_null {
        return ParsedPattern(Node::null());
    }

    ParsedPattern(parsed)
}

/// Parse de uma expressão (`IExpression`): vários padrões, com `when` e siblings.
pub fn parse_expression(expression: &Expression, options: &GlobOptions) -> ParsedExpression {
    let nodes: Vec<Node> = expression
        .own_property_names()
        .into_iter()
        .map(|(pattern, value)| parse_expression_pattern(pattern, value, options))
        .filter(|node| !node.is_null)
        .collect();
    let nodes = aggregate_basename_matches(nodes, None, options.ignore_case());

    if nodes.is_empty() {
        return ParsedExpression(Node::null());
    }

    if !nodes.iter().any(|node| node.requires_siblings) && nodes.len() == 1 {
        return ParsedExpression(nodes[0].clone());
    }

    let platform = options.platform;
    let combined = nodes.clone();
    let node = Node::with_fn(Rc::new(move |path, base, _, has_sibling| {
        let mut base = base.map(str::to_string);
        let mut name: Option<String> = None;
        let mut pending: Vec<Promise<Option<Rc<str>>>> = Vec::new();

        for node in &combined {
            // Pattern matches path
            if node.requires_siblings && has_sibling.is_some() {
                let env = PlatformEnv(platform);
                let path_module = PathModule::native(&env);
                if base.as_deref().is_none_or(str::is_empty) {
                    base = Some(path_module.basename(path, None));
                }
                if name.as_deref().is_none_or(str::is_empty) {
                    let base_units = units(base.as_deref().unwrap_or_default());
                    let ext_len = units(&path_module.extname(path)).len();
                    name = Some(String::from_utf16_lossy(&base_units[..base_units.len().saturating_sub(ext_len)]));
                }
            }

            match (node.f)(path, base.as_deref(), name.as_deref(), has_sibling) {
                // immediately return as soon as the first expression matches
                result @ ExprMatch::Matched(_) => return result,
                // If the result is a promise, we have to keep it for later processing and await
                // the result properly.
                ExprMatch::Pending(promise) => pending.push(promise),
                ExprMatch::None => {}
            }
        }

        // With result promises, we have to loop over each and await the result before we can
        // return any result.
        if pending.is_empty() { ExprMatch::None } else { ExprMatch::Pending(first_match(pending.into(), 0)) }
    }));

    ParsedExpression(Node { requires_siblings: nodes.iter().any(|node| node.requires_siblings), ..node.with_merged_terms(&nodes) })
}

/// Espera as promessas em ordem e devolve o primeiro resultado que for um padrão; uma rejeição
/// rejeita o conjunto, como o `await` do original.
fn first_match(promises: Rc<[Promise<Option<Rc<str>>>]>, index: usize) -> Promise<Option<Rc<str>>> {
    let Some(promise) = promises.get(index).cloned() else {
        return Promise::resolved(None);
    };
    promise.then_promise(move |result| if result.is_some() { Promise::resolved(result) } else { first_match(promises, index + 1) })
}

fn parse_expression_pattern(pattern: &str, value: &ExprValue, options: &GlobOptions) -> Node {
    if *value == ExprValue::Bool(false) {
        return Node::null(); // pattern is disabled
    }

    let parsed = parse_string_pattern(PatternArg::Str(pattern), options, Some(pattern));
    if parsed.is_null {
        return Node::null();
    }

    // Expression Pattern is <SiblingClause>
    let ExprValue::When(when) = value else {
        // Expression Pattern is <boolean>, or anything else
        return parsed;
    };

    let (when, pattern) = (when.clone(), Rc::<str>::from(pattern));
    Node {
        requires_siblings: true,
        ..Node::with_fn(Rc::new(move |path, basename, name, has_sibling| {
            let Some(has_sibling) = has_sibling else {
                return ExprMatch::None;
            };
            if parsed.call(path, basename).is_none() {
                return ExprMatch::None;
            }

            let clause_pattern = when.replacen("$(basename)", name.unwrap_or_default(), 1);
            match has_sibling(&clause_pattern) {
                Sibling::Pending(promise) => {
                    let pattern = pattern.clone();
                    ExprMatch::Pending(promise.then(move |found| found.then_some(pattern)))
                }
                Sibling::Ready(true) => ExprMatch::Matched(pattern.clone()),
                Sibling::Ready(false) => ExprMatch::None,
            }
        }))
    }
}

fn aggregate_basename_matches(parsed: Vec<Node>, result: Option<&str>, ignore_case: bool) -> Vec<Node> {
    let basename_patterns: Vec<&Node> = parsed.iter().filter(|node| node.basenames.is_some()).collect();
    if basename_patterns.len() < 2 {
        return parsed;
    }

    let basenames: Vec<String> = basename_patterns.iter().filter_map(|node| node.basenames.as_deref()).flatten().cloned().collect();
    let patterns: Vec<Rc<str>> = match result {
        Some(result) => {
            let result = Rc::<str>::from(result);
            vec![result; basenames.len()]
        }
        None => basename_patterns.iter().filter_map(|node| node.patterns.as_deref()).flatten().cloned().collect(),
    };

    let (matcher_basenames, matcher_patterns) = (basenames.clone(), patterns.clone());
    let aggregate = Node {
        basenames: Some(basenames.clone().into()),
        patterns: Some(patterns.into()),
        all_basenames: Some(basenames.into()),
        ..Node::simple(move |path, basename| {
            let basename = match basename {
                Some(basename) if !basename.is_empty() => basename,
                _ => path.rfind(['/', '\\']).map_or(path, |index| &path[index + 1..]),
            };

            let index = if ignore_case {
                let basename_units = units(basename);
                matcher_basenames.iter().position(|candidate| strings::equals_ignore_case(&units(candidate), &basename_units))
            } else {
                matcher_basenames.iter().position(|candidate| candidate == basename)
            };
            index.map(|index| matcher_patterns[index].clone())
        })
    };

    let mut aggregated: Vec<Node> = parsed.into_iter().filter(|node| node.basenames.is_none()).collect();
    aggregated.push(aggregate);
    aggregated
}

/// `parse(arg1, options)` para qualquer das três formas.
pub fn parse(glob: Glob<'_>, options: &GlobOptions) -> ParsedGlob {
    match glob {
        Glob::Pattern(pattern) => ParsedGlob::Pattern(parse_pattern(pattern, options)),
        Glob::Relative(relative) => ParsedGlob::Pattern(parse_pattern(relative, options)),
        Glob::Expression(expression) => ParsedGlob::Expression(parse_expression(expression, options)),
    }
}

/// `isRelativePattern`.
pub fn is_relative_pattern(glob: &Glob<'_>) -> bool {
    matches!(glob, Glob::Relative(_))
}

/// `match` para texto ou padrão relativo.
pub fn match_pattern<'a>(pattern: impl Into<PatternArg<'a>>, path: &str, options: &GlobOptions) -> bool {
    parse_pattern(pattern, options).matches(path, None)
}

/// `match` para uma expressão: o padrão que casou, ou `None`.
pub fn match_expression(expression: &Expression, path: &str, options: &GlobOptions) -> ExprMatch {
    parse_expression(expression, options).matches(path, None, None)
}

pub fn get_basename_terms(glob: &impl GlobTerms) -> Vec<String> {
    glob.all_basenames().unwrap_or_default().to_vec()
}

pub fn get_path_terms(glob: &impl GlobTerms) -> Vec<String> {
    glob.all_paths().unwrap_or_default().to_vec()
}

/// `string | IRelativePattern`, para `patternsEquals`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternItem {
    Str(String),
    Relative(RelativePattern),
}

impl From<&str> for PatternItem {
    fn from(value: &str) -> Self {
        PatternItem::Str(value.to_string())
    }
}

impl From<RelativePattern> for PatternItem {
    fn from(value: RelativePattern) -> Self {
        PatternItem::Relative(value)
    }
}

// NOTE: This is not used for actual matching, only for resetting watcher when patterns change.
// That is why it's ok to avoid case-insensitive comparison here.
pub fn patterns_equals(patterns_a: Option<&[PatternItem]>, patterns_b: Option<&[PatternItem]>) -> bool {
    arrays::equals(patterns_a, patterns_b, |a, b| a == b)
}
