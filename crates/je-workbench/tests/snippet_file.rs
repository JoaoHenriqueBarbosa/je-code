//! Porte de `src/vs/workbench/contrib/snippets/test/browser/snippetFile.test.ts` (suíte `Snippets`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo de cada caso.
//! - **`TestSnippetFile extends SnippetFile`.** Não há herança: [`test_snippet_file`] cria o `SnippetFile` com a origem
//!   `Extension`, sem extensão e sem escopos padrão, e acrescenta os snippets em `data`. O original passa `undefined!` como
//!   `IFileService` e `IExtensionResourceLoaderService`; o contrato do porte os exige, então são um `FileService` vazio e um
//!   carregador de recursos que falha se usado (nenhum caso lê o arquivo).
//! - **`Snippet`.** Os argumentos finais `include`, `exclude` e `extensionId` ausentes do original são `None`.
//! - **`SnippetParser.guessNeedsClipboard(body)`.** Recebe as unidades UTF-16 do texto.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::snippet::snippet_parser::SnippetParser;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::NullLogService;
use je_platform::resources::uri_file;
use je_workbench::contrib::snippets::snippets_file::{IExtensionResourceLoaderService, Snippet, SnippetFile, SnippetSource};

/// O `IExtensionResourceLoaderService` que o original deixa como `undefined!`.
struct UnusedExtensionResourceLoaderService;

impl IExtensionResourceLoaderService for UnusedExtensionResourceLoaderService {
    fn read_extension_resource(&self, _uri: &URI) -> Promise<String> {
        panic!("IExtensionResourceLoaderService não é usado por este teste (o original passa undefined)")
    }
}

/// `new TestSnippetFile(filepath, snippets)`.
fn test_snippet_file(guard: &LeakGuard, filepath: URI, snippets: Vec<Snippet>) -> Rc<SnippetFile> {
    let file_service = guard.add_rc(FileService::new(NullLogService::shared()));
    let file_service: Rc<dyn IFileService> = file_service;
    let file = SnippetFile::new(SnippetSource::Extension, filepath, None, None, file_service, Rc::new(UnusedExtensionResourceLoaderService));
    file.data.borrow_mut().extend(snippets.into_iter().map(Rc::new));
    file
}

/// `new Snippet(false, scopes, name, 'foo', '', body, 'test', SnippetSource.User, generateUuid())`.
fn a_snippet(scopes: &[&str], name: &str, body: &str) -> Snippet {
    Snippet::new(
        false,
        scopes.iter().map(|scope| scope.to_string()).collect::<Vec<_>>(),
        name,
        "foo",
        "",
        body,
        "test",
        SnippetSource::User,
        &generate_uuid(),
        None,
        None,
        None,
    )
}

#[test]
fn snippet_file_select() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut file = test_snippet_file(&guard, uri_file("somepath/foo.code-snippets"), Vec::new());
    let mut bucket: Vec<Rc<Snippet>> = Vec::new();
    file.select("", &mut bucket);
    assert_eq!(bucket.len(), 0);

    file = test_snippet_file(
        &guard,
        uri_file("somepath/foo.code-snippets"),
        vec![
            a_snippet(&["foo"], "FooSnippet1", "snippet"),
            a_snippet(&["foo"], "FooSnippet2", "snippet"),
            a_snippet(&["bar"], "BarSnippet1", "snippet"),
            a_snippet(&["bar.comment"], "BarSnippet2", "snippet"),
            a_snippet(&["bar.strings"], "BarSnippet2", "snippet"),
            a_snippet(&["bazz", "bazz"], "BazzSnippet1", "snippet"),
        ],
    );

    bucket = Vec::new();
    file.select("foo", &mut bucket);
    assert_eq!(bucket.len(), 2);

    bucket = Vec::new();
    file.select("fo", &mut bucket);
    assert_eq!(bucket.len(), 0);

    bucket = Vec::new();
    file.select("bar", &mut bucket);
    assert_eq!(bucket.len(), 1);

    bucket = Vec::new();
    file.select("bar.comment", &mut bucket);
    assert_eq!(bucket.len(), 2);

    bucket = Vec::new();
    file.select("bazz", &mut bucket);
    assert_eq!(bucket.len(), 1);
}

#[test]
fn snippet_file_select_any_scope() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let file = test_snippet_file(
        &guard,
        uri_file("somepath/foo.code-snippets"),
        vec![a_snippet(&[], "AnySnippet1", "snippet"), a_snippet(&["foo"], "FooSnippet1", "snippet")],
    );

    let mut bucket: Vec<Rc<Snippet>> = Vec::new();
    file.select("foo", &mut bucket);
    assert_eq!(bucket.len(), 2);
}

#[test]
fn snippet_needs_clipboard() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    fn assert_needs_clipboard(body: &str, expected: bool) {
        let snippet = a_snippet(&["foo"], "FooSnippet1", body);
        assert_eq!(snippet.needs_clipboard(), expected);

        let units: Vec<u16> = body.encode_utf16().collect();
        assert_eq!(SnippetParser::guess_needs_clipboard(&units), expected);
    }

    assert_needs_clipboard("foo$CLIPBOARD", true);
    assert_needs_clipboard("${CLIPBOARD}", true);
    assert_needs_clipboard("foo${CLIPBOARD}bar", true);
    assert_needs_clipboard("foo$clipboard", false);
    assert_needs_clipboard("foo${clipboard}", false);
    assert_needs_clipboard("baba", false);
}

#[test]
fn snippet_is_trivial() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    fn assert_is_trivial(body: &str, expected: bool) {
        let snippet = a_snippet(&["foo"], "FooSnippet1", body);
        assert_eq!(snippet.is_trivial(), expected);
    }

    assert_is_trivial("foo", true);
    assert_is_trivial("foo$0", true);
    assert_is_trivial("foo$0bar", false);
    assert_is_trivial("foo$1", false);
    assert_is_trivial("foo$1$0", false);
    assert_is_trivial("${1:foo}", false);
}
