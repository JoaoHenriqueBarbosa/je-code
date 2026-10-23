//! As contribuições `languages` e `grammars` dos `package.json` das extensões embutidas, como o
//! `languagesExtPoint` (`workbench/services/language/common/languageService.ts`) e o `grammarsExtPoint`
//! (`workbench/services/textMate/common/TMGrammars.ts`) as leem.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use je_base::uri::URI;
use je_editor::modes_registry::{LanguageExtensionPoint, LanguageIcon};
use serde_json::Value;

use crate::jsonc;

/// Uma entrada de `contributes.grammars` ainda sem validação contra o serviço de linguagem.
#[derive(Clone, Debug)]
pub struct GrammarContribution {
    /// `publisher.name` da extensão.
    pub extension_id: String,
    /// A pasta da extensão.
    pub extension_dir: PathBuf,
    pub language: Option<String>,
    pub scope_name: String,
    pub path: String,
    pub embedded_languages: Vec<(String, String)>,
    pub token_types: Vec<(String, String)>,
    pub inject_to: Option<Vec<String>>,
    pub balanced_bracket_scopes: Option<Vec<String>>,
    pub unbalanced_bracket_scopes: Option<Vec<String>>,
    /// `true` quando `language` estava presente mas não é string (o original rejeita).
    pub invalid_language_type: bool,
}

/// O catálogo das extensões de uma pasta (`extensions/` do VS Code).
pub struct ExtensionsCatalog {
    /// As linguagens válidas, na ordem das extensões (`setDynamicLanguages`).
    pub languages: Vec<LanguageExtensionPoint>,
    pub grammars: Vec<GrammarContribution>,
    /// As pastas das extensões, ordenadas.
    pub extension_dirs: Vec<PathBuf>,
    /// `editor.maxTokenizationLineLength` dos `configurationDefaults` (`"[linguagem]"`) das extensões.
    pub max_tokenization_line_length: HashMap<String, i64>,
}

fn string_array(value: &Value) -> Option<Option<Vec<String>>> {
    match value {
        Value::Null => Some(None),
        Value::Array(items) => {
            let strings: Option<Vec<String>> = items.iter().map(|v| v.as_str().map(str::to_owned)).collect();
            strings.map(Some)
        }
        _ => None,
    }
}

fn string_map(value: &Value) -> Vec<(String, String)> {
    value
        .as_object()
        .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned()))).collect())
        .unwrap_or_default()
}

/// `joinPath(extensionLocation, relative)` como `URI` de arquivo.
fn join_uri(dir: &Path, relative: &str) -> URI {
    let joined = dir.join(relative.trim_start_matches("./"));
    URI::file(joined.to_string_lossy().as_ref(), je_base::path::Platform::Posix)
}

/// `isValidLanguageExtensionPoint(value)`.
fn parse_language(dir: &Path, value: &Value) -> Option<LanguageExtensionPoint> {
    let id = value["id"].as_str()?;
    let optional_strings = |key: &str| string_array(&value[key]);
    let extensions = optional_strings("extensions")?;
    let filenames = optional_strings("filenames")?;
    let aliases = optional_strings("aliases")?;
    let mimetypes = optional_strings("mimetypes")?;
    let first_line = match &value["firstLine"] {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        _ => return None,
    };
    let configuration = match &value["configuration"] {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        _ => return None,
    };
    let icon = match &value["icon"] {
        Value::Null => None,
        Value::Object(o) => match (o.get("light").and_then(Value::as_str), o.get("dark").and_then(Value::as_str)) {
            (Some(light), Some(dark)) => Some(LanguageIcon { light: join_uri(dir, light), dark: join_uri(dir, dark) }),
            _ => return None,
        },
        _ => return None,
    };
    // `filenamePatterns` não é validado pelo original: um valor que não é lista de strings é ignorado.
    let filename_patterns = string_array(&value["filenamePatterns"]).flatten();
    Some(LanguageExtensionPoint {
        id: id.to_owned(),
        extensions,
        filenames,
        filename_patterns,
        first_line,
        aliases,
        mimetypes,
        configuration: configuration.filter(|c| !c.is_empty()).map(|c| join_uri(dir, &c)),
        icon,
    })
}

fn parse_grammar(extension_id: &str, dir: &Path, value: &Value) -> Option<GrammarContribution> {
    let scope_name = value["scopeName"].as_str().filter(|s| !s.is_empty())?;
    let path = value["path"].as_str().filter(|s| !s.is_empty())?;
    let inject_to = match &value["injectTo"] {
        Value::Null => None,
        other => Some(string_array(other)??),
    };
    if !matches!(&value["embeddedLanguages"], Value::Null | Value::Object(_)) || !matches!(&value["tokenTypes"], Value::Null | Value::Object(_)) {
        return None;
    }
    let (language, invalid_language_type) = match &value["language"] {
        Value::Null => (None, false),
        Value::String(s) if !s.is_empty() => (Some(s.clone()), false),
        Value::String(_) => (None, false),
        _ => (None, true),
    };
    // `asStringArray(array, default)`: uma lista que não é só de strings vira o padrão.
    let string_list = |key: &str| string_array(&value[key]).flatten();
    Some(GrammarContribution {
        extension_id: extension_id.to_owned(),
        extension_dir: dir.to_owned(),
        language,
        scope_name: scope_name.to_owned(),
        path: path.to_owned(),
        embedded_languages: string_map(&value["embeddedLanguages"]),
        token_types: string_map(&value["tokenTypes"]),
        inject_to,
        balanced_bracket_scopes: string_list("balancedBracketScopes"),
        unbalanced_bracket_scopes: string_list("unbalancedBracketScopes"),
        invalid_language_type,
    })
}

impl ExtensionsCatalog {
    /// Lê todos os `package.json` das pastas de `extensions_dir`, em ordem alfabética.
    pub fn load(extensions_dir: &Path) -> Result<Self, String> {
        let mut dirs: Vec<PathBuf> = fs::read_dir(extensions_dir)
            .map_err(|e| format!("{}: {e}", extensions_dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.join("package.json").is_file())
            .collect();
        dirs.sort();

        let mut catalog = ExtensionsCatalog {
            languages: Vec::new(),
            grammars: Vec::new(),
            extension_dirs: Vec::new(),
            max_tokenization_line_length: HashMap::new(),
        };
        for dir in dirs {
            let Ok(text) = fs::read_to_string(dir.join("package.json")) else { continue };
            let Ok(manifest) = jsonc::parse(&text) else { continue };
            let extension_id = format!(
                "{}.{}",
                manifest["publisher"].as_str().unwrap_or("undefined_publisher"),
                manifest["name"].as_str().unwrap_or_default()
            );
            for lang in manifest["contributes"]["languages"].as_array().into_iter().flatten() {
                if let Some(language) = parse_language(&dir, lang) {
                    catalog.languages.push(language);
                }
            }
            for grammar in manifest["contributes"]["grammars"].as_array().into_iter().flatten() {
                if let Some(grammar) = parse_grammar(&extension_id, &dir, grammar) {
                    catalog.grammars.push(grammar);
                }
            }
            for (key, value) in manifest["contributes"]["configurationDefaults"].as_object().into_iter().flatten() {
                let language = key.strip_prefix('[').and_then(|k| k.strip_suffix(']'));
                if let (Some(language), Some(length)) = (language, value["editor.maxTokenizationLineLength"].as_i64()) {
                    catalog.max_tokenization_line_length.insert(language.to_owned(), length);
                }
            }
            catalog.extension_dirs.push(dir);
        }
        Ok(catalog)
    }
}
