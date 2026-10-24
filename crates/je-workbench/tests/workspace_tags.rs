//! Porte de `src/vs/workbench/contrib/tags/test/node/workspaceTags.test.ts` (suíte `Telemetry - WorkspaceTags`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo de cada caso.
//! - **`await`.** Cada `await getHashedRemotesFromConfig(...)` esvazia as microtarefas ([`get_hashed_remotes`]) e lê o valor
//!   da promessa resolvida.
//! - **`crypto.createHash('sha256')`.** É o `sha2::Sha256` em hexadecimal minúsculo; `asyncHash` devolve a promessa resolvida.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::promise::Promise;
use je_base::runtime;
use je_workbench::contrib::tags::common::workspace_tags::get_hashed_remotes_from_config;
use sha2::{Digest, Sha256};

fn hash(value: &str) -> String {
    Sha256::digest(value.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `getHashedRemotesFromConfig(text, stripEndingDotGit = false)` do teste, esperado até o fim.
fn get_hashed_remotes(text: &str, strip_ending_dot_git: bool) -> Vec<String> {
    let result = get_hashed_remotes_from_config(text, strip_ending_dot_git, |remote| Promise::resolved(hash(remote)));
    runtime::run_microtasks();
    result.result().expect("a promessa resolveu").expect("sem erro")
}

fn remote(url: &str) -> String {
    format!("[remote \"origin\"]\n\turl = {url}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n")
}

#[test]
fn single_remote_hashed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_hashed_remotes(&remote("https://username:password@github3.com/username/repository.git"), false), vec![hash("github3.com/username/repository.git")]);
    assert_eq!(get_hashed_remotes(&remote("ssh://user@git.server.org/project.git"), false), vec![hash("git.server.org/project.git")]);
    assert_eq!(get_hashed_remotes(&remote("user@git.server.org:project.git"), false), vec![hash("git.server.org/project.git")]);
    assert_eq!(get_hashed_remotes(&remote("/opt/git/project.git"), false), Vec::<String>::new());

    // Strip .git
    assert_eq!(get_hashed_remotes(&remote("https://username:password@github3.com/username/repository.git"), true), vec![hash("github3.com/username/repository")]);
    assert_eq!(get_hashed_remotes(&remote("ssh://user@git.server.org/project.git"), true), vec![hash("git.server.org/project")]);
    assert_eq!(get_hashed_remotes(&remote("user@git.server.org:project.git"), true), vec![hash("git.server.org/project")]);
    assert_eq!(get_hashed_remotes(&remote("/opt/git/project.git"), true), Vec::<String>::new());

    // Compare Striped .git with no .git
    assert_eq!(
        get_hashed_remotes(&remote("https://username:password@github3.com/username/repository.git"), true),
        get_hashed_remotes(&remote("https://username:password@github3.com/username/repository"), false)
    );
    assert_eq!(get_hashed_remotes(&remote("ssh://user@git.server.org/project.git"), true), get_hashed_remotes(&remote("ssh://user@git.server.org/project"), false));
    assert_eq!(get_hashed_remotes(&remote("user@git.server.org:project.git"), true), vec![hash("git.server.org/project")]);
    assert_eq!(get_hashed_remotes(&remote("/opt/git/project.git"), true), get_hashed_remotes(&remote("/opt/git/project"), false));
}

#[test]
fn multiple_remotes_hashed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let config = ["https://github.com/microsoft/vscode.git", "https://git.example.com/gitproject.git"].map(remote).join(" ");
    assert_eq!(get_hashed_remotes(&config, false), vec![hash("github.com/microsoft/vscode.git"), hash("git.example.com/gitproject.git")]);

    // Strip .git
    assert_eq!(get_hashed_remotes(&config, true), vec![hash("github.com/microsoft/vscode"), hash("git.example.com/gitproject")]);

    // Compare Striped .git with no .git
    let no_dot_git_config = ["https://github.com/microsoft/vscode", "https://git.example.com/gitproject"].map(remote).join(" ");
    assert_eq!(get_hashed_remotes(&config, true), get_hashed_remotes(&no_dot_git_config, false));
}
