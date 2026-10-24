//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/browser/terminalGitBashHelpers.ts`: a conversão
//! entre caminhos absolutos do Git Bash e do Windows.
//!
//! Diferença do original: o `match(/^\/([a-zA-Z])(\/.*)?$/)` é feito à mão (o `.` do JavaScript não casa
//! terminadores de linha, então um resto com `\n`, `\r`, `\u2028` ou `\u2029` cai no fallback).

/// Converte um caminho absoluto do Git Bash em caminho absoluto do Windows.
/// Exemplos:
///   "/"      => "C:\\"
///   "/c/"    => "C:\\"
///   "/c/Users/foo" => "C:\\Users\\foo"
///   "/d/bar" => "D:\\bar"
pub fn git_bash_to_windows_path(path: &str, drive_letter: Option<&str>) -> String {
    // Dynamically determine the system drive (default to 'C:' if not set)
    let system_drive = drive_letter.filter(|drive| !drive.is_empty()).unwrap_or("C:").to_uppercase();
    // Handle root "/"
    if path == "/" {
        return format!("{system_drive}\\");
    }
    if let Some(rest) = path.strip_prefix('/') {
        let mut chars = rest.chars();
        if let Some(drive) = chars.next().filter(char::is_ascii_alphabetic) {
            let tail = chars.as_str();
            let tail_is_valid = tail.is_empty() || (tail.starts_with('/') && !tail.contains(['\n', '\r', '\u{2028}', '\u{2029}']));
            if tail_is_valid {
                let rest = if tail.is_empty() { "\\".to_string() } else { tail.replace('/', "\\") };
                return format!("{}:{rest}", drive.to_ascii_uppercase());
            }
        }
    }
    // Fallback: just replace slashes
    path.replace('/', "\\")
}

/// Converte um caminho absoluto do Windows (por exemplo, `C:\Users\foo`) em caminho absoluto do Git Bash
/// (por exemplo, `/c/Users/foo`).
pub fn windows_to_git_bash_path(path: &str) -> String {
    // Convert Windows path (e.g. C:\Users\foo) to Git Bash path (e.g. /c/Users/foo)
    let mut chars = path.chars();
    let converted = match (chars.next(), chars.next(), chars.next()) {
        (Some(drive), Some(':'), Some('\\')) if drive.is_ascii_alphabetic() => format!("/{}/{}", drive.to_ascii_lowercase(), chars.as_str()),
        _ => path.to_string(),
    };
    converted.replace('\\', "/")
}
