//! Porte de `src/vs/workbench/services/search/node/ripgrepFileSearch.ts`: os argumentos de linha de comando do ripgrep
//! para listar arquivos (`getRgArgs` e os auxiliares de glob), e `getAbsoluteGlob`/`fixDriveC`, exportados para teste.
//!
//! Fora do porte: `spawnRipgrepCmd`, que só liga `getRgArgs` ao `cp.spawn` do binário de `rgDiskPath()`; o porte do
//! processo filho e do caminho do binário é de quem for rodar o ripgrep, e este módulo entrega a ele os mesmos
//! argumentos e `siblingClauses` ([`RgArgs`]), exatamente como o original.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `Object.assign({}, folderQuery.excludePattern || {}, globalExclude || {})` copia o array `excludePattern` da pasta
//!   (`ExcludeGlobPattern[]`) como chaves `"0"`, `"1"`... com valores que são objetos `{ folder, pattern }`; nenhum deles
//!   é booleano nem tem `when`, então não gera glob nem sibling. Por isso só o `globalExclude` entra em
//!   [`folders_to_rg_exclude_globs`].
//! - `isMac` é `cfg!(target_os = "macos")`.
//! - `siblingClauses` e os padrões são [`Expression`]; o `value.when` truthy é [`ExprValue::When`] não vazio.

use std::collections::HashSet;

use je_base::extpath;
use je_base::glob::{ExprValue, Expression};
use je_base::normalization::normalize_nfd;
use je_base::path::PathModule;
use je_base::strings;
use je_base::uri::URI;
use je_platform::resources::{fs_path, host_environment};

use super::ripgrep_search_utils::anchor_glob;
use super::search::{IFileQuery, IFolderQuery, spread_expressions};

/// O que `getRgArgs` devolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgArgs {
    pub args: Vec<String>,
    pub sibling_clauses: Expression,
}

/// `IRgGlobResult`.
struct RgGlobResult {
    glob_args: Vec<String>,
    sibling_clauses: Expression,
}

/// O `args.push('-g', glob)` e, no macOS, a forma NFD dele quando difere.
fn push_glob_arg(args: &mut Vec<String>, glob: String) {
    args.push("-g".to_owned());
    args.push(glob.clone());
    if cfg!(target_os = "macos") {
        let units: Vec<u16> = glob.encode_utf16().collect();
        let normalized = normalize_nfd(&units).to_string_lossy();
        if normalized != glob {
            args.push("-g".to_owned());
            args.push(normalized);
        }
    }
}

/// `getRgArgs`.
pub fn get_rg_args(
    config: &IFileQuery,
    folder_query: &IFolderQuery<URI>,
    include_pattern: Option<&Expression>,
    exclude_pattern: Option<&Expression>,
    num_threads: Option<i64>,
) -> RgArgs {
    let mut args: Vec<String> = ["--files", "--hidden", "--case-sensitive", "--no-require-git"].iter().map(|arg| (*arg).to_owned()).collect();

    if config.common.ignore_glob_case.unwrap_or(false) || folder_query.ignore_glob_case.unwrap_or(false) {
        args.push("--glob-case-insensitive".to_owned());
        args.push("--ignore-file-case-insensitive".to_owned());
    }

    // includePattern can't have siblingClauses
    for glob_arg in folders_to_include_globs(std::slice::from_ref(folder_query), include_pattern, false) {
        push_glob_arg(&mut args, anchor_glob(&glob_arg));
    }

    let rg_globs = folders_to_rg_exclude_globs(std::slice::from_ref(folder_query), exclude_pattern, None, false);
    for glob_arg in &rg_globs.glob_args {
        push_glob_arg(&mut args, format!("!{}", anchor_glob(glob_arg)));
    }
    if folder_query.disregard_ignore_files != Some(false) {
        // Don't use .gitignore or .ignore
        args.push("--no-ignore".to_owned());
    } else if folder_query.disregard_parent_ignore_files != Some(false) {
        args.push("--no-ignore-parent".to_owned());
    }

    // Follow symlinks
    if !folder_query.ignore_symlinks.unwrap_or(false) {
        args.push("--follow".to_owned());
    }

    if config.exists.unwrap_or(false) {
        args.push("--quiet".to_owned());
    }

    if let Some(num_threads) = num_threads.filter(|num_threads| *num_threads != 0) {
        args.push("--threads".to_owned());
        args.push(num_threads.to_string());
    }

    args.push("--no-config".to_owned());
    if folder_query.disregard_global_ignore_files.unwrap_or(false) {
        args.push("--no-ignore-global".to_owned());
    }

    RgArgs { args, sibling_clauses: rg_globs.sibling_clauses }
}

/// `foldersToRgExcludeGlobs`.
fn folders_to_rg_exclude_globs(
    folder_queries: &[IFolderQuery<URI>],
    global_exclude: Option<&Expression>,
    excludes_to_skip: Option<&HashSet<String>>,
    absolute_globs: bool,
) -> RgGlobResult {
    let mut glob_args: Vec<String> = Vec::new();
    let mut sibling_clauses = Expression::new();
    for folder_query in folder_queries {
        let total_exclude_pattern = spread_expressions(None, global_exclude);
        let folder = absolute_globs.then(|| fs_path(&folder_query.folder));
        let result = glob_exprs_to_rg_globs(&total_exclude_pattern, folder.as_deref(), excludes_to_skip);
        glob_args.extend(result.glob_args);
        sibling_clauses = spread_expressions(Some(&sibling_clauses), Some(&result.sibling_clauses));
    }

    RgGlobResult { glob_args, sibling_clauses }
}

/// `foldersToIncludeGlobs`.
fn folders_to_include_globs(folder_queries: &[IFolderQuery<URI>], global_include: Option<&Expression>, absolute_globs: bool) -> Vec<String> {
    let mut glob_args: Vec<String> = Vec::new();
    for folder_query in folder_queries {
        let total_include_pattern = spread_expressions(global_include, folder_query.include_pattern.as_ref());
        let folder = absolute_globs.then(|| fs_path(&folder_query.folder));
        let result = glob_exprs_to_rg_globs(&total_include_pattern, folder.as_deref(), None);
        glob_args.extend(result.glob_args);
    }

    glob_args
}

/// `globExprsToRgGlobs`.
fn glob_exprs_to_rg_globs(patterns: &Expression, folder: Option<&str>, excludes_to_skip: Option<&HashSet<String>>) -> RgGlobResult {
    let mut glob_args: Vec<String> = Vec::new();
    let mut sibling_clauses = Expression::new();
    for (key, value) in patterns.entries() {
        if excludes_to_skip.is_some_and(|skip| skip.contains(key)) {
            continue;
        }

        if key.is_empty() {
            continue;
        }

        let absolute = match folder {
            Some(folder) if !folder.is_empty() => get_absolute_glob(folder, key),
            _ => key.clone(),
        };
        let mut key = trim_trailing_slash(&absolute);

        // glob.ts requires forward slashes, but a UNC path still must start with \\
        // #38165 and #38151
        if let Some(rest) = key.strip_prefix("\\\\") {
            key = format!("\\\\{}", rest.replace('\\', "/"));
        } else {
            key = key.replace('\\', "/");
        }

        match value {
            ExprValue::Bool(true) => {
                if key.starts_with("\\\\") {
                    // Absolute globs UNC paths don't work properly, see #58758
                    key.push_str("**");
                }

                glob_args.push(fix_drive_c(&key));
            }
            ExprValue::When(when) if !when.is_empty() => {
                sibling_clauses.insert(key, value.clone());
            }
            _ => {}
        }
    }

    RgGlobResult { glob_args, sibling_clauses }
}

/// Resolves a glob like "node_modules/**" in "/foo/bar" to "/foo/bar/node_modules/**".
/// Special cases C:/foo paths to write the glob like /foo instead - see https://github.com/BurntSushi/ripgrep/issues/530.
///
/// Exported for testing
pub fn get_absolute_glob(folder: &str, key: &str) -> String {
    let env = host_environment();
    let path = PathModule::native(env.as_ref());
    if path.is_absolute(key) { key.to_owned() } else { path.join(&[folder, key]) }
}

/// `trimTrailingSlash`.
fn trim_trailing_slash(str: &str) -> String {
    let rtrim = |text: &[u16], needle: u16| strings::rtrim(text, &[needle]).to_vec();
    let units: Vec<u16> = str.encode_utf16().collect();
    let units = rtrim(&units, u16::from(b'\\'));
    let units = rtrim(&units, u16::from(b'/'));
    String::from_utf16_lossy(&units)
}

/// `fixDriveC`.
pub fn fix_drive_c(path: &str) -> String {
    let env = host_environment();
    let sep = PathModule::native(env.as_ref()).sep();
    let root = extpath::get_root(path, sep);
    let bytes = path.as_bytes();
    if root.to_lowercase() == "c:/" && bytes.len() >= 3 && bytes[0].eq_ignore_ascii_case(&b'c') && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\') {
        format!("/{}", &path[3..])
    } else {
        path.to_owned()
    }
}
