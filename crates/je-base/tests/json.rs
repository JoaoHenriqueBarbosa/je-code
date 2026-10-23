//! Porte de `src/vs/base/test/common/json.test.ts` (suíte `JSON`), caso a caso e na mesma ordem.

use je_base::json::{
    self, NodeType, Object, ParseError, ParseErrorCode, ParseOptions, ScanError, Scanner, SyntaxKind, Tree, Value,
};
use je_base::json_error_messages::parse_error_message;
use SyntaxKind::*;

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn assert_kinds(text: &str, kinds: &[SyntaxKind]) {
    let units = utf16(text);
    let mut scanner = Scanner::new(&units, false);
    let mut expected = kinds.iter();
    loop {
        let kind = scanner.scan();
        if kind == EOF {
            break;
        }
        assert_eq!(Some(&kind), expected.next(), "texto {text:?}");
    }
    assert_eq!(expected.next(), None, "texto {text:?}");
}

fn assert_scan_error(text: &str, expected_kind: SyntaxKind, scan_error: ScanError) {
    let units = utf16(text);
    let mut scanner = Scanner::new(&units, false);
    scanner.scan();
    assert_eq!(scanner.token(), expected_kind);
    assert_eq!(scanner.token_error(), scan_error);
}

fn parse_with(input: &str, options: Option<ParseOptions>) -> (Option<Value>, Vec<ParseError>) {
    let mut errors = Vec::new();
    let actual = json::parse(&utf16(input), &mut errors, options.unwrap_or_default());
    (actual, errors)
}

fn assert_valid_parse(input: &str, expected: Value, options: Option<ParseOptions>) {
    let (actual, errors) = parse_with(input, options);
    if let Some(first) = errors.first() {
        panic!("{}", parse_error_message(first.error));
    }
    assert_eq!(actual, Some(expected), "texto {input:?}");
}

fn assert_invalid_parse(input: &str, expected: Value, options: Option<ParseOptions>) {
    let (actual, errors) = parse_with(input, options);
    assert!(!errors.is_empty(), "texto {input:?} deveria ter erro");
    assert_eq!(actual, Some(expected), "texto {input:?}");
}

/// O nó esperado nos testes de árvore, sem o pai (o original apaga `parent` antes de comparar).
#[derive(Debug, PartialEq)]
struct T {
    kind: NodeType,
    offset: i64,
    length: i64,
    colon_offset: Option<i64>,
    value: Option<Value>,
    children: Option<Vec<T>>,
}

fn leaf(kind: NodeType, offset: i64, length: i64, value: Value) -> T {
    T { kind, offset, length, colon_offset: None, value: Some(value), children: None }
}

fn node(kind: NodeType, offset: i64, length: i64, children: Vec<T>) -> T {
    T { kind, offset, length, colon_offset: None, value: None, children: Some(children) }
}

fn property(offset: i64, length: i64, colon_offset: i64, children: Vec<T>) -> T {
    T { kind: NodeType::Property, offset, length, colon_offset: Some(colon_offset), value: None, children: Some(children) }
}

fn snapshot(tree: &Tree, id: usize) -> T {
    let n = tree.node(id);
    // O `checkParent` do original: todo filho aponta para o pai.
    if let Some(children) = &n.children {
        for &child in children {
            assert_eq!(tree.node(child).parent, Some(id));
        }
    }
    T {
        kind: n.kind,
        offset: n.offset,
        length: n.length,
        colon_offset: n.colon_offset,
        value: n.value.clone(),
        children: n.children.as_ref().map(|c| c.iter().map(|&child| snapshot(tree, child)).collect()),
    }
}

fn assert_tree(input: &str, expected: T, expected_errors: &[ParseErrorCode], options: Option<ParseOptions>) {
    let mut errors = Vec::new();
    let tree = json::parse_tree(&utf16(input), &mut errors, options.unwrap_or_default());
    assert_eq!(errors.iter().map(|e| e.error).collect::<Vec<_>>(), expected_errors);
    let root = tree.root().expect("árvore sem raiz");
    assert_eq!(tree.node(root).parent, None);
    assert_eq!(snapshot(&tree, root), expected);
}

fn obj(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(entries.into_iter().collect::<Object>())
}

fn arr(items: Vec<Value>) -> Value {
    Value::Array(items)
}

fn num(n: f64) -> Value {
    Value::Number(n)
}

fn s(text: &str) -> Value {
    Value::from(text)
}

fn trailing(allow: bool) -> Option<ParseOptions> {
    Some(ParseOptions { allow_trailing_comma: allow, ..ParseOptions::default() })
}

#[test]
fn tokens() {
    assert_kinds("{", &[OpenBraceToken]);
    assert_kinds("}", &[CloseBraceToken]);
    assert_kinds("[", &[OpenBracketToken]);
    assert_kinds("]", &[CloseBracketToken]);
    assert_kinds(":", &[ColonToken]);
    assert_kinds(",", &[CommaToken]);
}

#[test]
fn comments() {
    assert_kinds("// this is a comment", &[LineCommentTrivia]);
    assert_kinds("// this is a comment\n", &[LineCommentTrivia, LineBreakTrivia]);
    assert_kinds("/* this is a comment*/", &[BlockCommentTrivia]);
    assert_kinds("/* this is a \r\ncomment*/", &[BlockCommentTrivia]);
    assert_kinds("/* this is a \ncomment*/", &[BlockCommentTrivia]);

    // unexpected end
    assert_kinds("/* this is a", &[BlockCommentTrivia]);
    assert_kinds("/* this is a \ncomment", &[BlockCommentTrivia]);

    // broken comment
    assert_kinds("/ ttt", &[Unknown, Trivia, Unknown]);
}

#[test]
fn strings() {
    assert_kinds("\"test\"", &[StringLiteral]);
    assert_kinds("\"\\\"\"", &[StringLiteral]);
    assert_kinds("\"\\/\"", &[StringLiteral]);
    assert_kinds("\"\\b\"", &[StringLiteral]);
    assert_kinds("\"\\f\"", &[StringLiteral]);
    assert_kinds("\"\\n\"", &[StringLiteral]);
    assert_kinds("\"\\r\"", &[StringLiteral]);
    assert_kinds("\"\\t\"", &[StringLiteral]);
    assert_kinds("\"\\v\"", &[StringLiteral]);
    assert_kinds("\"\u{88ff}\"", &[StringLiteral]);
    assert_kinds("\"\u{200b}\u{2028}\"", &[StringLiteral]);

    // unexpected end
    assert_kinds("\"test", &[StringLiteral]);
    assert_kinds("\"test\n\"", &[StringLiteral, LineBreakTrivia, StringLiteral]);

    // invalid characters
    assert_scan_error("\"\t\"", StringLiteral, ScanError::InvalidCharacter);
    assert_scan_error("\"\t \"", StringLiteral, ScanError::InvalidCharacter);
}

#[test]
fn numbers() {
    for text in ["0", "0.1", "-0.1", "-1", "1", "123456789", "10", "90", "90E+123", "90e+123", "90e-123", "90E-123", "90E123", "90e123"] {
        assert_kinds(text, &[NumericLiteral]);
    }

    // zero handling
    assert_kinds("01", &[NumericLiteral, NumericLiteral]);
    assert_kinds("-01", &[NumericLiteral, NumericLiteral]);

    // unexpected end
    assert_kinds("-", &[Unknown]);
    assert_kinds(".0", &[Unknown]);
}

#[test]
fn keywords_true_false_null() {
    assert_kinds("true", &[TrueKeyword]);
    assert_kinds("false", &[FalseKeyword]);
    assert_kinds("null", &[NullKeyword]);

    assert_kinds("true false null", &[TrueKeyword, Trivia, FalseKeyword, Trivia, NullKeyword]);

    // invalid words
    assert_kinds("nulllll", &[Unknown]);
    assert_kinds("True", &[Unknown]);
    assert_kinds("foo-bar", &[Unknown]);
    assert_kinds("foo bar", &[Unknown, Trivia, Unknown]);
}

#[test]
fn trivia() {
    assert_kinds(" ", &[Trivia]);
    assert_kinds("  \t  ", &[Trivia]);
    assert_kinds("  \t  \n  \t  ", &[Trivia, LineBreakTrivia, Trivia]);
    assert_kinds("\r\n", &[LineBreakTrivia]);
    assert_kinds("\r", &[LineBreakTrivia]);
    assert_kinds("\n", &[LineBreakTrivia]);
    assert_kinds("\n\r", &[LineBreakTrivia, LineBreakTrivia]);
    assert_kinds("\n   \n", &[LineBreakTrivia, Trivia, LineBreakTrivia]);
}

#[test]
fn parse_literals() {
    assert_valid_parse("true", Value::Bool(true), None);
    assert_valid_parse("false", Value::Bool(false), None);
    assert_valid_parse("null", Value::Null, None);
    assert_valid_parse("\"foo\"", s("foo"), None);
    assert_valid_parse("\"\\\"-\\\\-\\/-\\b-\\f-\\n-\\r-\\t\"", s("\"-\\-/-\u{8}-\u{c}-\n-\r-\t"), None);
    assert_valid_parse("\"\\u00DC\"", s("Ü"), None);
    assert_valid_parse("9", num(9.0), None);
    assert_valid_parse("-9", num(-9.0), None);
    assert_valid_parse("0.129", num(0.129), None);
    assert_valid_parse("23e3", num(23e3), None);
    assert_valid_parse("1.2E+3", num(1.2E+3), None);
    assert_valid_parse("1.2E-3", num(1.2E-3), None);
    assert_valid_parse("1.2E-3 // comment", num(1.2E-3), None);
}

#[test]
fn parse_objects() {
    assert_valid_parse("{}", obj(vec![]), None);
    assert_valid_parse("{ \"foo\": true }", obj(vec![("foo", Value::Bool(true))]), None);
    assert_valid_parse("{ \"bar\": 8, \"xoo\": \"foo\" }", obj(vec![("bar", num(8.0)), ("xoo", s("foo"))]), None);
    assert_valid_parse("{ \"hello\": [], \"world\": {} }", obj(vec![("hello", arr(vec![])), ("world", obj(vec![]))]), None);
    assert_valid_parse(
        "{ \"a\": false, \"b\": true, \"c\": [ 7.4 ] }",
        obj(vec![("a", Value::Bool(false)), ("b", Value::Bool(true)), ("c", arr(vec![num(7.4)]))]),
        None,
    );
    assert_valid_parse(
        "{ \"lineComment\": \"//\", \"blockComment\": [\"/*\", \"*/\"], \"brackets\": [ [\"{\", \"}\"], [\"[\", \"]\"], [\"(\", \")\"] ] }",
        obj(vec![
            ("lineComment", s("//")),
            ("blockComment", arr(vec![s("/*"), s("*/")])),
            ("brackets", arr(vec![arr(vec![s("{"), s("}")]), arr(vec![s("["), s("]")]), arr(vec![s("("), s(")")])])),
        ]),
        None,
    );
    assert_valid_parse("{ \"hello\": [], \"world\": {} }", obj(vec![("hello", arr(vec![])), ("world", obj(vec![]))]), None);
    assert_valid_parse(
        "{ \"hello\": { \"again\": { \"inside\": 5 }, \"world\": 1 }}",
        obj(vec![("hello", obj(vec![("again", obj(vec![("inside", num(5.0))])), ("world", num(1.0))]))]),
        None,
    );
    assert_valid_parse("{ \"foo\": /*hello*/true }", obj(vec![("foo", Value::Bool(true))]), None);
}

#[test]
fn parse_proto_own_property_is_preserved_without_polluting_prototype() {
    // Aqui não há protótipo: `__proto__` é uma chave própria como as outras, que é o que o teste
    // original exige (hasOwn verdadeiro, protótipo intacto, sem erro).
    let (actual, errors) = parse_with("{ \"__proto__\": { \"polluted\": true } }", None);
    let Some(Value::Object(object)) = actual else { panic!("esperava objeto") };
    assert!(object.contains_key("__proto__"));
    assert_eq!(object.get("__proto__"), Some(&obj(vec![("polluted", Value::Bool(true))])));
    assert!(errors.is_empty());
}

#[test]
fn parse_arrays() {
    assert_valid_parse("[]", arr(vec![]), None);
    assert_valid_parse("[ [],  [ [] ]]", arr(vec![arr(vec![]), arr(vec![arr(vec![])])]), None);
    assert_valid_parse("[ 1, 2, 3 ]", arr(vec![num(1.0), num(2.0), num(3.0)]), None);
    assert_valid_parse("[ { \"a\": null } ]", arr(vec![obj(vec![("a", Value::Null)])]), None);
}

#[test]
fn parse_objects_with_errors() {
    assert_invalid_parse("{,}", obj(vec![]), None);
    assert_invalid_parse("{ \"foo\": true, }", obj(vec![("foo", Value::Bool(true))]), trailing(false));
    assert_invalid_parse("{ \"bar\": 8 \"xoo\": \"foo\" }", obj(vec![("bar", num(8.0)), ("xoo", s("foo"))]), None);
    assert_invalid_parse("{ ,\"bar\": 8 }", obj(vec![("bar", num(8.0))]), None);
    assert_invalid_parse("{ ,\"bar\": 8, \"foo\" }", obj(vec![("bar", num(8.0))]), None);
    assert_invalid_parse("{ \"bar\": 8, \"foo\": }", obj(vec![("bar", num(8.0))]), None);
    assert_invalid_parse("{ 8, \"foo\": 9 }", obj(vec![("foo", num(9.0))]), None);
}

#[test]
fn parse_array_with_errors() {
    assert_invalid_parse("[,]", arr(vec![]), None);
    assert_invalid_parse("[ 1, 2, ]", arr(vec![num(1.0), num(2.0)]), trailing(false));
    assert_invalid_parse("[ 1 2, 3 ]", arr(vec![num(1.0), num(2.0), num(3.0)]), None);
    assert_invalid_parse("[ ,1, 2, 3 ]", arr(vec![num(1.0), num(2.0), num(3.0)]), None);
    assert_invalid_parse("[ ,1, 2, 3, ]", arr(vec![num(1.0), num(2.0), num(3.0)]), trailing(false));
}

#[test]
fn parse_disallow_commments() {
    let options = Some(ParseOptions { disallow_comments: true, ..ParseOptions::default() });

    assert_valid_parse("[ 1, 2, null, \"foo\" ]", arr(vec![num(1.0), num(2.0), Value::Null, s("foo")]), options);
    assert_valid_parse("{ \"hello\": [], \"world\": {} }", obj(vec![("hello", arr(vec![])), ("world", obj(vec![]))]), options);

    assert_invalid_parse("{ \"foo\": /*comment*/ true }", obj(vec![("foo", Value::Bool(true))]), options);
}

#[test]
fn parse_trailing_comma() {
    // default is allow
    assert_valid_parse("{ \"hello\": [], }", obj(vec![("hello", arr(vec![]))]), None);

    let options = trailing(true);
    assert_valid_parse("{ \"hello\": [], }", obj(vec![("hello", arr(vec![]))]), options);
    assert_valid_parse("{ \"hello\": [] }", obj(vec![("hello", arr(vec![]))]), options);
    assert_valid_parse("{ \"hello\": [], \"world\": {}, }", obj(vec![("hello", arr(vec![])), ("world", obj(vec![]))]), options);
    assert_valid_parse("{ \"hello\": [], \"world\": {} }", obj(vec![("hello", arr(vec![])), ("world", obj(vec![]))]), options);
    assert_valid_parse("{ \"hello\": [1,] }", obj(vec![("hello", arr(vec![num(1.0)]))]), options);

    let options = trailing(false);
    assert_invalid_parse("{ \"hello\": [], }", obj(vec![("hello", arr(vec![]))]), options);
    assert_invalid_parse("{ \"hello\": [], \"world\": {}, }", obj(vec![("hello", arr(vec![])), ("world", obj(vec![]))]), options);
}

#[test]
fn tree_literals() {
    use NodeType as N;
    assert_tree("true", leaf(N::Boolean, 0, 4, Value::Bool(true)), &[], None);
    assert_tree("false", leaf(N::Boolean, 0, 5, Value::Bool(false)), &[], None);
    assert_tree("null", leaf(N::Null, 0, 4, Value::Null), &[], None);
    assert_tree("23", leaf(N::Number, 0, 2, num(23.0)), &[], None);
    assert_tree("-1.93e-19", leaf(N::Number, 0, 9, num(-1.93e-19)), &[], None);
    assert_tree("\"hello\"", leaf(N::String, 0, 7, s("hello")), &[], None);
}

#[test]
fn tree_arrays() {
    use NodeType as N;
    assert_tree("[]", node(N::Array, 0, 2, vec![]), &[], None);
    assert_tree("[ 1 ]", node(N::Array, 0, 5, vec![leaf(N::Number, 2, 1, num(1.0))]), &[], None);
    assert_tree(
        "[ 1,\"x\"]",
        node(N::Array, 0, 8, vec![leaf(N::Number, 2, 1, num(1.0)), leaf(N::String, 4, 3, s("x"))]),
        &[],
        None,
    );
    assert_tree("[[]]", node(N::Array, 0, 4, vec![node(N::Array, 1, 2, vec![])]), &[], None);
}

#[test]
fn tree_objects() {
    use NodeType as N;
    assert_tree("{ }", node(N::Object, 0, 3, vec![]), &[], None);
    assert_tree(
        "{ \"val\": 1 }",
        node(N::Object, 0, 12, vec![property(2, 8, 7, vec![leaf(N::String, 2, 5, s("val")), leaf(N::Number, 9, 1, num(1.0))])]),
        &[],
        None,
    );
    assert_tree(
        "{\"id\": \"$\", \"v\": [ null, null] }",
        node(
            N::Object,
            0,
            32,
            vec![
                property(1, 9, 5, vec![leaf(N::String, 1, 4, s("id")), leaf(N::String, 7, 3, s("$"))]),
                property(
                    12,
                    18,
                    15,
                    vec![
                        leaf(N::String, 12, 3, s("v")),
                        node(N::Array, 17, 13, vec![leaf(N::Null, 19, 4, Value::Null), leaf(N::Null, 25, 4, Value::Null)]),
                    ],
                ),
            ],
        ),
        &[],
        None,
    );
    assert_tree(
        "{  \"id\": { \"foo\": { } } , }",
        node(
            N::Object,
            0,
            27,
            vec![property(
                3,
                20,
                7,
                vec![
                    leaf(N::String, 3, 4, s("id")),
                    node(
                        N::Object,
                        9,
                        14,
                        vec![property(11, 10, 16, vec![leaf(N::String, 11, 5, s("foo")), node(N::Object, 18, 3, vec![])])],
                    ),
                ],
            )],
        ),
        &[ParseErrorCode::PropertyNameExpected, ParseErrorCode::ValueExpected],
        trailing(false),
    );
}
