//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalLinkParsing.ts`.
//!
//! Extrai possíveis links de uma linha com acesso só ao texto da linha e ao sistema operacional de
//! destino, ou seja, não valida se os caminhos existem.
//!
//! O texto é UTF-16 (`&[u16]`), como o `string` do original, e os índices contam unidades UTF-16. As
//! expressões regulares do original são ECMAScript e rodam no `zyarr`. O `lastIndex` das regex com `g`
//! (laço de `exec`, `matchAll`) é o de `je_base::js_regexp`.
//!
//! Notas de tradução:
//! - `IParsedLink`, `ILinkSuffix` e `ILinkPartialRange` perdem o prefixo `I`.
//! - `row`, `col`, `rowEnd` e `colEnd` são `Option<f64>`, o `number | undefined` do original (`parseInt`).
//! - A regex `^(?<prefix>['"]+)` e a `(?<bracket>[\[\(])(?![\]\)])` do original viram varreduras de
//!   unidades, que casam exatamente o mesmo texto.
//! - As regex ficam em `thread_local!` dentro de um [`Lazy`], porque o `Lazy` não é `Sync`.

use std::rc::Rc;
use std::thread::LocalKey;

use je_base::js_regexp::match_all;
use je_base::js_string::{JsString, index_of, starts_with_at, substring, trim};
use je_base::lazy::Lazy;
use je_base::platform::OperatingSystem;
use zyarr::{Match, RegExp};

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedLink {
    pub path: LinkPartialRange,
    pub prefix: Option<LinkPartialRange>,
    pub suffix: Option<LinkSuffix>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinkSuffix {
    pub row: Option<f64>,
    pub col: Option<f64>,
    pub row_end: Option<f64>,
    pub col_end: Option<f64>,
    pub suffix: LinkPartialRange,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinkPartialRange {
    pub index: usize,
    pub text: JsString,
}

type LazyRegex = Lazy<Rc<RegExp>>;

/// Um `Lazy` que compila `pattern()` com `flags` na primeira leitura.
fn lazy_regex(pattern: impl FnOnce() -> String + 'static, flags: &'static str) -> LazyRegex {
    Lazy::new(move || {
        Rc::new(RegExp::from_str(&pattern(), flags).expect("a regex do terminalLinkParsing é válida"))
    })
}

fn regex(lazy: &'static LocalKey<LazyRegex>) -> Rc<RegExp> {
    lazy.with(Lazy::value)
}

thread_local! {
    /// Extrai o sufixo do link com linha e coluna. O sufixo termina no fim da linha.
    static LINK_SUFFIX_REGEX_EOL: LazyRegex = lazy_regex(|| generate_link_suffix_regex(true), "");
    /// Extrai o sufixo do link com linha e coluna.
    static LINK_SUFFIX_REGEX: LazyRegex = lazy_regex(|| generate_link_suffix_regex(false), "g");
    // Define os caracteres válidos de um caminho com sufixo: o primeiro `[]` tem os caracteres com que o
    // caminho não pode _começar_, o segundo os que não são permitidos de jeito nenhum. Se o caractere
    // aparece nos dois, o link para nele, senão para no espaço.
    static LINK_WITH_SUFFIX_PATH_CHARACTERS: LazyRegex =
        lazy_regex(|| r"(?<path>(?:file:\/\/\/)?[^\s\|<>\[\({][^\s\|<>]*)$".to_string(), "");
    static UNIX_LOCAL_LINK: LazyRegex = lazy_regex(unix_local_link_clause, "g");
    static WIN_LOCAL_LINK: LazyRegex = lazy_regex(win_local_link_clause, "g");
    /// Os git diffs como `--- a/foo`, `+++ b/foo`.
    static GIT_DIFF_LINE_REGEX: LazyRegex = lazy_regex(|| format!(r"^[-+]{{3}} {DIFF_FILE_PREFIX}"), "");
    /// Os nomes de arquivo de linhas como `diff --git a/foo b/foo`, sem o prefixo.
    static GIT_DIFF_TEXT_REGEX: LazyRegex = lazy_regex(|| format!("^{DIFF_FILE_PREFIX}"), "");
}

fn generate_link_suffix_regex(eol_only: bool) -> String {
    // Cada cláusula usa o seu índice nos nomes dos grupos (`row0`, `row1`, `row2`...), como os
    // contadores `ri`, `ci`, `rei` e `cei` do original.
    let group = |name: &str, i: usize| format!(r"(?<{name}{i}>\d+)");
    let eol_suffix = if eol_only { "$" } else { "" };

    // Os comentários abaixo usam strings e números reais para ler melhor, a legenda é:
    // - Path    = foo
    // - Row     = 339
    // - Col     = 12
    // - RowEnd  = 341
    // - ColEnd  = 789
    //
    // Todos aceitam aspas simples ' no lugar de " e [] no lugar de ().
    //
    // Os testes têm a lista exaustiva dos formatos aceitos.
    let (r0, c0, re0, ce0) = (group("row", 0), group("col", 0), group("rowEnd", 0), group("colEnd", 0));
    let (r1, c1, re1, ce1) = (group("row", 1), group("col", 1), group("rowEnd", 1), group("colEnd", 1));
    let (r2, c2) = (group("row", 2), group("col", 2));
    let line_and_column_regex_clauses = [
        // foo:339
        // foo:339:12
        // foo:339:12-789
        // foo:339:12-341.789
        // foo:339.12
        // foo 339
        // foo 339:12                              [#140780]
        // foo 339.12
        // foo#339
        // foo#339:12                              [#190288]
        // foo#339.12
        // foo, 339                                [#217927]
        // "foo",339
        // "foo",339:12
        // "foo",339.12
        // "foo",339.12-789
        // "foo",339.12-341.789
        format!(r#"(?::|#| |['"],|, ){r0}([:.]{c0}(?:-(?:{re0}\.)?{ce0})?)?{eol_suffix}"#),
        // As aspas abaixo são opcionais           [#171652]
        // "foo", line 339                         [#40468]
        // "foo", line 339, col 12
        // "foo", line 339, column 12
        // "foo":line 339
        // "foo":line 339, col 12
        // "foo":line 339, column 12
        // "foo": line 339
        // "foo": line 339, col 12
        // "foo": line 339, column 12
        // "foo" on line 339
        // "foo" on line 339, col 12
        // "foo" on line 339, column 12
        // "foo" line 339 column 12
        // "foo", line 339, character 12           [#171880]
        // "foo", line 339, characters 12-789      [#171880]
        // "foo", lines 339-341                    [#171880]
        // "foo", lines 339-341, characters 12-789 [#178287]
        format!(
            r#"['"]?(?:,? |: ?| on )lines? {r1}(?:-{re1})?(?:,? (?:col(?:umn)?|characters?) {c1}(?:-{ce1})?)?{eol_suffix}"#
        ),
        // () e [] são intercambiáveis
        // foo(339)
        // foo(339,12)
        // foo(339, 12)
        // foo (339)
        // foo (339,12)
        // foo (339, 12)
        // foo: (339)
        // foo: (339,12)
        // foo: (339, 12)
        // foo(339:12)                             [#229842]
        // foo (339:12)                            [#229842]
        format!(r":? ?[\[\(]{r2}(?:(?:, ?|:){c2})?[\]\)]{eol_suffix}"),
    ];

    let suffix_clause = line_and_column_regex_clauses
        // Junta todas as cláusulas
        .join("|")
        // Converte os espaços para aceitar também o espaço não separável (ascii 160)
        .replace(' ', "[\u{00A0} ]");

    format!("({suffix_clause})")
}

/// Remove o sufixo opcional do link, que tem a informação de linha e coluna.
pub fn remove_link_suffix(link: &[u16]) -> JsString {
    match get_link_suffix(link) {
        Some(suffix) => JsString::from(substring(link, 0, suffix.suffix.index as i64)),
        None => JsString::from(link),
    }
}

/// Remove a query string do link, se houver.
pub fn remove_link_query_string(link: &[u16]) -> JsString {
    // Pula o ? dos caminhos UNC
    let start = if starts_with_at(link, &utf16(r"\\?\"), 0) { 4 } else { 0 };
    match index_of(link, &[u16::from(b'?')], start) {
        Some(index) => JsString::from(&link[..index]),
        None => JsString::from(link),
    }
}

pub fn detect_link_suffixes(line: &[u16]) -> Vec<LinkSuffix> {
    // Acha todos os sufixos da linha. Com a flag global o `lastIndex` avança no lugar, então os
    // casamentos não se sobrepõem.
    match_all(&regex(&LINK_SUFFIX_REGEX), line).iter().map(|found| to_link_suffix(line, found)).collect()
}

/// O sufixo opcional do link, que tem a informação de linha e coluna.
pub fn get_link_suffix(link: &[u16]) -> Option<LinkSuffix> {
    regex(&LINK_SUFFIX_REGEX_EOL).exec(link, 0).map(|found| to_link_suffix(link, &found))
}

/// `toLinkSuffix`: o sufixo de um casamento da regex de sufixo sobre `input`.
pub fn to_link_suffix(input: &[u16], found: &Match) -> LinkSuffix {
    let index = |name: &str| {
        (0..3).find_map(|i| found.group(&format!("{name}{i}"))).map(|range| parse_int(&input[range]))
    };
    let whole = found.captures[0].clone().unwrap_or_default();
    LinkSuffix {
        row: index("row"),
        col: index("col"),
        row_end: index("rowEnd"),
        col_end: index("colEnd"),
        suffix: LinkPartialRange { index: found.start(), text: JsString::from(&input[whole]) },
    }
}

/// `parseInt` de uma sequência de dígitos.
fn parse_int(digits: &[u16]) -> f64 {
    String::from_utf16_lossy(digits).parse().unwrap_or(f64::NAN)
}

pub fn detect_links(line: &[u16], os: OperatingSystem) -> Vec<ParsedLink> {
    // 1: Detecta todos os links da linha pelos sufixos primeiro
    let mut results = detect_links_via_suffix(line);

    // 2: Detecta todos os links sem sufixo e junta os intervalos sem conflito aos resultados
    let no_suffix_paths = detect_paths_no_suffix(line, os);
    binary_insert_list(&mut results, &no_suffix_paths);

    results
}

fn binary_insert_list(list: &mut Vec<ParsedLink>, new_items: &[ParsedLink]) {
    if list.is_empty() {
        list.extend_from_slice(new_items);
    }
    for item in new_items {
        binary_insert(list, item, 0, list.len() as i64);
    }
}

fn binary_insert(list: &mut Vec<ParsedLink>, new_item: &ParsedLink, low: i64, high: i64) {
    if list.is_empty() {
        list.push(new_item.clone());
        return;
    }
    if low > high {
        return;
    }
    // Acha o índice onde o `new_item` entraria
    let mid = ((low + high) / 2) as usize;
    let index = new_item.path.index;
    let end_of = |link: &ParsedLink| link.path.index + link.path.text.len();
    if mid >= list.len() || (index < list[mid].path.index && (mid == 0 || index > list[mid - 1].path.index)) {
        // Confere se conflita com um link existente antes de acrescentar
        if mid >= list.len()
            || (index + new_item.path.text.len() < list[mid].path.index
                && (mid == 0 || index > end_of(&list[mid - 1])))
        {
            // O `splice` do JavaScript limita o índice ao tamanho da lista
            list.insert(mid.min(list.len()), new_item.clone());
        }
        return;
    }
    if index > list[mid].path.index {
        binary_insert(list, new_item, mid as i64 + 1, high);
    } else {
        binary_insert(list, new_item, low, mid as i64 - 1);
    }
}

fn is_quote(unit: u16) -> bool {
    unit == u16::from(b'\'') || unit == u16::from(b'"')
}

fn detect_links_via_suffix(line: &[u16]) -> Vec<ParsedLink> {
    let mut results = Vec::new();

    // 1: Detecta os sufixos de link da linha
    for suffix in detect_link_suffixes(line) {
        // Ignora sufixos seguidos de `/`, para os prefixos numéricos de git diff como `1/` saírem
        // como caminhos.
        let suffix_end_index = suffix.suffix.index + suffix.suffix.text.len();
        if line.get(suffix_end_index) == Some(&u16::from(b'/')) {
            continue;
        }
        let before_suffix = &line[..suffix.suffix.index];
        let Some(possible_path_match) = regex(&LINK_WITH_SUFFIX_PATH_CHARACTERS).exec(before_suffix, 0) else {
            continue;
        };
        let Some(path_range) = possible_path_match.group("path").filter(|range| !range.is_empty()) else {
            continue;
        };
        let mut link_start_index = possible_path_match.start();
        let full_path = &before_suffix[path_range];
        // Extrai o prefixo do caminho, se existir (não é parte do caminho, mas é parte do trecho
        // sublinhado)
        let prefix_length = full_path.iter().take_while(|&&unit| is_quote(unit)).count();
        let (prefix_units, path) = full_path.split_at(prefix_length);
        let mut prefix: Option<LinkPartialRange> = None;
        if prefix_length > 0 {
            let mut found_prefix =
                LinkPartialRange { index: link_start_index, text: JsString::from(prefix_units) };

            // Não devolve links com sufixo quando o próprio link é a string vazia
            if trim(path).is_empty() {
                continue;
            }

            // Se o prefixo tem vários caracteres, apara o prefixo quando o _primeiro_ caractere do
            // sufixo é igual ao último do prefixo. Por exemplo, para o texto `echo "'foo' on line 1"`:
            //
            // - Prefixo='
            // - Caminho=foo
            // - Sufixo=' on line 1
            //
            // Se isso falha num prefixo de vários caracteres, mantém o original.
            if prefix_length > 1 {
                let last_prefix_unit = prefix_units[prefix_length - 1];
                if suffix.suffix.text.units().first().is_some_and(|&first| is_quote(first) && first == last_prefix_unit)
                {
                    let trim_prefix_amount = prefix_length - 1;
                    found_prefix.index += trim_prefix_amount;
                    found_prefix.text = JsString::from(&prefix_units[trim_prefix_amount..]);
                    link_start_index += trim_prefix_amount;
                }
            }
            prefix = Some(found_prefix);
        }
        let path_index = link_start_index + prefix.as_ref().map_or(0, |prefix| prefix.text.len());
        results.push(ParsedLink {
            path: LinkPartialRange { index: path_index, text: JsString::from(path) },
            prefix: prefix.clone(),
            suffix: Some(suffix.clone()),
        });

        // Se o caminho tem um colchete de abertura, devolve o caminho que começa logo depois do
        // colchete como resultado adicional
        for (position, &unit) in path.iter().enumerate() {
            let is_opening_bracket = unit == u16::from(b'[') || unit == u16::from(b'(');
            let is_followed_by_closing =
                path.get(position + 1).is_some_and(|&next| next == u16::from(b']') || next == u16::from(b')'));
            if is_opening_bracket && !is_followed_by_closing {
                results.push(ParsedLink {
                    path: LinkPartialRange {
                        index: path_index + position + 1,
                        text: JsString::from(&path[position + 1..]),
                    },
                    prefix: prefix.clone(),
                    suffix: Some(suffix.clone()),
                });
            }
        }
    }

    results
}

// '":; são permitidos em caminhos, mas costumam ser separadores, então são ignorados. Também proíbe
// `\\` para evitar um caso de backtracking catastrófico (#24795).
const PATH_PREFIX: &str = r"(?:\.\.?|\~|file://)";
const PATH_SEPARATOR_CLAUSE: &str = r"\/";
const EXCLUDED_PATH_CHARACTERS_CLAUSE: &str = r#"[^\0<>\?\s!`&*()'":;\\]"#;
const EXCLUDED_START_PATH_CHARACTERS_CLAUSE: &str = r#"[^\0<>\?\s!`&*()\[\]'":;\\]"#;

const WIN_OTHER_PATH_PREFIX: &str = r"\.\.?|\~";
const WIN_PATH_SEPARATOR_CLAUSE: &str = r"(?:\\|\/)";
const WIN_EXCLUDED_PATH_CHARACTERS_CLAUSE: &str = r#"[^\0<>\?\|\/\s!`&*()'":;]"#;
const WIN_EXCLUDED_START_PATH_CHARACTERS_CLAUSE: &str = r#"[^\0<>\?\|\/\s!`&*()\[\]'":;]"#;

/// Uma regex que casa caminhos fora do Windows, como `/foo`, `~/foo`, `./foo`, `../foo` e `foo/bar`.
fn unix_local_link_clause() -> String {
    format!(
        "(?:(?:{PATH_PREFIX}|(?:{EXCLUDED_START_PATH_CHARACTERS_CLAUSE}{EXCLUDED_PATH_CHARACTERS_CLAUSE}*))?(?:{PATH_SEPARATOR_CLAUSE}(?:{EXCLUDED_PATH_CHARACTERS_CLAUSE})+)+)"
    )
}

/// Uma cláusula de regex que casa o começo de um caminho absoluto no Windows, como `C:`, `c:`,
/// `file:///c:` (uri) e `\\?\C:` (caminho UNC).
pub const WIN_DRIVE_PREFIX: &str = r"(?:\\\\\?\\|file:\/\/\/)?[a-zA-Z]:";

/// Uma regex que casa caminhos do Windows, como `\\?\c:\foo`, `c:\foo`, `~\foo`, `.\foo`, `..\foo` e
/// `foo\bar`.
fn win_local_link_clause() -> String {
    format!(
        "(?:(?:(?:{WIN_DRIVE_PREFIX}|{WIN_OTHER_PATH_PREFIX})|(?:{WIN_EXCLUDED_START_PATH_CHARACTERS_CLAUSE}{WIN_EXCLUDED_PATH_CHARACTERS_CLAUSE}*))?(?:{WIN_PATH_SEPARATOR_CLAUSE}(?:{WIN_EXCLUDED_PATH_CHARACTERS_CLAUSE})+)+)"
    )
}

/// Os prefixos de uma letra que o git usa nos diffs. Com `diff.mnemonicPrefix` o git usa letras
/// mnemônicas e usa `1/` e `2/` no `git diff --no-index`.
const DIFF_FILE_PREFIX: &str = r"[abciow12]\/";

fn detect_paths_no_suffix(line: &[u16], os: OperatingSystem) -> Vec<ParsedLink> {
    let mut results = Vec::new();

    let regex = regex(if os == OperatingSystem::Windows { &WIN_LOCAL_LINK } else { &UNIX_LOCAL_LINK });
    let is_git_diff_line = regex_test(&GIT_DIFF_LINE_REGEX, line);
    let is_diff_git = starts_with_at(line, &utf16("diff --git"), 0);
    let mut last_index = 0;
    while let Some(found) = regex.exec(line, last_index) {
        last_index = found.end();
        let mut text = &line[found.captures[0].clone().unwrap_or_default()];
        let mut index = found.start();
        if text.is_empty() {
            // Casou algo que não cumpre o índice dado, o que seria um bug da própria regex, então
            // não fazemos nada aqui
            break;
        }

        // Ajusta o intervalo do link para excluir um prefixo conhecido de git diff
        if
        // --- a/foo/bar
        // +++ b/foo/bar
        (is_git_diff_line && index == 4)
            // diff --git a/foo/bar b/foo/bar
            || (is_diff_git && regex_test(&GIT_DIFF_TEXT_REGEX, text))
        {
            text = &text[2.min(text.len())..];
            index += 2;
        }

        results.push(ParsedLink {
            path: LinkPartialRange { index, text: JsString::from(text) },
            prefix: None,
            suffix: None,
        });
    }

    results
}

/// `regex.test(text)` de uma regex sem a flag global.
fn regex_test(lazy: &'static LocalKey<LazyRegex>, text: &[u16]) -> bool {
    regex(lazy).exec(text, 0).is_some()
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}
