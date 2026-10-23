//! Porte de `src/vs/base/common/labels.ts`: rótulos de caminho (`getPathLabel`, `tildify`, `shorten`),
//! o `template` de títulos e os rótulos com mnemônicos de menu e de botão.
//!
//! O original lê `isWindows`, `isMacintosh` e `OS` do módulo `platform`, e o `path` nativo e o `extUri`
//! do processo. Aqui tudo isso vem do [`Platform`] que cada função recebe (o `host`): ele diz em que
//! sistema o código "está rodando", e o `os` de [`PathLabelFormatting`] continua sendo o sistema do
//! caminho. O diretório atual do [`FixedEnvironment`] interno é `/`, porque nenhuma função daqui
//! resolve caminho relativo ao diretório atual.
//!
//! Diferenças de forma, sem mudança de resultado: `getPathLabel` e as funções de caminho trabalham em
//! `String` (um surrogate isolado no caminho do URI vira U+FFFD, como no `resources`), e o cache
//! `normalizedUserHomeCached` do `tildify`, que só evita refazer a conta, não existe.

use std::collections::HashMap;
use std::rc::Rc;

use crate::char_code as cc;
use crate::extpath::{has_drive_letter, to_slashes};
use crate::js_string::JsString;
use crate::path::{FixedEnvironment, PathModule};
use crate::platform::{OperatingSystem, Platform};
use crate::resources::ExtUri;
use crate::strings::starts_with_ignore_case;
use crate::uri::{Field, UriChange, URI};

/// `IPathLabelFormatting`.
pub struct PathLabelFormatting<'a> {
    /// The OS the path label is from to produce a label that matches OS expectations.
    pub os: OperatingSystem,
    /// Whether to add a `~` when the path is in the user home directory.
    ///
    /// Note: this only applies to Linux, macOS but not Windows.
    pub tildify: Option<&'a UserHomeProvider>,
    /// Whether to convert to a relative path if the path is within any of the opened workspace folders.
    pub relative: Option<&'a dyn RelativePathProvider>,
}

/// `IUserHomeProvider`.
pub struct UserHomeProvider {
    pub user_home: URI,
}

/// `{ uri: URI; name?: string }`.
#[derive(Clone)]
pub struct WorkspaceFolder {
    pub uri: URI,
    pub name: Option<String>,
}

/// `{ folders: { uri: URI; name?: string }[] }`.
pub struct Workspace {
    pub folders: Vec<WorkspaceFolder>,
}

/// `IRelativePathProvider`.
pub trait RelativePathProvider {
    /// Whether to not add a prefix when in multi-root workspace.
    fn no_prefix(&self) -> bool {
        false
    }

    fn get_workspace(&self) -> Workspace;

    fn get_workspace_folder(&self, resource: &URI) -> Option<WorkspaceFolder>;
}

fn host_env(host: &Platform) -> Rc<FixedEnvironment> {
    Rc::new(FixedEnvironment::new(host.path_flavor(), "/"))
}

fn path_lib(env: &FixedEnvironment, os: OperatingSystem) -> PathModule<'_> {
    if os == OperatingSystem::Windows { PathModule::win32(env) } else { PathModule::posix(env) }
}

/// `resource.path[0] === posix.sep && resource.path[1] !== posix.sep`.
fn is_rooted_posix_path(resource: &URI) -> bool {
    let units = resource.path().units();
    units.first() == Some(&cc::Slash) && units.get(1) != Some(&cc::Slash)
}

fn with_path(uri: &URI, path: &JsString) -> URI {
    uri.with(&UriChange { path: Field::Set(path.clone()), ..UriChange::default() })
}

pub fn get_path_label(resource: &URI, formatting: &PathLabelFormatting<'_>, host: &Platform) -> String {
    let os = formatting.os;
    let env = host_env(host);

    // return early with a relative path if we can resolve one
    if let Some(relatifier) = formatting.relative
        && let Some(relative_path) = get_relative_path_label(resource, relatifier, os, &env)
    {
        return relative_path;
    }

    // otherwise try to resolve a absolute path label and
    // apply target OS standard path separators if target
    // OS differs from actual OS we are running in
    let mut absolute_path = resource.fs_path(host.path_flavor()).to_string_lossy();
    if os == OperatingSystem::Windows && !host.is_windows {
        absolute_path = absolute_path.replace('/', "\\");
    } else if os != OperatingSystem::Windows && host.is_windows {
        absolute_path = absolute_path.replace('\\', "/");
    }

    // macOS/Linux: tildify with provided user home directory
    if os != OperatingSystem::Windows
        && let Some(tildifier) = formatting.tildify
    {
        let user_home = tildifier.user_home.fs_path(host.path_flavor()).to_string_lossy();

        // This is a bit of a hack, but in order to figure out if the
        // resource is in the user home, we need to make sure to convert it
        // to a user home resource. We cannot assume that the resource is
        // already a user home resource.
        let user_home_candidate = if resource.scheme() != tildifier.user_home.scheme() && is_rooted_posix_path(resource) {
            with_path(&tildifier.user_home, resource.path()).fs_path(host.path_flavor()).to_string_lossy()
        } else {
            absolute_path
        };

        absolute_path = tildify(&user_home_candidate, &user_home, os, host);
    }

    // normalize
    path_lib(&env, os).normalize(&normalize_drive_letter(&absolute_path, os == OperatingSystem::Windows))
}

fn get_relative_path_label(resource: &URI, relative_path_provider: &dyn RelativePathProvider, os: OperatingSystem, env: &Rc<FixedEnvironment>) -> Option<String> {
    let path_lib = path_lib(env, os);
    let ext_uri_lib = if os == OperatingSystem::Linux { ExtUri::unbiased(env.clone()) } else { ExtUri::ignore_path_case(env.clone()) };

    let workspace = relative_path_provider.get_workspace();
    let first_folder = workspace.folders.first()?;

    // This is a bit of a hack, but in order to figure out the folder
    // the resource belongs to, we need to make sure to convert it
    // to a workspace resource. We cannot assume that the resource is
    // already matching the workspace.
    let resource = if resource.scheme() != first_folder.uri.scheme() && is_rooted_posix_path(resource) {
        with_path(&first_folder.uri, resource.path())
    } else {
        resource.clone()
    };

    let folder = relative_path_provider.get_workspace_folder(&resource)?;

    let mut relative_path_label = if ext_uri_lib.is_equal(Some(&folder.uri), Some(&resource), false) {
        String::new() // no label if paths are identical
    } else {
        ext_uri_lib.relative_path(&folder.uri, &resource).unwrap_or_default()
    };

    // normalize
    if !relative_path_label.is_empty() {
        relative_path_label = path_lib.normalize(&relative_path_label);
    }

    // always show root basename if there are multiple folders
    if workspace.folders.len() > 1 && !relative_path_provider.no_prefix() {
        let root_name = match &folder.name {
            Some(name) if !name.is_empty() => name.clone(),
            _ => ext_uri_lib.basename_or_authority(&folder.uri),
        };
        relative_path_label = if relative_path_label.is_empty() { root_name } else { format!("{root_name} \u{2022} {relative_path_label}") };
    }

    Some(relative_path_label)
}

/// `normalizeDriveLetter`: `is_windows_os` é o `isWindowsOS`, que no original tem o `isWindows` do processo por padrão.
pub fn normalize_drive_letter(path: &str, is_windows_os: bool) -> String {
    if has_drive_letter(path, is_windows_os) {
        let mut chars = path.chars();
        let first = chars.next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
        return first + chars.as_str();
    }

    path.to_string()
}

/// `tildify`: `os` é o sistema do caminho (no original, o `OS` do processo por padrão) e `host` onde roda.
pub fn tildify(path: &str, user_home: &str, os: OperatingSystem, host: &Platform) -> String {
    if os == OperatingSystem::Windows || path.is_empty() || user_home.is_empty() {
        return path.to_string(); // unsupported on Windows
    }

    let mut normalized_user_home = user_home.to_string();
    if host.is_windows {
        normalized_user_home = to_slashes(&normalized_user_home); // make sure that the path is POSIX normalized on Windows
    }
    normalized_user_home = format!("{}/", normalized_user_home.trim_end_matches('/'));

    let mut normalized_path = path.to_string();
    if host.is_windows {
        normalized_path = to_slashes(&normalized_path); // make sure that the path is POSIX normalized on Windows
    }

    // Linux: case sensitive, macOS: case insensitive
    let (home, candidate) = (JsString::from(normalized_user_home.as_str()), JsString::from(normalized_path.as_str()));
    let in_home = if os == OperatingSystem::Linux { candidate.units().starts_with(home.units()) } else { starts_with_ignore_case(candidate.units(), home.units()) };
    if in_home {
        return format!("~/{}", candidate.slice(home.len(), candidate.len()).to_string_lossy());
    }

    path.to_string()
}

/// A expansão de `$$`, `$&`, `` $` ``, `$'` e `$n` do segundo argumento de `String.prototype.replace`
/// (`GetSubstitution`), para um casamento no início do texto e sem grupos nomeados.
fn expand_replacement(template: &str, matched: &str, after: &str, groups: &[&str]) -> String {
    let mut out = String::new();
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '$' || i + 1 == chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        match chars[i + 1] {
            '$' => {
                out.push('$');
                i += 2;
            }
            '&' => {
                out.push_str(matched);
                i += 2;
            }
            '`' => i += 2, // o que vem antes do casamento é vazio
            '\'' => {
                out.push_str(after);
                i += 2;
            }
            d if d.is_ascii_digit() => {
                let one = d.to_digit(10).unwrap_or(0) as usize;
                let two = chars.get(i + 2).and_then(|c| c.to_digit(10)).map(|d2| one * 10 + d2 as usize);
                if let Some(index) = two.filter(|n| (1..=groups.len()).contains(n)) {
                    out.push_str(groups[index - 1]);
                    i += 3;
                } else if (1..=groups.len()).contains(&one) {
                    out.push_str(groups[one - 1]);
                    i += 2;
                } else {
                    out.push('$');
                    i += 1;
                }
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

/// `untildify`: `path.replace(/^~($|\/|\\)/, `${userHome}$1`)`.
pub fn untildify(path: &str, user_home: &str) -> String {
    let Some(rest) = path.strip_prefix('~') else { return path.to_string() };
    let group = match rest.chars().next() {
        None => "",
        Some('/') => "/",
        Some('\\') => "\\",
        Some(_) => return path.to_string(),
    };
    let after = &rest[group.len()..];
    let replacement = format!("{user_home}$1");
    format!("{}{after}", expand_replacement(&replacement, &format!("~{group}"), after, &[group]))
}

const ELLIPSIS: &str = "\u{2026}";
const UNC: &str = "\\\\";
const HOME: &str = "~";

/// `/^[^:/\\?#]+?:\/\//`.
fn has_url_scheme(path: &str) -> bool {
    path.find([':', '/', '\\', '?', '#']).is_some_and(|i| i > 0 && path[i..].starts_with("://"))
}

/// Shortens the paths but keeps them easy to distinguish.
/// Replaces not important parts with ellipsis.
/// Every shorten path matches only one original path and vice versa.
///
/// Algorithm for shortening paths is as follows:
/// 1. For every path in list, find unique substring of that path.
/// 2. Unique substring along with ellipsis is shortened path of that path.
/// 3. To find unique substring of path, consider every segment of length from 1 to path.length of path from end of string
///    and if present segment is not substring to any other paths then present segment is unique path,
///    else check if it is not present as suffix of any other path and present segment is suffix of path itself,
///    if it is true take present segment as unique path.
/// 4. Apply ellipsis to unique segment according to whether segment is present at start/in-between/end of path.
///
/// Example 1
/// 1. consider 2 paths i.e. `['a\\b\\c\\d', 'a\\f\\b\\c\\d']`
/// 2. find unique path of first path,
///    a. 'd' is present in path2 and is suffix of path2, hence not unique of present path.
///    b. 'c' is present in path2 and 'c' is not suffix of present path, similarly for 'b' and 'a' also.
///    c. 'd\\c' is suffix of path2.
///    d. 'b\\c' is not suffix of present path.
///    e. 'a\\b' is not present in path2, hence unique path is 'a\\b...'.
/// 3. for path2, 'f' is not present in path1 hence unique is '...\\f\\...'.
///
/// Example 2
/// 1. consider 2 paths i.e. `['a\\b', 'a\\b\\c']`.
///    a. Even if 'b' is present in path2, as 'b' is suffix of path1 and is not suffix of path2, unique path will be '...\\b'.
/// 2. for path2, 'c' is not present in path1 hence unique path is '..\\c'.
///
/// `None` é o `null` (ou `undefined`) que o original aceita na lista e devolve como veio.
pub fn shorten(paths: &[Option<&str>], default_path_separator: &str) -> Vec<Option<String>> {
    let mut shortened_paths: Vec<Option<String>> = vec![None; paths.len()];

    // for every path (o `match` do original vive fora do laço, mas é sempre reiniciado em `true` a cada caminho)
    for (path_index, original_path) in paths.iter().enumerate() {
        let mut path_separator = default_path_separator;

        let original_path = match original_path {
            Some("") => {
                shortened_paths[path_index] = Some(format!(".{path_separator}"));
                continue;
            }
            None => continue,
            Some(path) => *path,
        };

        let mut match_ = true;

        // trim for now and concatenate unc path (e.g. \\network) or root path (/etc, ~/etc) later
        let mut prefix = "";
        let mut trimmed_path = original_path;
        if has_url_scheme(trimmed_path) {
            let split = trimmed_path.find("//").map_or(0, |i| i + 2);
            (prefix, trimmed_path) = trimmed_path.split_at(split);
            path_separator = "/";
        } else if trimmed_path.starts_with(UNC) {
            (prefix, trimmed_path) = trimmed_path.split_at(UNC.len());
        } else if trimmed_path.starts_with(path_separator) {
            (prefix, trimmed_path) = trimmed_path.split_at(path_separator.len());
        } else if trimmed_path.starts_with(HOME) {
            (prefix, trimmed_path) = trimmed_path.split_at(HOME.len());
        }

        // pick the first shortest subpath found
        let segments: Vec<&str> = trimmed_path.split(path_separator).collect();
        let mut subpath_length = 1;
        while match_ && subpath_length <= segments.len() {
            let mut start = segments.len() as isize - subpath_length as isize;
            while match_ && start >= 0 {
                match_ = false;
                let mut subpath = segments[start as usize..start as usize + subpath_length].join(path_separator);

                // that is unique to any other path
                let mut other_path_index = 0;
                while !match_ && other_path_index < paths.len() {
                    // suffix subpath treated specially as we consider no match 'x' and 'x/...'
                    if let Some(other_path) = paths[other_path_index].filter(|p| !p.is_empty())
                        && other_path_index != path_index
                        && other_path.contains(subpath.as_str())
                    {
                        let is_subpath_ending = start as usize + subpath_length == segments.len();

                        // Adding separator as prefix for subpath, such that 'endsWith(src, trgt)' considers subpath as directory name instead of plain string.
                        // prefix is not added when either subpath is root directory or path[otherPathIndex] does not have multiple directories.
                        let subpath_with_sep = if start > 0 && other_path.contains(path_separator) { format!("{path_separator}{subpath}") } else { subpath.clone() };
                        let is_other_path_ending = other_path.ends_with(&subpath_with_sep);

                        match_ = !is_subpath_ending || is_other_path_ending;
                    }
                    other_path_index += 1;
                }

                // found unique subpath
                if !match_ {
                    let mut result = String::new();

                    // preserve disk drive or root prefix
                    if segments[0].ends_with(':') || !prefix.is_empty() {
                        if start == 1 {
                            // extend subpath to include disk drive prefix
                            start = 0;
                            subpath_length += 1;
                            subpath = format!("{}{path_separator}{subpath}", segments[0]);
                        }

                        if start > 0 {
                            result = format!("{}{path_separator}", segments[0]);
                        }

                        result = format!("{prefix}{result}");
                    }

                    // add ellipsis at the beginning if needed
                    if start > 0 {
                        result = format!("{result}{ELLIPSIS}{path_separator}");
                    }

                    result.push_str(&subpath);

                    // add ellipsis at the end if needed
                    if start as usize + subpath_length < segments.len() {
                        // If the last segment is empty, preserve the trailing slash.
                        if start as usize + subpath_length == segments.len() - 1 && segments[segments.len() - 1].is_empty() {
                            result.push_str(path_separator);
                        } else {
                            result = format!("{result}{path_separator}{ELLIPSIS}");
                        }
                    }

                    shortened_paths[path_index] = Some(result);
                }
                start -= 1;
            }
            subpath_length += 1;
        }

        if match_ {
            shortened_paths[path_index] = Some(original_path.to_string()); // use original path if no unique subpaths found
        }
    }

    shortened_paths
}

/// `ISeparator`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Separator {
    pub label: String,
}

/// O valor de uma variável do `template`: texto ou separador (as chaves ausentes são o `undefined`/`null`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateValue {
    Text(String),
    Separator(Separator),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SegmentType {
    Text,
    Variable,
    Separator,
}

struct Segment {
    value: String,
    type_: SegmentType,
}

/// Helper to insert values for specific template variables into the string. E.g. "this $(is) a $(template)" can be
/// passed to this function together with an object that maps "is" and "template" to strings to have them replaced.
pub fn template(template: &str, values: &HashMap<String, TemplateValue>) -> String {
    let mut segments: Vec<Segment> = Vec::new();

    let mut in_variable = false;
    let mut cur_val = String::new();
    for char in template.chars() {
        // Beginning of variable
        if char == '$' || (in_variable && char == '{') {
            if !cur_val.is_empty() {
                segments.push(Segment { value: std::mem::take(&mut cur_val), type_: SegmentType::Text });
            }

            cur_val.clear();
            in_variable = true;
        }
        // End of variable
        else if char == '}' && in_variable {
            match values.get(&cur_val) {
                // Variable
                Some(TemplateValue::Text(resolved)) if !resolved.is_empty() => {
                    segments.push(Segment { value: resolved.clone(), type_: SegmentType::Variable });
                }
                // Separator, sem repetir o separador anterior
                Some(TemplateValue::Separator(resolved)) if segments.last().is_none_or(|prev| prev.type_ != SegmentType::Separator) => {
                    segments.push(Segment { value: resolved.label.clone(), type_: SegmentType::Separator });
                }
                _ => {}
            }

            cur_val.clear();
            in_variable = false;
        }
        // Text or Variable Name
        else {
            cur_val.push(char);
        }
    }

    // Tail
    if !cur_val.is_empty() && !in_variable {
        segments.push(Segment { value: cur_val, type_: SegmentType::Text });
    }

    segments
        .iter()
        .enumerate()
        .filter(|(index, segment)| {
            // Only keep separator if we have values to the left and right
            if segment.type_ == SegmentType::Separator {
                let left = index.checked_sub(1).and_then(|i| segments.get(i));
                let right = segments.get(index + 1);

                return [left, right].into_iter().all(|segment| segment.is_some_and(|s| (s.type_ == SegmentType::Variable || s.type_ == SegmentType::Text) && !s.value.is_empty()));
            }

            // accept any TEXT and VARIABLE
            true
        })
        .map(|(_, segment)| segment.value.as_str())
        .collect()
}

/// `label.replace(/\(&&\w\)|&&/g, '')`.
fn remove_mnemonic_markers(label: &str) -> String {
    let chars: Vec<char> = label.chars().collect();
    let mut out = String::with_capacity(label.len());
    let mut i = 0;
    while i < chars.len() {
        let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
        if chars[i] == '(' && chars.get(i + 1) == Some(&'&') && chars.get(i + 2) == Some(&'&') && chars.get(i + 3).is_some_and(|&c| is_word(c)) && chars.get(i + 4) == Some(&')') {
            i += 5;
        } else if chars[i] == '&' && chars.get(i + 1) == Some(&'&') {
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// `label.replace(/&&|&/g, m => m === '&' ? '&&' : '&')`: troca `&&` por `&` e `&` sozinho por `&&`.
fn swap_ampersands(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut rest = label;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        if rest[i..].starts_with("&&") {
            out.push('&');
            rest = &rest[i + 2..];
        } else {
            out.push_str("&&");
            rest = &rest[i + 1..];
        }
    }
    out.push_str(rest);
    out
}

/// Handles mnemonics for menu items. Depending on OS:
/// - Windows: Supported via & character (replace && with &)
/// -   Linux: Supported via & character (replace && with &)
/// -   macOS: Unsupported (replace && with empty string)
pub fn mnemonic_menu_label(label: &str, force_disable_mnemonics: bool, host: &Platform) -> String {
    if host.is_macintosh || force_disable_mnemonics {
        return remove_mnemonic_markers(label).replace('&', if host.is_macintosh { "&" } else { "&&" });
    }

    swap_ampersands(label)
}

/// O resultado de `mnemonicButtonLabel` sem `forceDisableMnemonics`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MnemonicButtonLabel {
    pub with_mnemonic: String,
    pub without_mnemonic: String,
}

/// Handles mnemonics for buttons. Depending on OS:
/// - Windows: Supported via & character (replace && with & and & with && for escaping)
/// -   Linux: Supported via _ character (replace && with _)
/// -   macOS: Unsupported (replace && with empty string)
pub fn mnemonic_button_label(label: &str, host: &Platform) -> MnemonicButtonLabel {
    let without_mnemonic = remove_mnemonic_markers(label);

    if host.is_macintosh {
        return MnemonicButtonLabel { with_mnemonic: without_mnemonic.clone(), without_mnemonic };
    }

    let with_mnemonic = if host.is_windows { swap_ampersands(label) } else { label.replace("&&", "_") };
    MnemonicButtonLabel { with_mnemonic, without_mnemonic }
}

/// `mnemonicButtonLabel(label, true)`: com `forceDisableMnemonics` o original devolve só o texto sem mnemônicos.
pub fn mnemonic_button_label_force_disabled(label: &str) -> String {
    remove_mnemonic_markers(label)
}

pub fn unmnemonic_label(label: &str) -> String {
    label.replace('&', "&&")
}

/// O resultado de `splitRecentLabel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentLabel {
    pub name: String,
    pub parent_path: String,
}

/// Splits a recent label in name and parent path, supporting both '/' and '\' and workspace suffixes.
/// If the location is remote, the remote name is included in the name part.
pub fn split_recent_label(recent_label: &str, host: &Platform) -> RecentLabel {
    if recent_label.ends_with(']') {
        // label with workspace suffix
        if let Some(last_index_of_square_bracket) = recent_label.rfind(" [") {
            let split = split_name(&recent_label[..last_index_of_square_bracket], host);
            let remote_name_with_space = &recent_label[last_index_of_square_bracket..];
            return RecentLabel { name: format!("{}{remote_name_with_space}", split.name), parent_path: split.parent_path };
        }
    }
    split_name(recent_label, host)
}

fn split_name(full_path: &str, host: &Platform) -> RecentLabel {
    let env = host_env(host);
    let p = if full_path.contains('/') { PathModule::posix(&*env) } else { PathModule::win32(&*env) };
    let name = p.basename(full_path, None);
    let parent_path = p.dirname(full_path);
    if !name.is_empty() {
        return RecentLabel { name, parent_path };
    }
    // only the root segment
    RecentLabel { name: parent_path, parent_path: String::new() }
}
