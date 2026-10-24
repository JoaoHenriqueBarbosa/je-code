//! Os temas de cor e as configurações do usuário (`workbench.colorTheme`): o catálogo vem das extensões embutidas
//! (`contributes.themes` de cada `package.json`, com os rótulos de `package.nls.json`), e o `settings.json` do
//! usuário é o do VS Code (`User/settings.json` na pasta de dados dele), lido como JSONC, sem escrita.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// O tema padrão (o do oráculo) se o usuário não escolheu outro.
pub const DEFAULT_THEME_ID: &str = "Dark 2026";

#[derive(Clone, Debug)]
pub struct ThemeEntry {
    pub label: String,
    /// O valor de `workbench.colorTheme` que o escolhe (`settingsId`: o `id` do tema ou o rótulo).
    pub settings_id: String,
    pub path: PathBuf,
    /// O `uiTheme` do `package.json` (`vs`, `vs-dark`, `hc-black`, `hc-light`).
    pub ui_theme: String,
}

/// Remove comentários e vírgulas finais de um JSONC.
pub fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut in_string) = (0, false);
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        } else {
            out.push(c);
        }
        i += 1;
    }
    // Vírgulas antes de `}` ou `]`.
    let bytes: Vec<char> = out.chars().collect();
    let mut cleaned = String::with_capacity(out.len());
    let mut in_string = false;
    for (index, &c) in bytes.iter().enumerate() {
        if in_string {
            cleaned.push(c);
            if c == '"' && bytes[index.saturating_sub(1)] != '\\' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
        }
        if c == ',' && bytes[index + 1..].iter().find(|n| !n.is_whitespace()).is_some_and(|n| *n == '}' || *n == ']') {
            continue;
        }
        cleaned.push(c);
    }
    cleaned
}

/// A pasta de dados do usuário do VS Code (`JE_USER_DATA_DIR` a substitui).
pub fn user_data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("JE_USER_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Code"))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support/Code"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(|c| PathBuf::from(c).join("Code")).or_else(|| home().map(|h| h.join(".config/Code")))
    }
}

/// O `settings.json` do usuário como objeto JSON (vazio se não existe ou não lê).
pub fn user_settings() -> serde_json::Map<String, Value> {
    let Some(path) = user_data_dir().map(|d| d.join("User/settings.json")) else { return Default::default() };
    let Ok(text) = std::fs::read_to_string(path) else { return Default::default() };
    match serde_json::from_str::<Value>(&strip_jsonc(&text)) {
        Ok(Value::Object(map)) => map,
        _ => Default::default(),
    }
}

fn nls(extension: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(extension.join("package.nls.json")).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    json.get(key)?.as_str().map(str::to_owned).or_else(|| json.get(key)?.get("message")?.as_str().map(str::to_owned))
}

/// Os temas das extensões embutidas, na ordem em que o VS Code os lista (por extensão, na do `package.json`).
pub fn catalog(extensions: &Path) -> Vec<ThemeEntry> {
    let Ok(read) = std::fs::read_dir(extensions) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = read.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    let mut themes = Vec::new();
    for dir in dirs {
        let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else { continue };
        let Ok(json) = serde_json::from_str::<Value>(&text) else { continue };
        let Some(list) = json["contributes"]["themes"].as_array() else { continue };
        for theme in list {
            let Some(path) = theme["path"].as_str() else { continue };
            let raw = theme["label"].as_str().unwrap_or_default();
            let label = match raw.strip_prefix('%').and_then(|r| r.strip_suffix('%')) {
                Some(key) => nls(&dir, key).unwrap_or_else(|| key.to_owned()),
                None => raw.to_owned(),
            };
            let ui_theme = theme["uiTheme"].as_str().unwrap_or("vs-dark").to_owned();
            let settings_id = theme["id"].as_str().map_or_else(|| label.clone(), str::to_owned);
            themes.push(ThemeEntry { label, settings_id, path: dir.join(path.trim_start_matches("./")), ui_theme });
        }
    }
    themes
}

/// O tema de `workbench.colorTheme` do usuário (por `settingsId` ou rótulo), ou o padrão.
pub fn resolve(themes: &[ThemeEntry], wanted: Option<&str>) -> Option<usize> {
    let find = |name: &str| themes.iter().position(|t| t.settings_id == name || t.label == name);
    wanted.and_then(find).or_else(|| find(DEFAULT_THEME_ID))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc_comments_and_trailing_commas_are_removed() {
        let text = "{\n  // tema\n  \"workbench.colorTheme\": \"Dark+\", /* x */\n  \"url\": \"http://a//b\",\n  \"list\": [1, 2,],\n}\n";
        let value: Value = serde_json::from_str(&strip_jsonc(text)).expect("json válido");
        assert_eq!(value["workbench.colorTheme"], "Dark+");
        assert_eq!(value["url"], "http://a//b", "// dentro de texto não é comentário");
        assert_eq!(value["list"], serde_json::json!([1, 2]));
    }
}
