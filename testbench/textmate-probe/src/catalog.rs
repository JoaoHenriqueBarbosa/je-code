//! Catálogo de linguagens e gramáticas das extensões embutidas, montado como o VS Code monta
//! (`languagesRegistry.ts`, `languagesAssociations.ts` e `TMGrammarFactory.ts`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{glob, jsonc};

struct Association {
    language: String,
    extension: Option<String>,
    filename: Option<String>,
    pattern: Option<String>,
}

pub struct GrammarDefinition {
    pub scope_name: String,
    pub language: Option<String>,
    pub path: PathBuf,
    pub embedded_languages: Vec<(String, String)>,
    pub token_types: Vec<(String, String)>,
    pub inject_to: Vec<String>,
}

pub struct Catalog {
    /// Ids de linguagem na ordem de registro (o id codificado é a posição mais um).
    pub languages: Vec<String>,
    associations: Vec<Association>,
    pub grammars: Vec<GrammarDefinition>,
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn string_map(value: &Value, key: &str) -> Vec<(String, String)> {
    value[key]
        .as_object()
        .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect())
        .unwrap_or_default()
}

impl Catalog {
    pub fn load(extensions_dir: &Path) -> Result<Self, String> {
        let mut dirs: Vec<PathBuf> = fs::read_dir(extensions_dir)
            .map_err(|e| format!("{}: {e}", extensions_dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.join("package.json").is_file())
            .collect();
        dirs.sort();
        let manifests: Vec<(PathBuf, Value)> = dirs
            .into_iter()
            .filter_map(|dir| {
                let text = fs::read_to_string(dir.join("package.json")).ok()?;
                Some((dir, jsonc::parse(&text).ok()?))
            })
            .collect();

        let mut catalog = Catalog { languages: Vec::new(), associations: Vec::new(), grammars: Vec::new() };
        for (_, manifest) in &manifests {
            for lang in manifest["contributes"]["languages"].as_array().into_iter().flatten() {
                catalog.register_language(lang);
            }
        }
        for (dir, manifest) in &manifests {
            for grammar in manifest["contributes"]["grammars"].as_array().into_iter().flatten() {
                catalog.register_grammar(dir, grammar);
            }
        }
        Ok(catalog)
    }

    fn register_language(&mut self, lang: &Value) {
        let Some(id) = lang["id"].as_str() else { return };
        if !self.languages.iter().any(|l| l == id) {
            self.languages.push(id.to_string());
        }
        let mut push = |extension: Option<String>, filename: Option<String>, pattern: Option<String>| {
            self.associations.push(Association { language: id.to_string(), extension, filename, pattern });
        };
        for e in strings(lang, "extensions") {
            push(Some(e), None, None);
        }
        for f in strings(lang, "filenames") {
            push(None, Some(f), None);
        }
        for p in strings(lang, "filenamePatterns") {
            push(None, None, Some(p));
        }
    }

    fn register_grammar(&mut self, dir: &Path, grammar: &Value) {
        let (Some(scope_name), Some(path)) = (grammar["scopeName"].as_str(), grammar["path"].as_str()) else {
            return;
        };
        let registered = |id: &str| self.languages.iter().any(|l| l == id);
        let embedded_languages =
            string_map(grammar, "embeddedLanguages").into_iter().filter(|(_, l)| registered(l)).collect();
        let token_types = string_map(grammar, "tokenTypes");
        let language = grammar["language"].as_str().filter(|l| registered(l)).map(str::to_string);
        self.grammars.push(GrammarDefinition {
            scope_name: scope_name.to_string(),
            language,
            path: dir.join(path),
            embedded_languages,
            token_types,
            inject_to: strings(grammar, "injectTo"),
        });
    }

    pub fn encoded_language_id(&self, language: &str) -> u32 {
        self.languages.iter().position(|l| l == language).map_or(0, |i| i as u32 + 1)
    }

    /// `guessLanguageIdByFilepathOrFirstLine` para um arquivo cujo caminho é `/<nome>`, sem primeira linha.
    pub fn guess_language(&self, file_name: &str) -> Option<&str> {
        let filename = file_name.to_lowercase();
        let path = format!("/{filename}");
        let (mut by_name, mut by_pattern, mut by_extension): (Option<&Association>, Option<&Association>, Option<&Association>) =
            (None, None, None);
        for a in self.associations.iter().rev() {
            if a.filename.as_ref().is_some_and(|f| f.to_lowercase() == filename) {
                by_name = Some(a);
                break;
            }
            if let Some(p) = &a.pattern
                && by_pattern.is_none_or(|b| p.len() > b.pattern.as_ref().map_or(0, String::len))
            {
                let target = if p.contains('/') { &path } else { &filename };
                if glob::matches(p, target) {
                    by_pattern = Some(a);
                }
            }
            if let Some(e) = &a.extension
                && by_extension.is_none_or(|b| e.len() > b.extension.as_ref().map_or(0, String::len))
                && filename.ends_with(&e.to_lowercase())
            {
                by_extension = Some(a);
            }
        }
        by_name.or(by_pattern).or(by_extension).map(|a| a.language.as_str())
    }

    /// Escopo da gramática de cada linguagem (a última registrada vence, como o `_languageToScope`).
    pub fn language_scopes(&self) -> BTreeMap<&str, &str> {
        self.grammars
            .iter()
            .filter_map(|g| g.language.as_deref().map(|l| (l, g.scope_name.as_str())))
            .collect()
    }

    /// Injeções por escopo alvo, em ordem de registro (`_injections`).
    pub fn injections(&self) -> BTreeMap<&str, Vec<&str>> {
        let mut map: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for g in &self.grammars {
            for target in &g.inject_to {
                map.entry(target.as_str()).or_default().push(g.scope_name.as_str());
            }
        }
        map
    }

    /// Mapa `embeddedLanguages` efetivo de um escopo raiz: o da própria definição mais os das
    /// gramáticas injetadas nele (`createGrammar`).
    pub fn embedded_languages_for(&self, scope: &str) -> Vec<(String, String)> {
        let mut result: Vec<(String, String)> = self
            .grammars
            .iter()
            .rev()
            .find(|g| g.scope_name == scope)
            .map(|g| g.embedded_languages.clone())
            .unwrap_or_default();
        for g in self.grammars.iter().filter(|g| g.inject_to.iter().any(|t| t == scope)) {
            for (k, v) in &g.embedded_languages {
                result.retain(|(rk, _)| rk != k);
                result.push((k.clone(), v.clone()));
            }
        }
        result
    }
}
