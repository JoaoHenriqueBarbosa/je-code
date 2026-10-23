//! Porte de `src/vs/base/test/common/extpath.test.ts` (suíte `Paths`), caso a caso e na mesma ordem.
//!
//! O original escolhe o ramo por `isWindows`; aqui os testes com ramo de Windows rodam duas vezes, com a
//! plataforma injetada em `Posix` (o ramo que o original executa no Linux) e em `Win32` (sufixo `_win32`).
//! O caso `isUNC` é `test.skip` fora do Windows, então só existe a variante `_win32`.

use je_base::char_code as cc;
use je_base::extpath;
use je_base::path::{FixedEnvironment, Platform};

fn env(platform: Platform) -> FixedEnvironment {
    FixedEnvironment::new(platform, "/workspace/project")
}

fn is_windows(platform: Platform) -> bool {
    platform == Platform::Win32
}

#[test]
fn to_forward_slashes() {
    assert_eq!(extpath::to_slashes("\\\\server\\share\\some\\path"), "//server/share/some/path");
    assert_eq!(extpath::to_slashes("c:\\test"), "c:/test");
    assert_eq!(extpath::to_slashes("foo\\bar"), "foo/bar");
    assert_eq!(extpath::to_slashes("/user/far"), "/user/far");
}

#[test]
fn get_root() {
    let root = |path: &str| extpath::get_root(path, "/");
    assert_eq!(root("/user/far"), "/");
    assert_eq!(root("\\\\server\\share\\some\\path"), "//server/share/");
    assert_eq!(root("//server/share/some/path"), "//server/share/");
    assert_eq!(root("//server/share"), "/");
    assert_eq!(root("//server"), "/");
    assert_eq!(root("//server//"), "/");
    assert_eq!(root("c:/user/far"), "c:/");
    assert_eq!(root("c:user/far"), "c:");
    assert_eq!(root("http://www"), "");
    assert_eq!(root("http://www/"), "http://www/");
    assert_eq!(root("file:///foo"), "file:///");
    assert_eq!(root("file://foo"), "");
}

#[test]
fn is_unc_win32() {
    let e = env(Platform::Win32);
    assert!(!extpath::is_unc("foo", &e));
    assert!(!extpath::is_unc("/foo", &e));
    assert!(!extpath::is_unc("\\foo", &e));
    assert!(!extpath::is_unc("\\\\foo", &e));
    assert!(extpath::is_unc("\\\\a\\b", &e));
    assert!(!extpath::is_unc("//a/b", &e));
    assert!(extpath::is_unc("\\\\server\\share", &e));
    assert!(extpath::is_unc("\\\\server\\share\\", &e));
    assert!(extpath::is_unc("\\\\server\\share\\path", &e));
}

fn is_valid_basename(platform: Platform) {
    let win = is_windows(platform);
    let valid = |name: &str| extpath::is_valid_basename(Some(name), win);
    assert!(!extpath::is_valid_basename(None, win));
    assert!(!valid(""));
    assert!(valid("test.txt"));
    assert!(!valid("/test.txt"));

    if win {
        assert!(!valid("\\test.txt"));
        assert!(!valid("aux"));
        assert!(!valid("Aux"));
        assert!(!valid("LPT0"));
        assert!(!valid("aux.txt"));
        assert!(!valid("com0.abc"));
        assert!(valid("LPT00"));
        assert!(valid("aux1"));
        assert!(valid("aux1.txt"));
        assert!(valid("aux1.aux.txt"));

        assert!(!valid("test.txt."));
        assert!(!valid("test.txt.."));
        assert!(!valid("test.txt "));
        assert!(!valid("test.txt\t"));
        assert!(!valid("tes:t.txt"));
        assert!(!valid("tes\"t.txt"));
    } else {
        assert!(valid("\\test.txt"));
    }
}

#[test]
fn is_valid_basename_posix() {
    is_valid_basename(Platform::Posix);
}

#[test]
fn is_valid_basename_win32() {
    is_valid_basename(Platform::Win32);
}

fn sanitize_file_path(platform: Platform) {
    let e = env(platform);
    let sanitize = |candidate: &str, cwd: &str| extpath::sanitize_file_path(candidate, cwd, &e);
    if is_windows(platform) {
        assert_eq!(sanitize(".", "C:\\the\\cwd"), "C:\\the\\cwd");
        assert_eq!(sanitize("", "C:\\the\\cwd"), "C:\\the\\cwd");

        assert_eq!(sanitize("C:", "C:\\the\\cwd"), "C:\\");
        assert_eq!(sanitize("C:\\", "C:\\the\\cwd"), "C:\\");
        assert_eq!(sanitize("C:\\\\", "C:\\the\\cwd"), "C:\\");

        assert_eq!(sanitize("C:\\folder\\my.txt", "C:\\the\\cwd"), "C:\\folder\\my.txt");
        assert_eq!(sanitize("C:\\folder\\my", "C:\\the\\cwd"), "C:\\folder\\my");
        assert_eq!(sanitize("C:\\folder\\..\\my", "C:\\the\\cwd"), "C:\\my");
        assert_eq!(sanitize("C:\\folder\\my\\", "C:\\the\\cwd"), "C:\\folder\\my");
        assert_eq!(sanitize("C:\\folder\\my\\\\\\", "C:\\the\\cwd"), "C:\\folder\\my");

        assert_eq!(sanitize("my.txt", "C:\\the\\cwd"), "C:\\the\\cwd\\my.txt");
        assert_eq!(sanitize("my.txt\\", "C:\\the\\cwd"), "C:\\the\\cwd\\my.txt");

        assert_eq!(sanitize("\\\\localhost\\folder\\my", "C:\\the\\cwd"), "\\\\localhost\\folder\\my");
        assert_eq!(sanitize("\\\\localhost\\folder\\my\\", "C:\\the\\cwd"), "\\\\localhost\\folder\\my");
    } else {
        assert_eq!(sanitize(".", "/the/cwd"), "/the/cwd");
        assert_eq!(sanitize("", "/the/cwd"), "/the/cwd");
        assert_eq!(sanitize("/", "/the/cwd"), "/");

        assert_eq!(sanitize("/folder/my.txt", "/the/cwd"), "/folder/my.txt");
        assert_eq!(sanitize("/folder/my", "/the/cwd"), "/folder/my");
        assert_eq!(sanitize("/folder/../my", "/the/cwd"), "/my");
        assert_eq!(sanitize("/folder/my/", "/the/cwd"), "/folder/my");
        assert_eq!(sanitize("/folder/my///", "/the/cwd"), "/folder/my");

        assert_eq!(sanitize("my.txt", "/the/cwd"), "/the/cwd/my.txt");
        assert_eq!(sanitize("my.txt/", "/the/cwd"), "/the/cwd/my.txt");
    }
}

#[test]
fn sanitize_file_path_posix() {
    sanitize_file_path(Platform::Posix);
}

#[test]
fn sanitize_file_path_win32() {
    sanitize_file_path(Platform::Win32);
}

fn is_root_or_drive_letter(platform: Platform) {
    let e = env(platform);
    let check = |path: &str| extpath::is_root_or_drive_letter(path, &e);
    if is_windows(platform) {
        assert!(check("c:"));
        assert!(check("D:"));
        assert!(check("D:/"));
        assert!(check("D:\\"));
        assert!(!check("D:\\path"));
        assert!(!check("D:/path"));
    } else {
        assert!(check("/"));
        assert!(!check("/path"));
    }
}

#[test]
fn is_root_or_drive_letter_posix() {
    is_root_or_drive_letter(Platform::Posix);
}

#[test]
fn is_root_or_drive_letter_win32() {
    is_root_or_drive_letter(Platform::Win32);
}

fn has_drive_letter(platform: Platform) {
    let win = is_windows(platform);
    let check = |path: &str| extpath::has_drive_letter(path, win);
    if win {
        assert!(check("c:"));
        assert!(check("D:"));
        assert!(check("D:/"));
        assert!(check("D:\\"));
        assert!(check("D:\\path"));
        assert!(check("D:/path"));
    } else {
        assert!(!check("/"));
        assert!(!check("/path"));
    }
}

#[test]
fn has_drive_letter_posix() {
    has_drive_letter(Platform::Posix);
}

#[test]
fn has_drive_letter_win32() {
    has_drive_letter(Platform::Win32);
}

fn get_drive_letter(platform: Platform) {
    let win = is_windows(platform);
    let letter = |path: &str| extpath::get_drive_letter(path, win);
    if win {
        assert_eq!(letter("c:"), Some('c'));
        assert_eq!(letter("D:"), Some('D'));
        assert_eq!(letter("D:/"), Some('D'));
        assert_eq!(letter("D:\\"), Some('D'));
        assert_eq!(letter("D:\\path"), Some('D'));
        assert_eq!(letter("D:/path"), Some('D'));
    } else {
        assert!(letter("/").is_none());
        assert!(letter("/path").is_none());
    }
}

#[test]
fn get_drive_letter_posix() {
    get_drive_letter(Platform::Posix);
}

#[test]
fn get_drive_letter_win32() {
    get_drive_letter(Platform::Win32);
}

#[test]
fn is_windows_drive_letter() {
    assert!(!extpath::is_windows_drive_letter(0));
    assert!(!extpath::is_windows_drive_letter(-1));
    assert!(extpath::is_windows_drive_letter(i32::from(cc::A)));
    assert!(extpath::is_windows_drive_letter(i32::from(cc::z)));
}

#[test]
fn index_of_path() {
    assert_eq!(extpath::index_of_path("/foo", "/bar", true), None);
    assert_eq!(extpath::index_of_path("/foo", "/FOO", false), None);
    assert_eq!(extpath::index_of_path("/foo", "/FOO", true), Some(0));
    assert_eq!(extpath::index_of_path("/some/long/path", "/some/long", false), Some(0));
    assert_eq!(extpath::index_of_path("/some/long/path", "/PATH", true), Some(10));
}

#[test]
fn parse_line_and_column_aware() {
    let parse = extpath::parse_line_and_column_aware;

    let mut res = parse("/foo/bar");
    assert_eq!(res.path, "/foo/bar");
    assert_eq!(res.line, None);
    assert_eq!(res.column, None);

    res = parse("/foo/bar:33");
    assert_eq!(res.path, "/foo/bar");
    assert_eq!(res.line, Some(33.0));
    assert_eq!(res.column, Some(1.0));

    res = parse("/foo/bar:33:34");
    assert_eq!(res.path, "/foo/bar");
    assert_eq!(res.line, Some(33.0));
    assert_eq!(res.column, Some(34.0));

    res = parse("C:\\foo\\bar");
    assert_eq!(res.path, "C:\\foo\\bar");
    assert_eq!(res.line, None);
    assert_eq!(res.column, None);

    res = parse("C:\\foo\\bar:33");
    assert_eq!(res.path, "C:\\foo\\bar");
    assert_eq!(res.line, Some(33.0));
    assert_eq!(res.column, Some(1.0));

    res = parse("C:\\foo\\bar:33:34");
    assert_eq!(res.path, "C:\\foo\\bar");
    assert_eq!(res.line, Some(33.0));
    assert_eq!(res.column, Some(34.0));

    res = parse("/foo/bar:abb");
    assert_eq!(res.path, "/foo/bar:abb");
    assert_eq!(res.line, None);
    assert_eq!(res.column, None);
}

#[test]
fn random_path() {
    let e = env(Platform::Posix);
    let mut res = extpath::random_path(Some("/foo/bar"), None, 8, &e);
    assert!(!res.is_empty());

    res = extpath::random_path(Some("/foo/bar"), Some("prefix-"), 8, &e);
    // `assert.ok(res.indexOf('prefix-'))`: o `indexOf` só é falso na posição 0.
    assert!(res.find("prefix-") != Some(0));

    let r1 = extpath::random_path(Some("/foo/bar"), None, 8, &e);
    let r2 = extpath::random_path(Some("/foo/bar"), None, 8, &e);

    assert_ne!(r1, r2);

    let r3 = extpath::random_path(Some(""), Some(""), 3, &e);
    assert_eq!(r3.len(), 3);

    let r4 = extpath::random_path(None, None, 8, &e);
    assert!(!r4.is_empty());
}
