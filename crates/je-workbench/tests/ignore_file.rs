//! Porte de `src/vs/workbench/services/search/test/common/ignoreFile.test.ts` (suíte `Parsing .gitignore files`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - Os literais de várias linhas do original (indentados com tabs, que o `trim` por linha descarta) viram
//!   [`lines`], que junta as linhas com `\n`.
//! - Os `throw` do original viram `Err`, que os casos desembrulham: nenhum caso espera o erro.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::services::search::ignore_file::IgnoreFile;

fn lines(parts: &[&str]) -> String {
    parts.join("\n")
}

fn run_assert(
    input: &str,
    ignore_file: &str,
    ignore_file_location: &str,
    should_match: bool,
    traverse: bool,
    ignore_case: bool,
) -> impl Fn(&str) {
    let input = input.to_string();
    let ignore_file = ignore_file.to_string();
    let ignore_file_location = ignore_file_location.to_string();
    move |prefix: &str| {
        let is_dir = input.ends_with('/');
        let raw_input = if is_dir { &input[..input.len() - 1] } else { input.as_str() };

        let matcher =
            IgnoreFile::new(&ignore_file, &format!("{prefix}{ignore_file_location}"), None, ignore_case).unwrap();
        let kind = if is_dir { "dir" } else { "file" };
        if traverse {
            let traverses = matcher.is_path_included_in_traversal(&format!("{prefix}{raw_input}"), is_dir).unwrap();

            if should_match {
                assert!(traverses, "{ignore_file_location}: {ignore_file} should traverse {kind} {prefix}{raw_input}");
            } else {
                assert!(
                    !traverses,
                    "{ignore_file_location}: {ignore_file} should not traverse {kind} {prefix}{raw_input}"
                );
            }
        } else {
            let ignores = matcher.is_arbitrary_path_ignored(&format!("{prefix}{raw_input}"), is_dir).unwrap();

            if should_match {
                assert!(ignores, "{ignore_file_location}: {ignore_file} should ignore {kind} {prefix}{raw_input}");
            } else {
                assert!(
                    !ignores,
                    "{ignore_file_location}: {ignore_file} should not ignore {kind} {prefix}{raw_input}"
                );
            }
        }
    }
}

fn assert_no_traverses(ignore_file: &str, ignore_file_location: &str, input: &str, ignore_case: bool) {
    let run_with_prefix = run_assert(input, ignore_file, ignore_file_location, false, true, ignore_case);

    run_with_prefix("");
    run_with_prefix("/someFolder");
}

fn assert_traverses(ignore_file: &str, ignore_file_location: &str, input: &str, ignore_case: bool) {
    let run_with_prefix = run_assert(input, ignore_file, ignore_file_location, true, true, ignore_case);

    run_with_prefix("");
    run_with_prefix("/someFolder");
}

fn assert_ignore_match(ignore_file: &str, ignore_file_location: &str, input: &str, ignore_case: bool) {
    let run_with_prefix = run_assert(input, ignore_file, ignore_file_location, true, false, ignore_case);

    run_with_prefix("");
    run_with_prefix("/someFolder");
}

fn assert_no_ignore_match(ignore_file: &str, ignore_file_location: &str, input: &str, ignore_case: bool) {
    let run_with_prefix = run_assert(input, ignore_file, ignore_file_location, false, false, ignore_case);

    run_with_prefix("");
    run_with_prefix("/someFolder");
}

#[test]
fn paths_with_trailing_slashes_do_not_match_files() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let i = "node_modules/\n";

    assert_no_ignore_match(i, "/", "/node_modules", false);
    assert_ignore_match(i, "/", "/node_modules/", false);

    assert_no_ignore_match(i, "/", "/inner/node_modules", false);
    assert_ignore_match(i, "/", "/inner/node_modules/", false);
}

#[test]
fn parsing_simple_gitignore_files() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut i = "node_modules\nout\n";

    assert_ignore_match(i, "/", "/node_modules", false);
    assert_no_traverses(i, "/", "/node_modules", false);
    assert_ignore_match(i, "/", "/node_modules/file", false);
    assert_ignore_match(i, "/", "/dir/node_modules", false);
    assert_ignore_match(i, "/", "/dir/node_modules/file", false);

    assert_ignore_match(i, "/", "/out", false);
    assert_no_traverses(i, "/", "/out", false);
    assert_ignore_match(i, "/", "/out/file", false);
    assert_ignore_match(i, "/", "/dir/out", false);
    assert_ignore_match(i, "/", "/dir/out/file", false);

    i = "/node_modules\n/out\n";

    assert_ignore_match(i, "/", "/node_modules", false);
    assert_ignore_match(i, "/", "/node_modules/file", false);
    assert_no_ignore_match(i, "/", "/dir/node_modules", false);
    assert_no_ignore_match(i, "/", "/dir/node_modules/file", false);

    assert_ignore_match(i, "/", "/out", false);
    assert_ignore_match(i, "/", "/out/file", false);
    assert_no_ignore_match(i, "/", "/dir/out", false);
    assert_no_ignore_match(i, "/", "/dir/out/file", false);

    i = "node_modules/\nout/\n";

    assert_no_ignore_match(i, "/", "/node_modules", false);
    assert_ignore_match(i, "/", "/node_modules/", false);
    assert_ignore_match(i, "/", "/node_modules/file", false);
    assert_ignore_match(i, "/", "/dir/node_modules/", false);
    assert_no_ignore_match(i, "/", "/dir/node_modules", false);
    assert_ignore_match(i, "/", "/dir/node_modules/file", false);

    assert_ignore_match(i, "/", "/out/", false);
    assert_no_ignore_match(i, "/", "/out", false);
    assert_ignore_match(i, "/", "/out/file", false);
    assert_no_ignore_match(i, "/", "/dir/out", false);
    assert_ignore_match(i, "/", "/dir/out/", false);
    assert_ignore_match(i, "/", "/dir/out/file", false);
}

#[test]
fn parsing_files_in_folder_exclude() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut i = "node_modules/*\n";

    assert_no_ignore_match(i, "/", "/node_modules", false);
    assert_no_ignore_match(i, "/", "/node_modules/", false);
    assert_traverses(i, "/", "/node_modules", false);
    assert_traverses(i, "/", "/node_modules/", false);
    assert_ignore_match(i, "/", "/node_modules/something", false);
    assert_no_traverses(i, "/", "/node_modules/something", false);
    assert_ignore_match(i, "/", "/node_modules/something/else", false);
    assert_ignore_match(i, "/", "/node_modules/@types", false);
    assert_no_traverses(i, "/", "/node_modules/@types", false);

    i = "node_modules/**/*\n";

    assert_no_ignore_match(i, "/", "/node_modules", false);
    assert_no_ignore_match(i, "/", "/node_modules/", false);
    assert_ignore_match(i, "/", "/node_modules/something", false);
    assert_ignore_match(i, "/", "/node_modules/something/else", false);
    assert_ignore_match(i, "/", "/node_modules/@types", false);
}

#[test]
fn parsing_simple_negations() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut i = "node_modules/*\n!node_modules/@types\n";

    assert_no_ignore_match(i, "/", "/node_modules", false);
    assert_traverses(i, "/", "/node_modules", false);

    assert_ignore_match(i, "/", "/node_modules/something", false);
    assert_no_traverses(i, "/", "/node_modules/something", false);
    assert_ignore_match(i, "/", "/node_modules/something/else", false);

    assert_no_ignore_match(i, "/", "/node_modules/@types", false);
    assert_traverses(i, "/", "/node_modules/@types", false);
    assert_traverses(i, "/", "/node_modules/@types/boop", false);

    i = "*.log\n!important.log\n";

    assert_ignore_match(i, "/", "/test.log", false);
    assert_ignore_match(i, "/", "/inner/test.log", false);

    assert_no_ignore_match(i, "/", "/important.log", false);
    assert_no_ignore_match(i, "/", "/inner/important.log", false);

    assert_no_traverses(i, "/", "/test.log", false);
    assert_no_traverses(i, "/", "/inner/test.log", false);
    assert_traverses(i, "/", "/important.log", false);
    assert_traverses(i, "/", "/inner/important.log", false);
}

#[test]
fn nested_gitignores() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut i = "node_modules\nout\n";

    assert_ignore_match(i, "/inner/", "/inner/node_modules", false);
    assert_ignore_match(i, "/inner/", "/inner/more/node_modules", false);

    i = "/node_modules\n/out\n";

    assert_ignore_match(i, "/inner/", "/inner/node_modules", false);
    assert_no_ignore_match(i, "/inner/", "/inner/more/node_modules", false);
    assert_no_ignore_match(i, "/inner/", "/node_modules", false);

    i = "node_modules/\nout/\n";

    assert_no_ignore_match(i, "/inner/", "/inner/node_modules", false);
    assert_ignore_match(i, "/inner/", "/inner/node_modules/", false);
    assert_no_ignore_match(i, "/inner/", "/inner/more/node_modules", false);
    assert_ignore_match(i, "/inner/", "/inner/more/node_modules/", false);
    assert_no_ignore_match(i, "/inner/", "/node_modules", false);
}

#[test]
fn file_extension_matches() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut i = "*.js\n";

    assert_no_ignore_match(i, "/", "/myFile.ts", false);
    assert_ignore_match(i, "/", "/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/myFile.ts", false);
    assert_ignore_match(i, "/", "/inner/myFile.js", false);

    i = "/*.js";
    assert_no_ignore_match(i, "/", "/myFile.ts", false);
    assert_ignore_match(i, "/", "/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/myFile.ts", false);
    assert_no_ignore_match(i, "/", "/inner/myFile.js", false);

    i = "**/*.js";
    assert_no_ignore_match(i, "/", "/myFile.ts", false);
    assert_ignore_match(i, "/", "/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/myFile.ts", false);
    assert_ignore_match(i, "/", "/inner/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/more/myFile.ts", false);
    assert_ignore_match(i, "/", "/inner/more/myFile.js", false);

    i = "inner/*.js";
    assert_no_ignore_match(i, "/", "/myFile.ts", false);
    assert_no_ignore_match(i, "/", "/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/myFile.ts", false);
    assert_ignore_match(i, "/", "/inner/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/more/myFile.ts", false);
    assert_no_ignore_match(i, "/", "/inner/more/myFile.js", false);

    i = "/inner/*.js";
    assert_no_ignore_match(i, "/", "/myFile.ts", false);
    assert_no_ignore_match(i, "/", "/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/myFile.ts", false);
    assert_ignore_match(i, "/", "/inner/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/more/myFile.ts", false);
    assert_no_ignore_match(i, "/", "/inner/more/myFile.js", false);

    i = "**/inner/*.js";
    assert_no_ignore_match(i, "/", "/myFile.ts", false);
    assert_no_ignore_match(i, "/", "/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/myFile.ts", false);
    assert_ignore_match(i, "/", "/inner/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/more/myFile.ts", false);
    assert_no_ignore_match(i, "/", "/inner/more/myFile.js", false);

    i = "**/inner/**/*.js";
    assert_no_ignore_match(i, "/", "/myFile.ts", false);
    assert_no_ignore_match(i, "/", "/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/myFile.ts", false);
    assert_ignore_match(i, "/", "/inner/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/more/myFile.ts", false);
    assert_ignore_match(i, "/", "/inner/more/myFile.js", false);

    i = "**/more/*.js";
    assert_no_ignore_match(i, "/", "/myFile.ts", false);
    assert_no_ignore_match(i, "/", "/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/myFile.ts", false);
    assert_no_ignore_match(i, "/", "/inner/myFile.js", false);
    assert_no_ignore_match(i, "/", "/inner/more/myFile.ts", false);
    assert_ignore_match(i, "/", "/inner/more/myFile.js", false);
}

#[test]
fn real_world_example_vscode_js_debug() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let i = lines(&[
        ".cache/",
        ".profile/",
        ".cdp-profile/",
        ".headless-profile/",
        ".vscode-test/",
        ".DS_Store",
        "node_modules/",
        "out/",
        "dist",
        "/coverage",
        "/.nyc_output",
        "demos/web-worker/vscode-pwa-dap.log",
        "demos/web-worker/vscode-pwa-cdp.log",
        ".dynamic-testWorkspace",
        "**/test/**/*.actual",
        "/testWorkspace/web/tmp",
        "/testWorkspace/**/debug.log",
        "/testWorkspace/webview/win/true/",
        "*.cpuprofile",
    ]);

    let included = [
        "/distro",
        "/inner/coverage",
        "/inner/.nyc_output",
        "/inner/demos/web-worker/vscode-pwa-dap.log",
        "/inner/demos/web-worker/vscode-pwa-cdp.log",
        "/testWorkspace/webview/win/true",
        "/a/best/b/c.actual",
        "/best/b/c.actual",
    ];

    let excluded = [
        "/.profile/",
        "/inner/.profile/",
        "/.DS_Store",
        "/inner/.DS_Store",
        "/coverage",
        "/.nyc_output",
        "/demos/web-worker/vscode-pwa-dap.log",
        "/demos/web-worker/vscode-pwa-cdp.log",
        "/.dynamic-testWorkspace",
        "/inner/.dynamic-testWorkspace",
        "/test/.actual",
        "/test/hello.actual",
        "/a/test/.actual",
        "/a/test/b.actual",
        "/a/test/b/.actual",
        "/a/test/b/c.actual",
        "/a/b/test/.actual",
        "/a/b/test/f/c.actual",
        "/testWorkspace/web/tmp",
        "/testWorkspace/debug.log",
        "/testWorkspace/a/debug.log",
        "/testWorkspace/a/b/debug.log",
        "/testWorkspace/webview/win/true/",
        "/.cpuprofile",
        "/a.cpuprofile",
        "/aa/a.cpuprofile",
        "/aaa/aa/a.cpuprofile",
    ];

    for include in included {
        assert_no_ignore_match(&i, "/", include, false);
    }

    for exclude in excluded {
        assert_ignore_match(&i, "/", exclude, false);
    }
}

#[test]
fn real_world_example_vscode() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let i = lines(&[
        ".DS_Store",
        ".cache",
        "npm-debug.log",
        "Thumbs.db",
        "node_modules/",
        ".build/",
        "extensions/**/dist/",
        "/out*/",
        "/extensions/**/out/",
        "src/vs/server",
        "resources/server",
        "build/node_modules",
        "coverage/",
        "test_data/",
        "test-results/",
        "yarn-error.log",
        "vscode.lsif",
        "vscode.db",
        "/.profile-oss",
    ]);

    let included = [
        "/inner/extensions/dist",
        "/inner/extensions/boop/dist/test",
        "/inner/extensions/boop/doop/dist",
        "/inner/extensions/boop/doop/dist/test",
        "/inner/extensions/boop/doop/dist/test",
        "/inner/extensions/out/test",
        "/inner/extensions/boop/out",
        "/inner/extensions/boop/out/test",
        "/inner/out/",
        "/inner/out/test",
        "/inner/out1/",
        "/inner/out1/test",
        "/inner/out2/",
        "/inner/out2/test",
        "/inner/.profile-oss",
        // Files.
        "/extensions/dist",
        "/extensions/boop/doop/dist",
        "/extensions/boop/out",
    ];

    let excluded = [
        "/extensions/dist/",
        "/extensions/boop/dist/test",
        "/extensions/boop/doop/dist/",
        "/extensions/boop/doop/dist/test",
        "/extensions/boop/doop/dist/test",
        "/extensions/out/test",
        "/extensions/boop/out/",
        "/extensions/boop/out/test",
        "/out/",
        "/out/test",
        "/out1/",
        "/out1/test",
        "/out2/",
        "/out2/test",
        "/.profile-oss",
    ];

    for include in included {
        assert_no_ignore_match(&i, "/", include, false);
    }

    for exclude in excluded {
        assert_ignore_match(&i, "/", exclude, false);
    }
}

#[test]
fn various_advanced_constructs_found_in_popular_repos() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let run_test = |pattern: &str, included: &[&str], excluded: &[&str]| {
        for include in included {
            assert_no_ignore_match(pattern, "/", include, false);
        }

        for exclude in excluded {
            assert_ignore_match(pattern, "/", exclude, false);
        }
    };

    run_test(
        &lines(&["**/node_modules", "/packages/*/dist"]),
        &[
            "/inner/packages/a/dist",
            "/inner/packages/abc/dist",
            "/inner/packages/abc/dist/test",
            "/packages/dist",
            "/packages/dist/test",
            "/packages/a/b/dist",
            "/packages/a/b/dist/test",
        ],
        &[
            "/node_modules",
            "/test/node_modules",
            "/node_modules/test",
            "/test/node_modules/test",
            "/packages/a/dist",
            "/packages/abc/dist",
            "/packages/abc/dist/test",
        ],
    );

    run_test(
        &lines(&[
            ".yarn/*",
            "# !.yarn/cache",
            "!.yarn/patches",
            "!.yarn/plugins",
            "!.yarn/releases",
            "!.yarn/sdks",
            "!.yarn/versions",
        ]),
        &[
            "/inner/.yarn/test",
            "/inner/.yarn/cache",
            "/.yarn/patches",
            "/.yarn/plugins",
            "/.yarn/releases",
            "/.yarn/sdks",
            "/.yarn/versions",
        ],
        &["/.yarn/test", "/.yarn/cache"],
    );

    run_test(
        &lines(&["[._]*s[a-w][a-z]", "[._]s[a-w][a-z]", "*.un~", "*~"]),
        &["/.jaa"],
        &[
            "/~",
            "/abc~",
            "/inner/~",
            "/inner/abc~",
            "/.un~",
            "/a.un~",
            "/test/.un~",
            "/test/a.un~",
            "/.saa",
            "/....saa",
            "/._._sby",
            "/inner/._._sby",
            "/_swz",
        ],
    );

    // TODO: the rest of these :)
    run_test(
        &lines(&[
            "*.pbxuser",
            "!default.pbxuser",
            "*.mode1v3",
            "!default.mode1v3",
            "*.mode2v3",
            "!default.mode2v3",
            "*.perspectivev3",
            "!default.perspectivev3",
        ]),
        &[],
        &[],
    );

    run_test(
        &lines(&[
            "[Dd]ebug/",
            "[Dd]ebugPublic/",
            "[Rr]elease/",
            "[Rr]eleases/",
            "*.[Mm]etrics.xml",
            "[Tt]est[Rr]esult*/",
            "[Bb]uild[Ll]og.*",
            "bld/",
            "[Bb]in/",
            "[Oo]bj/",
            "[Ll]og/",
        ]),
        &[],
        &[],
    );

    run_test(
        &lines(&["Dockerfile*", "!/tests/bud/*/Dockerfile*", "!/tests/conformance/**/Dockerfile*"]),
        &[],
        &[],
    );

    run_test(
        &lines(&[
            "*.pdf",
            "*.html",
            "!author_bio.html",
            "!colo.html",
            "!copyright.html",
            "!cover.html",
            "!ix.html",
            "!titlepage.html",
            "!toc.html",
        ]),
        &[],
        &[],
    );

    run_test(&lines(&["/log/*", "/tmp/*", "!/log/.keep", "!/tmp/.keep"]), &[], &[]);
}

#[test]
fn child_negation_overrides_parent_ignore() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Simulates: global gitignore has `.myconfig`, project .gitignore has `!.myconfig/`
    // The child negation should override the parent positive pattern.
    let parent_ignore = IgnoreFile::new(".myconfig\n", "/", None, false).unwrap();
    let child_ignore = IgnoreFile::new("!.myconfig/\n", "/", Some(&parent_ignore), false).unwrap();

    // The directory should NOT be ignored (child negates parent)
    assert!(
        !child_ignore.is_arbitrary_path_ignored("/.myconfig", true).unwrap(),
        "child !.myconfig/ should override parent .myconfig for directories"
    );

    // Files inside the directory should also not be ignored
    assert!(
        !child_ignore.is_arbitrary_path_ignored("/.myconfig/settings/test.md", false).unwrap(),
        "files inside un-ignored directory should not be ignored"
    );

    // Parent should still ignore when child has no negation
    let child_no_negation = IgnoreFile::new("node_modules/\n", "/", Some(&parent_ignore), false).unwrap();
    assert!(
        child_no_negation.is_arbitrary_path_ignored("/.myconfig", true).unwrap(),
        "without negation, parent ignore should still apply for directories"
    );
    assert!(
        child_no_negation.is_arbitrary_path_ignored("/.myconfig/settings/test.md", false).unwrap(),
        "without negation, files under parent-ignored directory should still be ignored"
    );
}

#[test]
fn child_negation_overrides_parent_ignore_for_files() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Parent ignores all .log files, child un-ignores important.log
    let parent_ignore = IgnoreFile::new("*.log\n", "/", None, false).unwrap();
    let child_ignore = IgnoreFile::new("!important.log\n", "/", Some(&parent_ignore), false).unwrap();

    assert!(
        !child_ignore.is_arbitrary_path_ignored("/important.log", false).unwrap(),
        "child !important.log should override parent *.log"
    );
    assert!(
        child_ignore.is_arbitrary_path_ignored("/other.log", false).unwrap(),
        "other .log files should still be ignored via parent"
    );
}

#[test]
fn case_insensitive_ignore_files() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f1 = "node_modules/\n";
    assert_no_ignore_match(f1, "/", "/Node_Modules/", false);
    assert_ignore_match(f1, "/", "/Node_Modules/", true);

    let f2 = "NODE_MODULES/\n";
    assert_no_ignore_match(f2, "/", "/Node_Modules/", false);
    assert_ignore_match(f2, "/", "/Node_Modules/", true);

    let f3 = "\ntemp/*\n!temp/keep\n";
    assert_no_ignore_match(f3, "/", "/TEMP/other", false);
    assert_ignore_match(f3, "/", "/temp/KEEP", false);
    assert_ignore_match(f3, "/", "/TEMP/other", true);
    assert_no_ignore_match(f3, "/", "/TEMP/KEEP", true);
}
