//! Porte de `src/vs/workbench/services/search/common/ignoreFile.ts`: o leitor de `.gitignore` da busca.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - os `throw Error(...)` viram [`IgnoreFileError`] num `Result`.
//! - a função `isPathIgnored` que o original guarda no campo (e que o filho chama por `parent.isPathIgnored`,
//!   enxergando um `updateContents` posterior do pai) vive num estado compartilhado: o filho guarda uma
//!   referência ao estado do pai, não uma cópia da função.
//! - como no original, o `location` guardado é o recebido: a barra final acrescentada no construtor só
//!   valia para a variável local e nunca chegava ao `parseIgnoreFile`.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use je_base::glob::{self, Expression, GlobOptions, ParsedExpression};
use je_base::js_string::is_js_whitespace;
use je_base::strings::starts_with_ignore_case;

/// O `Error` que o original lança para um caminho em formato inesperado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgnoreFileError(pub String);

impl fmt::Display for IgnoreFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IgnoreFileError {}

type PathMatcher = Rc<dyn Fn(&str, bool) -> bool>;

/// O que o filho precisa enxergar do pai: o `isPathIgnored` corrente dele.
struct State {
    is_path_ignored: RefCell<PathMatcher>,
}

impl State {
    fn is_path_ignored(&self, path: &str, is_dir: bool) -> bool {
        let matcher = self.is_path_ignored.borrow().clone();
        matcher(path, is_dir)
    }
}

pub struct IgnoreFile {
    state: Rc<State>,
    location: String,
    parent: Option<Rc<State>>,
    ignore_case: bool,
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `String#trim` do JavaScript (o conjunto de espaços dele difere do `str::trim` do Rust).
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.len_utf16() == 1 && is_js_whitespace(c as u16))
}

fn unexpected_path_format(path: &str) -> IgnoreFileError {
    IgnoreFileError(format!("Unexpected path format, expected to begin with slash and end without. got:{path}"))
}

impl IgnoreFile {
    pub fn new(
        contents: &str,
        location: &str,
        parent: Option<&IgnoreFile>,
        ignore_case: bool,
    ) -> Result<Self, IgnoreFileError> {
        if location.ends_with('\\') {
            return Err(IgnoreFileError("Unexpected path format, do not use trailing backslashes".to_string()));
        }
        let parent = parent.map(|parent| parent.state.clone());
        let matcher = Self::parse_ignore_file(contents, location, parent.clone(), ignore_case);
        Ok(Self {
            state: Rc::new(State { is_path_ignored: RefCell::new(matcher) }),
            location: location.to_string(),
            parent,
            ignore_case,
        })
    }

    /// Atualiza o conteúdo do arquivo de ignore, preservando o local e o pai.
    pub fn update_contents(&self, contents: &str) {
        let matcher = Self::parse_ignore_file(contents, &self.location, self.parent.clone(), self.ignore_case);
        *self.state.is_path_ignored.borrow_mut() = matcher;
    }

    /// Verdadeiro se um caminho de um diretório percorrível não foi ignorado.
    ///
    /// Nota: por desempenho isto não confere se os diretórios pais foram ignorados, então deve sempre
    /// ser usado junto de `shouldTraverseDir` ao percorrer um diretório.
    ///
    /// Quando um caminho precisa ser testado isolado, use [`IgnoreFile::is_arbitrary_path_ignored`].
    pub fn is_path_included_in_traversal(&self, path: &str, is_dir: bool) -> Result<bool, IgnoreFileError> {
        if !path.starts_with('/') || path.ends_with('/') {
            return Err(unexpected_path_format(path));
        }

        let ignored = self.state.is_path_ignored(path, is_dir);

        Ok(!ignored)
    }

    /// Verdadeiro se um caminho qualquer foi ignorado. Operação cara, só para uso fora de travessias.
    pub fn is_arbitrary_path_ignored(&self, path: &str, is_dir: bool) -> Result<bool, IgnoreFileError> {
        if !path.starts_with('/') || path.ends_with('/') {
            return Err(unexpected_path_format(path));
        }

        let segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();
        let mut ignored = false;

        let mut walking_path = String::new();

        for (i, segment) in segments.iter().enumerate() {
            let is_last = i == segments.len() - 1;

            walking_path.push('/');
            walking_path.push_str(segment);

            if !self.is_path_included_in_traversal(&walking_path, if is_last { is_dir } else { true })? {
                ignored = true;
                break;
            }
        }

        Ok(ignored)
    }

    fn gitignore_lines_to_expression(
        lines: &[String],
        dir_path: &str,
        trim_for_exclusions: bool,
        ignore_case: bool,
    ) -> ParsedExpression {
        let mut include_expression = Expression::new();
        for line in lines {
            include_expression.insert(Self::gitignore_line_to_glob(line, dir_path), true);
        }

        glob::parse_expression(
            &include_expression,
            &GlobOptions { trim_for_exclusions, ignore_case: Some(ignore_case), ..GlobOptions::default() },
        )
    }

    fn parse_ignore_file(
        ignore_contents: &str,
        dir_path: &str,
        parent: Option<Rc<State>>,
        ignore_case: bool,
    ) -> PathMatcher {
        let content_lines: Vec<String> = ignore_contents
            .split('\n')
            .map(js_trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_string)
            .collect();

        let ignore_lines = |lines: &[String]| -> Vec<String> {
            lines.iter().filter(|line| !line.contains('!')).cloned().collect()
        };
        // TODO do original: ingenuidade que pode reintroduzir arquivos demais em .gitignore complexos.
        let include_lines = |lines: &[String]| -> Vec<String> {
            lines.iter().filter(|line| line.contains('!')).map(|line| line.replace('!', "")).collect()
        };

        // Pull out all the lines that end with `/`, those only apply to directories
        let file_lines: Vec<String> = content_lines.iter().filter(|line| !line.ends_with('/')).cloned().collect();

        let is_file_ignored =
            Self::gitignore_lines_to_expression(&ignore_lines(&file_lines), dir_path, true, ignore_case);
        let is_file_included =
            Self::gitignore_lines_to_expression(&include_lines(&file_lines), dir_path, false, ignore_case);

        // When checking if a dir is ignored we can use all lines
        let is_dir_ignored =
            Self::gitignore_lines_to_expression(&ignore_lines(&content_lines), dir_path, true, ignore_case);
        let is_dir_included =
            Self::gitignore_lines_to_expression(&include_lines(&content_lines), dir_path, false, ignore_case);

        let dir_path = dir_path.to_string();
        let matches = |expression: &ParsedExpression, path: &str| !expression.matches(path, None, None).is_none();

        Rc::new(move |path: &str, is_dir: bool| {
            let under_dir = if ignore_case {
                starts_with_ignore_case(&units(path), &units(&dir_path))
            } else {
                path.starts_with(dir_path.as_str())
            };
            if !under_dir {
                return false;
            }

            let dir_included = is_dir && matches(&is_dir_included, path);
            if is_dir && matches(&is_dir_ignored, path) && !dir_included {
                return true;
            }

            let file_included = matches(&is_file_included, path);
            if matches(&is_file_ignored, path) && !file_included {
                return true;
            }

            // If this file explicitly un-ignores a path via a negation pattern
            // (e.g., `!.myconfig/`), do not delegate to the parent. In git, a
            // negation in a child .gitignore overrides a positive pattern in a
            // parent or global .gitignore.
            if dir_included || file_included {
                return false;
            }

            if let Some(parent) = &parent {
                return parent.is_path_ignored(path, is_dir);
            }

            false
        })
    }

    fn gitignore_line_to_glob(line: &str, dir_path: &str) -> String {
        let mut line = line.to_string();
        match line.find('/') {
            None => line = format!("**/{line}"),
            Some(first_sep) if first_sep == line.len() - 1 => line = format!("**/{line}"),
            Some(first_sep) => {
                if first_sep == 0 {
                    if dir_path.ends_with('/') {
                        line = line[1..].to_string();
                    }
                } else if !dir_path.ends_with('/') {
                    line = format!("/{line}");
                }
                line = format!("{dir_path}{line}");
            }
        }

        line
    }
}
