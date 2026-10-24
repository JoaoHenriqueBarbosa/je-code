//! Caminhos, modos de arquivo e tamanhos legíveis: o que os programas repetiam cada um por si.
//!
//! O `basename` que tira as barras finais (o do GNU) mora em `sysabi::util::basename`; aqui ficam as
//! variantes que não são essa: o último componente cru, o diretório pai sem a normalização do
//! `dirname(3)`, o `mkdir -p`, o `rm -rf`, a string de modo do `ls -l` e os formatos de tamanho.

use sysabi::{AtFlags, Errno, Fd, FileType, Mode, sys};

/// O que vem depois da última `/`, sem tratar barras finais: `a/b/` dá vazio, `a` dá `a`.
pub fn after_last_slash(p: &[u8]) -> &[u8] {
    match p.iter().rposition(|&b| b == b'/') {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

/// Diretório pai cru, o que vem antes da última `/` (`/a/b` -> `/a`, `/a` -> `/`, `a` -> vazio).
/// Não tira barras finais nem devolve `.`: `a/` dá `a`.
pub fn dirname(p: &[u8]) -> &[u8] {
    match p.iter().rposition(|&b| b == b'/') {
        Some(0) => b"/",
        Some(i) => &p[..i],
        None => b"",
    }
}

fn is_dir(path: &[u8]) -> bool {
    sys::stat(path).map(|s| s.file_type() == FileType::Directory).unwrap_or(false)
}

/// `mkdir -p`: cria o caminho e os pais que faltam com `mode`. Existindo, o último componente tem
/// de ser diretório (senão `EEXIST`); um pai que existe e não é diretório dá o erro do `mkdir`.
pub fn mkdir_p(path: &[u8], mode: Mode) -> Result<(), Errno> {
    let s = sys::current();
    match s.mkdirat(Fd::CWD, path, mode) {
        Ok(()) => return Ok(()),
        Err(Errno::EEXIST) => return if is_dir(path) { Ok(()) } else { Err(Errno::EEXIST) },
        Err(Errno::ENOENT) => {}
        Err(e) => return Err(e),
    }
    let parent = dirname(path);
    if !parent.is_empty() && parent != path {
        mkdir_p(parent, mode)?;
    }
    match s.mkdirat(Fd::CWD, path, mode) {
        Ok(()) | Err(Errno::EEXIST) => Ok(()),
        Err(e) => Err(e),
    }
}

/// `rm -rf`: remove o caminho e, sendo diretório (sem seguir link), tudo o que ele tem. Caminho que não existe não é
/// erro.
pub fn remove_tree(path: &[u8]) -> Result<(), Errno> {
    let s = sys::current();
    let kind = match sys::lstat(path) {
        Ok(stat) => stat.file_type(),
        Err(Errno::ENOENT) => return Ok(()),
        Err(e) => return Err(e),
    };
    if kind != FileType::Directory {
        return s.unlinkat(Fd::CWD, path, AtFlags::empty());
    }
    for entry in sys::read_dir(path)? {
        remove_tree(&[path, b"/", &entry.name].concat())?;
    }
    s.unlinkat(Fd::CWD, path, AtFlags::REMOVEDIR)
}

/// As nove letras de permissão (`rw-r--r--`), sem tipo e sem setuid/setgid/sticky.
pub fn perm_string(mode: u32) -> String {
    const BITS: [(u32, char); 9] = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    BITS.iter().map(|&(b, c)| if mode & b != 0 { c } else { '-' }).collect()
}

/// `drwxr-xr-x` do `ls -l` (o `strmode`): `kind` é a letra do tipo e setuid/setgid/sticky viram
/// `s`/`S`/`t`/`T` no lugar do `x`.
pub fn mode_string(kind: char, mode: u32) -> String {
    let mut s: Vec<char> = std::iter::once(kind).chain(perm_string(mode).chars()).collect();
    for (bit, idx, set, unset) in [(0o4000, 3, 's', 'S'), (0o2000, 6, 's', 'S'), (0o1000, 9, 't', 'T')] {
        if mode & bit != 0 {
            s[idx] = if s[idx] == 'x' { set } else { unset };
        }
    }
    s.into_iter().collect()
}

/// `size_to_human_string` do util-linux, que arredonda. Sem `one_letter` é o
/// `SIZE_SUFFIX_3LETTER | SIZE_SUFFIX_SPACE`: `512 B`, `1.5 KiB`, `10 MiB`; com ele, o
/// `SIZE_SUFFIX_1LETTER`: `512B`, `1.5K`, `10M`. Com `two_digits` (`SIZE_DECIMAL_2DIGITS`) a fração
/// leva até duas casas (`1.05 KiB`); sem ele, uma só (`1.1 KiB`).
pub fn size_to_human_string(bytes: u64, two_digits: bool, one_letter: bool) -> String {
    // get_exp: o expoente da maior potência de 1024 que não passa de `bytes` (no máximo 60)
    let mut shft = 10u32;
    while shft <= 60 {
        if bytes < (1u64 << shft) {
            break;
        }
        shft += 10;
    }
    let exp = shft - 10;
    let letter = b"BKMGTPE"[if exp != 0 { (exp / 10) as usize } else { 0 }] as char;
    let mut dec = if exp != 0 { bytes / (1u64 << exp) } else { bytes };
    let mut frac = if exp != 0 { bytes % (1u64 << exp) } else { 0 };
    let suffix = match (one_letter, letter) {
        (true, _) => format!("{letter}"),
        (false, 'B') => " B".to_string(),
        (false, _) => format!(" {letter}iB"),
    };
    if frac != 0 {
        // três dígitos depois do ponto
        if frac >= u64::MAX / 1000 {
            frac = ((frac / 1024) * 1000) / (1u64 << (exp - 10));
        } else {
            frac = (frac * 1000) / (1u64 << exp);
        }
        frac = if two_digits { (frac + 5) / 10 } else { ((frac + 50) / 100) * 10 };
        if frac == 100 {
            dec += 1;
            frac = 0;
        }
    }
    if frac == 0 {
        return format!("{dec}{suffix}");
    }
    let mut s = format!("{dec}.{frac:02}");
    if s.ends_with('0') {
        s.pop();
    }
    s.push_str(&suffix);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    use sysabi::testkit::{TestKit, TreeEntry};
    use sysabi::{Ctx, Program};

    #[test]
    fn last_slash_and_dirname() {
        assert_eq!(after_last_slash(b"a/b/c"), b"c");
        assert_eq!(after_last_slash(b"a/b/"), b"");
        assert_eq!(after_last_slash(b"x"), b"x");
        assert_eq!(dirname(b"/a/b"), b"/a");
        assert_eq!(dirname(b"/a"), b"/");
        assert_eq!(dirname(b"a"), b"");
        assert_eq!(dirname(b"a/"), b"a");
    }

    #[test]
    fn mode_strings() {
        assert_eq!(mode_string('-', 0o4755), "-rwsr-xr-x");
        assert_eq!(mode_string('d', 0o1777), "drwxrwxrwt");
        assert_eq!(mode_string('-', 0o4644), "-rwSr--r--");
        assert_eq!(mode_string('-', 0o2750), "-rwxr-s---");
        assert_eq!(mode_string('d', 0o1666), "drw-rw-rwT");
        assert_eq!(perm_string(0o644), "rw-r--r--");
        assert_eq!(perm_string(0o4755), "rwxr-xr-x");
    }

    #[test]
    fn rounded_sizes() {
        assert_eq!(size_to_human_string(0, false, false), "0 B");
        assert_eq!(size_to_human_string(1000, false, false), "1000 B");
        assert_eq!(size_to_human_string(4096, false, false), "4 KiB");
        assert_eq!(size_to_human_string(1536, false, false), "1.5 KiB");
        assert_eq!(size_to_human_string(1048576, false, false), "1 MiB");
        assert_eq!(size_to_human_string(10 * 1024 * 1024 + 1, false, false), "10 MiB");
        assert_eq!(size_to_human_string(1025, true, false), "1 KiB");
        assert_eq!(size_to_human_string(1024 * 1024 + 1024 * 100, true, false), "1.1 MiB");
        assert_eq!(size_to_human_string(1024 + 51, true, false), "1.05 KiB");
        assert_eq!(size_to_human_string(2007, false, false), "2 KiB");
        // `mkswap` do Debian 13: 3325952 bytes saem `3.2 MiB` (arredonda, não trunca).
        assert_eq!(size_to_human_string(3_325_952, false, false), "3.2 MiB");
        assert_eq!(size_to_human_string(1_032_192, false, false), "1008 KiB");
    }

    #[test]
    fn one_letter_sizes() {
        // `lsmem` do Debian 13: `128M`, `32G`, `0B`.
        assert_eq!(size_to_human_string(0, false, true), "0B");
        assert_eq!(size_to_human_string(128 << 20, false, true), "128M");
        assert_eq!(size_to_human_string(32 << 30, false, true), "32G");
        assert_eq!(size_to_human_string(1536, false, true), "1.5K");
    }

    /// Programa de teste: `mkp CAMINHO` roda o `mkdir_p` e imprime `ok` ou o número do errno.
    fn mkp(ctx: &mut Ctx, args: &[OsString]) -> i32 {
        let path = args[1].as_bytes();
        let out = match mkdir_p(path, 0o750) {
            Ok(()) => "ok".to_string(),
            Err(e) => e.0.to_string(),
        };
        ctx.stdout().write_all(out.as_bytes()).unwrap();
        0
    }

    #[test]
    fn mkdir_p_creates_parents_and_checks_last() {
        let kit = TestKit::new().programs([Program::bin("mkp", mkp)]).file("/work/f", "x", 0o644);
        assert_eq!(kit.run(&["mkp", "/a/b/c"], b"").stdout_str(), "ok");
        assert_eq!(kit.run(&["mkp", "/a/b/c"], b"").stdout_str(), "ok");
        assert!(matches!(kit.tree("/a").iter().find(|(p, _)| p == b"b/c"), Some((_, TreeEntry::Dir { mode: 0o750 }))));
        assert_eq!(kit.run(&["mkp", "/work/f"], b"").stdout_str(), Errno::EEXIST.0.to_string());
    }
}
