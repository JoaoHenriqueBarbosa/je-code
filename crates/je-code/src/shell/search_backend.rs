//! O motor da busca em arquivos (`workbench/services/search`, no original o ripgrep): percorre o workspace numa
//! thread, abre cada arquivo de texto num `TextModel` e usa o `find_matches` do editor (a mesma busca do widget
//! de busca: maiúsculas, palavra inteira e regex idênticos) e devolve os resultados por lotes, com o aviso à
//! janela a cada lote. Uma nova busca cancela a anterior.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::Arc;

use je_base::JsString;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::text_model::{FindScope, TextModel, TextSource};
use je_editor::undo_redo_service::UndoRedoService;
use je_terminal::Wake;

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MATCHES_PER_FILE: usize = 10_000;
const MAX_TOTAL_MATCHES: usize = 20_000;
const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub text: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    /// O `files.exclude` mais o `search.exclude`.
    pub excludes: super::excludes::Excludes,
}

/// Uma linha com ocorrências: o texto (com o recuo cortado, como o `previewText` do VS Code) e as colunas dos
/// trechos casados dentro dele (em unidades UTF-16 do texto mostrado).
#[derive(Clone, Debug, PartialEq)]
pub struct LineMatch {
    pub line_number: i32,
    /// A coluna (1) da primeira ocorrência na linha original, para abrir o arquivo nela.
    pub column: i32,
    pub preview: String,
    pub ranges: Vec<(usize, usize)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileResult {
    pub path: PathBuf,
    pub lines: Vec<LineMatch>,
    pub match_count: usize,
}

pub struct Search {
    rx: Receiver<FileResult>,
    cancelled: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}

impl Search {
    /// Começa a busca de `query` em `root` e devolve o canal dos resultados.
    pub fn start(root: &Path, query: Query, wake: Wake) -> Self {
        let (tx, rx) = channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let (root, c, d) = (root.to_path_buf(), cancelled.clone(), done.clone());
        std::thread::spawn(move || {
            run(&root, &query, &tx, &c, &wake);
            d.store(true, Ordering::Release);
            wake();
        });
        Self { rx, cancelled, done }
    }

    /// Os resultados que chegaram desde a última chamada.
    pub fn poll(&self) -> Vec<FileResult> {
        self.rx.try_iter().collect()
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

fn ignored_names(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join(".gitignore"))
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('!'))
                .map(|l| l.trim_end_matches('/'))
                .filter(|l| !l.contains(['*', '?', '[', '/']))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn run(root: &Path, query: &Query, tx: &Sender<FileResult>, cancelled: &AtomicBool, wake: &Wake) {
    if query.text.is_empty() {
        return;
    }
    let ignored = ignored_names(root);
    let exclude = query.excludes.matcher();
    let mut pending = vec![root.to_path_buf()];
    let mut total = 0usize;
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(dir) = pending.pop() {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if exclude.excludes(root, &path) || ignored.contains(&name) {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    for path in files {
        if cancelled.load(Ordering::Relaxed) || total >= MAX_TOTAL_MATCHES {
            return;
        }
        if let Some(result) = search_file(&path, query) {
            total += result.match_count;
            if tx.send(result).is_err() {
                return;
            }
            wake();
        }
    }
}

/// Procura `query` no arquivo; `None` se não é texto, é grande demais ou não tem ocorrências.
pub fn search_file(path: &Path, query: &Query) -> Option<FileResult> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut options = TextModel::default_creation_options();
    options.detect_indentation = false;
    let model = TextModel::new(TextSource::from(text.as_ref()), "plaintext", options, None, UndoRedoService::for_tests());
    let separators = query.whole_word.then_some(WORD_SEPARATORS);
    let found = model.find_matches(JsString::from(query.text.as_str()), FindScope::Whole, query.regex, query.match_case, separators, false, MAX_MATCHES_PER_FILE);
    if found.is_empty() {
        return None;
    }
    let match_count = found.len();
    let mut lines: Vec<LineMatch> = Vec::new();
    for m in &found {
        let r = &m.range;
        let line_number = r.start_line_number;
        let content = model.get_line_content(line_number);
        let units = content.units();
        // O recuo sai do texto mostrado (o `previewText` do VS Code começa no primeiro caractere útil).
        let lead = units.iter().take_while(|u| **u == 0x20 || **u == 0x09).count();
        let end_column = if r.end_line_number == line_number { r.end_column } else { units.len() as i32 + 1 };
        let (start, end) = (((r.start_column - 1) as usize).max(lead) - lead, ((end_column - 1) as usize).max(lead) - lead);
        let cut: Vec<u16> = units[lead..].iter().copied().take(300).collect();
        if lines.last().is_none_or(|l| l.line_number != line_number) {
            lines.push(LineMatch { line_number, column: r.start_column, preview: String::from_utf16_lossy(&cut), ranges: Vec::new() });
        }
        if let Some(last) = lines.last_mut()
            && start < cut.len()
        {
            last.ranges.push((start, end.min(cut.len())));
        }
    }
    Some(FileResult { path: path.to_path_buf(), lines, match_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn query(text: &str) -> Query {
        Query { text: text.to_owned(), match_case: false, whole_word: false, regex: false, excludes: super::super::excludes::Excludes::search(&serde_json::Map::new()) }
    }

    #[test]
    fn matches_come_with_trimmed_previews_and_columns() {
        let dir = std::env::temp_dir().join(format!("je-code-{}-search", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).expect("pasta");
        std::fs::write(dir.join("a.txt"), "um Count\n    outro count aqui\nsem\n").expect("a");
        std::fs::write(dir.join("sub/b.txt"), "count count\n").expect("b");
        std::fs::write(dir.join("bin.dat"), b"count\0\0").expect("bin");
        let result = search_file(&dir.join("a.txt"), &query("count")).expect("acha");
        assert_eq!(result.match_count, 2);
        assert_eq!(result.lines[0], LineMatch { line_number: 1, column: 4, preview: "um Count".to_owned(), ranges: vec![(3, 8)] });
        assert_eq!(result.lines[1].preview, "outro count aqui", "o recuo sai do texto mostrado");
        assert_eq!(result.lines[1].ranges, vec![(6, 11)]);
        assert!(search_file(&dir.join("a.txt"), &Query { match_case: true, ..query("count") }).is_some_and(|r| r.match_count == 1), "maiúsculas contam");
        assert!(search_file(&dir.join("bin.dat"), &query("count")).is_none(), "binário fica de fora");
        let search = Search::start(&dir, query("count"), Arc::new(|| {}));
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut found = Vec::new();
        while !search.is_done() && Instant::now() < deadline {
            found.extend(search.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        found.extend(search.poll());
        let names: Vec<_> = found.iter().map(|r| r.path.strip_prefix(&dir).expect("dentro").to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["a.txt", "sub/b.txt"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
