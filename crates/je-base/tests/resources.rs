//! Porte de `src/vs/base/test/common/resources.test.ts` (suíte `Resources`), caso a caso e na mesma ordem.
//!
//! O original escolhe o ramo por `isWindows` e usa os singletons `extUri` e `extUriIgnorePathCase` (e as
//! funções soltas ligadas a `extUri`). Aqui as instâncias saem de `ExtUri::unbiased` e
//! `ExtUri::ignore_path_case` com a plataforma injetada: os testes com ramo de Windows rodam duas vezes,
//! com `Posix` (o ramo que o original executa no Linux) e com `Win32` (sufixo `_win32`). As funções
//! soltas do original (`joinPath`, `basename`...) são métodos da instância não enviesada.
//! Os testes gerados por laços `for` do original (esquema e `ignoreCase`) viram uma função por
//! combinação, com a combinação no nome.

use std::rc::Rc;

use je_base::extpath::to_slashes;
use je_base::network::schemas;
use je_base::path::{FixedEnvironment, PathModule, Platform};
use je_base::resources::{distinct_parents, ExtUri};
use je_base::uri::{Field, UriChange, UriComponents, URI};
use je_base::JsString;

const POSIX: Platform = Platform::Posix;
const WIN32: Platform = Platform::Win32;

struct Ctx {
    platform: Platform,
    env: Rc<FixedEnvironment>,
    ext_uri: ExtUri,
    ext_uri_ignore_path_case: ExtUri,
}

impl Ctx {
    fn new(platform: Platform) -> Self {
        let env = Rc::new(FixedEnvironment::new(platform, if platform == WIN32 { "C:\\workspace\\project" } else { "/workspace/project" }));
        Self {
            platform,
            ext_uri: ExtUri::unbiased(env.clone()),
            ext_uri_ignore_path_case: ExtUri::ignore_path_case(env.clone()),
            env,
        }
    }

    fn is_windows(&self) -> bool {
        self.platform == WIN32
    }

    fn file(&self, path: &str) -> URI {
        URI::file(path, self.platform)
    }

    fn util(&self, ignore_case: Option<bool>) -> &ExtUri {
        if ignore_case == Some(true) { &self.ext_uri_ignore_path_case } else { &self.ext_uri }
    }

    fn assert_equal_uri(&self, actual: &URI, expected: &URI, message: &str, ignore_case: bool) {
        let util = self.util(Some(ignore_case));
        if !util.is_equal(Some(expected), Some(actual), false) {
            assert_eq!(actual.to_string(false), expected.to_string(false), "{message}");
        }
    }
}

fn parse(value: &str) -> URI {
    URI::parse(value, false)
}

fn from_components(scheme: &str, authority: &str, path: &str) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(scheme.into()),
            authority: Some(authority.into()),
            path: Some(path.into()),
            ..UriComponents::default()
        },
        false,
    )
}

fn with(uri: &URI, change: UriChange) -> URI {
    uri.with(&change)
}

fn with_path(uri: &URI, path: impl Into<JsString>) -> URI {
    with(uri, UriChange { path: Field::Set(path.into()), ..UriChange::default() })
}

fn ts(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

#[test]
fn distinct_parents_case() {
    let c = Ctx::new(POSIX);

    // Basic
    let mut resources = vec![c.file("/some/folderA/file.txt"), c.file("/some/folderB/file.txt"), c.file("/some/folderC/file.txt")];

    let mut distinct = distinct_parents(&resources, |r| r.clone(), &c.ext_uri);
    assert_eq!(distinct.len(), 3);
    assert_eq!(ts(distinct[0]), ts(&resources[0]));
    assert_eq!(ts(distinct[1]), ts(&resources[1]));
    assert_eq!(ts(distinct[2]), ts(&resources[2]));

    // Parent / Child
    resources = vec![
        c.file("/some/folderA"),
        c.file("/some/folderA/file.txt"),
        c.file("/some/folderA/child/file.txt"),
        c.file("/some/folderA2/file.txt"),
        c.file("/some/file.txt"),
    ];

    distinct = distinct_parents(&resources, |r| r.clone(), &c.ext_uri);
    assert_eq!(distinct.len(), 3);
    assert_eq!(ts(distinct[0]), ts(&resources[0]));
    assert_eq!(ts(distinct[1]), ts(&resources[3]));
    assert_eq!(ts(distinct[2]), ts(&resources[4]));
}

fn dirname(platform: Platform) {
    let c = Ctx::new(platform);
    let dirname = |uri: &URI| ts(&c.ext_uri.dirname(uri));
    if c.is_windows() {
        assert_eq!(dirname(&c.file("c:\\some\\file\\test.txt")), "file:///c%3A/some/file");
        assert_eq!(dirname(&c.file("c:\\some\\file")), "file:///c%3A/some");
        assert_eq!(dirname(&c.file("c:\\some\\file\\")), "file:///c%3A/some");
        assert_eq!(dirname(&c.file("c:\\some")), "file:///c%3A/");
        assert_eq!(dirname(&c.file("C:\\some")), "file:///c%3A/");
        assert_eq!(dirname(&c.file("c:\\")), "file:///c%3A/");
    } else {
        assert_eq!(dirname(&c.file("/some/file/test.txt")), "file:///some/file");
        assert_eq!(dirname(&c.file("/some/file/")), "file:///some");
        assert_eq!(dirname(&c.file("/some/file")), "file:///some");
    }
    assert_eq!(dirname(&parse("foo://a/some/file/test.txt")), "foo://a/some/file");
    assert_eq!(dirname(&parse("foo://a/some/file/")), "foo://a/some");
    assert_eq!(dirname(&parse("foo://a/some/file")), "foo://a/some");
    assert_eq!(dirname(&parse("foo://a/some")), "foo://a/");
    assert_eq!(dirname(&parse("foo://a/")), "foo://a/");
    assert_eq!(dirname(&parse("foo://a")), "foo://a");

    // does not explode (https://github.com/microsoft/vscode/issues/41987)
    c.ext_uri.dirname(&from_components("file", "/users/someone/portal.h", ""));

    assert_eq!(dirname(&parse("foo://a/b/c?q")), "foo://a/b?q");
}

#[test]
fn dirname_posix() {
    dirname(POSIX);
}

#[test]
fn dirname_win32() {
    dirname(WIN32);
}

fn basename(platform: Platform) {
    let c = Ctx::new(platform);
    let basename = |uri: &URI| c.ext_uri.basename(uri, None);
    if c.is_windows() {
        assert_eq!(basename(&c.file("c:\\some\\file\\test.txt")), "test.txt");
        assert_eq!(basename(&c.file("c:\\some\\file")), "file");
        assert_eq!(basename(&c.file("c:\\some\\file\\")), "file");
        assert_eq!(basename(&c.file("C:\\some\\file\\")), "file");
    } else {
        assert_eq!(basename(&c.file("/some/file/test.txt")), "test.txt");
        assert_eq!(basename(&c.file("/some/file/")), "file");
        assert_eq!(basename(&c.file("/some/file")), "file");
        assert_eq!(basename(&c.file("/some")), "some");
    }
    assert_eq!(basename(&parse("foo://a/some/file/test.txt")), "test.txt");
    assert_eq!(basename(&parse("foo://a/some/file/")), "file");
    assert_eq!(basename(&parse("foo://a/some/file")), "file");
    assert_eq!(basename(&parse("foo://a/some")), "some");
    assert_eq!(basename(&parse("foo://a/")), "");
    assert_eq!(basename(&parse("foo://a")), "");
}

#[test]
fn basename_posix() {
    basename(POSIX);
}

#[test]
fn basename_win32() {
    basename(WIN32);
}

fn join_path(platform: Platform) {
    let c = Ctx::new(platform);
    let join = |uri: &URI, fragments: &[&str]| ts(&c.ext_uri.join_path(uri, fragments));
    if c.is_windows() {
        assert_eq!(join(&c.file("c:\\foo\\bar"), &["/file.js"]), "file:///c%3A/foo/bar/file.js");
        assert_eq!(join(&c.file("c:\\foo\\bar\\"), &["file.js"]), "file:///c%3A/foo/bar/file.js");
        assert_eq!(join(&c.file("c:\\foo\\bar\\"), &["/file.js"]), "file:///c%3A/foo/bar/file.js");
        assert_eq!(join(&c.file("c:\\"), &["/file.js"]), "file:///c%3A/file.js");
        assert_eq!(join(&c.file("c:\\"), &["bar/file.js"]), "file:///c%3A/bar/file.js");
        assert_eq!(join(&c.file("c:\\foo"), &["./file.js"]), "file:///c%3A/foo/file.js");
        assert_eq!(join(&c.file("c:\\foo"), &["/./file.js"]), "file:///c%3A/foo/file.js");
        assert_eq!(join(&c.file("C:\\foo"), &["../file.js"]), "file:///c%3A/file.js");
        assert_eq!(join(&c.file("C:\\foo\\."), &["../file.js"]), "file:///c%3A/file.js");
    } else {
        assert_eq!(join(&c.file("/foo/bar"), &["/file.js"]), "file:///foo/bar/file.js");
        assert_eq!(join(&c.file("/foo/bar"), &["file.js"]), "file:///foo/bar/file.js");
        assert_eq!(join(&c.file("/foo/bar/"), &["/file.js"]), "file:///foo/bar/file.js");
        assert_eq!(join(&c.file("/"), &["/file.js"]), "file:///file.js");
        assert_eq!(join(&c.file("/foo/bar"), &["./file.js"]), "file:///foo/bar/file.js");
        assert_eq!(join(&c.file("/foo/bar"), &["/./file.js"]), "file:///foo/bar/file.js");
        assert_eq!(join(&c.file("/foo/bar"), &["../file.js"]), "file:///foo/file.js");
    }
    assert_eq!(join(&parse("foo://a/foo/bar"), &[]), "foo://a/foo/bar");
    assert_eq!(join(&parse("foo://a/foo/bar"), &["/file.js"]), "foo://a/foo/bar/file.js");
    assert_eq!(join(&parse("foo://a/foo/bar"), &["file.js"]), "foo://a/foo/bar/file.js");
    assert_eq!(join(&parse("foo://a/foo/bar/"), &["/file.js"]), "foo://a/foo/bar/file.js");
    assert_eq!(join(&parse("foo://a/"), &["/file.js"]), "foo://a/file.js");
    assert_eq!(join(&parse("foo://a/foo/bar/"), &["./file.js"]), "foo://a/foo/bar/file.js");
    assert_eq!(join(&parse("foo://a/foo/bar/"), &["/./file.js"]), "foo://a/foo/bar/file.js");
    assert_eq!(join(&parse("foo://a/foo/bar/"), &["../file.js"]), "foo://a/foo/file.js");

    let full = URI::from(
        &UriComponents {
            scheme: Some("myScheme".into()),
            authority: Some("authority".into()),
            path: Some("/path".into()),
            query: Some("query".into()),
            fragment: Some("fragment".into()),
        },
        false,
    );
    assert_eq!(join(&full, &["/file.js"]), "myScheme://authority/path/file.js?query#fragment");
}

#[test]
fn join_path_posix() {
    join_path(POSIX);
}

#[test]
fn join_path_win32() {
    join_path(WIN32);
}

fn normalize_path(platform: Platform) {
    let c = Ctx::new(platform);
    let normalize = |uri: &URI| ts(&c.ext_uri.normalize_path(uri));
    if c.is_windows() {
        assert_eq!(normalize(&c.file("c:\\foo\\.\\bar")), "file:///c%3A/foo/bar");
        assert_eq!(normalize(&c.file("c:\\foo\\.")), "file:///c%3A/foo");
        assert_eq!(normalize(&c.file("c:\\foo\\.\\")), "file:///c%3A/foo/");
        assert_eq!(normalize(&c.file("c:\\foo\\..")), "file:///c%3A/");
        assert_eq!(normalize(&c.file("c:\\foo\\..\\bar")), "file:///c%3A/bar");
        assert_eq!(normalize(&c.file("c:\\foo\\..\\..\\bar")), "file:///c%3A/bar");
        assert_eq!(normalize(&c.file("c:\\foo\\foo\\..\\..\\bar")), "file:///c%3A/bar");
        assert_eq!(normalize(&c.file("C:\\foo\\foo\\.\\..\\..\\bar")), "file:///c%3A/bar");
        assert_eq!(normalize(&c.file("C:\\foo\\foo\\.\\..\\some\\..\\bar")), "file:///c%3A/foo/bar");
    } else {
        assert_eq!(normalize(&c.file("/foo/./bar")), "file:///foo/bar");
        assert_eq!(normalize(&c.file("/foo/.")), "file:///foo");
        assert_eq!(normalize(&c.file("/foo/./")), "file:///foo/");
        assert_eq!(normalize(&c.file("/foo/..")), "file:///");
        assert_eq!(normalize(&c.file("/foo/../bar")), "file:///bar");
        assert_eq!(normalize(&c.file("/foo/../../bar")), "file:///bar");
        assert_eq!(normalize(&c.file("/foo/foo/../../bar")), "file:///bar");
        assert_eq!(normalize(&c.file("/foo/foo/./../../bar")), "file:///bar");
        assert_eq!(normalize(&c.file("/foo/foo/./../some/../bar")), "file:///foo/bar");
        assert_eq!(normalize(&c.file("/f")), "file:///f");
    }
    assert_eq!(normalize(&parse("foo://a/foo/./bar")), "foo://a/foo/bar");
    assert_eq!(normalize(&parse("foo://a/foo/.")), "foo://a/foo");
    assert_eq!(normalize(&parse("foo://a/foo/./")), "foo://a/foo/");
    assert_eq!(normalize(&parse("foo://a/foo/..")), "foo://a/");
    assert_eq!(normalize(&parse("foo://a/foo/../bar")), "foo://a/bar");
    assert_eq!(normalize(&parse("foo://a/foo/../../bar")), "foo://a/bar");
    assert_eq!(normalize(&parse("foo://a/foo/foo/../../bar")), "foo://a/bar");
    assert_eq!(normalize(&parse("foo://a/foo/foo/./../../bar")), "foo://a/bar");
    assert_eq!(normalize(&parse("foo://a/foo/foo/./../some/../bar")), "foo://a/foo/bar");
    assert_eq!(normalize(&parse("foo://a")), "foo://a");
    assert_eq!(normalize(&parse("foo://a/")), "foo://a/");
    assert_eq!(normalize(&parse("foo://a/foo/./bar?q=1")), ts(&parse("foo://a/foo/bar?q%3D1")));
}

#[test]
fn normalize_path_posix() {
    normalize_path(POSIX);
}

#[test]
fn normalize_path_win32() {
    normalize_path(WIN32);
}

fn is_absolute(platform: Platform) {
    let c = Ctx::new(platform);
    let is_absolute_path = |uri: &URI| c.ext_uri.is_absolute_path(uri);
    if c.is_windows() {
        assert!(is_absolute_path(&c.file("c:\\foo\\")));
        assert!(is_absolute_path(&c.file("C:\\foo\\")));
        assert!(is_absolute_path(&c.file("bar"))); // URI normalizes all file URIs to be absolute
    } else {
        assert!(is_absolute_path(&c.file("/foo/bar")));
        assert!(is_absolute_path(&c.file("bar"))); // URI normalizes all file URIs to be absolute
    }
    assert!(!is_absolute_path(&parse("foo:foo")));
    assert!(is_absolute_path(&parse("foo://a/foo/.")));
}

#[test]
fn is_absolute_posix() {
    is_absolute(POSIX);
}

#[test]
fn is_absolute_win32() {
    is_absolute(WIN32);
}

fn assert_trailing_separator(c: &Ctx, u1: &URI, expected: bool) {
    assert_eq!(c.ext_uri.has_trailing_path_separator(u1, None), expected, "{}", ts(u1));
}

fn assert_remove_trailing_separator(c: &Ctx, u1: &URI, expected: &URI) {
    c.assert_equal_uri(&c.ext_uri.remove_trailing_path_separator(u1, None), expected, &ts(u1), false);
}

fn assert_add_trailing_separator(c: &Ctx, u1: &URI, expected: &URI) {
    c.assert_equal_uri(&c.ext_uri.add_trailing_path_separator(u1, None), expected, &ts(u1), false);
}

fn trailing_path_separator(platform: Platform) {
    let c = Ctx::new(platform);
    assert_trailing_separator(&c, &parse("foo://a/foo"), false);
    assert_trailing_separator(&c, &parse("foo://a/foo/"), true);
    assert_trailing_separator(&c, &parse("foo://a/"), false);
    assert_trailing_separator(&c, &parse("foo://a"), false);

    assert_remove_trailing_separator(&c, &parse("foo://a/foo"), &parse("foo://a/foo"));
    assert_remove_trailing_separator(&c, &parse("foo://a/foo/"), &parse("foo://a/foo"));
    assert_remove_trailing_separator(&c, &parse("foo://a/"), &parse("foo://a/"));
    assert_remove_trailing_separator(&c, &parse("foo://a"), &parse("foo://a"));

    assert_add_trailing_separator(&c, &parse("foo://a/foo"), &parse("foo://a/foo/"));
    assert_add_trailing_separator(&c, &parse("foo://a/foo/"), &parse("foo://a/foo/"));
    assert_add_trailing_separator(&c, &parse("foo://a/"), &parse("foo://a/"));
    assert_add_trailing_separator(&c, &parse("foo://a"), &parse("foo://a/"));

    if c.is_windows() {
        assert_trailing_separator(&c, &c.file("c:\\a\\foo"), false);
        assert_trailing_separator(&c, &c.file("c:\\a\\foo\\"), true);
        assert_trailing_separator(&c, &c.file("c:\\"), false);
        assert_trailing_separator(&c, &c.file("\\\\server\\share\\some\\"), true);
        assert_trailing_separator(&c, &c.file("\\\\server\\share\\"), false);

        assert_remove_trailing_separator(&c, &c.file("c:\\a\\foo"), &c.file("c:\\a\\foo"));
        assert_remove_trailing_separator(&c, &c.file("c:\\a\\foo\\"), &c.file("c:\\a\\foo"));
        assert_remove_trailing_separator(&c, &c.file("c:\\"), &c.file("c:\\"));
        assert_remove_trailing_separator(&c, &c.file("\\\\server\\share\\some\\"), &c.file("\\\\server\\share\\some"));
        assert_remove_trailing_separator(&c, &c.file("\\\\server\\share\\"), &c.file("\\\\server\\share\\"));

        assert_add_trailing_separator(&c, &c.file("c:\\a\\foo"), &c.file("c:\\a\\foo\\"));
        assert_add_trailing_separator(&c, &c.file("c:\\a\\foo\\"), &c.file("c:\\a\\foo\\"));
        assert_add_trailing_separator(&c, &c.file("c:\\"), &c.file("c:\\"));
        assert_add_trailing_separator(&c, &c.file("\\\\server\\share\\some"), &c.file("\\\\server\\share\\some\\"));
        assert_add_trailing_separator(&c, &c.file("\\\\server\\share\\some\\"), &c.file("\\\\server\\share\\some\\"));
    } else {
        assert_trailing_separator(&c, &c.file("/foo/bar"), false);
        assert_trailing_separator(&c, &c.file("/foo/bar/"), true);
        assert_trailing_separator(&c, &c.file("/"), false);

        assert_remove_trailing_separator(&c, &c.file("/foo/bar"), &c.file("/foo/bar"));
        assert_remove_trailing_separator(&c, &c.file("/foo/bar/"), &c.file("/foo/bar"));
        assert_remove_trailing_separator(&c, &c.file("/"), &c.file("/"));

        assert_add_trailing_separator(&c, &c.file("/foo/bar"), &c.file("/foo/bar/"));
        assert_add_trailing_separator(&c, &c.file("/foo/bar/"), &c.file("/foo/bar/"));
        assert_add_trailing_separator(&c, &c.file("/"), &c.file("/"));
    }
}

#[test]
fn trailing_path_separator_posix() {
    trailing_path_separator(POSIX);
}

#[test]
fn trailing_path_separator_win32() {
    trailing_path_separator(WIN32);
}

fn assert_relative_path(c: &Ctx, u1: &URI, u2: &URI, expected_path: Option<&str>, ignore_join: bool, ignore_case: bool) {
    let util = c.util(Some(ignore_case));

    assert_eq!(util.relative_path(u1, u2).as_deref(), expected_path, "from {} to {}", ts(u1), ts(u2));
    if let Some(expected_path) = expected_path
        && !ignore_join
    {
        c.assert_equal_uri(
            &c.ext_uri.remove_trailing_path_separator(&c.ext_uri.join_path(u1, &[expected_path]), None),
            &c.ext_uri.remove_trailing_path_separator(u2, None),
            "joinPath on relativePath should be equal",
            ignore_case,
        );
    }
}

fn relative_path(platform: Platform) {
    let c = Ctx::new(platform);
    let check = |u1: &str, u2: &str, expected: Option<&str>, ignore_join: bool, ignore_case: bool| {
        assert_relative_path(&c, &parse(u1), &parse(u2), expected, ignore_join, ignore_case);
    };
    check("foo://a/foo", "foo://a/foo/bar", Some("bar"), false, false);
    check("foo://a/foo", "foo://a/foo/bar/", Some("bar"), false, false);
    check("foo://a/foo", "foo://a/foo/bar/goo", Some("bar/goo"), false, false);
    check("foo://a/", "foo://a/foo/bar/goo", Some("foo/bar/goo"), false, false);
    check("foo://a/foo/xoo", "foo://a/foo/bar", Some("../bar"), false, false);
    check("foo://a/foo/xoo/yoo", "foo://a", Some("../../.."), true, false);
    check("foo://a/foo", "foo://a/foo/", Some(""), false, false);
    check("foo://a/foo/", "foo://a/foo", Some(""), false, false);
    check("foo://a/foo/", "foo://a/foo/", Some(""), false, false);
    check("foo://a/foo", "foo://a/foo", Some(""), false, false);
    check("foo://a", "foo://a", Some(""), true, false);
    check("foo://a/", "foo://a/", Some(""), false, false);
    check("foo://a/", "foo://a", Some(""), true, false);
    check("foo://a/foo?q", "foo://a/foo/bar#h", Some("bar"), true, false);
    check("foo://", "foo://a/b", None, false, false);
    check("foo://a2/b", "foo://a/b", None, false, false);
    check("goo://a/b", "foo://a/b", None, false, false);

    check("foo://a/foo", "foo://A/FOO/bar/goo", Some("bar/goo"), false, true);
    check("foo://a/foo", "foo://A/FOO/BAR/GOO", Some("BAR/GOO"), false, true);
    check("foo://a/foo/xoo", "foo://A/FOO/BAR/GOO", Some("../BAR/GOO"), false, true);
    check("foo:///c:/a/foo", "foo:///C:/a/foo/xoo/", Some("xoo"), false, true);

    let both = |u1: &URI, u2: &URI, expected: Option<&str>, ignore_join: bool| assert_relative_path(&c, u1, u2, expected, ignore_join, false);
    if c.is_windows() {
        both(&c.file("c:\\foo\\bar"), &c.file("c:\\foo\\bar"), Some(""), false);
        both(&c.file("c:\\foo\\bar\\huu"), &c.file("c:\\foo\\bar"), Some(".."), false);
        both(&c.file("c:\\foo\\bar\\a1\\a2"), &c.file("c:\\foo\\bar"), Some("../.."), false);
        both(&c.file("c:\\foo\\bar\\"), &c.file("c:\\foo\\bar\\a1\\a2"), Some("a1/a2"), false);
        both(&c.file("c:\\foo\\bar\\"), &c.file("c:\\foo\\bar\\a1\\a2\\"), Some("a1/a2"), false);
        both(&c.file("c:\\"), &c.file("c:\\foo\\bar"), Some("foo/bar"), false);
        both(&c.file("\\\\server\\share\\some\\"), &c.file("\\\\server\\share\\some\\path"), Some("path"), false);
        // ignore joinPath assert: path.join is not root aware
        both(&c.file("\\\\server\\share\\some\\"), &c.file("\\\\server\\share2\\some\\path"), Some("../../share2/some/path"), true);
    } else {
        both(&c.file("/a/foo"), &c.file("/a/foo/bar"), Some("bar"), false);
        both(&c.file("/a/foo"), &c.file("/a/foo/bar/"), Some("bar"), false);
        both(&c.file("/a/foo"), &c.file("/a/foo/bar/goo"), Some("bar/goo"), false);
        both(&c.file("/a/"), &c.file("/a/foo/bar/goo"), Some("foo/bar/goo"), false);
        both(&c.file("/"), &c.file("/a/foo/bar/goo"), Some("a/foo/bar/goo"), false);
        both(&c.file("/a/foo/xoo"), &c.file("/a/foo/bar"), Some("../bar"), false);
        both(&c.file("/a/foo/xoo/yoo"), &c.file("/a"), Some("../../.."), false);
        both(&c.file("/a/foo"), &c.file("/a/foo/"), Some(""), false);
        both(&c.file("/a/foo"), &c.file("/b/foo/"), Some("../../b/foo"), false);
    }
}

#[test]
fn relative_path_posix() {
    relative_path(POSIX);
}

#[test]
fn relative_path_win32() {
    relative_path(WIN32);
}

fn assert_resolve(c: &Ctx, u1: &URI, path: &str, expected: &URI) {
    let actual = c.ext_uri.resolve_path(u1, path);
    c.assert_equal_uri(&actual, expected, &format!("from {} and {}", ts(u1), path), false);

    let module = if path.contains('/') { PathModule::posix(c.env.as_ref()) } else { PathModule::win32(c.env.as_ref()) };
    if !module.is_absolute(path) {
        let mut expected_path = if c.is_windows() { to_slashes(path) } else { path.to_string() };
        expected_path = expected_path.strip_prefix("./").map(str::to_string).unwrap_or(expected_path);
        assert_eq!(
            c.ext_uri.relative_path(u1, &actual).as_deref(),
            Some(expected_path.as_str()),
            "relativePath ({}) on actual ({}) should be to path ({})",
            ts(u1),
            ts(&actual),
            expected_path
        );
    }
}

fn resolve(platform: Platform) {
    let c = Ctx::new(platform);
    if c.is_windows() {
        assert_resolve(&c, &c.file("c:\\foo\\bar"), "file.js", &c.file("c:\\foo\\bar\\file.js"));
        assert_resolve(&c, &c.file("c:\\foo\\bar"), "t\\file.js", &c.file("c:\\foo\\bar\\t\\file.js"));
        assert_resolve(&c, &c.file("c:\\foo\\bar"), ".\\t\\file.js", &c.file("c:\\foo\\bar\\t\\file.js"));
        assert_resolve(&c, &c.file("c:\\foo\\bar"), "a1/file.js", &c.file("c:\\foo\\bar\\a1\\file.js"));
        assert_resolve(&c, &c.file("c:\\foo\\bar"), "./a1/file.js", &c.file("c:\\foo\\bar\\a1\\file.js"));
        assert_resolve(&c, &c.file("c:\\foo\\bar"), "\\b1\\file.js", &c.file("c:\\b1\\file.js"));
        assert_resolve(&c, &c.file("c:\\foo\\bar"), "/b1/file.js", &c.file("c:\\b1\\file.js"));
        assert_resolve(&c, &c.file("c:\\foo\\bar\\"), "file.js", &c.file("c:\\foo\\bar\\file.js"));

        assert_resolve(&c, &c.file("c:\\"), "file.js", &c.file("c:\\file.js"));
        assert_resolve(&c, &c.file("c:\\"), "\\b1\\file.js", &c.file("c:\\b1\\file.js"));
        assert_resolve(&c, &c.file("c:\\"), "/b1/file.js", &c.file("c:\\b1\\file.js"));
        assert_resolve(&c, &c.file("c:\\"), "d:\\foo\\bar.txt", &c.file("d:\\foo\\bar.txt"));

        assert_resolve(&c, &c.file("\\\\server\\share\\some\\"), "b1\\file.js", &c.file("\\\\server\\share\\some\\b1\\file.js"));
        assert_resolve(&c, &c.file("\\\\server\\share\\some\\"), "\\file.js", &c.file("\\\\server\\share\\file.js"));

        assert_resolve(&c, &c.file("c:\\"), "\\\\server\\share\\some\\", &c.file("\\\\server\\share\\some"));
        assert_resolve(&c, &c.file("\\\\server\\share\\some\\"), "c:\\", &c.file("c:\\"));
    } else {
        assert_resolve(&c, &c.file("/foo/bar"), "file.js", &c.file("/foo/bar/file.js"));
        assert_resolve(&c, &c.file("/foo/bar"), "./file.js", &c.file("/foo/bar/file.js"));
        assert_resolve(&c, &c.file("/foo/bar"), "/file.js", &c.file("/file.js"));
        assert_resolve(&c, &c.file("/foo/bar/"), "file.js", &c.file("/foo/bar/file.js"));
        assert_resolve(&c, &c.file("/"), "file.js", &c.file("/file.js"));
        assert_resolve(&c, &c.file(""), "./file.js", &c.file("/file.js"));
        assert_resolve(&c, &c.file(""), "/file.js", &c.file("/file.js"));
    }

    assert_resolve(&c, &parse("foo://server/foo/bar"), "file.js", &parse("foo://server/foo/bar/file.js"));
    assert_resolve(&c, &parse("foo://server/foo/bar"), "./file.js", &parse("foo://server/foo/bar/file.js"));
    assert_resolve(&c, &parse("foo://server/foo/bar"), "./file.js", &parse("foo://server/foo/bar/file.js"));
    assert_resolve(&c, &parse("foo://server/foo/bar"), "c:\\a1\\b1", &parse("foo://server/c:/a1/b1"));
    assert_resolve(&c, &parse("foo://server/foo/bar"), "c:\\", &parse("foo://server/c:"));
}

#[test]
fn resolve_posix() {
    resolve(POSIX);
}

#[test]
fn resolve_win32() {
    resolve(WIN32);
}

fn assert_is_equal(c: &Ctx, u1: &URI, u2: &URI, ignore_case: Option<bool>, expected: bool) {
    let util = c.util(ignore_case);

    assert_eq!(util.is_equal(Some(u1), Some(u2), false), expected, "{}{}{}", ts(u1), if expected { "===" } else { "!==" }, ts(u2));
    assert_eq!(util.compare(u1, u2, false) == 0, expected);
    assert_eq!(
        util.get_comparison_key(u1, false) == util.get_comparison_key(u2, false),
        expected,
        "comparison keys {}, {}",
        ts(u1),
        ts(u2)
    );
    assert_eq!(util.is_equal_or_parent(u1, u2, false, false), expected, "isEqualOrParent {}, {}", ts(u1), ts(u2));
    if ignore_case != Some(true) {
        assert_eq!(u1.to_string(false) == u2.to_string(false), expected);
    }
}

fn is_equal(platform: Platform) {
    let c = Ctx::new(platform);
    let file_uri = if c.is_windows() { c.file("c:\\foo\\bar") } else { c.file("/foo/bar") };
    let file_uri2 = if c.is_windows() { c.file("C:\\foo\\Bar") } else { c.file("/foo/Bar") };
    assert_is_equal(&c, &file_uri, &file_uri, Some(true), true);
    assert_is_equal(&c, &file_uri, &file_uri, Some(false), true);
    assert_is_equal(&c, &file_uri, &file_uri, None, true);
    assert_is_equal(&c, &file_uri, &file_uri2, Some(true), true);
    assert_is_equal(&c, &file_uri, &file_uri2, Some(false), false);

    let file_uri3 = parse("foo://server:453/foo/bar");
    let file_uri4 = parse("foo://server:453/foo/Bar");
    assert_is_equal(&c, &file_uri3, &file_uri3, Some(true), true);
    assert_is_equal(&c, &file_uri3, &file_uri3, Some(false), true);
    assert_is_equal(&c, &file_uri3, &file_uri3, None, true);
    assert_is_equal(&c, &file_uri3, &file_uri4, Some(true), true);
    assert_is_equal(&c, &file_uri3, &file_uri4, Some(false), false);

    assert_is_equal(&c, &file_uri, &file_uri3, Some(true), false);

    assert_is_equal(&c, &parse("file://server"), &parse("file://server/"), Some(true), true);
    assert_is_equal(&c, &parse("http://server"), &parse("http://server/"), Some(true), true);
    assert_is_equal(&c, &parse("foo://server"), &parse("foo://server/"), Some(true), false); // only selected scheme have / as the default path
    assert_is_equal(&c, &parse("foo://server/foo"), &parse("foo://server/foo/"), Some(true), false);
    assert_is_equal(&c, &parse("foo://server/foo"), &parse("foo://server/foo?"), Some(true), true);

    let file_uri5 = parse("foo://server:453/foo/bar?q=1");
    let file_uri6 = parse("foo://server:453/foo/bar#xy");

    assert_is_equal(&c, &file_uri5, &file_uri5, Some(true), true);
    assert_is_equal(&c, &file_uri5, &file_uri3, Some(true), false);
    assert_is_equal(&c, &file_uri6, &file_uri6, Some(true), true);
    assert_is_equal(&c, &file_uri6, &file_uri5, Some(true), false);
    assert_is_equal(&c, &file_uri6, &file_uri3, Some(true), false);
}

#[test]
fn is_equal_posix() {
    is_equal(POSIX);
}

#[test]
fn is_equal_win32() {
    is_equal(WIN32);
}

fn is_equal_or_parent(platform: Platform) {
    let c = Ctx::new(platform);
    let (ic, ext) = (&c.ext_uri_ignore_path_case, &c.ext_uri);
    let file_uri = if c.is_windows() { c.file("c:\\foo\\bar") } else { c.file("/foo/bar") };
    let file_uri2 = if c.is_windows() { c.file("c:\\foo") } else { c.file("/foo") };
    let file_uri2b = if c.is_windows() { c.file("C:\\Foo\\") } else { c.file("/Foo/") };
    assert!(ic.is_equal_or_parent(&file_uri, &file_uri, false, false), "1");
    assert!(ext.is_equal_or_parent(&file_uri, &file_uri, false, false), "2");
    assert!(ic.is_equal_or_parent(&file_uri, &file_uri2, false, false), "3");
    assert!(ext.is_equal_or_parent(&file_uri, &file_uri2, false, false), "4");
    assert!(ic.is_equal_or_parent(&file_uri, &file_uri2b, false, false), "5");
    assert!(!ext.is_equal_or_parent(&file_uri, &file_uri2b, false, false), "6");

    assert!(!ext.is_equal_or_parent(&file_uri2, &file_uri, false, false), "7");
    assert!(ic.is_equal_or_parent(&file_uri2b, &file_uri2, false, false), "8");

    let file_uri3 = parse("foo://server:453/foo/bar/goo");
    let file_uri4 = parse("foo://server:453/foo/");
    let file_uri5 = parse("foo://server:453/foo");
    assert!(ic.is_equal_or_parent(&file_uri3, &file_uri3, true, false), "11");
    assert!(ext.is_equal_or_parent(&file_uri3, &file_uri3, false, false), "12");
    assert!(ic.is_equal_or_parent(&file_uri3, &file_uri4, true, false), "13");
    assert!(ext.is_equal_or_parent(&file_uri3, &file_uri4, false, false), "14");
    assert!(!ic.is_equal_or_parent(&file_uri3, &file_uri, true, false), "15");
    assert!(ic.is_equal_or_parent(&file_uri5, &file_uri5, true, false), "16");

    let file_uri6 = parse("foo://server:453/foo?q=1");
    let file_uri7 = parse("foo://server:453/foo/bar?q=1");
    assert!(!ic.is_equal_or_parent(&file_uri6, &file_uri5, false, false), "17");
    assert!(ic.is_equal_or_parent(&file_uri6, &file_uri6, false, false), "18");
    assert!(ic.is_equal_or_parent(&file_uri7, &file_uri6, false, false), "19");
    assert!(!ic.is_equal_or_parent(&file_uri7, &file_uri5, false, false), "20");
}

#[test]
fn is_equal_or_parent_posix() {
    is_equal_or_parent(POSIX);
}

#[test]
fn is_equal_or_parent_win32() {
    is_equal_or_parent(WIN32);
}

#[test]
fn is_equal_or_parent_handles_dot_dot_slash() {
    let c = Ctx::new(POSIX);
    let file_uri = c.file("/home/user/projects");
    let file_uri2 = c.file("/home/user/projects/../../../../tmp/folder");
    assert!(!c.ext_uri.is_equal_or_parent(&file_uri2, &file_uri, false, false));
}

fn is_equal_or_parent_handles_backslash_traversal(scheme: &str, ignore_case: bool) {
    let c = Ctx::new(POSIX);
    let util = c.util(Some(ignore_case));
    let root = from_components(scheme, if scheme == schemas::VSCODE_REMOTE { "host" } else { "" }, "/c:/lab/ws");
    let cases: Vec<(String, bool)> = vec![
        ("readme.md".into(), true),
        ("nested/file.txt".into(), true),
        ("nested\\file.txt".into(), true),
        ("nested/..\\readme.md".into(), true),
        ("nested\\..\\readme.md".into(), true),
        ("nested\\..".into(), true),
        ("../outside.txt".into(), false),
        ("..%2Foutside.txt".into(), false),
        ("..%5Coutside.txt".into(), false),
        ("..%5C..%5Coutside.txt".into(), false),
        ("..%5C..%5C..%5Coutside.txt".into(), false),
        ("..\\..\\..\\..\\outside.txt".into(), false),
        (format!("{}outside.txt", "..\\".repeat(5)), false),
        (format!("{}outside.txt", "..%5C".repeat(6)), false),
        ("..\\\\..\\\\outside.txt".into(), false),
        ("nested/..\\..\\outside.txt".into(), false),
        (format!("{}outside.txt", "..\\".repeat(12)), false),
        ("..%255Coutside.txt".into(), true),
        ("nested\\file.txt?query=1".into(), false),
        ("nested\\file.txt#fragment".into(), false),
    ];

    let actual: Vec<(String, bool)> = cases
        .iter()
        .map(|(path, _)| {
            (path.clone(), util.is_equal_or_parent(&parse(&format!("{}/{}", ts(&root), path)), &root, false, false))
        })
        .collect();
    assert_eq!(actual, cases);
}

#[test]
fn is_equal_or_parent_handles_backslash_traversal_vscode_userdata_ignore_case_false() {
    is_equal_or_parent_handles_backslash_traversal(schemas::VSCODE_USER_DATA, false);
}

#[test]
fn is_equal_or_parent_handles_backslash_traversal_vscode_userdata_ignore_case_true() {
    is_equal_or_parent_handles_backslash_traversal(schemas::VSCODE_USER_DATA, true);
}

#[test]
fn is_equal_or_parent_handles_backslash_traversal_vscode_remote_ignore_case_false() {
    is_equal_or_parent_handles_backslash_traversal(schemas::VSCODE_REMOTE, false);
}

#[test]
fn is_equal_or_parent_handles_backslash_traversal_vscode_remote_ignore_case_true() {
    is_equal_or_parent_handles_backslash_traversal(schemas::VSCODE_REMOTE, true);
}

#[test]
fn is_equal_or_parent_handles_backslash_traversal_with_backslashes_in_the_parent_path() {
    let c = Ctx::new(POSIX);
    let root = from_components(schemas::VSCODE_REMOTE, "host", "/c:/lab/ws\\nested");
    let paths = ["readme.md", "..\\outside.txt", "child\\..\\readme.md"];

    let mut actual: Vec<bool> =
        paths.iter().map(|path| c.ext_uri.is_equal_or_parent(&c.ext_uri.join_path(&root, &[path]), &root, false, false)).collect();
    actual.push(c.ext_uri.is_equal_or_parent(
        &with_path(&root, "/c:/lab/outside.txt"),
        &with_path(&root, format!("{}/..", root.path())),
        false,
        false,
    ));
    assert_eq!(actual, vec![true, false, true, false]);
}

fn is_equal_or_parent_handles_backslash_traversal_through_unc_roots(ignore_case: bool) {
    let c = Ctx::new(POSIX);
    let util = c.util(Some(ignore_case));
    let root = from_components(schemas::VSCODE_REMOTE, "host", r"/\server/share/ws");
    let cases: Vec<(String, bool)> = vec![
        (root.path().to_string_lossy(), true),
        (r"/\server/share/ws/readme.md".into(), true),
        (r"/\server/share/ws/nested\..\readme.md".into(), true),
        (r"/\SERVER/SHARE/WS/nested\..\readme.md".into(), ignore_case),
        (r"/\server/share/ws/..\..\..\server\share\ws\marker.txt".into(), false),
    ];

    let actual: Vec<(String, bool)> =
        cases.iter().map(|(path, _)| (path.clone(), util.is_equal_or_parent(&with_path(&root, path.as_str()), &root, false, false))).collect();
    assert_eq!(actual, cases);
}

#[test]
fn is_equal_or_parent_handles_backslash_traversal_through_unc_roots_ignore_case_false() {
    is_equal_or_parent_handles_backslash_traversal_through_unc_roots(false);
}

#[test]
fn is_equal_or_parent_handles_backslash_traversal_through_unc_roots_ignore_case_true() {
    is_equal_or_parent_handles_backslash_traversal_through_unc_roots(true);
}

fn is_equal_or_parent_uses_explicit_posix_semantics_for_non_file_uris(ignore_case: bool) {
    let c = Ctx::new(POSIX);
    let util = c.util(Some(ignore_case));
    let root = from_components(schemas::VSCODE_REMOTE, "host", "/workspace/project");
    let child = c.ext_uri.join_path(&root, &["..\\file.txt"]);
    let backslash_root = with_path(&root, "/workspace/parent\\..\\project");

    struct Case {
        base: URI,
        parent: Option<URI>,
        ignore_fragment: bool,
        expected: bool,
    }
    let case = |base: URI, parent: Option<URI>, ignore_fragment: bool, expected: bool| Case { base, parent, ignore_fragment, expected };

    let cases = vec![
        case(child.clone(), None, false, true),
        case(c.ext_uri.join_path(&root, &["nested/..\\..\\file.txt"]), None, false, true),
        case(c.ext_uri.join_path(&backslash_root, &["..\\file.txt"]), Some(backslash_root.clone()), false, true),
        case(with_path(&root, "/workspace/project/a\\b/../../outside.txt"), None, false, false),
        case(c.ext_uri.join_path(&root, &["../outside.txt"]), None, false, false),
        case(with_path(&root, "/workspace/project-other/..\\file.txt"), None, false, false),
        case(with_path(&child, child.path().to_upper_case()), None, false, ignore_case),
        case(with(&child, UriChange { scheme: Field::from(schemas::VSCODE_USER_DATA), ..UriChange::default() }), None, false, false),
        case(with(&child, UriChange { authority: "other".into(), ..UriChange::default() }), None, false, false),
        case(with(&child, UriChange { query: "version=1".into(), ..UriChange::default() }), None, false, false),
        case(with(&child, UriChange { fragment: "section".into(), ..UriChange::default() }), None, false, false),
        case(with(&child, UriChange { fragment: "section".into(), ..UriChange::default() }), None, true, true),
    ];

    let actual: Vec<bool> = cases
        .iter()
        .map(|case| util.is_equal_or_parent(&case.base, case.parent.as_ref().unwrap_or(&root), case.ignore_fragment, /* forcePosixSemantics */ true))
        .collect();
    let expected: Vec<bool> = cases.iter().map(|case| case.expected).collect();
    assert_eq!(actual, expected);
}

#[test]
fn is_equal_or_parent_uses_explicit_posix_semantics_for_non_file_uris_ignore_case_false() {
    is_equal_or_parent_uses_explicit_posix_semantics_for_non_file_uris(false);
}

#[test]
fn is_equal_or_parent_uses_explicit_posix_semantics_for_non_file_uris_ignore_case_true() {
    is_equal_or_parent_uses_explicit_posix_semantics_for_non_file_uris(true);
}
