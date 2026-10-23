//! Porte de `src/vs/base/common/path.ts`: a cópia que o VS Code mantém do módulo `path` do Node
//! (v22.15.0) para poder rodar fora do Node, com as variantes `posix` e `win32`.
//!
//! O texto segue a semântica do JavaScript: todos os índices e comparações são em unidades UTF-16
//! (`charCodeAt`). Por dentro o módulo trabalha sobre `Vec<u16>`; a API pública recebe `&str` e
//! devolve `String`, o que só muda o resultado para surrogates isolados (que viram U+FFFD na
//! conversão, ver [`JsString::to_string_lossy`]).
//!
//! O original lê `process.cwd()`, `process.env` e `process.platform` (via `vs/base/common/process.ts`).
//! Aqui isso é o trait [`Environment`], injetado em [`PathModule`], de modo que as funções puras
//! não leiam estado global escondido e os testes possam fixar o diretório atual e a plataforma.
//!
//! Os argumentos que o original valida em tempo de execução (`validateString`, `validateObject`,
//! que lançam `ERR_INVALID_ARG_TYPE`) aqui são garantidos pelo sistema de tipos.

use std::collections::HashMap;

use crate::char_code as cc;
use crate::JsString;

type Units = Vec<u16>;

/// Plataforma do processo hospedeiro, o `process.platform === 'win32'` do original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Posix,
    Win32,
}

/// Fonte de `cwd`, variáveis de ambiente e plataforma (o objeto `process` do original).
pub trait Environment {
    /// `process.platform`, reduzido ao que o módulo distingue.
    fn platform(&self) -> Platform;
    /// `process.cwd()`.
    fn cwd(&self) -> String;
    /// `process.env[name]`.
    fn env_var(&self, name: &str) -> Option<String>;
}

/// Ambiente do processo real: `process.env['VSCODE_CWD'] || process.cwd()`, como em `process.ts`
/// no ambiente Node. Na web (wasm) o diretório atual é fixo em `/`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn platform(&self) -> Platform {
        if cfg!(windows) { Platform::Win32 } else { Platform::Posix }
    }

    fn cwd(&self) -> String {
        if cfg!(target_arch = "wasm32") {
            return "/".to_string();
        }
        match self.env_var("VSCODE_CWD") {
            Some(cwd) if !cwd.is_empty() => cwd,
            _ => std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        }
    }

    fn env_var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

/// Ambiente fixo, para testes e para quem quer resolver caminhos de outra máquina.
#[derive(Clone, Debug)]
pub struct FixedEnvironment {
    pub platform: Platform,
    pub cwd: String,
    pub vars: HashMap<String, String>,
}

impl FixedEnvironment {
    pub fn new(platform: Platform, cwd: &str) -> Self {
        Self { platform, cwd: cwd.to_string(), vars: HashMap::new() }
    }
}

impl Environment for FixedEnvironment {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn cwd(&self) -> String {
        self.cwd.clone()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }
}

/// `ParsedPath` do original.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedPath {
    pub root: String,
    pub dir: String,
    pub base: String,
    pub ext: String,
    pub name: String,
}

/// Uma das três instâncias de `IPath` do original (`posix`, `win32` e a da plataforma atual),
/// ligada ao [`Environment`] que dá o diretório atual.
#[derive(Clone, Copy)]
pub struct PathModule<'e> {
    flavor: Platform,
    env: &'e dyn Environment,
}

impl<'e> PathModule<'e> {
    /// O objeto `posix` exportado pelo original.
    pub fn posix(env: &'e dyn Environment) -> Self {
        Self { flavor: Platform::Posix, env }
    }

    /// O objeto `win32` exportado pelo original.
    pub fn win32(env: &'e dyn Environment) -> Self {
        Self { flavor: Platform::Win32, env }
    }

    /// Os exports de nível de módulo do original (`normalize`, `join`, ...): `win32` quando a
    /// plataforma do ambiente é Windows, `posix` nos demais casos.
    pub fn native(env: &'e dyn Environment) -> Self {
        Self { flavor: env.platform(), env }
    }

    pub fn flavor(&self) -> Platform {
        self.flavor
    }

    /// `sep`.
    pub fn sep(&self) -> &'static str {
        match self.flavor {
            Platform::Posix => "/",
            Platform::Win32 => "\\",
        }
    }

    /// `delimiter`.
    pub fn delimiter(&self) -> &'static str {
        match self.flavor {
            Platform::Posix => ":",
            Platform::Win32 => ";",
        }
    }

    /// `resolve(...pathSegments)`.
    pub fn resolve(&self, path_segments: &[&str]) -> String {
        let segments: Vec<Units> = path_segments.iter().map(|s| units(s)).collect();
        text(&match self.flavor {
            Platform::Posix => posix::resolve(self.env, &segments),
            Platform::Win32 => win32::resolve(self.env, &segments),
        })
    }

    /// `normalize(path)`.
    pub fn normalize(&self, path: &str) -> String {
        text(&match self.flavor {
            Platform::Posix => posix::normalize(&units(path)),
            Platform::Win32 => win32::normalize(&units(path)),
        })
    }

    /// `isAbsolute(path)`.
    pub fn is_absolute(&self, path: &str) -> bool {
        let path = units(path);
        match self.flavor {
            Platform::Posix => !path.is_empty() && ch(&path, 0) == Some(cc::Slash),
            Platform::Win32 => win32::is_absolute(&path),
        }
    }

    /// `join(...paths)`.
    pub fn join(&self, paths: &[&str]) -> String {
        let paths: Vec<Units> = paths.iter().map(|s| units(s)).collect();
        text(&match self.flavor {
            Platform::Posix => posix::join(&paths),
            Platform::Win32 => win32::join(&paths),
        })
    }

    /// `relative(from, to)`.
    pub fn relative(&self, from: &str, to: &str) -> String {
        text(&match self.flavor {
            Platform::Posix => posix::relative(self.env, &units(from), &units(to)),
            Platform::Win32 => win32::relative(self.env, &units(from), &units(to)),
        })
    }

    /// `toNamespacedPath(path)`. O original devolve a entrada intacta quando ela não é string ou é
    /// vazia, e no `posix` sempre.
    pub fn to_namespaced_path(&self, path: &str) -> String {
        match self.flavor {
            Platform::Posix => path.to_string(),
            Platform::Win32 => text(&win32::to_namespaced_path(self.env, &units(path))),
        }
    }

    /// `dirname(path)`.
    pub fn dirname(&self, path: &str) -> String {
        text(&match self.flavor {
            Platform::Posix => posix::dirname(&units(path)),
            Platform::Win32 => win32::dirname(&units(path)),
        })
    }

    /// `basename(path, suffix?)`.
    pub fn basename(&self, path: &str, suffix: Option<&str>) -> String {
        let path = units(path);
        let suffix = suffix.map(units);
        let (is_sep, start): (SepFn, usize) = match self.flavor {
            Platform::Posix => (is_posix_path_separator, 0),
            Platform::Win32 => (is_path_separator, win32::drive_prefix_len(&path)),
        };
        text(&basename(&path, suffix.as_deref(), is_sep, start))
    }

    /// `extname(path)`.
    pub fn extname(&self, path: &str) -> String {
        let path = units(path);
        let (is_sep, start): (SepFn, usize) = match self.flavor {
            Platform::Posix => (is_posix_path_separator, 0),
            Platform::Win32 => (is_path_separator, win32::drive_prefix_len(&path)),
        };
        text(&extname(&path, is_sep, start))
    }

    /// `format(pathObject)`.
    pub fn format(&self, path_object: &ParsedPath) -> String {
        let dir = if path_object.dir.is_empty() { &path_object.root } else { &path_object.dir };
        let base = if path_object.base.is_empty() {
            format!("{}{}", path_object.name, format_ext(&path_object.ext))
        } else {
            path_object.base.clone()
        };
        if dir.is_empty() {
            return base;
        }
        if *dir == path_object.root {
            format!("{dir}{base}")
        } else {
            format!("{dir}{}{base}", self.sep())
        }
    }

    /// `parse(path)`.
    pub fn parse(&self, path: &str) -> ParsedPath {
        let path = units(path);
        match self.flavor {
            Platform::Posix => posix::parse(&path),
            Platform::Win32 => win32::parse(&path),
        }
    }
}

fn units(s: &str) -> Units {
    JsString::from(s).units().to_vec()
}

fn text(units: &[u16]) -> String {
    JsString::from_units(units.to_vec()).to_string_lossy()
}

/// `String.prototype.toLowerCase`.
fn lower(units: &[u16]) -> Units {
    text(units).to_lowercase().encode_utf16().collect()
}

/// `charCodeAt(i)`: `None` fora do texto (o `NaN` do JavaScript, que nunca é igual a nada).
fn ch(path: &[u16], i: usize) -> Option<u16> {
    path.get(i).copied()
}

/// `path.slice(start, end)` com índices não negativos, truncados ao tamanho do texto.
fn slice(path: &[u16], start: usize, end: usize) -> &[u16] {
    let end = end.min(path.len());
    let start = start.min(end);
    &path[start..end]
}

fn concat(parts: &[&[u16]]) -> Units {
    parts.concat()
}

type SepFn = fn(Option<u16>) -> bool;

fn is_path_separator(code: Option<u16>) -> bool {
    code == Some(cc::Slash) || code == Some(cc::Backslash)
}

fn is_posix_path_separator(code: Option<u16>) -> bool {
    code == Some(cc::Slash)
}

fn is_windows_device_root(code: Option<u16>) -> bool {
    matches!(code, Some(c) if (cc::A..=cc::Z).contains(&c) || (cc::a..=cc::z).contains(&c))
}

fn format_ext(ext: &str) -> String {
    if ext.is_empty() {
        String::new()
    } else {
        format!("{}{ext}", if ext.starts_with('.') { "" } else { "." })
    }
}

/// Resolve `.` e `..` em elementos de um caminho com nomes de diretório.
fn normalize_string(path: &[u16], allow_above_root: bool, separator: u16, is_sep: SepFn) -> Units {
    let mut res: Units = Vec::new();
    let mut last_segment_length: usize = 0;
    let mut last_slash: isize = -1;
    let mut dots: isize = 0;
    let mut code: Option<u16> = Some(0);
    for i in 0..=path.len() {
        if i < path.len() {
            code = Some(path[i]);
        } else if is_sep(code) {
            break;
        } else {
            code = Some(cc::Slash);
        }

        if is_sep(code) {
            if last_slash == i as isize - 1 || dots == 1 {
                // NOOP
            } else if dots == 2 {
                let ends_with_dots = res.len() >= 2 && res[res.len() - 1] == cc::Period && res[res.len() - 2] == cc::Period;
                if res.len() < 2 || last_segment_length != 2 || !ends_with_dots {
                    if res.len() > 2 {
                        match res.iter().rposition(|&c| c == separator) {
                            None => {
                                res.clear();
                                last_segment_length = 0;
                            }
                            Some(last_slash_index) => {
                                res.truncate(last_slash_index);
                                last_segment_length = (res.len() as isize - 1 - res.iter().rposition(|&c| c == separator).map_or(-1, |p| p as isize)) as usize;
                            }
                        }
                        last_slash = i as isize;
                        dots = 0;
                        continue;
                    } else if !res.is_empty() {
                        res.clear();
                        last_segment_length = 0;
                        last_slash = i as isize;
                        dots = 0;
                        continue;
                    }
                }
                if allow_above_root {
                    if !res.is_empty() {
                        res.push(separator);
                    }
                    res.extend_from_slice(&[cc::Period, cc::Period]);
                    last_segment_length = 2;
                }
            } else {
                let segment = slice(path, (last_slash + 1) as usize, i);
                if !res.is_empty() {
                    res.push(separator);
                }
                res.extend_from_slice(segment);
                last_segment_length = (i as isize - last_slash - 1) as usize;
            }
            last_slash = i as isize;
            dots = 0;
        } else if code == Some(cc::Period) && dots != -1 {
            dots += 1;
        } else {
            dots = -1;
        }
    }
    res
}

/// Corpo comum de `basename`; `start` é o deslocamento do prefixo de unidade de disco (win32).
fn basename(path: &[u16], suffix: Option<&[u16]>, is_sep: SepFn, mut start: usize) -> Units {
    let mut end: isize = -1;
    let mut matched_slash = true;

    if let Some(suffix) = suffix.filter(|s| !s.is_empty() && s.len() <= path.len()) {
        if suffix == path {
            return Vec::new();
        }
        let mut ext_idx = suffix.len() as isize - 1;
        let mut first_non_slash_end: isize = -1;
        for i in (start..path.len()).rev() {
            let code = path[i];
            if is_sep(Some(code)) {
                // Chegou a um separador que não fazia parte dos separadores finais: para.
                if !matched_slash {
                    start = i + 1;
                    break;
                }
            } else {
                if first_non_slash_end == -1 {
                    // Primeiro não separador: guarda o índice caso a extensão não case.
                    matched_slash = false;
                    first_non_slash_end = i as isize + 1;
                }
                if ext_idx >= 0 {
                    if code == suffix[ext_idx as usize] {
                        ext_idx -= 1;
                        if ext_idx == -1 {
                            // A extensão casou: este é o fim do componente.
                            end = i as isize;
                        }
                    } else {
                        // A extensão não casa: o resultado é o componente inteiro.
                        ext_idx = -1;
                        end = first_non_slash_end;
                    }
                }
            }
        }

        if start as isize == end {
            end = first_non_slash_end;
        } else if end == -1 {
            end = path.len() as isize;
        }
        return slice(path, start, end as usize).to_vec();
    }
    for i in (start..path.len()).rev() {
        if is_sep(Some(path[i])) {
            if !matched_slash {
                start = i + 1;
                break;
            }
        } else if end == -1 {
            // Primeiro não separador: marca o fim do componente.
            matched_slash = false;
            end = i as isize + 1;
        }
    }

    if end == -1 {
        return Vec::new();
    }
    slice(path, start, end as usize).to_vec()
}

/// Corpo comum de `extname`; `start` é o deslocamento do prefixo de unidade de disco (win32).
fn extname(path: &[u16], is_sep: SepFn, start: usize) -> Units {
    let mut start_dot: isize = -1;
    let mut start_part = start;
    let mut end: isize = -1;
    let mut matched_slash = true;
    // Estado dos caracteres vistos antes do primeiro ponto e depois de qualquer separador.
    let mut pre_dot_state: i8 = 0;

    for i in (start..path.len()).rev() {
        let code = path[i];
        if is_sep(Some(code)) {
            if !matched_slash {
                start_part = i + 1;
                break;
            }
            continue;
        }
        if end == -1 {
            matched_slash = false;
            end = i as isize + 1;
        }
        if code == cc::Period {
            if start_dot == -1 {
                start_dot = i as isize;
            } else if pre_dot_state != 1 {
                pre_dot_state = 1;
            }
        } else if start_dot != -1 {
            // Um não ponto antes do ponto: boa chance de a extensão ser não vazia.
            pre_dot_state = -1;
        }
    }

    if start_dot == -1
        || end == -1
        // Um caractere que não é ponto logo antes do ponto.
        || pre_dot_state == 0
        // O componente mais à direita, sem separadores finais, é exatamente `..`.
        || (pre_dot_state == 1 && start_dot == end - 1 && start_dot == start_part as isize + 1)
    {
        return Vec::new();
    }
    slice(path, start_dot as usize, end as usize).to_vec()
}

/// Casamento de uma raiz UNC (`\\servidor\compartilhamento`) no começo de `path`, que já começa com
/// dois separadores. Devolve os intervalos do servidor e do compartilhamento, mais o fim da raiz.
struct UncRoot {
    server: (usize, usize),
    share: (usize, usize),
}

fn match_unc_root(path: &[u16]) -> Option<UncRoot> {
    let len = path.len();
    let skip = |mut j: usize, separators: bool| {
        while j < len && is_path_separator(ch(path, j)) == separators {
            j += 1;
        }
        j
    };
    // Um ou mais não separadores (o servidor).
    let server_end = skip(2, false);
    if !(server_end < len && server_end != 2) {
        return None;
    }
    // Um ou mais separadores.
    let share_start = skip(server_end, true);
    if !(share_start < len && share_start != server_end) {
        return None;
    }
    // Um ou mais não separadores (o compartilhamento).
    let share_end = skip(share_start, false);
    Some(UncRoot { server: (2, server_end), share: (share_start, share_end) })
}

impl UncRoot {
    /// O dispositivo `\\servidor\compartilhamento`, com barras invertidas.
    fn device(&self, path: &[u16]) -> Units {
        concat(&[
            &[cc::Backslash, cc::Backslash],
            slice(path, self.server.0, self.server.1),
            &[cc::Backslash],
            slice(path, self.share.0, self.share.1),
        ])
    }

    fn end(&self) -> usize {
        self.share.1
    }
}

/// Um `..` para cada componente de `from[first..end]` (o laço que monta o prefixo de `relative`),
/// unidos por `separator`.
fn parent_segments(from: &[u16], first: usize, end: usize, separator: u16) -> Units {
    if first > end {
        return Vec::new();
    }
    let count = from[first..end].iter().filter(|&&c| c == separator).count() + 1;
    vec![vec![cc::Period, cc::Period]; count].join(&separator)
}

mod win32 {
    use super::*;

    const BS: u16 = cc::Backslash;

    /// Tamanho do prefixo de unidade de disco (`C:`), 2 se houver, senão 0.
    pub(super) fn drive_prefix_len(path: &[u16]) -> usize {
        if path.len() >= 2 && is_windows_device_root(ch(path, 0)) && ch(path, 1) == Some(cc::Colon) { 2 } else { 0 }
    }

    pub(super) fn resolve(env: &dyn Environment, path_segments: &[Units]) -> Units {
        let mut resolved_device: Units = Vec::new();
        let mut resolved_tail: Units = Vec::new();
        let mut resolved_absolute = false;

        for i in (-1..path_segments.len() as isize).rev() {
            let path: Units;
            if i >= 0 {
                path = path_segments[i as usize].clone();
                // Pula entradas vazias.
                if path.is_empty() {
                    continue;
                }
            } else if resolved_device.is_empty() {
                path = units(&env.cwd());
            } else {
                // O Windows tem diretório atual por unidade de disco. Se já se resolveu uma letra
                // de unidade mas ainda não um caminho absoluto, pega o cwd daquela unidade, ou o
                // do processo se não houver. Neste ponto o dispositivo não é UNC, pois UNC é
                // sempre absoluto.
                let drive_cwd = env.env_var(&format!("={}", text(&resolved_device))).filter(|v| !v.is_empty());
                let mut candidate = units(&drive_cwd.unwrap_or_else(|| env.cwd()));
                // Confere se o cwd achado aponta para a nossa unidade; se não, usa a raiz dela.
                if lower(slice(&candidate, 0, 2)) != lower(&resolved_device) && ch(&candidate, 2) == Some(BS) {
                    candidate = concat(&[&resolved_device, &[BS]]);
                }
                path = candidate;
            }

            let len = path.len();
            let mut root_end = 0;
            let mut device: Units = Vec::new();
            let mut is_absolute = false;
            let code = ch(&path, 0);

            // Tenta casar uma raiz.
            if len == 1 {
                if is_path_separator(code) {
                    // `path` é só um separador.
                    root_end = 1;
                    is_absolute = true;
                }
            } else if is_path_separator(code) {
                // Possível raiz UNC. Começar com separador já garante um caminho absoluto de
                // algum tipo (UNC ou não).
                is_absolute = true;
                if is_path_separator(ch(&path, 1)) {
                    if let Some(unc) = match_unc_root(&path) {
                        device = unc.device(&path);
                        root_end = unc.end();
                    }
                } else {
                    root_end = 1;
                }
            } else if is_windows_device_root(code) && ch(&path, 1) == Some(cc::Colon) {
                // Possível raiz de dispositivo.
                device = path[..2].to_vec();
                root_end = 2;
                if len > 2 && is_path_separator(ch(&path, 2)) {
                    // Separador depois do nome da unidade indica caminho absoluto.
                    is_absolute = true;
                    root_end = 3;
                }
            }

            if !device.is_empty() {
                if !resolved_device.is_empty() {
                    if lower(&device) != lower(&resolved_device) {
                        // Este caminho aponta para outro dispositivo, então não se aplica.
                        continue;
                    }
                } else {
                    resolved_device = device;
                }
            }

            if resolved_absolute {
                if !resolved_device.is_empty() {
                    break;
                }
            } else {
                resolved_tail = concat(&[slice(&path, root_end, len), &[BS], &resolved_tail]);
                resolved_absolute = is_absolute;
                if is_absolute && !resolved_device.is_empty() {
                    break;
                }
            }
        }

        // Aqui o caminho já deveria ser absoluto completo, mas trata o relativo por segurança
        // (pode acontecer se `process.cwd()` falhar). Normaliza a cauda.
        let resolved_tail = normalize_string(&resolved_tail, !resolved_absolute, BS, is_path_separator);

        if resolved_absolute {
            concat(&[&resolved_device, &[BS], &resolved_tail])
        } else {
            let joined = concat(&[&resolved_device, &resolved_tail]);
            if joined.is_empty() { vec![cc::Period] } else { joined }
        }
    }

    pub(super) fn normalize(path: &[u16]) -> Units {
        let len = path.len();
        if len == 0 {
            return vec![cc::Period];
        }
        let mut root_end = 0;
        let mut device: Option<Units> = None;
        let mut is_absolute = false;
        let code = ch(path, 0);

        // Tenta casar uma raiz.
        if len == 1 {
            // Um único caractere: sai cedo para evitar trabalho à toa.
            return if is_posix_path_separator(code) { vec![BS] } else { path.to_vec() };
        }
        if is_path_separator(code) {
            // Possível raiz UNC.
            is_absolute = true;
            if is_path_separator(ch(path, 1)) {
                if let Some(unc) = match_unc_root(path) {
                    if unc.end() == len {
                        // Só a raiz UNC: devolve a raiz normalizada, não há mais o que processar.
                        return concat(&[&unc.device(path), &[BS]]);
                    }
                    device = Some(unc.device(path));
                    root_end = unc.end();
                }
            } else {
                root_end = 1;
            }
        } else if is_windows_device_root(code) && ch(path, 1) == Some(cc::Colon) {
            // Possível raiz de dispositivo.
            device = Some(path[..2].to_vec());
            root_end = 2;
            if len > 2 && is_path_separator(ch(path, 2)) {
                is_absolute = true;
                root_end = 3;
            }
        }

        let mut tail =
            if root_end < len { normalize_string(&path[root_end..], !is_absolute, BS, is_path_separator) } else { Vec::new() };
        if tail.is_empty() && !is_absolute {
            tail = vec![cc::Period];
        }
        if !tail.is_empty() && is_path_separator(ch(path, len - 1)) {
            tail.push(BS);
        }
        if !is_absolute && device.is_none() && path.contains(&cc::Colon) {
            // Se o caminho original não era absoluto e não se resolveu contra um dispositivo,
            // a `tail` não pode ter virado algo que o Windows leia como absoluto (CVE-2024-36139).
            if tail.len() >= 2 && is_windows_device_root(ch(&tail, 0)) && ch(&tail, 1) == Some(cc::Colon) {
                return concat(&[&[cc::Period, BS], &tail]);
            }
            let mut index = path.iter().position(|&c| c == cc::Colon);
            while let Some(colon) = index {
                if colon == len - 1 || is_path_separator(ch(path, colon + 1)) {
                    return concat(&[&[cc::Period, BS], &tail]);
                }
                index = path[colon + 1..].iter().position(|&c| c == cc::Colon).map(|p| p + colon + 1);
            }
        }
        match device {
            None => if is_absolute { concat(&[&[BS], &tail]) } else { tail },
            Some(device) => if is_absolute { concat(&[&device, &[BS], &tail]) } else { concat(&[&device, &tail]) },
        }
    }

    pub(super) fn is_absolute(path: &[u16]) -> bool {
        let len = path.len();
        if len == 0 {
            return false;
        }
        let code = ch(path, 0);
        is_path_separator(code)
            // Possível raiz de dispositivo.
            || (len > 2
                && is_windows_device_root(code)
                && ch(path, 1) == Some(cc::Colon)
                && is_path_separator(ch(path, 2)))
    }

    pub(super) fn join(paths: &[Units]) -> Units {
        if paths.is_empty() {
            return vec![cc::Period];
        }

        let mut joined: Option<Units> = None;
        let mut first_part: Option<&Units> = None;
        for arg in paths.iter().filter(|a| !a.is_empty()) {
            match &mut joined {
                None => {
                    joined = Some(arg.clone());
                    first_part = Some(arg);
                }
                Some(joined) => {
                    joined.push(BS);
                    joined.extend_from_slice(arg);
                }
            }
        }

        let Some(mut joined) = joined else {
            return vec![cc::Period];
        };

        // Garante que o caminho junto não comece com duas barras, porque `normalize()` o tomaria
        // por UNC. Isso é pulado quando está claro que o usuário quis um UNC: o primeiro argumento
        // não vazio começa com exatamente duas barras seguidas de ao menos um não barra.
        //
        // Para `normalize()` tratar como UNC são precisos ao menos 2 componentes, então não se
        // filtra isso aqui. Assim `join('//server', 'share')` dá `\\server\share\`.
        let mut needs_replace = true;
        let mut slash_count = 0;
        if let Some(first_part) = first_part.filter(|f| is_path_separator(ch(f, 0))) {
            slash_count += 1;
            let first_len = first_part.len();
            if first_len > 1 && is_path_separator(ch(first_part, 1)) {
                slash_count += 1;
                if first_len > 2 {
                    if is_path_separator(ch(first_part, 2)) {
                        slash_count += 1;
                    } else {
                        // O primeiro componente já é um caminho UNC.
                        needs_replace = false;
                    }
                }
            }
        }
        if needs_replace {
            // Acha mais barras consecutivas a substituir.
            while slash_count < joined.len() && is_path_separator(ch(&joined, slash_count)) {
                slash_count += 1;
            }
            if slash_count >= 2 {
                joined = concat(&[&[BS], slice(&joined, slash_count, joined.len())]);
            }
        }

        normalize(&joined)
    }

    /// Caminho relativo de `from` até `to`, por exemplo `C:\orandea\test\aaa` para
    /// `C:\orandea\impl\bbb` dá `..\..\impl\bbb`.
    pub(super) fn relative(env: &dyn Environment, from: &[u16], to: &[u16]) -> Units {
        if from == to {
            return Vec::new();
        }

        let from_orig = resolve(env, &[from.to_vec()]);
        let to_orig = resolve(env, &[to.to_vec()]);

        if from_orig == to_orig {
            return Vec::new();
        }

        let from = lower(&from_orig);
        let to = lower(&to_orig);

        if from == to {
            return Vec::new();
        }

        if from_orig.len() != from.len() || to_orig.len() != to.len() {
            let split = |p: &[u16]| {
                let mut parts: Vec<Units> = p.split(|&c| c == BS).map(<[u16]>::to_vec).collect();
                if parts.last().is_some_and(Vec::is_empty) {
                    parts.pop();
                }
                parts
            };
            let from_split = split(&from_orig);
            let to_split = split(&to_orig);

            let from_len = from_split.len();
            let to_len = to_split.len();
            let length = from_len.min(to_len);

            let mut i = 0;
            while i < length {
                if lower(&from_split[i]) != lower(&to_split[i]) {
                    break;
                }
                i += 1;
            }

            let join_parts = |parts: &[Units]| parts.join(&BS);
            if i == 0 {
                return to_orig;
            } else if i == length {
                if to_len > length {
                    return join_parts(&to_split[i..]);
                }
                if from_len > length {
                    return concat(&[&[cc::Period, cc::Period, BS].repeat(from_len - 1 - i), &[cc::Period, cc::Period]]);
                }
                return Vec::new();
            }

            return concat(&[&[cc::Period, cc::Period, BS].repeat(from_len - i), &join_parts(&to_split[i..])]);
        }

        // Tira as barras invertidas iniciais.
        let mut from_start = 0;
        while from_start < from.len() && from[from_start] == BS {
            from_start += 1;
        }
        // Tira as barras invertidas finais (só se aplica a UNC).
        let mut from_end = from.len();
        while from_end > from_start + 1 && from[from_end - 1] == BS {
            from_end -= 1;
        }
        let from_len = from_end - from_start;

        let mut to_start = 0;
        while to_start < to.len() && to[to_start] == BS {
            to_start += 1;
        }
        let mut to_end = to.len();
        while to_end > to_start + 1 && to[to_end - 1] == BS {
            to_end -= 1;
        }
        let to_len = to_end - to_start;

        // Compara os caminhos para achar o maior trecho comum desde a raiz.
        let length = from_len.min(to_len);
        let mut last_common_sep: isize = -1;
        let mut i = 0;
        while i < length {
            let from_code = from[from_start + i];
            if Some(from_code) != ch(&to, to_start + i) {
                break;
            } else if from_code == BS {
                last_common_sep = i as isize;
            }
            i += 1;
        }

        // Divergiu antes de ver o primeiro separador comum: devolve o `to` original.
        if i != length {
            if last_common_sep == -1 {
                return to_orig;
            }
        } else {
            if to_len > length {
                if ch(&to, to_start + i) == Some(BS) {
                    // `from` é exatamente o caminho base de `to`: from='C:\foo\bar', to='C:\foo\bar\baz'.
                    return slice(&to_orig, to_start + i + 1, usize::MAX).to_vec();
                }
                if i == 2 {
                    // `from` é a raiz do dispositivo: from='C:\', to='C:\foo'.
                    return slice(&to_orig, to_start + i, usize::MAX).to_vec();
                }
            }
            if from_len > length {
                if ch(&from, from_start + i) == Some(BS) {
                    // `to` é exatamente o caminho base de `from`: from='C:\foo\bar', to='C:\foo'.
                    last_common_sep = i as isize;
                } else if i == 2 {
                    // `to` é a raiz do dispositivo: from='C:\foo\bar', to='C:\'.
                    last_common_sep = 3;
                }
            }
            if last_common_sep == -1 {
                last_common_sep = 0;
            }
        }

        // Gera o caminho relativo pela diferença entre `to` e `from`.
        let out = parent_segments(&from, (from_start as isize + last_common_sep + 1) as usize, from_end, BS);

        to_start = (to_start as isize + last_common_sep) as usize;

        // Por fim, anexa o resto do destino que vem depois das partes comuns.
        if !out.is_empty() {
            return concat(&[&out, slice(&to_orig, to_start, to_end)]);
        }

        if ch(&to_orig, to_start) == Some(BS) {
            to_start += 1;
        }

        slice(&to_orig, to_start, to_end).to_vec()
    }

    pub(super) fn to_namespaced_path(env: &dyn Environment, path: &[u16]) -> Units {
        if path.is_empty() {
            return path.to_vec();
        }

        let resolved_path = resolve(env, &[path.to_vec()]);

        if resolved_path.len() <= 2 {
            return path.to_vec();
        }

        if resolved_path[0] == BS {
            // Possível raiz UNC.
            if resolved_path[1] == BS {
                let code = ch(&resolved_path, 2);
                if code != Some(cc::QuestionMark) && code != Some(cc::Period) {
                    // Raiz UNC curta: converte para UNC longo.
                    return concat(&[&units("\\\\?\\UNC\\"), &resolved_path[2..]]);
                }
            }
        } else if is_windows_device_root(ch(&resolved_path, 0))
            && ch(&resolved_path, 1) == Some(cc::Colon)
            && ch(&resolved_path, 2) == Some(BS)
        {
            // Raiz de dispositivo: converte para UNC longo.
            return concat(&[&units("\\\\?\\"), &resolved_path]);
        }

        resolved_path
    }

    pub(super) fn dirname(path: &[u16]) -> Units {
        let len = path.len();
        if len == 0 {
            return vec![cc::Period];
        }
        let mut root_end: isize = -1;
        let mut offset = 0;
        let code = ch(path, 0);

        if len == 1 {
            // Só um separador: sai cedo para evitar trabalho ou um ponto.
            return if is_path_separator(code) { path.to_vec() } else { vec![cc::Period] };
        }

        // Tenta casar uma raiz.
        if is_path_separator(code) {
            // Possível raiz UNC.
            root_end = 1;
            offset = 1;

            if let Some(unc) = match_unc_root(path).filter(|_| is_path_separator(ch(path, 1))) {
                if unc.end() == len {
                    // Só a raiz UNC.
                    return path.to_vec();
                }
                // Soma 1 para incluir o separador depois da raiz UNC, tratando-a como uma
                // "raiz normal" sobre a raiz (UNC).
                offset = unc.end() + 1;
                root_end = offset as isize;
            }
        // Possível raiz de dispositivo.
        } else if is_windows_device_root(code) && ch(path, 1) == Some(cc::Colon) {
            root_end = if len > 2 && is_path_separator(ch(path, 2)) { 3 } else { 2 };
            offset = root_end as usize;
        }

        let mut end: isize = -1;
        let mut matched_slash = true;
        for i in (offset..len).rev() {
            if is_path_separator(Some(path[i])) {
                if !matched_slash {
                    end = i as isize;
                    break;
                }
            } else {
                // Viu o primeiro não separador.
                matched_slash = false;
            }
        }

        if end == -1 {
            if root_end == -1 {
                return vec![cc::Period];
            }
            end = root_end;
        }
        slice(path, 0, end as usize).to_vec()
    }

    pub(super) fn parse(path: &[u16]) -> ParsedPath {
        let mut ret = ParsedPath::default();
        if path.is_empty() {
            return ret;
        }

        let len = path.len();
        let mut root_end = 0;
        let code = ch(path, 0);

        if len == 1 {
            if is_path_separator(code) {
                // Só um separador: sai cedo.
                ret.root = text(path);
                ret.dir = ret.root.clone();
                return ret;
            }
            ret.base = text(path);
            ret.name = ret.base.clone();
            return ret;
        }
        // Tenta casar uma raiz.
        if is_path_separator(code) {
            // Possível raiz UNC.
            root_end = 1;
            if let Some(unc) = match_unc_root(path).filter(|_| is_path_separator(ch(path, 1))) {
                root_end = if unc.end() == len { unc.end() } else { unc.end() + 1 };
            }
        } else if is_windows_device_root(code) && ch(path, 1) == Some(cc::Colon) {
            // Possível raiz de dispositivo.
            if len <= 2 {
                // Só a raiz da unidade: sai cedo.
                ret.root = text(path);
                ret.dir = ret.root.clone();
                return ret;
            }
            root_end = 2;
            if is_path_separator(ch(path, 2)) {
                if len == 3 {
                    ret.root = text(path);
                    ret.dir = ret.root.clone();
                    return ret;
                }
                root_end = 3;
            }
        }
        if root_end > 0 {
            ret.root = text(&path[..root_end]);
        }

        let mut start_dot: isize = -1;
        let mut start_part = root_end;
        let mut end: isize = -1;
        let mut matched_slash = true;
        let mut pre_dot_state: i8 = 0;

        // Obtém as informações do que não é diretório.
        for i in (root_end..len).rev() {
            let code = path[i];
            if is_path_separator(Some(code)) {
                if !matched_slash {
                    start_part = i + 1;
                    break;
                }
                continue;
            }
            if end == -1 {
                matched_slash = false;
                end = i as isize + 1;
            }
            if code == cc::Period {
                if start_dot == -1 {
                    start_dot = i as isize;
                } else if pre_dot_state != 1 {
                    pre_dot_state = 1;
                }
            } else if start_dot != -1 {
                pre_dot_state = -1;
            }
        }

        if end != -1 {
            let end = end as usize;
            if start_dot == -1
                || pre_dot_state == 0
                || (pre_dot_state == 1 && start_dot == end as isize - 1 && start_dot == start_part as isize + 1)
            {
                ret.base = text(slice(path, start_part, end));
                ret.name = ret.base.clone();
            } else {
                let start_dot = start_dot as usize;
                ret.name = text(slice(path, start_part, start_dot));
                ret.base = text(slice(path, start_part, end));
                ret.ext = text(slice(path, start_dot, end));
            }
        }

        // Se o diretório é a raiz, usa a raiz inteira como `dir`, com a barra final se houver
        // (`C:\abc` vira `C:\`). Senão, tira a barra final (`C:\abc\def` vira `C:\abc`).
        if start_part > 0 && start_part != root_end {
            ret.dir = text(slice(path, 0, start_part - 1));
        } else {
            ret.dir = ret.root.clone();
        }

        ret
    }
}

mod posix {
    use super::*;

    const SLASH: u16 = cc::Slash;

    /// `posixCwd`: no Windows converte as barras invertidas em barras e descarta a unidade.
    fn posix_cwd(env: &dyn Environment) -> Units {
        let cwd = units(&env.cwd());
        if env.platform() != Platform::Win32 {
            return cwd;
        }
        let cwd: Units = cwd.iter().map(|&c| if c == cc::Backslash { SLASH } else { c }).collect();
        // `cwd.slice(cwd.indexOf('/'))`: com `indexOf` igual a -1, o `slice(-1)` pega a última unidade.
        let from = cwd.iter().position(|&c| c == SLASH).unwrap_or_else(|| cwd.len().saturating_sub(1));
        cwd[from..].to_vec()
    }

    pub(super) fn resolve(env: &dyn Environment, path_segments: &[Units]) -> Units {
        let mut resolved_path: Units = Vec::new();
        let mut resolved_absolute = false;

        for path in path_segments.iter().rev() {
            if resolved_absolute {
                break;
            }
            // Pula entradas vazias.
            if path.is_empty() {
                continue;
            }

            resolved_path = concat(&[path, &[SLASH], &resolved_path]);
            resolved_absolute = ch(path, 0) == Some(SLASH);
        }

        if !resolved_absolute {
            let cwd = posix_cwd(env);
            resolved_path = concat(&[&cwd, &[SLASH], &resolved_path]);
            resolved_absolute = ch(&cwd, 0) == Some(SLASH);
        }

        // Aqui o caminho já deveria ser absoluto completo, mas trata o relativo por segurança
        // (pode acontecer se `process.cwd()` falhar). Normaliza o caminho.
        let resolved_path = normalize_string(&resolved_path, !resolved_absolute, SLASH, is_posix_path_separator);

        if resolved_absolute {
            return concat(&[&[SLASH], &resolved_path]);
        }
        if resolved_path.is_empty() { vec![cc::Period] } else { resolved_path }
    }

    pub(super) fn normalize(path: &[u16]) -> Units {
        if path.is_empty() {
            return vec![cc::Period];
        }

        let is_absolute = ch(path, 0) == Some(SLASH);
        let trailing_separator = ch(path, path.len() - 1) == Some(SLASH);

        let mut path = normalize_string(path, !is_absolute, SLASH, is_posix_path_separator);

        if path.is_empty() {
            if is_absolute {
                return vec![SLASH];
            }
            return if trailing_separator { vec![cc::Period, SLASH] } else { vec![cc::Period] };
        }
        if trailing_separator {
            path.push(SLASH);
        }

        if is_absolute { concat(&[&[SLASH], &path]) } else { path }
    }

    pub(super) fn join(paths: &[Units]) -> Units {
        if paths.is_empty() {
            return vec![cc::Period];
        }

        let non_empty: Vec<Units> = paths.iter().filter(|p| !p.is_empty()).cloned().collect();
        if non_empty.is_empty() {
            return vec![cc::Period];
        }

        normalize(&non_empty.join(&SLASH))
    }

    pub(super) fn relative(env: &dyn Environment, from: &[u16], to: &[u16]) -> Units {
        if from == to {
            return Vec::new();
        }

        // Tira as barras iniciais.
        let from = resolve(env, &[from.to_vec()]);
        let to = resolve(env, &[to.to_vec()]);

        if from == to {
            return Vec::new();
        }

        let from_start = 1;
        let from_end = from.len();
        let from_len = from_end - from_start;
        let to_start = 1;
        let to_len = to.len() - to_start;

        // Compara os caminhos para achar o maior trecho comum desde a raiz.
        let length = from_len.min(to_len);
        let mut last_common_sep: isize = -1;
        let mut i = 0;
        while i < length {
            let from_code = from[from_start + i];
            if Some(from_code) != ch(&to, to_start + i) {
                break;
            } else if from_code == SLASH {
                last_common_sep = i as isize;
            }
            i += 1;
        }
        if i == length {
            if to_len > length {
                if ch(&to, to_start + i) == Some(SLASH) {
                    // `from` é exatamente o caminho base de `to`: from='/foo/bar', to='/foo/bar/baz'.
                    return slice(&to, to_start + i + 1, usize::MAX).to_vec();
                }
                if i == 0 {
                    // `from` é a raiz: from='/', to='/foo'.
                    return slice(&to, to_start + i, usize::MAX).to_vec();
                }
            } else if from_len > length {
                if ch(&from, from_start + i) == Some(SLASH) {
                    // `to` é exatamente o caminho base de `from`: from='/foo/bar/baz', to='/foo/bar'.
                    last_common_sep = i as isize;
                } else if i == 0 {
                    // `to` é a raiz: from='/foo/bar', to='/'.
                    last_common_sep = 0;
                }
            }
        }

        // Gera o caminho relativo pela diferença entre `to` e `from`.
        let out = parent_segments(&from, (from_start as isize + last_common_sep + 1) as usize, from_end, SLASH);

        // Por fim, anexa o resto do destino que vem depois das partes comuns.
        concat(&[&out, slice(&to, (to_start as isize + last_common_sep) as usize, usize::MAX)])
    }

    pub(super) fn dirname(path: &[u16]) -> Units {
        if path.is_empty() {
            return vec![cc::Period];
        }
        let has_root = ch(path, 0) == Some(SLASH);
        let mut end: isize = -1;
        let mut matched_slash = true;
        for i in (1..path.len()).rev() {
            if path[i] == SLASH {
                if !matched_slash {
                    end = i as isize;
                    break;
                }
            } else {
                // Viu o primeiro não separador.
                matched_slash = false;
            }
        }

        if end == -1 {
            return if has_root { vec![SLASH] } else { vec![cc::Period] };
        }
        if has_root && end == 1 {
            return vec![SLASH, SLASH];
        }
        slice(path, 0, end as usize).to_vec()
    }

    pub(super) fn parse(path: &[u16]) -> ParsedPath {
        let mut ret = ParsedPath::default();
        if path.is_empty() {
            return ret;
        }
        let is_absolute = ch(path, 0) == Some(SLASH);
        if is_absolute {
            ret.root = "/".to_string();
        }
        let start = usize::from(is_absolute);
        let mut start_dot: isize = -1;
        let mut start_part = 0;
        let mut end: isize = -1;
        let mut matched_slash = true;
        let mut pre_dot_state: i8 = 0;

        // Obtém as informações do que não é diretório.
        for i in (start..path.len()).rev() {
            let code = path[i];
            if code == SLASH {
                if !matched_slash {
                    start_part = i + 1;
                    break;
                }
                continue;
            }
            if end == -1 {
                matched_slash = false;
                end = i as isize + 1;
            }
            if code == cc::Period {
                if start_dot == -1 {
                    start_dot = i as isize;
                } else if pre_dot_state != 1 {
                    pre_dot_state = 1;
                }
            } else if start_dot != -1 {
                pre_dot_state = -1;
            }
        }

        if end != -1 {
            let end = end as usize;
            let start = if start_part == 0 && is_absolute { 1 } else { start_part };
            if start_dot == -1
                || pre_dot_state == 0
                || (pre_dot_state == 1 && start_dot == end as isize - 1 && start_dot == start_part as isize + 1)
            {
                ret.base = text(slice(path, start, end));
                ret.name = ret.base.clone();
            } else {
                let start_dot = start_dot as usize;
                ret.name = text(slice(path, start, start_dot));
                ret.base = text(slice(path, start, end));
                ret.ext = text(slice(path, start_dot, end));
            }
        }

        if start_part > 0 {
            ret.dir = text(slice(path, 0, start_part - 1));
        } else if is_absolute {
            ret.dir = "/".to_string();
        }

        ret
    }
}
