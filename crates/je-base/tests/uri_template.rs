//! Porte de `src/vs/base/test/common/uriTemplate.test.ts` (suíte `UriTemplate`), caso a caso e na mesma ordem.
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite` não entra, porque o `UriTemplate` não cria `IDisposable`.

use je_base::dynamic::Dynamic;
use je_base::uri_template::{UriTemplate, UriTemplateComponent, UriTemplateExpression, UriTemplateVariable};

fn var(name: &str, explodable: bool, prefix_length: Option<usize>) -> UriTemplateVariable {
    UriTemplateVariable { explodable, name: name.into(), optional: false, prefix_length, repeatable: explodable }
}

fn expr(expression: &str, operator: &str, variables: Vec<UriTemplateVariable>) -> UriTemplateExpression {
    UriTemplateExpression { expression: expression.into(), operator: operator.into(), variables }
}

/// Helper function to test template parsing and component extraction
fn test_parsing(template: &str, expected_components: Vec<UriTemplateExpression>) -> UriTemplate {
    let templ = UriTemplate::parse(template);
    let objects: Vec<UriTemplateExpression> = templ
        .components
        .iter()
        .filter_map(|c| match c {
            UriTemplateComponent::Expression(e) => Some(e.clone()),
            UriTemplateComponent::Literal(_) => None,
        })
        .collect();
    assert_eq!(objects, expected_components);
    templ
}

fn s(text: &str) -> Dynamic {
    Dynamic::string(text)
}

fn list(items: &[&str]) -> Dynamic {
    Dynamic::array(items.iter().map(|i| s(i)))
}

fn record(entries: Vec<(&str, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

/// Helper function to test template resolution
fn test_resolution(template: &str, variables: &Dynamic, expected: &str) {
    let templ = UriTemplate::parse(template);
    assert_eq!(templ.resolve(variables), expected, "template {template}");
}

fn keys() -> Dynamic {
    record(vec![("semi", s(";")), ("dot", s(".")), ("comma", s(","))])
}

#[test]
fn simple_replacement() {
    let templ = UriTemplate::parse("http://example.com/{var}");
    assert_eq!(
        templ.components,
        vec![
            UriTemplateComponent::Literal("http://example.com/".into()),
            UriTemplateComponent::Expression(expr("{var}", "", vec![var("var", false, None)])),
            UriTemplateComponent::Literal(String::new()),
        ]
    );
    let result = templ.resolve(&record(vec![("var", s("value"))]));
    assert_eq!(result, "http://example.com/value");
}

#[test]
fn parsing_components_correctly() {
    // Simple component
    test_parsing("http://example.com/{var}", vec![expr("{var}", "", vec![var("var", false, None)])]);

    // Component with operator
    test_parsing("http://example.com/{+path}", vec![expr("{+path}", "+", vec![var("path", false, None)])]);

    // Component with multiple variables
    test_parsing(
        "http://example.com/{x,y}",
        vec![expr("{x,y}", "", vec![var("x", false, None), var("y", false, None)])],
    );

    // Component with value modifiers
    test_parsing("http://example.com/{var:3}", vec![expr("{var:3}", "", vec![var("var", false, Some(3))])]);

    test_parsing("http://example.com/{list*}", vec![expr("{list*}", "", vec![var("list", true, None)])]);

    // Multiple components
    test_parsing(
        "http://example.com/{x}/path/{y}",
        vec![expr("{x}", "", vec![var("x", false, None)]), expr("{y}", "", vec![var("y", false, None)])],
    );
}

#[test]
fn level_1_simple_string_expansion() {
    // Test cases from RFC 6570 Section 1.2
    let variables = record(vec![("var", s("value")), ("hello", s("Hello World!"))]);

    test_resolution("{var}", &variables, "value");
    test_resolution("{hello}", &variables, "Hello%20World%21");
}

#[test]
fn control_characters_are_percent_encoded_with_two_hex_digits() {
    // Code points below 0x10 must be zero-padded (e.g. %09, not %9) so the
    // output is a valid percent-encoding that decodeURIComponent accepts.
    test_resolution("{x}", &record(vec![("x", s("a\tb"))]), "a%09b");
    test_resolution("{x}", &record(vec![("x", s("\n"))]), "%0A");
    test_resolution("{x}", &record(vec![("x", s("\r"))]), "%0D");
}

#[test]
fn level_2_reserved_expansion() {
    // Test cases from RFC 6570 Section 1.2
    let variables = record(vec![("var", s("value")), ("hello", s("Hello World!")), ("path", s("/foo/bar"))]);

    test_resolution("{+var}", &variables, "value");
    test_resolution("{+hello}", &variables, "Hello%20World!");
    test_resolution("{+path}/here", &variables, "/foo/bar/here");
    test_resolution("here?ref={+path}", &variables, "here?ref=/foo/bar");
}

#[test]
fn level_2_fragment_expansion() {
    // Test cases from RFC 6570 Section 1.2
    let variables = record(vec![("var", s("value")), ("hello", s("Hello World!"))]);

    test_resolution("X{#var}", &variables, "X#value");
    test_resolution("X{#hello}", &variables, "X#Hello%20World!");
}

#[test]
fn level_3_string_expansion_with_multiple_variables() {
    // Test cases from RFC 6570 Section 1.2
    let variables = record(vec![
        ("var", s("value")),
        ("hello", s("Hello World!")),
        ("empty", s("")),
        ("path", s("/foo/bar")),
        ("x", s("1024")),
        ("y", s("768")),
    ]);

    test_resolution("map?{x,y}", &variables, "map?1024,768");
    test_resolution("{x,hello,y}", &variables, "1024,Hello%20World%21,768");
}

#[test]
fn level_3_reserved_expansion_with_multiple_variables() {
    // Test cases from RFC 6570 Section 1.2
    let variables = record(vec![
        ("var", s("value")),
        ("hello", s("Hello World!")),
        ("path", s("/foo/bar")),
        ("x", s("1024")),
        ("y", s("768")),
    ]);

    test_resolution("{+x,hello,y}", &variables, "1024,Hello%20World!,768");
    test_resolution("{+path,x}/here", &variables, "/foo/bar,1024/here");
}

#[test]
fn level_3_fragment_expansion_with_multiple_variables() {
    // Test cases from RFC 6570 Section 1.2
    let variables = record(vec![
        ("var", s("value")),
        ("hello", s("Hello World!")),
        ("path", s("/foo/bar")),
        ("x", s("1024")),
        ("y", s("768")),
    ]);

    test_resolution("{#x,hello,y}", &variables, "#1024,Hello%20World!,768");
    test_resolution("{#path,x}/here", &variables, "#/foo/bar,1024/here");
}

#[test]
fn level_3_label_expansion_with_dot_prefix() {
    // Test cases from RFC 6570 Section 1.2
    let variables = record(vec![("var", s("value")), ("x", s("1024")), ("y", s("768"))]);

    test_resolution("X{.var}", &variables, "X.value");
    test_resolution("X{.x,y}", &variables, "X.1024.768");
}

#[test]
fn level_3_path_segments_expansion() {
    // Test cases from RFC 6570 Section 1.2
    let variables = record(vec![("var", s("value")), ("x", s("1024"))]);

    test_resolution("{/var}", &variables, "/value");
    test_resolution("{/var,x}/here", &variables, "/value/1024/here");
}

#[test]
fn level_3_path_style_parameter_expansion() {
    // Test cases from RFC 6570 Section 1.2
    let variables = record(vec![("x", s("1024")), ("y", s("768")), ("empty", s(""))]);

    test_resolution("{;x,y}", &variables, ";x=1024;y=768");
    test_resolution("{;x,y,empty}", &variables, ";x=1024;y=768;empty");
}

#[test]
fn level_3_form_style_query_expansion() {
    // Test cases from RFC 6570 Section 1.2
    let variables = record(vec![("x", s("1024")), ("y", s("768")), ("empty", s(""))]);

    test_resolution("{?x,y}", &variables, "?x=1024&y=768");
    test_resolution("{?x,y,empty}", &variables, "?x=1024&y=768&empty=");
}

#[test]
fn level_3_form_style_query_continuation() {
    // Test cases from RFC 6570 Section 1.2
    let variables = record(vec![("x", s("1024")), ("y", s("768")), ("empty", s(""))]);

    test_resolution("?fixed=yes{&x}", &variables, "?fixed=yes&x=1024");
    test_resolution("{&x,y,empty}", &variables, "&x=1024&y=768&empty=");
}

fn level_4_variables() -> Dynamic {
    record(vec![
        ("var", s("value")),
        ("hello", s("Hello World!")),
        ("path", s("/foo/bar")),
        ("list", list(&["red", "green", "blue"])),
        ("keys", keys()),
    ])
}

#[test]
fn level_4_string_expansion_with_value_modifiers() {
    // Test cases from RFC 6570 Section 1.2
    let variables = level_4_variables();

    test_resolution("{var:3}", &variables, "val");
    test_resolution("{var:30}", &variables, "value");
    test_resolution("{list}", &variables, "red,green,blue");
    test_resolution("{list*}", &variables, "red,green,blue");
}

#[test]
fn level_4_reserved_expansion_with_value_modifiers() {
    // Test cases related to Level 4 features
    let variables = level_4_variables();

    test_resolution("{+path:6}/here", &variables, "/foo/b/here");
    test_resolution("{+list}", &variables, "red,green,blue");
    test_resolution("{+list*}", &variables, "red,green,blue");
    test_resolution("{+keys}", &variables, "semi,;,dot,.,comma,,");
    test_resolution("{+keys*}", &variables, "semi=;,dot=.,comma=,");
}

#[test]
fn level_4_fragment_expansion_with_value_modifiers() {
    // Test cases related to Level 4 features
    let variables = level_4_variables();

    test_resolution("{#path:6}/here", &variables, "#/foo/b/here");
    test_resolution("{#list}", &variables, "#red,green,blue");
    test_resolution("{#list*}", &variables, "#red,green,blue");
    test_resolution("{#keys}", &variables, "#semi,;,dot,.,comma,,");
    test_resolution("{#keys*}", &variables, "#semi=;,dot=.,comma=,");
}

#[test]
fn level_4_label_expansion_with_value_modifiers() {
    // Test cases related to Level 4 features
    let variables = record(vec![("var", s("value")), ("list", list(&["red", "green", "blue"])), ("keys", keys())]);

    test_resolution("X{.var:3}", &variables, "X.val");
    test_resolution("X{.list}", &variables, "X.red,green,blue");
    test_resolution("X{.list*}", &variables, "X.red.green.blue");
    test_resolution("X{.keys}", &variables, "X.semi,;,dot,.,comma,,");
    test_resolution("X{.keys*}", &variables, "X.semi=;.dot=..comma=,");
}

#[test]
fn level_4_path_expansion_with_value_modifiers() {
    // Test cases related to Level 4 features
    let variables = record(vec![
        ("var", s("value")),
        ("list", list(&["red", "green", "blue"])),
        ("path", s("/foo/bar")),
        ("keys", keys()),
    ]);

    test_resolution("{/var:1,var}", &variables, "/v/value");
    test_resolution("{/list}", &variables, "/red,green,blue");
    test_resolution("{/list*}", &variables, "/red/green/blue");
    test_resolution("{/list*,path:4}", &variables, "/red/green/blue/%2Ffoo");
    test_resolution("{/keys}", &variables, "/semi,;,dot,.,comma,,");
    test_resolution("{/keys*}", &variables, "/semi=%3B/dot=./comma=%2C");
}

#[test]
fn level_4_path_style_parameters_with_value_modifiers() {
    // Test cases related to Level 4 features
    let variables = record(vec![("var", s("value")), ("list", list(&["red", "green", "blue"])), ("keys", keys())]);

    test_resolution("{;hello:5}", &record(vec![("hello", s("Hello World!"))]), ";hello=Hello");
    test_resolution("{;list}", &variables, ";list=red,green,blue");
    test_resolution("{;list*}", &variables, ";list=red;list=green;list=blue");
    test_resolution("{;keys}", &variables, ";keys=semi,;,dot,.,comma,,");
    test_resolution("{;keys*}", &variables, ";semi=;;dot=.;comma=,");
}

#[test]
fn level_4_form_style_query_with_value_modifiers() {
    // Test cases related to Level 4 features
    let variables = record(vec![("var", s("value")), ("list", list(&["red", "green", "blue"])), ("keys", keys())]);

    test_resolution("{?var:3}", &variables, "?var=val");
    test_resolution("{?list}", &variables, "?list=red,green,blue");
    test_resolution("{?list*}", &variables, "?list=red&list=green&list=blue");
    test_resolution("{?keys}", &variables, "?keys=semi,;,dot,.,comma,,");
    test_resolution("{?keys*}", &variables, "?semi=;&dot=.&comma=,");
}

#[test]
fn level_4_form_style_query_continuation_with_value_modifiers() {
    // Test cases related to Level 4 features
    let variables = record(vec![("var", s("value")), ("list", list(&["red", "green", "blue"])), ("keys", keys())]);

    test_resolution("?fixed=yes{&var:3}", &variables, "?fixed=yes&var=val");
    test_resolution("?fixed=yes{&list}", &variables, "?fixed=yes&list=red,green,blue");
    test_resolution("?fixed=yes{&list*}", &variables, "?fixed=yes&list=red&list=green&list=blue");
    test_resolution("?fixed=yes{&keys}", &variables, "?fixed=yes&keys=semi,;,dot,.,comma,,");
    test_resolution("?fixed=yes{&keys*}", &variables, "?fixed=yes&semi=;&dot=.&comma=,");
}

#[test]
fn handling_undefined_or_null_values() {
    // Test handling of undefined/null values for different operators
    let variables = record(vec![
        ("defined", s("value")),
        ("undef", Dynamic::Undefined),
        ("null", Dynamic::Null),
        ("empty", s("")),
    ]);

    // Simple string expansion
    test_resolution("{defined,undef,null,empty}", &variables, "value,");

    // Reserved expansion
    test_resolution("{+defined,undef,null,empty}", &variables, "value,");

    // Fragment expansion
    test_resolution("{#defined,undef,null,empty}", &variables, "#value,");

    // Label expansion
    test_resolution("X{.defined,undef,null,empty}", &variables, "X.value");

    // Path segments
    test_resolution("{/defined,undef,null}", &variables, "/value");

    // Path-style parameters
    test_resolution("{;defined,empty}", &variables, ";defined=value;empty");

    // Form-style query
    test_resolution("{?defined,undef,null,empty}", &variables, "?defined=value&undef=&null=&empty=");

    // Form-style query continuation
    test_resolution("{&defined,undef,null,empty}", &variables, "&defined=value&undef=&null=&empty=");
}

#[test]
fn complex_templates() {
    // Test more complex template combinations
    let variables = record(vec![
        ("domain", s("example.com")),
        ("user", s("fred")),
        ("path", list(&["path", "to", "resource"])),
        ("query", s("search")),
        ("page", Dynamic::Number(5.0)),
        ("lang", s("en")),
        ("sessionId", s("123abc")),
        ("filters", list(&["color:blue", "shape:square"])),
        ("coordinates", record(vec![("lat", s("37.7")), ("lon", s("-122.4"))])),
    ]);

    // RESTful URL pattern
    test_resolution(
        "https://{domain}/api/v1/users/{user}{/path*}{?query,page,lang}",
        &variables,
        "https://example.com/api/v1/users/fred/path/to/resource?query=search&page=5&lang=en",
    );

    // Complex query parameters
    test_resolution(
        "https://{domain}/search{?query,filters,coordinates*}",
        &variables,
        "https://example.com/search?query=search&filters=color:blue,shape:square&lat=37.7&lon=-122.4",
    );

    // Multiple expression types
    test_resolution(
        "https://{domain}/users/{user}/profile{.lang}{?sessionId}{#path}",
        &variables,
        "https://example.com/users/fred/profile.en?sessionId=123abc#path,to,resource",
    );
}

#[test]
fn literals_and_escaping() {
    // Test literal segments and escaping
    test_parsing("http://example.com/literal", vec![]);
    test_parsing(
        "http://example.com/{var}literal{var2}",
        vec![expr("{var}", "", vec![var("var", false, None)]), expr("{var2}", "", vec![var("var2", false, None)])],
    );

    // Test that escaped braces are treated as literals
    // Note: The current implementation might not handle this case
    test_resolution("http://example.com/{{var}}", &record(vec![("var", s("value"))]), "http://example.com/{var}");
}

#[test]
fn edge_cases() {
    // Empty template
    test_resolution("", &record(vec![]), "");

    // Template with only literals
    test_resolution("http://example.com/path", &record(vec![]), "http://example.com/path");

    // No variables provided for resolution
    test_resolution("{var}", &record(vec![]), "");

    // Multiple sequential expressions
    test_resolution("{a}{b}{c}", &record(vec![("a", s("1")), ("b", s("2")), ("c", s("3"))]), "123");

    // Expressions with special characters in variable names
    test_resolution("{_hidden.var-name$}", &record(vec![("_hidden.var-name$", s("value"))]), "value");
}
