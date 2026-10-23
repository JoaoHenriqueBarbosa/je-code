//! Porte do subconjunto de `src/vs/base/common/semver/semver.js` (node-semver) que o VS Code usa (o
//! gerenciamento de extensões e a sincronização): `valid`, `parse`, `compare`, `gt`, `lt`, `eq`, `lte`, `rcompare` e
//! `satisfies`.
//!
//! Modo estrito (`loose` falso), como o VS Code chama. O número de cada componente cabe em `u64` (o original
//! limita a `Number.MAX_SAFE_INTEGER`).

use std::cmp::Ordering;

const MAX_LENGTH: usize = 256;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Um identificador de pré-lançamento: numérico (comparado como número) ou alfanumérico.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Identifier {
    Numeric(u64),
    Text(String),
}

/// `SemVer`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Vec<Identifier>,
    pub build: Vec<String>,
}

fn parse_number(part: &str) -> Option<u64> {
    // `0|[1-9]\d*`: sem zeros à esquerda.
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) || (part.len() > 1 && part.starts_with('0')) {
        return None;
    }
    part.parse::<u64>().ok().filter(|n| *n <= MAX_SAFE_INTEGER)
}

fn parse_identifier(part: &str) -> Option<Identifier> {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    if part.bytes().all(|b| b.is_ascii_digit()) {
        // `0|[1-9]\d*` ou, se tiver zero à esquerda, o regex original recusa.
        return parse_number(part).map(Identifier::Numeric);
    }
    Some(Identifier::Text(part.to_owned()))
}

impl SemVer {
    /// `semver.parse(version)`: `None` quando não é uma versão semver estrita.
    pub fn parse(version: &str) -> Option<SemVer> {
        if version.len() > MAX_LENGTH {
            return None;
        }
        // O regex estrito é `^v?(\d+).(\d+).(\d+)(-pre)?(+build)?$`, sem `trim` (o `trim` é só do construtor).
        let rest = version.strip_prefix('v').unwrap_or(version);
        let (rest, build) = match rest.split_once('+') {
            Some((head, build)) => (head, Some(build)),
            None => (rest, None),
        };
        let (main, pre) = match rest.split_once('-') {
            Some((main, pre)) => (main, Some(pre)),
            None => (rest, None),
        };
        let mut numbers = main.split('.');
        let major = parse_number(numbers.next()?)?;
        let minor = parse_number(numbers.next()?)?;
        let patch = parse_number(numbers.next()?)?;
        if numbers.next().is_some() {
            return None;
        }
        let prerelease = match pre {
            Some(pre) => pre.split('.').map(parse_identifier).collect::<Option<Vec<_>>>()?,
            None => Vec::new(),
        };
        let build = match build {
            Some(build) => {
                let parts: Vec<&str> = build.split('.').collect();
                if parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')) {
                    return None;
                }
                parts.into_iter().map(str::to_owned).collect()
            }
            None => Vec::new(),
        };
        Some(SemVer { major, minor, patch, prerelease, build })
    }

    /// `SemVer#version` (a forma normalizada, sem `v` e sem `build`).
    pub fn version(&self) -> String {
        let mut out = format!("{}.{}.{}", self.major, self.minor, self.patch);
        if !self.prerelease.is_empty() {
            out.push('-');
            let parts: Vec<String> = self
                .prerelease
                .iter()
                .map(|id| match id {
                    Identifier::Numeric(n) => n.to_string(),
                    Identifier::Text(t) => t.clone(),
                })
                .collect();
            out.push_str(&parts.join("."));
        }
        out
    }

    /// `SemVer#compare`.
    pub fn compare(&self, other: &SemVer) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| self.compare_pre(other))
    }

    fn compare_pre(&self, other: &SemVer) -> Ordering {
        match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
            (false, true) => return Ordering::Less,
            (true, false) => return Ordering::Greater,
            (true, true) => return Ordering::Equal,
            (false, false) => {}
        }
        let mut index = 0;
        loop {
            match (self.prerelease.get(index), other.prerelease.get(index)) {
                (None, None) => return Ordering::Equal,
                (Some(_), None) => return Ordering::Greater,
                (None, Some(_)) => return Ordering::Less,
                (Some(a), Some(b)) if a == b => {}
                (Some(a), Some(b)) => return compare_identifiers(a, b),
            }
            index += 1;
        }
    }
}

/// `compareIdentifiers`: numérico vem antes de texto.
fn compare_identifiers(a: &Identifier, b: &Identifier) -> Ordering {
    match (a, b) {
        (Identifier::Numeric(a), Identifier::Numeric(b)) => a.cmp(b),
        (Identifier::Numeric(_), Identifier::Text(_)) => Ordering::Less,
        (Identifier::Text(_), Identifier::Numeric(_)) => Ordering::Greater,
        (Identifier::Text(a), Identifier::Text(b)) => a.cmp(b),
    }
}

/// `semver.valid(version)`: a versão normalizada ou `None`.
pub fn valid(version: &str) -> Option<String> {
    SemVer::parse(version).map(|v| v.version())
}

/// `semver.compare(a, b)`, com o `trim` do construtor. Versão inválida lança no original; aqui é `None`.
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    Some(construct(a)?.compare(&construct(b)?))
}

/// `new SemVer(version)`: aceita espaços nas pontas.
fn construct(version: &str) -> Option<SemVer> {
    if version.len() > MAX_LENGTH {
        return None;
    }
    SemVer::parse(version.trim())
}

/// `semver.gt`.
pub fn gt(a: &str, b: &str) -> bool {
    compare(a, b) == Some(Ordering::Greater)
}

/// `semver.lt`.
pub fn lt(a: &str, b: &str) -> bool {
    compare(a, b) == Some(Ordering::Less)
}

/// `semver.lte`.
pub fn lte(a: &str, b: &str) -> bool {
    matches!(compare(a, b), Some(Ordering::Less | Ordering::Equal))
}

/// `semver.eq`.
pub fn eq(a: &str, b: &str) -> bool {
    compare(a, b) == Some(Ordering::Equal)
}

/// `semver.rcompare(a, b)`, para ordenar do maior para o menor.
pub fn rcompare(a: &str, b: &str) -> Ordering {
    compare(b, a).unwrap_or(Ordering::Equal)
}

// ---- Ranges ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Lt,
    Lte,
    Gt,
    Gte,
    Eq,
}

#[derive(Clone, Debug)]
struct Comparator {
    /// `None` é o comparador vazio (`*`), que aceita qualquer versão.
    bound: Option<(Op, SemVer)>,
}

impl Comparator {
    fn test(&self, version: &SemVer) -> bool {
        match &self.bound {
            None => true,
            Some((op, bound)) => {
                let ordering = version.compare(bound);
                match op {
                    Op::Lt => ordering == Ordering::Less,
                    Op::Lte => ordering != Ordering::Greater,
                    Op::Gt => ordering == Ordering::Greater,
                    Op::Gte => ordering != Ordering::Less,
                    Op::Eq => ordering == Ordering::Equal,
                }
            }
        }
    }
}

fn is_x(part: &str) -> bool {
    part.is_empty() || part.eq_ignore_ascii_case("x") || part == "*"
}

/// Um componente parcial de versão (`1`, `1.2`, `1.2.x`, `1.2.3-beta`).
struct Partial<'a> {
    major: &'a str,
    minor: &'a str,
    patch: &'a str,
    pre: &'a str,
}

fn split_partial(text: &str) -> Option<Partial<'_>> {
    let text = text.trim_start_matches(['v', '=']).trim();
    let (text, _build) = text.split_once('+').unwrap_or((text, ""));
    let mut parts = text.splitn(3, '.');
    let major = parts.next().unwrap_or("");
    let minor = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    let (patch, pre) = match rest.split_once('-') {
        Some((patch, pre)) => (patch, pre),
        None => (rest, ""),
    };
    let numeric_or_x = |part: &str| is_x(part) || part.bytes().all(|b| b.is_ascii_digit());
    if !(numeric_or_x(major) && numeric_or_x(minor) && numeric_or_x(patch)) {
        return None;
    }
    Some(Partial { major, minor, patch, pre })
}

fn plain(op: Op, version: &str) -> Option<Comparator> {
    Some(Comparator { bound: Some((op, SemVer::parse(version)?)) })
}

fn num(part: &str) -> u64 {
    part.parse().unwrap_or(0)
}

/// Expande um comparador (já sem espaço entre operador e versão) para comparadores simples.
fn expand(token: &str) -> Option<Vec<Comparator>> {
    let token = token.trim();
    let mut out = Vec::new();
    if let Some(rest) = token.strip_prefix('^') {
        let p = split_partial(rest)?;
        if is_x(p.major) {
            out.push(Comparator { bound: None });
        } else if is_x(p.minor) {
            out.push(plain(Op::Gte, &format!("{}.0.0", p.major))?);
            out.push(plain(Op::Lt, &format!("{}.0.0", num(p.major) + 1))?);
        } else if is_x(p.patch) {
            out.push(plain(Op::Gte, &format!("{}.{}.0", p.major, p.minor))?);
            if num(p.major) == 0 {
                out.push(plain(Op::Lt, &format!("{}.{}.0", p.major, num(p.minor) + 1))?);
            } else {
                out.push(plain(Op::Lt, &format!("{}.0.0", num(p.major) + 1))?);
            }
        } else {
            let pre = if p.pre.is_empty() { String::new() } else { format!("-{}", p.pre) };
            out.push(plain(Op::Gte, &format!("{}.{}.{}{}", p.major, p.minor, p.patch, pre))?);
            if num(p.major) == 0 {
                if num(p.minor) == 0 {
                    out.push(plain(Op::Lt, &format!("{}.{}.{}", p.major, p.minor, num(p.patch) + 1))?);
                } else {
                    out.push(plain(Op::Lt, &format!("{}.{}.0", p.major, num(p.minor) + 1))?);
                }
            } else {
                out.push(plain(Op::Lt, &format!("{}.0.0", num(p.major) + 1))?);
            }
        }
        return Some(out);
    }
    if let Some(rest) = token.strip_prefix("~>").or_else(|| token.strip_prefix('~')) {
        let p = split_partial(rest)?;
        if is_x(p.major) {
            out.push(Comparator { bound: None });
        } else if is_x(p.minor) {
            out.push(plain(Op::Gte, &format!("{}.0.0", p.major))?);
            out.push(plain(Op::Lt, &format!("{}.0.0", num(p.major) + 1))?);
        } else if is_x(p.patch) {
            out.push(plain(Op::Gte, &format!("{}.{}.0", p.major, p.minor))?);
            out.push(plain(Op::Lt, &format!("{}.{}.0", p.major, num(p.minor) + 1))?);
        } else {
            let pre = if p.pre.is_empty() { String::new() } else { format!("-{}", p.pre) };
            out.push(plain(Op::Gte, &format!("{}.{}.{}{}", p.major, p.minor, p.patch, pre))?);
            out.push(plain(Op::Lt, &format!("{}.{}.0", p.major, num(p.minor) + 1))?);
        }
        return Some(out);
    }
    // Operador seguido de uma versão parcial (x-range).
    let (op, rest) = if let Some(rest) = token.strip_prefix(">=") {
        (">=", rest)
    } else if let Some(rest) = token.strip_prefix("<=") {
        ("<=", rest)
    } else if let Some(rest) = token.strip_prefix('>') {
        (">", rest)
    } else if let Some(rest) = token.strip_prefix('<') {
        ("<", rest)
    } else if let Some(rest) = token.strip_prefix('=') {
        ("=", rest)
    } else {
        ("", token)
    };
    let p = split_partial(rest)?;
    let major_x = is_x(p.major);
    let minor_x = major_x || is_x(p.minor);
    let patch_x = minor_x || is_x(p.patch);
    let op = if op == "=" && patch_x { "" } else { op };
    if major_x {
        // `*`, `>*` e afins: `>` e `<` não casam nada, o resto casa tudo.
        if op == ">" || op == "<" {
            out.push(plain(Op::Lt, "0.0.0")?);
        } else {
            out.push(Comparator { bound: None });
        }
        return Some(out);
    }
    if patch_x {
        let (major, minor) = (num(p.major), if minor_x { 0 } else { num(p.minor) });
        match op {
            ">" => {
                let (major, minor) = if minor_x { (major + 1, 0) } else { (major, minor + 1) };
                out.push(plain(Op::Gte, &format!("{major}.{minor}.0"))?);
            }
            "<=" => {
                let (major, minor) = if minor_x { (major + 1, 0) } else { (major, minor + 1) };
                out.push(plain(Op::Lt, &format!("{major}.{minor}.0"))?);
            }
            ">=" => out.push(plain(Op::Gte, &format!("{major}.{minor}.0"))?),
            "<" => out.push(plain(Op::Lt, &format!("{major}.{minor}.0"))?),
            _ => {
                out.push(plain(Op::Gte, &format!("{major}.{minor}.0"))?);
                if minor_x {
                    out.push(plain(Op::Lt, &format!("{}.0.0", major + 1))?);
                } else {
                    out.push(plain(Op::Lt, &format!("{major}.{}.0", minor + 1))?);
                }
            }
        }
        return Some(out);
    }
    let pre = if p.pre.is_empty() { String::new() } else { format!("-{}", p.pre) };
    let version = format!("{}.{}.{}{}", p.major, p.minor, p.patch, pre);
    let op = match op {
        ">=" => Op::Gte,
        "<=" => Op::Lte,
        ">" => Op::Gt,
        "<" => Op::Lt,
        _ => Op::Eq,
    };
    out.push(plain(op, &version)?);
    Some(out)
}

/// Um conjunto de comparadores (um lado de um `||`).
fn parse_set(text: &str) -> Option<Vec<Comparator>> {
    let mut text = text.trim().to_owned();
    // Hífen: `1.2.3 - 2.3.4`.
    if let Some((from, to)) = text.split_once(" - ") {
        let (from, to) = (from.trim().to_owned(), to.trim().to_owned());
        let low = split_partial(&from)?;
        let high = split_partial(&to)?;
        let lower = if is_x(low.major) {
            String::new()
        } else if is_x(low.minor) {
            format!(">={}.0.0", low.major)
        } else if is_x(low.patch) {
            format!(">={}.{}.0", low.major, low.minor)
        } else {
            format!(">={from}")
        };
        let upper = if is_x(high.major) {
            String::new()
        } else if is_x(high.minor) {
            format!("<{}.0.0", num(high.major) + 1)
        } else if is_x(high.patch) {
            format!("<{}.{}.0", high.major, num(high.minor) + 1)
        } else {
            format!("<={to}")
        };
        text = format!("{lower} {upper}");
    }
    // Junta o operador solto ao seu operando (`>= 1.2.3`).
    let mut tokens: Vec<String> = Vec::new();
    for token in text.split_whitespace() {
        match tokens.last_mut() {
            Some(last) if matches!(last.as_str(), ">" | ">=" | "<" | "<=" | "=" | "^" | "~" | "~>") => last.push_str(token),
            _ => tokens.push(token.to_owned()),
        }
    }
    let mut out = Vec::new();
    for token in &tokens {
        out.extend(expand(token)?);
    }
    if out.is_empty() {
        out.push(Comparator { bound: None });
    }
    Some(out)
}

/// `semver.satisfies(version, range)`. Versão ou range inválido dá `false`, como o original.
pub fn satisfies(version: &str, range: &str) -> bool {
    let Some(version) = construct(version) else { return false };
    let sets: Option<Vec<Vec<Comparator>>> = range.split("||").map(parse_set).collect();
    let Some(sets) = sets else { return false };
    sets.iter().any(|set| {
        if !set.iter().all(|c| c.test(&version)) {
            return false;
        }
        if version.prerelease.is_empty() {
            return true;
        }
        // Pré-lançamento só casa se algum comparador da mesma tupla também for pré-lançamento.
        set.iter().any(|c| {
            c.bound
                .as_ref()
                .is_some_and(|(_, b)| !b.prerelease.is_empty() && (b.major, b.minor, b.patch) == (version.major, version.minor, version.patch))
        })
    })
}
