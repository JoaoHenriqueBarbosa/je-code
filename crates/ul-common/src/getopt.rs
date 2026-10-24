//! `getopt_long` da glibc 2.41, com as mesmas regras e as mesmas mensagens.
//!
//! - Permutação por padrão: operandos podem vir antes, entre ou depois das opções e voltam na ordem
//!   original. Com `+` no começo da especificação curta, ou com `POSIXLY_CORRECT` no ambiente, a
//!   varredura para no primeiro operando (REQUIRE_ORDER).
//! - `--` encerra as opções; `-` sozinho é operando.
//! - Opções curtas agrupadas (`-bL`), argumento colado (`-F:`) ou no próximo argv (`-F :`); `::`
//!   marca argumento opcional, que só vale colado.
//! - Opções longas por prefixo único; prefixo de várias opções equivalentes (mesmo `has_arg` e mesmo
//!   id) escolhe a primeira, como a glibc; senão é ambíguo e a mensagem lista a primeira e as que
//!   diferem dela, na ordem da tabela.
//! - Sem tabela de opções longas é o `getopt(3)` puro: `--help` são as curtas `-`, `h`, `e`, ...
//! - As mensagens usam o `argv[0]` exatamente como veio (a glibc não tira o diretório) e carregam o
//!   byte da opção inválida sem passar por UTF-8 (ver [`GetoptError::message_bytes`]).
//!
//! Dois jeitos de consumir: [`Getopt::next_opt`] devolve só as opções e junta os operandos em
//! [`Getopt::operands`]; o `Iterator` devolve [`Item`], opções e operandos na ordem em que aparecem,
//! pra quem precisa saber o que veio antes de quê (o `tar -C`, por exemplo). Quem passa o argv
//! inteiro (com o `argv[0]`) liga [`Getopt::after_argv0`].

use std::fmt;

/// Se a opção leva argumento.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HasArg {
    No,
    Required,
    /// Só na forma colada: `-xVALOR` ou `--nome=VALOR`.
    Optional,
}

/// Uma entrada da tabela de opções longas. A ordem da tabela importa: é a ordem em que a glibc lista
/// as possibilidades de uma abreviação ambígua.
#[derive(Copy, Clone, Debug)]
pub struct LongOpt {
    pub name: &'static str,
    pub has_arg: HasArg,
    /// O que a varredura devolve em [`Opt::id`] quando a opção casa: o código de uma opção curta
    /// (`'b' as i32`) ou um número próprio acima de 255 pra opções só longas.
    pub id: i32,
}

impl LongOpt {
    pub const fn new(name: &'static str, has_arg: HasArg, id: i32) -> LongOpt {
        LongOpt { name, has_arg, id }
    }
}

/// Uma opção reconhecida.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opt {
    /// Caractere da curta (`'u' as i32`) ou o `id` da longa.
    pub id: i32,
    pub arg: Option<Vec<u8>>,
    /// Índice em `args` do elemento em que a opção apareceu (o argumento separado, quando há, está em
    /// `index + 1`).
    pub index: usize,
    /// Nome da longa como está na tabela; `None` pra curta.
    pub long: Option<&'static str>,
}

impl Opt {
    /// O id como `char`, quando é uma opção curta.
    pub fn short(&self) -> Option<char> {
        u8::try_from(self.id).ok().map(char::from)
    }

    pub fn arg_str(&self) -> String {
        self.arg
            .as_deref()
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .unwrap_or_default()
    }

    /// Como a opção apareceu (`-b`, `--brief`), pra mensagens dos programas.
    pub fn spelled(&self) -> String {
        match self.long {
            Some(name) => format!("--{name}"),
            None => format!("-{}", self.short().unwrap_or('?')),
        }
    }
}

/// Item devolvido pelo `Iterator`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Opt(Opt),
    Operand(Vec<u8>),
}

/// Erro de `getopt`, com o texto da glibc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GetoptError {
    /// `invalid option -- 'x'`
    Invalid(u8),
    /// `unrecognized option '--foo=bar'` (o texto inteiro como veio, com `--` e `=valor`).
    Unrecognized(Vec<u8>),
    /// `option requires an argument -- 'x'`
    MissingShort(u8),
    /// `option '--foo' requires an argument` (o nome da tabela, sem `--`).
    MissingLong(&'static str),
    /// `option '--foo' doesn't allow an argument` (o nome da tabela, sem `--`).
    NoArgAllowed(&'static str),
    /// `option '--fo' is ambiguous; possibilities: '--foo' '--fob'`
    Ambiguous {
        /// O texto inteiro como veio, com `--` e `=valor`.
        given: Vec<u8>,
        /// Nomes da tabela, sem `--`.
        candidates: Vec<&'static str>,
    },
}

impl GetoptError {
    /// A frase da glibc sem o prefixo `argv0: ` e sem o `\n`, em bytes (a opção inválida pode não ser
    /// UTF-8).
    pub fn detail(&self) -> Vec<u8> {
        let mut m = Vec::new();
        match self {
            GetoptError::Invalid(c) => {
                m.extend_from_slice(b"invalid option -- '");
                m.push(*c);
                m.push(b'\'');
            }
            GetoptError::Unrecognized(text) => {
                m.extend_from_slice(b"unrecognized option '");
                m.extend_from_slice(text);
                m.push(b'\'');
            }
            GetoptError::MissingShort(c) => {
                m.extend_from_slice(b"option requires an argument -- '");
                m.push(*c);
                m.push(b'\'');
            }
            GetoptError::MissingLong(name) => {
                m.extend_from_slice(format!("option '--{name}' requires an argument").as_bytes());
            }
            GetoptError::NoArgAllowed(name) => {
                m.extend_from_slice(format!("option '--{name}' doesn't allow an argument").as_bytes());
            }
            GetoptError::Ambiguous { given, candidates } => {
                m.extend_from_slice(b"option '");
                m.extend_from_slice(given);
                m.extend_from_slice(b"' is ambiguous; possibilities:");
                for c in candidates {
                    m.extend_from_slice(format!(" '--{c}'").as_bytes());
                }
            }
        }
        m
    }

    /// A linha que a glibc escreve no stderr, sem o `\n`, em bytes.
    pub fn message_bytes(&self, argv0: &str) -> Vec<u8> {
        let mut m = format!("{argv0}: ").into_bytes();
        m.extend_from_slice(&self.detail());
        m
    }

    /// [`GetoptError::message_bytes`] com o `\n` final, pronta pro stderr.
    pub fn message_line(&self, argv0: &str) -> Vec<u8> {
        let mut m = self.message_bytes(argv0);
        m.push(b'\n');
        m
    }

    /// A linha da glibc sem o `\n`, como texto (byte fora do UTF-8 vira U+FFFD).
    pub fn message(&self, argv0: &str) -> String {
        String::from_utf8_lossy(&self.message_bytes(argv0)).into_owned()
    }

    /// A opção curta envolvida (o `optopt` da glibc), quando há.
    pub fn optopt(&self) -> Option<char> {
        match self {
            GetoptError::Invalid(c) | GetoptError::MissingShort(c) => Some(char::from(*c)),
            _ => None,
        }
    }
}

impl fmt::Display for GetoptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message("getopt"))
    }
}

/// Varredura de `args` no estilo `getopt_long`.
pub struct Getopt<'a> {
    args: Vec<Vec<u8>>,
    shorts: Vec<(u8, HasArg)>,
    longs: &'a [LongOpt],
    require_order: bool,
    /// Próximo índice de `args` a examinar.
    idx: usize,
    /// Resto de um agrupamento de curtas em curso: (índice do elemento, posição do próximo byte).
    cluster: Option<(usize, usize)>,
    /// Depois de `--` (ou do primeiro operando com `require_order`): tudo é operando.
    only_operands: bool,
    /// Operandos que [`Getopt::next_opt`] já pulou.
    operands: Vec<Vec<u8>>,
}

impl<'a> Getopt<'a> {
    /// `args` sem o `argv[0]` (veja [`Getopt::after_argv0`] pra passar o argv inteiro). `spec` é a
    /// string de opções curtas do `getopt` (`"bF:i::"`); `+` no começo (ou `posixly_correct`, de
    /// `POSIXLY_CORRECT` no ambiente) liga o modo REQUIRE_ORDER, e `-` ou `:` no começo são aceitos
    /// e ignorados.
    pub fn new(args: &[Vec<u8>], spec: &str, longs: &'a [LongOpt], posixly_correct: bool) -> Getopt<'a> {
        let mut spec = spec.as_bytes();
        let mut require_order = posixly_correct;
        while let Some((&c, rest)) = spec.split_first() {
            match c {
                b'+' => require_order = true,
                b'-' | b':' => {}
                _ => break,
            }
            spec = rest;
        }
        let mut shorts = Vec::new();
        let mut i = 0;
        while i < spec.len() {
            let c = spec[i];
            let mut kind = HasArg::No;
            if spec.get(i + 1) == Some(&b':') {
                kind = HasArg::Required;
                i += 1;
                if spec.get(i + 1) == Some(&b':') {
                    kind = HasArg::Optional;
                    i += 1;
                }
            }
            shorts.push((c, kind));
            i += 1;
        }
        Getopt {
            args: args.to_vec(),
            shorts,
            longs,
            require_order,
            idx: 0,
            cluster: None,
            only_operands: false,
            operands: Vec::new(),
        }
    }

    /// Atalho que lê `POSIXLY_CORRECT` do ambiente do processo corrente.
    pub fn from_env(args: &[Vec<u8>], spec: &str, longs: &'a [LongOpt]) -> Getopt<'a> {
        let posix =
            sysabi::sys::try_current().is_some_and(|s| s.getenv(b"POSIXLY_CORRECT").is_some());
        Getopt::new(args, spec, longs, posix)
    }

    /// Pra quem passa o argv inteiro: a varredura começa no índice 1 e o `argv[0]` não vira operando.
    pub fn after_argv0(mut self) -> Getopt<'a> {
        self.idx = 1;
        self
    }

    fn short_kind(&self, c: u8) -> Option<HasArg> {
        if c == b':' {
            return None;
        }
        self.shorts.iter().find(|(s, _)| *s == c).map(|(_, k)| *k)
    }

    /// O próximo elemento de `args` como valor de uma opção que o pede.
    fn take_value(&mut self) -> Option<Vec<u8>> {
        let value = self.args.get(self.idx)?.clone();
        self.idx += 1;
        Some(value)
    }

    /// Próxima opção; `None` quando acabaram (aí [`Getopt::operands`] tem os operandos).
    pub fn next_opt(&mut self) -> Option<Result<Opt, GetoptError>> {
        loop {
            match self.next()? {
                Ok(Item::Operand(operand)) => self.operands.push(operand),
                Ok(Item::Opt(opt)) => return Some(Ok(opt)),
                Err(e) => return Some(Err(e)),
            }
        }
    }

    fn short(&mut self, index: usize, pos: usize) -> Result<Opt, GetoptError> {
        let arg = self.args[index].clone();
        let c = arg[pos];
        let rest_start = pos + 1;
        let has_rest = rest_start < arg.len();
        self.cluster = has_rest.then_some((index, rest_start));
        let Some(kind) = self.short_kind(c) else {
            return Err(GetoptError::Invalid(c));
        };
        let value = match kind {
            HasArg::No => None,
            HasArg::Optional => {
                self.cluster = None;
                has_rest.then(|| arg[rest_start..].to_vec())
            }
            HasArg::Required => {
                self.cluster = None;
                if has_rest {
                    Some(arg[rest_start..].to_vec())
                } else {
                    Some(self.take_value().ok_or(GetoptError::MissingShort(c))?)
                }
            }
        };
        Ok(Opt { id: i32::from(c), arg: value, index, long: None })
    }

    /// `arg` é o elemento `index` de `args`, que começa com `--` e tem algo depois.
    fn long(&mut self, index: usize, arg: &[u8]) -> Result<Opt, GetoptError> {
        let body = &arg[2..];
        let (name, value) = match body.iter().position(|&b| b == b'=') {
            Some(p) => (&body[..p], Some(body[p + 1..].to_vec())),
            None => (body, None),
        };
        let found = match self.longs.iter().find(|l| l.name.as_bytes() == name) {
            Some(l) => *l,
            None => {
                let mut matches = self.longs.iter().filter(|l| l.name.as_bytes().starts_with(name));
                let Some(first) = matches.next() else {
                    return Err(GetoptError::Unrecognized(arg.to_vec()));
                };
                let differing: Vec<&LongOpt> =
                    matches.filter(|l| l.has_arg != first.has_arg || l.id != first.id).collect();
                if !differing.is_empty() {
                    let candidates = std::iter::once(first).chain(differing).map(|l| l.name).collect();
                    return Err(GetoptError::Ambiguous { given: arg.to_vec(), candidates });
                }
                if name.is_empty() {
                    return Err(GetoptError::Unrecognized(arg.to_vec()));
                }
                *first
            }
        };
        let value = match (found.has_arg, value) {
            (HasArg::No, Some(_)) => return Err(GetoptError::NoArgAllowed(found.name)),
            (HasArg::No, None) => None,
            (HasArg::Optional, v) => v,
            (HasArg::Required, Some(v)) => Some(v),
            (HasArg::Required, None) => {
                Some(self.take_value().ok_or(GetoptError::MissingLong(found.name))?)
            }
        };
        Ok(Opt { id: found.id, arg: value, index, long: Some(found.name) })
    }

    /// Operandos na ordem original. Chame depois que [`Getopt::next_opt`] devolver `None`; se a
    /// varredura parou num erro, inclui o que ainda não foi examinado.
    pub fn operands(&self) -> Vec<Vec<u8>> {
        let mut out = self.operands.clone();
        out.extend(self.args[self.idx.min(self.args.len())..].iter().cloned());
        out
    }

    /// Índice (em `args`) do próximo argumento ainda não consumido: o `optind - 1` da glibc, sem a
    /// permutação.
    pub fn index(&self) -> usize {
        self.idx
    }
}

impl Iterator for Getopt<'_> {
    type Item = Result<Item, GetoptError>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((index, pos)) = self.cluster {
            return Some(self.short(index, pos).map(Item::Opt));
        }
        let index = self.idx;
        let arg = self.args.get(index)?.clone();
        self.idx += 1;
        if self.only_operands {
            return Some(Ok(Item::Operand(arg)));
        }
        if arg == b"--" {
            self.only_operands = true;
            return self.next();
        }
        // Sem tabela de longas é o getopt(3) puro: `--help` são as curtas `-`, `h`, ...
        if arg.len() > 2 && arg.starts_with(b"--") && !self.longs.is_empty() {
            return Some(self.long(index, &arg).map(Item::Opt));
        }
        if arg.len() > 1 && arg[0] == b'-' {
            return Some(self.short(index, 1).map(Item::Opt));
        }
        if self.require_order {
            self.only_operands = true;
        }
        Some(Ok(Item::Operand(arg)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<Vec<u8>> {
        args.iter().map(|a| a.as_bytes().to_vec()).collect()
    }

    const LONGS: &[LongOpt] = &[
        LongOpt::new("mime", HasArg::No, 'i' as i32),
        LongOpt::new("mime-type", HasArg::No, 300),
        LongOpt::new("mime-encoding", HasArg::No, 301),
        LongOpt::new("separator", HasArg::Required, 'F' as i32),
        LongOpt::new("brief", HasArg::No, 'b' as i32),
        LongOpt::new("color", HasArg::Optional, 302),
        LongOpt::new("colour", HasArg::Optional, 302),
        LongOpt::new("count", HasArg::No, 'c' as i32),
        LongOpt::new("context", HasArg::Required, 'C' as i32),
    ];

    fn collect(args: &[&str], spec: &str) -> (Vec<Result<Opt, GetoptError>>, Vec<Vec<u8>>) {
        let a = v(args);
        let mut g = Getopt::new(&a, spec, LONGS, false);
        let mut out = Vec::new();
        while let Some(r) = g.next_opt() {
            let stop = r.is_err();
            out.push(r);
            if stop {
                break;
            }
        }
        let ops = g.operands();
        (out, ops)
    }

    fn first_error(args: &[&str], spec: &str) -> String {
        let (opts, _) = collect(args, spec);
        opts.into_iter().find_map(Result::err).unwrap().message("x")
    }

    #[test]
    fn permutes_and_clusters() {
        let (opts, ops) = collect(&["x", "-bF:", "y", "--brief", "--", "-z"], "bF:");
        let ids: Vec<i32> = opts.iter().map(|o| o.as_ref().unwrap().id).collect();
        assert_eq!(ids, vec!['b' as i32, 'F' as i32, 'b' as i32]);
        assert_eq!(opts[1].as_ref().unwrap().arg.as_deref(), Some(&b":"[..]));
        assert_eq!(ops, v(&["x", "y", "-z"]));
    }

    #[test]
    fn require_order_stops_at_first_operand() {
        let (opts, ops) = collect(&["-b", "x", "-b"], "+b");
        assert_eq!(opts.len(), 1);
        assert_eq!(ops, v(&["x", "-b"]));
        let a = v(&["x", "-b"]);
        let mut g = Getopt::new(&a, "b", LONGS, true);
        assert!(g.next_opt().is_none());
        assert_eq!(g.operands(), a);
    }

    #[test]
    fn long_prefixes_and_errors() {
        let (opts, _) = collect(&["--sep", ","], "");
        assert_eq!(opts[0].as_ref().unwrap().arg.as_deref(), Some(&b","[..]));
        let (opts, _) = collect(&["--mim"], "");
        assert_eq!(
            opts[0].as_ref().unwrap_err().message("file"),
            "file: option '--mim' is ambiguous; possibilities: '--mime' '--mime-type' '--mime-encoding'"
        );
        assert_eq!(first_error(&["--bogus=1"], ""), "x: unrecognized option '--bogus=1'");
        assert_eq!(first_error(&["--separator"], ""), "x: option '--separator' requires an argument");
        assert_eq!(first_error(&["--br=1"], ""), "x: option '--brief' doesn't allow an argument");
        assert_eq!(first_error(&["-Z"], "b"), "x: invalid option -- 'Z'");
        assert_eq!(first_error(&["-F"], "F:"), "x: option requires an argument -- 'F'");
    }

    #[test]
    fn equivalent_prefix_picks_first_and_ambiguity_lists_the_differing() {
        // `color` e `colour` são a mesma opção: o prefixo escolhe a primeira.
        let (opts, _) = collect(&["--col=always", "--cou", "--con", "3"], "");
        assert_eq!(opts[0].as_ref().unwrap().long, Some("color"));
        assert_eq!(opts[0].as_ref().unwrap().arg.as_deref(), Some(&b"always"[..]));
        assert_eq!(opts[1].as_ref().unwrap().id, 'c' as i32);
        assert_eq!(opts[2].as_ref().unwrap().arg.as_deref(), Some(&b"3"[..]));
        // `co` casa com todas; a lista traz a primeira e as que diferem dela (`colour` não entra).
        assert_eq!(
            first_error(&["--co"], ""),
            "x: option '--co' is ambiguous; possibilities: '--color' '--count' '--context'"
        );
    }

    #[test]
    fn optional_arguments_only_attached() {
        let (opts, ops) = collect(&["--color", "x", "--color=never", "-ifoo"], "i::");
        assert_eq!(opts[0].as_ref().unwrap().arg, None);
        assert_eq!(opts[1].as_ref().unwrap().arg.as_deref(), Some(&b"never"[..]));
        assert_eq!(opts[2].as_ref().unwrap().arg.as_deref(), Some(&b"foo"[..]));
        assert_eq!(ops, v(&["x"]));
    }

    #[test]
    fn dash_alone_is_operand() {
        let (opts, ops) = collect(&["-", "-b"], "b");
        assert_eq!(opts.len(), 1);
        assert_eq!(ops, v(&["-"]));
    }

    #[test]
    fn without_long_table_double_dash_words_are_short_options() {
        let a = v(&["--help"]);
        let mut g = Getopt::new(&a, "h", &[], false);
        let e = g.next_opt().unwrap().unwrap_err();
        assert_eq!(e, GetoptError::Invalid(b'-'));
        assert_eq!(e.optopt(), Some('-'));
    }

    #[test]
    fn scan_continues_after_an_error_inside_a_cluster() {
        let a = v(&["-zb", "x"]);
        let mut g = Getopt::new(&a, "b", LONGS, false);
        assert_eq!(g.next_opt().unwrap().unwrap_err(), GetoptError::Invalid(b'z'));
        assert_eq!(g.next_opt().unwrap().unwrap().id, 'b' as i32);
        assert!(g.next_opt().is_none());
        assert_eq!(g.operands(), v(&["x"]));
    }

    #[test]
    fn operands_after_an_error_include_what_was_not_examined() {
        let a = v(&["x", "-Z", "y"]);
        let mut g = Getopt::new(&a, "b", LONGS, false);
        assert!(g.next_opt().unwrap().is_err());
        assert_eq!(g.operands(), v(&["x", "y"]));
        assert_eq!(g.index(), 2);
    }

    #[test]
    fn items_keep_operands_in_place_and_skip_argv0() {
        let a = v(&["prog", "a", "-quU5", "--label=x", "--label", "y", "b", "--", "-c"]);
        let longs = [LongOpt::new("label", HasArg::Required, 300)];
        let items: Vec<Item> =
            Getopt::new(&a, "quU:y", &longs, false).after_argv0().map(Result::unwrap).collect();
        let opt = |id: i32, arg: Option<&str>, index: usize, long| {
            Item::Opt(Opt { id, arg: arg.map(|s| s.as_bytes().to_vec()), index, long })
        };
        assert_eq!(
            items,
            vec![
                Item::Operand(b"a".to_vec()),
                opt('q' as i32, None, 2, None),
                opt('u' as i32, None, 2, None),
                opt('U' as i32, Some("5"), 2, None),
                opt(300, Some("x"), 3, Some("label")),
                opt(300, Some("y"), 4, Some("label")),
                Item::Operand(b"b".to_vec()),
                Item::Operand(b"-c".to_vec()),
            ]
        );
    }

    #[test]
    fn items_stop_at_first_operand_in_require_order() {
        let a = v(&["x", "a", "-q"]);
        let items: Vec<Item> =
            Getopt::new(&a, "q", LONGS, true).after_argv0().map(Result::unwrap).collect();
        assert_eq!(items, vec![Item::Operand(b"a".to_vec()), Item::Operand(b"-q".to_vec())]);
    }

    #[test]
    fn messages_carry_raw_bytes_and_a_trailing_newline_on_request() {
        let e = GetoptError::Invalid(0xc3);
        assert_eq!(e.message_bytes("prog"), b"prog: invalid option -- '\xc3'".to_vec());
        assert_eq!(e.message_line("prog"), b"prog: invalid option -- '\xc3'\n".to_vec());
        assert_eq!(e.detail(), b"invalid option -- '\xc3'".to_vec());
    }

    #[test]
    fn spelled_and_short() {
        let (opts, _) = collect(&["-b", "--brief", "--mime-type"], "b");
        let spelled: Vec<String> = opts.iter().map(|o| o.as_ref().unwrap().spelled()).collect();
        assert_eq!(spelled, ["-b", "--brief", "--mime-type"]);
        assert_eq!(opts[2].as_ref().unwrap().short(), None);
    }
}
