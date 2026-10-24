//! `fnmatch(3)` do glibc: o motor único de padrões de nome (`*`, `?`, listas `[...]` com `!`/`^`,
//! faixas e classes nomeadas `[:alpha:]`, `\` como escape) que tar, diff, grep, tree e os utilitários
//! de `ul-misc` usavam cada um à sua maneira.
//!
//! O que muda de um programa pra outro é parâmetro:
//!
//! - as [`Flags`] do `fnmatch` (`PATHNAME`, `PERIOD`, `NOESCAPE`, `CASEFOLD`, `LEADING_DIR`) mais duas
//!   variações: `PLAIN_BRACKET` (o matcher próprio do tree) e `TRAILING_BACKSLASH_LITERAL` (o tree e o
//!   fnmatch do gnulib que tar, diff e grep levam, que casam o `\` final como ele mesmo);
//! - o [`Alphabet`], que diz o que é uma unidade do texto (byte, `char` ou o `Unit` do grep), como ela
//!   se compara a um caractere ASCII, e o que é uma classe nomeada pra ela.
//!
//! O casamento é por retrocesso em cada `*`, com memória dos pares (padrão, texto) que já falharam, então
//! padrões como `*a*a*a*b` não explodem; com `PATHNAME` o `*` não atravessa `/`, o que um retrocesso só
//! no último `*` não cobriria.

use std::marker::PhantomData;
use std::ops::BitOr;

/// Conjunto de flags do `fnmatch`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags(u8);

impl Flags {
    /// Nenhuma flag: `*` e `?` casam `/` e o ponto inicial, `\` escapa.
    pub const NONE: Flags = Flags(0);
    /// `FNM_PATHNAME`: `*`, `?` e listas `[...]` não casam `/`.
    pub const PATHNAME: Flags = Flags(1);
    /// `FNM_PERIOD`: o ponto inicial (do texto, ou depois de `/` com `PATHNAME`) só casa com `.` literal.
    pub const PERIOD: Flags = Flags(2);
    /// `FNM_NOESCAPE`: `\` é um caractere comum.
    pub const NOESCAPE: Flags = Flags(4);
    /// `FNM_CASEFOLD`: literais e faixas comparam sem distinguir maiúsculas (a unidade é dobrada pelo
    /// [`Alphabet::fold`]; as classes nomeadas olham a unidade original).
    pub const CASEFOLD: Flags = Flags(8);
    /// `FNM_LEADING_DIR`: o padrão também casa se cobrir um prefixo de diretórios do texto (`a` casa `a/b/c`).
    pub const LEADING_DIR: Flags = Flags(16);
    /// Lista `[...]` simples, como a do matcher próprio do tree: sem classes `[:nome:]` e sem `\` dentro
    /// da lista (a lista fecha no primeiro `]` depois do primeiro item).
    pub const PLAIN_BRACKET: Flags = Flags(32);
    /// `\` no fim do padrão vale como ela mesma (tree, e o gnulib de tar, diff e grep, conferido no
    /// Debian 13); sem esta flag falha, como no glibc ("trailing \ loses").
    pub const TRAILING_BACKSLASH_LITERAL: Flags = Flags(64);

    pub const fn contains(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }

    /// Liga `other` quando `on`; devolve `self` quando não. Serve pra montar as flags a partir de opções.
    pub const fn with(self, other: Flags, on: bool) -> Flags {
        if on { Flags(self.0 | other.0) } else { self }
    }
}

impl BitOr for Flags {
    type Output = Flags;

    fn bitor(self, rhs: Flags) -> Flags {
        Flags(self.0 | rhs.0)
    }
}

/// O que o motor precisa saber de uma unidade de texto.
pub trait Alphabet {
    type Unit: Copy + Eq;

    /// A unidade é o caractere ASCII `ascii`.
    fn is(unit: Self::Unit, ascii: u8) -> bool;

    /// Dobra a unidade pra comparação sem distinguir maiúsculas (só usado com `CASEFOLD`).
    fn fold(unit: Self::Unit) -> Self::Unit {
        unit
    }

    /// `lo <= c <= hi` (as três já dobradas quando há `CASEFOLD`).
    fn in_range(lo: Self::Unit, hi: Self::Unit, c: Self::Unit) -> bool;

    /// `c` pertence à classe `[:name:]`. `None` quando o nome não é uma classe conhecida: o motor trata
    /// então o `[` como caractere comum da lista. `Some(false)` consome a classe sem casar nada.
    fn in_class(name: &[Self::Unit], c: Self::Unit) -> Option<bool>;
}

/// Texto em bytes, classes ASCII (o `fnmatch` do glibc em C pros nomes de arquivo).
pub struct Bytes;

impl Alphabet for Bytes {
    type Unit = u8;

    fn is(unit: u8, ascii: u8) -> bool {
        unit == ascii
    }

    fn fold(unit: u8) -> u8 {
        unit.to_ascii_lowercase()
    }

    fn in_range(lo: u8, hi: u8, c: u8) -> bool {
        lo <= c && c <= hi
    }

    fn in_class(name: &[u8], c: u8) -> Option<bool> {
        Some(match name {
            b"alpha" => c.is_ascii_alphabetic(),
            b"digit" => c.is_ascii_digit(),
            b"alnum" => c.is_ascii_alphanumeric(),
            b"upper" => c.is_ascii_uppercase(),
            b"lower" => c.is_ascii_lowercase(),
            b"space" => matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c),
            b"blank" => matches!(c, b' ' | b'\t'),
            b"punct" => c.is_ascii_punctuation(),
            b"print" => (0x20..0x7f).contains(&c),
            b"graph" => (0x21..0x7f).contains(&c),
            b"cntrl" => c < 0x20 || c == 0x7f,
            b"xdigit" => c.is_ascii_hexdigit(),
            _ => false,
        })
    }
}

/// Texto em caracteres Unicode (o glibc em C.UTF-8): classes Unicode, e nome de classe desconhecido
/// devolve `None`.
pub struct Chars;

impl Alphabet for Chars {
    type Unit = char;

    fn is(unit: char, ascii: u8) -> bool {
        unit == char::from(ascii)
    }

    fn fold(unit: char) -> char {
        let mut lower = unit.to_lowercase();
        match (lower.next(), lower.next()) {
            (Some(one), None) => one,
            _ => unit,
        }
    }

    fn in_range(lo: char, hi: char, c: char) -> bool {
        lo <= c && c <= hi
    }

    fn in_class(name: &[char], c: char) -> Option<bool> {
        let name: String = name.iter().collect();
        Some(match name.as_str() {
            "alpha" => c.is_alphabetic(),
            "digit" => c.is_ascii_digit(),
            "alnum" => c.is_alphanumeric(),
            "upper" => c.is_uppercase(),
            "lower" => c.is_lowercase(),
            "space" => matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'),
            "blank" => c == ' ' || c == '\t',
            "punct" => {
                c.is_ascii_punctuation() || (!c.is_ascii() && !c.is_alphanumeric() && !c.is_whitespace() && !c.is_control())
            }
            "print" => !c.is_control(),
            "graph" => !c.is_control() && !c.is_whitespace(),
            "cntrl" => c.is_control(),
            "xdigit" => c.is_ascii_hexdigit(),
            _ => return None,
        })
    }
}

/// Profundidade máxima de `*` aninhados; passou disso, não casa.
const MAX_DEPTH: usize = 10_000;

struct Matcher<'a, A: Alphabet> {
    p: &'a [A::Unit],
    s: &'a [A::Unit],
    f: Flags,
    /// `failed[pi * (s.len() + 1) + si]`: o resto do padrão a partir de `pi` já falhou no texto a partir
    /// de `si`. Só é alocado no primeiro `*` que não fecha o padrão.
    failed: Vec<bool>,
    alphabet: PhantomData<A>,
}

impl<A: Alphabet> Matcher<'_, A> {
    fn fold(&self, u: A::Unit) -> A::Unit {
        if self.f.contains(Flags::CASEFOLD) { A::fold(u) } else { u }
    }

    fn same(&self, a: A::Unit, b: A::Unit) -> bool {
        self.fold(a) == self.fold(b)
    }

    fn slash_at(&self, si: usize) -> bool {
        self.s.get(si).is_some_and(|&u| A::is(u, b'/'))
    }

    /// O texto em `si` é um ponto inicial que `*`, `?` e `[...]` não podem casar (`FNM_PERIOD`).
    fn blocks_period(&self, si: usize) -> bool {
        self.f.contains(Flags::PERIOD)
            && self.s.get(si).is_some_and(|&u| A::is(u, b'.'))
            && (si == 0 || (self.f.contains(Flags::PATHNAME) && self.slash_at(si - 1)))
    }

    /// Lista `[...]` que começa em `p[start] == '['`: `Some((casou, índice depois do ']'))`, ou `None`
    /// quando a lista não fecha (e o `[` vale como caractere comum).
    fn bracket(&self, start: usize, c: A::Unit) -> Option<(bool, usize)> {
        let (p, f) = (self.p, self.f);
        let plain = f.contains(Flags::PLAIN_BRACKET);
        let escapes = !plain && !f.contains(Flags::NOESCAPE);
        let mut i = start + 1;
        let negate = p.get(i).is_some_and(|&u| A::is(u, b'!') || A::is(u, b'^'));
        if negate {
            i += 1;
        }
        let cc = self.fold(c);
        let mut matched = false;
        let mut first = true;
        loop {
            let u = *p.get(i)?;
            if A::is(u, b']') && !first {
                i += 1;
                break;
            }
            first = false;
            if !plain
                && A::is(u, b'[')
                && p.get(i + 1).is_some_and(|&x| A::is(x, b':'))
                && let Some(end) = (i + 2..p.len().saturating_sub(1)).find(|&k| A::is(p[k], b':') && A::is(p[k + 1], b']'))
                && let Some(in_class) = A::in_class(&p[i + 2..end], c)
            {
                matched |= in_class;
                i = end + 2;
                continue;
            }
            let mut lo = u;
            if escapes && A::is(lo, b'\\') {
                i += 1;
                lo = *p.get(i)?;
            }
            i += 1;
            if p.get(i).is_some_and(|&x| A::is(x, b'-')) && p.get(i + 1).is_some_and(|&x| !A::is(x, b']')) {
                let mut hi = p[i + 1];
                i += 2;
                if escapes && A::is(hi, b'\\') {
                    hi = *p.get(i)?;
                    i += 1;
                }
                matched |= A::in_range(self.fold(lo), self.fold(hi), cc);
            } else {
                matched |= self.fold(lo) == cc;
            }
        }
        if f.contains(Flags::PATHNAME) && A::is(c, b'/') {
            return Some((false, i));
        }
        Some((matched != negate, i))
    }

    /// `*` já consumido, `pi` no que vem depois (que não é `*`): tenta o resto em cada `k`.
    fn star(&mut self, pi: usize, si: usize, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        if self.failed.is_empty() {
            self.failed = vec![false; (self.p.len() + 1) * (self.s.len() + 1)];
        }
        let width = self.s.len() + 1;
        let mut k = si;
        loop {
            if !self.failed[pi * width + k] {
                if self.run(pi, k, depth + 1) {
                    return true;
                }
                self.failed[pi * width + k] = true;
            }
            if k >= self.s.len() || (self.f.contains(Flags::PATHNAME) && self.slash_at(k)) {
                return false;
            }
            k += 1;
        }
    }

    fn run(&mut self, mut pi: usize, mut si: usize, depth: usize) -> bool {
        let (p, s, f) = (self.p, self.s, self.f);
        loop {
            let Some(&pu) = p.get(pi) else {
                return si == s.len() || (f.contains(Flags::LEADING_DIR) && A::is(s[si], b'/'));
            };
            if A::is(pu, b'*') {
                while p.get(pi).is_some_and(|&u| A::is(u, b'*')) {
                    pi += 1;
                }
                if self.blocks_period(si) {
                    return false;
                }
                if pi == p.len() {
                    // Com LEADING_DIR o `*` pode parar na próxima barra e o resto vale.
                    return !f.contains(Flags::PATHNAME) || f.contains(Flags::LEADING_DIR) || !s[si..].iter().any(|&u| A::is(u, b'/'));
                }
                return self.star(pi, si, depth);
            }
            if A::is(pu, b'?') {
                if si >= s.len() || (f.contains(Flags::PATHNAME) && self.slash_at(si)) || self.blocks_period(si) {
                    return false;
                }
            } else if A::is(pu, b'[') {
                if si >= s.len() || self.blocks_period(si) {
                    return false;
                }
                match self.bracket(pi, s[si]) {
                    Some((true, next)) => {
                        pi = next;
                        si += 1;
                        continue;
                    }
                    Some((false, _)) => return false,
                    None => {
                        if !A::is(s[si], b'[') {
                            return false;
                        }
                    }
                }
            } else if A::is(pu, b'\\') && !f.contains(Flags::NOESCAPE) && (pi + 1 < p.len() || !f.contains(Flags::TRAILING_BACKSLASH_LITERAL)) {
                // Depois de `\` o próximo caractere vale literalmente; `\` no fim perde (glibc).
                let Some(&lit) = p.get(pi + 1) else { return false };
                if si >= s.len() || !self.same(lit, s[si]) {
                    return false;
                }
                pi += 2;
                si += 1;
                continue;
            } else if si >= s.len() || !self.same(pu, s[si]) {
                return false;
            }
            pi += 1;
            si += 1;
        }
    }
}

/// Casa `text` com o padrão `pattern`, como `fnmatch(pattern, text, flags)`.
pub fn fnmatch<A: Alphabet>(pattern: &[A::Unit], text: &[A::Unit], flags: Flags) -> bool {
    Matcher::<A> { p: pattern, s: text, f: flags, failed: Vec::new(), alphabet: PhantomData }.run(0, 0, 0)
}

/// `fnmatch` sobre caracteres: bytes que não são UTF-8 válido viram U+FFFD, como o glibc em C.UTF-8.
pub fn fnmatch_utf8(pattern: &[u8], text: &[u8], flags: Flags) -> bool {
    let p: Vec<char> = String::from_utf8_lossy(pattern).chars().collect();
    let s: Vec<char> = String::from_utf8_lossy(text).chars().collect();
    fnmatch::<Chars>(&p, &s, flags)
}

/// O padrão tem caracteres de wildcard (`*`, `?`, `[` ou `\`).
pub fn has_wildcards(pattern: &[u8]) -> bool {
    pattern.iter().any(|&c| matches!(c, b'*' | b'?' | b'[' | b'\\'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, text: &str, flags: Flags) -> bool {
        fnmatch::<Bytes>(pattern.as_bytes(), text.as_bytes(), flags)
    }

    fn mu(pattern: &str, text: &str, flags: Flags) -> bool {
        fnmatch_utf8(pattern.as_bytes(), text.as_bytes(), flags)
    }

    const N: Flags = Flags::NONE;

    #[test]
    fn plain_wildcards() {
        assert!(m("*.o", "a.o", N));
        assert!(!m("*.o", "a.c", N));
        assert!(m("*", ".hidden", N));
        assert!(m("*", "", N));
        assert!(m("a?c", "abc", N));
        assert!(!m("?s", "s", N));
        assert!(m("*a*b*", "xxaxxbxx", N));
        assert!(m("*.txt", "a/b.txt", N));
        assert!(m("l*", "ls", N));
        assert!(m(".*", ".hidden", N));
        assert!(m("", "", N));
        assert!(!m("", "a", N));
        assert!(!m("a", "", N));
        assert!(m("a**", "a", N));
    }

    #[test]
    fn brackets() {
        assert!(m("[a-c]?", "bx", N));
        assert!(!m("[a-c]?", "dx", N));
        assert!(m("[!a]x", "bx", N));
        assert!(!m("[!a-c]x", "bx", N));
        assert!(m("[^a]x", "bx", N));
        assert!(m("[]]", "]", N));
        assert!(m("[!]]", "a", N));
        assert!(m("[a-]", "-", N));
        assert!(m("[", "[", N));
        assert!(m("[a", "[a", N));
        assert!(!m("[", "x", N));
        assert!(m("[\\]]", "]", N));
        assert!(m("[a\\-c]", "-", N));
        assert!(m("[\\a-\\c]", "b", N));
    }

    #[test]
    fn named_classes() {
        assert!(m("[[:digit:]]*", "9z", N));
        assert!(m("[[:digit:]]*", "7up", N));
        assert!(!m("[[:digit:]]", "z", N));
        assert!(m("[[:alpha:][:digit:]]", "q", N));
        assert!(m("[![:space:]]", "a", N));
        assert!(!m("[![:space:]]", " ", N));
        assert!(m("[[:xdigit:]]", "f", N));
        assert!(m("[[:punct:]]", "!", N));
        assert!(m("[[:upper:]][[:lower:]]", "Ab", N));
        // Classe desconhecida em bytes: consumida, não casa nada.
        assert!(!m("[[:foo:]]", "f", N));
        assert!(!m("[[:foo:]]", "[", N));
    }

    #[test]
    fn escapes() {
        assert!(m("\\*", "*", N));
        assert!(!m("\\*", "x", N));
        assert!(m("a\\*b", "a*b", N));
        assert!(!m("a\\*b", "axb", N));
        assert!(m("\\\\", "\\", N));
        // NOESCAPE: a barra é um caractere comum.
        assert!(m("\\*", "\\x", Flags::NOESCAPE));
        assert!(!m("\\*", "*", Flags::NOESCAPE));
        assert!(m("[\\]", "\\", Flags::NOESCAPE));
    }

    #[test]
    fn trailing_backslash() {
        assert!(!m("a\\", "a\\", N));
        assert!(!m("a\\", "a", N));
        assert!(m("a\\", "a\\", Flags::TRAILING_BACKSLASH_LITERAL));
        assert!(!m("a\\", "a", Flags::TRAILING_BACKSLASH_LITERAL));
        assert!(m("a\\", "a\\", Flags::NOESCAPE));
    }

    #[test]
    fn pathname() {
        assert!(!m("*.txt", "a/b.txt", Flags::PATHNAME));
        assert!(m("*/*.txt", "a/b.txt", Flags::PATHNAME));
        assert!(!m("a?b", "a/b", Flags::PATHNAME));
        assert!(m("a?b", "a/b", N));
        assert!(!m("a[/]b", "a/b", Flags::PATHNAME));
        assert!(!m("a[!x]b", "a/b", Flags::PATHNAME));
        assert!(m("a/*", "a/b", Flags::PATHNAME));
        assert!(!m("a/*", "a/b/c", Flags::PATHNAME));
        // O `*` do começo precisa ceder pra o resto alinhar nas barras: exige retrocesso nos dois.
        assert!(m("*a/*b", "xa/yab", Flags::PATHNAME));
        assert!(m("*a/*a/b", "xa/ya/b", Flags::PATHNAME));
    }

    #[test]
    fn leading_dir() {
        assert!(m("a", "a/b/c", Flags::LEADING_DIR));
        assert!(!m("a", "ab", Flags::LEADING_DIR));
        assert!(m("a", "a", Flags::LEADING_DIR));
        assert!(!m("a", "a/b/c", N));
        assert!(m("a*", "a/b/c", Flags::LEADING_DIR | Flags::PATHNAME));
        assert!(m("a/b", "a/b/c", Flags::LEADING_DIR | Flags::PATHNAME));
        assert!(!m("a/c", "a/b/c", Flags::LEADING_DIR | Flags::PATHNAME));
    }

    #[test]
    fn casefold() {
        assert!(m("ABC", "abc", Flags::CASEFOLD));
        assert!(!m("ABC", "abc", N));
        assert!(m("[A-C]x", "bX", Flags::CASEFOLD));
        assert!(m("[a-c]x", "BX", Flags::CASEFOLD));
        assert!(m("\\A", "a", Flags::CASEFOLD));
        assert!(m("[!a]", "b", Flags::CASEFOLD));
        assert!(!m("[!a]", "A", Flags::CASEFOLD));
        assert!(mu("ÉCOLE", "école", Flags::CASEFOLD));
    }

    #[test]
    fn period() {
        assert!(!m("*", ".a", Flags::PERIOD));
        assert!(!m("?a", ".a", Flags::PERIOD));
        assert!(!m("[.]a", ".a", Flags::PERIOD));
        assert!(m(".*", ".a", Flags::PERIOD));
        assert!(m("*", "a.b", Flags::PERIOD));
        assert!(m("*", ".a", N));
        // Sem PATHNAME só o começo do texto conta; com PATHNAME, também depois de `/`.
        assert!(m("a/*", "a/.b", Flags::PERIOD));
        assert!(!m("a/*", "a/.b", Flags::PERIOD | Flags::PATHNAME));
        assert!(m("a/.*", "a/.b", Flags::PERIOD | Flags::PATHNAME));
    }

    #[test]
    fn plain_bracket_like_tree() {
        let t = Flags::PLAIN_BRACKET | Flags::TRAILING_BACKSLASH_LITERAL;
        // Sem classes: a lista `[[:digit:]` fecha no primeiro `]`, e o `]` seguinte é literal.
        assert!(m("[[:digit:]]", "d]", t));
        assert!(!m("[[:digit:]]", "5]", t));
        // Sem escape dentro da lista: `[\]` é a lista de uma barra, e o `]` depois é literal.
        assert!(m("[\\]]", "\\]", t));
        assert!(m("[a-c]x", "bx", t));
        assert!(m("[^a-c]x", "dx", t));
        assert!(m("[a-]", "-", t));
        assert!(m("[A-C]x", "bx", t | Flags::CASEFOLD));
        assert!(m("[", "[", t));
        assert!(m("a\\*", "a*", t));
        assert!(m("a\\", "a\\", t));
    }

    #[test]
    fn utf8_chars() {
        assert!(mu("?s", "ás", N));
        assert!(!m("?s", "ás", N));
        assert!(m("??s", "ás", N));
        assert!(mu("[[:alpha:]]", "é", N));
        assert!(!m("[[:alpha:]]", "\u{e9}", N));
        assert!(mu("[à-ü]", "é", N));
        assert!(mu("a\\*b", "a*b", N));
        // Classe desconhecida em caracteres: o `[` vira item comum da lista.
        assert!(mu("[[:foo:]]", "f]", N));
        assert!(mu("[[:foo:]]", "[]", N));
        assert!(!mu("[[:foo:]]", "f", N));
        // Byte inválido vira U+FFFD.
        assert!(fnmatch_utf8(b"a?b", b"a\xffb", N));
    }

    #[test]
    fn no_exponential_blowup() {
        let text = "a".repeat(60);
        assert!(!m("*a*a*a*a*a*a*a*a*b", &text, N));
        assert!(!m("*a*a*a*a*a*a*a*a*b", &text, Flags::PATHNAME));
        assert!(m("*a*a*a*a*a*a*a*a*", &text, N));
    }

    #[test]
    fn wildcard_detection() {
        assert!(has_wildcards(b"a*"));
        assert!(has_wildcards(b"a?"));
        assert!(has_wildcards(b"[a]"));
        assert!(has_wildcards(b"a\\b"));
        assert!(!has_wildcards(b"abc.txt"));
    }

    #[test]
    fn flags_builder() {
        let f = Flags::NONE.with(Flags::PATHNAME, true).with(Flags::CASEFOLD, false);
        assert!(f.contains(Flags::PATHNAME));
        assert!(!f.contains(Flags::CASEFOLD));
        assert_eq!(Flags::PATHNAME | Flags::PERIOD, Flags::NONE.with(Flags::PERIOD, true).with(Flags::PATHNAME, true));
    }
}
