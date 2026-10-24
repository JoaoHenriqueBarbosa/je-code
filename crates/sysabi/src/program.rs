//! Programas embutidos: cada crate de userland expõe uma lista de [`Program`], e o kernel monta
//! `/usr/bin` (e `/usr/sbin`) com um inode por programa. O `execve` de um desses inodes chama `main`.

use std::ffi::OsString;

use crate::ctx::Ctx;

/// Entrada de um programa: recebe o argv completo (argv[0] incluído) e devolve o código de saída.
/// Pode também terminar com [`crate::sys::exit`].
pub type Main = fn(&mut Ctx, &[OsString]) -> i32;

#[derive(Clone, Copy)]
pub struct Program {
    /// Nome no diretório (ex.: "grep").
    pub name: &'static str,
    /// Diretório onde o Debian 13 instala (`/usr/bin`, `/usr/sbin`). `/bin` e `/sbin` são symlinks
    /// pra `usr/bin` e `usr/sbin`, como no Debian com /usr unificado.
    pub dir: &'static str,
    pub main: Main,
}

impl Program {
    pub const fn bin(name: &'static str, main: Main) -> Program {
        Program { name, dir: "/usr/bin", main }
    }

    pub const fn sbin(name: &'static str, main: Main) -> Program {
        Program { name, dir: "/usr/sbin", main }
    }

    pub fn path(&self) -> String {
        format!("{}/{}", self.dir, self.name)
    }
}

impl std::fmt::Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Program({})", self.path())
    }
}
