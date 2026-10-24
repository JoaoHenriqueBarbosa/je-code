//! Porte de `src/vs/code/test/node/cliArgs.test.ts` (suíte `combineUriFlags`), caso a caso e na mesma ordem.

use je_code::cli_args::combine_uri_flags;

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

#[test]
fn rewrites_folder_uri_and_file_uri_followed_by_a_uri_into_flag_value() {
    assert_eq!(
        combine_uri_flags(&strings(&[
            "--wait",
            "--folder-uri",
            "vscode-remote://ssh-remote+host/workspace",
            "--file-uri",
            "vscode-remote://ssh-remote+host/file.txt",
            "--new-window",
            "--folder-uri=vscode-remote://already-joined/workspace",
            "--folder-uri", // trailing flag with no value
        ])),
        strings(&[
            "--wait",
            "--folder-uri=vscode-remote://ssh-remote+host/workspace",
            "--file-uri=vscode-remote://ssh-remote+host/file.txt",
            "--new-window",
            "--folder-uri=vscode-remote://already-joined/workspace",
            "--folder-uri",
        ])
    );
}

#[test]
fn does_not_join_when_next_argument_is_a_flag() {
    assert_eq!(combine_uri_flags(&strings(&["--folder-uri", "--wait", "somepath"])), strings(&["--folder-uri", "--wait", "somepath"]));
}

#[test]
fn leaves_unrelated_arguments_untouched() {
    assert_eq!(combine_uri_flags(&strings(&["--wait", "--new-window", "C:\\some\\path"])), strings(&["--wait", "--new-window", "C:\\some\\path"]));
}

#[test]
fn does_not_rewrite_past_the_end_of_options_marker() {
    assert_eq!(
        combine_uri_flags(&strings(&[
            "--wait",
            "--folder-uri",
            "vscode-remote://host/before",
            "--",
            "--folder-uri",
            "vscode-remote://host/after",
            "--file-uri",
            "vscode-remote://host/file.txt",
        ])),
        strings(&[
            "--wait",
            "--folder-uri=vscode-remote://host/before",
            "--",
            "--folder-uri",
            "vscode-remote://host/after",
            "--file-uri",
            "vscode-remote://host/file.txt",
        ])
    );
}
