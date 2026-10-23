//! Porte de `src/vs/base/test/common/glob.test.ts` (suíte `Glob`), caso a caso e na mesma ordem.
//!
//! O original lê `isWindows`, `isMacintosh`, `isLinux` e `sep` do ambiente. Aqui a plataforma é
//! injetada ([`Ctx`]): os casos sem ramo de plataforma rodam como no Linux; os com ramo rodam uma
//! vez por plataforma (`Posix` no Linux, sufixo `_win32` e `_macos` nos demais).
//! Onde o original passa `null` ou `undefined` (`glob.match(null, ...)`, `parse(null)(...)`, caminho
//! `null`), o sistema de tipos do Rust torna o argumento impossível: só ficam as asserções com
//! valor vazio (`''`), e a omissão está dita no caso. O `ensureNoDisposablesAreLeakedInTestSuite`
//! não entra, porque o `glob` não cria nenhum `IDisposable`.
//! O teste `perf` do original está inteiro comentado e não é um caso.

mod common;

use je_base::glob::{
    ExprMatch, ExprValue, Expression, Glob, GlobOptions, ParsedGlob, PatternArg, PatternItem, RelativePattern, Sibling, get_basename_terms, get_path_terms,
    match_expression, match_pattern, parse, parse_expression, parse_pattern,
};
use je_base::glob;
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::uri::{Field, UriChange, URI};
use je_base::JsString;

#[derive(Clone, Copy)]
struct Ctx {
    platform: Platform,
    is_linux: bool,
}

const LINUX: Ctx = Ctx { platform: Platform::Posix, is_linux: true };
const MACOS: Ctx = Ctx { platform: Platform::Posix, is_linux: false };
const WIN32: Ctx = Ctx { platform: Platform::Win32, is_linux: false };

impl Ctx {
    fn is_windows(&self) -> bool {
        self.platform == Platform::Win32
    }

    fn opts(&self, ignore_case: Option<bool>) -> GlobOptions {
        GlobOptions { ignore_case, platform: self.platform, is_linux: self.is_linux, ..GlobOptions::default() }
    }

    fn trim_opts(&self) -> GlobOptions {
        GlobOptions { trim_for_exclusions: true, ..self.opts(None) }
    }

    fn native_sep(&self, slash_path: &str) -> String {
        if self.is_windows() { slash_path.replace('/', "\\") } else { slash_path.to_string() }
    }

    fn assert_glob_match<'a>(&self, pattern: impl Into<PatternArg<'a>> + Copy, input: &str, ignore_case: Option<bool>) {
        let options = self.opts(ignore_case);
        assert!(match_pattern(pattern, input, &options), "pattern should match {input}");
        let native = self.native_sep(input);
        assert!(match_pattern(pattern, &native, &options), "pattern should match {native}");
    }

    fn assert_no_glob_match<'a>(&self, pattern: impl Into<PatternArg<'a>> + Copy, input: &str, ignore_case: Option<bool>) {
        let options = self.opts(ignore_case);
        assert!(!match_pattern(pattern, input, &options), "pattern should not match {input}");
        let native = self.native_sep(input);
        assert!(!match_pattern(pattern, &native, &options), "pattern should not match {native}");
    }
}

fn assert_glob_match<'a>(pattern: impl Into<PatternArg<'a>> + Copy, input: &str) {
    LINUX.assert_glob_match(pattern, input, None);
}

fn assert_no_glob_match<'a>(pattern: impl Into<PatternArg<'a>> + Copy, input: &str) {
    LINUX.assert_no_glob_match(pattern, input, None);
}

fn assert_glob_match_ignore_case<'a>(pattern: impl Into<PatternArg<'a>> + Copy, input: &str) {
    LINUX.assert_glob_match(pattern, input, Some(true));
}

/// `glob.parse(pattern)(path, basename)` para texto.
fn parsed(pattern: &str, path: &str, basename: Option<&str>) -> bool {
    parse_pattern(pattern, &LINUX.opts(None)).matches(path, basename)
}

/// `glob.parse(pattern, { trimForExclusions: true })(path, basename)`.
fn parsed_trim(pattern: &str, path: &str, basename: Option<&str>) -> bool {
    parse_pattern(pattern, &LINUX.trim_opts()).matches(path, basename)
}

fn expr<const N: usize>(entries: [(&str, ExprValue); N]) -> Expression {
    Expression::from_entries(entries)
}

fn when(clause: &str) -> ExprValue {
    ExprValue::When(clause.to_string())
}

type SiblingFn<'a> = &'a dyn Fn(&str) -> Sibling;

fn has_sibling_in(siblings: &'static [&'static str]) -> impl Fn(&str) -> Sibling {
    move |name| Sibling::Ready(siblings.contains(&name))
}

/// `parse(expression)(path, undefined, hasSibling)`, com o resultado já conhecido.
fn parsed_expr(expression: &Expression, path: &str, has_sibling: Option<&dyn Fn(&str) -> Sibling>) -> Option<String> {
    parse_expression(expression, &LINUX.opts(None)).matches(path, None, has_sibling).matched().map(str::to_string)
}

fn matched(expression: &Expression, path: &str) -> Option<String> {
    match_expression(expression, path, &LINUX.opts(None)).matched().map(str::to_string)
}

fn matched_ignore_case(expression: &Expression, path: &str) -> Option<String> {
    match_expression(expression, path, &LINUX.opts(Some(true))).matched().map(str::to_string)
}

fn some(text: &str) -> Option<String> {
    Some(text.to_string())
}

#[test]
fn simple() {
    let mut p = "node_modules";

    assert_glob_match(p, "node_modules");
    assert_no_glob_match(p, "node_module");
    assert_no_glob_match(p, "/node_modules");
    assert_no_glob_match(p, "test/node_modules");

    p = "test.txt";
    assert_glob_match(p, "test.txt");
    assert_no_glob_match(p, "test?txt");
    assert_no_glob_match(p, "/text.txt");
    assert_no_glob_match(p, "test/test.txt");

    p = "test(.txt";
    assert_glob_match(p, "test(.txt");
    assert_no_glob_match(p, "test?txt");

    p = "qunit";

    assert_glob_match(p, "qunit");
    assert_no_glob_match(p, "qunit.css");
    assert_no_glob_match(p, "test/qunit");

    // Absolute

    p = "/DNXConsoleApp/**/*.cs";
    assert_glob_match(p, "/DNXConsoleApp/Program.cs");
    assert_glob_match(p, "/DNXConsoleApp/foo/Program.cs");

    p = "C:/DNXConsoleApp/**/*.cs";
    assert_glob_match(p, "C:\\DNXConsoleApp\\Program.cs");
    assert_glob_match(p, "C:\\DNXConsoleApp\\foo\\Program.cs");

    p = "*";
    assert_glob_match(p, "");
}

#[test]
fn dot_hidden() {
    let mut p = ".*";

    assert_glob_match(p, ".git");
    assert_glob_match(p, ".hidden.txt");
    assert_no_glob_match(p, "git");
    assert_no_glob_match(p, "hidden.txt");
    assert_no_glob_match(p, "path/.git");
    assert_no_glob_match(p, "path/.hidden.txt");

    p = "**/.*";
    assert_glob_match(p, ".git");
    assert_glob_match(p, "/.git");
    assert_glob_match(p, ".hidden.txt");
    assert_no_glob_match(p, "git");
    assert_no_glob_match(p, "hidden.txt");
    assert_glob_match(p, "path/.git");
    assert_glob_match(p, "path/.hidden.txt");
    assert_glob_match(p, "/path/.git");
    assert_glob_match(p, "/path/.hidden.txt");
    assert_no_glob_match(p, "path/git");
    assert_no_glob_match(p, "pat.h/hidden.txt");

    p = "._*";

    assert_glob_match(p, "._git");
    assert_glob_match(p, "._hidden.txt");
    assert_no_glob_match(p, "git");
    assert_no_glob_match(p, "hidden.txt");
    assert_no_glob_match(p, "path/._git");
    assert_no_glob_match(p, "path/._hidden.txt");

    p = "**/._*";
    assert_glob_match(p, "._git");
    assert_glob_match(p, "._hidden.txt");
    assert_no_glob_match(p, "git");
    assert_no_glob_match(p, "hidden._txt");
    assert_glob_match(p, "path/._git");
    assert_glob_match(p, "path/._hidden.txt");
    assert_glob_match(p, "/path/._git");
    assert_glob_match(p, "/path/._hidden.txt");
    assert_no_glob_match(p, "path/git");
    assert_no_glob_match(p, "pat.h/hidden._txt");
}

#[test]
fn file_pattern() {
    let mut p = "*.js";

    assert_glob_match(p, "foo.js");
    assert_no_glob_match(p, "folder/foo.js");
    assert_no_glob_match(p, "/node_modules/foo.js");
    assert_no_glob_match(p, "foo.jss");
    assert_no_glob_match(p, "some.js/test");

    p = "html.*";
    assert_glob_match(p, "html.js");
    assert_glob_match(p, "html.txt");
    assert_no_glob_match(p, "htm.txt");

    p = "*.*";
    assert_glob_match(p, "html.js");
    assert_glob_match(p, "html.txt");
    assert_glob_match(p, "htm.txt");
    assert_no_glob_match(p, "folder/foo.js");
    assert_no_glob_match(p, "/node_modules/foo.js");

    p = "node_modules/test/*.js";
    assert_glob_match(p, "node_modules/test/foo.js");
    assert_no_glob_match(p, "folder/foo.js");
    assert_no_glob_match(p, "/node_module/test/foo.js");
    assert_no_glob_match(p, "foo.jss");
    assert_no_glob_match(p, "some.js/test");
}

#[test]
fn star() {
    let mut p = "node*modules";

    assert_glob_match(p, "node_modules");
    assert_glob_match(p, "node_super_modules");
    assert_no_glob_match(p, "node_module");
    assert_no_glob_match(p, "/node_modules");
    assert_no_glob_match(p, "test/node_modules");

    p = "*";
    assert_glob_match(p, "html.js");
    assert_glob_match(p, "html.txt");
    assert_glob_match(p, "htm.txt");
    assert_no_glob_match(p, "folder/foo.js");
    assert_no_glob_match(p, "/node_modules/foo.js");
}

#[test]
fn file_folder_match() {
    let p = "**/node_modules/**";

    assert_glob_match(p, "node_modules");
    assert_glob_match(p, "node_modules/");
    assert_glob_match(p, "a/node_modules");
    assert_glob_match(p, "a/node_modules/");
    assert_glob_match(p, "node_modules/foo");
    assert_glob_match(p, "foo/node_modules/foo/bar");

    assert_glob_match(p, "/node_modules");
    assert_glob_match(p, "/node_modules/");
    assert_glob_match(p, "/a/node_modules");
    assert_glob_match(p, "/a/node_modules/");
    assert_glob_match(p, "/node_modules/foo");
    assert_glob_match(p, "/foo/node_modules/foo/bar");
}

#[test]
fn questionmark() {
    let mut p = "node?modules";

    assert_glob_match(p, "node_modules");
    assert_no_glob_match(p, "node_super_modules");
    assert_no_glob_match(p, "node_module");
    assert_no_glob_match(p, "/node_modules");
    assert_no_glob_match(p, "test/node_modules");

    p = "?";
    assert_glob_match(p, "h");
    assert_no_glob_match(p, "html.txt");
    assert_no_glob_match(p, "htm.txt");
    assert_no_glob_match(p, "folder/foo.js");
    assert_no_glob_match(p, "/node_modules/foo.js");
}

#[test]
fn globstar() {
    let mut p = "**/*.js";

    assert_glob_match(p, "foo.js");
    assert_glob_match(p, "/foo.js");
    assert_glob_match(p, "folder/foo.js");
    assert_glob_match(p, "/node_modules/foo.js");
    assert_no_glob_match(p, "foo.jss");
    assert_no_glob_match(p, "some.js/test");
    assert_no_glob_match(p, "/some.js/test");
    assert_no_glob_match(p, "\\some.js\\test");

    p = "**/project.json";

    assert_glob_match(p, "project.json");
    assert_glob_match(p, "/project.json");
    assert_glob_match(p, "some/folder/project.json");
    assert_glob_match(p, "/some/folder/project.json");
    assert_no_glob_match(p, "some/folder/file_project.json");
    assert_no_glob_match(p, "some/folder/fileproject.json");
    assert_no_glob_match(p, "some/rrproject.json");
    assert_no_glob_match(p, "some\\rrproject.json");

    p = "test/**";
    assert_glob_match(p, "test");
    assert_glob_match(p, "test/foo");
    assert_glob_match(p, "test/foo/");
    assert_glob_match(p, "test/foo.js");
    assert_glob_match(p, "test/other/foo.js");
    assert_no_glob_match(p, "est/other/foo.js");

    p = "**";
    assert_glob_match(p, "/");
    assert_glob_match(p, "foo.js");
    assert_glob_match(p, "folder/foo.js");
    assert_glob_match(p, "folder/foo/");
    assert_glob_match(p, "/node_modules/foo.js");
    assert_glob_match(p, "foo.jss");
    assert_glob_match(p, "some.js/test");

    p = "test/**/*.js";
    assert_glob_match(p, "test/foo.js");
    assert_glob_match(p, "test/other/foo.js");
    assert_glob_match(p, "test/other/more/foo.js");
    assert_no_glob_match(p, "test/foo.ts");
    assert_no_glob_match(p, "test/other/foo.ts");
    assert_no_glob_match(p, "test/other/more/foo.ts");

    p = "**/**/*.js";

    assert_glob_match(p, "foo.js");
    assert_glob_match(p, "/foo.js");
    assert_glob_match(p, "folder/foo.js");
    assert_glob_match(p, "/node_modules/foo.js");
    assert_no_glob_match(p, "foo.jss");
    assert_no_glob_match(p, "some.js/test");

    p = "**/node_modules/**/*.js";

    assert_no_glob_match(p, "foo.js");
    assert_no_glob_match(p, "folder/foo.js");
    assert_glob_match(p, "node_modules/foo.js");
    assert_glob_match(p, "/node_modules/foo.js");
    assert_glob_match(p, "node_modules/some/folder/foo.js");
    assert_glob_match(p, "/node_modules/some/folder/foo.js");
    assert_no_glob_match(p, "node_modules/some/folder/foo.ts");
    assert_no_glob_match(p, "foo.jss");
    assert_no_glob_match(p, "some.js/test");

    p = "{**/node_modules/**,**/.git/**,**/bower_components/**}";

    assert_glob_match(p, "node_modules");
    assert_glob_match(p, "/node_modules");
    assert_glob_match(p, "/node_modules/more");
    assert_glob_match(p, "some/test/node_modules");
    assert_glob_match(p, "some\\test\\node_modules");
    assert_glob_match(p, "/some/test/node_modules");
    assert_glob_match(p, "\\some\\test\\node_modules");
    assert_glob_match(p, "C:\\\\some\\test\\node_modules");
    assert_glob_match(p, "C:\\\\some\\test\\node_modules\\more");

    assert_glob_match(p, "bower_components");
    assert_glob_match(p, "bower_components/more");
    assert_glob_match(p, "/bower_components");
    assert_glob_match(p, "some/test/bower_components");
    assert_glob_match(p, "some\\test\\bower_components");
    assert_glob_match(p, "/some/test/bower_components");
    assert_glob_match(p, "\\some\\test\\bower_components");
    assert_glob_match(p, "C:\\\\some\\test\\bower_components");
    assert_glob_match(p, "C:\\\\some\\test\\bower_components\\more");

    assert_glob_match(p, ".git");
    assert_glob_match(p, "/.git");
    assert_glob_match(p, "some/test/.git");
    assert_glob_match(p, "some\\test\\.git");
    assert_glob_match(p, "/some/test/.git");
    assert_glob_match(p, "\\some\\test\\.git");
    assert_glob_match(p, "C:\\\\some\\test\\.git");

    assert_no_glob_match(p, "tempting");
    assert_no_glob_match(p, "/tempting");
    assert_no_glob_match(p, "some/test/tempting");
    assert_no_glob_match(p, "some\\test\\tempting");
    assert_no_glob_match(p, "/some/test/tempting");
    assert_no_glob_match(p, "\\some\\test\\tempting");
    assert_no_glob_match(p, "C:\\\\some\\test\\tempting");

    p = "{**/package.json,**/project.json}";
    assert_glob_match(p, "package.json");
    assert_glob_match(p, "/package.json");
    assert_no_glob_match(p, "xpackage.json");
    assert_no_glob_match(p, "/xpackage.json");
}

#[test]
fn issue_41724() {
    let mut p = "some/**/*.js";

    assert_glob_match(p, "some/foo.js");
    assert_glob_match(p, "some/folder/foo.js");
    assert_no_glob_match(p, "something/foo.js");
    assert_no_glob_match(p, "something/folder/foo.js");

    p = "some/**/*";

    assert_glob_match(p, "some/foo.js");
    assert_glob_match(p, "some/folder/foo.js");
    assert_no_glob_match(p, "something/foo.js");
    assert_no_glob_match(p, "something/folder/foo.js");
}

#[test]
fn brace_expansion() {
    let mut p = "*.{html,js}";

    assert_glob_match(p, "foo.js");
    assert_glob_match(p, "foo.html");
    assert_no_glob_match(p, "folder/foo.js");
    assert_no_glob_match(p, "/node_modules/foo.js");
    assert_no_glob_match(p, "foo.jss");
    assert_no_glob_match(p, "some.js/test");

    p = "*.{html}";

    assert_glob_match(p, "foo.html");
    assert_no_glob_match(p, "foo.js");
    assert_no_glob_match(p, "folder/foo.js");
    assert_no_glob_match(p, "/node_modules/foo.js");
    assert_no_glob_match(p, "foo.jss");
    assert_no_glob_match(p, "some.js/test");

    p = "{node_modules,testing}";
    assert_glob_match(p, "node_modules");
    assert_glob_match(p, "testing");
    assert_no_glob_match(p, "node_module");
    assert_no_glob_match(p, "dtesting");

    p = "**/{foo,bar}";
    assert_glob_match(p, "foo");
    assert_glob_match(p, "bar");
    assert_glob_match(p, "test/foo");
    assert_glob_match(p, "test/bar");
    assert_glob_match(p, "other/more/foo");
    assert_glob_match(p, "other/more/bar");
    assert_glob_match(p, "/foo");
    assert_glob_match(p, "/bar");
    assert_glob_match(p, "/test/foo");
    assert_glob_match(p, "/test/bar");
    assert_glob_match(p, "/other/more/foo");
    assert_glob_match(p, "/other/more/bar");

    p = "{foo,bar}/**";
    assert_glob_match(p, "foo");
    assert_glob_match(p, "bar");
    assert_glob_match(p, "bar/");
    assert_glob_match(p, "foo/test");
    assert_glob_match(p, "bar/test");
    assert_glob_match(p, "bar/test/");
    assert_glob_match(p, "foo/other/more");
    assert_glob_match(p, "bar/other/more");
    assert_glob_match(p, "bar/other/more/");

    p = "{**/*.d.ts,**/*.js}";

    assert_glob_match(p, "foo.js");
    assert_glob_match(p, "testing/foo.js");
    assert_glob_match(p, "testing\\foo.js");
    assert_glob_match(p, "/testing/foo.js");
    assert_glob_match(p, "\\testing\\foo.js");
    assert_glob_match(p, "C:\\testing\\foo.js");

    assert_glob_match(p, "foo.d.ts");
    assert_glob_match(p, "testing/foo.d.ts");
    assert_glob_match(p, "testing\\foo.d.ts");
    assert_glob_match(p, "/testing/foo.d.ts");
    assert_glob_match(p, "\\testing\\foo.d.ts");
    assert_glob_match(p, "C:\\testing\\foo.d.ts");

    assert_no_glob_match(p, "foo.d");
    assert_no_glob_match(p, "testing/foo.d");
    assert_no_glob_match(p, "testing\\foo.d");
    assert_no_glob_match(p, "/testing/foo.d");
    assert_no_glob_match(p, "\\testing\\foo.d");
    assert_no_glob_match(p, "C:\\testing\\foo.d");

    p = "{**/*.d.ts,**/*.js,path/simple.jgs}";

    assert_glob_match(p, "foo.js");
    assert_glob_match(p, "testing/foo.js");
    assert_glob_match(p, "testing\\foo.js");
    assert_glob_match(p, "/testing/foo.js");
    assert_glob_match(p, "path/simple.jgs");
    assert_no_glob_match(p, "/path/simple.jgs");
    assert_glob_match(p, "\\testing\\foo.js");
    assert_glob_match(p, "C:\\testing\\foo.js");

    p = "{**/*.d.ts,**/*.js,foo.[0-9]}";

    assert_glob_match(p, "foo.5");
    assert_glob_match(p, "foo.8");
    assert_no_glob_match(p, "bar.5");
    assert_no_glob_match(p, "foo.f");
    assert_glob_match(p, "foo.js");

    p = "prefix/{**/*.d.ts,**/*.js,foo.[0-9]}";

    assert_glob_match(p, "prefix/foo.5");
    assert_glob_match(p, "prefix/foo.8");
    assert_no_glob_match(p, "prefix/bar.5");
    assert_no_glob_match(p, "prefix/foo.f");
    assert_glob_match(p, "prefix/foo.js");
}

#[test]
fn expression_support_single() {
    let has_sibling = has_sibling_in(&["test.html", "test.txt", "test.ts", "test.js"]);

    // { "**/*.js": { "when": "$(basename).ts" } }
    let mut expression = expr([("**/*.js", when("$(basename).ts"))]);

    assert_eq!(parsed_expr(&expression, "test.js", Some(&has_sibling)), some("**/*.js"));
    assert_eq!(parsed_expr(&expression, "test.js", Some(&|_| Sibling::Ready(false))), None);
    assert_eq!(parsed_expr(&expression, "test.js", Some(&|name| Sibling::Ready(name == "te.ts"))), None);
    assert_eq!(parsed_expr(&expression, "test.js", None), None);

    expression = expr([("**/*.js", when(""))]);

    assert_eq!(parsed_expr(&expression, "test.js", Some(&has_sibling)), None);

    // `{}` no lugar da cláusula (o `as any` do original): sem `when` string, o padrão vale sozinho.
    expression = expr([("**/*.js", ExprValue::Other)]);

    assert_eq!(parsed_expr(&expression, "test.js", Some(&has_sibling)), some("**/*.js"));

    expression = Expression::new();

    assert_eq!(parsed_expr(&expression, "test.js", Some(&has_sibling)), None);
}

#[test]
fn expression_support_multiple() {
    let has_sibling = has_sibling_in(&["test.html", "test.txt", "test.ts", "test.js"]);

    // { "**/*.js": { "when": "$(basename).ts" } }
    let expression = expr([
        ("**/*.js", when("$(basename).ts")),
        ("**/*.as", ExprValue::Bool(true)),
        ("**/*.foo", ExprValue::Bool(false)),
        ("**/*.bananas", ExprValue::Other), // `{ bananas: true } as any` no original
    ]);

    assert_eq!(parsed_expr(&expression, "test.js", Some(&has_sibling)), some("**/*.js"));
    assert_eq!(parsed_expr(&expression, "test.as", Some(&has_sibling)), some("**/*.as"));
    assert_eq!(parsed_expr(&expression, "test.bananas", Some(&has_sibling)), some("**/*.bananas"));
    assert_eq!(parsed_expr(&expression, "test.bananas", None), some("**/*.bananas"));
    assert_eq!(parsed_expr(&expression, "test.foo", Some(&has_sibling)), None);
}

#[test]
fn brackets() {
    let mut p = "foo.[0-9]";

    assert_glob_match(p, "foo.5");
    assert_glob_match(p, "foo.8");
    assert_no_glob_match(p, "bar.5");
    assert_no_glob_match(p, "foo.f");

    p = "foo.[^0-9]";

    assert_no_glob_match(p, "foo.5");
    assert_no_glob_match(p, "foo.8");
    assert_no_glob_match(p, "bar.5");
    assert_glob_match(p, "foo.f");

    p = "foo.[!0-9]";

    assert_no_glob_match(p, "foo.5");
    assert_no_glob_match(p, "foo.8");
    assert_no_glob_match(p, "bar.5");
    assert_glob_match(p, "foo.f");

    p = "foo.[0!^*?]";

    assert_no_glob_match(p, "foo.5");
    assert_no_glob_match(p, "foo.8");
    assert_glob_match(p, "foo.0");
    assert_glob_match(p, "foo.!");
    assert_glob_match(p, "foo.^");
    assert_glob_match(p, "foo.*");
    assert_glob_match(p, "foo.?");

    p = "foo[/]bar";

    assert_no_glob_match(p, "foo/bar");

    p = "foo.[[]";

    assert_glob_match(p, "foo.[");

    p = "foo.[]]";

    assert_glob_match(p, "foo.]");

    p = "foo.[][!]";

    assert_glob_match(p, "foo.]");
    assert_glob_match(p, "foo.[");
    assert_glob_match(p, "foo.!");

    p = "foo.[]-]";

    assert_glob_match(p, "foo.]");
    assert_glob_match(p, "foo.-");
}

#[test]
fn full_path() {
    assert_glob_match("testing/this/foo.txt", "testing/this/foo.txt");
}

#[test]
fn ending_path() {
    assert_glob_match("**/testing/this/foo.txt", "some/path/testing/this/foo.txt");
}

/// As asserções de `prefix agnostic` e `cached properly` sobre `**/*.js`, na ordem do original.
fn assert_js_suffix_matches() {
    let p = "**/*.js";

    assert_glob_match(p, "foo.js");
    assert_glob_match(p, "testing/foo.js");
    assert_glob_match(p, "testing\\foo.js");
    assert_glob_match(p, "/testing/foo.js");
    assert_glob_match(p, "\\testing\\foo.js");
    assert_glob_match(p, "C:\\testing\\foo.js");

    assert_no_glob_match(p, "foo.ts");
    assert_no_glob_match(p, "testing/foo.ts");
    assert_no_glob_match(p, "testing\\foo.ts");
    assert_no_glob_match(p, "/testing/foo.ts");
    assert_no_glob_match(p, "\\testing\\foo.ts");
    assert_no_glob_match(p, "C:\\testing\\foo.ts");

    assert_no_glob_match(p, "foo.js.txt");
    assert_no_glob_match(p, "testing/foo.js.txt");
    assert_no_glob_match(p, "testing\\foo.js.txt");
    assert_no_glob_match(p, "/testing/foo.js.txt");
    assert_no_glob_match(p, "\\testing\\foo.js.txt");
    assert_no_glob_match(p, "C:\\testing\\foo.js.txt");

    assert_no_glob_match(p, "testing.js/foo");
    assert_no_glob_match(p, "testing.js\\foo");
    assert_no_glob_match(p, "/testing.js/foo");
    assert_no_glob_match(p, "\\testing.js\\foo");
    assert_no_glob_match(p, "C:\\testing.js\\foo");
}

#[test]
fn prefix_agnostic() {
    let mut p = "**/*.js";

    assert_glob_match(p, "foo.js");
    assert_glob_match(p, "/foo.js");
    assert_glob_match(p, "\\foo.js");
    assert_js_suffix_matches();

    p = "**/foo.js";

    assert_glob_match(p, "foo.js");
    assert_glob_match(p, "/foo.js");
    assert_glob_match(p, "\\foo.js");
    assert_glob_match(p, "testing/foo.js");
    assert_glob_match(p, "testing\\foo.js");
    assert_glob_match(p, "/testing/foo.js");
    assert_glob_match(p, "\\testing\\foo.js");
    assert_glob_match(p, "C:\\testing\\foo.js");
}

#[test]
fn cached_properly() {
    assert_js_suffix_matches();

    // Run again and make sure the regex are properly reused

    assert_js_suffix_matches();
}

#[test]
fn invalid_glob() {
    let p = "**/*(.js";

    assert_no_glob_match(p, "foo.js");
}

#[test]
fn split_glob_aware() {
    assert_eq!(glob::split_glob_aware("foo,bar", ','), ["foo", "bar"]);
    assert_eq!(glob::split_glob_aware("foo", ','), ["foo"]);
    assert_eq!(glob::split_glob_aware("{foo,bar}", ','), ["{foo,bar}"]);
    assert_eq!(glob::split_glob_aware("foo,bar,{foo,bar}", ','), ["foo", "bar", "{foo,bar}"]);
    assert_eq!(glob::split_glob_aware("{foo,bar},foo,bar,{foo,bar}", ','), ["{foo,bar}", "foo", "bar", "{foo,bar}"]);

    assert_eq!(glob::split_glob_aware("[foo,bar]", ','), ["[foo,bar]"]);
    assert_eq!(glob::split_glob_aware("foo,bar,[foo,bar]", ','), ["foo", "bar", "[foo,bar]"]);
    assert_eq!(glob::split_glob_aware("[foo,bar],foo,bar,[foo,bar]", ','), ["[foo,bar]", "foo", "bar", "[foo,bar]"]);
}

#[test]
fn expression_with_disabled_glob() {
    let expr = expr([("**/*.js", ExprValue::Bool(false))]);

    assert_eq!(matched(&expr, "foo.js"), None);
}

#[test]
fn expression_with_two_non_trivia_globs() {
    let expr = expr([("**/*.j?", true.into()), ("**/*.t?", true.into())]);

    assert_eq!(matched(&expr, "foo.js"), some("**/*.j?"));
    assert_eq!(matched(&expr, "foo.as"), None);
}

#[test]
fn expression_with_non_trivia_glob_issue_144458() {
    let pattern = "**/p*";
    let opts = LINUX.opts(None);
    let matches = |path: &str| match_pattern(pattern, path, &opts);

    assert!(!matches("foo/barp"));
    assert!(!matches("foo/bar/ap"));
    assert!(!matches("ap"));

    assert!(!matches("foo/barp1"));
    assert!(!matches("foo/bar/ap1"));
    assert!(!matches("ap1"));

    assert!(!matches("/foo/barp"));
    assert!(!matches("/foo/bar/ap"));
    assert!(!matches("/ap"));

    assert!(!matches("/foo/barp1"));
    assert!(!matches("/foo/bar/ap1"));
    assert!(!matches("/ap1"));

    assert!(matches("foo/pbar"));
    assert!(matches("/foo/pbar"));
    assert!(matches("foo/bar/pa"));
    assert!(matches("/p"));
}

#[test]
fn expression_with_empty_glob() {
    let expr = expr([("", true.into())]);

    assert_eq!(matched(&expr, "foo.js"), None);
}

#[test]
fn expression_with_other_falsy_value() {
    // `{ '**/*.js': 0 } as any` no original: só `false` desliga o padrão.
    let expr = expr([("**/*.js", ExprValue::Other)]);

    assert_eq!(matched(&expr, "foo.js"), some("**/*.js"));
}

#[test]
fn expression_with_two_basename_globs() {
    let expr = expr([("**/bar", true.into()), ("**/baz", true.into())]);

    assert_eq!(matched(&expr, "bar"), some("**/bar"));
    assert_eq!(matched(&expr, "foo"), None);
    assert_eq!(matched(&expr, "foo/bar"), some("**/bar"));
    assert_eq!(matched(&expr, "foo\\bar"), some("**/bar"));
    assert_eq!(matched(&expr, "foo/foo"), None);
}

#[test]
fn expression_with_two_basename_globs_ignores_case() {
    let expr = expr([("**/BAR", true.into()), ("**/BAZ", true.into())]);

    assert_eq!(matched_ignore_case(&expr, "bar"), some("**/BAR"));
    assert_eq!(matched_ignore_case(&expr, "baz"), some("**/BAZ"));
    assert_eq!(matched_ignore_case(&expr, "src/bar"), some("**/BAR"));
    assert_eq!(matched(&expr, "bar"), None);
}

#[test]
fn expression_with_cached_basename_globs_ignores_case() {
    parse_pattern("**/bar", &LINUX.opts(Some(true)));

    let expr = expr([("**/BAR", true.into()), ("**/BAZ", true.into())]);

    assert_eq!(matched_ignore_case(&expr, "BaR"), some("**/BAR"));
}

#[test]
fn expression_cache_does_not_collide_with_string_pattern_cache() {
    parse_pattern("**/BAR", &LINUX.opts(Some(true)));

    let expr = expr([("**/bar", true.into()), ("**/baz", true.into())]);

    assert_eq!(matched_ignore_case(&expr, "bar"), some("**/bar"));
}

fn expression_with_two_basename_globs_and_a_siblings_expression_on(ctx: Ctx) {
    let expr = expr([("**/bar", true.into()), ("**/baz", true.into()), ("**/*.js", when("$(basename).ts"))]);

    let has_sibling = has_sibling_in(&["foo.ts", "foo.js", "foo", "bar"]);
    let run = |path: &str| parse_expression(&expr, &ctx.opts(None)).matches(path, None, Some(&has_sibling)).matched().map(str::to_string);

    assert_eq!(run("bar"), some("**/bar"));
    assert_eq!(run("foo"), None);
    assert_eq!(run("foo/bar"), some("**/bar"));
    if ctx.is_windows() {
        // backslash is a valid file name character on posix
        assert_eq!(run("foo\\bar"), some("**/bar"));
    }
    assert_eq!(run("foo/foo"), None);
    assert_eq!(run("foo.js"), some("**/*.js"));
    assert_eq!(run("bar.js"), None);
}

#[test]
fn expression_with_two_basename_globs_and_a_siblings_expression() {
    expression_with_two_basename_globs_and_a_siblings_expression_on(LINUX);
}

#[test]
fn expression_with_two_basename_globs_and_a_siblings_expression_win32() {
    expression_with_two_basename_globs_and_a_siblings_expression_on(WIN32);
}

#[test]
fn expression_with_multipe_basename_globs() {
    let expr = expr([("**/bar", true.into()), ("{**/baz,**/foo}", true.into())]);

    assert_eq!(matched(&expr, "bar"), some("**/bar"));
    assert_eq!(matched(&expr, "foo"), some("{**/baz,**/foo}"));
    assert_eq!(matched(&expr, "baz"), some("{**/baz,**/foo}"));
    assert_eq!(matched(&expr, "abc"), None);
}

#[test]
fn falsy_expression_pattern() {
    // `glob.match(null, ...)` e `glob.parse(null)` não existem em Rust (sem `null`): só o vazio.
    assert!(!match_pattern("", "foo", &LINUX.opts(None)));
    assert!(!parsed("", "foo", None));
}

#[test]
fn falsy_path() {
    // O caminho `null` do original não existe em Rust: só o caminho vazio.
    assert!(!parsed("foo", "", None));
    assert!(!parsed("**/*.j?", "", None));
    assert!(!parsed("**/*.foo", "", None));
    assert!(!parsed("**/foo", "", None));
    assert!(!parsed("{**/baz,**/foo}", "", None));
    assert!(!parsed("{**/*.baz,**/*.foo}", "", None));
}

#[test]
fn expression_pattern_basename() {
    assert!(!parsed("**/foo", "bar/baz", Some("baz")));
    assert!(parsed("**/foo", "bar/foo", Some("foo")));

    assert!(!parsed("{**/baz,**/foo}", "baz/bar", Some("bar")));
    assert!(parsed("{**/baz,**/foo}", "baz/foo", Some("foo")));

    let expr = expr([("**/*.js", when("$(basename).ts"))]);
    let has_sibling = has_sibling_in(&["foo.ts", "foo.js"]);
    let run = |path: &str, basename: &str| {
        parse_expression(&expr, &LINUX.opts(None)).matches(path, Some(basename), Some(&has_sibling)).matched().map(str::to_string)
    };

    assert_eq!(run("bar/baz.js", "baz.js"), None);
    assert_eq!(run("bar/foo.js", "foo.js"), some("**/*.js"));
}

fn basename_terms(pattern: &str) -> Vec<String> {
    get_basename_terms(&parse_pattern(pattern, &LINUX.opts(None)))
}

fn terms(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

#[test]
fn expression_pattern_basename_terms() {
    assert_eq!(basename_terms("**/*.foo"), Vec::<String>::new());
    assert_eq!(basename_terms("**/foo"), terms(&["foo"]));
    assert_eq!(basename_terms("**/foo/"), terms(&["foo"]));
    assert_eq!(basename_terms("{**/baz,**/foo}"), terms(&["baz", "foo"]));
    assert_eq!(basename_terms("{**/baz/,**/foo/}"), terms(&["baz", "foo"]));

    let opts = LINUX.opts(None);
    assert_eq!(
        get_basename_terms(&parse_expression(
            &expr([
                ("**/foo", true.into()),
                ("{**/bar,**/baz}", true.into()),
                ("{**/bar2/,**/baz2/}", true.into()),
                ("**/bulb", false.into()),
            ]),
            &opts
        )),
        terms(&["foo", "bar", "baz", "bar2", "baz2"])
    );
    assert_eq!(
        get_basename_terms(&parse_expression(&expr([("**/foo", when("$(basename).zip")), ("**/bar", true.into())]), &opts)),
        terms(&["bar"])
    );
}

/// `testOptimizationForBasenames` e `testOptimizationForPaths`: `get_terms` escolhe entre os dois.
fn test_optimization(
    ctx: Ctx,
    pattern: Glob<'_>,
    get_terms: fn(&ParsedGlob) -> Vec<String>,
    expected_terms: &[&str],
    matches: &[(&str, Option<&str>)],
    siblings_fns: &[Option<SiblingFn<'_>>],
) {
    let parsed = parse(pattern, &ctx.trim_opts());
    assert_eq!(get_terms(&parsed), terms(expected_terms));
    for (index, (text, result)) in matches.iter().enumerate() {
        let sibling = siblings_fns.get(index).copied().flatten();
        let actual = match &parsed {
            ParsedGlob::Pattern(p) => p.matches(text, None).then_some("true".to_string()),
            ParsedGlob::Expression(e) => e.matches(text, None, sibling).matched().map(str::to_string),
        };
        // Para texto, o original compara com um booleano: `Some("true")` e `Some("false")` aqui.
        let expected = result.map(str::to_string);
        match (&parsed, expected.as_deref()) {
            (ParsedGlob::Pattern(_), Some("false")) => assert!(actual.is_none(), "{text}"),
            (ParsedGlob::Pattern(_), Some("true")) => assert!(actual.is_some(), "{text}"),
            _ => assert_eq!(actual, expected, "{text}"),
        }
    }
}

fn basenames_of(glob: &ParsedGlob) -> Vec<String> {
    get_basename_terms(glob)
}

fn paths_of(glob: &ParsedGlob) -> Vec<String> {
    get_path_terms(glob)
}

#[test]
fn expression_pattern_optimization_for_basenames() {
    let opts = LINUX.opts(None);
    assert_eq!(get_basename_terms(&parse_pattern("**/foo/**", &opts)), Vec::<String>::new());
    assert_eq!(get_basename_terms(&parse_pattern("**/foo/**", &LINUX.trim_opts())), terms(&["foo"]));

    let t = Some("true");
    let f = Some("false");
    test_optimization(LINUX, Glob::Pattern("**/*.foo/**"), basenames_of, &[], &[("baz/bar.foo/bar/baz", t)], &[]);
    test_optimization(LINUX, Glob::Pattern("**/foo/**"), basenames_of, &["foo"], &[("bar/foo", t), ("bar/foo/baz", f)], &[]);
    test_optimization(LINUX, Glob::Pattern("{**/baz/**,**/foo/**}"), basenames_of, &["baz", "foo"], &[("bar/baz", t), ("bar/foo", t)], &[]);

    let expression = expr([("**/foo/**", true.into()), ("{**/bar/**,**/baz/**}", true.into()), ("**/bulb/**", false.into())]);
    test_optimization(
        LINUX,
        Glob::Expression(&expression),
        basenames_of,
        &["foo", "bar", "baz"],
        &[("bar/foo", Some("**/foo/**")), ("foo/bar", Some("{**/bar/**,**/baz/**}")), ("bar/nope", None)],
        &[],
    );

    let has_sibling = has_sibling_in(&["baz", "baz.zip", "nope"]);
    let expression = expr([("**/foo/**", when("$(basename).zip")), ("**/bar/**", true.into())]);
    test_optimization(
        LINUX,
        Glob::Expression(&expression),
        basenames_of,
        &["bar"],
        &[("bar/foo", None), ("bar/foo/baz", None), ("bar/foo/nope", None), ("foo/bar", Some("**/bar/**"))],
        &[None, Some(&has_sibling), Some(&has_sibling)],
    );
}

#[test]
fn trailing_slash() {
    // Testing existing (more or less intuitive) behavior
    assert!(!parsed("**/foo/", "bar/baz", Some("baz")));
    assert!(parsed("**/foo/", "bar/foo", Some("foo")));
    assert!(!parsed("**/*.foo/", "bar/file.baz", Some("file.baz")));
    assert!(parsed("**/*.foo/", "bar/file.foo", Some("file.foo")));
    assert!(!parsed("{**/foo/,**/abc/}", "bar/baz", Some("baz")));
    assert!(parsed("{**/foo/,**/abc/}", "bar/foo", Some("foo")));
    assert!(parsed("{**/foo/,**/abc/}", "bar/abc", Some("abc")));
    assert!(!parsed_trim("{**/foo/,**/abc/}", "bar/baz", Some("baz")));
    assert!(parsed_trim("{**/foo/,**/abc/}", "bar/foo", Some("foo")));
    assert!(parsed_trim("{**/foo/,**/abc/}", "bar/abc", Some("abc")));
}

#[test]
fn expression_pattern_path() {
    let c = LINUX;
    let n = |p: &str| c.native_sep(p);

    assert!(!parsed("**/foo/bar", &n("foo/baz"), Some("baz")));
    assert!(parsed("**/foo/bar", &n("foo/bar"), Some("bar")));
    assert!(parsed("**/foo/bar", &n("bar/foo/bar"), Some("bar")));
    assert!(parsed("**/foo/bar/**", &n("bar/foo/bar"), Some("bar")));
    assert!(parsed("**/foo/bar/**", &n("bar/foo/bar/baz"), Some("baz")));
    assert!(parsed_trim("**/foo/bar/**", &n("bar/foo/bar"), Some("bar")));
    assert!(!parsed_trim("**/foo/bar/**", &n("bar/foo/bar/baz"), Some("baz")));

    assert!(!parsed("foo/bar", &n("foo/baz"), Some("baz")));
    assert!(parsed("foo/bar", &n("foo/bar"), Some("bar")));
    assert!(parsed("foo/bar/baz", &n("foo/bar/baz"), Some("baz"))); // #15424
    assert!(!parsed("foo/bar", &n("bar/foo/bar"), Some("bar")));
    assert!(parsed("foo/bar/**", &n("foo/bar/baz"), Some("baz")));
    assert!(parsed_trim("foo/bar/**", &n("foo/bar"), Some("bar")));
    assert!(!parsed_trim("foo/bar/**", &n("foo/bar/baz"), Some("baz")));
}

fn path_terms(pattern: &str) -> Vec<String> {
    get_path_terms(&parse_pattern(pattern, &LINUX.opts(None)))
}

#[test]
fn expression_pattern_paths() {
    assert_eq!(path_terms("**/*.foo"), Vec::<String>::new());
    assert_eq!(path_terms("**/foo"), Vec::<String>::new());
    assert_eq!(path_terms("**/foo/bar"), terms(&["*/foo/bar"]));
    assert_eq!(path_terms("**/foo/bar/"), terms(&["*/foo/bar"]));
    // Not supported
    // assert.deepStrictEqual(glob.getPathTerms(glob.parse('{**/baz/bar,**/foo/bar,**/bar}')), ['*/baz/bar', '*/foo/bar']);
    // assert.deepStrictEqual(glob.getPathTerms(glob.parse('{**/baz/bar/,**/foo/bar/,**/bar/}')), ['*/baz/bar', '*/foo/bar']);

    let opts = LINUX.opts(None);
    let parsed = parse_expression(
        &expr([
            ("**/foo/bar", true.into()),
            ("**/foo2/bar2", true.into()),
            // Not supported
            // '{**/bar/foo,**/baz/foo}': true,
            // '{**/bar2/foo/,**/baz2/foo/}': true,
            ("**/bulb", true.into()),
            ("**/bulb2", true.into()),
            ("**/bulb/foo", false.into()),
        ]),
        &opts,
    );
    assert_eq!(get_path_terms(&parsed), terms(&["*/foo/bar", "*/foo2/bar2"]));
    assert_eq!(get_basename_terms(&parsed), terms(&["bulb", "bulb2"]));
    assert_eq!(
        get_path_terms(&parse_expression(
            &expr([("**/foo/bar", when("$(basename).zip")), ("**/bar/foo", true.into()), ("**/bar2/foo2", true.into())]),
            &opts
        )),
        terms(&["*/bar/foo", "*/bar2/foo2"])
    );
}

#[test]
fn expression_pattern_optimization_for_paths() {
    let opts = LINUX.opts(None);
    assert_eq!(get_path_terms(&parse_pattern("**/foo/bar/**", &opts)), Vec::<String>::new());
    assert_eq!(get_path_terms(&parse_pattern("**/foo/bar/**", &LINUX.trim_opts())), terms(&["*/foo/bar"]));

    let n = |p: &str| LINUX.native_sep(p);
    let (t, f) = (Some("true"), Some("false"));
    test_optimization(LINUX, Glob::Pattern("**/*.foo/bar/**"), paths_of, &[], &[(&n("baz/bar.foo/bar/baz"), t)], &[]);
    test_optimization(LINUX, Glob::Pattern("**/foo/bar/**"), paths_of, &["*/foo/bar"], &[(&n("bar/foo/bar"), t), (&n("bar/foo/bar/baz"), f)], &[]);
    // Not supported
    // testOptimizationForPaths('{**/baz/bar/**,**/foo/bar/**}', ['*/baz/bar', '*/foo/bar'], [[nativeSep('bar/baz/bar'), true], [nativeSep('bar/foo/bar'), true]]);

    // Not supported: '{**/bar/bar/**,**/baz/bar/**}': true
    let expression = expr([("**/foo/bar/**", true.into()), ("**/bulb/bar/**", false.into())]);
    test_optimization(
        LINUX,
        Glob::Expression(&expression),
        paths_of,
        &["*/foo/bar"],
        &[(&n("bar/foo/bar"), Some("**/foo/bar/**")), (&n("/foo/bar/nope"), None)],
        &[],
    );

    let has_sibling = has_sibling_in(&["baz", "baz.zip", "nope"]);
    let expression = expr([("**/foo/123/**", when("$(basename).zip")), ("**/bar/123/**", true.into())]);
    test_optimization(
        LINUX,
        Glob::Expression(&expression),
        paths_of,
        &["*/bar/123"],
        &[(&n("bar/foo/123"), None), (&n("bar/foo/123/baz"), None), (&n("bar/foo/123/nope"), None), (&n("foo/bar/123"), Some("**/bar/123/**"))],
        &[None, Some(&has_sibling), Some(&has_sibling)],
    );
}

fn relative(base: &str, pattern: &str) -> RelativePattern {
    RelativePattern { base: base.to_string(), pattern: pattern.to_string() }
}

/// Os casos `isWindows ? win32 : posix` dos testes de padrão relativo: o ramo do Windows usa
/// `win_base` e caminhos com `\`, o outro `posix_base` e caminhos com `/`.
fn relative_pattern_glob_star_on(ctx: Ctx) {
    if ctx.is_windows() {
        let p = relative("C:\\DNXConsoleApp\\foo", "**/*.cs");
        ctx.assert_glob_match(&p, "C:\\DNXConsoleApp\\foo\\Program.cs", None);
        ctx.assert_glob_match(&p, "C:\\DNXConsoleApp\\foo\\bar\\Program.cs", None);
        ctx.assert_no_glob_match(&p, "C:\\DNXConsoleApp\\foo\\Program.ts", None);
        ctx.assert_no_glob_match(&p, "C:\\DNXConsoleApp\\Program.cs", None);
        ctx.assert_no_glob_match(&p, "C:\\other\\DNXConsoleApp\\foo\\Program.ts", None);
    } else {
        let p = relative("/DNXConsoleApp/foo", "**/*.cs");
        ctx.assert_glob_match(&p, "/DNXConsoleApp/foo/Program.cs", None);
        ctx.assert_glob_match(&p, "/DNXConsoleApp/foo/bar/Program.cs", None);
        ctx.assert_no_glob_match(&p, "/DNXConsoleApp/foo/Program.ts", None);
        ctx.assert_no_glob_match(&p, "/DNXConsoleApp/Program.cs", None);
        ctx.assert_no_glob_match(&p, "/other/DNXConsoleApp/foo/Program.ts", None);
    }
}

#[test]
fn relative_pattern_glob_star() {
    relative_pattern_glob_star_on(LINUX);
}

#[test]
fn relative_pattern_glob_star_win32() {
    relative_pattern_glob_star_on(WIN32);
}

fn relative_pattern_single_star_on(ctx: Ctx) {
    if ctx.is_windows() {
        let p = relative("C:\\DNXConsoleApp\\foo", "*.cs");
        ctx.assert_glob_match(&p, "C:\\DNXConsoleApp\\foo\\Program.cs", None);
        ctx.assert_no_glob_match(&p, "C:\\DNXConsoleApp\\foo\\bar\\Program.cs", None);
        ctx.assert_no_glob_match(&p, "C:\\DNXConsoleApp\\foo\\Program.ts", None);
        ctx.assert_no_glob_match(&p, "C:\\DNXConsoleApp\\Program.cs", None);
        ctx.assert_no_glob_match(&p, "C:\\other\\DNXConsoleApp\\foo\\Program.ts", None);
    } else {
        let p = relative("/DNXConsoleApp/foo", "*.cs");
        ctx.assert_glob_match(&p, "/DNXConsoleApp/foo/Program.cs", None);
        ctx.assert_no_glob_match(&p, "/DNXConsoleApp/foo/bar/Program.cs", None);
        ctx.assert_no_glob_match(&p, "/DNXConsoleApp/foo/Program.ts", None);
        ctx.assert_no_glob_match(&p, "/DNXConsoleApp/Program.cs", None);
        ctx.assert_no_glob_match(&p, "/other/DNXConsoleApp/foo/Program.ts", None);
    }
}

#[test]
fn relative_pattern_single_star() {
    relative_pattern_single_star_on(LINUX);
}

#[test]
fn relative_pattern_single_star_win32() {
    relative_pattern_single_star_on(WIN32);
}

fn relative_pattern_single_star_with_path_on(ctx: Ctx) {
    if ctx.is_windows() {
        let p = relative("C:\\DNXConsoleApp\\foo", "something/*.cs");
        ctx.assert_glob_match(&p, "C:\\DNXConsoleApp\\foo\\something\\Program.cs", None);
        ctx.assert_no_glob_match(&p, "C:\\DNXConsoleApp\\foo\\Program.cs", None);
    } else {
        let p = relative("/DNXConsoleApp/foo", "something/*.cs");
        ctx.assert_glob_match(&p, "/DNXConsoleApp/foo/something/Program.cs", None);
        ctx.assert_no_glob_match(&p, "/DNXConsoleApp/foo/Program.cs", None);
    }
}

#[test]
fn relative_pattern_single_star_with_path() {
    relative_pattern_single_star_with_path_on(LINUX);
}

#[test]
fn relative_pattern_single_star_with_path_win32() {
    relative_pattern_single_star_with_path_on(WIN32);
}

fn relative_pattern_single_star_alone_on(ctx: Ctx) {
    if ctx.is_windows() {
        let p = relative("C:\\DNXConsoleApp\\foo\\something\\Program.cs", "*");
        ctx.assert_glob_match(&p, "C:\\DNXConsoleApp\\foo\\something\\Program.cs", None);
        ctx.assert_no_glob_match(&p, "C:\\DNXConsoleApp\\foo\\Program.cs", None);
    } else {
        let p = relative("/DNXConsoleApp/foo/something/Program.cs", "*");
        ctx.assert_glob_match(&p, "/DNXConsoleApp/foo/something/Program.cs", None);
        ctx.assert_no_glob_match(&p, "/DNXConsoleApp/foo/Program.cs", None);
    }
}

#[test]
fn relative_pattern_single_star_alone() {
    relative_pattern_single_star_alone_on(LINUX);
}

#[test]
fn relative_pattern_single_star_alone_win32() {
    relative_pattern_single_star_alone_on(WIN32);
}

fn relative_pattern_ignores_case_on_macos_windows_on(ctx: Ctx) {
    if ctx.is_windows() {
        let p = relative("C:\\DNXConsoleApp\\foo", "something/*.cs");
        ctx.assert_glob_match(&p, &"C:\\DNXConsoleApp\\foo\\something\\Program.cs".to_lowercase(), None);
    } else if !ctx.is_linux {
        let p = relative("/DNXConsoleApp/foo", "something/*.cs");
        ctx.assert_glob_match(&p, &"/DNXConsoleApp/foo/something/Program.cs".to_lowercase(), None);
    } else {
        let p = relative("/DNXConsoleApp/foo", "something/*.cs");
        ctx.assert_no_glob_match(&p, &"/DNXConsoleApp/foo/something/Program.cs".to_lowercase(), None);
    }
}

#[test]
fn relative_pattern_ignores_case_on_macos_windows() {
    relative_pattern_ignores_case_on_macos_windows_on(LINUX);
}

#[test]
fn relative_pattern_ignores_case_on_macos_windows_macos() {
    relative_pattern_ignores_case_on_macos_windows_on(MACOS);
}

#[test]
fn relative_pattern_ignores_case_on_macos_windows_win32() {
    relative_pattern_ignores_case_on_macos_windows_on(WIN32);
}

fn relative_pattern_trailing_slash_backslash_162498_on(ctx: Ctx) {
    if ctx.is_windows() {
        let mut p = relative("C:\\", "foo.cs");
        ctx.assert_glob_match(&p, "C:\\foo.cs", None);

        p = relative("C:\\bar\\", "foo.cs");
        ctx.assert_glob_match(&p, "C:\\bar\\foo.cs", None);
    } else {
        let mut p = relative("/", "foo.cs");
        ctx.assert_glob_match(&p, "/foo.cs", None);

        p = relative("/bar/", "foo.cs");
        ctx.assert_glob_match(&p, "/bar/foo.cs", None);
    }
}

#[test]
fn relative_pattern_trailing_slash_backslash_162498() {
    relative_pattern_trailing_slash_backslash_162498_on(LINUX);
}

#[test]
fn relative_pattern_trailing_slash_backslash_162498_win32() {
    relative_pattern_trailing_slash_backslash_162498_on(WIN32);
}

#[test]
fn pattern_with_base_does_not_explode_36081() {
    assert!(matched(&expr([("base", true.into())]), "base").is_some());
}

fn relative_pattern_57475_on(ctx: Ctx) {
    if ctx.is_windows() {
        let p = relative("C:\\DNXConsoleApp\\foo", "styles/style.css");
        ctx.assert_glob_match(&p, "C:\\DNXConsoleApp\\foo\\styles\\style.css", None);
        ctx.assert_no_glob_match(&p, "C:\\DNXConsoleApp\\foo\\Program.cs", None);
    } else {
        let p = relative("/DNXConsoleApp/foo", "styles/style.css");
        ctx.assert_glob_match(&p, "/DNXConsoleApp/foo/styles/style.css", None);
        ctx.assert_no_glob_match(&p, "/DNXConsoleApp/foo/Program.cs", None);
    }
}

#[test]
fn relative_pattern_57475() {
    relative_pattern_57475_on(LINUX);
}

#[test]
fn relative_pattern_57475_win32() {
    relative_pattern_57475_on(WIN32);
}

#[test]
fn uri_match() {
    let p = "scheme:/**/*.md";
    let change = UriChange { scheme: Field::Set(JsString::from("scheme")), ..UriChange::default() };
    let uri = URI::file("super/duper/long/some/file.md", Platform::Posix).with(&change);
    assert_glob_match(p, &uri.to_string(false).to_string_lossy());
}

#[test]
fn expression_fails_when_siblings_use_promises() {
    common::run_with_faked_timers(async {
        let siblings = ["test.html", "test.txt", "test.ts"];
        let has_sibling = move |name: &str| Sibling::from(Promise::resolved(siblings.contains(&name)));

        // { "**/*.js": { "when": "$(basename).ts" } }
        let expression = expr([("**/test.js", when("$(basename).js")), ("**/*.js", when("$(basename).ts"))]);

        let parsed_expression = parse_expression(&expression, &LINUX.opts(None));

        let result: ExprMatch = parsed_expression.matches("test.js", None, Some(&has_sibling));
        assert_eq!(result.into_promise().await.expect("promessa rejeitada").as_deref(), Some("**/*.js"));
    });
}

#[test]
fn patterns_equals() {
    let items = |list: &[&str]| list.iter().map(|s| PatternItem::from(*s)).collect::<Vec<_>>();
    let eq = |a: Option<&[PatternItem]>, b: Option<&[PatternItem]>| glob::patterns_equals(a, b);

    assert!(eq(Some(&items(&["a"])), Some(&items(&["a"]))));
    assert!(!eq(Some(&items(&["a"])), Some(&items(&["b"]))));

    assert!(eq(Some(&items(&["a", "b", "c"])), Some(&items(&["a", "b", "c"]))));
    assert!(!eq(Some(&items(&["1", "2"])), Some(&items(&["1", "3"]))));

    let mixed = || vec![PatternItem::from(relative("a", "*")), "b".into(), "c".into()];
    assert!(eq(Some(&mixed()), Some(&mixed())));

    assert!(eq(None, None));
    assert!(!eq(None, Some(&items(&["b"]))));
    assert!(!eq(Some(&items(&["a"])), None));
}

#[test]
fn is_empty_pattern() {
    let opts = LINUX.opts(None);
    let empty = |glob: Glob<'_>| glob::is_empty_pattern(&parse(glob, &opts));

    assert!(empty(Glob::Pattern("")));
    // `parse(undefined)` e `parse(null)` não existem em Rust.

    assert!(empty(Glob::Expression(&Expression::new())));
    assert!(empty(Glob::Expression(&expr([("", true.into())]))));
    assert!(empty(Glob::Expression(&expr([("**/*.js", false.into())]))));
}

#[test]
fn case_insensitive_match() {
    assert_no_glob_match("PATH/FOO.js", "path/foo.js");
    assert_glob_match_ignore_case("PATH/FOO.js", "path/foo.js");
    // T1
    assert_no_glob_match("**/*.JS", "bar/foo.js");
    assert_glob_match_ignore_case("**/*.JS", "bar/foo.js");
    // T2
    assert_no_glob_match("**/package", "bar/Package");
    assert_glob_match_ignore_case("**/package", "bar/Package");
    // T3
    assert_no_glob_match("{**/*.JS,**/*.TS}", "bar/foo.ts");
    assert_no_glob_match("{**/*.JS,**/*.TS}", "bar/foo.js");
    assert_glob_match_ignore_case("{**/*.JS,**/*.TS}", "bar/foo.ts");
    assert_glob_match_ignore_case("{**/*.JS,**/*.TS}", "bar/foo.js");
    assert_no_glob_match("{**/BAR,**/BAZ}", "bar");
    assert_glob_match_ignore_case("{**/BAR,**/BAZ}", "bar");
    // T4
    assert_no_glob_match("**/FOO/Bar", "bar/foo/bar");
    assert_glob_match_ignore_case("**/FOO/Bar", "bar/foo/bar");
    // T5
    assert_no_glob_match("FOO/Bar", "foo/bar");
    assert_glob_match_ignore_case("FOO/Bar", "foo/bar");
    // Other
    assert_no_glob_match("some/*/Random/*/Path.FILE", "some/very/random/unusual/path.file");
    assert_glob_match_ignore_case("some/*/Random/*/Path.FILE", "some/very/random/unusual/path.file");
}
