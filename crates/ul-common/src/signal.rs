//! Nomes de sinal e a leitura de `HUP`, `SIGHUP`, `term`, `RTMIN+3`.
//!
//! A tabela de nomes é uma só: a que o `sysabi` gera do Debian real (`Signal::name`). Cada programa do
//! Debian lê o nome do seu jeito (com ou sem `SIG`, qualquer caixa ou só maiúsculas, o 29 como `IO` ou
//! `POLL`, apelidos, tempo real), e uma [`Table`] descreve esse jeito sem copiar nome nenhum.
//!
//! Só o nome mora aqui. O que cada programa faz com um número (faixa aceita, sinal à frente do
//! número, saturação) é decisão do chamador, porque é aí que os programas divergem; [`parse_decimal`]
//! cobre o caso comum de "só dígitos".

use sysabi::Signal;
use sysabi::linux::{SIGRTMAX, SIGRTMIN};

/// Último sinal padrão (os de 1 a 31, que não são de tempo real).
pub const LAST_STANDARD: i32 = 31;

/// Como a tabela chama o sinal 29: a glibc diz `IO`, o procps e o psmisc dizem `POLL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sig29 {
    Io,
    Poll,
}

/// Caixa do nome (e do prefixo `SIG`) que o programa aceita.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    /// Só maiúsculas: `hup` e `sigHUP` não casam.
    Exact,
    /// Qualquer caixa: `hup`, `SigHup`.
    Any,
}

/// O jeito que um programa lê o nome de um sinal padrão (1 a 31). O prefixo `SIG` é sempre
/// opcional e tirado uma vez só.
#[derive(Clone, Copy, Debug)]
pub struct Table {
    /// Nome do 29 na tabela.
    pub sig29: Sig29,
    /// Caixa aceita.
    pub case: Case,
    /// Nomes a mais que o programa aceita (`IOT`, `CLD`, o outro nome do 29...), sem o `SIG`.
    pub aliases: &'static [(&'static str, i32)],
}

/// Como o psmisc (`killall`, `fuser -l`), o sysvinit (`killall5`) e o dpkg (`start-stop-daemon`) leem:
/// nome em maiúsculas, `SIG` opcional, 29 é `POLL`, sem apelidos.
pub const EXACT_POLL: Table = Table { sig29: Sig29::Poll, case: Case::Exact, aliases: &[] };

/// Nome do sinal padrão `n` (1 a 31) sem o `SIG`, ou `None` fora dessa faixa.
pub fn standard_name(n: i32, sig29: Sig29) -> Option<String> {
    if !(1..=LAST_STANDARD).contains(&n) {
        return None;
    }
    if n == 29 && sig29 == Sig29::Poll {
        return Some("POLL".to_string());
    }
    Signal(n).name()
}

/// Os nomes dos sinais 1 a 31, na ordem dos números.
pub fn standard_names(sig29: Sig29) -> Vec<String> {
    (1..=LAST_STANDARD).filter_map(|n| standard_name(n, sig29)).collect()
}

/// Tira `word` do começo de `s` (na caixa pedida).
fn strip_word<'a>(s: &'a [u8], word: &[u8], case: Case) -> Option<&'a [u8]> {
    let (head, tail) = s.split_at_checked(word.len())?;
    let same = match case {
        Case::Exact => head == word,
        Case::Any => head.eq_ignore_ascii_case(word),
    };
    same.then_some(tail)
}

/// Tira um `SIG` do começo, se houver.
fn strip_sig(s: &[u8], case: Case) -> &[u8] {
    strip_word(s, b"SIG", case).unwrap_or(s)
}

/// Número do sinal padrão chamado `s` (`HUP`, `SIGHUP`...) segundo a [`Table`]; `None` se não é nome
/// de sinal padrão. Não olha número nem tempo real.
pub fn parse_name(s: &[u8], table: &Table) -> Option<i32> {
    let bare = strip_sig(s, table.case);
    let same = |name: &str| match table.case {
        Case::Exact => name.as_bytes() == bare,
        Case::Any => name.as_bytes().eq_ignore_ascii_case(bare),
    };
    (1..=LAST_STANDARD)
        .find(|&n| standard_name(n, table.sig29).is_some_and(|name| same(&name)))
        .or_else(|| table.aliases.iter().find(|(name, _)| same(name)).map(|&(_, n)| n))
}

/// Sinal de tempo real `RTMIN`, `RTMIN+n`, `RTMAX`, `RTMAX-n` (com ou sem `SIG`), como o `signal_rt`
/// do procps: o deslocamento é qualquer inteiro do Rust (`RTMIN3` vale `RTMIN+3`, `RTMAX-0` vale
/// `RTMAX`) e o resultado tem que cair entre `SIGRTMIN` e `SIGRTMAX`. A caixa vale pro `SIG` e pro
/// nome base.
pub fn parse_realtime(s: &[u8], case: Case) -> Option<i32> {
    let s = strip_sig(s, case);
    let (base, rest) = match strip_word(s, b"RTMIN", case) {
        Some(rest) => (SIGRTMIN, rest),
        None => (SIGRTMAX, strip_word(s, b"RTMAX", case)?),
    };
    if rest.is_empty() {
        return Some(base);
    }
    let offset: i32 = std::str::from_utf8(rest).ok()?.parse().ok()?;
    let n = base.checked_add(offset)?;
    (SIGRTMIN..=SIGRTMAX).contains(&n).then_some(n)
}

/// Só dígitos decimais (nenhum sinal, nenhum espaço) cabendo num `i32`.
pub fn parse_decimal(s: &[u8]) -> Option<i32> {
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse().ok()
}

/// O que o bash aceita em `kill -SINAL`, `kill -s`, `trap` e `kill -l`: só dígitos valem se são o
/// número de um sinal que tem nome (1 a 31, 34 a 64); senão o nome (`TERM`, `SIGTERM`, `term`,
/// `RTMIN+1`, mais `IOT`, `POLL` e `CLD`) ou um inteiro com sinal `+` à frente, pela `Signal::parse`.
/// O 0 nunca é sinal aqui.
pub fn parse_shell(s: &[u8]) -> Option<Signal> {
    let text = String::from_utf8_lossy(s);
    let sig = match parse_decimal(text.as_bytes()) {
        Some(n) => return (1..=SIGRTMAX).contains(&n).then_some(Signal(n)).filter(|s| s.name().is_some()),
        None => Signal::parse(&text)?,
    };
    (sig.0 > 0).then_some(sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IO_ANY: Table = Table { sig29: Sig29::Io, case: Case::Any, aliases: &[] };
    const PROCPS_LIKE: Table =
        Table { sig29: Sig29::Poll, case: Case::Any, aliases: &[("IO", 29), ("IOT", 6), ("CLD", 17)] };
    const UNSHARE_LIKE: Table = Table { sig29: Sig29::Io, case: Case::Any, aliases: &[("IOT", 6), ("POLL", 29)] };

    #[test]
    fn standard_names_follow_the_glibc_table() {
        assert_eq!(standard_name(1, Sig29::Io).as_deref(), Some("HUP"));
        assert_eq!(standard_name(16, Sig29::Io).as_deref(), Some("STKFLT"));
        assert_eq!(standard_name(29, Sig29::Io).as_deref(), Some("IO"));
        assert_eq!(standard_name(29, Sig29::Poll).as_deref(), Some("POLL"));
        assert_eq!(standard_name(31, Sig29::Poll).as_deref(), Some("SYS"));
        for n in [i32::MIN, -1, 0, 32, 34, 64, 65] {
            assert_eq!(standard_name(n, Sig29::Io), None);
        }
        let names = standard_names(Sig29::Poll);
        assert_eq!(names.len(), 31);
        assert_eq!(names[28], "POLL");
        assert_eq!(names[8], "KILL");
    }

    #[test]
    fn exact_mode_wants_uppercase_and_one_optional_sig() {
        let p = |s: &str| parse_name(s.as_bytes(), &EXACT_POLL);
        assert_eq!(p("HUP"), Some(1));
        assert_eq!(p("SIGHUP"), Some(1));
        assert_eq!(p("POLL"), Some(29));
        assert_eq!(p("SIGSYS"), Some(31));
        for s in ["hup", "sighup", "SigHUP", "sigHUP", "IO", "IOT", "CLD", "SIGSIGHUP", "SIG", "", "RTMIN", "1", "HUP "] {
            assert_eq!(p(s), None, "{s:?}");
        }
        assert_eq!(parse_name(b"\xff", &EXACT_POLL), None);
    }

    #[test]
    fn any_mode_ignores_case_and_honours_the_alias_list() {
        let io = |s: &str| parse_name(s.as_bytes(), &IO_ANY);
        assert_eq!(io("sigkill"), Some(9));
        assert_eq!(io("Term"), Some(15));
        assert_eq!(io("IO"), Some(29));
        assert_eq!(io("POLL"), None);
        assert_eq!(io("IOT"), None);
        assert_eq!(io("SIGSIGKILL"), None);

        let pr = |s: &str| parse_name(s.as_bytes(), &PROCPS_LIKE);
        assert_eq!(pr("POLL"), Some(29));
        assert_eq!(pr("io"), Some(29));
        assert_eq!(pr("sigiot"), Some(6));
        assert_eq!(pr("Cld"), Some(17));
        assert_eq!(pr("SIGCHLD"), Some(17));

        let un = |s: &str| parse_name(s.as_bytes(), &UNSHARE_LIKE);
        assert_eq!(un("IO"), Some(29));
        assert_eq!(un("poll"), Some(29));
        assert_eq!(un("IOT"), Some(6));
        assert_eq!(un("CLD"), None);
    }

    #[test]
    fn realtime_any_case_takes_any_offset_inside_the_range() {
        let r = |s: &str| parse_realtime(s.as_bytes(), Case::Any);
        assert_eq!(r("RTMIN"), Some(34));
        assert_eq!(r("RTMAX"), Some(64));
        assert_eq!(r("RTMIN+2"), Some(36));
        assert_eq!(r("rtmax-1"), Some(63));
        assert_eq!(r("SIGRTMIN+1"), Some(35));
        assert_eq!(r("sigrtmax"), Some(64));
        assert_eq!(r("RTMIN+0"), Some(34));
        assert_eq!(r("RTMAX-0"), Some(64));
        assert_eq!(r("RTMIN3"), Some(37));
        assert_eq!(r("RTMIN+30"), Some(64));
        assert_eq!(r("RTMAX-30"), Some(34));
        for s in ["RTMIN-1", "RTMAX+1", "RTMIN+31", "RTMAX-31", "RTMIN+", "RTMINx", "RTMIN+2147483647", "RT", "HUP", ""] {
            assert_eq!(r(s), None, "{s:?}");
        }
    }

    #[test]
    fn realtime_exact_case_needs_the_uppercase_names() {
        let r = |s: &str| parse_realtime(s.as_bytes(), Case::Exact);
        assert_eq!(r("RTMIN+1"), Some(35));
        assert_eq!(r("SIGRTMAX-3"), Some(61));
        assert_eq!(r("rtmin+1"), None);
        assert_eq!(r("sigRTMIN"), None);
        assert_eq!(r("RtMin"), None);
    }

    #[test]
    fn decimal_is_digits_only() {
        assert_eq!(parse_decimal(b"9"), Some(9));
        assert_eq!(parse_decimal(b"0064"), Some(64));
        assert_eq!(parse_decimal(b"2147483647"), Some(i32::MAX));
        for s in ["", "+9", "-1", " 9", "9 ", "9x", "2147483648", "99999999999"] {
            assert_eq!(parse_decimal(s.as_bytes()), None, "{s:?}");
        }
    }

    #[test]
    fn shell_mode_reads_names_and_numbers_like_bash() {
        let p = |s: &str| parse_shell(s.as_bytes()).map(|s| s.0);
        assert_eq!(p("TERM"), Some(15));
        assert_eq!(p("sigterm"), Some(15));
        assert_eq!(p("15"), Some(15));
        assert_eq!(p("+9"), Some(9));
        assert_eq!(p("IO"), Some(29));
        assert_eq!(p("poll"), Some(29));
        assert_eq!(p("iot"), Some(6));
        assert_eq!(p("cld"), Some(17));
        assert_eq!(p("RTMIN"), Some(34));
        assert_eq!(p("RTMIN+1"), Some(35));
        assert_eq!(p("RTMAX-1"), Some(63));
        assert_eq!(p("64"), Some(64));
        // Dígitos só valem pra sinal que tem nome: o 32 e o 33 não têm.
        for s in ["0", "32", "33", "65", "-9", "", "SIG", "RTMIN+0", "RTMAX-0", "HUPS", "99999999999", " 9"] {
            assert_eq!(p(s), None, "{s:?}");
        }
    }
}
