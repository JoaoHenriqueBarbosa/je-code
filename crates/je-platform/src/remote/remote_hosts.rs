//! Porte de `src/vs/platform/remote/common/remoteHosts.ts`.
//!
//! As expressões regulares de `parseAuthority` viram código manual sobre unidades UTF-16, com a mesma
//! semântica ECMAScript (classes ASCII, `.` que não casa terminador de linha, `$` só no fim do texto).
//! Os overloads de `getRemoteName` e `getRemoteServerRootPath` viram uma função cada, sobre
//! `Option<&JsString>`. A porta é `number` no original (resultado de `parseInt`), aqui `f64`.

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::network::schemas;
use je_base::uri::URI;

const PLUS: u16 = b'+' as u16;
const COLON: u16 = b':' as u16;
const OPEN_BRACKET: u16 = b'[' as u16;
const CLOSE_BRACKET: u16 = b']' as u16;

/// `{ host: string; port: number }`.
#[derive(Clone, Debug, PartialEq)]
pub struct HostAndPort {
    pub host: JsString,
    pub port: f64,
}

pub fn get_remote_authority(uri: &URI) -> Option<JsString> {
    if uri.scheme() == schemas::VSCODE_REMOTE { Some(uri.authority().clone()) } else { None }
}

pub fn get_remote_name(authority: Option<&JsString>) -> Option<JsString> {
    let authority = authority.filter(|a| !a.is_empty())?;
    match authority.units().iter().position(|&u| u == PLUS) {
        // e.g. localhost:8000
        None => Some(authority.clone()),
        Some(pos) => Some(authority.slice(0, pos)),
    }
}

/// Returns the suffix part of the authority after the '+' character.
/// For remote connections, this is typically the server/tunnel identifier.
/// Examples:
/// - For tunnels: `tunnel+myTunnel` returns `myTunnel`
/// - For SSH: `ssh+myserver` returns `myserver`
/// - For localhost: `localhost:8000` returns `undefined`
pub fn get_remote_server_root_path(authority: Option<&JsString>) -> Option<JsString> {
    let authority = authority.filter(|a| !a.is_empty())?;
    let pos = authority.units().iter().position(|&u| u == PLUS)?;
    Some(authority.slice(pos + 1, authority.len()))
}

pub fn parse_authority_with_port(authority: &JsString) -> HostAndPort {
    let (host, port) = parse_authority(authority);
    match port {
        Some(port) => HostAndPort { host, port },
        None => errors::throw(Error::new(format!(
            "Invalid remote authority: {authority}. It must either be a remote of form <remoteName>+<arg> or a remote host of form <host>:<port>."
        ))),
    }
}

pub fn parse_authority_with_optional_port(authority: &JsString, default_port: f64) -> HostAndPort {
    let (host, port) = parse_authority(authority);
    HostAndPort { host, port: port.unwrap_or(default_port) }
}

/// `parseInt(digits, 10)` para uma sequência não vazia de dígitos ASCII.
fn parse_port(digits: &[u16]) -> f64 {
    let text: String = digits.iter().map(|&u| char::from(u as u8)).collect();
    text.parse::<f64>().unwrap_or(f64::NAN)
}

/// Dígitos de `\d+` até o fim do texto: não vazio e só ASCII `0-9`.
fn all_digits(units: &[u16]) -> bool {
    !units.is_empty() && units.iter().all(|&u| (u16::from(b'0')..=u16::from(b'9')).contains(&u))
}

/// `\[[0-9a-z:]+\]` no início do texto: o índice do `]` que fecha, se o miolo for não vazio e só de
/// `0-9`, `a-z` e `:` (sem `i`, então só minúsculas).
fn bracketed_end(units: &[u16]) -> Option<usize> {
    if units.first() != Some(&OPEN_BRACKET) {
        return None;
    }
    let close = units.iter().position(|&u| u == CLOSE_BRACKET)?;
    let inner = &units[1..close];
    let valid = !inner.is_empty()
        && inner.iter().all(|&u| {
            (u16::from(b'0')..=u16::from(b'9')).contains(&u) || (u16::from(b'a')..=u16::from(b'z')).contains(&u) || u == COLON
        });
    valid.then_some(close)
}

/// Terminadores de linha que o `.` da ECMAScript não casa.
fn is_line_terminator(unit: u16) -> bool {
    matches!(unit, 0x0A | 0x0D | 0x2028 | 0x2029)
}

fn parse_authority(authority: &JsString) -> (JsString, Option<f64>) {
    let units = authority.units();

    if let Some(close) = bracketed_end(units) {
        // check for ipv6 with port: /^(\[[0-9a-z:]+\]):(\d+)$/
        if units.get(close + 1) == Some(&COLON) && all_digits(&units[close + 2..]) {
            return (authority.slice(0, close + 1), Some(parse_port(&units[close + 2..])));
        }
        // check for ipv6 without port: /^(\[[0-9a-z:]+\])$/
        if close + 1 == units.len() {
            return (authority.clone(), None);
        }
    }

    // anything with a trailing port: /(.*):(\d+)$/. Sem a flag `s`, o `.` não cruza terminador de linha,
    // então o casamento só pode começar depois do último terminador.
    let start = units.iter().rposition(|&u| is_line_terminator(u)).map_or(0, |i| i + 1);
    let segment = &units[start..];
    if let Some(colon) = segment.iter().rposition(|&u| u == COLON)
        && all_digits(&segment[colon + 1..])
    {
        return (authority.slice(start, start + colon), Some(parse_port(&segment[colon + 1..])));
    }

    // doesn't contain a port
    (authority.clone(), None)
}

const LOOPBACK_HOSTS: [&str; 6] = [
    "localhost",
    "127.0.0.1",
    "::1",
    "[::1]",
    "0000:0000:0000:0000:0000:0000:0000:0001",
    "[0000:0000:0000:0000:0000:0000:0000:0001]",
];

/// Returns whether the given host (as found in a direct `<host>:<port>` remote
/// authority) refers to the local loopback interface. The check is intentionally
/// strict: only `localhost` and the IPv4/IPv6 loopback literals are considered
/// local. Any other host (a routable IP address or a hostname) is treated as a
/// connection that leaves the local machine.
pub fn is_loopback_host(host: &JsString) -> bool {
    let lowered = host.to_lower_case();
    LOOPBACK_HOSTS.iter().any(|&loopback| lowered == loopback)
}
