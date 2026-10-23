//! Porte de `src/vs/base/test/common/path.test.ts` (suíte `Paths (Node Implementation)`), caso a caso e na mesma ordem.

use je_base::path::{Environment, FixedEnvironment, PathModule, Platform};

/// O ambiente do teste original é o processo Node em Linux: `isWindows` falso, `isWeb` falso e
/// `process.cwd()` fixo aqui para o teste não depender de onde o `cargo test` roda.
fn linux_env() -> FixedEnvironment {
    FixedEnvironment::new(Platform::Posix, "/workspace/project")
}

fn is_windows(env: &dyn Environment) -> bool {
    env.platform() == Platform::Win32
}

/// `JSON.stringify` de uma string, para as mensagens de falha.
fn json(s: &str) -> String {
    format!("{s:?}")
}

fn assert_no_failures(failures: &[String]) {
    assert_eq!(failures.len(), 0, "{}", failures.join(""));
}

#[test]
fn join() {
    let env = linux_env();
    let posix = PathModule::posix(&env);
    let win32 = PathModule::win32(&env);
    let mut failures: Vec<String> = Vec::new();

    let common: Vec<(Vec<&str>, &str)> = vec![
        (vec![".", "x/b", "..", "/b/c.js"], "x/b/c.js"),
        (vec![], "."),
        (vec!["/.", "x/b", "..", "/b/c.js"], "/x/b/c.js"),
        (vec!["/foo", "../../../bar"], "/bar"),
        (vec!["foo", "../../../bar"], "../../bar"),
        (vec!["foo/", "../../../bar"], "../../bar"),
        (vec!["foo/x", "../../../bar"], "../bar"),
        (vec!["foo/x", "./bar"], "foo/x/bar"),
        (vec!["foo/x/", "./bar"], "foo/x/bar"),
        (vec!["foo/x/", ".", "bar"], "foo/x/bar"),
        (vec!["./"], "./"),
        (vec![".", "./"], "./"),
        (vec![".", ".", "."], "."),
        (vec![".", "./", "."], "."),
        (vec![".", "/./", "."], "."),
        (vec![".", "/////./", "."], "."),
        (vec!["."], "."),
        (vec!["", "."], "."),
        (vec!["", "foo"], "foo"),
        (vec!["foo", "/bar"], "foo/bar"),
        (vec!["", "/foo"], "/foo"),
        (vec!["", "", "/foo"], "/foo"),
        (vec!["", "", "foo"], "foo"),
        (vec!["foo", ""], "foo"),
        (vec!["foo/", ""], "foo/"),
        (vec!["foo", "", "/bar"], "foo/bar"),
        (vec!["./", "..", "/foo"], "../foo"),
        (vec!["./", "..", "..", "/foo"], "../../foo"),
        (vec![".", "..", "..", "/foo"], "../../foo"),
        (vec!["", "..", "..", "/foo"], "../../foo"),
        (vec!["/"], "/"),
        (vec!["/", "."], "/"),
        (vec!["/", ".."], "/"),
        (vec!["/", "..", ".."], "/"),
        (vec![""], "."),
        (vec!["", ""], "."),
        (vec![" /foo"], " /foo"),
        (vec![" ", "foo"], " /foo"),
        (vec![" ", "."], " "),
        (vec![" ", "/"], " /"),
        (vec![" ", ""], " "),
        (vec!["/", "foo"], "/foo"),
        (vec!["/", "/foo"], "/foo"),
        (vec!["/", "//foo"], "/foo"),
        (vec!["/", "", "/foo"], "/foo"),
        (vec!["", "/", "foo"], "/foo"),
        (vec!["", "/", "/foo"], "/foo"),
    ];

    // Testes específicos do Windows.
    let win32_only: Vec<(Vec<&str>, &str)> = vec![
        // Caminho UNC esperado.
        (vec!["//foo/bar"], "\\\\foo\\bar\\"),
        (vec!["\\/foo/bar"], "\\\\foo\\bar\\"),
        (vec!["\\\\foo/bar"], "\\\\foo\\bar\\"),
        // Caminho UNC esperado, servidor e compartilhamento separados.
        (vec!["//foo", "bar"], "\\\\foo\\bar\\"),
        (vec!["//foo/", "bar"], "\\\\foo\\bar\\"),
        (vec!["//foo", "/bar"], "\\\\foo\\bar\\"),
        // Caminho UNC esperado, discutível.
        (vec!["//foo", "", "bar"], "\\\\foo\\bar\\"),
        (vec!["//foo/", "", "bar"], "\\\\foo\\bar\\"),
        (vec!["//foo/", "", "/bar"], "\\\\foo\\bar\\"),
        // Caminho UNC esperado, ainda mais discutível.
        (vec!["", "//foo", "bar"], "\\\\foo\\bar\\"),
        (vec!["", "//foo/", "bar"], "\\\\foo\\bar\\"),
        (vec!["", "//foo/", "/bar"], "\\\\foo\\bar\\"),
        // Sem UNC (sem barra dupla no primeiro componente).
        (vec!["\\", "foo/bar"], "\\foo\\bar"),
        (vec!["\\", "/foo/bar"], "\\foo\\bar"),
        (vec!["", "/", "/foo/bar"], "\\foo\\bar"),
        // Sem UNC (sem não barras no primeiro componente, discutível).
        (vec!["//", "foo/bar"], "\\foo\\bar"),
        (vec!["//", "/foo/bar"], "\\foo\\bar"),
        (vec!["\\\\", "/", "/foo/bar"], "\\foo\\bar"),
        (vec!["//"], "\\"),
        // Sem UNC (nome do compartilhamento ausente, discutível).
        (vec!["//foo"], "\\foo"),
        (vec!["//foo/"], "\\foo\\"),
        (vec!["//foo", "/"], "\\foo\\"),
        (vec!["//foo", "", "/"], "\\foo\\"),
        // Sem UNC (barras demais no começo, discutível).
        (vec!["///foo/bar"], "\\foo\\bar"),
        (vec!["////foo", "bar"], "\\foo\\bar"),
        (vec!["\\\\\\/foo/bar"], "\\foo\\bar"),
        // Relativo à unidade vs absoluto na unidade. Só descreve o status quo, não é
        // necessariamente o certo.
        (vec!["c:"], "c:."),
        (vec!["c:."], "c:."),
        (vec!["c:", ""], "c:."),
        (vec!["", "c:"], "c:."),
        (vec!["c:.", "/"], "c:.\\"),
        (vec!["c:.", "file"], "c:file"),
        (vec!["c:", "/"], "c:\\"),
        (vec!["c:", "file"], "c:\\file"),
    ];
    let win32_all: Vec<(Vec<&str>, &str)> = common.iter().cloned().chain(win32_only).collect();

    for (os, module, cases) in [("posix", posix, &common), ("win32", win32, &win32_all)] {
        for (args, expected) in cases {
            let actual = module.join(args);
            // Para os testes não específicos do Windows com o `join` do Windows, tenta trocar as
            // barras, pois o `expected` desses testes usa barras normais.
            let actual_alt = (os == "win32").then(|| actual.replace('\\', "/"));
            let message = format!(
                "path.{os}.join({})\n  expect={}\n  actual={}",
                args.iter().map(|a| json(a)).collect::<Vec<_>>().join(","),
                json(expected),
                json(&actual)
            );
            if actual != *expected && actual_alt.as_deref() != Some(*expected) {
                failures.push(format!("\n{message}"));
            }
        }
    }
    assert_no_failures(&failures);
}

#[test]
fn dirname() {
    let env = linux_env();
    let posix = PathModule::posix(&env);
    let win32 = PathModule::win32(&env);

    assert_eq!(posix.dirname("/a/b/"), "/a");
    assert_eq!(posix.dirname("/a/b"), "/a");
    assert_eq!(posix.dirname("/a"), "/");
    assert_eq!(posix.dirname(""), ".");
    assert_eq!(posix.dirname("/"), "/");
    assert_eq!(posix.dirname("////"), "/");
    assert_eq!(posix.dirname("//a"), "//");
    assert_eq!(posix.dirname("foo"), ".");

    assert_eq!(win32.dirname("c:\\"), "c:\\");
    assert_eq!(win32.dirname("c:\\foo"), "c:\\");
    assert_eq!(win32.dirname("c:\\foo\\"), "c:\\");
    assert_eq!(win32.dirname("c:\\foo\\bar"), "c:\\foo");
    assert_eq!(win32.dirname("c:\\foo\\bar\\"), "c:\\foo");
    assert_eq!(win32.dirname("c:\\foo\\bar\\baz"), "c:\\foo\\bar");
    assert_eq!(win32.dirname("\\"), "\\");
    assert_eq!(win32.dirname("\\foo"), "\\");
    assert_eq!(win32.dirname("\\foo\\"), "\\");
    assert_eq!(win32.dirname("\\foo\\bar"), "\\foo");
    assert_eq!(win32.dirname("\\foo\\bar\\"), "\\foo");
    assert_eq!(win32.dirname("\\foo\\bar\\baz"), "\\foo\\bar");
    assert_eq!(win32.dirname("c:"), "c:");
    assert_eq!(win32.dirname("c:foo"), "c:");
    assert_eq!(win32.dirname("c:foo\\"), "c:");
    assert_eq!(win32.dirname("c:foo\\bar"), "c:foo");
    assert_eq!(win32.dirname("c:foo\\bar\\"), "c:foo");
    assert_eq!(win32.dirname("c:foo\\bar\\baz"), "c:foo\\bar");
    assert_eq!(win32.dirname("file:stream"), ".");
    assert_eq!(win32.dirname("dir\\file:stream"), "dir");
    assert_eq!(win32.dirname("\\\\unc\\share"), "\\\\unc\\share");
    assert_eq!(win32.dirname("\\\\unc\\share\\foo"), "\\\\unc\\share\\");
    assert_eq!(win32.dirname("\\\\unc\\share\\foo\\"), "\\\\unc\\share\\");
    assert_eq!(win32.dirname("\\\\unc\\share\\foo\\bar"), "\\\\unc\\share\\foo");
    assert_eq!(win32.dirname("\\\\unc\\share\\foo\\bar\\"), "\\\\unc\\share\\foo");
    assert_eq!(win32.dirname("\\\\unc\\share\\foo\\bar\\baz"), "\\\\unc\\share\\foo\\bar");
    assert_eq!(win32.dirname("/a/b/"), "/a");
    assert_eq!(win32.dirname("/a/b"), "/a");
    assert_eq!(win32.dirname("/a"), "/");
    assert_eq!(win32.dirname(""), ".");
    assert_eq!(win32.dirname("/"), "/");
    assert_eq!(win32.dirname("////"), "/");
    assert_eq!(win32.dirname("foo"), ".");

    // Testes do VSCode.

    let assert_dirname = |p: &str, expected: &str, win: bool| {
        let actual = if win { win32.dirname(p) } else { posix.dirname(p) };
        assert_eq!(actual, expected, "{p}: expected: {expected}, ours: {actual}");
    };

    assert_dirname("foo/bar", "foo", false);
    assert_dirname("foo\\bar", "foo", true);
    assert_dirname("/foo/bar", "/foo", false);
    assert_dirname("\\foo\\bar", "\\foo", true);
    assert_dirname("/foo", "/", false);
    assert_dirname("\\foo", "\\", true);
    assert_dirname("/", "/", false);
    assert_dirname("\\", "\\", true);
    assert_dirname("foo", ".", false);
    assert_dirname("f", ".", false);
    assert_dirname("f/", ".", false);
    assert_dirname("/folder/", "/", false);
    assert_dirname("c:\\some\\file.txt", "c:\\some", true);
    assert_dirname("c:\\some", "c:\\", true);
    assert_dirname("c:\\", "c:\\", true);
    assert_dirname("c:", "c:", true);
    assert_dirname("\\\\server\\share\\some\\path", "\\\\server\\share\\some", true);
    assert_dirname("\\\\server\\share\\some", "\\\\server\\share\\", true);
    assert_dirname("\\\\server\\share\\", "\\\\server\\share\\", true);
}

#[test]
fn extname() {
    let env = linux_env();
    let posix = PathModule::posix(&env);
    let win32 = PathModule::win32(&env);
    let native = PathModule::native(&env);
    let mut failures: Vec<String> = Vec::new();
    let filename = "path.test.js";

    let cases: &[(&str, &str)] = &[
        (filename, ".js"),
        ("", ""),
        ("/path/to/file", ""),
        ("/path/to/file.ext", ".ext"),
        ("/path.to/file.ext", ".ext"),
        ("/path.to/file", ""),
        ("/path.to/.file", ""),
        ("/path.to/.file.ext", ".ext"),
        ("/path/to/f.ext", ".ext"),
        ("/path/to/..ext", ".ext"),
        ("/path/to/..", ""),
        ("file", ""),
        ("file.ext", ".ext"),
        (".file", ""),
        (".file.ext", ".ext"),
        ("/file", ""),
        ("/file.ext", ".ext"),
        ("/.file", ""),
        ("/.file.ext", ".ext"),
        (".path/file.ext", ".ext"),
        ("file.ext.ext", ".ext"),
        ("file.", "."),
        (".", ""),
        ("./", ""),
        (".file.ext", ".ext"),
        (".file", ""),
        (".file.", "."),
        (".file..", "."),
        ("..", ""),
        ("../", ""),
        ("..file.ext", ".ext"),
        ("..file", ".file"),
        ("..file.", "."),
        ("..file..", "."),
        ("...", "."),
        ("...ext", ".ext"),
        ("....", "."),
        ("file.ext/", ".ext"),
        ("file.ext//", ".ext"),
        ("file/", ""),
        ("file//", ""),
        ("file./", "."),
        ("file.//", "."),
    ];
    for (input, expected) in cases {
        for (os, module) in [("posix", posix), ("win32", win32)] {
            let input = if os == "win32" { input.replace('/', "\\") } else { input.to_string() };
            let actual = module.extname(&input);
            let message = format!("path.{os}.extname({})\n  expect={}\n  actual={}", json(&input), json(expected), json(&actual));
            if actual != *expected {
                failures.push(format!("\n{message}"));
            }
        }
        {
            let input = format!("C:{}", input.replace('/', "\\"));
            let actual = win32.extname(&input);
            let message = format!("path.win32.extname({})\n  expect={}\n  actual={}", json(&input), json(expected), json(&actual));
            if actual != *expected {
                failures.push(format!("\n{message}"));
            }
        }
    }
    assert_no_failures(&failures);

    // No Windows, a barra invertida é separador de caminho.
    assert_eq!(win32.extname(".\\"), "");
    assert_eq!(win32.extname("..\\"), "");
    assert_eq!(win32.extname("file.ext\\"), ".ext");
    assert_eq!(win32.extname("file.ext\\\\"), ".ext");
    assert_eq!(win32.extname("file\\"), "");
    assert_eq!(win32.extname("file\\\\"), "");
    assert_eq!(win32.extname("file.\\"), ".");
    assert_eq!(win32.extname("file.\\\\"), ".");

    // Em *nix, a barra invertida é um caractere de nome como qualquer outro.
    assert_eq!(posix.extname(".\\"), "");
    assert_eq!(posix.extname("..\\"), ".\\");
    assert_eq!(posix.extname("file.ext\\"), ".ext\\");
    assert_eq!(posix.extname("file.ext\\\\"), ".ext\\\\");
    assert_eq!(posix.extname("file\\"), "");
    assert_eq!(posix.extname("file\\\\"), "");
    assert_eq!(posix.extname("file.\\"), ".\\");
    assert_eq!(posix.extname("file.\\\\"), ".\\\\");

    // Testes do VSCode.
    assert_eq!(native.extname("far.boo"), ".boo");
    assert_eq!(native.extname("far.b"), ".b");
    assert_eq!(native.extname("far."), ".");
    assert_eq!(native.extname("far.boo/boo.far"), ".far");
    assert_eq!(native.extname("far.boo/boo"), "");
}

#[test]
fn resolve() {
    let env = linux_env();
    let posix = PathModule::posix(&env);
    let win32 = PathModule::win32(&env);
    let native = PathModule::native(&env);
    let mut failures: Vec<String> = Vec::new();
    let cwd = env.cwd();

    let win32_cases: Vec<(Vec<&str>, String)> = vec![
        (vec!["c:/blah\\blah", "d:/games", "c:../a"], "c:\\blah\\a".into()),
        (vec!["c:/ignore", "d:\\a/b\\c/d", "\\e.exe"], "d:\\e.exe".into()),
        (vec!["c:/ignore", "c:/some/file"], "c:\\some\\file".into()),
        (vec!["d:/ignore", "d:some/dir//"], "d:\\ignore\\some\\dir".into()),
        (vec!["//server/share", "..", "relative\\"], "\\\\server\\share\\relative".into()),
        (vec!["c:/", "//"], "c:\\".into()),
        (vec!["c:/", "//dir"], "c:\\dir".into()),
        (vec!["c:/", "//server/share"], "\\\\server\\share\\".into()),
        (vec!["c:/", "//server//share"], "\\\\server\\share\\".into()),
        (vec!["c:/", "///some//dir"], "c:\\some\\dir".into()),
        (vec!["C:\\foo\\tmp.3\\", "..\\tmp.3\\cycles\\root.js"], "C:\\foo\\tmp.3\\cycles\\root.js".into()),
    ];
    let posix_cases: Vec<(Vec<&str>, String)> = vec![
        (vec!["/var/lib", "../", "file/"], "/var/file".into()),
        (vec!["/var/lib", "/../", "file/"], "/file".into()),
        (vec!["/some/dir", ".", "/absolute/"], "/absolute".into()),
        (vec!["/foo/tmp.3/", "../tmp.3/cycles/root.js"], "/foo/tmp.3/cycles/root.js".into()),
    ];
    // `isWeb` é falso, então o terceiro grupo usa `path.resolve` (a variante da plataforma).
    let native_cases: Vec<(Vec<&str>, String)> =
        vec![(vec!["."], cwd.clone()), (vec!["a/b/c", "../../.."], cwd.clone())];

    let windows_host = is_windows(&env);
    for (os, module, cases) in [("win32", win32, &win32_cases), ("posix", posix, &posix_cases), ("native", native, &native_cases)] {
        let is_win32_resolve = module.flavor() == Platform::Win32;
        for (args, expected) in cases {
            let actual = module.resolve(args);
            let actual_alt = if is_win32_resolve && !windows_host {
                Some(actual.replace('\\', "/"))
            } else if !is_win32_resolve && windows_host {
                Some(actual.replace('/', "\\"))
            } else {
                None
            };
            let message = format!(
                "path.{os}.resolve({})\n  expect={}\n  actual={}",
                args.iter().map(|a| json(a)).collect::<Vec<_>>().join(","),
                json(expected),
                json(&actual)
            );
            if actual != *expected && actual_alt.as_deref() != Some(expected.as_str()) {
                failures.push(format!("\n{message}"));
            }
        }
    }
    assert_no_failures(&failures);

    // if (isWindows) {
    // 	// Test resolving the current Windows drive letter from a spawned process.
    // 	// See https://github.com/nodejs/node/issues/7215
    // 	...
    // }
    // (comentado no original)
}

#[test]
fn basename() {
    let env = linux_env();
    let posix = PathModule::posix(&env);
    let win32 = PathModule::win32(&env);
    let native = PathModule::native(&env);
    let filename = "path.test.js";

    assert_eq!(native.basename(filename, None), "path.test.js");
    assert_eq!(native.basename(filename, Some(".js")), "path.test");
    assert_eq!(native.basename(".js", Some(".js")), "");
    assert_eq!(native.basename("", None), "");
    assert_eq!(native.basename("/dir/basename.ext", None), "basename.ext");
    assert_eq!(native.basename("/basename.ext", None), "basename.ext");
    assert_eq!(native.basename("basename.ext", None), "basename.ext");
    assert_eq!(native.basename("basename.ext/", None), "basename.ext");
    assert_eq!(native.basename("basename.ext//", None), "basename.ext");
    assert_eq!(native.basename("aaa/bbb", Some("/bbb")), "bbb");
    assert_eq!(native.basename("aaa/bbb", Some("a/bbb")), "bbb");
    assert_eq!(native.basename("aaa/bbb", Some("bbb")), "bbb");
    assert_eq!(native.basename("aaa/bbb//", Some("bbb")), "bbb");
    assert_eq!(native.basename("aaa/bbb", Some("bb")), "b");
    assert_eq!(native.basename("aaa/bbb", Some("b")), "bb");
    assert_eq!(native.basename("/aaa/bbb", Some("/bbb")), "bbb");
    assert_eq!(native.basename("/aaa/bbb", Some("a/bbb")), "bbb");
    assert_eq!(native.basename("/aaa/bbb", Some("bbb")), "bbb");
    assert_eq!(native.basename("/aaa/bbb//", Some("bbb")), "bbb");
    assert_eq!(native.basename("/aaa/bbb", Some("bb")), "b");
    assert_eq!(native.basename("/aaa/bbb", Some("b")), "bb");
    assert_eq!(native.basename("/aaa/bbb", None), "bbb");
    assert_eq!(native.basename("/aaa/", None), "aaa");
    assert_eq!(native.basename("/aaa/b", None), "b");
    assert_eq!(native.basename("/a/b", None), "b");
    assert_eq!(native.basename("//a", None), "a");
    assert_eq!(native.basename("a", Some("a")), "");

    // No Windows a barra invertida age como separador de caminho.
    assert_eq!(win32.basename("\\dir\\basename.ext", None), "basename.ext");
    assert_eq!(win32.basename("\\basename.ext", None), "basename.ext");
    assert_eq!(win32.basename("basename.ext", None), "basename.ext");
    assert_eq!(win32.basename("basename.ext\\", None), "basename.ext");
    assert_eq!(win32.basename("basename.ext\\\\", None), "basename.ext");
    assert_eq!(win32.basename("foo", None), "foo");
    assert_eq!(win32.basename("aaa\\bbb", Some("\\bbb")), "bbb");
    assert_eq!(win32.basename("aaa\\bbb", Some("a\\bbb")), "bbb");
    assert_eq!(win32.basename("aaa\\bbb", Some("bbb")), "bbb");
    assert_eq!(win32.basename("aaa\\bbb\\\\\\\\", Some("bbb")), "bbb");
    assert_eq!(win32.basename("aaa\\bbb", Some("bb")), "b");
    assert_eq!(win32.basename("aaa\\bbb", Some("b")), "bb");
    assert_eq!(win32.basename("C:", None), "");
    assert_eq!(win32.basename("C:.", None), ".");
    assert_eq!(win32.basename("C:\\", None), "");
    assert_eq!(win32.basename("C:\\dir\\base.ext", None), "base.ext");
    assert_eq!(win32.basename("C:\\basename.ext", None), "basename.ext");
    assert_eq!(win32.basename("C:basename.ext", None), "basename.ext");
    assert_eq!(win32.basename("C:basename.ext\\", None), "basename.ext");
    assert_eq!(win32.basename("C:basename.ext\\\\", None), "basename.ext");
    assert_eq!(win32.basename("C:foo", None), "foo");
    assert_eq!(win32.basename("file:stream", None), "file:stream");
    assert_eq!(win32.basename("a", Some("a")), "");

    // No unix a barra invertida é um caractere como outro qualquer.
    assert_eq!(posix.basename("\\dir\\basename.ext", None), "\\dir\\basename.ext");
    assert_eq!(posix.basename("\\basename.ext", None), "\\basename.ext");
    assert_eq!(posix.basename("basename.ext", None), "basename.ext");
    assert_eq!(posix.basename("basename.ext\\", None), "basename.ext\\");
    assert_eq!(posix.basename("basename.ext\\\\", None), "basename.ext\\\\");
    assert_eq!(posix.basename("foo", None), "foo");

    // Nomes de arquivo POSIX podem ter caracteres de controle, cf.
    // http://www.dwheeler.com/essays/fixing-unix-linux-filenames.html
    let control_char_filename = format!("Icon{}", char::from(13u8));
    assert_eq!(posix.basename(&format!("/a/b/{control_char_filename}"), None), control_char_filename);

    // Testes do VSCode.
    assert_eq!(native.basename("foo/bar", None), "bar");
    assert_eq!(posix.basename("foo\\bar", None), "foo\\bar");
    assert_eq!(win32.basename("foo\\bar", None), "bar");
    assert_eq!(native.basename("/foo/bar", None), "bar");
    assert_eq!(posix.basename("\\foo\\bar", None), "\\foo\\bar");
    assert_eq!(win32.basename("\\foo\\bar", None), "bar");
    assert_eq!(native.basename("./bar", None), "bar");
    assert_eq!(posix.basename(".\\bar", None), ".\\bar");
    assert_eq!(win32.basename(".\\bar", None), "bar");
    assert_eq!(native.basename("/bar", None), "bar");
    assert_eq!(posix.basename("\\bar", None), "\\bar");
    assert_eq!(win32.basename("\\bar", None), "bar");
    assert_eq!(native.basename("bar/", None), "bar");
    assert_eq!(posix.basename("bar\\", None), "bar\\");
    assert_eq!(win32.basename("bar\\", None), "bar");
    assert_eq!(native.basename("bar", None), "bar");
    assert_eq!(native.basename("////////", None), "");
    assert_eq!(posix.basename("\\\\\\\\", None), "\\\\\\\\");
    assert_eq!(win32.basename("\\\\\\\\", None), "");
}

#[test]
fn relative() {
    let env = linux_env();
    let posix = PathModule::posix(&env);
    let win32 = PathModule::win32(&env);
    let mut failures: Vec<String> = Vec::new();

    let win32_cases: &[(&str, &str, &str)] = &[
        ("c:/blah\\blah", "d:/games", "d:\\games"),
        ("c:/aaaa/bbbb", "c:/aaaa", ".."),
        ("c:/aaaa/bbbb", "c:/cccc", "..\\..\\cccc"),
        ("c:/aaaa/bbbb", "c:/aaaa/bbbb", ""),
        ("c:/aaaa/bbbb", "c:/aaaa/cccc", "..\\cccc"),
        ("c:/aaaa/", "c:/aaaa/cccc", "cccc"),
        ("c:/", "c:\\aaaa\\bbbb", "aaaa\\bbbb"),
        ("c:/aaaa/bbbb", "d:\\", "d:\\"),
        ("c:/AaAa/bbbb", "c:/aaaa/bbbb", ""),
        ("c:/aaaaa/", "c:/aaaa/cccc", "..\\aaaa\\cccc"),
        ("C:\\foo\\bar\\baz\\quux", "C:\\", "..\\..\\..\\.."),
        ("C:\\foo\\test", "C:\\foo\\test\\bar\\package.json", "bar\\package.json"),
        ("C:\\foo\\bar\\baz-quux", "C:\\foo\\bar\\baz", "..\\baz"),
        ("C:\\foo\\bar\\baz", "C:\\foo\\bar\\baz-quux", "..\\baz-quux"),
        ("\\\\foo\\bar", "\\\\foo\\bar\\baz", "baz"),
        ("\\\\foo\\bar\\baz", "\\\\foo\\bar", ".."),
        ("\\\\foo\\bar\\baz-quux", "\\\\foo\\bar\\baz", "..\\baz"),
        ("\\\\foo\\bar\\baz", "\\\\foo\\bar\\baz-quux", "..\\baz-quux"),
        ("C:\\baz-quux", "C:\\baz", "..\\baz"),
        ("C:\\baz", "C:\\baz-quux", "..\\baz-quux"),
        ("\\\\foo\\baz-quux", "\\\\foo\\baz", "..\\baz"),
        ("\\\\foo\\baz", "\\\\foo\\baz-quux", "..\\baz-quux"),
        ("C:\\baz", "\\\\foo\\bar\\baz", "\\\\foo\\bar\\baz"),
        ("\\\\foo\\bar\\baz", "C:\\baz", "C:\\baz"),
    ];
    let posix_cases: &[(&str, &str, &str)] = &[
        ("/var/lib", "/var", ".."),
        ("/var/lib", "/bin", "../../bin"),
        ("/var/lib", "/var/lib", ""),
        ("/var/lib", "/var/apache", "../apache"),
        ("/var/", "/var/lib", "lib"),
        ("/", "/var/lib", "var/lib"),
        ("/foo/test", "/foo/test/bar/package.json", "bar/package.json"),
        ("/Users/a/web/b/test/mails", "/Users/a/web/b", "../.."),
        ("/foo/bar/baz-quux", "/foo/bar/baz", "../baz"),
        ("/foo/bar/baz", "/foo/bar/baz-quux", "../baz-quux"),
        ("/baz-quux", "/baz", "../baz"),
        ("/baz", "/baz-quux", "../baz-quux"),
    ];

    for (os, module, cases) in [("win32", win32, win32_cases), ("posix", posix, posix_cases)] {
        for (from, to, expected) in cases {
            let actual = module.relative(from, to);
            let message =
                format!("path.{os}.relative({},{})\n  expect={}\n  actual={}", json(from), json(to), json(expected), json(&actual));
            if actual != *expected {
                failures.push(format!("\n{message}"));
            }
        }
    }
    assert_no_failures(&failures);
}

#[test]
fn normalize() {
    let env = linux_env();
    let posix = PathModule::posix(&env);
    let win32 = PathModule::win32(&env);

    assert_eq!(win32.normalize("./fixtures///b/../b/c.js"), "fixtures\\b\\c.js");
    assert_eq!(win32.normalize("/foo/../../../bar"), "\\bar");
    assert_eq!(win32.normalize("a//b//../b"), "a\\b");
    assert_eq!(win32.normalize("a//b//./c"), "a\\b\\c");
    assert_eq!(win32.normalize("a//b//."), "a\\b");
    assert_eq!(win32.normalize("//server/share/dir/file.ext"), "\\\\server\\share\\dir\\file.ext");
    assert_eq!(win32.normalize("/a/b/c/../../../x/y/z"), "\\x\\y\\z");
    assert_eq!(win32.normalize("C:"), "C:.");
    assert_eq!(win32.normalize("C:..\\abc"), "C:..\\abc");
    assert_eq!(win32.normalize("C:..\\..\\abc\\..\\def"), "C:..\\..\\def");
    assert_eq!(win32.normalize("C:\\."), "C:\\");
    assert_eq!(win32.normalize("file:stream"), "file:stream");
    assert_eq!(win32.normalize("bar\\foo..\\..\\"), "bar\\");
    assert_eq!(win32.normalize("bar\\foo..\\.."), "bar");
    assert_eq!(win32.normalize("bar\\foo..\\..\\baz"), "bar\\baz");
    assert_eq!(win32.normalize("bar\\foo..\\"), "bar\\foo..\\");
    assert_eq!(win32.normalize("bar\\foo.."), "bar\\foo..");
    assert_eq!(win32.normalize("..\\foo..\\..\\..\\bar"), "..\\..\\bar");
    assert_eq!(win32.normalize("..\\...\\..\\.\\...\\..\\..\\bar"), "..\\..\\bar");
    assert_eq!(win32.normalize("../../../foo/../../../bar"), "..\\..\\..\\..\\..\\bar");
    assert_eq!(win32.normalize("../../../foo/../../../bar/../../"), "..\\..\\..\\..\\..\\..\\");
    assert_eq!(win32.normalize("../foobar/barfoo/foo/../../../bar/../../"), "..\\..\\");
    assert_eq!(win32.normalize("../.../../foobar/../../../bar/../../baz"), "..\\..\\..\\..\\baz");
    assert_eq!(win32.normalize("foo/bar\\baz"), "foo\\bar\\baz");

    assert_eq!(posix.normalize("./fixtures///b/../b/c.js"), "fixtures/b/c.js");
    assert_eq!(posix.normalize("/foo/../../../bar"), "/bar");
    assert_eq!(posix.normalize("a//b//../b"), "a/b");
    assert_eq!(posix.normalize("a//b//./c"), "a/b/c");
    assert_eq!(posix.normalize("a//b//."), "a/b");
    assert_eq!(posix.normalize("/a/b/c/../../../x/y/z"), "/x/y/z");
    assert_eq!(posix.normalize("///..//./foo/.//bar"), "/foo/bar");
    assert_eq!(posix.normalize("bar/foo../../"), "bar/");
    assert_eq!(posix.normalize("bar/foo../.."), "bar");
    assert_eq!(posix.normalize("bar/foo../../baz"), "bar/baz");
    assert_eq!(posix.normalize("bar/foo../"), "bar/foo../");
    assert_eq!(posix.normalize("bar/foo.."), "bar/foo..");
    assert_eq!(posix.normalize("../foo../../../bar"), "../../bar");
    assert_eq!(posix.normalize("../.../.././.../../../bar"), "../../bar");
    assert_eq!(posix.normalize("../../../foo/../../../bar"), "../../../../../bar");
    assert_eq!(posix.normalize("../../../foo/../../../bar/../../"), "../../../../../../");
    assert_eq!(posix.normalize("../foobar/barfoo/foo/../../../bar/../../"), "../../");
    assert_eq!(posix.normalize("../.../../foobar/../../../bar/../../baz"), "../../../../baz");
    assert_eq!(posix.normalize("foo/bar\\baz"), "foo/bar\\baz");
}

#[test]
fn is_absolute() {
    let env = linux_env();
    let posix = PathModule::posix(&env);
    let win32 = PathModule::win32(&env);

    assert!(win32.is_absolute("/"));
    assert!(win32.is_absolute("//"));
    assert!(win32.is_absolute("//server"));
    assert!(win32.is_absolute("//server/file"));
    assert!(win32.is_absolute("\\\\server\\file"));
    assert!(win32.is_absolute("\\\\server"));
    assert!(win32.is_absolute("\\\\"));
    assert!(!win32.is_absolute("c"));
    assert!(!win32.is_absolute("c:"));
    assert!(win32.is_absolute("c:\\"));
    assert!(win32.is_absolute("c:/"));
    assert!(win32.is_absolute("c://"));
    assert!(win32.is_absolute("C:/Users/"));
    assert!(win32.is_absolute("C:\\Users\\"));
    assert!(!win32.is_absolute("C:cwd/another"));
    assert!(!win32.is_absolute("C:cwd\\another"));
    assert!(!win32.is_absolute("directory/directory"));
    assert!(!win32.is_absolute("directory\\directory"));

    assert!(posix.is_absolute("/home/foo"));
    assert!(posix.is_absolute("/home/foo/.."));
    assert!(!posix.is_absolute("bar/"));
    assert!(!posix.is_absolute("./baz"));

    // Testes do VSCode.

    // Caminhos absolutos.
    for absolute_path in
        ["C:/", "C:\\", "C:/foo", "C:\\foo", "z:/foo/bar.txt", "z:\\foo\\bar.txt", "\\\\localhost\\c$\\foo", "/", "/foo"]
    {
        assert!(win32.is_absolute(absolute_path), "{absolute_path}");
    }

    for absolute_path in ["/", "/foo", "/foo/bar.txt"] {
        assert!(posix.is_absolute(absolute_path), "{absolute_path}");
    }

    // Caminhos relativos.
    for non_absolute_path in ["", "foo", "foo/bar", "./foo", "http://foo.com/bar"] {
        assert!(!win32.is_absolute(non_absolute_path), "{non_absolute_path}");
    }

    for non_absolute_path in ["", "foo", "foo/bar", "./foo", "http://foo.com/bar", "z:/foo/bar.txt"] {
        assert!(!posix.is_absolute(non_absolute_path), "{non_absolute_path}");
    }
}

#[test]
fn path() {
    let env = linux_env();
    let posix = PathModule::posix(&env);
    let win32 = PathModule::win32(&env);

    // Testes de `path.sep`: windows.
    assert_eq!(win32.sep(), "\\");
    // posix.
    assert_eq!(posix.sep(), "/");

    // Testes de `path.delimiter`: windows.
    assert_eq!(win32.delimiter(), ";");
    // posix.
    assert_eq!(posix.delimiter(), ":");

    // if (isWindows) {
    // 	assert.strictEqual(path, path.win32);
    // } else {
    // 	assert.strictEqual(path, path.posix);
    // }
    // (comentado no original; aqui a variante da plataforma é a do ambiente)
    assert_eq!(PathModule::native(&env).flavor(), if is_windows(&env) { Platform::Win32 } else { Platform::Posix });
}
