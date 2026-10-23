//! Porte de `src/vs/base/test/common/labels.test.ts` (suíte `Labels`), caso a caso e na mesma ordem.
//!
//! O original escolhe o ramo por `isWindows`/`isMacintosh` do processo e pula o que não é do sistema
//! (`test.skip`). Aqui o sistema e o separador padrão entram como argumento, então os dois ramos de
//! `shorten` rodam sempre (cada um com o separador do seu sistema), e `mnemonicButtonLabel` confere
//! os três sistemas, cada um com o seu `Platform`. O `getPathLabel` roda sobre um host Linux, que é onde
//! o original é executado, e também sobre hosts Windows e macOS, porque as asserções valem em qualquer
//! um (o original roda no CI dos três). O `ensureNoDisposablesAreLeakedInTestSuite()` não tem o que
//! conferir: nenhum caso cria descartável.

use std::collections::HashMap;

use je_base::labels::{self, PathLabelFormatting, RelativePathProvider, TemplateValue, UserHomeProvider, Workspace, WorkspaceFolder, Separator};
use je_base::platform::{NativeEnvironment, OperatingSystem, Platform};
use je_base::uri::{Field, UriChange, UriComponents, URI};

fn host(process_platform: &str) -> Platform {
    Platform::from_native(&NativeEnvironment { platform: process_platform, env: &|_| None, is_electron: false })
}

fn shorten(paths: &[Option<&str>], sep: &str) -> Vec<Option<String>> {
    labels::shorten(paths, sep)
}

/// `assert.deepStrictEqual(labels.shorten(paths), expected)`, com `None` no lugar do `null`.
fn check(paths: &[&str], expected: &[&str], sep: &str) {
    let paths: Vec<Option<&str>> = paths.iter().map(|p| Some(*p)).collect();
    let expected: Vec<Option<String>> = expected.iter().map(|p| Some((*p).to_string())).collect();
    assert_eq!(shorten(&paths, sep), expected);
}

#[test]
fn shorten_windows() {
    const S: &str = "\\";
    let check = |paths: &[&str], expected: &[&str]| check(paths, expected, S);

    // nothing to shorten
    check(&["a"], &["a"]);
    check(&["a", "b"], &["a", "b"]);
    check(&["a", "b", "c"], &["a", "b", "c"]);
    check(&["\\\\x\\a", "\\\\x\\a"], &["\\\\x\\a", "\\\\x\\a"]);
    check(&["C:\\a", "C:\\b"], &["C:\\a", "C:\\b"]);

    // completely different paths
    check(&["a\\b", "c\\d", "e\\f"], &["…\\b", "…\\d", "…\\f"]);

    // same beginning
    check(&["a", "a\\b"], &["a", "…\\b"]);
    check(&["a\\b", "a\\b\\c"], &["…\\b", "…\\c"]);
    check(&["a", "a\\b", "a\\b\\c"], &["a", "…\\b", "…\\c"]);
    check(&["x:\\a\\b", "x:\\a\\c"], &["x:\\…\\b", "x:\\…\\c"]);
    check(&["\\\\a\\b", "\\\\a\\c"], &["\\\\a\\b", "\\\\a\\c"]);

    // same ending
    check(&["a", "b\\a"], &["a", "b\\…"]);
    check(&["a\\b\\c", "d\\b\\c"], &["a\\…", "d\\…"]);
    check(&["a\\b\\c\\d", "f\\b\\c\\d"], &["a\\…", "f\\…"]);
    check(&["d\\e\\a\\b\\c", "d\\b\\c"], &["…\\a\\…", "d\\b\\…"]);
    check(&["a\\b\\c\\d", "a\\f\\b\\c\\d"], &["a\\b\\…", "…\\f\\…"]);
    check(&["a\\b\\a", "b\\b\\a"], &["a\\b\\…", "b\\b\\…"]);
    check(&["d\\f\\a\\b\\c", "h\\d\\b\\c"], &["…\\a\\…", "h\\…"]);
    check(&["a\\b\\c", "x:\\0\\a\\b\\c"], &["a\\b\\c", "x:\\0\\…"]);
    check(&["x:\\a\\b\\c", "x:\\0\\a\\b\\c"], &["x:\\a\\…", "x:\\0\\…"]);
    check(&["x:\\a\\b", "y:\\a\\b"], &["x:\\…", "y:\\…"]);
    check(&["x:\\a", "x:\\c"], &["x:\\a", "x:\\c"]);
    check(&["x:\\a\\b", "y:\\x\\a\\b"], &["x:\\…", "y:\\…"]);
    check(&["\\\\x\\b", "\\\\y\\b"], &["\\\\x\\…", "\\\\y\\…"]);
    check(&["\\\\x\\a", "\\\\x\\b"], &["\\\\x\\a", "\\\\x\\b"]);

    // same name ending
    check(&["a\\b", "a\\c", "a\\e-b"], &["…\\b", "…\\c", "…\\e-b"]);

    // same in the middle
    check(&["a\\b\\c", "d\\b\\e"], &["…\\c", "…\\e"]);

    // case-sensetive
    check(&["a\\b\\c", "d\\b\\C"], &["…\\c", "…\\C"]);

    // empty or null
    assert_eq!(shorten(&[Some(""), None], S), vec![Some(".\\".to_string()), None]);

    check(&["a", "a\\b", "a\\b\\c", "d\\b\\c", "d\\b"], &["a", "a\\b", "a\\b\\c", "d\\b\\c", "d\\b"]);
    check(&["a", "a\\b", "b"], &["a", "a\\b", "b"]);
    check(&["", "a", "b", "b\\c", "a\\c"], &[".\\", "a", "b", "b\\c", "a\\c"]);
    check(
        &[
            "src\\vs\\workbench\\parts\\execution\\electron-browser",
            "src\\vs\\workbench\\parts\\execution\\electron-browser\\something",
            "src\\vs\\workbench\\parts\\terminal\\electron-browser",
        ],
        &["…\\execution\\electron-browser", "…\\something", "…\\terminal\\…"],
    );

    // url paths
    check(&["https://a.com/b", "C:\\foo\\d"], &["https://a.com/b", "C:\\…\\d"]);
    check(&["https://a.com/x", "C:\\foo\\x"], &["https://a.com/…", "C:\\foo\\…"]);
    check(&["https://a.com/y/z", "C:\\foo\\bar\\z"], &["https://a.com/y/…", "C:\\…\\bar\\…"]);
    check(&["file://C:/foo/bar/z", "C:\\foo\\bar\\z"], &["file://C:/…/bar/z", "C:\\…\\bar\\z"]);
}

#[test]
fn shorten_not_windows() {
    const S: &str = "/";
    let check = |paths: &[&str], expected: &[&str]| check(paths, expected, S);

    // nothing to shorten
    check(&["a"], &["a"]);
    check(&["a", "b"], &["a", "b"]);
    check(&["/a", "/b"], &["/a", "/b"]);
    check(&["~/a/b/c", "~/a/b/c"], &["~/a/b/c", "~/a/b/c"]);
    check(&["a", "b", "c"], &["a", "b", "c"]);

    // completely different paths
    check(&["a/b", "c/d", "e/f"], &["…/b", "…/d", "…/f"]);

    // same beginning
    check(&["a", "a/b"], &["a", "…/b"]);
    check(&["a/b", "a/b/c"], &["…/b", "…/c"]);
    check(&["a", "a/b", "a/b/c"], &["a", "…/b", "…/c"]);
    check(&["/a/b", "/a/c"], &["/a/b", "/a/c"]);

    // same ending
    check(&["a", "b/a"], &["a", "b/…"]);
    check(&["a/b/c", "d/b/c"], &["a/…", "d/…"]);
    check(&["a/b/c/d", "f/b/c/d"], &["a/…", "f/…"]);
    check(&["d/e/a/b/c", "d/b/c"], &["…/a/…", "d/b/…"]);
    check(&["a/b/c/d", "a/f/b/c/d"], &["a/b/…", "…/f/…"]);
    check(&["a/b/a", "b/b/a"], &["a/b/…", "b/b/…"]);
    check(&["d/f/a/b/c", "h/d/b/c"], &["…/a/…", "h/…"]);
    check(&["/x/b", "/y/b"], &["/x/…", "/y/…"]);

    // same name ending
    check(&["a/b", "a/c", "a/e-b"], &["…/b", "…/c", "…/e-b"]);

    // same in the middle
    check(&["a/b/c", "d/b/e"], &["…/c", "…/e"]);

    // case-sensitive
    check(&["a/b/c", "d/b/C"], &["…/c", "…/C"]);

    // empty or null
    assert_eq!(shorten(&[Some(""), None], S), vec![Some("./".to_string()), None]);

    check(&["a", "a/b", "a/b/c", "d/b/c", "d/b"], &["a", "a/b", "a/b/c", "d/b/c", "d/b"]);
    check(&["a", "a/b", "b"], &["a", "a/b", "b"]);
    check(&["", "a", "b", "b/c", "a/c"], &["./", "a", "b", "b/c", "a/c"]);

    // url paths
    check(&["https://a.com/b"], &["https://a.com/b"]);
    check(&["https://a.com/", "https://b.com/"], &["https://a.com/", "https://b.com/"]);
    check(&["https://a.com/b", "https://a.com/c"], &["https://a.com/b", "https://a.com/c"]);
    check(&["https://a.com/x/y", "https://b.com/x/y"], &["https://a.com/…", "https://b.com/…"]);
    check(&["https://a.com/b", "https://a.com/b/c"], &["https://a.com/b", "https://a.com/…/c"]);
    check(&["https://a.com/b", "http://a.com/b"], &["https://a.com/b", "http://a.com/b"]);
    check(&["https://a.com/x/y/z", "https://a.com/x/w/z"], &["https://a.com/…/y/…", "https://a.com/…/w/…"]);
    check(&["https://a.com/b", "/c/d"], &["https://a.com/b", "/c/d"]);
    check(&["https://a.com/x", "/c/x"], &["https://a.com/…", "/c/x"]);
    check(&["https://a.com/x/y", "/c/x/y"], &["https://a.com/…", "/c/x/…"]);
    check(&["file:///foo/bar/z", "/foo/bar/z"], &["file:///foo/bar/z", "/foo/bar/z"]);
}

fn values(entries: &[(&str, TemplateValue)]) -> HashMap<String, TemplateValue> {
    entries.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

fn text(value: &str) -> TemplateValue {
    TemplateValue::Text(value.to_string())
}

fn sep(label: &str) -> TemplateValue {
    TemplateValue::Separator(Separator { label: label.to_string() })
}

#[test]
fn template() {
    let none = HashMap::new();

    // simple
    assert_eq!(labels::template("Foo Bar", &none), "Foo Bar");
    assert_eq!(labels::template("Foo${}Bar", &none), "FooBar");
    assert_eq!(labels::template("$FooBar", &none), "");
    assert_eq!(labels::template("}FooBar", &none), "}FooBar");
    assert_eq!(labels::template("Foo ${one} Bar", &values(&[("one", text("value"))])), "Foo value Bar");
    assert_eq!(labels::template("Foo ${one} Bar ${two}", &values(&[("one", text("value")), ("two", text("other value"))])), "Foo value Bar other value");

    // conditional separator
    assert_eq!(labels::template("Foo${separator}Bar", &none), "FooBar");
    assert_eq!(labels::template("Foo${separator}Bar", &values(&[("separator", sep(" - "))])), "Foo - Bar");
    assert_eq!(labels::template("${separator}Foo${separator}Bar", &values(&[("value", text("something")), ("separator", sep(" - "))])), "Foo - Bar");
    assert_eq!(labels::template("${value} Foo${separator}Bar", &values(&[("value", text("something")), ("separator", sep(" - "))])), "something Foo - Bar");

    // real world example (macOS)
    let mut t = "${activeEditorShort}${separator}${rootName}";
    assert_eq!(labels::template(t, &values(&[("activeEditorShort", text("")), ("rootName", text("")), ("separator", sep(" - "))])), "");
    assert_eq!(labels::template(t, &values(&[("activeEditorShort", text("")), ("rootName", text("root")), ("separator", sep(" - "))])), "root");
    assert_eq!(labels::template(t, &values(&[("activeEditorShort", text("markdown.txt")), ("rootName", text("root")), ("separator", sep(" - "))])), "markdown.txt - root");

    // real world example (other)
    t = "${dirty}${activeEditorShort}${separator}${rootName}${separator}${appName}";
    let app = |dirty: &str, editor: &str, root: &str, app_name: &str| {
        values(&[("dirty", text(dirty)), ("activeEditorShort", text(editor)), ("rootName", text(root)), ("appName", text(app_name)), ("separator", sep(" - "))])
    };
    assert_eq!(labels::template(t, &app("", "", "", "")), "");
    assert_eq!(labels::template(t, &app("", "", "", "Visual Studio Code")), "Visual Studio Code");
    assert_eq!(labels::template(t, &app("", "Untitled-1", "", "Visual Studio Code")), "Untitled-1 - Visual Studio Code");
    assert_eq!(labels::template(t, &app("", "", "monaco", "Visual Studio Code")), "monaco - Visual Studio Code");
    assert_eq!(labels::template(t, &app("", "somefile.txt", "monaco", "Visual Studio Code")), "somefile.txt - monaco - Visual Studio Code");
    assert_eq!(labels::template(t, &app("* ", "somefile.txt", "monaco", "Visual Studio Code")), "* somefile.txt - monaco - Visual Studio Code");

    // real world example (other)
    t = "${dirty}${activeEditorShort}${separator}${rootNameShort}${separator}${appName}";
    assert_eq!(
        labels::template(
            t,
            &values(&[
                ("dirty", text("")),
                ("activeEditorShort", text("")),
                ("rootName", text("monaco (Workspace)")),
                ("rootNameShort", text("monaco")),
                ("appName", text("Visual Studio Code")),
                ("separator", sep(" - ")),
            ])
        ),
        "monaco - Visual Studio Code"
    );
}

#[test]
fn mnemonic_button_label() {
    for (process_platform, hello, do_not_save) in [
        ("win32", "Hello && World", "Do &not Save && Continue"),
        ("darwin", "Hello & World", "Do not Save & Continue"),
        ("linux", "Hello & World", "Do _not Save & Continue"),
    ] {
        let host = host(process_platform);
        assert_eq!(labels::mnemonic_button_label("Hello World", &host).with_mnemonic, "Hello World");
        assert_eq!(labels::mnemonic_button_label("", &host).with_mnemonic, "");
        assert_eq!(labels::mnemonic_button_label("Hello & World", &host).with_mnemonic, hello);
        assert_eq!(labels::mnemonic_button_label("Do &&not Save & Continue", &host).with_mnemonic, do_not_save);
    }
}

struct FixedWorkspace {
    folder: URI,
}

impl RelativePathProvider for FixedWorkspace {
    fn get_workspace(&self) -> Workspace {
        Workspace { folders: vec![WorkspaceFolder { uri: self.folder.clone(), name: None }] }
    }

    fn get_workspace_folder(&self, _resource: &URI) -> Option<WorkspaceFolder> {
        Some(WorkspaceFolder { uri: self.folder.clone(), name: None })
    }
}

fn with(uri: &URI, change: UriChange) -> URI {
    uri.with(&change)
}

#[test]
fn get_path_label() {
    for host in [host("linux"), host("win32"), host("darwin")] {
        let flavor = host.path_flavor();
        let file = |path: &str| URI::file(path, flavor);
        let win_file_uri = file("c:/some/folder/file.txt");
        let nix_file_uri = file("/some/folder/file.txt");
        let nix_bad_file_uri = URI::revive(
            &UriComponents { scheme: Some("vscode".into()), authority: Some("file".into()), path: Some("//some/folder/file.txt".into()), ..UriComponents::default() }.into(),
            flavor,
        );
        let unc_file_uri = with(&file("c:/some/folder/file.txt"), UriChange { authority: Field::from("auth"), ..UriChange::default() });
        let remote_file_uri = with(&file("/some/folder/file.txt"), UriChange { scheme: Field::from("vscode-test"), authority: Field::from("auth"), ..UriChange::default() });

        let label = |uri: &URI, os: OperatingSystem| labels::get_path_label(uri, &PathLabelFormatting { os, tildify: None, relative: None }, &host);
        let label_tildify = |uri: &URI, os: OperatingSystem, home: &UserHomeProvider| {
            labels::get_path_label(uri, &PathLabelFormatting { os, tildify: Some(home), relative: None }, &host)
        };
        let label_relative = |uri: &URI, os: OperatingSystem, provider: &FixedWorkspace| {
            labels::get_path_label(uri, &PathLabelFormatting { os, tildify: None, relative: Some(provider) }, &host)
        };
        use OperatingSystem::{Linux, Macintosh, Windows};

        // Basics

        assert_eq!(label(&win_file_uri, Windows), "C:\\some\\folder\\file.txt");
        assert_eq!(label(&win_file_uri, Macintosh), "c:/some/folder/file.txt");
        assert_eq!(label(&win_file_uri, Linux), "c:/some/folder/file.txt");

        assert_eq!(label(&nix_file_uri, Windows), "\\some\\folder\\file.txt");
        assert_eq!(label(&nix_file_uri, Macintosh), "/some/folder/file.txt");
        assert_eq!(label(&nix_file_uri, Linux), "/some/folder/file.txt");

        assert_eq!(label(&unc_file_uri, Windows), "\\\\auth\\c:\\some\\folder\\file.txt");
        assert_eq!(label(&unc_file_uri, Macintosh), "/auth/c:/some/folder/file.txt");
        assert_eq!(label(&unc_file_uri, Linux), "/auth/c:/some/folder/file.txt");

        assert_eq!(label(&remote_file_uri, Windows), "\\some\\folder\\file.txt");
        assert_eq!(label(&remote_file_uri, Macintosh), "/some/folder/file.txt");
        assert_eq!(label(&remote_file_uri, Linux), "/some/folder/file.txt");

        // Tildify

        let nix_user_home = UserHomeProvider { user_home: file("/some") };
        let remote_user_home = UserHomeProvider { user_home: with(&file("/some"), UriChange { scheme: Field::from("vscode-test"), authority: Field::from("auth"), ..UriChange::default() }) };

        assert_eq!(label_tildify(&nix_file_uri, Windows, &nix_user_home), "\\some\\folder\\file.txt");
        assert_eq!(label_tildify(&nix_file_uri, Macintosh, &nix_user_home), "~/folder/file.txt");
        assert_eq!(label_tildify(&nix_bad_file_uri, Macintosh, &nix_user_home), "/some/folder/file.txt");
        assert_eq!(label_tildify(&nix_file_uri, Linux, &nix_user_home), "~/folder/file.txt");

        assert_eq!(label_tildify(&nix_file_uri, Windows, &remote_user_home), "\\some\\folder\\file.txt");
        assert_eq!(label_tildify(&nix_file_uri, Macintosh, &remote_user_home), "~/folder/file.txt");
        assert_eq!(label_tildify(&nix_file_uri, Linux, &remote_user_home), "~/folder/file.txt");

        let nix_untitled_uri = with(&file("/some/folder/file.txt"), UriChange { scheme: Field::from("untitled"), ..UriChange::default() });

        assert_eq!(label_tildify(&nix_untitled_uri, Windows, &nix_user_home), "\\some\\folder\\file.txt");
        assert_eq!(label_tildify(&nix_untitled_uri, Macintosh, &nix_user_home), "~/folder/file.txt");
        assert_eq!(label_tildify(&nix_untitled_uri, Linux, &nix_user_home), "~/folder/file.txt");

        assert_eq!(label_tildify(&nix_untitled_uri, Windows, &remote_user_home), "\\some\\folder\\file.txt");
        assert_eq!(label_tildify(&nix_untitled_uri, Macintosh, &remote_user_home), "~/folder/file.txt");
        assert_eq!(label_tildify(&nix_untitled_uri, Linux, &remote_user_home), "~/folder/file.txt");

        // Relative

        let win_relative_path_provider = FixedWorkspace { folder: file("c:/some") };

        assert_eq!(label_relative(&win_file_uri, Windows, &win_relative_path_provider), "folder\\file.txt");
        assert_eq!(label_relative(&win_file_uri, Macintosh, &win_relative_path_provider), "folder/file.txt");
        assert_eq!(label_relative(&win_file_uri, Linux, &win_relative_path_provider), "folder/file.txt");

        let nix_relative_path_provider = FixedWorkspace { folder: file("/some") };

        assert_eq!(label_relative(&nix_file_uri, Windows, &nix_relative_path_provider), "folder\\file.txt");
        assert_eq!(label_relative(&nix_file_uri, Macintosh, &nix_relative_path_provider), "folder/file.txt");
        assert_eq!(label_relative(&nix_file_uri, Linux, &nix_relative_path_provider), "folder/file.txt");

        assert_eq!(label_relative(&nix_untitled_uri, Windows, &nix_relative_path_provider), "folder\\file.txt");
        assert_eq!(label_relative(&nix_untitled_uri, Macintosh, &nix_relative_path_provider), "folder/file.txt");
        assert_eq!(label_relative(&nix_untitled_uri, Linux, &nix_relative_path_provider), "folder/file.txt");

    }
}
