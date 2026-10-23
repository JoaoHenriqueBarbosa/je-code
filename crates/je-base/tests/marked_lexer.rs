//! Conferência do lexer do `marked` (`src/marked`) contra o `marked.js` real (v14.0.0 do VS Code) rodando no
//! Node: a árvore de tokens (`type`, `raw`, `text` e os campos de cada tipo) e o mapa `links` de cada amostra,
//! e o fonte e as flags de cada regex por modo.
//!
//! Os arquivos `golden/marked_lexer.jsonl` e `golden/marked_rules.jsonl` são gerados por
//! `scripts/gen_marked_golden.mjs` (amostras escritas à mão e amostras sorteadas com semente fixa). Cada linha
//! do primeiro é `{"mode", "src", "tokens", "links"}` (ou `"error"`); o `mode` é `default`, `breaks`, `nogfm`
//! ou `pedantic`.

use je_base::JsString;
use je_base::json::{Object, Value};
use je_base::jsonc::json_parse;
use je_base::marked::{Align, Lexer, MarkedError, Options, Token, TokensList, lexer_with};

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn string(text: &JsString) -> Value {
    Value::String(text.clone())
}

fn object(fields: Vec<(&str, Value)>) -> Value {
    let mut object = Object::new();
    for (key, value) in fields {
        object.insert(js(key), value);
    }
    Value::Object(object)
}

fn tokens(list: &[Token]) -> Value {
    Value::Array(list.iter().map(token).collect())
}

fn optional_string(text: &Option<JsString>) -> Value {
    text.as_ref().map_or(Value::Null, string)
}

fn align(align: Option<Align>) -> Value {
    align.map_or(Value::Null, |a| Value::String(js(a.as_str())))
}

fn cell(cell: &je_base::marked::TableCell) -> Value {
    object(vec![
        ("text", string(&cell.text)),
        ("tokens", tokens(&cell.tokens)),
        ("header", Value::Bool(cell.header)),
        ("align", align(cell.align)),
    ])
}

/// A mesma projeção canônica de `proj` em `gen_marked_golden.mjs`, com as chaves na mesma ordem.
fn token(token: &Token) -> Value {
    let mut fields: Vec<(&str, Value)> = vec![
        ("type", Value::String(js(token.type_name()))),
        ("raw", string(token.raw())),
    ];
    match token {
        Token::Space(_) | Token::Hr(_) | Token::Br(_) | Token::Def(_) => {}
        Token::Code(t) => {
            if t.indented {
                fields.push(("codeBlockStyle", Value::String(js("indented"))));
            }
            if let Some(lang) = &t.lang {
                fields.push(("lang", string(lang)));
            }
            fields.push(("text", string(&t.text)));
        }
        Token::Heading(t) => {
            fields.extend([
                ("depth", Value::Number(f64::from(t.depth))),
                ("text", string(&t.text)),
                ("tokens", tokens(&t.tokens)),
            ]);
        }
        Token::Table(t) => {
            fields.extend([
                ("align", Value::Array(t.align.iter().map(|a| align(*a)).collect())),
                ("header", Value::Array(t.header.iter().map(cell).collect())),
                (
                    "rows",
                    Value::Array(
                        t.rows
                            .iter()
                            .map(|r| Value::Array(r.iter().map(cell).collect()))
                            .collect(),
                    ),
                ),
            ]);
        }
        Token::Blockquote(t) => fields.extend([("text", string(&t.text)), ("tokens", tokens(&t.tokens))]),
        Token::List(t) => {
            fields.extend([
                ("ordered", Value::Bool(t.ordered)),
                (
                    "start",
                    t.start.map_or(Value::String(js("")), |n| Value::Number(f64::from(n))),
                ),
                ("loose", Value::Bool(t.loose)),
                ("items", tokens(&t.items)),
            ]);
        }
        Token::ListItem(t) => {
            fields.push(("task", Value::Bool(t.task)));
            if let Some(checked) = t.checked {
                fields.push(("checked", Value::Bool(checked)));
            }
            fields.extend([
                ("loose", Value::Bool(t.loose)),
                ("text", string(&t.text)),
                ("tokens", tokens(&t.tokens)),
            ]);
        }
        Token::Paragraph(t) => fields.extend([("text", string(&t.text)), ("tokens", tokens(&t.tokens))]),
        Token::Html(t) => {
            fields.push(("block", Value::Bool(t.block)));
            for (key, value) in [("pre", t.pre), ("inLink", t.in_link), ("inRawBlock", t.in_raw_block)] {
                if let Some(value) = value {
                    fields.push((key, Value::Bool(value)));
                }
            }
            fields.push(("text", string(&t.text)));
        }
        Token::Text(t) => {
            fields.push(("text", string(&t.text)));
            if let Some(children) = &t.tokens {
                fields.push(("tokens", tokens(children)));
            }
        }
        Token::Escape(t) => fields.push(("text", string(&t.text))),
        Token::Codespan(t) => fields.push(("text", string(&t.text))),
        Token::Image(t) => fields.extend([
            ("href", string(&t.href)),
            ("title", optional_string(&t.title)),
            ("text", string(&t.text)),
        ]),
        Token::Link(t) => fields.extend([
            ("href", string(&t.href)),
            ("title", optional_string(&t.title)),
            ("text", string(&t.text)),
            ("tokens", tokens(&t.tokens)),
        ]),
        Token::Strong(t) => fields.extend([("text", string(&t.text)), ("tokens", tokens(&t.tokens))]),
        Token::Em(t) => fields.extend([("text", string(&t.text)), ("tokens", tokens(&t.tokens))]),
        Token::Del(t) => fields.extend([("text", string(&t.text)), ("tokens", tokens(&t.tokens))]),
    }
    object(fields)
}

fn links(list: &TokensList) -> Value {
    let mut keys: Vec<&JsString> = list.links.keys().collect();
    keys.sort();
    Value::Array(
        keys.into_iter()
            .map(|key| {
                let definition = &list.links[key];
                Value::Array(vec![
                    string(key),
                    string(&definition.href),
                    optional_string(&definition.title),
                ])
            })
            .collect(),
    )
}

fn options(mode: &str) -> Options {
    let defaults = Options::default();
    match mode {
        "default" => defaults,
        "breaks" => Options {
            breaks: true,
            ..defaults
        },
        "nogfm" => Options { gfm: false, ..defaults },
        "pedantic" => Options {
            pedantic: true,
            ..defaults
        },
        other => panic!("modo desconhecido: {other}"),
    }
}

fn field<'a>(object: &'a Object, key: &str) -> &'a Value {
    object.get(key).unwrap_or_else(|| panic!("falta o campo {key}"))
}

fn as_string(value: &Value) -> &JsString {
    let Value::String(text) = value else {
        panic!("não é string: {value:?}")
    };
    text
}

#[test]
fn lexer_matches_marked_js_on_golden_samples() {
    let golden = include_str!("golden/marked_lexer.jsonl");
    let mut cases = 0;
    let mut token_total = 0;
    let mut failures: Vec<String> = Vec::new();
    for line in golden.lines() {
        let Value::Object(case) = json_parse(&line.encode_utf16().collect::<Vec<u16>>()).expect("linha JSON") else {
            panic!("linha não é objeto")
        };
        let mode = as_string(field(&case, "mode")).to_string_lossy();
        let src = as_string(field(&case, "src"));
        let result = lexer_with(src.units(), options(&mode));
        cases += 1;
        let mismatch = match (&result, case.get("error")) {
            (Err(error), Some(expected)) => {
                let expected = as_string(expected).to_string_lossy();
                let kind = match error {
                    MarkedError::InfiniteLoop { .. } => "Error",
                    MarkedError::TypeError(_) => "TypeError",
                    MarkedError::RangeError => "RangeError",
                    // O lexer nunca devolve um token que o `Parser` não conhece.
                    MarkedError::UnknownToken(_) => "Error",
                };
                (!expected.starts_with(kind) || (kind == "Error" && !expected.ends_with(&error.to_string())))
                    .then(|| format!("erro {error} no lugar de {expected}"))
            }
            (Err(error), None) => Some(format!("erro inesperado: {error}")),
            (Ok(_), Some(expected)) => Some(format!("faltou o erro {:?}", as_string(expected))),
            (Ok(list), None) => {
                token_total += count(&list.tokens);
                let actual = object(vec![("tokens", tokens(&list.tokens)), ("links", links(list))]);
                let expected = object(vec![
                    ("tokens", field(&case, "tokens").clone()),
                    ("links", field(&case, "links").clone()),
                ]);
                (actual != expected).then(|| format!("\n  esperado: {expected:?}\n  obtido:   {actual:?}"))
            }
        };
        if let Some(message) = mismatch {
            failures.push(format!("[{mode}] {src:?}: {message}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} de {cases} casos divergem; o primeiro:\n{}",
        failures.len(),
        failures[0]
    );
    // O golden cobre estes números (ver o resumo que `gen_marked_golden.mjs` imprime).
    assert!(cases >= 2000, "casos: {cases}");
    assert!(token_total >= 10000, "tokens: {token_total}");
    println!("{cases} casos, {token_total} tokens");
}

fn count(list: &[Token]) -> usize {
    list.iter()
        .map(|t| {
            let nested = match t {
                Token::Table(table) => table
                    .header
                    .iter()
                    .chain(table.rows.iter().flatten())
                    .map(|c| count(&c.tokens))
                    .sum(),
                Token::List(list) => count(&list.items),
                other => other.tokens().map_or(0, count),
            };
            1 + nested
        })
        .sum()
}

#[test]
fn regex_sources_match_marked_js_in_every_mode() {
    let golden = include_str!("golden/marked_rules.jsonl");
    let mut rules_checked = 0;
    for line in golden.lines() {
        let Value::Object(case) = json_parse(&line.encode_utf16().collect::<Vec<u16>>()).expect("linha JSON") else {
            panic!("linha não é objeto")
        };
        let mode = as_string(field(&case, "mode")).to_string_lossy();
        let Value::Array(expected) = field(&case, "rules") else {
            panic!("rules não é lista")
        };
        let actual = Lexer::rule_sources(&options(&mode));
        assert_eq!(actual.len(), expected.len(), "modo {mode}: número de regras");
        for (rule, (name, source, flags)) in expected.iter().zip(actual) {
            let Value::Array(rule) = rule else {
                panic!("regra não é lista")
            };
            let expected_name = as_string(&rule[0]).to_string_lossy();
            let expected_source = as_string(&rule[1]).to_string_lossy();
            let expected_flags = as_string(&rule[2]).to_string_lossy();
            assert_eq!(name, expected_name, "modo {mode}: nome");
            // O `.source` do V8 escapa `/` e a quebra de linha; a conferência normaliza os dois lados.
            let normalized = source.replace("\\/", "/").replace('\n', "\\n");
            assert_eq!(normalized, expected_source, "modo {mode}: fonte de {name}");
            assert_eq!(flags, expected_flags, "modo {mode}: flags de {name}");
            rules_checked += 1;
        }
    }
    assert_eq!(rules_checked, 4 * 32);
}

#[test]
fn lexer_returns_the_links_map_and_tokens_of_a_reference_definition() {
    let list = lexer_with(
        &"[A  b]: /u \"t\"\n\n[a b]".encode_utf16().collect::<Vec<u16>>(),
        Options::default(),
    )
    .unwrap();
    let definition = &list.links[&js("a b")];
    assert_eq!(
        (
            definition.href.to_string_lossy(),
            definition.title.as_ref().map(JsString::to_string_lossy)
        ),
        ("/u".to_owned(), Some("t".to_owned()))
    );
    assert_eq!(list.tokens.len(), 1);
}

#[test]
fn deep_nesting_is_a_range_error_instead_of_a_stack_overflow() {
    let source: Vec<u16> = format!("{}a", "> ".repeat(5000)).encode_utf16().collect();
    assert_eq!(
        lexer_with(&source, Options::default()).unwrap_err(),
        MarkedError::RangeError
    );
}
