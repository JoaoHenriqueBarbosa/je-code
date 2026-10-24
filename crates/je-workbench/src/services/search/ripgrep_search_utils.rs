//! Porte de `src/vs/workbench/services/search/node/ripgrepSearchUtils.ts`: utilitários da busca com o ripgrep.
//!
//! Divergências:
//! - `Maybe<T>` (`T | null | undefined`) é `Option<T>`.
//! - O `@ILogService` do construtor de `OutputChannel` é o [`LogServiceRef`] recebido. O `debug(message, msg)` do
//!   original, com a mensagem como argumento extra, é `debug_args` com um [`LogArg`].
//! - As linhas do stderr são `&str` (UTF-8): nenhum índice é usado, só `split('\n')` e `includes`.

use je_platform::log::{LogArg, LogServiceRef};

use super::search::SearchRange;
use super::search_ext_types::Range;

pub type Maybe<T> = Option<T>;

pub fn anchor_glob(glob: &str) -> String {
    if glob.starts_with("**") || glob.starts_with('/') { glob.to_owned() } else { format!("/{glob}") }
}

pub fn range_to_search_range(range: &Range) -> SearchRange {
    SearchRange::new(range.start.line, range.start.character, range.end.line, range.end.character)
}

pub fn search_range_to_range(range: &SearchRange) -> Range {
    Range::new(range.start_line_number, range.start_column, range.end_line_number, range.end_column)
}

/// O número de laços de link simbólico que uma execução do ripgrep pode relatar antes de pararmos de seguir
/// links simbólicos. O ripgrep só detecta links que apontam de volta para um ancestral, então uma árvore de links
/// simbólicos cruzados (por exemplo um sysfs falso) o faz enumerar um número exponencial de caminhos e a busca
/// na prática nunca termina. Workspaces comuns relatam cerca de um laço por link que aponta para si mesmo.
pub const MAX_RIPGREP_SYMLINK_LOOPS: usize = 1000;

/// Conta os laços de link simbólico que o ripgrep relata no stderr enquanto segue links simbólicos.
#[derive(Debug, Default)]
pub struct RipgrepSymlinkLoopDetector {
    loop_count: usize,
    partial_line: String,
}

impl RipgrepSymlinkLoopDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Devolve se o ripgrep já relatou mais de [`MAX_RIPGREP_SYMLINK_LOOPS`] laços.
    pub fn handle_stderr(&mut self, data: &str) -> bool {
        let joined = format!("{}{}", self.partial_line, data);
        let mut lines: Vec<&str> = joined.split('\n').collect();
        let last = lines.pop().unwrap_or("").to_owned();
        for line in lines {
            if line.contains("File system loop found") {
                self.loop_count += 1;
            }
        }
        self.partial_line = last;

        self.loop_count > MAX_RIPGREP_SYMLINK_LOOPS
    }
}

pub trait IOutputChannel {
    fn append_line(&self, msg: &str);
}

pub struct OutputChannel {
    prefix: String,
    log_service: LogServiceRef,
}

impl OutputChannel {
    pub fn new(prefix: impl Into<String>, log_service: LogServiceRef) -> Self {
        Self { prefix: prefix.into(), log_service }
    }
}

impl IOutputChannel for OutputChannel {
    fn append_line(&self, msg: &str) {
        self.log_service.debug_args(&format!("{}#search", self.prefix), &[LogArg::from(msg)]);
    }
}
