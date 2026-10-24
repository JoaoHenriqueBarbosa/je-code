//! O Git do workspace, como a extensão `git` do VS Code o usa: pelo executável `git` do sistema (`git status
//! --porcelain=v1 -z`, `git add`, `git restore --staged`, `git commit`), sem biblioteca. As operações rodam numa
//! thread e devolvem o estado novo, com o aviso à janela, para a interface não travar em repositórios grandes.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

use je_terminal::Wake;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Conflict,
    TypeChanged,
}

impl Kind {
    /// A letra que o VS Code mostra à direita (`M`, `A`, `D`, `R`, `U`...).
    pub fn letter(self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Copied => "C",
            Self::Untracked => "U",
            Self::Conflict => "!",
            Self::TypeChanged => "T",
        }
    }

    /// O id da cor de decoração (`gitDecoration.*ResourceForeground`).
    pub fn color_id(self) -> &'static str {
        match self {
            Self::Modified | Self::TypeChanged => "gitDecoration.modifiedResourceForeground",
            Self::Added => "gitDecoration.addedResourceForeground",
            Self::Deleted => "gitDecoration.deletedResourceForeground",
            Self::Renamed | Self::Copied => "gitDecoration.renamedResourceForeground",
            Self::Untracked => "gitDecoration.untrackedResourceForeground",
            Self::Conflict => "gitDecoration.conflictingResourceForeground",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// O caminho relativo à raiz do repositório, com `/`.
    pub path: String,
    pub kind: Kind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: String,
    /// O ramo tem um remoto de acompanhamento (`main...origin/main`).
    pub upstream: bool,
    pub ahead: u32,
    pub behind: u32,
    pub staged: Vec<Change>,
    /// As mudanças da árvore de trabalho, inclusive os arquivos novos (não rastreados) e os conflitos.
    pub changes: Vec<Change>,
}

impl Status {
    pub fn count(&self) -> usize {
        self.staged.len() + self.changes.len()
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "core.quotepath=off"])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Não consegui rodar o git: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if message.is_empty() { String::from_utf8_lossy(&output.stdout).trim().to_owned() } else { message })
    }
}

/// A raiz do repositório que contém `dir` (ou `None` se não é de um repositório ou o git não existe).
pub fn find_root(dir: &Path) -> Option<PathBuf> {
    git(dir, &["rev-parse", "--show-toplevel"]).ok().map(|s| PathBuf::from(s.trim()))
}

/// O conteúdo de `path` (relativo à raiz) no HEAD, para o diff da margem; `None` se o arquivo é novo ou não há commits.
pub fn head_text(root: &Path, path: &str) -> Option<String> {
    git(root, &["show", &format!("HEAD:{path}")]).ok()
}

/// O histórico de um arquivo para a Timeline: assunto e data relativa dos últimos 50 commits que o tocaram.
pub fn file_history(root: &Path, path: &Path) -> Vec<(String, String)> {
    let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
    git(root, &["log", "-n", "50", "--follow", "--format=%s%x09%ar", "--", &relative])
        .map(|out| out.lines().filter_map(|l| l.split_once('\t')).map(|(s, d)| (s.to_owned(), d.to_owned())).collect())
        .unwrap_or_default()
}

fn kind_of(code: char) -> Option<Kind> {
    Some(match code {
        'M' => Kind::Modified,
        'A' => Kind::Added,
        'D' => Kind::Deleted,
        'R' => Kind::Renamed,
        'C' => Kind::Copied,
        'T' => Kind::TypeChanged,
        _ => return None,
    })
}

/// Lê a saída de `git status --porcelain=v1 -z -b`.
pub fn parse_status(output: &str) -> Status {
    let mut status = Status::default();
    let mut records = output.split('\0');
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("## ") {
            // `main...origin/main [ahead 1, behind 2]`, `main`, `No commits yet on main` ou `HEAD (no branch)`.
            let (name, tracking) = header.split_once(" [").map_or((header, ""), |(n, t)| (n, t));
            status.branch = name.strip_prefix("No commits yet on ").unwrap_or(name).split("...").next().unwrap_or(name).to_owned();
            status.upstream = name.contains("...");
            for part in tracking.trim_end_matches(']').split(", ") {
                if let Some(n) = part.strip_prefix("ahead ") {
                    status.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix("behind ") {
                    status.behind = n.parse().unwrap_or(0);
                }
            }
            continue;
        }
        let mut chars = record.chars();
        let (Some(x), Some(y)) = (chars.next(), chars.next()) else { continue };
        let path: String = record.chars().skip(3).collect();
        // Renomeações e cópias trazem o caminho antigo no registro seguinte.
        if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            records.next();
        }
        let conflict = matches!((x, y), ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D'));
        if conflict {
            status.changes.push(Change { path, kind: Kind::Conflict });
        } else if (x, y) == ('?', '?') {
            status.changes.push(Change { path, kind: Kind::Untracked });
        } else {
            if let Some(kind) = kind_of(x) {
                status.staged.push(Change { path: path.clone(), kind });
            }
            if let Some(kind) = kind_of(y) {
                status.changes.push(Change { path, kind });
            }
        }
    }
    status
}

pub fn status(root: &Path) -> Result<Status, String> {
    git(root, &["status", "--porcelain=v1", "-z", "-b", "--untracked-files=all"]).map(|out| parse_status(&out))
}

/// Uma operação sobre o repositório (a ordem do `Repository` da extensão).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Stage(Vec<String>),
    Unstage(Vec<String>),
    /// Descarta as mudanças da árvore de trabalho: o arquivo volta ao índice, o novo é apagado.
    Discard(Vec<Change>),
    Commit(String),
    Refresh,
    /// `git fetch` (o `git.autofetch` roda este a cada `git.autofetchPeriod`).
    Fetch,
    Pull,
    Push,
    /// O `Sync` da barra de status: pull e depois push.
    Sync,
}

fn run_operation(root: &Path, op: &Operation) -> Result<(), String> {
    let paths = |list: &[String]| -> Vec<String> { list.to_vec() };
    match op {
        Operation::Refresh => Ok(()),
        Operation::Stage(list) => {
            let mut args = vec!["add".to_owned(), "-A".to_owned(), "--".to_owned()];
            args.extend(paths(list));
            git(root, &args.iter().map(String::as_str).collect::<Vec<_>>()).map(|_| ())
        }
        Operation::Unstage(list) => {
            let mut args = vec!["reset".to_owned(), "-q".to_owned(), "HEAD".to_owned(), "--".to_owned()];
            args.extend(paths(list));
            // Sem commits ainda não existe `HEAD`: tira do índice com `rm --cached`.
            if git(root, &args.iter().map(String::as_str).collect::<Vec<_>>()).is_err() {
                let mut args = vec!["rm".to_owned(), "-r".to_owned(), "-q".to_owned(), "--cached".to_owned(), "--".to_owned()];
                args.extend(paths(list));
                return git(root, &args.iter().map(String::as_str).collect::<Vec<_>>()).map(|_| ());
            }
            Ok(())
        }
        Operation::Discard(changes) => {
            for change in changes {
                if change.kind == Kind::Untracked {
                    let path = root.join(&change.path);
                    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                } else {
                    git(root, &["checkout", "--", &change.path])?;
                }
            }
            Ok(())
        }
        Operation::Commit(message) => git(root, &["commit", "-m", message]).map(|_| ()),
        Operation::Fetch => git(root, &["fetch", "--quiet"]).map(|_| ()),
        Operation::Pull => git(root, &["pull", "--quiet"]).map(|_| ()),
        Operation::Push => git(root, &["push", "--quiet"]).map(|_| ()),
        Operation::Sync => git(root, &["pull", "--quiet"]).and_then(|_| git(root, &["push", "--quiet"])).map(|_| ()),
    }
}

/// Roda a operação numa thread e devolve o canal com o estado novo (ou o erro).
pub fn spawn(root: PathBuf, op: Operation, wake: Wake) -> Receiver<Result<Status, String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let result = run_operation(&root, &op).and_then(|()| status(&root));
        let _ = tx.send(result);
        wake();
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("je-code-{}-git", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).expect("pasta");
        let run = |args: &[&str]| {
            git(&dir, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "a@b.c"]);
        run(&["config", "user.name", "teste"]);
        std::fs::write(dir.join("notes.txt"), "one\ntwo\n").expect("notes");
        std::fs::write(dir.join("old.txt"), "old\n").expect("old");
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "first"]);
        dir
    }

    #[test]
    fn parses_branch_and_the_two_columns() {
        assert!(parse_status("## main...origin/main\0").upstream);
        assert!(!parse_status("## main\0").upstream);
        let status = parse_status("## main...origin/main [ahead 1, behind 2]\0 M notes.txt\0M  src/a.rs\0D  old.txt\0?? fresh.txt\0R  new.txt\0was.txt\0UU both.txt\0");
        assert_eq!((status.branch.as_str(), status.ahead, status.behind), ("main", 1, 2));
        let staged: Vec<_> = status.staged.iter().map(|c| (c.path.as_str(), c.kind)).collect();
        assert_eq!(staged, vec![("src/a.rs", Kind::Modified), ("old.txt", Kind::Deleted), ("new.txt", Kind::Renamed)]);
        let changes: Vec<_> = status.changes.iter().map(|c| (c.path.as_str(), c.kind)).collect();
        assert_eq!(changes, vec![("notes.txt", Kind::Modified), ("fresh.txt", Kind::Untracked), ("both.txt", Kind::Conflict)]);
        assert_eq!(parse_status("## No commits yet on main\0").branch, "main");
    }

    #[test]
    fn stage_unstage_discard_and_commit_run_git() {
        let dir = repo();
        std::fs::write(dir.join("notes.txt"), "one\nTWO\n").expect("edita");
        std::fs::write(dir.join("fresh.txt"), "new\n").expect("novo");
        let root = find_root(&dir).expect("é um repositório");
        let wake: Wake = std::sync::Arc::new(|| {});
        let run = |op: Operation| spawn(root.clone(), op, wake.clone()).recv().expect("resposta").expect("git ok");
        let status = run(Operation::Refresh);
        assert_eq!(status.count(), 2);
        let status = run(Operation::Stage(vec!["notes.txt".to_owned(), "fresh.txt".to_owned()]));
        assert_eq!((status.staged.len(), status.changes.len()), (2, 0));
        let status = run(Operation::Unstage(vec!["fresh.txt".to_owned()]));
        assert_eq!(status.changes, vec![Change { path: "fresh.txt".to_owned(), kind: Kind::Untracked }]);
        let status = run(Operation::Discard(status.changes.clone()));
        assert!(!dir.join("fresh.txt").exists() && status.changes.is_empty(), "o arquivo novo foi apagado");
        let status = run(Operation::Commit("segundo".to_owned()));
        assert_eq!(status.count(), 0, "o commit levou o que estava no índice");
        assert!(git(&dir, &["log", "--oneline"]).expect("log").contains("segundo"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
