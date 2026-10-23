//! Conferência do `Parser` e do `Renderer` do `marked` (`src/marked/html.rs`) contra o `marked.js` real (v14.0.0 do
//! VS Code) rodando no Node: o HTML que `marked.parse` gera para cada amostra do `marked_lexer.jsonl` (as mesmas
//! 2338 amostras, nos quatro modos `default`, `breaks`, `nogfm` e `pedantic`). Não é porte de teste do VS Code
//! (o `marked.js` é uma biblioteca de terceiros, sem teste na árvore do VS Code).
//!
//! O arquivo `golden/marked_html.jsonl` é gerado por `scripts/gen_marked_golden.mjs`; cada linha é
//! `{"mode", "src", "html"}` (ou `"error"`).

use je_base::JsString;
use je_base::json::Value;
use je_base::jsonc::json_parse;
use je_base::marked::{Options, escape, parse, parse_inline};

const GOLDEN: &str = include_str!("golden/marked_html.jsonl");

fn options(mode: &str) -> Options {
    match mode {
        "default" => Options::default(),
        "breaks" => Options { breaks: true, ..Options::default() },
        "nogfm" => Options { gfm: false, ..Options::default() },
        "pedantic" => Options { pedantic: true, ..Options::default() },
        other => panic!("modo desconhecido {other}"),
    }
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a JsString> {
    match value {
        Value::Object(o) => match o.get(name) {
            Some(Value::String(s)) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

#[test]
fn parse_matches_marked_js_html_on_every_sample() {
    let mut checked = 0;
    for line in GOLDEN.lines() {
        let line_units: Vec<u16> = line.encode_utf16().collect();
        let case = json_parse(&line_units).expect("JSON válido");
        let (mode, src) = (field(&case, "mode").unwrap().to_string_lossy(), field(&case, "src").unwrap());
        let actual = parse(src.units(), &options(&mode));
        match (field(&case, "html"), field(&case, "error")) {
            (Some(expected), _) => {
                let actual = actual.unwrap_or_else(|e| panic!("[{mode}] {src:?}: erro {e}"));
                assert_eq!(actual.to_string_lossy(), expected.to_string_lossy(), "[{mode}] fonte {:?}", src.to_string_lossy());
            }
            (None, Some(_)) => assert!(actual.is_err(), "[{mode}] {:?} devia errar", src.to_string_lossy()),
            (None, None) => panic!("linha sem html nem error"),
        }
        checked += 1;
    }
    assert_eq!(checked, 2338);
}

#[test]
fn escape_and_parse_inline_follow_the_original() {
    let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    assert_eq!(escape(&units("a & b < c > \"d\" 'e' &amp; &#39; &x"), false).to_string_lossy(), "a &amp; b &lt; c &gt; &quot;d&quot; &#39;e&#39; &amp; &#39; &amp;x");
    assert_eq!(escape(&units("&amp;"), true).to_string_lossy(), "&amp;amp;");
    assert_eq!(parse_inline(&units("**bold** and `code`"), &Options::default()).unwrap().to_string_lossy(), "<strong>bold</strong> and <code>code</code>");
}
