//! Porte de `src/vs/workbench/contrib/browserView/test/common/browserSearch.test.ts`, caso a caso e na mesma
//! ordem (suítes `BrowserSearch - resolveAddressBarInput`, `buildSearchUrl` e `getBrowserSearchEngineLabel`).
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `inputs.map(resolveAddressBarInputType)` e o `deepStrictEqual` com `inputs.map(() => 'url')` viram
//!   `classify` e um vetor repetido do mesmo tamanho.
//! - `'nonexistent' as BrowserSearchEngineId` é o texto `"nonexistent"`, que as funções recebem como `&str`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::browser_view::browser_search::{
    AddressBarInputKind, BrowserSearchEngineId, build_search_url, get_browser_search_engine_label,
    resolve_address_bar_input_type,
};

fn classify(inputs: &[&str]) -> Vec<AddressBarInputKind> {
    inputs.iter().map(|input| resolve_address_bar_input_type(input)).collect()
}

fn assert_all(inputs: &[&str], expected: AddressBarInputKind) {
    assert_eq!(classify(inputs), vec![expected; inputs.len()]);
}

#[test]
fn recognized_schemes_are_urls() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let inputs = [
        "https://example.com",
        "http://example.com/path?q=1",
        "file:///etc/passwd",
        "about:blank",
        "data:text/html,<h1>hi</h1>",
        "view-source:https://example.com",
        "ftp://files.example.com",
        "vscode://settings",
        "HTTP://EXAMPLE.COM", // esquema sem diferença de caixa
        "https://münchen.de/pfad?q=wert", // Unicode depois do esquema
    ];
    assert_all(&inputs, AddressBarInputKind::Url);
}

#[test]
fn scheme_less_hosts_and_ips_are_urls() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let inputs = [
        // Hostnames com TLD
        "example.com",
        "sub.example.com:8080/path",
        "example.com?q=1",
        // Qualquer último rótulo de 2 ou mais letras vale como TLD (cobre gTLDs novos e de marca sem lista).
        "foo.bank",
        "acme.google",
        "foo.zzzznotatld",
        // IPv4 (navegável: primeiro octeto diferente de 0, ou exatamente 0.0.0.0)
        "127.0.0.1",
        "127.0.0.1:8080",
        "192.168.1.1/admin",
        "0.0.0.0",
        "0.0.0.0:8080",
        // IPv6 (entre colchetes)
        "[::]",
        "[::]:8080",
        "[::1]",
        "[::1]:8080",
        "[0:0:0:0:0:0:0:0]",
        "[0:0:0:0:0:0:0:1]",
        "[2001:db8::1]/path",
        // localhost
        "localhost",
        "localhost:3000",
        "localhost/foo",
        "localhost?q=1",
        // Atalhos de intranet (host solto com caminho terminado em `/`)
        "go/",
        "intranet/",
        "go/there/",
        "wiki/page/sub/",
        // Caminhos absolutos
        "/",
        "/usr/local/bin",
        "//example.com",
        // Nomes de domínio internacionalizados (IDN)
        "münchen.de",
        "xn--mnchen-3ya.de", // forma Punycode
        "münchen.de:8080/pfad",
        // Literais IPv6 entre colchetes (validados e canonicalizados pelo parser de URL)
        "[::1]",
        "[2001:0db8::0001]",
        // Subdomínios dos TLDs especiais da RFC 6761 exigem pelo menos 1 subdomínio
        "foo.test",
        "foo.local",
        "foo.example",
        "foo.internal",
    ];
    assert_all(&inputs, AddressBarInputKind::Url);
}

#[test]
fn queries_whitespace_or_invalid_host_chars_return_query() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let inputs = [
        "hello world",
        "what is 2+2",
        "  spaced  query  ",
        "a&b?c=d",     // caractere inválido de host `&`
        "0.1.2.3",     // IPv4 com primeiro octeto 0 (não é 0.0.0.0)
        "[:::::::]",   // IPv6 malformado
        "foo bar/baz", // espaço no host, caminho solto
    ];
    assert_all(&inputs, AddressBarInputKind::Query);
}

#[test]
fn whitespace_in_the_path_query_fragment_is_a_url_percent_encoded_later() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let inputs = [
        "http://localhost:8888/my file.php", // espaço no caminho, esquema explícito (issue #326784)
        "https://example.com/my file.php",
        "localhost:8888/my file.php", // host:porta sem esquema
        "example.com/foo bar",        // host sem esquema com TLD conhecido
        "example.com?q=hello world",  // espaço na consulta
        "example.com/a#frag ment",    // espaço no fragmento
    ];
    assert_all(&inputs, AddressBarInputKind::Url);
}

#[test]
fn whitespace_in_the_userinfo_is_unknown_defaults_to_search() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let inputs = ["user name@example.com", "user name@example.com/", "user name@localhost"];
    assert_all(&inputs, AddressBarInputKind::Unknown);
}

#[test]
fn an_explicit_http_s_scheme_keeps_whitespace_userinfo_input_a_url() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Os casos sem esquema acima são `unknown`, mas um esquema http(s) explícito vence (a exceção
    // `!isHttpScheme` na guarda do userinfo), como a regra "esquema explícito vira URL" do Chromium.
    let inputs = ["http://user name@example.com", "https://user name@example.com"];
    assert_all(&inputs, AddressBarInputKind::Url);
}

#[test]
fn whitespace_in_the_userinfo_with_an_ip_literal_host_is_still_a_url() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // O `AutocompleteInput::Parse` do Chromium classifica hosts IPv4/IPv6 como URL antes de aplicar a
    // heurística "espaço no username", então estes continuam URLs mesmo que um host de domínio com espaço
    // no userinfo seja `unknown`.
    let inputs = ["user name@127.0.0.1", "user name@[::1]"];
    assert_all(&inputs, AddressBarInputKind::Url);
}

#[test]
fn ambiguous_inputs_return_unknown() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let inputs = [
        "cats",              // palavra solta
        "intranet",          // palavra solta
        "wiki/page",         // host solto com caminho, sem barra no fim e sem TLD
        "c#",                // token único com fragmento
        "日本語",            // palavra Unicode solta, sem TLD
        "unknownscheme:foo", // esquema desconhecido que não parece userinfo
        "foo.invalid",       // reservado pela RFC 6761 como não navegável
    ];
    assert_all(&inputs, AddressBarInputKind::Unknown);
}

#[test]
fn input_is_trimmed_before_classification() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        vec![
            resolve_address_bar_input_type("   example.com"),
            resolve_address_bar_input_type("https://example.com   "),
            resolve_address_bar_input_type("\tlocalhost:3000\n"),
        ],
        vec![AddressBarInputKind::Url, AddressBarInputKind::Url, AddressBarInputKind::Url],
    );
}

#[test]
fn empty_whitespace_input_returns_empty() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        vec![
            resolve_address_bar_input_type(""),
            resolve_address_bar_input_type("   "),
            resolve_address_bar_input_type("\t\n"),
        ],
        vec![AddressBarInputKind::Empty, AddressBarInputKind::Empty, AddressBarInputKind::Empty],
    );
}

#[test]
fn unknown_scheme_that_looks_like_user_password_host_is_a_url() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(resolve_address_bar_input_type("user:pass@example.com"), AddressBarInputKind::Url);
}

#[test]
fn javascript_with_non_code_body_is_unknown() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(resolve_address_bar_input_type("javascript:hello"), AddressBarInputKind::Unknown);
    assert_eq!(resolve_address_bar_input_type("javascript:alert(1)"), AddressBarInputKind::Url);
}

#[test]
fn encodes_queries_for_each_engine() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let query = "hello world";
    let actual: Vec<String> = [
        BrowserSearchEngineId::Bing,
        BrowserSearchEngineId::Yahoo,
        BrowserSearchEngineId::Google,
        BrowserSearchEngineId::DuckDuckGo,
    ]
    .iter()
    .map(|engine| build_search_url(query, engine.as_str()))
    .collect();
    assert_eq!(
        actual,
        vec![
            "https://www.bing.com/search?q=hello+world",
            "https://search.yahoo.com/search?p=hello+world",
            "https://www.google.com/search?q=hello+world",
            "https://duckduckgo.com/?q=hello+world",
        ]
    );
}

#[test]
fn trims_and_collapses_internal_whitespace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        build_search_url("  spaced  query  ", BrowserSearchEngineId::Bing.as_str()),
        "https://www.bing.com/search?q=spaced+query"
    );
}

#[test]
fn encodes_special_characters() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let bing = BrowserSearchEngineId::Bing.as_str();
    assert_eq!(
        vec![
            build_search_url("wiki/page", bing),
            build_search_url("what is 2+2", bing),
            build_search_url("c#", bing),
            build_search_url("日本語", bing),
            build_search_url("a&b?c=d", bing),
            build_search_url("unknownscheme:foo", bing),
        ],
        vec![
            "https://www.bing.com/search?q=wiki%2Fpage",
            "https://www.bing.com/search?q=what+is+2%2B2",
            "https://www.bing.com/search?q=c%23",
            "https://www.bing.com/search?q=%E6%97%A5%E6%9C%AC%E8%AA%9E",
            "https://www.bing.com/search?q=a%26b%3Fc%3Dd",
            "https://www.bing.com/search?q=unknownscheme%3Afoo",
        ]
    );
}

#[test]
fn unknown_engine_id_falls_back_to_default_bing() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(build_search_url("cats", "nonexistent"), "https://www.bing.com/search?q=cats");
}

#[test]
fn returns_the_engine_label_falling_back_to_default_for_unknown_ids() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        vec![
            get_browser_search_engine_label(BrowserSearchEngineId::Bing.as_str()),
            get_browser_search_engine_label(BrowserSearchEngineId::Google.as_str()),
            get_browser_search_engine_label(BrowserSearchEngineId::DuckDuckGo.as_str()),
            get_browser_search_engine_label("nonexistent"),
        ],
        vec!["Bing", "Google", "DuckDuckGo", "Bing"]
    );
}
