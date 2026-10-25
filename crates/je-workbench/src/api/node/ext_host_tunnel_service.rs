//! Porte parcial de `src/vs/workbench/api/node/extHostTunnelService.ts`: as funções que leem as tabelas de conexão do Linux
//! (`/proc/net/tcp`, `/proc/net/tcp6`), a saída de `ls -l /proc/[0-9]*/fd/[0-9]*` e a de `ps -F -A -l` para achar as portas em
//! escuta e o processo dono de cada uma (`getSockets`, `loadListeningPorts`, `parseIpAddress`, `loadConnectionTable`,
//! `getRootProcesses`, `findPorts` e `tryFindRootPorts`).
//!
//! Fica de fora o `NodeExtHostTunnelService` (a varredura periódica `$registerCandidateFinder`, o `findCandidatePorts` que roda
//! `ls` e `ps` e lê `/proc`, e o `makeManagedTunnelFactory`) e o `ExtHostManagedSocket`: dependem do `ExtHostTunnelService` comum,
//! do `NodeRemoteTunnel` e do `ManagedSocket`, que o porte ainda não tem.
//!
//! Divergências:
//! - **`Record<string, ...>` indexado por número.** O `getSockets` devolve um [`HashMap`] pelo número do socket (as chaves de um
//!   objeto JS são o texto do número, e `socketMap[socket]` coage o número, o mesmo mapa).
//! - **`Record<string, string>[]` do `loadConnectionTable`.** São [`BTreeMap`] de texto para texto (a ordem das chaves de um objeto
//!   não entra na comparação).
//! - **`parseInt` que dá `NaN`.** A tabela do kernel sempre tem o inode decimal e a porta hexadecimal; uma linha em escuta sem um
//!   dos dois (que no JavaScript viraria uma entrada com `NaN`) fica de fora de `loadListeningPorts`.
//! - **`Map<number, CandidatePort & { ppid }>`.** É o [`RootPortMap`], que guarda a ordem de inserção como o `Map` (`set` de uma
//!   chave existente troca o valor sem mudar a posição). O `ppid` é `f64` porque o `Number.MAX_VALUE` do original é um valor dele.
//! - **`findPorts` assíncrona.** Devolve a [`Promise`] já resolvida (o corpo do original não tem `await`).
//! - **`String#trim` e `split(/\s+/)`.** Usam o conjunto de espaços do JavaScript (`\s`), que inclui o `U+FEFF` e não inclui o
//!   `U+0085`, e não o de `char::is_whitespace`.

use std::collections::{BTreeMap, HashMap};

use je_base::promise::Promise;
use zyarr::RegExp;

use crate::services::remote::tunnel_model::CandidatePort;

/// `\s` do JavaScript (os terminadores de linha e os espaços Unicode, mais o `U+FEFF`).
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{0B}'
            | '\u{0C}'
            | '\r'
            | ' '
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// `text.trim()`.
fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `text.split(/\s+/)`: cada corrida de espaços separa; espaços nas pontas geram texto vazio nas pontas.
fn split_js_whitespace(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut run_start: Option<usize> = None;
    for (index, c) in text.char_indices() {
        if is_js_whitespace(c) {
            run_start.get_or_insert(index);
        } else if let Some(run) = run_start.take() {
            parts.push(&text[start..run]);
            start = index;
        }
    }
    match run_start {
        Some(run) => {
            parts.push(&text[start..run]);
            parts.push("");
        }
        None => parts.push(&text[start..]),
    }
    parts
}

/// `text.substring(start, end)` sobre texto ASCII (o hexadecimal das tabelas): os índices passam do fim sem lançar.
fn substring(text: &str, start: usize, end: usize) -> &str {
    let (start, end) = (start.min(text.len()), end.min(text.len()));
    let (start, end) = if start <= end { (start, end) } else { (end, start) };
    text.get(start..end).unwrap_or("")
}

/// Os dígitos do começo de `text` na base dada (`parseInt(text, radix)` sem o prefixo `0x`), ou `None` quando não há nenhum (`NaN`).
fn parse_int(text: &str, radix: u32) -> Option<u64> {
    let digits: String = js_trim(text).chars().take_while(|c| c.is_digit(radix)).collect();
    if digits.is_empty() { None } else { Some(u64::from_str_radix(&digits, radix).unwrap_or(u64::MAX)) }
}

/// `parseInt(text, 16)` escrito como o JavaScript escreve o número (`NaN` quando não há dígito).
fn hex_to_decimal_text(text: &str) -> String {
    parse_int(text, 16).map_or_else(|| "NaN".to_owned(), |value| value.to_string())
}

/// `parseInt(text, 16).toString(16)`.
fn normalize_hex_text(text: &str) -> String {
    parse_int(text, 16).map_or_else(|| "NaN".to_owned(), |value| format!("{value:x}"))
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn group_text(units: &[u16], range: Option<&std::ops::Range<usize>>) -> String {
    range.map_or_else(String::new, |range| String::from_utf16_lossy(&units[range.clone()]))
}

/// O processo dono de um socket (`{ pid, socket }`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessSocket {
    pub pid: u32,
    pub socket: u64,
}

/// Uma porta em escuta (`{ socket, ip, port }`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListeningPort {
    pub socket: u64,
    pub ip: String,
    pub port: u32,
}

/// Um processo da lista de `/proc` (`{ pid, cwd, cmd }`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub cwd: String,
    pub cmd: String,
}

/// Um processo de `root` da saída do `ps` (`{ pid, cmd, ppid }`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootProcess {
    pub pid: u32,
    pub cmd: String,
    pub ppid: u32,
}

/// `CandidatePort & { ppid: number }`.
#[derive(Clone, Debug, PartialEq)]
pub struct RootPort {
    pub host: String,
    pub port: u32,
    pub detail: Option<String>,
    pub pid: Option<u32>,
    pub ppid: f64,
}

impl RootPort {
    /// O mesmo valor visto como `CandidatePort` (sem o `ppid`).
    pub fn candidate(&self) -> CandidatePort {
        CandidatePort { host: self.host.clone(), port: self.port, detail: self.detail.clone(), pid: self.pid }
    }
}

/// `Map<number, CandidatePort & { ppid: number }>`, com a ordem de inserção.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RootPortMap {
    entries: Vec<(u32, RootPort)>,
}

impl RootPortMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// `map.size`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `map.get(port)`.
    pub fn get(&self, port: u32) -> Option<&RootPort> {
        self.entries.iter().find(|(key, _)| *key == port).map(|(_, value)| value)
    }

    /// `map.set(port, value)`: uma chave existente troca o valor sem mudar a posição.
    pub fn set(&mut self, port: u32, value: RootPort) {
        match self.entries.iter_mut().find(|(key, _)| *key == port) {
            Some((_, existing)) => *existing = value,
            None => self.entries.push((port, value)),
        }
    }

    /// `map.values()`.
    pub fn values(&self) -> impl Iterator<Item = &RootPort> {
        self.entries.iter().map(|(_, value)| value)
    }
}

thread_local! {
    static SOCKET_LINE: RegExp = RegExp::from_str(r"\/proc\/(\d+)\/fd\/\d+ -> socket:\[(\d+)\]", "").expect("a expressão do socket é válida");
    static ROOT_PROCESS_LINE: RegExp =
        RegExp::from_str(r"^\d+\s+\D+\s+root\s+(\d+)\s+(\d+).+\d+\:\d+\:\d+\s+(.+)$", "").expect("a expressão do processo root é válida");
    static KNOWN_EXCLUDE_CMDLINE: RegExp = RegExp::from_str(r".*\.vscode-server-[a-zA-Z]+\/bin.*", "").expect("a expressão do servidor é válida");
}

/// `getSockets(stdout)`.
pub fn get_sockets(stdout: &str) -> HashMap<u64, ProcessSocket> {
    let mut socket_map = HashMap::new();
    for line in js_trim(stdout).split('\n') {
        let units = utf16(line);
        let captures = SOCKET_LINE.with(|regex| regex.exec(&units, 0)).map(|found| found.captures);
        if let Some(captures) = captures
            && captures.len() >= 3
        {
            let pid = parse_int(&group_text(&units, captures[1].as_ref()), 10).unwrap_or(0);
            let socket = parse_int(&group_text(&units, captures[2].as_ref()), 10).unwrap_or(0);
            socket_map.insert(socket, ProcessSocket { pid: u32::try_from(pid).unwrap_or(u32::MAX), socket });
        }
    }
    socket_map
}

/// `loadListeningPorts(...stdouts)`.
pub fn load_listening_ports(stdouts: &[&str]) -> Vec<ListeningPort> {
    let table: Vec<BTreeMap<String, String>> = stdouts.iter().flat_map(|stdout| load_connection_table(stdout)).collect();
    let mut ports: Vec<ListeningPort> = Vec::new();
    for row in table.iter().filter(|row| row.get("st").map(String::as_str) == Some("0A")) {
        let mut address = row.get("local_address").map(String::as_str).unwrap_or("").split(':');
        let (ip_hex, port_hex) = (address.next().unwrap_or(""), address.next().unwrap_or(""));
        let (Some(socket), Some(port)) = (parse_int(row.get("inode").map(String::as_str).unwrap_or(""), 10), parse_int(port_hex, 16)) else {
            continue;
        };
        let port = ListeningPort { socket, ip: parse_ip_address(ip_hex), port: u32::try_from(port).unwrap_or(u32::MAX) };
        // `new Map(entries)`: a chave `ip:port` repetida troca o valor sem mudar a posição.
        match ports.iter_mut().find(|existing| existing.ip == port.ip && existing.port == port.port) {
            Some(existing) => *existing = port,
            None => ports.push(port),
        }
    }
    ports
}

/// `parseIpAddress(hex)`.
pub fn parse_ip_address(hex: &str) -> String {
    let mut result = String::new();
    if hex.len() == 8 {
        let mut i = hex.len() as isize - 2;
        while i >= 0 {
            let at = i as usize;
            result += &hex_to_decimal_text(substring(hex, at, at + 2));
            if i != 0 {
                result.push('.');
            }
            i -= 2;
        }
    } else {
        // Nice explanation of host format in tcp6 file: https://serverfault.com/questions/592574/why-does-proc-net-tcp6-represents-1-as-1000
        let mut i = 0;
        while i < hex.len() {
            let word = substring(hex, i, i + 8);
            let mut sub_word = String::new();
            let mut j = 8;
            while j >= 2 {
                sub_word += substring(word, j - 2, j);
                if j == 6 || j == 2 {
                    // Trim leading zeros
                    sub_word = normalize_hex_text(&sub_word);
                    result += &sub_word;
                    sub_word = String::new();
                    if i + j != hex.len().wrapping_sub(6) {
                        result.push(':');
                    }
                }
                j -= 2;
            }
            i += 8;
        }
    }
    result
}

/// `loadConnectionTable(stdout)`.
pub fn load_connection_table(stdout: &str) -> Vec<BTreeMap<String, String>> {
    let mut lines = js_trim(stdout).split('\n');
    let header = lines.next().unwrap_or("");
    let names: Vec<&str> = split_js_whitespace(js_trim(header)).into_iter().filter(|name| *name != "rx_queue" && *name != "tm->when").collect();
    lines
        .map(|line| {
            let mut row = BTreeMap::new();
            for (index, value) in split_js_whitespace(js_trim(line)).into_iter().enumerate() {
                let key = match names.get(index) {
                    Some(name) if !name.is_empty() => (*name).to_owned(),
                    _ => index.to_string(),
                };
                row.insert(key, value.to_owned());
            }
            row
        })
        .collect()
}

fn known_exclude_cmdline(command: &str) -> bool {
    if command.encode_utf16().count() > 500 {
        return false;
    }
    let units = utf16(command);
    KNOWN_EXCLUDE_CMDLINE.with(|regex| regex.exec(&units, 0).is_some()) || command.contains("out/server-main.js") || command.contains("_productName=VSCode")
}

/// `getRootProcesses(stdout)`.
pub fn get_root_processes(stdout: &str) -> Vec<RootProcess> {
    let mut mapped = Vec::new();
    for line in js_trim(stdout).split('\n') {
        let units = utf16(line);
        let captures = ROOT_PROCESS_LINE.with(|regex| regex.exec(&units, 0)).map(|found| found.captures);
        if let Some(captures) = captures
            && captures.len() >= 4
        {
            mapped.push(RootProcess {
                pid: u32::try_from(parse_int(&group_text(&units, captures[1].as_ref()), 10).unwrap_or(0)).unwrap_or(u32::MAX),
                ppid: u32::try_from(parse_int(&group_text(&units, captures[2].as_ref()), 10).unwrap_or(0)).unwrap_or(u32::MAX),
                cmd: group_text(&units, captures[3].as_ref()),
            });
        }
    }
    mapped
}

/// `findPorts(connections, socketMap, processes)`.
pub fn find_ports(connections: &[ListeningPort], socket_map: &HashMap<u64, ProcessSocket>, processes: &[ProcessInfo]) -> Promise<Vec<CandidatePort>> {
    let mut process_map: HashMap<u32, &ProcessInfo> = HashMap::new();
    for process in processes {
        process_map.insert(process.pid, process);
    }

    let mut ports = Vec::new();
    for ListeningPort { socket, ip, port } in connections {
        let pid = socket_map.get(socket).map(|found| found.pid);
        let command = pid.and_then(|pid| process_map.get(&pid)).map(|process| process.cmd.as_str());
        if let (Some(pid), Some(command)) = (pid.filter(|pid| *pid != 0), command.filter(|command| !command.is_empty()))
            && !known_exclude_cmdline(command)
        {
            ports.push(CandidatePort { host: ip.clone(), port: *port, detail: Some(command.to_owned()), pid: Some(pid) });
        }
    }
    Promise::resolved(ports)
}

/// `tryFindRootPorts(connections, rootProcessesStdout, previousPorts)`.
pub fn try_find_root_ports(connections: &[ListeningPort], root_processes_stdout: &str, previous_ports: &RootPortMap) -> RootPortMap {
    let mut ports = RootPortMap::new();
    let root_processes = get_root_processes(root_processes_stdout);

    for connection in connections {
        if let Some(previous_port) = previous_ports.get(connection.port) {
            ports.set(connection.port, previous_port.clone());
            continue;
        }
        let port_text = connection.port.to_string();
        let root_process_match = root_processes.iter().find(|value| value.cmd.contains(&port_text));
        if let Some(root_process_match) = root_process_match {
            let mut best_match = root_process_match;
            // There are often several processes that "look" like they could match the port.
            // The one we want is usually the child of the other. Find the most child process.
            while let Some(most_child) = root_processes.iter().find(|value| value.ppid == best_match.pid) {
                best_match = most_child;
            }
            ports.set(
                connection.port,
                RootPort {
                    host: connection.ip.clone(),
                    port: connection.port,
                    detail: Some(best_match.cmd.clone()),
                    pid: Some(best_match.pid),
                    ppid: f64::from(best_match.ppid),
                },
            );
        } else {
            ports.set(connection.port, RootPort { host: connection.ip.clone(), port: connection.port, detail: None, pid: None, ppid: f64::MAX });
        }
    }

    ports
}
