//! Porte de `src/vs/base/test/common/uri.test.ts` (suíte `URI`), caso a caso e na mesma ordem.
//!
//! Pendente: os dois casos `test.skip` do original ("Links in markdown are broken if url contains encoded
//! parameters #79474" e "Uri#parse can break path-component #45515"), que lá já estão desligados.
//! Pendente: no `URI.isUri` do caso #114971, os dois objetos com a forma de um URI (duck typing, com
//! `fsPath` como getter e como propriedade): em Rust só uma instância de `URI` conta como URI.
//! Pendente: nas asserções `new URL(fragment, base).href` de `URI#joinPath` (comparação com o
//! `URL` da web, que o porte não tem), e o `assert.throws(() => new URL('bazz', 'foo:'))`; o lado do
//! `URI` de cada caso está portado.
//!
//! O original escolhe o ramo por `isWindows`; aqui cada teste com ramo de Windows roda duas vezes, com a
//! plataforma injetada em `Posix` (o ramo que o original executa no Linux) e em `Win32` (sufixo `_win32`).

use je_base::errors;
use je_base::path::Platform;
use je_base::uri::{is_uri_components, Field, UriChange, UriComponents, UriState, URI};
use je_base::JsString;

const POSIX: Platform = Platform::Posix;
const WIN32: Platform = Platform::Win32;

fn parse(value: &str) -> URI {
    URI::parse(value, false)
}

fn file(path: &str, platform: Platform) -> URI {
    URI::file(path, platform)
}

/// `{ key: 'value', ... }` com as chaves `scheme`, `authority`, `path`, `query` e `fragment`.
fn comps(pairs: &[(&str, &str)]) -> UriComponents {
    let mut components = UriComponents::default();
    for &(key, value) in pairs {
        let value = Some(JsString::from(value));
        match key {
            "scheme" => components.scheme = value,
            "authority" => components.authority = value,
            "path" => components.path = value,
            "query" => components.query = value,
            "fragment" => components.fragment = value,
            other => panic!("chave desconhecida {other}"),
        }
    }
    components
}

fn from(pairs: &[(&str, &str)]) -> URI {
    URI::from(&comps(pairs), false)
}

/// O argumento de `with`; o valor `NULL` é o `null` do original.
const NULL: &str = "\u{0}null";

fn change(pairs: &[(&str, &str)]) -> UriChange {
    let mut change = UriChange::default();
    for &(key, value) in pairs {
        let field = if value == NULL { Field::Clear } else { Field::from(value) };
        match key {
            "scheme" => change.scheme = field,
            "authority" => change.authority = field,
            "path" => change.path = field,
            "query" => change.query = field,
            "fragment" => change.fragment = field,
            other => panic!("chave desconhecida {other}"),
        }
    }
    change
}

fn with(uri: &URI, pairs: &[(&str, &str)]) -> URI {
    uri.with(&change(pairs))
}

fn throws(f: impl FnOnce()) -> bool {
    errors::catch(f).is_err()
}

/// Texto com escapes `\uXXXX` literais, para surrogates isolados que um `&str` não representa.
fn lit(text: &str) -> JsString {
    let mut units: Vec<u16> = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'u') {
            chars.next();
            let hex: String = (0..4).filter_map(|_| chars.next()).collect();
            units.push(u16::from_str_radix(&hex, 16).expect("escape \\uXXXX"));
        } else {
            let mut buf = [0u16; 2];
            units.extend_from_slice(c.encode_utf16(&mut buf));
        }
    }
    JsString::from_units(units)
}

#[test]
fn file_to_string() {
    assert_eq!(file("c:/win/path", POSIX).to_string(false), "file:///c%3A/win/path");
    assert_eq!(file("C:/win/path", POSIX).to_string(false), "file:///c%3A/win/path");
    assert_eq!(file("c:/win/path/", POSIX).to_string(false), "file:///c%3A/win/path/");
    assert_eq!(file("/c:/win/path", POSIX).to_string(false), "file:///c%3A/win/path");
}

fn uri_file_win_special(platform: Platform) {
    if platform == WIN32 {
        assert_eq!(file("c:\\win\\path", platform).to_string(false), "file:///c%3A/win/path");
        assert_eq!(file("c:\\win/path", platform).to_string(false), "file:///c%3A/win/path");
    } else {
        assert_eq!(file("c:\\win\\path", platform).to_string(false), "file:///c%3A%5Cwin%5Cpath");
        assert_eq!(file("c:\\win/path", platform).to_string(false), "file:///c%3A%5Cwin/path");
    }
}

#[test]
fn uri_file_win_special_posix() {
    uri_file_win_special(POSIX);
}

#[test]
fn uri_file_win_special_win32() {
    uri_file_win_special(WIN32);
}

fn file_fs_path_win_special(platform: Platform) {
    if platform == WIN32 {
        assert_eq!(file("c:\\win\\path", platform).fs_path(platform), "c:\\win\\path");
        assert_eq!(file("c:\\win/path", platform).fs_path(platform), "c:\\win\\path");

        assert_eq!(file("c:/win/path", platform).fs_path(platform), "c:\\win\\path");
        assert_eq!(file("c:/win/path/", platform).fs_path(platform), "c:\\win\\path\\");
        assert_eq!(file("C:/win/path", platform).fs_path(platform), "c:\\win\\path");
        assert_eq!(file("/c:/win/path", platform).fs_path(platform), "c:\\win\\path");
        assert_eq!(file("./c/win/path", platform).fs_path(platform), "\\.\\c\\win\\path");
    } else {
        assert_eq!(file("c:/win/path", platform).fs_path(platform), "c:/win/path");
        assert_eq!(file("c:/win/path/", platform).fs_path(platform), "c:/win/path/");
        assert_eq!(file("C:/win/path", platform).fs_path(platform), "c:/win/path");
        assert_eq!(file("/c:/win/path", platform).fs_path(platform), "c:/win/path");
        assert_eq!(file("./c/win/path", platform).fs_path(platform), "/./c/win/path");
    }
}

#[test]
fn file_fs_path_win_special_posix() {
    file_fs_path_win_special(POSIX);
}

#[test]
fn file_fs_path_win_special_win32() {
    file_fs_path_win_special(WIN32);
}

fn uri_fs_path_no_fs_path_when_no_path(platform: Platform) {
    let value = parse("file://%2Fhome%2Fticino%2Fdesktop%2Fcpluscplus%2Ftest.cpp");
    assert_eq!(value.authority(), "/home/ticino/desktop/cpluscplus/test.cpp");
    assert_eq!(value.path(), "/");
    if platform == WIN32 {
        assert_eq!(value.fs_path(platform), "\\");
    } else {
        assert_eq!(value.fs_path(platform), "/");
    }
}

#[test]
fn uri_fs_path_no_fs_path_when_no_path_posix() {
    uri_fs_path_no_fs_path_when_no_path(POSIX);
}

#[test]
fn uri_fs_path_no_fs_path_when_no_path_win32() {
    uri_fs_path_no_fs_path_when_no_path(WIN32);
}

#[test]
fn http_to_string() {
    let http = |pairs: &[(&str, &str)]| {
        let mut all = vec![("scheme", "http")];
        all.extend_from_slice(pairs);
        from(&all).to_string(false)
    };
    assert_eq!(http(&[("authority", "www.example.com"), ("path", "/my/path")]), "http://www.example.com/my/path");
    assert_eq!(http(&[("authority", "www.example.com"), ("path", "/my/path")]), "http://www.example.com/my/path");
    assert_eq!(http(&[("authority", "www.EXAMPLE.com"), ("path", "/my/path")]), "http://www.example.com/my/path");
    assert_eq!(http(&[("authority", ""), ("path", "my/path")]), "http:/my/path");
    assert_eq!(http(&[("authority", ""), ("path", "/my/path")]), "http:/my/path");
    //http://example.com/#test=true
    assert_eq!(http(&[("authority", "example.com"), ("path", "/"), ("query", "test=true")]), "http://example.com/?test%3Dtrue");
    assert_eq!(
        http(&[("authority", "example.com"), ("path", "/"), ("query", ""), ("fragment", "test=true")]),
        "http://example.com/#test%3Dtrue"
    );
}

#[test]
fn http_to_string_encode_false() {
    assert_eq!(
        from(&[("scheme", "http"), ("authority", "example.com"), ("path", "/"), ("query", "test=true")]).to_string(true),
        "http://example.com/?test=true"
    );
    assert_eq!(
        from(&[("scheme", "http"), ("authority", "example.com"), ("path", "/"), ("query", ""), ("fragment", "test=true")]).to_string(true),
        "http://example.com/#test=true"
    );
    assert_eq!(
        from(&[("scheme", "http"), ("path", "/api/files/test.me"), ("query", "t=1234")]).to_string(true),
        "http:/api/files/test.me?t=1234"
    );

    let value = parse("file://shares/pröjects/c%23/#l12");
    assert_eq!(value.authority(), "shares");
    assert_eq!(value.path(), "/pröjects/c#/");
    assert_eq!(value.fragment(), "l12");
    assert_eq!(value.to_string(false), "file://shares/pr%C3%B6jects/c%23/#l12");
    assert_eq!(value.to_string(true), "file://shares/pröjects/c%23/#l12");

    let uri2 = parse(&value.to_string(true).to_string_lossy());
    let uri3 = parse(&value.to_string(false).to_string_lossy());
    assert_eq!(uri2.authority(), uri3.authority());
    assert_eq!(uri2.path(), uri3.path());
    assert_eq!(uri2.query(), uri3.query());
    assert_eq!(uri2.fragment(), uri3.fragment());
}

#[test]
fn to_string_minimal_encoding_preserves_delimiter_boundaries_and_unchanged_runs() {
    let suffix = "segment/".repeat(32);
    let prefixed = format!("prefix?{suffix}#tail");
    let prefixed_expected = format!("prefix%3F{suffix}%23tail");
    let cases: Vec<(JsString, JsString)> = vec![
        (lit(""), lit("")),
        (lit("a"), lit("a")),
        (lit("plain/path"), lit("plain/path")),
        (lit("?"), lit("%3F")),
        (lit("#"), lit("%23")),
        (lit("?#"), lit("%3F%23")),
        (lit("#?"), lit("%23%3F")),
        (lit("??##"), lit("%3F%3F%23%23")),
        (lit("?tail"), lit("%3Ftail")),
        (lit("#tail"), lit("%23tail")),
        (lit("head?"), lit("head%3F")),
        (lit("head#"), lit("head%23")),
        (lit("before?between#after"), lit("before%3Fbetween%23after")),
        (lit("before#between?after"), lit("before%23between%3Fafter")),
        (lit("before??##after"), lit("before%3F%3F%23%23after")),
        (lit("%23/%3F?raw#"), lit("%23/%3F%3Fraw%23")),
        (lit("\\u00e9/\\ud83d\\udc31?\\u7d42#"), lit("\\u00e9/\\ud83d\\udc31%3F\\u7d42%23")),
        (lit("\\ud800x?\\udc00y#\\ud800"), lit("\\ud800x%3F\\udc00y%23\\ud800")),
        (lit(&prefixed), lit(&prefixed_expected)),
    ];

    let actual: Vec<(JsString, JsString)> = cases
        .iter()
        .map(|(path, _)| {
            let uri = URI::from(
                &UriComponents { scheme: Some("test".into()), path: Some(path.clone()), ..UriComponents::default() },
                false,
            );
            (path.clone(), uri.to_string(true))
        })
        .collect();
    let expected: Vec<(JsString, JsString)> = cases
        .iter()
        .map(|(path, expected)| {
            let mut full = JsString::from("test:");
            full.push_units(expected.units());
            (path.clone(), full)
        })
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn to_string_minimal_component_encoding_is_independent_of_normal_serialization_caching() {
    let components = [
        ("scheme", "foo"),
        ("authority", "us?er:pa#ss@EXAMPLE.com:8080"),
        ("path", "/path#before?after"),
        ("query", "query#part?more"),
        ("fragment", "fragment?#"),
    ];
    let minimal_first = from(&components);
    let normal_first = from(&components);
    let minimal = "foo://us%3Fer:pa%23ss@example.com:8080/path%23before%3Fafter?query%23part%3Fmore#fragment?#";
    let normal = "foo://us%3Fer:pa%23ss@example.com:8080/path%23before%3Fafter?query%23part%3Fmore#fragment%3F%23";

    let first = [minimal_first.to_string(true), minimal_first.to_string(false), minimal_first.to_string(true)];
    let second = [normal_first.to_string(false), normal_first.to_string(true), normal_first.to_string(false)];
    assert_eq!(first, [minimal, normal, minimal].map(JsString::from));
    assert_eq!(second, [normal, minimal, normal].map(JsString::from));
}

#[test]
fn with_identity() {
    let uri = parse("foo:bar/path");

    // `with(null)`, `with(undefined)` e `with({})` do original são o `UriChange` vazio.
    let mut uri2 = uri.with(&UriChange::default());
    assert!(URI::ptr_eq(&uri, &uri2));
    uri2 = with(&uri, &[]);
    assert!(URI::ptr_eq(&uri, &uri2));
    uri2 = with(&uri, &[("scheme", "foo"), ("path", "bar/path")]);
    assert!(URI::ptr_eq(&uri, &uri2));
}

#[test]
fn with_changes() {
    assert_eq!(with(&parse("before:some/file/path"), &[("scheme", "after")]).to_string(false), "after:some/file/path");
    let s = from(&[("scheme", "s")]);
    assert_eq!(
        with(&s, &[("scheme", "http"), ("path", "/api/files/test.me"), ("query", "t=1234")]).to_string(false),
        "http:/api/files/test.me?t%3D1234"
    );
    for (scheme, expected) in [
        ("http", "http:/api/files/test.me?t%3D1234"),
        ("https", "https:/api/files/test.me?t%3D1234"),
        ("HTTP", "HTTP:/api/files/test.me?t%3D1234"),
        ("HTTPS", "HTTPS:/api/files/test.me?t%3D1234"),
        ("boo", "boo:/api/files/test.me?t%3D1234"),
    ] {
        assert_eq!(
            with(&s, &[("scheme", scheme), ("authority", ""), ("path", "/api/files/test.me"), ("query", "t=1234"), ("fragment", "")])
                .to_string(false),
            expected
        );
    }
}

#[test]
fn with_remove_components_8465() {
    assert_eq!(with(&parse("scheme://authority/path"), &[("authority", "")]).to_string(false), "scheme:/path");
    assert_eq!(
        with(&with(&parse("scheme:/path"), &[("authority", "authority")]), &[("authority", "")]).to_string(false),
        "scheme:/path"
    );
    assert_eq!(
        with(&with(&parse("scheme:/path"), &[("authority", "authority")]), &[("authority", NULL)]).to_string(false),
        "scheme:/path"
    );
    assert_eq!(
        with(&with(&parse("scheme:/path"), &[("authority", "authority")]), &[("path", "")]).to_string(false),
        "scheme://authority"
    );
    assert_eq!(
        with(&with(&parse("scheme:/path"), &[("authority", "authority")]), &[("path", NULL)]).to_string(false),
        "scheme://authority"
    );
    assert_eq!(with(&parse("scheme:/path"), &[("authority", "")]).to_string(false), "scheme:/path");
    assert_eq!(with(&parse("scheme:/path"), &[("authority", NULL)]).to_string(false), "scheme:/path");
}

#[test]
fn with_validation() {
    let uri = parse("foo:bar/path");
    assert!(throws(|| drop(with(&uri, &[("scheme", "fai:l")]))));
    assert!(throws(|| drop(with(&uri, &[("scheme", "fäil")]))));
    assert!(throws(|| drop(with(&uri, &[("authority", "fail")]))));
    assert!(throws(|| drop(with(&uri, &[("path", "//fail")]))));
}

fn parse_case(platform: Platform) {
    let win = platform == WIN32;
    let check = |value: &URI, scheme: &str, authority: &str, path: &str, query: &str, fragment: &str| {
        assert_eq!(value.scheme(), scheme);
        assert_eq!(value.authority(), authority);
        assert_eq!(value.path(), path);
        assert_eq!(value.query(), query);
        assert_eq!(value.fragment(), fragment);
    };

    let mut value = parse("http:/api/files/test.me?t=1234");
    check(&value, "http", "", "/api/files/test.me", "t=1234", "");

    value = parse("http://api/files/test.me?t=1234");
    check(&value, "http", "api", "/files/test.me", "t=1234", "");

    value = parse("file:///c:/test/me");
    check(&value, "file", "", "/c:/test/me", "", "");
    assert_eq!(value.fs_path(platform), if win { "c:\\test\\me" } else { "c:/test/me" });

    value = parse("file://shares/files/c%23/p.cs");
    check(&value, "file", "shares", "/files/c#/p.cs", "", "");
    assert_eq!(value.fs_path(platform), if win { "\\\\shares\\files\\c#\\p.cs" } else { "//shares/files/c#/p.cs" });

    value = parse("file:///c:/Source/Z%C3%BCrich%20or%20Zurich%20(%CB%88zj%CA%8A%C9%99r%C9%AAk,/Code/resources/app/plugins/c%23/plugin.json");
    check(&value, "file", "", "/c:/Source/Zürich or Zurich (ˈzjʊərɪk,/Code/resources/app/plugins/c#/plugin.json", "", "");

    value = parse("file:///c:/test %25/path");
    check(&value, "file", "", "/c:/test %/path", "", "");

    value = parse("inmemory:");
    check(&value, "inmemory", "", "", "", "");

    value = parse("foo:api/files/test");
    check(&value, "foo", "", "api/files/test", "", "");

    value = parse("file:?q");
    check(&value, "file", "", "/", "q", "");

    value = parse("file:#d");
    check(&value, "file", "", "/", "", "d");

    value = parse("f3ile:#d");
    check(&value, "f3ile", "", "", "", "d");

    value = parse("foo+bar:path");
    check(&value, "foo+bar", "", "path", "", "");

    value = parse("foo-bar:path");
    check(&value, "foo-bar", "", "path", "", "");

    value = parse("foo.bar:path");
    check(&value, "foo.bar", "", "path", "", "");
}

#[test]
fn parse_posix() {
    parse_case(POSIX);
}

#[test]
fn parse_win32() {
    parse_case(WIN32);
}

#[test]
fn parse_disallow_double_slash_path_when_no_authority() {
    assert!(throws(|| drop(parse("file:////shares/files/p.cs"))));
}

fn uri_file_win_speciale(platform: Platform) {
    if platform != WIN32 {
        return;
    }
    let mut value = file("c:\\test\\drive", platform);
    assert_eq!(value.path(), "/c:/test/drive");
    assert_eq!(value.to_string(false), "file:///c%3A/test/drive");

    value = file("\\\\shäres\\path\\c#\\plugin.json", platform);
    assert_eq!(value.scheme(), "file");
    assert_eq!(value.authority(), "shäres");
    assert_eq!(value.path(), "/path/c#/plugin.json");
    assert_eq!(value.fragment(), "");
    assert_eq!(value.query(), "");
    assert_eq!(value.to_string(false), "file://sh%C3%A4res/path/c%23/plugin.json");

    value = file("\\\\localhost\\c$\\GitDevelopment\\express", platform);
    assert_eq!(value.scheme(), "file");
    assert_eq!(value.path(), "/c$/GitDevelopment/express");
    assert_eq!(value.fs_path(platform), "\\\\localhost\\c$\\GitDevelopment\\express");
    assert_eq!(value.query(), "");
    assert_eq!(value.fragment(), "");
    assert_eq!(value.to_string(false), "file://localhost/c%24/GitDevelopment/express");

    value = file("c:\\test with %\\path", platform);
    assert_eq!(value.path(), "/c:/test with %/path");
    assert_eq!(value.to_string(false), "file:///c%3A/test%20with%20%25/path");

    value = file("c:\\test with %25\\path", platform);
    assert_eq!(value.path(), "/c:/test with %25/path");
    assert_eq!(value.to_string(false), "file:///c%3A/test%20with%20%2525/path");

    value = file("c:\\test with %25\\c#code", platform);
    assert_eq!(value.path(), "/c:/test with %25/c#code");
    assert_eq!(value.to_string(false), "file:///c%3A/test%20with%20%2525/c%23code");

    value = file("\\\\shares", platform);
    assert_eq!(value.scheme(), "file");
    assert_eq!(value.authority(), "shares");
    assert_eq!(value.path(), "/"); // slash is always there

    value = file("\\\\shares\\", platform);
    assert_eq!(value.scheme(), "file");
    assert_eq!(value.authority(), "shares");
    assert_eq!(value.path(), "/");
}

#[test]
fn uri_file_win_speciale_posix() {
    uri_file_win_speciale(POSIX);
}

#[test]
fn uri_file_win_speciale_win32() {
    uri_file_win_speciale(WIN32);
}

fn vscode_uri_module_drive_letter_path_regex_is_incorrect_32961(platform: Platform) {
    let uri = parse("file:///_:/path");
    assert_eq!(uri.fs_path(platform), if platform == WIN32 { "\\_:\\path" } else { "/_:/path" });
}

#[test]
fn vscode_uri_module_drive_letter_path_regex_is_incorrect_32961_posix() {
    vscode_uri_module_drive_letter_path_regex_is_incorrect_32961(POSIX);
}

#[test]
fn vscode_uri_module_drive_letter_path_regex_is_incorrect_32961_win32() {
    vscode_uri_module_drive_letter_path_regex_is_incorrect_32961(WIN32);
}

#[test]
fn uri_file_no_path_is_uri_check() {
    // we don't complain here
    let value = file("file://path/to/file", POSIX);
    assert_eq!(value.scheme(), "file");
    assert_eq!(value.authority(), "");
    assert_eq!(value.path(), "/file://path/to/file");
}

#[test]
fn uri_file_always_slash() {
    let mut value = file("a.file", POSIX);
    assert_eq!(value.scheme(), "file");
    assert_eq!(value.authority(), "");
    assert_eq!(value.path(), "/a.file");
    assert_eq!(value.to_string(false), "file:///a.file");

    value = parse(&value.to_string(false).to_string_lossy());
    assert_eq!(value.scheme(), "file");
    assert_eq!(value.authority(), "");
    assert_eq!(value.path(), "/a.file");
    assert_eq!(value.to_string(false), "file:///a.file");
}

#[test]
fn uri_to_string_only_scheme_and_query() {
    let value = parse("stuff:?qüery");
    assert_eq!(value.to_string(false), "stuff:?q%C3%BCery");
}

#[test]
fn uri_to_string_upper_case_percent_espaces() {
    let value = parse("file://sh%c3%a4res/path");
    assert_eq!(value.to_string(false), "file://sh%C3%A4res/path");
}

#[test]
fn uri_to_string_lower_case_windows_drive_letter() {
    assert_eq!(parse("untitled:c:/Users/jrieken/Code/abc.txt").to_string(false), "untitled:c%3A/Users/jrieken/Code/abc.txt");
    assert_eq!(parse("untitled:C:/Users/jrieken/Code/abc.txt").to_string(false), "untitled:c%3A/Users/jrieken/Code/abc.txt");
}

#[test]
fn uri_to_string_preserves_drive_like_http_path_case() {
    let schemes = ["http", "https", "HtTp", "HTTPS"];
    let actual: Vec<[JsString; 3]> = schemes
        .iter()
        .map(|scheme| {
            let uri = parse(&format!("{scheme}://example.test/C:/Secret"));
            [uri.to_string(false), uri.to_string(true), parse(&uri.to_string(false).to_string_lossy()).to_string(true)]
        })
        .collect();
    let expected: Vec<[JsString; 3]> = schemes
        .iter()
        .map(|scheme| {
            [
                format!("{scheme}://example.test/C%3A/Secret").into(),
                format!("{scheme}://example.test/C:/Secret").into(),
                format!("{scheme}://example.test/C:/Secret").into(),
            ]
        })
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn uri_to_string_retains_non_http_drive_letter_normalization() {
    let schemes = ["file", "untitled", "vscode-remote", "custom"];
    let actual: Vec<JsString> = schemes.iter().map(|scheme| parse(&format!("{scheme}://server/C:/Secret")).to_string(true)).collect();
    let expected: Vec<JsString> = schemes.iter().map(|scheme| format!("{scheme}://server/c:/Secret").into()).collect();
    assert_eq!(actual, expected);
}

#[test]
fn uri_to_string_escape_all_the_bits() {
    let value = file("/Users/jrieken/Code/_samples/18500/Mödel + Other Thîngß/model.js", POSIX);
    assert_eq!(
        value.to_string(false),
        "file:///Users/jrieken/Code/_samples/18500/M%C3%B6del%20%2B%20Other%20Th%C3%AEng%C3%9F/model.js"
    );
}

#[test]
fn uri_to_string_dont_encode_port() {
    let mut value = parse("http://localhost:8080/far");
    assert_eq!(value.to_string(false), "http://localhost:8080/far");

    value = from(&[("scheme", "http"), ("authority", "löcalhost:8080"), ("path", "/far")]);
    assert_eq!(value.to_string(false), "http://l%C3%B6calhost:8080/far");
}

#[test]
fn uri_to_string_user_information_in_authority() {
    let mut value = parse("http://foo:bar@localhost/far");
    assert_eq!(value.to_string(false), "http://foo:bar@localhost/far");

    value = parse("http://foo@localhost/far");
    assert_eq!(value.to_string(false), "http://foo@localhost/far");

    value = parse("http://foo:bAr@localhost:8080/far");
    assert_eq!(value.to_string(false), "http://foo:bAr@localhost:8080/far");

    value = parse("http://foo@localhost:8080/far");
    assert_eq!(value.to_string(false), "http://foo@localhost:8080/far");

    value = from(&[("scheme", "http"), ("authority", "föö:bör@löcalhost:8080"), ("path", "/far")]);
    assert_eq!(value.to_string(false), "http://f%C3%B6%C3%B6:b%C3%B6r@l%C3%B6calhost:8080/far");
}

#[test]
fn uri_to_string_colon_in_password_splits_userinfo_at_the_first_colon() {
    let value = parse("http://foo:pa:ss@localhost/far");
    assert_eq!(value.to_string(false), "http://foo:pa%3Ass@localhost/far");
    assert_eq!(parse(&value.to_string(false).to_string_lossy()).authority(), "foo:pa:ss@localhost");
}

fn correct_file_uri_to_file_path2(platform: Platform) {
    let win = platform == WIN32;
    let test = |input: &str, expected: &str| {
        let value = parse(input);
        assert_eq!(value.fs_path(platform), expected, "Result for {input}");
        let value2 = file(&value.fs_path(platform).to_string_lossy(), platform);
        assert_eq!(value2.fs_path(platform), expected, "Result for {input}");
        assert_eq!(value.to_string(false), value2.to_string(false));
    };

    test("file:///c:/alex.txt", if win { "c:\\alex.txt" } else { "c:/alex.txt" });
    test(
        "file:///c:/Source/Z%C3%BCrich%20or%20Zurich%20(%CB%88zj%CA%8A%C9%99r%C9%AAk,/Code/resources/app/plugins",
        if win {
            "c:\\Source\\Zürich or Zurich (ˈzjʊərɪk,\\Code\\resources\\app\\plugins"
        } else {
            "c:/Source/Zürich or Zurich (ˈzjʊərɪk,/Code/resources/app/plugins"
        },
    );
    test("file://monacotools/folder/isi.txt", if win { "\\\\monacotools\\folder\\isi.txt" } else { "//monacotools/folder/isi.txt" });
    test(
        "file://monacotools1/certificates/SSL/",
        if win { "\\\\monacotools1\\certificates\\SSL\\" } else { "//monacotools1/certificates/SSL/" },
    );
}

#[test]
fn correct_file_uri_to_file_path2_posix() {
    correct_file_uri_to_file_path2(POSIX);
}

#[test]
fn correct_file_uri_to_file_path2_win32() {
    correct_file_uri_to_file_path2(WIN32);
}

#[test]
fn uri_http_query_and_to_string() {
    let mut uri = parse("https://go.microsoft.com/fwlink/?LinkId=518008");
    assert_eq!(uri.query(), "LinkId=518008");
    assert_eq!(uri.to_string(true), "https://go.microsoft.com/fwlink/?LinkId=518008");
    assert_eq!(uri.to_string(false), "https://go.microsoft.com/fwlink/?LinkId%3D518008");

    let mut uri2 = parse(&uri.to_string(false).to_string_lossy());
    assert_eq!(uri2.query(), "LinkId=518008");
    assert_eq!(uri2.query(), uri.query());

    uri = parse("https://go.microsoft.com/fwlink/?LinkId=518008&foö&ké¥=üü");
    assert_eq!(uri.query(), "LinkId=518008&foö&ké¥=üü");
    assert_eq!(uri.to_string(true), "https://go.microsoft.com/fwlink/?LinkId=518008&foö&ké¥=üü");
    assert_eq!(uri.to_string(false), "https://go.microsoft.com/fwlink/?LinkId%3D518008%26fo%C3%B6%26k%C3%A9%C2%A5%3D%C3%BC%C3%BC");

    uri2 = parse(&uri.to_string(false).to_string_lossy());
    assert_eq!(uri2.query(), "LinkId=518008&foö&ké¥=üü");
    assert_eq!(uri2.query(), uri.query());

    // #24849
    uri = parse("https://twitter.com/search?src=typd&q=%23tag");
    assert_eq!(uri.to_string(true), "https://twitter.com/search?src=typd&q=%23tag");
}

#[test]
fn class_uri_cannot_represent_relative_file_paths_34449() {
    let mut path = "/foo/bar";
    assert_eq!(file(path, POSIX).path(), path);
    path = "foo/bar";
    assert_eq!(file(path, POSIX).path(), "/foo/bar");
    path = "./foo/bar";
    assert_eq!(file(path, POSIX).path(), "/./foo/bar"); // missing normalization

    let file_uri1 = parse("file:foo/bar");
    assert_eq!(file_uri1.path(), "/foo/bar");
    assert_eq!(file_uri1.authority(), "");
    let uri = file_uri1.to_string(false);
    assert_eq!(uri, "file:///foo/bar");
    let file_uri2 = parse(&uri.to_string_lossy());
    assert_eq!(file_uri2.path(), "/foo/bar");
    assert_eq!(file_uri2.authority(), "");
}

#[test]
fn ctrl_click_to_follow_hash_query_param_url_gets_urlencoded_49628() {
    let mut input = "http://localhost:3000/#/foo?bar=baz";
    let mut uri = parse(input);
    assert_eq!(uri.to_string(true), input);

    input = "http://localhost:3000/foo?bar=baz";
    uri = parse(input);
    assert_eq!(uri.to_string(true), input);
}

#[test]
fn unable_to_open_a0_txt_uri_malformed_76506() {
    let mut uri = file("/foo/%A0.txt", POSIX);
    let mut uri2 = parse(&uri.to_string(false).to_string_lossy());
    assert_eq!(uri.scheme(), uri2.scheme());
    assert_eq!(uri.path(), uri2.path());

    uri = file("/foo/%2e.txt", POSIX);
    uri2 = parse(&uri.to_string(false).to_string_lossy());
    assert_eq!(uri.scheme(), uri2.scheme());
    assert_eq!(uri.path(), uri2.path());
}

#[test]
fn bug_in_uri_is_uri_that_fails_thing_type_comparison_114971() {
    let uri = file("/foo/bazz.txt", POSIX);
    assert!(URI::is_uri(&uri));
    assert!(!URI::is_uri(&uri.to_json()));

    assert!(!URI::is_uri(&1));
    assert!(!URI::is_uri(&"1"));
    assert!(!URI::is_uri(&"http://sample.com"));
    assert!(!URI::is_uri(&Option::<URI>::None)); // null
    assert!(!URI::is_uri(&())); // undefined
}

#[test]
fn is_uri_components_case() {
    assert!(is_uri_components(&file("a", POSIX)));
    assert!(is_uri_components(&file("a", POSIX).to_json()));
    assert!(is_uri_components(&file("", POSIX)));
    assert!(is_uri_components(&file("", POSIX).to_json()));

    assert!(!is_uri_components(&1));
    assert!(!is_uri_components(&true));
    assert!(!is_uri_components(&"true"));
    assert!(!is_uri_components(&UriComponents::default())); // {}
    assert!(is_uri_components(&comps(&[("scheme", "")]))); // valid components but INVALID uri
    assert!(is_uri_components(&comps(&[("scheme", "fo")])));
    assert!(is_uri_components(&comps(&[("scheme", "fo"), ("path", "/p")])));
    assert!(!is_uri_components(&comps(&[("path", "/p")]).tap_clear_scheme()));
}

/// `{ path: '/p' }`: componentes sem `scheme`.
trait ClearScheme {
    fn tap_clear_scheme(self) -> Self;
}

impl ClearScheme for UriComponents {
    fn tap_clear_scheme(mut self) -> Self {
        self.scheme = None;
        self
    }
}

#[test]
fn from_from_strict_revive() {
    assert!(throws(|| drop(URI::from(&comps(&[("scheme", "")]), true))));
    assert_eq!(URI::from(&comps(&[("scheme", "")]), false).scheme(), "file");
    assert_eq!(URI::revive(&UriState::from(comps(&[("scheme", "")])), POSIX).scheme(), "");
}

#[test]
fn unable_to_open_a0_txt_uri_malformed_76506_part_2() {
    assert_eq!(parse("file://some/%.txt").to_string(false), "file://some/%25.txt");
    assert_eq!(parse("file://some/%A0.txt").to_string(false), "file://some/%25A0.txt");
}

fn uri_de_serialize(platform: Platform) {
    let values = [
        parse("http://localhost:8080/far"),
        file("c:\\test with %25\\c#code", platform),
        file("\\\\shäres\\path\\c#\\plugin.json", platform),
        parse("http://api/files/test.me?t=1234"),
        parse("http://api/files/test.me?t=1234#fff"),
        parse("http://api/files/test.me#fff"),
    ];

    for value in &values {
        let data = value.to_json();
        let clone = URI::revive(&data, platform);

        assert_eq!(clone.scheme(), value.scheme());
        assert_eq!(clone.authority(), value.authority());
        assert_eq!(clone.path(), value.path());
        assert_eq!(clone.query(), value.query());
        assert_eq!(clone.fragment(), value.fragment());
        assert_eq!(clone.fs_path(platform), value.fs_path(platform));
        assert_eq!(clone.to_string(false), value.to_string(false));
    }
}

#[test]
fn uri_de_serialize_posix() {
    uri_de_serialize(POSIX);
}

#[test]
fn uri_de_serialize_win32() {
    uri_de_serialize(WIN32);
}

fn assert_joined(base: &str, fragment: &str, expected: &str, platform: Platform) {
    let base_uri = parse(base);
    let new_uri = URI::join_path(&base_uri, &[fragment], platform);
    let actual = new_uri.to_string(true);
    assert_eq!(actual, expected);
}

#[test]
fn uri_join_path() {
    let p = POSIX;
    assert_joined("file:///foo/", "../../bazz", "file:///bazz", p);
    assert_joined("file:///foo", "../../bazz", "file:///bazz", p);
    assert_joined("file:///foo", "../../bazz", "file:///bazz", p);
    assert_joined("file:///foo/bar/", "./bazz", "file:///foo/bar/bazz", p);
    assert_joined("file:///foo/bar", "./bazz", "file:///foo/bar/bazz", p);
    assert_joined("file:///foo/bar", "bazz", "file:///foo/bar/bazz", p);

    // "auto-path" scheme
    assert_joined("file:", "bazz", "file:///bazz", p);
    assert_joined("http://domain", "bazz", "http://domain/bazz", p);
    assert_joined("https://domain", "bazz", "https://domain/bazz", p);
    assert_joined("http:", "bazz", "http:/bazz", p);
    assert_joined("https:", "bazz", "https:/bazz", p);

    // no "auto-path" scheme with and w/o paths
    assert_joined("foo:/", "bazz", "foo:/bazz", p);
    assert_joined("foo://bar/", "bazz", "foo://bar/bazz", p);

    // no "auto-path" + no path -> error
    assert!(throws(|| assert_joined("foo:", "bazz", "", p)));
    assert!(throws(|| assert_joined("foo://bar", "bazz", "", p)));
}

#[test]
fn uri_join_path_posix() {
    let p = POSIX;
    assert_joined("file:///c:/foo/", "../../bazz", "file:///bazz", p);
    assert_joined("file://server/share/c:/", "../../bazz", "file://server/bazz", p);
    assert_joined("file://server/share/c:", "../../bazz", "file://server/bazz", p);

    assert_joined("file://ser/foo/", "../../bazz", "file://ser/bazz", p); // Firefox -> Different, Edge, Chrome, Safar -> OK
    assert_joined("file://ser/foo", "../../bazz", "file://ser/bazz", p); // Firefox -> Different, Edge, Chrome, Safar -> OK
}

#[test]
fn uri_join_path_windows() {
    let p = WIN32;
    assert_joined("file:///c:/foo/", "../../bazz", "file:///c:/bazz", p);
    assert_joined("file://server/share/c:/", "../../bazz", "file://server/share/bazz", p);
    assert_joined("file://server/share/c:", "../../bazz", "file://server/share/bazz", p);

    assert_joined("file://ser/foo/", "../../bazz", "file://ser/foo/bazz", p);
    assert_joined("file://ser/foo", "../../bazz", "file://ser/foo/bazz", p);

    //https://github.com/microsoft/vscode/issues/93831
    assert_joined("file:///c:/foo/bar", "./other/foo.img", "file:///c:/foo/bar/other/foo.img", p);
}

#[test]
fn vscode_uri_to_string_wrongly_encode_ipv6_literals_154048() {
    assert_eq!(
        parse("http://[FEDC:BA98:7654:3210:FEDC:BA98:7654:3210]:80/index.html").to_string(false),
        "http://[fedc:ba98:7654:3210:fedc:ba98:7654:3210]:80/index.html"
    );

    assert_eq!(
        parse("http://user@[FEDC:BA98:7654:3210:FEDC:BA98:7654:3210]:80/index.html").to_string(false),
        "http://user@[fedc:ba98:7654:3210:fedc:ba98:7654:3210]:80/index.html"
    );
    assert_eq!(
        parse("http://us[er@[FEDC:BA98:7654:3210:FEDC:BA98:7654:3210]:80/index.html").to_string(false),
        "http://us%5Ber@[fedc:ba98:7654:3210:fedc:ba98:7654:3210]:80/index.html"
    );
}

fn file_paths_containing_apostrophes_276075(platform: Platform) {
    if platform != WIN32 {
        return;
    }
    let file_path = "C:\\Users\\Abd-al-Haseeb's_Dell\\Studio\\w3mage\\wp-content\\database.ht.sqlite";
    let uri = file(file_path, platform);
    assert_eq!(uri.path(), "/C:/Users/Abd-al-Haseeb's_Dell/Studio/w3mage/wp-content/database.ht.sqlite");
    assert_eq!(uri.fs_path(platform), "c:\\Users\\Abd-al-Haseeb's_Dell\\Studio\\w3mage\\wp-content\\database.ht.sqlite");
}

#[test]
fn file_paths_containing_apostrophes_break_uri_parsing_and_cannot_be_opened_276075_posix() {
    file_paths_containing_apostrophes_276075(POSIX);
}

#[test]
fn file_paths_containing_apostrophes_break_uri_parsing_and_cannot_be_opened_276075_win32() {
    file_paths_containing_apostrophes_276075(WIN32);
}
