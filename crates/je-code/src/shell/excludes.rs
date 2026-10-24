//! O `files.exclude` e o `search.exclude` do usuário, com os padrões do VS Code e a mesma regra de mescla dos
//! objetos de configuração (o do usuário soma aos padrões e pode desligar um com `false`), casados pelo
//! `je_base::glob` (o `glob.ts` portado) contra o caminho relativo à pasta do workspace.

use std::path::Path;

use je_base::glob::{Expression, ExprValue, GlobOptions, ParsedExpression, parse_expression};
use serde_json::Value;

/// O padrão de `files.exclude` do VS Code 1.143.
const FILES_EXCLUDE: [&str; 6] = ["**/.git", "**/.svn", "**/.hg", "**/.DS_Store", "**/Thumbs.db", "**/CVS"];
/// O padrão de `search.exclude` (que soma ao `files.exclude` na busca).
const SEARCH_EXCLUDE: [&str; 3] = ["**/node_modules", "**/bower_components", "**/*.code-search"];

/// Os padrões ligados ou desligados, na ordem (`Send`: atravessa para a thread da busca).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Excludes(Vec<(String, bool)>);

impl Excludes {
    fn merged(defaults: &[&str], user: Option<&Value>) -> Self {
        let mut list: Vec<(String, bool)> = defaults.iter().map(|p| ((*p).to_owned(), true)).collect();
        if let Some(Value::Object(map)) = user {
            for (pattern, value) in map {
                // `{ "when": ... }` e outros valores que não `false` ligam o padrão, como o `ExprValue::Other`.
                let enabled = !matches!(value, Value::Bool(false));
                match list.iter_mut().find(|(p, _)| p == pattern) {
                    Some(entry) => entry.1 = enabled,
                    None => list.push((pattern.clone(), enabled)),
                }
            }
        }
        Self(list)
    }

    /// O `files.exclude` do `settings.json` sobre os padrões.
    pub fn files(settings: &serde_json::Map<String, Value>) -> Self {
        Self::merged(&FILES_EXCLUDE, settings.get("files.exclude"))
    }

    /// O `files.exclude` mais o `search.exclude`, como a busca do VS Code usa.
    pub fn search(settings: &serde_json::Map<String, Value>) -> Self {
        let mut all = Self::files(settings);
        all.0.extend(Self::merged(&SEARCH_EXCLUDE, settings.get("search.exclude")).0);
        all
    }

    pub fn matcher(&self) -> Matcher {
        let expression = Expression::from_entries(self.0.iter().map(|(p, on)| (p.clone(), ExprValue::Bool(*on))));
        Matcher(parse_expression(&expression, &GlobOptions::default()))
    }
}

/// A expressão já compilada.
pub struct Matcher(ParsedExpression);

impl Matcher {
    /// Se `path` (dentro de `root`) está excluído.
    pub fn excludes(&self, root: &Path, path: &Path) -> bool {
        let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        self.0.matches(&relative, name.as_deref(), None).matched().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_merge_with_the_user_object() {
        let settings: serde_json::Map<String, Value> = serde_json::from_str(r#"{"files.exclude": {"**/.git": false, "**/target": true}}"#).expect("json");
        let m = Excludes::files(&settings).matcher();
        let root = Path::new("/w");
        assert!(!m.excludes(root, Path::new("/w/.git")), "o usuário desligou o padrão");
        assert!(m.excludes(root, Path::new("/w/target")));
        assert!(m.excludes(root, Path::new("/w/sub/.DS_Store")), "os outros padrões continuam");
        assert!(!m.excludes(root, Path::new("/w/src")));
        let search = Excludes::search(&settings).matcher();
        assert!(search.excludes(root, Path::new("/w/node_modules")));
        assert!(!m.excludes(root, Path::new("/w/node_modules")), "o node_modules só sai da busca");
    }
}
