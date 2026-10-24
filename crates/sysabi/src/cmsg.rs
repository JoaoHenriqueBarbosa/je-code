//! Dados auxiliares de socket (`struct cmsghdr`): o formato que `sendmsg` lê e `recvmsg` escreve.
//!
//! O kernel e os programas trocam o `msg_control` como bytes, com o layout do x86-64: cabeçalho de 16
//! bytes (`cmsg_len` em 64 bits, `cmsg_level` e `cmsg_type` em 32), dados, e preenchimento até 8 bytes.
//! A leitura segue o `__scm_send` do Linux 6.12 (`CMSG_OK`, `CMSG_NXTHDR`) e a escrita o `put_cmsg`
//! e o `scm_detach_fds` (corte do que não cabe e `MSG_CTRUNC`).

use crate::Errno;

/// `SOL_SOCKET`.
pub const SOL_SOCKET: i32 = 1;
/// `SCM_RIGHTS`: descritores de arquivo (`AF_UNIX`).
pub const SCM_RIGHTS: i32 = 1;
/// `SCM_CREDENTIALS`: `struct ucred` (`pid`, `uid`, `gid`).
pub const SCM_CREDENTIALS: i32 = 2;
/// `SCM_MAX_FD`: descritores por mensagem.
pub const SCM_MAX_FD: usize = 253;
/// `net.core.optmem_max` do Debian 13: o `msg_control` de `sendmsg` não passa disto (ENOBUFS).
pub const OPTMEM_MAX: usize = 20480;
/// `sizeof(struct cmsghdr)`.
pub const HEADER: usize = 16;
/// `sizeof(struct ucred)`.
pub const UCRED_LEN: usize = 12;

/// `CMSG_ALIGN`.
pub const fn align(n: usize) -> usize {
    (n + 7) & !7
}

/// `CMSG_LEN(n)`: cabeçalho mais `n` bytes de dados.
pub const fn len(n: usize) -> usize {
    HEADER + n
}

/// `CMSG_SPACE(n)`: o que um item de `n` bytes de dados ocupa, com o preenchimento.
pub const fn space(n: usize) -> usize {
    HEADER + align(n)
}

/// Um item de dados auxiliares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item<'a> {
    pub level: i32,
    pub kind: i32,
    pub data: &'a [u8],
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn i32_at(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// Os itens de um `msg_control` que o `sendmsg` recebe. Sem espaço para um cabeçalho não há item (e não
/// há erro); `cmsg_len` menor que o cabeçalho ou maior que o resto do buffer é EINVAL (`CMSG_OK`).
pub fn parse(control: &[u8]) -> Result<Vec<Item<'_>>, Errno> {
    let mut items = Vec::new();
    let mut at = 0usize;
    while control.len() - at >= HEADER {
        let cmsg_len = u64_at(control, at);
        if cmsg_len < HEADER as u64 || cmsg_len > (control.len() - at) as u64 {
            return Err(Errno::EINVAL);
        }
        let cmsg_len = cmsg_len as usize;
        items.push(Item { level: i32_at(control, at + 8), kind: i32_at(control, at + 12), data: &control[at + HEADER..at + cmsg_len] });
        // `__cmsg_nxthdr`: o próximo só existe se o cabeçalho dele cabe.
        at += align(cmsg_len);
        if at + HEADER > control.len() {
            break;
        }
    }
    Ok(items)
}

/// O `msg_control` de um `recvmsg`: o que couber em `cap` bytes, e se algo foi cortado (`MSG_CTRUNC`).
#[derive(Debug, Default)]
pub struct Builder {
    cap: usize,
    buf: Vec<u8>,
    pub truncated: bool,
}

impl Builder {
    /// Um buffer de `cap` bytes (o `msg_controllen` que o programa passou).
    pub fn new(cap: usize) -> Builder {
        Builder { cap, buf: Vec::new(), truncated: false }
    }

    fn left(&self) -> usize {
        self.cap - self.buf.len()
    }

    /// O `put_cmsg`: sem lugar para o cabeçalho, só marca o corte; com lugar parcial, grava o item cortado
    /// (o `cmsg_len` é o que coube) e consome o buffer inteiro.
    pub fn put(&mut self, level: i32, kind: i32, data: &[u8]) {
        if self.left() < HEADER {
            self.truncated = true;
            return;
        }
        let mut cmlen = len(data.len());
        if self.left() < cmlen {
            self.truncated = true;
            cmlen = self.left();
        }
        self.buf.extend_from_slice(&(cmlen as u64).to_le_bytes());
        self.buf.extend_from_slice(&level.to_le_bytes());
        self.buf.extend_from_slice(&kind.to_le_bytes());
        self.buf.extend_from_slice(&data[..cmlen - HEADER]);
        let padded = space(data.len()).min(self.left() + cmlen);
        self.buf.resize(self.buf.len() + padded - cmlen, 0);
    }

    /// Quantos descritores cabem agora (`scm_max_fds`).
    pub fn fd_room(&self) -> usize {
        if self.left() <= HEADER { 0 } else { (self.left() - HEADER) / 4 }
    }

    /// O item `SCM_RIGHTS` do `scm_detach_fds` com os descritores já instalados.
    pub fn put_fds(&mut self, fds: &[i32]) {
        let data: Vec<u8> = fds.iter().flat_map(|fd| fd.to_le_bytes()).collect();
        self.put(SOL_SOCKET, SCM_RIGHTS, &data);
    }

    /// Os bytes gravados: o `msg_controllen` que volta.
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Monta um `msg_control` com itens (o que um programa faz com `CMSG_FIRSTHDR`/`CMSG_NXTHDR` antes do `sendmsg`):
/// cada item com o preenchimento de `CMSG_SPACE`.
pub fn build(items: &[Item<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for item in items {
        out.extend_from_slice(&(len(item.data.len()) as u64).to_le_bytes());
        out.extend_from_slice(&item.level.to_le_bytes());
        out.extend_from_slice(&item.kind.to_le_bytes());
        out.extend_from_slice(item.data);
        out.resize(out.len() + align(item.data.len()) - item.data.len(), 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fds(items: &[i32]) -> Vec<u8> {
        items.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    #[test]
    fn lengths_match_the_glibc_macros() {
        assert_eq!(len(0), 16);
        assert_eq!(len(4), 20);
        assert_eq!(space(4), 24);
        assert_eq!(space(12), 32);
        assert_eq!(space(8), 24);
    }

    #[test]
    fn parse_round_trips_build() {
        let data = fds(&[3, 4]);
        let cred = [1u8; 12];
        let control = build(&[
            Item { level: SOL_SOCKET, kind: SCM_RIGHTS, data: &data },
            Item { level: SOL_SOCKET, kind: SCM_CREDENTIALS, data: &cred },
        ]);
        assert_eq!(control.len(), space(8) + space(12));
        let items = parse(&control).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].data, &data[..]);
        assert_eq!(items[1].kind, SCM_CREDENTIALS);
    }

    #[test]
    fn parse_rejects_a_length_outside_the_buffer() {
        let mut control = build(&[Item { level: 1, kind: 1, data: &fds(&[3]) }]);
        control[0] = 200;
        assert_eq!(parse(&control), Err(Errno::EINVAL));
        control[0] = 8;
        assert_eq!(parse(&control), Err(Errno::EINVAL));
    }

    #[test]
    fn parse_ignores_a_buffer_shorter_than_a_header() {
        assert_eq!(parse(&[0u8; 15]), Ok(vec![]));
        assert_eq!(parse(&[]), Ok(vec![]));
    }

    #[test]
    fn builder_cuts_what_does_not_fit() {
        let mut b = Builder::new(HEADER + 4);
        assert_eq!(b.fd_room(), 1);
        b.put(SOL_SOCKET, SCM_CREDENTIALS, &[7u8; 12]);
        assert!(b.truncated);
        let out = b.finish();
        assert_eq!(out.len(), 20);
        assert_eq!(u64_at(&out, 0), 20);
        assert_eq!(&out[16..], &[7u8; 4]);
    }

    #[test]
    fn builder_without_room_for_a_header_writes_nothing() {
        let mut b = Builder::new(HEADER - 1);
        assert_eq!(b.fd_room(), 0);
        b.put_fds(&[5]);
        assert!(b.truncated);
        assert!(b.finish().is_empty());
    }

    #[test]
    fn builder_pads_between_items() {
        let mut b = Builder::new(64);
        b.put(SOL_SOCKET, SCM_CREDENTIALS, &[1u8; 12]);
        b.put_fds(&[9]);
        assert!(!b.truncated);
        let out = b.finish();
        assert_eq!(out.len(), space(12) + space(4));
        assert_eq!(u64_at(&out, space(12)), 20);
        assert_eq!(i32_at(&out, space(12) + 16), 9);
    }
}
