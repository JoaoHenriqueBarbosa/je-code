//! Porte de `src/vs/workbench/contrib/files/common/explorerFileNestingTrie.ts`: o aninhamento de arquivos do
//! explorador (`explorer.fileNesting.patterns`).
//!
//! O `Map` e o `Set` do original iteram na ordem de inserção, então o porte usa [`OrderedMap`] (o `Set` é um
//! mapa com valor `()`). As chaves das tries são os caracteres do texto, como o `key[0]` do original.

use std::fmt::{self, Display};

use je_base::errors::{self, Error};
use je_base::map::OrderedMap;

/// `FilenameAttributes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilenameAttributes {
    /// `index.test` em `index.test.json`.
    pub basename: String,
    /// `json` em `index.test.json`.
    pub extname: String,
    /// `my-folder` em `my-folder/index.test.json`.
    pub dirname: String,
}

/// O `Set<string>` do original, na ordem de inserção.
pub type OrderedSet = OrderedMap<String, ()>;

/// O resultado de [`ExplorerFileNestingTrie::nest`]: pai para o conjunto dos filhos.
pub type NestingResult = OrderedMap<String, OrderedSet>;

/// A sort of double-ended trie, used to efficiently query for matches to "star" patterns, where
/// a given key represents a parent and may contain a capturing group ("*"), which can then be
/// referenced via the token "$(capture)" in associated child patterns.
///
/// The generated tree will have at most two levels, as subtrees are flattened rather than nested.
///
/// Example:
/// The config: [
/// [ *.ts , [ $(capture).*.ts ; $(capture).js ] ]
/// [ *.js , [ $(capture).min.js ] ] ]
/// Nests the files: [ a.ts ; a.d.ts ; a.js ; a.min.js ; b.ts ; b.min.js ]
/// As:
/// - a.ts => [ a.d.ts ; a.js ; a.min.js ]
/// - b.ts => [ ]
/// - b.min.ts => [ ]
pub struct ExplorerFileNestingTrie {
    root: PreTrie,
}

impl ExplorerFileNestingTrie {
    pub fn new(config: &[(&str, &[&str])]) -> Self {
        let mut root = PreTrie::new();
        for (parent_pattern, child_patterns) in config {
            for child_pattern in *child_patterns {
                root.add(parent_pattern, child_pattern);
            }
        }
        Self { root }
    }

    fn get_attributes(filename: &str, dirname: &str) -> FilenameAttributes {
        match filename.rfind('.') {
            Some(last_dot) if last_dot >= 1 => FilenameAttributes {
                dirname: dirname.to_string(),
                basename: filename[..last_dot].to_string(),
                extname: filename[last_dot + 1..].to_string(),
            },
            _ => FilenameAttributes {
                dirname: dirname.to_string(),
                basename: filename.to_string(),
                extname: String::new(),
            },
        }
    }

    pub fn nest(&self, files: &[&str], dirname: &str) -> NestingResult {
        let mut parent_finder = PreTrie::new();

        for potential_parent in files {
            let attributes = Self::get_attributes(potential_parent, dirname);
            let children = self.root.get(potential_parent, &attributes);
            for child in children {
                parent_finder.add(&child, potential_parent);
            }
        }

        let mut result = NestingResult::new();
        for file in files {
            let mut seen = OrderedSet::new();
            let mut ancestors = Self::find_all_root_ancestors(&parent_finder, dirname, file, &mut seen);
            if ancestors.is_empty() {
                ancestors = vec![file.to_string()];
            }
            for ancestor in ancestors {
                if !result.contains_key(&ancestor) {
                    result.insert(ancestor.clone(), OrderedSet::new());
                }
                if *file != ancestor {
                    result.get_mut(&ancestor).expect("acabou de inserir").insert(file.to_string(), ());
                }
            }
        }
        result
    }

    fn find_all_root_ancestors(
        parent_finder: &PreTrie,
        dirname: &str,
        file: &str,
        seen: &mut OrderedSet,
    ) -> Vec<String> {
        if seen.contains_key(&file.to_string()) {
            return Vec::new();
        }
        seen.insert(file.to_string(), ());
        let attributes = Self::get_attributes(file, dirname);
        // A file may match its own child patterns (possibly several times, e.g. `${basename}.*`
        // and `${basename}*.${extname}`), but it should never be treated as its own parent.
        let mut ancestors = OrderedSet::new();
        for ancestor in parent_finder.get(file, &attributes) {
            ancestors.insert(ancestor, ());
        }
        ancestors.remove(&file.to_string());
        if ancestors.is_empty() {
            return vec![file.to_string()];
        }

        ancestors
            .keys()
            .flat_map(|ancestor| Self::find_all_root_ancestors(parent_finder, dirname, ancestor, seen))
            .collect()
    }
}

impl Display for ExplorerFileNestingTrie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.root.to_string_indented(""))
    }
}

/// Divide o texto no primeiro caractere (`key[0]` e `key.slice(1)`).
fn split_head(key: &str) -> Option<(char, &str)> {
    let mut chars = key.chars();
    let head = chars.next()?;
    Some((head, chars.as_str()))
}

/// Divide o texto no último caractere (`key[key.length - 1]` e `key.slice(0, key.length - 1)`).
fn split_tail(key: &str) -> Option<(char, &str)> {
    let tail = key.chars().next_back()?;
    Some((tail, &key[..key.len() - tail.len_utf8()]))
}

/// Junta as linhas com a indentação na frente de cada uma (`lines.map(l => indentation + l).join('\n')`).
fn indent_lines(lines: &[String], indentation: &str) -> String {
    lines.iter().map(|line| format!("{indentation}{line}")).collect::<Vec<_>>().join("\n")
}

/// Export for test only.
#[derive(Default)]
pub struct PreTrie {
    value: SufTrie,
    map: OrderedMap<char, PreTrie>,
}

impl PreTrie {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, key: &str, value: &str) {
        match split_head(key) {
            None | Some(('*', _)) => self.value.add(key, value),
            Some((head, rest)) => {
                if !self.map.contains_key(&head) {
                    self.map.insert(head, PreTrie::new());
                }
                self.map.get_mut(&head).expect("acabou de inserir").add(rest, value);
            }
        }
    }

    pub fn get(&self, key: &str, attributes: &FilenameAttributes) -> Vec<String> {
        let mut results = self.value.get(key, attributes);

        if let Some((head, rest)) = split_head(key) {
            if let Some(existing) = self.map.get(&head) {
                results.extend(existing.get(rest, attributes));
            }
        }

        results
    }

    /// `toString(indentation)`.
    pub fn to_string_indented(&self, indentation: &str) -> String {
        let mut lines = Vec::new();
        if self.value.has_items {
            lines.push(format!("* => \n{}", self.value.to_string_indented(&format!("{indentation}  "))));
        }
        for (key, trie) in self.map.iter() {
            lines.push(format!("^{key} => \n{}", trie.to_string_indented(&format!("{indentation}  "))));
        }
        indent_lines(&lines, indentation)
    }
}

impl Display for PreTrie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_indented(""))
    }
}

/// Export for test only.
#[derive(Default)]
pub struct SufTrie {
    star: Vec<SubstitutionString>,
    epsilon: Vec<SubstitutionString>,
    map: OrderedMap<char, SufTrie>,
    pub has_items: bool,
}

impl SufTrie {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, key: &str, value: &str) {
        self.has_items = true;
        if key == "*" {
            self.star.push(SubstitutionString::new(value));
        } else if key.is_empty() {
            self.epsilon.push(SubstitutionString::new(value));
        } else if let Some((tail, rest)) = split_tail(key) {
            if tail == '*' {
                errors::throw(Error::new(&format!("Unexpected star in SufTrie key: {key}")));
            }
            if !self.map.contains_key(&tail) {
                self.map.insert(tail, SufTrie::new());
            }
            self.map.get_mut(&tail).expect("acabou de inserir").add(rest, value);
        }
    }

    pub fn get(&self, key: &str, attributes: &FilenameAttributes) -> Vec<String> {
        let mut results = Vec::new();
        if key.is_empty() {
            results.extend(self.epsilon.iter().map(|ss| ss.substitute(attributes, None)));
        }
        if !self.star.is_empty() {
            results.extend(self.star.iter().map(|ss| ss.substitute(attributes, Some(key))));
        }

        if let Some((tail, rest)) = split_tail(key) {
            if let Some(existing) = self.map.get(&tail) {
                results.extend(existing.get(rest, attributes));
            }
        }

        results
    }

    /// `toString(indentation)`.
    pub fn to_string_indented(&self, indentation: &str) -> String {
        let mut lines = Vec::new();
        if !self.star.is_empty() {
            lines.push(format!("* => {}", join_substitutions(&self.star)));
        }

        if !self.epsilon.is_empty() {
            lines.push(format!("ε => {}", join_substitutions(&self.epsilon)));
        }

        for (key, trie) in self.map.iter() {
            lines.push(format!("{key}$ => \n{}", trie.to_string_indented(&format!("{indentation}  "))));
        }

        indent_lines(&lines, indentation)
    }
}

impl Display for SufTrie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_indented(""))
    }
}

/// `this.star.join('; ')`: o original não define `toString` em `SubstitutionString`, então cada item sai
/// como `[object Object]`.
fn join_substitutions(items: &[SubstitutionString]) -> String {
    items.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubstitutionType {
    Capture,
    Basename,
    Dirname,
    Extname,
}

/// Os nomes do tokenizador `/\$[({](capture|basename|dirname|extname)[)}]/g`.
const SUBSTITUTION_NAMES: [(&str, SubstitutionType); 4] = [
    ("capture", SubstitutionType::Capture),
    ("basename", SubstitutionType::Basename),
    ("dirname", SubstitutionType::Dirname),
    ("extname", SubstitutionType::Extname),
];

#[derive(Debug)]
enum Token {
    Text(String),
    Capture(SubstitutionType),
}

#[derive(Debug)]
struct SubstitutionString {
    tokens: Vec<Token>,
}

impl SubstitutionString {
    fn new(pattern: &str) -> Self {
        let bytes = pattern.as_bytes();
        let mut tokens = Vec::new();
        let mut last_index = 0;
        let mut index = 0;
        while index < bytes.len() {
            let mut matched = None;
            if bytes[index] == b'$' && index + 1 < bytes.len() && matches!(bytes[index + 1], b'(' | b'{') {
                let rest = &pattern[index + 2..];
                for (name, kind) in SUBSTITUTION_NAMES {
                    let after = index + 2 + name.len();
                    if rest.starts_with(name) && after < bytes.len() && matches!(bytes[after], b')' | b'}') {
                        matched = Some((kind, after + 1));
                        break;
                    }
                }
            }
            match matched {
                Some((kind, end)) => {
                    tokens.push(Token::Text(pattern[last_index..index].to_string()));
                    tokens.push(Token::Capture(kind));
                    last_index = end;
                    index = end;
                }
                None => index += 1,
            }
        }

        if last_index != pattern.len() {
            tokens.push(Token::Text(pattern[last_index..].to_string()));
        }
        Self { tokens }
    }

    fn substitute(&self, attributes: &FilenameAttributes, capture: Option<&str>) -> String {
        self.tokens
            .iter()
            .map(|token| match token {
                Token::Text(text) => text.as_str(),
                Token::Capture(SubstitutionType::Basename) => attributes.basename.as_str(),
                Token::Capture(SubstitutionType::Dirname) => attributes.dirname.as_str(),
                Token::Capture(SubstitutionType::Extname) => attributes.extname.as_str(),
                Token::Capture(SubstitutionType::Capture) => capture.unwrap_or(""),
            })
            .collect()
    }
}

impl Display for SubstitutionString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[object Object]")
    }
}
