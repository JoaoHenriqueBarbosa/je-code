//! Porte de `src/vs/platform/contextkey/common/contextkey.ts`: as expressões de contexto (`when`),
//! a normalização, o `negate`, o `evaluate`, o `implies` e o `Parser`.
//!
//! Representação:
//! - Uma expressão é um [`ContextKeyExpression`], um `Rc` imutável sobre o enum [`Expr`]. As folhas
//!   `true` e `false` são instâncias únicas por thread (o original as compara por identidade em
//!   `eliminateConstantsInArray`).
//! - Chaves são `String`; a ordem (`cmp`) compara em unidades UTF-16, como o `<` do JavaScript.
//! - Os valores de `==`, `!=` e do contexto (`any`) são [`Dynamic`].
//! - `RegExp` é o [`ContextRegExp`], sobre o `zyarr` (a mesma semântica ECMAScript do V8). `lastIndex` de
//!   regex global não é modelado: `test` sempre parte do início.
//! - O `negated` guardado em `And`/`Or` é a negação já calculada. A ligação de volta (a negação aponta
//!   para a expressão original) é um `Weak`, para o ciclo de `Rc` não vazar. Nas folhas a negação é
//!   recalculada, o que é barato e dá o mesmo resultado estrutural.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy, number_to_js_string};
use je_base::errors::{self, Error};
use je_base::js_string;
use je_base::nls::localize;
use je_base::platform::Platform;
use zyarr::{RegExp, RegExpError};

use super::scanner::{LexingError, Scanner, Token, TokenType, get_lexeme};
use crate::js::{has_own_property, less_than, parse_float, to_js_string};

thread_local! {
    static PLATFORM: Platform = Platform::detect();
    static CONSTANT_VALUES: RefCell<HashMap<String, bool>> = RefCell::new(initial_constants());
    static TRUE_INSTANCE: ContextKeyExpression = ContextKeyExpression(Rc::new(Expr::True));
    static FALSE_INSTANCE: ContextKeyExpression = ContextKeyExpression(Rc::new(Expr::False));
}

fn initial_constants() -> HashMap<String, bool> {
    PLATFORM.with(|p| {
        let mut map = HashMap::new();
        map.insert("false".to_string(), false);
        map.insert("true".to_string(), true);
        map.insert("isMac".to_string(), p.is_macintosh);
        map.insert("isLinux".to_string(), p.is_linux);
        map.insert("isWindows".to_string(), p.is_windows);
        map.insert("isChromeOS".to_string(), p.is_chrome_os);
        map.insert("isWeb".to_string(), p.is_web);
        map.insert("isMacNative".to_string(), p.is_macintosh && !p.is_web);
        map.insert("isEdge".to_string(), p.is_edge());
        map.insert("isFirefox".to_string(), p.is_firefox());
        map.insert("isChrome".to_string(), p.is_chrome());
        map.insert("isSafari".to_string(), p.is_safari());
        map
    })
}

fn constant_value(key: &str) -> Option<bool> {
    CONSTANT_VALUES.with(|c| c.borrow().get(key).copied())
}

fn is_windows() -> bool {
    PLATFORM.with(|p| p.is_windows)
}

/// `setConstant`: registra uma constante só conhecida depois da partida (exige rodar `substitute_constants`).
pub fn set_constant(key: &str, value: bool) {
    if constant_value(key).is_some() {
        errors::throw(Error::new("contextkey.setConstant(k, v) invoked with already set constant `k`"));
    }
    CONSTANT_VALUES.with(|c| c.borrow_mut().insert(key.to_string(), value));
}

/// `ContextKeyExprType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextKeyExprType {
    False = 0,
    True = 1,
    Defined = 2,
    Not = 3,
    Equals = 4,
    NotEquals = 5,
    And = 6,
    Regex = 7,
    NotRegex = 8,
    Or = 9,
    In = 10,
    NotIn = 11,
    Greater = 12,
    GreaterEquals = 13,
    Smaller = 14,
    SmallerEquals = 15,
}

/// O valor de `>`, `>=`, `<`, `<=`: `number | string`.
#[derive(Clone, Debug, PartialEq)]
pub enum NumOrStr {
    Num(f64),
    Str(String),
}

impl NumOrStr {
    fn to_dynamic(&self) -> Dynamic {
        match self {
            NumOrStr::Num(n) => Dynamic::Number(*n),
            NumOrStr::Str(s) => Dynamic::string(s),
        }
    }

    fn serialize(&self) -> String {
        match self {
            NumOrStr::Num(n) => number_to_js_string(*n),
            NumOrStr::Str(s) => s.clone(),
        }
    }
}

/// `RegExp` do contexto: o padrão e as flags como o JavaScript os expõe (`source` escapado, `flags`
/// em ordem canônica) sobre o motor do `zyarr`.
#[derive(Clone)]
pub struct ContextRegExp {
    source: JsString,
    flags: String,
    compiled: Rc<RegExp>,
}

/// `EscapeRegExpPattern` do V8: `/` fora de classe, quebras de linha, e `(?:)` para o padrão vazio.
fn escape_regexp_source(pattern: &str) -> String {
    if pattern.is_empty() {
        return "(?:)".to_string();
    }
    let mut out = String::new();
    let mut in_escape = false;
    let mut in_class = false;
    for c in pattern.chars() {
        if in_escape {
            // Depois de uma barra invertida o caractere segue como está, salvo quebra de linha.
            match c {
                '\n' => out.push('n'),
                '\r' => out.push('r'),
                '\u{2028}' => out.push_str("u2028"),
                '\u{2029}' => out.push_str("u2029"),
                _ => out.push(c),
            }
            in_escape = false;
            continue;
        }
        match c {
            '\\' => {
                in_escape = true;
                out.push(c);
            }
            '/' if !in_class => out.push_str("\\/"),
            '[' => {
                in_class = true;
                out.push(c);
            }
            ']' => {
                in_class = false;
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(c),
        }
    }
    out
}

impl ContextRegExp {
    /// `new RegExp(pattern, flags)`.
    pub fn new(pattern: &str, flags: &str) -> Result<Self, RegExpError> {
        let compiled = RegExp::from_str(pattern, flags)?;
        Ok(Self {
            source: JsString::from(escape_regexp_source(pattern)),
            flags: compiled.flags().to_flags_string(),
            compiled: Rc::new(compiled),
        })
    }

    /// `regexp.source`.
    pub fn source(&self) -> &JsString {
        &self.source
    }

    /// `regexp.flags`.
    pub fn flags(&self) -> &str {
        &self.flags
    }

    /// `regexp.test(text)`.
    pub fn test(&self, text: &[u16]) -> bool {
        self.compiled.exec(text, 0).is_some()
    }
}

impl fmt::Debug for ContextRegExp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{}/{}", self.source.to_string_lossy(), self.flags)
    }
}

/// `IContext`: de onde o `evaluate` lê os valores.
pub trait IContext {
    fn get_value(&self, key: &str) -> Dynamic;
}

/// `IContextKeyExprMapper`.
pub trait ContextKeyExprMapper {
    fn map_defined(&self, key: &str) -> ContextKeyExpression;
    fn map_not(&self, key: &str) -> ContextKeyExpression;
    fn map_equals(&self, key: &str, value: &Dynamic) -> ContextKeyExpression;
    fn map_not_equals(&self, key: &str, value: &Dynamic) -> ContextKeyExpression;
    fn map_greater(&self, key: &str, value: &NumOrStr) -> ContextKeyExpression;
    fn map_greater_equals(&self, key: &str, value: &NumOrStr) -> ContextKeyExpression;
    fn map_smaller(&self, key: &str, value: &NumOrStr) -> ContextKeyExpression;
    fn map_smaller_equals(&self, key: &str, value: &NumOrStr) -> ContextKeyExpression;
    fn map_regex(&self, key: &str, regexp: Option<&ContextRegExp>) -> ContextKeyExpression;
    fn map_in(&self, key: &str, value_key: &str) -> ContextKeyExpression;
    fn map_not_in(&self, key: &str, value_key: &str) -> ContextKeyExpression;
}

/// A negação já calculada de um `And`/`Or`.
#[derive(Clone)]
#[doc(hidden)]
pub enum Negated {
    Unknown,
    Cached(ContextKeyExpression),
    /// A expressão original da qual esta é a negação (ligação fraca: não forma ciclo de `Rc`).
    Original(Weak<Expr>),
}

/// Os nós da árvore. Construa por [`ContextKeyExpr`] (que normaliza), nunca direto.
pub enum Expr {
    False,
    True,
    Defined { key: String },
    Not { key: String },
    Equals { key: String, value: Dynamic },
    NotEquals { key: String, value: Dynamic },
    Regex { key: String, regexp: Option<ContextRegExp> },
    NotRegex(ContextKeyExpression),
    And { expr: Vec<ContextKeyExpression>, negated: RefCell<Negated> },
    Or { expr: Vec<ContextKeyExpression>, negated: RefCell<Negated> },
    In { key: String, value_key: String },
    NotIn { key: String, value_key: String },
    Greater { key: String, value: NumOrStr },
    GreaterEquals { key: String, value: NumOrStr },
    Smaller { key: String, value: NumOrStr },
    SmallerEquals { key: String, value: NumOrStr },
}

/// `ContextKeyExpression`: uma expressão imutável e compartilhável.
#[derive(Clone)]
pub struct ContextKeyExpression(Rc<Expr>);

impl std::ops::Deref for ContextKeyExpression {
    type Target = Expr;
    fn deref(&self) -> &Expr {
        &self.0
    }
}

impl fmt::Debug for ContextKeyExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContextKeyExpression({})", self.serialize())
    }
}

/// `a < b` e `a > b` do JavaScript sobre texto: unidades UTF-16.
fn cmp1(key1: &str, key2: &str) -> i32 {
    match key1.encode_utf16().cmp(key2.encode_utf16()) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn cmp2(key1: &str, value1: &Dynamic, key2: &str, value2: &Dynamic) -> i32 {
    let by_key = cmp1(key1, key2);
    if by_key != 0 {
        return by_key;
    }
    if less_than(value1, value2) {
        return -1;
    }
    if less_than(value2, value1) {
        return 1;
    }
    0
}

fn cmp_expr(a: &ContextKeyExpression, b: &ContextKeyExpression) -> i32 {
    a.cmp(b)
}

impl ContextKeyExpression {
    fn new(expr: Expr) -> Self {
        Self(Rc::new(expr))
    }

    fn true_instance() -> Self {
        TRUE_INSTANCE.with(Clone::clone)
    }

    fn false_instance() -> Self {
        FALSE_INSTANCE.with(Clone::clone)
    }

    /// A identidade do `Rc` (o `===` do original).
    pub fn same_instance(&self, other: &ContextKeyExpression) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// O `type` do original.
    pub fn expr_type(&self) -> ContextKeyExprType {
        match &*self.0 {
            Expr::False => ContextKeyExprType::False,
            Expr::True => ContextKeyExprType::True,
            Expr::Defined { .. } => ContextKeyExprType::Defined,
            Expr::Not { .. } => ContextKeyExprType::Not,
            Expr::Equals { .. } => ContextKeyExprType::Equals,
            Expr::NotEquals { .. } => ContextKeyExprType::NotEquals,
            Expr::Regex { .. } => ContextKeyExprType::Regex,
            Expr::NotRegex(_) => ContextKeyExprType::NotRegex,
            Expr::And { .. } => ContextKeyExprType::And,
            Expr::Or { .. } => ContextKeyExprType::Or,
            Expr::In { .. } => ContextKeyExprType::In,
            Expr::NotIn { .. } => ContextKeyExprType::NotIn,
            Expr::Greater { .. } => ContextKeyExprType::Greater,
            Expr::GreaterEquals { .. } => ContextKeyExprType::GreaterEquals,
            Expr::Smaller { .. } => ContextKeyExprType::Smaller,
            Expr::SmallerEquals { .. } => ContextKeyExprType::SmallerEquals,
        }
    }

    /// Os termos de um `And`/`Or` (vazio nos outros).
    pub fn terms(&self) -> &[ContextKeyExpression] {
        match &*self.0 {
            Expr::And { expr, .. } | Expr::Or { expr, .. } => expr,
            _ => &[],
        }
    }

    /// A chave de uma expressão de uma chave só (`Defined`, `Not`, `==`, `!=`, `=~`, comparações, `in`, `not in`).
    pub fn key(&self) -> Option<&str> {
        match &*self.0 {
            Expr::Defined { key }
            | Expr::Not { key }
            | Expr::Equals { key, .. }
            | Expr::NotEquals { key, .. }
            | Expr::Regex { key, .. }
            | Expr::In { key, .. }
            | Expr::NotIn { key, .. }
            | Expr::Greater { key, .. }
            | Expr::GreaterEquals { key, .. }
            | Expr::Smaller { key, .. }
            | Expr::SmallerEquals { key, .. } => Some(key),
            Expr::NotRegex(actual) => actual.key(),
            _ => None,
        }
    }

    /// `cmp`: a ordem total que a normalização usa.
    #[allow(clippy::should_implement_trait)] // devolve `i32` como o `cmp` do original, não `Ordering`
    pub fn cmp(&self, other: &ContextKeyExpression) -> i32 {
        let (a, b) = (self.expr_type(), other.expr_type());
        if a != b {
            return a as i32 - b as i32;
        }
        match (&*self.0, &*other.0) {
            (Expr::Defined { key: k1 }, Expr::Defined { key: k2 }) | (Expr::Not { key: k1 }, Expr::Not { key: k2 }) => cmp1(k1, k2),
            (Expr::Equals { key: k1, value: v1 }, Expr::Equals { key: k2, value: v2 })
            | (Expr::NotEquals { key: k1, value: v1 }, Expr::NotEquals { key: k2, value: v2 }) => cmp2(k1, v1, k2, v2),
            (Expr::In { key: k1, value_key: v1 }, Expr::In { key: k2, value_key: v2 })
            | (Expr::NotIn { key: k1, value_key: v1 }, Expr::NotIn { key: k2, value_key: v2 }) => {
                cmp2(k1, &Dynamic::string(v1), k2, &Dynamic::string(v2))
            }
            (Expr::Greater { key: k1, value: v1 }, Expr::Greater { key: k2, value: v2 })
            | (Expr::GreaterEquals { key: k1, value: v1 }, Expr::GreaterEquals { key: k2, value: v2 })
            | (Expr::Smaller { key: k1, value: v1 }, Expr::Smaller { key: k2, value: v2 })
            | (Expr::SmallerEquals { key: k1, value: v1 }, Expr::SmallerEquals { key: k2, value: v2 }) => {
                cmp2(k1, &v1.to_dynamic(), k2, &v2.to_dynamic())
            }
            (Expr::Regex { key: k1, regexp: r1 }, Expr::Regex { key: k2, regexp: r2 }) => {
                let by_key = cmp1(k1, k2);
                if by_key != 0 {
                    return by_key;
                }
                let s1 = r1.as_ref().map(|r| r.source.units()).unwrap_or_default();
                let s2 = r2.as_ref().map(|r| r.source.units()).unwrap_or_default();
                match s1.cmp(s2) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                }
            }
            (Expr::NotRegex(a), Expr::NotRegex(b)) => a.cmp(b),
            (Expr::And { expr: e1, .. }, Expr::And { expr: e2, .. }) | (Expr::Or { expr: e1, .. }, Expr::Or { expr: e2, .. }) => {
                if e1.len() < e2.len() {
                    return -1;
                }
                if e1.len() > e2.len() {
                    return 1;
                }
                for (x, y) in e1.iter().zip(e2.iter()) {
                    let r = cmp_expr(x, y);
                    if r != 0 {
                        return r;
                    }
                }
                0
            }
            // `False` e `True`: o mesmo tipo é igual.
            _ => 0,
        }
    }

    /// `equals`.
    pub fn equals(&self, other: &ContextKeyExpression) -> bool {
        if self.expr_type() != other.expr_type() {
            return false;
        }
        match (&*self.0, &*other.0) {
            (Expr::Defined { key: k1 }, Expr::Defined { key: k2 }) | (Expr::Not { key: k1 }, Expr::Not { key: k2 }) => k1 == k2,
            (Expr::Equals { key: k1, value: v1 }, Expr::Equals { key: k2, value: v2 })
            | (Expr::NotEquals { key: k1, value: v1 }, Expr::NotEquals { key: k2, value: v2 }) => k1 == k2 && v1.strict_equals(v2),
            (Expr::In { key: k1, value_key: v1 }, Expr::In { key: k2, value_key: v2 })
            | (Expr::NotIn { key: k1, value_key: v1 }, Expr::NotIn { key: k2, value_key: v2 }) => k1 == k2 && v1 == v2,
            (Expr::Greater { key: k1, value: v1 }, Expr::Greater { key: k2, value: v2 })
            | (Expr::GreaterEquals { key: k1, value: v1 }, Expr::GreaterEquals { key: k2, value: v2 })
            | (Expr::Smaller { key: k1, value: v1 }, Expr::Smaller { key: k2, value: v2 })
            | (Expr::SmallerEquals { key: k1, value: v1 }, Expr::SmallerEquals { key: k2, value: v2 }) => k1 == k2 && v1 == v2,
            (Expr::Regex { key: k1, regexp: r1 }, Expr::Regex { key: k2, regexp: r2 }) => {
                let s1 = r1.as_ref().map(|r| r.source.units()).unwrap_or_default();
                let s2 = r2.as_ref().map(|r| r.source.units()).unwrap_or_default();
                k1 == k2 && s1 == s2
            }
            (Expr::NotRegex(a), Expr::NotRegex(b)) => a.equals(b),
            (Expr::And { expr: e1, .. }, Expr::And { expr: e2, .. }) | (Expr::Or { expr: e1, .. }, Expr::Or { expr: e2, .. }) => {
                e1.len() == e2.len() && e1.iter().zip(e2.iter()).all(|(a, b)| a.equals(b))
            }
            _ => true,
        }
    }

    /// `substituteConstants`: troca as chaves constantes (`isMac`...) por `true`/`false`.
    pub fn substitute_constants(&self) -> Option<ContextKeyExpression> {
        match &*self.0 {
            Expr::Defined { key } => Some(match constant_value(key) {
                Some(true) => Self::true_instance(),
                Some(false) => Self::false_instance(),
                None => self.clone(),
            }),
            Expr::Not { key } => Some(match constant_value(key) {
                Some(true) => Self::false_instance(),
                Some(false) => Self::true_instance(),
                None => self.clone(),
            }),
            Expr::Equals { key, value } => Some(match constant_value(key) {
                Some(constant) => {
                    let true_value = if constant { "true" } else { "false" };
                    if value.strict_equals(&Dynamic::string(true_value)) { Self::true_instance() } else { Self::false_instance() }
                }
                None => self.clone(),
            }),
            Expr::NotEquals { key, value } => Some(match constant_value(key) {
                Some(constant) => {
                    let false_value = if constant { "true" } else { "false" };
                    if value.strict_equals(&Dynamic::string(false_value)) { Self::false_instance() } else { Self::true_instance() }
                }
                None => self.clone(),
            }),
            Expr::And { expr, negated } => {
                let items = eliminate_constants_in_array(expr);
                match items {
                    None => Some(self.clone()),
                    Some(items) => ContextKeyAndExpr::create(&items, negated.borrow().clone(), false),
                }
            }
            Expr::Or { expr, negated } => {
                let items = eliminate_constants_in_array(expr);
                match items {
                    None => Some(self.clone()),
                    Some(items) => ContextKeyOrExpr::create(&items, negated.borrow().clone(), false),
                }
            }
            _ => Some(self.clone()),
        }
    }

    /// `evaluate`.
    pub fn evaluate(&self, context: &dyn IContext) -> bool {
        match &*self.0 {
            Expr::False => false,
            Expr::True => true,
            Expr::Defined { key } => context.get_value(key).is_truthy(),
            Expr::Not { key } => !context.get_value(key).is_truthy(),
            Expr::Equals { key, value } => crate::js::loose_equals(&context.get_value(key), value),
            Expr::NotEquals { key, value } => !crate::js::loose_equals(&context.get_value(key), value),
            Expr::Regex { key, regexp } => match regexp {
                Some(regexp) => regexp.test(to_js_string(&context.get_value(key)).units()),
                None => false,
            },
            Expr::NotRegex(actual) => !actual.evaluate(context),
            Expr::And { expr, .. } => expr.iter().all(|e| e.evaluate(context)),
            Expr::Or { expr, .. } => expr.iter().any(|e| e.evaluate(context)),
            Expr::In { key, value_key } => evaluate_in(context, key, value_key),
            Expr::NotIn { key, value_key } => !evaluate_in(context, key, value_key),
            Expr::Greater { key, value } => compare_number(context, key, value, |a, b| a > b),
            Expr::GreaterEquals { key, value } => compare_number(context, key, value, |a, b| a >= b),
            Expr::Smaller { key, value } => compare_number(context, key, value, |a, b| a < b),
            Expr::SmallerEquals { key, value } => compare_number(context, key, value, |a, b| a <= b),
        }
    }

    /// `serialize`.
    pub fn serialize(&self) -> String {
        match &*self.0 {
            Expr::False => "false".to_string(),
            Expr::True => "true".to_string(),
            Expr::Defined { key } => key.clone(),
            Expr::Not { key } => format!("!{key}"),
            Expr::Equals { key, value } => format!("{key} == '{}'", to_js_string(value).to_string_lossy()),
            Expr::NotEquals { key, value } => format!("{key} != '{}'", to_js_string(value).to_string_lossy()),
            Expr::Regex { key, regexp } => {
                let value = match regexp {
                    Some(r) => format!("/{}/{}", r.source.to_string_lossy(), r.flags),
                    None => "/invalid/".to_string(),
                };
                format!("{key} =~ {value}")
            }
            Expr::NotRegex(actual) => format!("!({})", actual.serialize()),
            Expr::And { expr, .. } => expr.iter().map(ContextKeyExpression::serialize).collect::<Vec<_>>().join(" && "),
            Expr::Or { expr, .. } => expr.iter().map(ContextKeyExpression::serialize).collect::<Vec<_>>().join(" || "),
            Expr::In { key, value_key } => format!("{key} in '{value_key}'"),
            Expr::NotIn { key, value_key } => format!("{key} not in '{value_key}'"),
            Expr::Greater { key, value } => format!("{key} > {}", value.serialize()),
            Expr::GreaterEquals { key, value } => format!("{key} >= {}", value.serialize()),
            Expr::Smaller { key, value } => format!("{key} < {}", value.serialize()),
            Expr::SmallerEquals { key, value } => format!("{key} <= {}", value.serialize()),
        }
    }

    /// `keys`: as chaves que a expressão lê, com repetição.
    pub fn keys(&self) -> Vec<String> {
        match &*self.0 {
            Expr::False | Expr::True => Vec::new(),
            Expr::In { key, value_key } | Expr::NotIn { key, value_key } => vec![key.clone(), value_key.clone()],
            Expr::NotRegex(actual) => actual.keys(),
            Expr::And { expr, .. } | Expr::Or { expr, .. } => expr.iter().flat_map(ContextKeyExpression::keys).collect(),
            _ => vec![self.key().unwrap_or_default().to_string()],
        }
    }

    /// `map`: reconstrói a expressão por um mapeador (as folhas passam pelo mapeador; `And`/`Or` não renormalizam).
    pub fn map(&self, mapper: &dyn ContextKeyExprMapper) -> ContextKeyExpression {
        match &*self.0 {
            Expr::False | Expr::True => self.clone(),
            Expr::Defined { key } => mapper.map_defined(key),
            Expr::Not { key } => mapper.map_not(key),
            Expr::Equals { key, value } => mapper.map_equals(key, value),
            Expr::NotEquals { key, value } => mapper.map_not_equals(key, value),
            Expr::Regex { key, regexp } => mapper.map_regex(key, regexp.as_ref()),
            Expr::NotRegex(actual) => ContextKeyExpression::new(Expr::NotRegex(actual.map(mapper))),
            Expr::And { expr, .. } => ContextKeyExpression::new(Expr::And {
                expr: expr.iter().map(|e| e.map(mapper)).collect(),
                negated: RefCell::new(Negated::Unknown),
            }),
            Expr::Or { expr, .. } => ContextKeyExpression::new(Expr::Or {
                expr: expr.iter().map(|e| e.map(mapper)).collect(),
                negated: RefCell::new(Negated::Unknown),
            }),
            Expr::In { key, value_key } => mapper.map_in(key, value_key),
            Expr::NotIn { key, value_key } => mapper.map_not_in(key, value_key),
            Expr::Greater { key, value } => mapper.map_greater(key, value),
            Expr::GreaterEquals { key, value } => mapper.map_greater_equals(key, value),
            Expr::Smaller { key, value } => mapper.map_smaller(key, value),
            Expr::SmallerEquals { key, value } => mapper.map_smaller_equals(key, value),
        }
    }

    /// `negate`.
    pub fn negate(&self) -> ContextKeyExpression {
        match &*self.0 {
            Expr::False => Self::true_instance(),
            Expr::True => Self::false_instance(),
            Expr::Defined { key } => ContextKeyNotExpr::create(key),
            Expr::Not { key } => ContextKeyDefinedExpr::create(key),
            Expr::Equals { key, value } => ContextKeyNotEqualsExpr::create(key, value.clone()),
            Expr::NotEquals { key, value } => ContextKeyEqualsExpr::create(key, value.clone()),
            Expr::Regex { .. } => ContextKeyExpression::new(Expr::NotRegex(self.clone())),
            Expr::NotRegex(actual) => actual.clone(),
            Expr::In { key, value_key } => ContextKeyNotInExpr::create(key, value_key),
            Expr::NotIn { key, value_key } => ContextKeyInExpr::create(key, value_key),
            Expr::Greater { key, value } => ContextKeyExpression::new(Expr::SmallerEquals { key: key.clone(), value: value.clone() }),
            Expr::GreaterEquals { key, value } => ContextKeyExpression::new(Expr::Smaller { key: key.clone(), value: value.clone() }),
            Expr::Smaller { key, value } => ContextKeyExpression::new(Expr::GreaterEquals { key: key.clone(), value: value.clone() }),
            Expr::SmallerEquals { key, value } => ContextKeyExpression::new(Expr::Greater { key: key.clone(), value: value.clone() }),
            Expr::And { expr, negated } => {
                if let Some(known) = known_negation(negated) {
                    return known;
                }
                let result: Vec<Option<ContextKeyExpression>> = expr.iter().map(|e| Some(e.negate())).collect();
                let negation = ContextKeyOrExpr::create(&result, Negated::Original(Rc::downgrade(&self.0)), true)
                    .expect("a negação de um And tem termos");
                *negated.borrow_mut() = Negated::Cached(negation.clone());
                negation
            }
            Expr::Or { expr, negated } => {
                if let Some(known) = known_negation(negated) {
                    return known;
                }
                let mut result: Vec<ContextKeyExpression> = expr.iter().map(ContextKeyExpression::negate).collect();
                // Não há parênteses: o AND se distribui sobre os termos do OR, sempre nos dois primeiros.
                while result.len() > 1 {
                    let left = result.remove(0);
                    let right = result.remove(0);
                    let mut all = Vec::new();
                    for l in get_terminals(&left) {
                        for r in get_terminals(&right) {
                            all.push(Some(ContextKeyAndExpr::create(&[Some(l.clone()), Some(r.clone())], Negated::Unknown, false).expect("dois termos")));
                        }
                    }
                    result.insert(0, ContextKeyOrExpr::create(&all, Negated::Unknown, false).expect("termos"));
                }
                let result: Vec<Option<ContextKeyExpression>> = result.into_iter().map(Some).collect();
                let negation = ContextKeyOrExpr::create(&result, Negated::Original(Rc::downgrade(&self.0)), true).expect("termos");
                *negated.borrow_mut() = Negated::Cached(negation.clone());
                negation
            }
        }
    }
}

fn known_negation(negated: &RefCell<Negated>) -> Option<ContextKeyExpression> {
    match &*negated.borrow() {
        Negated::Unknown => None,
        Negated::Cached(expr) => Some(expr.clone()),
        Negated::Original(weak) => weak.upgrade().map(ContextKeyExpression),
    }
}

fn get_terminals(node: &ContextKeyExpression) -> Vec<ContextKeyExpression> {
    match &*node.0 {
        Expr::Or { expr, .. } => expr.clone(),
        _ => vec![node.clone()],
    }
}

/// `eliminateConstantsInArray`: `None` se nada mudou.
fn eliminate_constants_in_array(arr: &[ContextKeyExpression]) -> Option<Vec<Option<ContextKeyExpression>>> {
    let mut new_arr: Option<Vec<Option<ContextKeyExpression>>> = None;
    for (i, item) in arr.iter().enumerate() {
        let new_expr = item.substitute_constants();
        let changed = !new_expr.as_ref().is_some_and(|n| n.same_instance(item));
        if changed && new_arr.is_none() {
            new_arr = Some(arr[..i].iter().cloned().map(Some).collect());
        }
        if let Some(out) = new_arr.as_mut() {
            out.push(new_expr);
        }
    }
    new_arr
}

fn evaluate_in(context: &dyn IContext, key: &str, value_key: &str) -> bool {
    let source = context.get_value(value_key);
    let item = context.get_value(key);
    if let Dynamic::Array(items) = &source {
        let items = items.to_vec();
        if items.contains(&item) {
            return true;
        }
        // No Windows os caminhos de arquivo não diferenciam caixa, então as URIs `file:///` também não.
        if is_windows()
            && let Dynamic::String(item) = &item
            && item.units().starts_with(&"file:///".encode_utf16().collect::<Vec<_>>())
        {
            let lower = item.to_lower_case();
            return items.iter().any(|s| matches!(s, Dynamic::String(s) if s.to_lower_case() == lower));
        }
        return false;
    }
    if let Dynamic::String(item) = &item
        && matches!(source, Dynamic::Object(_) | Dynamic::Date(_) | Dynamic::RegExp(_) | Dynamic::TypedArray(_))
    {
        if has_own_property(&source, item) {
            return true;
        }
        if is_windows() && item.units().starts_with(&"file:///".encode_utf16().collect::<Vec<_>>()) {
            let lower = item.to_lower_case();
            return source.own_keys().iter().any(|k| k.to_lower_case() == lower);
        }
        return false;
    }
    false
}

fn compare_number(context: &dyn IContext, key: &str, value: &NumOrStr, op: fn(f64, f64) -> bool) -> bool {
    match value {
        NumOrStr::Str(_) => false,
        NumOrStr::Num(n) => op(parse_float(to_js_string(&context.get_value(key)).units()), *n),
    }
}

/// `withFloatOrStr`: o valor vira número se for texto numérico; `None` (a expressão `false`) se não for
/// texto nem número.
fn with_float_or_str(value: &Dynamic) -> Option<NumOrStr> {
    match value {
        Dynamic::String(s) => {
            let n = parse_float(s.units());
            Some(if n.is_nan() { NumOrStr::Str(s.to_string_lossy()) } else { NumOrStr::Num(n) })
        }
        Dynamic::Number(n) => Some(NumOrStr::Num(*n)),
        _ => None,
    }
}

// As fábricas por tipo, com a normalização do original.

pub struct ContextKeyDefinedExpr;
impl ContextKeyDefinedExpr {
    pub fn create(key: &str) -> ContextKeyExpression {
        match constant_value(key) {
            Some(true) => ContextKeyExpression::true_instance(),
            Some(false) => ContextKeyExpression::false_instance(),
            None => ContextKeyExpression::new(Expr::Defined { key: key.to_string() }),
        }
    }
}

pub struct ContextKeyNotExpr;
impl ContextKeyNotExpr {
    pub fn create(key: &str) -> ContextKeyExpression {
        match constant_value(key) {
            Some(true) => ContextKeyExpression::false_instance(),
            Some(false) => ContextKeyExpression::true_instance(),
            None => ContextKeyExpression::new(Expr::Not { key: key.to_string() }),
        }
    }
}

pub struct ContextKeyEqualsExpr;
impl ContextKeyEqualsExpr {
    pub fn create(key: &str, value: Dynamic) -> ContextKeyExpression {
        if let Dynamic::Bool(b) = value {
            return if b { ContextKeyDefinedExpr::create(key) } else { ContextKeyNotExpr::create(key) };
        }
        if let Some(constant) = constant_value(key) {
            let true_value = if constant { "true" } else { "false" };
            return if value.strict_equals(&Dynamic::string(true_value)) {
                ContextKeyExpression::true_instance()
            } else {
                ContextKeyExpression::false_instance()
            };
        }
        ContextKeyExpression::new(Expr::Equals { key: key.to_string(), value })
    }
}

pub struct ContextKeyNotEqualsExpr;
impl ContextKeyNotEqualsExpr {
    pub fn create(key: &str, value: Dynamic) -> ContextKeyExpression {
        if let Dynamic::Bool(b) = value {
            return if b { ContextKeyNotExpr::create(key) } else { ContextKeyDefinedExpr::create(key) };
        }
        if let Some(constant) = constant_value(key) {
            let false_value = if constant { "true" } else { "false" };
            return if value.strict_equals(&Dynamic::string(false_value)) {
                ContextKeyExpression::false_instance()
            } else {
                ContextKeyExpression::true_instance()
            };
        }
        ContextKeyExpression::new(Expr::NotEquals { key: key.to_string(), value })
    }
}

pub struct ContextKeyInExpr;
impl ContextKeyInExpr {
    pub fn create(key: &str, value_key: &str) -> ContextKeyExpression {
        ContextKeyExpression::new(Expr::In { key: key.to_string(), value_key: value_key.to_string() })
    }
}

pub struct ContextKeyNotInExpr;
impl ContextKeyNotInExpr {
    pub fn create(key: &str, value_key: &str) -> ContextKeyExpression {
        ContextKeyExpression::new(Expr::NotIn { key: key.to_string(), value_key: value_key.to_string() })
    }
}

macro_rules! comparison_factory {
    ($name:ident, $variant:ident) => {
        pub struct $name;
        impl $name {
            pub fn create(key: &str, value: &Dynamic) -> ContextKeyExpression {
                match with_float_or_str(value) {
                    Some(value) => ContextKeyExpression::new(Expr::$variant { key: key.to_string(), value }),
                    None => ContextKeyExpression::false_instance(),
                }
            }
        }
    };
}
comparison_factory!(ContextKeyGreaterExpr, Greater);
comparison_factory!(ContextKeyGreaterEqualsExpr, GreaterEquals);
comparison_factory!(ContextKeySmallerExpr, Smaller);
comparison_factory!(ContextKeySmallerEqualsExpr, SmallerEquals);

pub struct ContextKeyRegexExpr;
impl ContextKeyRegexExpr {
    pub fn create(key: &str, regexp: Option<ContextRegExp>) -> ContextKeyExpression {
        ContextKeyExpression::new(Expr::Regex { key: key.to_string(), regexp })
    }
}

pub struct ContextKeyAndExpr;
impl ContextKeyAndExpr {
    fn create(items: &[Option<ContextKeyExpression>], negated: Negated, extra_redundant_check: bool) -> Option<ContextKeyExpression> {
        Self::normalize(items, negated, extra_redundant_check)
    }

    fn normalize(items: &[Option<ContextKeyExpression>], negated: Negated, extra_redundant_check: bool) -> Option<ContextKeyExpression> {
        let mut expr: Vec<ContextKeyExpression> = Vec::new();
        let mut has_true = false;
        for e in items.iter().flatten() {
            match e.expr_type() {
                // anything && true ==> anything
                ContextKeyExprType::True => {
                    has_true = true;
                    continue;
                }
                // anything && false ==> false
                ContextKeyExprType::False => return Some(ContextKeyExpression::false_instance()),
                ContextKeyExprType::And => {
                    expr.extend(e.terms().iter().cloned());
                    continue;
                }
                _ => expr.push(e.clone()),
            }
        }
        if expr.is_empty() && has_true {
            return Some(ContextKeyExpression::true_instance());
        }
        if expr.is_empty() {
            return None;
        }
        if expr.len() == 1 {
            return expr.pop();
        }
        sort_exprs(&mut expr);
        // eliminate duplicate terms
        let mut i = 1;
        while i < expr.len() {
            if expr[i - 1].equals(&expr[i]) {
                expr.remove(i);
            } else {
                i += 1;
            }
        }
        if expr.len() == 1 {
            return expr.pop();
        }
        // Não há parênteses: todo OR é distribuído. Os OR ficam no fim (pelas regras de ordenação).
        while expr.len() > 1 {
            let last = expr[expr.len() - 1].clone();
            if last.expr_type() != ContextKeyExprType::Or {
                break;
            }
            expr.pop();
            let second_to_last = expr.pop().expect("havia ao menos dois termos");
            let is_finished = expr.is_empty();
            let distributed: Vec<Option<ContextKeyExpression>> = last
                .terms()
                .iter()
                .map(|el| Self::create(&[Some(el.clone()), Some(second_to_last.clone())], Negated::Unknown, extra_redundant_check))
                .collect();
            if let Some(result) = ContextKeyOrExpr::create(&distributed, Negated::Unknown, is_finished) {
                expr.push(result);
                sort_exprs(&mut expr);
            }
        }
        if expr.len() == 1 {
            return expr.pop();
        }
        // resolve false AND expressions
        if extra_redundant_check {
            for i in 0..expr.len() {
                for j in (i + 1)..expr.len() {
                    if expr[i].negate().equals(&expr[j]) {
                        // A && !A
                        return Some(ContextKeyExpression::false_instance());
                    }
                }
            }
            if expr.len() == 1 {
                return expr.pop();
            }
        }
        Some(ContextKeyExpression::new(Expr::And { expr, negated: RefCell::new(negated) }))
    }
}

pub struct ContextKeyOrExpr;
impl ContextKeyOrExpr {
    fn create(items: &[Option<ContextKeyExpression>], negated: Negated, extra_redundant_check: bool) -> Option<ContextKeyExpression> {
        let mut expr: Vec<ContextKeyExpression> = Vec::new();
        let mut has_false = false;
        for e in items.iter().flatten() {
            match e.expr_type() {
                // anything || false ==> anything
                ContextKeyExprType::False => {
                    has_false = true;
                    continue;
                }
                // anything || true ==> true
                ContextKeyExprType::True => return Some(ContextKeyExpression::true_instance()),
                ContextKeyExprType::Or => {
                    expr.extend(e.terms().iter().cloned());
                    continue;
                }
                _ => expr.push(e.clone()),
            }
        }
        if expr.is_empty() && has_false {
            return Some(ContextKeyExpression::false_instance());
        }
        sort_exprs(&mut expr);
        if expr.is_empty() {
            return None;
        }
        if expr.len() == 1 {
            return expr.pop();
        }
        let mut i = 1;
        while i < expr.len() {
            if expr[i - 1].equals(&expr[i]) {
                expr.remove(i);
            } else {
                i += 1;
            }
        }
        if expr.len() == 1 {
            return expr.pop();
        }
        // resolve true OR expressions
        if extra_redundant_check {
            for i in 0..expr.len() {
                for j in (i + 1)..expr.len() {
                    if expr[i].negate().equals(&expr[j]) {
                        // A || !A
                        return Some(ContextKeyExpression::true_instance());
                    }
                }
            }
            if expr.len() == 1 {
                return expr.pop();
            }
        }
        Some(ContextKeyExpression::new(Expr::Or { expr, negated: RefCell::new(negated) }))
    }
}

/// `expr.sort(cmp)`: o `sort` do JavaScript é estável.
fn sort_exprs(expr: &mut [ContextKeyExpression]) {
    expr.sort_by(|a, b| a.cmp(b).cmp(&0));
}

/// `ContextKeyExpr`: as fábricas públicas.
pub struct ContextKeyExpr;

impl ContextKeyExpr {
    pub fn false_() -> ContextKeyExpression {
        ContextKeyExpression::false_instance()
    }

    pub fn true_() -> ContextKeyExpression {
        ContextKeyExpression::true_instance()
    }

    pub fn has(key: &str) -> ContextKeyExpression {
        ContextKeyDefinedExpr::create(key)
    }

    pub fn equals(key: &str, value: impl Into<Dynamic>) -> ContextKeyExpression {
        ContextKeyEqualsExpr::create(key, value.into())
    }

    pub fn not_equals(key: &str, value: impl Into<Dynamic>) -> ContextKeyExpression {
        ContextKeyNotEqualsExpr::create(key, value.into())
    }

    pub fn regex(key: &str, value: ContextRegExp) -> ContextKeyExpression {
        ContextKeyRegexExpr::create(key, Some(value))
    }

    pub fn in_(key: &str, value: &str) -> ContextKeyExpression {
        ContextKeyInExpr::create(key, value)
    }

    pub fn not_in(key: &str, value: &str) -> ContextKeyExpression {
        ContextKeyNotInExpr::create(key, value)
    }

    pub fn not(key: &str) -> ContextKeyExpression {
        ContextKeyNotExpr::create(key)
    }

    /// `and(...expr)`: `None` quando não sobra termo nenhum.
    pub fn and<I>(items: I) -> Option<ContextKeyExpression>
    where
        I: IntoIterator,
        I::Item: Into<Option<ContextKeyExpression>>,
    {
        let items: Vec<Option<ContextKeyExpression>> = items.into_iter().map(Into::into).collect();
        ContextKeyAndExpr::create(&items, Negated::Unknown, true)
    }

    /// `or(...expr)`.
    pub fn or<I>(items: I) -> Option<ContextKeyExpression>
    where
        I: IntoIterator,
        I::Item: Into<Option<ContextKeyExpression>>,
    {
        let items: Vec<Option<ContextKeyExpression>> = items.into_iter().map(Into::into).collect();
        ContextKeyOrExpr::create(&items, Negated::Unknown, true)
    }

    pub fn greater(key: &str, value: impl Into<Dynamic>) -> ContextKeyExpression {
        ContextKeyGreaterExpr::create(key, &value.into())
    }

    pub fn greater_equals(key: &str, value: impl Into<Dynamic>) -> ContextKeyExpression {
        ContextKeyGreaterEqualsExpr::create(key, &value.into())
    }

    pub fn smaller(key: &str, value: impl Into<Dynamic>) -> ContextKeyExpression {
        ContextKeySmallerExpr::create(key, &value.into())
    }

    pub fn smaller_equals(key: &str, value: impl Into<Dynamic>) -> ContextKeyExpression {
        ContextKeySmallerEqualsExpr::create(key, &value.into())
    }

    /// `deserialize`: `None` para `None`, para erro de sintaxe e para a expressão vazia.
    pub fn deserialize(serialized: Option<&str>) -> Option<ContextKeyExpression> {
        let serialized = serialized?;
        DESERIALIZER.with(|parser| parser.borrow_mut().parse(serialized))
    }
}

thread_local! {
    static DESERIALIZER: RefCell<Parser> = RefCell::new(Parser::new(ParserConfig { regex_parsing_with_error_recovery: false }));
}

/// `implies`: verdadeiro quando `p` implica `q` de forma demonstrável.
pub fn implies(p: &ContextKeyExpression, q: &ContextKeyExpression) -> bool {
    use ContextKeyExprType as T;
    if p.expr_type() == T::False || q.expr_type() == T::True {
        // false implies anything; anything implies true
        return true;
    }
    if p.expr_type() == T::Or {
        if q.expr_type() == T::Or {
            // `a || b || c` só implica algo como `a || b || c || d`
            return all_elements_included(p.terms(), q.terms());
        }
        return false;
    }
    if q.expr_type() == T::Or {
        return q.terms().iter().any(|element| implies(p, element));
    }
    if p.expr_type() == T::And {
        if q.expr_type() == T::And {
            // `a && b && c` implica `a && c`
            return all_elements_included(q.terms(), p.terms());
        }
        return p.terms().iter().any(|element| implies(element, q));
    }
    p.equals(q)
}

/// Verdadeiro se todo elemento de `p` está em `q`; os dois estão ordenados.
fn all_elements_included(p: &[ContextKeyExpression], q: &[ContextKeyExpression]) -> bool {
    let (mut pi, mut qi) = (0, 0);
    while pi < p.len() && qi < q.len() {
        let cmp = p[pi].cmp(&q[qi]);
        if cmp < 0 {
            return false;
        } else if cmp == 0 {
            pi += 1;
            qi += 1;
        } else {
            qi += 1;
        }
    }
    pi == p.len()
}

/// `expressionsAreEqualWithConstantSubstitution`.
pub fn expressions_are_equal_with_constant_substitution(a: Option<&ContextKeyExpression>, b: Option<&ContextKeyExpression>) -> bool {
    let a_expr = a.and_then(ContextKeyExpression::substitute_constants);
    let b_expr = b.and_then(ContextKeyExpression::substitute_constants);
    match (a_expr, b_expr) {
        (None, None) => true,
        (Some(a), Some(b)) => a.equals(&b),
        _ => false,
    }
}

/// `ParserConfig`.
#[derive(Clone, Copy, Debug)]
pub struct ParserConfig {
    /// Com a recuperação ligada o parser aceita barras sem escape: `/src//` vale como `/src\//`.
    pub regex_parsing_with_error_recovery: bool,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self { regex_parsing_with_error_recovery: true }
    }
}

/// `ParsingError`.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsingError {
    pub message: String,
    pub offset: usize,
    pub lexeme: String,
    pub additional_info: Option<String>,
}

/// O `throw Parser._parseError` do original.
struct ParseAbort;

type ParseResult<T> = Result<T, ParseAbort>;

/// `Parser`.
pub struct Parser {
    config: ParserConfig,
    scanner: Scanner,
    tokens: Vec<Token>,
    current: usize,
    parsing_errors: Vec<ParsingError>,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new(ParserConfig::default())
    }
}

impl Parser {
    pub fn new(config: ParserConfig) -> Self {
        Self { config, scanner: Scanner::new(), tokens: Vec::new(), current: 0, parsing_errors: Vec::new() }
    }

    pub fn lexing_errors(&self) -> &[LexingError] {
        self.scanner.errors()
    }

    pub fn parsing_errors(&self) -> &[ParsingError] {
        &self.parsing_errors
    }

    /// `parse`: `None` se houve erro (veja `lexing_errors` e `parsing_errors`).
    pub fn parse(&mut self, input: &str) -> Option<ContextKeyExpression> {
        if input.is_empty() {
            self.parsing_errors.push(ParsingError {
                message: localize("contextkey.parser.error.emptyString", "Empty context key expression", &[]),
                offset: 0,
                lexeme: String::new(),
                additional_info: Some(localize(
                    "contextkey.parser.error.emptyString.hint",
                    "Did you forget to write an expression? You can also put 'false' or 'true' to always evaluate to false or true, respectively.",
                    &[],
                )),
            });
            return None;
        }

        self.tokens = self.scanner.reset(input).scan();
        // Não para nos erros léxicos, para reconstruir regex com barras sem escape.
        self.current = 0;
        self.parsing_errors = Vec::new();

        let result = self.expr().and_then(|expr| {
            if !self.is_at_end() {
                let peek = self.peek().clone();
                let additional_info = (peek.ty == TokenType::Str).then(|| {
                    localize("contextkey.parser.error.unexpectedToken.hint", "Did you forget to put && or || before the token?", &[])
                });
                self.parsing_errors.push(ParsingError {
                    message: localize("contextkey.parser.error.unexpectedToken", "Unexpected token", &[]),
                    offset: peek.offset,
                    lexeme: get_lexeme(&peek),
                    additional_info,
                });
                return Err(ParseAbort);
            }
            Ok(expr)
        });
        result.unwrap_or(None)
    }

    fn expr(&mut self) -> ParseResult<Option<ContextKeyExpression>> {
        self.or()
    }

    fn or(&mut self) -> ParseResult<Option<ContextKeyExpression>> {
        let mut expr = vec![self.and()?];
        while self.match_one(TokenType::Or) {
            expr.push(self.and()?);
        }
        Ok(if expr.len() == 1 { expr.remove(0) } else { ContextKeyExpr::or(expr) })
    }

    fn and(&mut self) -> ParseResult<Option<ContextKeyExpression>> {
        let mut expr = vec![self.term()?];
        while self.match_one(TokenType::And) {
            expr.push(self.term()?);
        }
        Ok(if expr.len() == 1 { expr.remove(0) } else { ContextKeyExpr::and(expr) })
    }

    fn term(&mut self) -> ParseResult<Option<ContextKeyExpression>> {
        if self.match_one(TokenType::Neg) {
            let peek = self.peek().clone();
            return match peek.ty {
                TokenType::True => {
                    self.advance();
                    Ok(Some(ContextKeyExpression::false_instance()))
                }
                TokenType::False => {
                    self.advance();
                    Ok(Some(ContextKeyExpression::true_instance()))
                }
                TokenType::LParen => {
                    self.advance();
                    let expr = self.expr()?;
                    self.consume(TokenType::RParen, &localize("contextkey.parser.error.closingParenthesis", "closing parenthesis ')'", &[]))?;
                    Ok(expr.map(|e| e.negate()))
                }
                TokenType::Str => {
                    self.advance();
                    Ok(Some(ContextKeyNotExpr::create(peek.lexeme())))
                }
                _ => Err(self.err_expected_but_got("KEY | true | false | '(' expression ')'", &peek, None)),
            };
        }
        self.primary()
    }

    fn primary(&mut self) -> ParseResult<Option<ContextKeyExpression>> {
        let peek = self.peek().clone();
        match peek.ty {
            TokenType::True => {
                self.advance();
                Ok(Some(ContextKeyExpression::true_instance()))
            }
            TokenType::False => {
                self.advance();
                Ok(Some(ContextKeyExpression::false_instance()))
            }
            TokenType::LParen => {
                self.advance();
                let expr = self.expr()?;
                self.consume(TokenType::RParen, &localize("contextkey.parser.error.closingParenthesis", "closing parenthesis ')'", &[]))?;
                Ok(expr)
            }
            TokenType::Str => {
                let key = peek.lexeme().to_string();
                self.advance();
                // =~ regex
                if self.match_one(TokenType::RegexOp) {
                    return self.regex_expr(&key).map(Some);
                }
                // [ 'not' 'in' value ]
                if self.match_one(TokenType::Not) {
                    self.consume(TokenType::In, &localize("contextkey.parser.error.noInAfterNot", "'in' after 'not'.", &[]))?;
                    let right = self.value();
                    return Ok(Some(ContextKeyNotInExpr::create(&key, &right)));
                }
                // [ ('==' | '!=' | '<' | '<=' | '>' | '>=' | 'in') value ]
                let expr = match self.peek().ty {
                    TokenType::Eq => {
                        self.advance();
                        let right = self.value();
                        if self.previous().ty == TokenType::QuotedStr {
                            // "foo == 'true'" se preserva, mas "foo == true" vira "foo"
                            ContextKeyExpr::equals(&key, right.as_str())
                        } else {
                            match right.as_str() {
                                "true" => ContextKeyExpr::has(&key),
                                "false" => ContextKeyExpr::not(&key),
                                _ => ContextKeyExpr::equals(&key, right.as_str()),
                            }
                        }
                    }
                    TokenType::NotEq => {
                        self.advance();
                        let right = self.value();
                        if self.previous().ty == TokenType::QuotedStr {
                            ContextKeyExpr::not_equals(&key, right.as_str())
                        } else {
                            match right.as_str() {
                                "true" => ContextKeyExpr::not(&key),
                                "false" => ContextKeyExpr::has(&key),
                                _ => ContextKeyExpr::not_equals(&key, right.as_str()),
                            }
                        }
                    }
                    TokenType::Lt => {
                        self.advance();
                        ContextKeySmallerExpr::create(&key, &Dynamic::string(&self.value()))
                    }
                    TokenType::LtEq => {
                        self.advance();
                        ContextKeySmallerEqualsExpr::create(&key, &Dynamic::string(&self.value()))
                    }
                    TokenType::Gt => {
                        self.advance();
                        ContextKeyGreaterExpr::create(&key, &Dynamic::string(&self.value()))
                    }
                    TokenType::GtEq => {
                        self.advance();
                        ContextKeyGreaterEqualsExpr::create(&key, &Dynamic::string(&self.value()))
                    }
                    TokenType::In => {
                        self.advance();
                        ContextKeyExpr::in_(&key, &self.value())
                    }
                    _ => ContextKeyExpr::has(&key),
                };
                Ok(Some(expr))
            }
            TokenType::Eof => {
                self.parsing_errors.push(ParsingError {
                    message: localize("contextkey.parser.error.unexpectedEOF", "Unexpected end of expression", &[]),
                    offset: peek.offset,
                    lexeme: String::new(),
                    additional_info: Some(localize("contextkey.parser.error.unexpectedEOF.hint", "Did you forget to put a context key?", &[])),
                });
                Err(ParseAbort)
            }
            _ => {
                let expected = "true | false | KEY \n\t| KEY '=~' REGEX \n\t| KEY ('==' | '!=' | '<' | '<=' | '>' | '>=' | 'in' | 'not' 'in') value";
                let token = self.peek().clone();
                Err(self.err_expected_but_got(expected, &token, None))
            }
        }
    }

    /// O que vem depois de `KEY =~`.
    fn regex_expr(&mut self, key: &str) -> ParseResult<ContextKeyExpression> {
        // As regex são reconstruídas dos tokens porque há extensões com barras sem escape.
        let expr = self.peek().clone();

        if !self.config.regex_parsing_with_error_recovery {
            self.advance();
            if expr.ty != TokenType::RegexStr {
                return Err(self.err_expected_but_got("REGEX", &expr, None));
            }
            return match build_regexp(expr.lexeme()) {
                Some(regexp) => Ok(ContextKeyRegexExpr::create(key, Some(regexp))),
                None => Err(self.err_expected_but_got("REGEX", &expr, None)),
            };
        }

        match expr.ty {
            // RegexStr ou Error (por exemplo `/(/file)/`)
            TokenType::RegexStr | TokenType::Error => {
                let mut reconstruction: Vec<String> = vec![expr.lexeme().to_string()];
                self.advance();

                let mut following = self.peek().clone();
                let expr_units: Vec<u16> = expr.lexeme().encode_utf16().collect();
                let mut paren_balance: i64 = 0;
                for unit in &expr_units {
                    if *unit == b'(' as u16 {
                        paren_balance += 1;
                    } else if *unit == b')' as u16 {
                        paren_balance -= 1;
                    }
                }

                while !self.is_at_end() && following.ty != TokenType::And && following.ty != TokenType::Or {
                    match following.ty {
                        TokenType::LParen => paren_balance += 1,
                        TokenType::RParen => paren_balance -= 1,
                        TokenType::RegexStr | TokenType::QuotedStr => {
                            // O original compara `followingToken.lexeme[i]` com `(` mas o `expr.lexeme[i]` com `)`.
                            let following_units: Vec<u16> = following.lexeme().encode_utf16().collect();
                            for (i, unit) in following_units.iter().enumerate() {
                                if *unit == b'(' as u16 {
                                    paren_balance += 1;
                                } else if expr_units.get(i) == Some(&(b')' as u16)) {
                                    paren_balance -= 1;
                                }
                            }
                        }
                        _ => {}
                    }
                    if paren_balance < 0 {
                        break;
                    }
                    reconstruction.push(get_lexeme(&following));
                    self.advance();
                    following = self.peek().clone();
                }

                let regex_lexeme = reconstruction.concat();
                match build_regexp(&regex_lexeme) {
                    Some(regexp) => Ok(ContextKeyExpr::regex(key, regexp)),
                    None => Err(self.err_expected_but_got("REGEX", &expr, None)),
                }
            }
            TokenType::QuotedStr => {
                let serialized_value = expr.lexeme().to_string();
                self.advance();
                // replica o comportamento antigo
                let mut regex = None;
                if !is_falsy_or_whitespace(&serialized_value) {
                    let units: Vec<u16> = serialized_value.encode_utf16().collect();
                    let start = units.iter().position(|u| *u == b'/' as u16);
                    let end = units.iter().rposition(|u| *u == b'/' as u16);
                    if let (Some(start), Some(end)) = (start, end)
                        && start != end
                    {
                        let value = String::from_utf16_lossy(&units[start + 1..end]);
                        let case_ignore = if units.get(end + 1) == Some(&(b'i' as u16)) { "i" } else { "" };
                        match ContextRegExp::new(&value, case_ignore) {
                            Ok(r) => regex = Some(r),
                            Err(_) => return Err(self.err_expected_but_got("REGEX", &expr, None)),
                        }
                    }
                }
                match regex {
                    Some(r) => Ok(ContextKeyRegexExpr::create(key, Some(r))),
                    None => Err(self.err_expected_but_got("REGEX", &expr, None)),
                }
            }
            _ => {
                let token = self.peek().clone();
                Err(self.err_expected_but_got("REGEX", &token, None))
            }
        }
    }

    fn value(&mut self) -> String {
        let token = self.peek().clone();
        match token.ty {
            TokenType::Str | TokenType::QuotedStr => {
                self.advance();
                token.lexeme().to_string()
            }
            TokenType::True => {
                self.advance();
                "true".to_string()
            }
            TokenType::False => {
                self.advance();
                "false".to_string()
            }
            // `in` vale como valor (`"when": "languageId == in"` existe em extensões)
            TokenType::In => {
                self.advance();
                "in".to_string()
            }
            // Permite `"when": "foo == "`, usado por extensões; não avança de propósito.
            _ => String::new(),
        }
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.current - 1]
    }

    fn match_one(&mut self, ty: TokenType) -> bool {
        if self.check(ty) {
            self.advance();
            return true;
        }
        false
    }

    fn advance(&mut self) {
        if !self.is_at_end() {
            self.current += 1;
        }
    }

    fn consume(&mut self, ty: TokenType, message: &str) -> ParseResult<()> {
        if self.check(ty) {
            self.advance();
            return Ok(());
        }
        let token = self.peek().clone();
        Err(self.err_expected_but_got(message, &token, None))
    }

    fn err_expected_but_got(&mut self, expected: &str, got: &Token, additional_info: Option<String>) -> ParseAbort {
        let lexeme = get_lexeme(got);
        let message = localize("contextkey.parser.error.expectedButGot", "Expected: {0}\nReceived: '{1}'.", &[expected, &lexeme]);
        self.parsing_errors.push(ParsingError { message, offset: got.offset, lexeme, additional_info });
        ParseAbort
    }

    fn check(&self, ty: TokenType) -> bool {
        self.peek().ty == ty
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn is_at_end(&self) -> bool {
        self.peek().ty == TokenType::Eof
    }
}

/// `isFalsyOrWhitespace`.
fn is_falsy_or_whitespace(text: &str) -> bool {
    text.is_empty() || js_string::trim(&text.encode_utf16().collect::<Vec<_>>()).is_empty()
}

/// `new RegExp(lexeme.substring(1, closingSlashIndex), flagsSemGY)` de um `/REGEX/FLAGS`.
fn build_regexp(lexeme: &str) -> Option<ContextRegExp> {
    let units: Vec<u16> = lexeme.encode_utf16().collect();
    let closing = units.iter().rposition(|u| *u == b'/' as u16).map_or(-1, |i| i as i64);
    let flags: String = if closing == units.len() as i64 - 1 {
        String::new()
    } else {
        String::from_utf16_lossy(js_string::substring(&units, closing + 1, i64::MAX)).replace(['g', 'y'], "")
    };
    let pattern = String::from_utf16_lossy(js_string::substring(&units, 1, closing));
    ContextRegExp::new(&pattern, &flags).ok()
}

/// O resultado de `validateWhenClauses` para um erro.
#[derive(Clone, Debug, PartialEq)]
pub struct WhenClauseValidationError {
    pub error_message: String,
    pub offset: usize,
    pub length: usize,
}

/// `validateWhenClauses`.
pub fn validate_when_clauses(when_clauses: &[&str]) -> Vec<Vec<WhenClauseValidationError>> {
    // Sem recuperação, para guiar o usuário a escrever as regex corretas.
    let mut parser = Parser::new(ParserConfig { regex_parsing_with_error_recovery: false });
    when_clauses
        .iter()
        .map(|when_clause| {
            parser.parse(when_clause);
            if !parser.lexing_errors().is_empty() {
                parser
                    .lexing_errors()
                    .iter()
                    .map(|se| WhenClauseValidationError {
                        error_message: match &se.additional_info {
                            Some(info) => localize("contextkey.scanner.errorForLinterWithHint", "Unexpected token. Hint: {0}", &[info]),
                            None => localize("contextkey.scanner.errorForLinter", "Unexpected token.", &[]),
                        },
                        offset: se.offset,
                        length: se.lexeme.encode_utf16().count(),
                    })
                    .collect()
            } else {
                parser
                    .parsing_errors()
                    .iter()
                    .map(|pe| WhenClauseValidationError {
                        error_message: match &pe.additional_info {
                            Some(info) => format!("{}. {}", pe.message, info),
                            None => pe.message.clone(),
                        },
                        offset: pe.offset,
                        length: pe.lexeme.encode_utf16().count(),
                    })
                    .collect()
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// RawContextKey e o contrato do serviço de contexto.
// ---------------------------------------------------------------------------------------------

use je_base::event::Event;
use je_base::lifecycle::IDisposable;

/// `ContextKeyInfo`.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextKeyInfo {
    pub key: String,
    pub ty: Option<String>,
    pub description: Option<String>,
}

/// O terceiro argumento do `RawContextKey` (`string | true | { type, description }`).
#[derive(Clone, Debug, PartialEq)]
pub enum ContextKeyMeta {
    /// Sem metadado (`undefined`): a chave é registrada sem descrição.
    None,
    /// `true`: a chave não aparece na lista central.
    Hide,
    Description(String),
    Info { ty: String, description: String },
}

impl From<&str> for ContextKeyMeta {
    fn from(description: &str) -> Self {
        ContextKeyMeta::Description(description.to_string())
    }
}

impl From<String> for ContextKeyMeta {
    fn from(description: String) -> Self {
        ContextKeyMeta::Description(description)
    }
}

thread_local! {
    static RAW_CONTEXT_KEY_INFO: RefCell<Vec<ContextKeyInfo>> = const { RefCell::new(Vec::new()) };
}

/// `typeof defaultValue`.
fn type_of_default(value: &Dynamic) -> Option<String> {
    if value.is_nullish() { None } else { Some(value.type_of().to_string()) }
}

/// `RawContextKey`: o nome da chave, o valor padrão e a expressão `Defined` da chave.
#[derive(Clone)]
pub struct RawContextKey {
    expr: ContextKeyExpression,
    key: String,
    default_value: Dynamic,
}

impl RawContextKey {
    pub fn new(key: &str, default_value: impl Into<Dynamic>, meta_or_hide: impl Into<ContextKeyMeta>) -> Self {
        let default_value = default_value.into();
        // Sem passar por `ContextKeyDefinedExpr.create`: a chave `isMac` continua uma chave de verdade.
        let expr = ContextKeyExpression::new(Expr::Defined { key: key.to_string() });
        // Reúne todas as chaves num lugar só.
        match meta_or_hide.into() {
            ContextKeyMeta::Info { ty, description } => RAW_CONTEXT_KEY_INFO
                .with(|info| info.borrow_mut().push(ContextKeyInfo { key: key.to_string(), ty: Some(ty), description: Some(description) })),
            ContextKeyMeta::Hide => {}
            meta => {
                let description = match meta {
                    ContextKeyMeta::Description(d) => Some(d),
                    _ => None,
                };
                RAW_CONTEXT_KEY_INFO.with(|info| {
                    info.borrow_mut().push(ContextKeyInfo { key: key.to_string(), ty: type_of_default(&default_value), description })
                });
            }
        }
        Self { expr, key: key.to_string(), default_value }
    }

    /// `RawContextKey.all()`.
    pub fn all() -> Vec<ContextKeyInfo> {
        RAW_CONTEXT_KEY_INFO.with(|info| info.borrow().clone())
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// A expressão `Defined` desta chave (o `RawContextKey` do original é ela própria).
    pub fn expr(&self) -> ContextKeyExpression {
        self.expr.clone()
    }

    pub fn bind_to(&self, target: &dyn IContextKeyService) -> Rc<dyn IContextKey> {
        target.create_key(&self.key, self.default_value.clone())
    }

    pub fn get_value(&self, target: &dyn IContextKeyService) -> Dynamic {
        target.get_context_key_value(&self.key)
    }

    pub fn to_negated(&self) -> ContextKeyExpression {
        self.expr.negate()
    }

    pub fn is_equal_to(&self, value: impl Into<Dynamic>) -> ContextKeyExpression {
        ContextKeyEqualsExpr::create(&self.key, value.into())
    }

    pub fn not_equals_to(&self, value: impl Into<Dynamic>) -> ContextKeyExpression {
        ContextKeyNotEqualsExpr::create(&self.key, value.into())
    }

    pub fn greater(&self, value: impl Into<Dynamic>) -> ContextKeyExpression {
        ContextKeyGreaterExpr::create(&self.key, &value.into())
    }
}

impl From<&RawContextKey> for ContextKeyExpression {
    fn from(key: &RawContextKey) -> Self {
        key.expr()
    }
}

/// `IContextKey`.
pub trait IContextKey {
    fn set(&self, value: Dynamic);
    fn reset(&self);
    fn get(&self) -> Dynamic;
}

/// `IContextKeyServiceTarget`: o que o serviço escopado lê do nó (o DOM, no original).
pub trait IContextKeyServiceTarget {
    fn parent_element(&self) -> Option<Rc<dyn IContextKeyServiceTarget>>;
    fn set_attribute(&self, attr: &str, value: &str);
    fn remove_attribute(&self, attr: &str);
    fn has_attribute(&self, attr: &str) -> bool;
    fn get_attribute(&self, attr: &str) -> Option<String>;
}

/// `IReadableSet<string>`.
pub trait ReadableSet {
    fn has(&self, value: &str) -> bool;
}

impl ReadableSet for std::collections::HashSet<String> {
    fn has(&self, value: &str) -> bool {
        self.contains(value)
    }
}

impl ReadableSet for std::collections::BTreeSet<String> {
    fn has(&self, value: &str) -> bool {
        self.contains(value)
    }
}

/// `IContextKeyChangeEvent`.
pub trait IContextKeyChangeEvent {
    fn affects_some(&self, keys: &dyn ReadableSet) -> bool;
    fn all_keys_contained_in(&self, keys: &dyn ReadableSet) -> bool;
}

/// `IScopedContextKeyService` (`IContextKeyService & IDisposable`).
pub trait IScopedContextKeyService: IContextKeyService + IDisposable {}

/// `IContextKeyService`.
pub trait IContextKeyService {
    fn on_did_change_context(&self) -> Event<Rc<dyn IContextKeyChangeEvent>>;
    fn buffer_change_events(&self, callback: &dyn Fn());

    fn create_key(&self, key: &str, default_value: Dynamic) -> Rc<dyn IContextKey>;
    fn context_matches_rules(&self, rules: Option<&ContextKeyExpression>) -> bool;
    fn get_context_key_value(&self, key: &str) -> Dynamic;

    fn create_scoped(&self, target: Rc<dyn IContextKeyServiceTarget>) -> Rc<dyn IScopedContextKeyService>;
    fn create_overlay(&self, overlay: Vec<(String, Dynamic)>) -> Rc<dyn IContextKeyService>;
    fn get_context(&self, target: Option<Rc<dyn IContextKeyServiceTarget>>) -> Rc<dyn IContext>;

    fn update_parent(&self, parent_context_key_service: Rc<dyn IContextKeyService>);

    /// O `contextId` do original (as implementações concretas o expõem; o contrato do TypeScript não).
    fn context_id(&self) -> i64;
    /// O `getContextValuesContainer` do original.
    fn get_context_values_container(&self, context_id: i64) -> Rc<dyn IContext>;
    /// O serviço como `AbstractContextKeyService`, para `updateParent` (o cast do original).
    fn as_abstract(&self) -> Option<Rc<super::service::AbstractContextKeyService>> {
        None
    }
}

je_instantiation::service_id!(pub I_CONTEXT_KEY_SERVICE: dyn IContextKeyService = "contextKeyService");
