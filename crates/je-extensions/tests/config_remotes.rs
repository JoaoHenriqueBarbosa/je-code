//! Porte de `src/vs/platform/extensionManagement/test/common/configRemotes.test.ts` (suíte `Config Remotes`), caso a caso e na mesma ordem.

use je_extensions::config_remotes::{get_domains_of_remotes, get_remotes};

const ALLOWED_DOMAINS: [&str; 8] = ["github.com", "github2.com", "github3.com", "example.com", "example2.com", "example3.com", "server.org", "server2.org"];

fn remote(url: &str) -> String {
    format!("[remote \"origin\"]\n\turl = {url}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n")
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn https_remotes() {
    assert_eq!(get_domains_of_remotes(&remote("https://github.com/microsoft/vscode.git"), &ALLOWED_DOMAINS), strings(&["github.com"]));
    assert_eq!(get_domains_of_remotes(&remote("https://git.example.com/gitproject.git"), &ALLOWED_DOMAINS), strings(&["example.com"]));
    assert_eq!(get_domains_of_remotes(&remote("https://username@github2.com/username/repository.git"), &ALLOWED_DOMAINS), strings(&["github2.com"]));
    assert_eq!(get_domains_of_remotes(&remote("https://username:password@github3.com/username/repository.git"), &ALLOWED_DOMAINS), strings(&["github3.com"]));
    assert_eq!(get_domains_of_remotes(&remote("https://username:password@example2.com:1234/username/repository.git"), &ALLOWED_DOMAINS), strings(&["example2.com"]));
    assert_eq!(get_domains_of_remotes(&remote("https://example3.com:1234/username/repository.git"), &ALLOWED_DOMAINS), strings(&["example3.com"]));
}

#[test]
fn ssh_remotes() {
    assert_eq!(get_domains_of_remotes(&remote("ssh://user@git.server.org/project.git"), &ALLOWED_DOMAINS), strings(&["server.org"]));
}

#[test]
fn scp_like_remotes() {
    assert_eq!(get_domains_of_remotes(&remote("git@github.com:microsoft/vscode.git"), &ALLOWED_DOMAINS), strings(&["github.com"]));
    assert_eq!(get_domains_of_remotes(&remote("user@git.server.org:project.git"), &ALLOWED_DOMAINS), strings(&["server.org"]));
    assert_eq!(get_domains_of_remotes(&remote("git.server2.org:project.git"), &ALLOWED_DOMAINS), strings(&["server2.org"]));
}

#[test]
fn local_remotes() {
    assert_eq!(get_domains_of_remotes(&remote("/opt/git/project.git"), &ALLOWED_DOMAINS), strings(&[]));
    assert_eq!(get_domains_of_remotes(&remote("file:///opt/git/project.git"), &ALLOWED_DOMAINS), strings(&[]));
}

#[test]
fn multiple_remotes() {
    let config = [remote("https://github.com/microsoft/vscode.git"), remote("https://git.example.com/gitproject.git")].join("");
    let mut actual = get_domains_of_remotes(&config, &ALLOWED_DOMAINS);
    actual.sort();
    assert_eq!(actual, strings(&["example.com", "github.com"]));
}

#[test]
fn non_allowed_domains_are_anonymized() {
    let config = [remote("https://github.com/microsoft/vscode.git"), remote("https://git.foobar.com/gitproject.git")].join("");
    let mut actual = get_domains_of_remotes(&config, &ALLOWED_DOMAINS);
    actual.sort();
    assert_eq!(actual, strings(&["aaaaaa.aaa", "github.com"]));
}

#[test]
fn https_remotes_to_be_hashed() {
    assert_eq!(get_remotes(&remote("https://github.com/microsoft/vscode.git"), false), strings(&["github.com/microsoft/vscode.git"]));
    assert_eq!(get_remotes(&remote("https://git.example.com/gitproject.git"), false), strings(&["git.example.com/gitproject.git"]));
    assert_eq!(get_remotes(&remote("https://username@github2.com/username/repository.git"), false), strings(&["github2.com/username/repository.git"]));
    assert_eq!(get_remotes(&remote("https://username:password@github3.com/username/repository.git"), false), strings(&["github3.com/username/repository.git"]));
    assert_eq!(get_remotes(&remote("https://username:password@example2.com:1234/username/repository.git"), false), strings(&["example2.com/username/repository.git"]));
    assert_eq!(get_remotes(&remote("https://example3.com:1234/username/repository.git"), false), strings(&["example3.com/username/repository.git"]));

    // Strip .git
    assert_eq!(get_remotes(&remote("https://github.com/microsoft/vscode.git"), true), strings(&["github.com/microsoft/vscode"]));
    assert_eq!(get_remotes(&remote("https://git.example.com/gitproject.git"), true), strings(&["git.example.com/gitproject"]));
    assert_eq!(get_remotes(&remote("https://username@github2.com/username/repository.git"), true), strings(&["github2.com/username/repository"]));
    assert_eq!(get_remotes(&remote("https://username:password@github3.com/username/repository.git"), true), strings(&["github3.com/username/repository"]));
    assert_eq!(get_remotes(&remote("https://username:password@example2.com:1234/username/repository.git"), true), strings(&["example2.com/username/repository"]));
    assert_eq!(get_remotes(&remote("https://example3.com:1234/username/repository.git"), true), strings(&["example3.com/username/repository"]));

    // Compare Striped .git with no .git
    assert_eq!(get_remotes(&remote("https://github.com/microsoft/vscode.git"), true), get_remotes(&remote("https://github.com/microsoft/vscode"), false));
    assert_eq!(get_remotes(&remote("https://git.example.com/gitproject.git"), true), get_remotes(&remote("https://git.example.com/gitproject"), false));
    assert_eq!(get_remotes(&remote("https://username@github2.com/username/repository.git"), true), get_remotes(&remote("https://username@github2.com/username/repository"), false));
    assert_eq!(get_remotes(&remote("https://username:password@github3.com/username/repository.git"), true), get_remotes(&remote("https://username:password@github3.com/username/repository"), false));
    assert_eq!(get_remotes(&remote("https://username:password@example2.com:1234/username/repository.git"), true), get_remotes(&remote("https://username:password@example2.com:1234/username/repository"), false));
    assert_eq!(get_remotes(&remote("https://example3.com:1234/username/repository.git"), true), get_remotes(&remote("https://example3.com:1234/username/repository"), false));
}

#[test]
fn ssh_remotes_to_be_hashed() {
    assert_eq!(get_remotes(&remote("ssh://user@git.server.org/project.git"), false), strings(&["git.server.org/project.git"]));

    // Strip .git
    assert_eq!(get_remotes(&remote("ssh://user@git.server.org/project.git"), true), strings(&["git.server.org/project"]));

    // Compare Striped .git with no .git
    assert_eq!(get_remotes(&remote("ssh://user@git.server.org/project.git"), true), get_remotes(&remote("ssh://user@git.server.org/project"), false));
}

#[test]
fn scp_like_remotes_to_be_hashed() {
    assert_eq!(get_remotes(&remote("git@github.com:microsoft/vscode.git"), false), strings(&["github.com/microsoft/vscode.git"]));
    assert_eq!(get_remotes(&remote("user@git.server.org:project.git"), false), strings(&["git.server.org/project.git"]));
    assert_eq!(get_remotes(&remote("git.server2.org:project.git"), false), strings(&["git.server2.org/project.git"]));

    // Strip .git
    assert_eq!(get_remotes(&remote("git@github.com:microsoft/vscode.git"), true), strings(&["github.com/microsoft/vscode"]));
    assert_eq!(get_remotes(&remote("user@git.server.org:project.git"), true), strings(&["git.server.org/project"]));
    assert_eq!(get_remotes(&remote("git.server2.org:project.git"), true), strings(&["git.server2.org/project"]));

    // Compare Striped .git with no .git
    assert_eq!(get_remotes(&remote("git@github.com:microsoft/vscode.git"), true), get_remotes(&remote("git@github.com:microsoft/vscode"), false));
    assert_eq!(get_remotes(&remote("user@git.server.org:project.git"), true), get_remotes(&remote("user@git.server.org:project"), false));
    assert_eq!(get_remotes(&remote("git.server2.org:project.git"), true), get_remotes(&remote("git.server2.org:project"), false));
}

#[test]
fn local_remotes_to_be_hashed() {
    assert_eq!(get_remotes(&remote("/opt/git/project.git"), false), strings(&[]));
    assert_eq!(get_remotes(&remote("file:///opt/git/project.git"), false), strings(&[]));
}

#[test]
fn multiple_remotes_to_be_hashed() {
    let config = [remote("https://github.com/microsoft/vscode.git"), remote("https://git.example.com/gitproject.git")].join(" ");
    assert_eq!(get_remotes(&config, false), strings(&["github.com/microsoft/vscode.git", "git.example.com/gitproject.git"]));

    // Strip .git
    assert_eq!(get_remotes(&config, true), strings(&["github.com/microsoft/vscode", "git.example.com/gitproject"]));

    // Compare Striped .git with no .git
    let no_dot_git_config = [remote("https://github.com/microsoft/vscode"), remote("https://git.example.com/gitproject")].join(" ");
    assert_eq!(get_remotes(&config, true), get_remotes(&no_dot_git_config, false));
}
