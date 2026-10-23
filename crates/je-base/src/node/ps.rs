//! Porte de `src/vs/base/node/ps.ts`: `listProcesses`, a árvore de processos a partir de um pid raiz, e o
//! analisador da saída do `ps`.
//!
//! Notas de tradução:
//! - O original devolve `Promise<ProcessItem>` e roda o `ps` por `child_process.exec` (assíncrono). Aqui a
//!   execução é bloqueante e a função devolve a promessa já assentada (`Promise::resolved` ou `rejected`).
//! - `exec(cmd)` do Node roda `/bin/sh -c cmd` (no Windows, `cmd.exe /d /s /c`), junta o erro de saída não
//!   zero num `Error` e respeita `maxBuffer` (1 MiB por padrão): [`exec`] reproduz os três. A opção `env` do
//!   Node SUBSTITUI o ambiente inteiro, não o complementa: o `ps` roda só com `LC_NUMERIC`.
//! - No original, os itens da árvore e do `map` são os mesmos objetos, e o cálculo de CPU do Linux altera o
//!   `load` pelo `map`. Aqui os nós ficam num vetor e o `map` guarda índices, o que preserva esse
//!   compartilhamento (inclusive quando um pid repetido troca o item do `map` e deixa o antigo na árvore).
//! - `JS_FILENAME_PATTERN` (`/[a-zA-Z-]+\.js\b/g`) vira [`js_filename_matches`], escrita à mão: não há
//!   backtracking útil nesse padrão, porque a sequência `[a-zA-Z-]+` nunca contém o `.` que a segue.
//! - Windows: o original usa o módulo nativo `@vscode/windows-process-tree` (não há equivalente em Rust
//!   puro). Aqui a lista de processos vem do `Get-CimInstance Win32_Process` do PowerShell (pid, ppid, linha de
//!   comando e `WorkingSetSize`), a árvore é montada como no original, e o `load` fica em 0, porque a
//!   amostragem de CPU (`getProcessCpuUsage`) da biblioteca nativa não tem equivalente aqui.
//! - `totalmem()` do `os`: ver [`totalmem`].

use std::collections::{HashMap, VecDeque};
use std::process::{Command, Stdio};

use crate::errors::Error;
use crate::network::FileAccess;
use crate::path::Platform as PathPlatform;
use crate::platform::Platform;
use crate::processes::ProcessItem;
use crate::promise::Promise;

/// O `maxBuffer` padrão do `exec` do Node (1024 * 1024).
const DEFAULT_MAX_BUFFER: usize = 1024 * 1024;
/// O `maxBuffer` que o original passa ao `ps`.
const PS_MAX_BUFFER: usize = 1000 * 1024;

/// O `.` de uma regex sem a flag `s`: qualquer unidade menos os terminadores de linha.
fn is_dot(c: char) -> bool {
    !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// O `\s` do JavaScript (também o conjunto de `String.prototype.trim`).
fn js_is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}'
            | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// Todas as ocorrências de `JS_FILENAME_PATTERN` (`/[a-zA-Z-]+\.js\b/g`) em `cmd`, na ordem.
pub fn js_filename_matches(cmd: &str) -> Vec<String> {
    let bytes = cmd.as_bytes();
    let is_name = |b: u8| b.is_ascii_alphabetic() || b == b'-';
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut matches = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !is_name(bytes[i]) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < bytes.len() && is_name(bytes[j]) {
            j += 1;
        }
        let after = j + 3;
        if bytes[j..].starts_with(b".js") && (after >= bytes.len() || !is_word(bytes[after])) {
            matches.push(cmd[i..after].to_owned());
            i = after;
        } else {
            // Começar em qualquer posição dentro da mesma sequência falharia do mesmo jeito.
            i = j.max(i + 1);
        }
    }
    matches
}

/// `/conhost\.exe.+--headless/i`, sobre o texto já em minúsculas ASCII.
fn is_conpty(lower: &str) -> bool {
    const HOST: &str = "conhost.exe";
    lower.match_indices(HOST).any(|(at, _)| {
        let rest = &lower[at + HOST.len()..];
        rest.match_indices("--headless").any(|(h, _)| h >= 1 && rest[..h].chars().all(is_dot))
    })
}

/// O grupo de `/--type=([a-zA-Z-]+)/` (a primeira ocorrência que tem ao menos uma letra ou hífen depois).
fn type_argument(cmd: &str) -> Option<&str> {
    cmd.match_indices("--type=").find_map(|(at, marker)| {
        let rest = &cmd[at + marker.len()..];
        let end = rest.find(|c: char| !(c.is_ascii_alphabetic() || c == '-')).unwrap_or(rest.len());
        (end > 0).then(|| &rest[..end])
    })
}

/// `findName`: o nome amigável de um processo a partir da linha de comando.
pub fn find_name(cmd: &str) -> String {
    let lower = cmd.to_ascii_lowercase();

    // find windows crash reporter
    if lower.contains("--crashes-directory") {
        return "electron-crash-reporter".to_owned();
    }

    // find conpty process
    if is_conpty(&lower) {
        return "conpty-agent".to_owned();
    }

    // find "--type=xxxx"
    if let Some(kind) = type_argument(cmd) {
        return match kind {
            "renderer" => "window".to_owned(),
            "utility" => {
                if lower.contains("--utility-sub-type=network") {
                    "utility-network-service".to_owned()
                } else {
                    "utility-process".to_owned()
                }
            }
            // normalize remote extension host type
            "extensionHost" => "extension-host".to_owned(),
            other => other.to_owned(),
        };
    }

    if !cmd.contains("node ") && !cmd.contains("node.exe") {
        // find all xyz.js
        let result: String = js_filename_matches(cmd).iter().map(|file| format!("{file} ")).collect();
        if !result.is_empty() {
            return format!("electron-nodejs ({})", result.trim_matches(js_is_whitespace));
        }
    }

    cmd.to_owned()
}

/// `parseFloat`: o prefixo decimal do texto (depois do espaço inicial), ou `NaN`.
fn js_parse_float(text: &str) -> f64 {
    let s = text.trim_start_matches(js_is_whitespace);
    let bytes = s.as_bytes();
    let mut i = 0;
    let negative = bytes.first() == Some(&b'-');
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    if s[i..].starts_with("Infinity") {
        return if negative { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let digits_from = |mut k: usize| {
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        k
    };
    let int_end = digits_from(i);
    let mut end = int_end;
    let mut has_digits = int_end > i;
    if end < bytes.len() && bytes[end] == b'.' {
        let frac_end = digits_from(end + 1);
        if has_digits || frac_end > end + 1 {
            has_digits = true;
            end = frac_end;
        }
    }
    if !has_digits {
        return f64::NAN;
    }
    if end < bytes.len() && matches!(bytes[end], b'e' | b'E') {
        let mut k = end + 1;
        if k < bytes.len() && matches!(bytes[k], b'+' | b'-') {
            k += 1;
        }
        let exp_end = digits_from(k);
        if exp_end > k {
            end = exp_end;
        }
    }
    s[..end].parse::<f64>().unwrap_or(f64::NAN)
}

/// `[0-9]+` no começo de `s`: o número e o resto.
fn take_digits(s: &str) -> Option<(&str, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    (end > 0).then(|| s.split_at(end))
}

/// `\s+` no começo de `s`: o resto.
fn skip_whitespace(s: &str) -> Option<&str> {
    let rest = s.trim_start_matches(js_is_whitespace);
    (rest.len() < s.len()).then_some(rest)
}

/// `[0-9]+\.[0-9]+` no começo de `s`: o texto do número e o resto.
fn take_decimal(s: &str) -> Option<(&str, &str)> {
    let (int, after_int) = take_digits(s)?;
    let after_dot = after_int.strip_prefix('.')?;
    let (frac, rest) = take_digits(after_dot)?;
    Some((&s[..int.len() + 1 + frac.len()], rest))
}

/// `/^\s*([0-9]+)\s+([0-9]+)\s+([0-9]+\.[0-9]+)\s+([0-9]+\.[0-9]+)\s+(.+)$/` sobre a linha já aparada.
fn parse_ps_line(line: &str) -> Option<(f64, f64, f64, f64, &str)> {
    let s = line.trim_start_matches(js_is_whitespace);
    let (pid, s) = take_digits(s)?;
    let (ppid, s) = take_digits(skip_whitespace(s)?)?;
    let (load, s) = take_decimal(skip_whitespace(s)?)?;
    let (mem, s) = take_decimal(skip_whitespace(s)?)?;
    let cmd = skip_whitespace(s)?;
    if cmd.is_empty() || !cmd.chars().all(is_dot) {
        return None;
    }
    // `parseInt` e `parseFloat` sobre texto que a regex já garantiu ser numérico.
    Some((pid.parse().ok()?, ppid.parse().ok()?, load.parse().ok()?, mem.parse().ok()?, cmd))
}

/// `parsePsOutput`: chama `add_to_tree(pid, ppid, cmd, load, mem)` para cada linha que casa com o formato de
/// `ps -ax -o pid=,ppid=,pcpu=,pmem=,command=`.
pub fn parse_ps_output(stdout: &str, mut add_to_tree: impl FnMut(f64, f64, &str, f64, f64)) {
    for line in stdout.split('\n') {
        if let Some((pid, ppid, load, mem, cmd)) = parse_ps_line(line.trim_matches(js_is_whitespace)) {
            add_to_tree(pid, ppid, cmd, load, mem);
        }
    }
}

/// `totalmem()` do `os`, em bytes. Linux e Android: `MemTotal` de `/proc/meminfo`. macOS: `hw.memsize` do
/// `sysctl`. Outros alvos (Windows, web) não têm leitura em Rust puro aqui e devolvem 0: em Windows o valor
/// não é usado (a memória já vem em bytes do sistema, ver `is_windows` em [`list_processes`]).
pub fn totalmem() -> f64 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|text| {
                let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
                let kib: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
                Some(kib * 1024.0)
            })
            .unwrap_or(0.0)
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|out| String::from_utf8_lossy(&out.stdout).trim().parse().ok())
            .unwrap_or(0.0)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
    {
        0.0
    }
}

/// O resultado de `exec`: o `err`, o `stdout` e o `stderr` do callback.
struct Exec {
    err: Option<Error>,
    stdout: String,
    stderr: String,
}

impl Exec {
    /// `err || stderr`: o erro a rejeitar, se houve erro ou saída de erro.
    fn failure(&self) -> Option<Error> {
        self.err.clone().or_else(|| (!self.stderr.is_empty()).then(|| Error::new(self.stderr.clone())))
    }
}

/// `child_process.exec(command, { maxBuffer, env })`. Com `env`, o ambiente do filho é só esse.
fn exec(command: &str, env: Option<(&str, &str)>, max_buffer: usize) -> Exec {
    let mut process = if cfg!(windows) {
        let mut p = Command::new("cmd.exe");
        p.args(["/d", "/s", "/c", command]);
        p
    } else {
        let mut p = Command::new("/bin/sh");
        p.args(["-c", command]);
        p
    };
    if let Some((key, value)) = env {
        process.env_clear().env(key, value);
    }
    process.stdin(Stdio::null());
    match process.output() {
        Err(e) => Exec { err: Some(Error::new(e.to_string())), stdout: String::new(), stderr: String::new() },
        Ok(out) => {
            let mut stdout = out.stdout;
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            let err = if stdout.len() > max_buffer {
                stdout.truncate(max_buffer);
                Some(Error::new("stdout maxBuffer length exceeded"))
            } else if !out.status.success() {
                Some(Error::new(format!("Command failed: {command}\n{stderr}")))
            } else {
                None
            };
            Exec { err, stdout: String::from_utf8_lossy(&stdout).into_owned(), stderr }
        }
    }
}

struct Node {
    item: ProcessItem,
    children: Vec<usize>,
}

/// O `map` e a árvore de `listProcesses`, sobre um vetor de nós.
struct Tree {
    root_pid: f64,
    nodes: Vec<Node>,
    map: HashMap<u64, usize>,
    root: Option<usize>,
}

impl Tree {
    fn new(root_pid: f64) -> Self {
        Self { root_pid, nodes: Vec::new(), map: HashMap::new(), root: None }
    }

    fn lookup(&self, pid: f64) -> Option<usize> {
        self.map.get(&pid.to_bits()).copied()
    }

    /// Ordena os filhos de `parent` por pid (`sort((a, b) => a.pid - b.pid)`, estável).
    fn sort_children(&mut self, parent: usize) {
        let mut children = std::mem::take(&mut self.nodes[parent].children);
        children.sort_by(|a, b| self.nodes[*a].item.pid.partial_cmp(&self.nodes[*b].item.pid).unwrap_or(std::cmp::Ordering::Equal));
        self.nodes[parent].children = children;
    }

    fn insert(&mut self, item: ProcessItem) -> usize {
        let index = self.nodes.len();
        self.map.insert(item.pid.to_bits(), index);
        self.nodes.push(Node { item, children: Vec::new() });
        index
    }

    /// `addToTree`.
    fn add_to_tree(&mut self, total_memory: f64, is_windows: bool, pid: f64, ppid: f64, cmd: &str, load: f64, mem: f64) {
        let parent = self.lookup(ppid);
        if pid == self.root_pid || parent.is_some() {
            let index = self.insert(ProcessItem {
                name: find_name(cmd),
                cmd: cmd.to_owned(),
                pid,
                ppid,
                load,
                mem: if is_windows { mem } else { total_memory * (mem / 100.0) },
                children: None,
            });

            if pid == self.root_pid {
                self.root = Some(index);
            }

            if let Some(parent) = parent {
                self.nodes[parent].children.push(index);
                if self.nodes[parent].children.len() > 1 {
                    self.sort_children(parent);
                }
            }
        }
    }

    /// O `ProcessItem` do nó, com a subárvore (`children` só existe quando há filhos).
    fn to_item(&self, index: usize) -> ProcessItem {
        let node = &self.nodes[index];
        let mut item = node.item.clone();
        if !node.children.is_empty() {
            item.children = Some(node.children.iter().map(|c| self.to_item(*c)).collect());
        }
        item
    }

    fn root_not_found(&self) -> Error {
        Error::new(format!("Root process {} not found", self.root_pid))
    }
}

/// `JSON.stringify(path)`: protege os espaços do caminho para o shell.
fn quote_path(path: &str) -> String {
    serde_json::to_string(path).unwrap_or_else(|_| format!("\"{path}\""))
}

fn script_path(file_access: &FileAccess, resource: &str) -> String {
    let platform = if cfg!(windows) { PathPlatform::Win32 } else { PathPlatform::Posix };
    quote_path(&file_access.as_file_uri(resource).fs_path(platform).to_string_lossy())
}

/// `calculateLinuxCpuUsage`: o `pcpu` do Linux é a média da vida do processo, então o `cpuUsage.sh` mede
/// de novo numa janela de um segundo.
fn calculate_linux_cpu_usage(tree: &mut Tree, file_access: &FileAccess) -> Result<ProcessItem, Error> {
    // Flatten rootItem to get a list of all VSCode processes
    let mut queue: VecDeque<usize> = tree.root.into_iter().collect();
    let mut pids: Vec<f64> = Vec::new();
    while let Some(index) = queue.pop_front() {
        pids.push(tree.nodes[index].item.pid);
        queue.extend(tree.nodes[index].children.iter().copied());
    }

    let mut cmd = script_path(file_access, "vs/base/node/cpuUsage.sh");
    cmd.push(' ');
    cmd.push_str(&pids.iter().map(f64::to_string).collect::<Vec<_>>().join(" "));

    let result = exec(&cmd, None, DEFAULT_MAX_BUFFER);
    if let Some(error) = result.failure() {
        return Err(error);
    }
    let cpu_usage: Vec<&str> = result.stdout.split('\n').collect();
    for (i, pid) in pids.iter().enumerate() {
        if let Some(index) = tree.lookup(*pid) {
            tree.nodes[index].item.load = cpu_usage.get(i).map_or(f64::NAN, |v| js_parse_float(v));
        }
    }

    match tree.root {
        Some(root) => Ok(tree.to_item(root)),
        None => Err(tree.root_not_found()),
    }
}

/// OS X & Linux.
fn list_unix(root_pid: f64, file_access: &FileAccess, platform: &Platform) -> Result<ProcessItem, Error> {
    let is_linux = cfg!(any(target_os = "linux", target_os = "android"));
    let mut tree = Tree::new(root_pid);
    let total_memory = totalmem();
    let mut add = |tree: &mut Tree, stdout: &str| {
        parse_ps_output(stdout, |pid, ppid, cmd, load, mem| tree.add_to_tree(total_memory, platform.is_windows, pid, ppid, cmd, load, mem));
    };

    let which = exec("which ps", None, DEFAULT_MAX_BUFFER);
    if let Some(error) = which.failure() {
        if !is_linux {
            return Err(error);
        }
        let ps_script = exec(&script_path(file_access, "vs/base/node/ps.sh"), None, DEFAULT_MAX_BUFFER);
        if let Some(error) = ps_script.failure() {
            return Err(error);
        }
        add(&mut tree, &ps_script.stdout);
        return calculate_linux_cpu_usage(&mut tree, file_access);
    }

    let ps = which.stdout.trim_matches(js_is_whitespace);
    let args = "-ax -o pid=,ppid=,pcpu=,pmem=,command=";

    // Set numeric locale to ensure '.' is used as the decimal separator
    let result = exec(&format!("{ps} {args}"), Some(("LC_NUMERIC", "en_US.UTF-8")), PS_MAX_BUFFER);
    // Silently ignoring the screen size is bogus error. See https://github.com/microsoft/vscode/issues/98590
    if let Some(error) = result.err.clone() {
        return Err(error);
    }
    if !result.stderr.is_empty() && !result.stderr.contains("screen size is bogus") {
        return Err(Error::new(result.stderr));
    }
    add(&mut tree, &result.stdout);

    if is_linux {
        calculate_linux_cpu_usage(&mut tree, file_access)
    } else {
        match tree.root {
            Some(root) => Ok(tree.to_item(root)),
            None => Err(tree.root_not_found()),
        }
    }
}

/// Uma linha da lista de processos do Windows (o `IProcessInfo` da biblioteca nativa, sem a CPU).
#[derive(Clone, Debug, PartialEq)]
pub struct WindowsProcess {
    pub pid: f64,
    pub ppid: f64,
    pub command_line: Option<String>,
    pub memory: Option<f64>,
}

/// `cleanUNCPrefix`.
fn clean_unc_prefix(value: &str) -> String {
    if value.starts_with("\\\\?\\") || value.starts_with("\\??\\") {
        value[4..].to_owned()
    } else if value.starts_with("\"\\\\?\\") || value.starts_with("\"\\??\\") {
        format!("\"{}", &value[5..])
    } else {
        value.to_owned()
    }
}

/// O ramo do Windows: da lista de processos (`getProcessList` + `getProcessCpuUsage` do original) à árvore.
/// Só entram o processo raiz e seus descendentes, como na biblioteca nativa.
pub fn build_windows_tree(root_pid: f64, processes: &[WindowsProcess]) -> Result<ProcessItem, Error> {
    let mut tree = Tree::new(root_pid);
    let mut included: Vec<bool> = vec![false; processes.len()];
    let mut wanted = vec![root_pid];
    while let Some(pid) = wanted.pop() {
        for (i, process) in processes.iter().enumerate() {
            if !included[i] && (process.pid == pid || (process.ppid == pid && process.pid != process.ppid)) {
                included[i] = true;
                wanted.push(process.pid);
            }
        }
    }
    if !processes.iter().zip(&included).any(|(p, inc)| *inc && p.pid == root_pid) {
        return Err(tree.root_not_found());
    }

    for (process, _) in processes.iter().zip(&included).filter(|(_, inc)| **inc) {
        let command_line = clean_unc_prefix(process.command_line.as_deref().unwrap_or(""));
        tree.insert(ProcessItem {
            name: find_name(&command_line),
            cmd: command_line,
            pid: process.pid,
            ppid: process.ppid,
            load: 0.0,
            mem: process.memory.filter(|m| *m != 0.0).unwrap_or(0.0),
            children: None,
        });
    }

    let Some(root) = tree.lookup(root_pid) else {
        return Err(tree.root_not_found());
    };
    let live: Vec<usize> = (0..tree.nodes.len()).filter(|i| tree.lookup(tree.nodes[*i].item.pid) == Some(*i)).collect();
    for index in &live {
        if let Some(parent) = tree.lookup(tree.nodes[*index].item.ppid) {
            tree.nodes[parent].children.push(*index);
        }
    }
    for index in &live {
        if !tree.nodes[*index].children.is_empty() {
            tree.sort_children(*index);
        }
    }
    Ok(tree.to_item(root))
}

/// A lista de processos do Windows pelo PowerShell (`pid|ppid|WorkingSetSize|CommandLine`).
fn windows_process_list() -> Result<Vec<WindowsProcess>, Error> {
    let script = "Get-CimInstance Win32_Process | ForEach-Object { '{0}|{1}|{2}|{3}' -f $_.ProcessId, $_.ParentProcessId, $_.WorkingSetSize, $_.CommandLine }";
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::new(e.to_string()))?;
    if !out.status.success() {
        return Err(Error::new(format!("Command failed: powershell.exe\n{}", String::from_utf8_lossy(&out.stderr))));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, '|');
            let pid = fields.next()?.trim().parse().ok()?;
            let ppid = fields.next()?.trim().parse().ok()?;
            let memory = fields.next()?.trim().parse().ok();
            let command_line = fields.next().map(str::to_owned);
            Some(WindowsProcess { pid, ppid, command_line, memory })
        })
        .collect())
}

/// `listProcesses`: a árvore de processos a partir de `root_pid`. `platform` é o `isWindows` do original e
/// `file_access` resolve os scripts `vs/base/node/ps.sh` e `vs/base/node/cpuUsage.sh` (só usados no Linux).
pub fn list_processes(root_pid: f64, file_access: &FileAccess, platform: &Platform) -> Promise<ProcessItem> {
    let result = if cfg!(windows) {
        windows_process_list().and_then(|list| build_windows_tree(root_pid, &list))
    } else {
        list_unix(root_pid, file_access, platform)
    };
    match result {
        Ok(item) => Promise::resolved(item),
        Err(error) => Promise::rejected(error),
    }
}
