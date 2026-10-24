//! Testes do registro de classificação de tokens (`tokenClassificationRegistry.ts`): seletores, hierarquia de
//! tipos, tipos e modificadores padrão, `TokenStyle`, `SemanticTokenRule` e o esquema `tokenStylingSchema`.

use je_theme::color::Color;
use je_theme::token_classification_registry::{
    ClassifierString, SemanticTokenRule, TOKEN_STYLING_SCHEMA_ID, TYPE_AND_MODIFIER_ID_PATTERN, TokenClassificationError,
    TokenClassificationRegistry, TokenStyle, get_token_classification_registry, parse_classifier_string,
};
use serde_json::{Value, json};

const NO_MODIFIERS: [&str; 0] = [];

fn classifier(token_type: &str, modifiers: &[&str], language: Option<&str>) -> ClassifierString {
    ClassifierString {
        token_type: token_type.to_owned(),
        modifiers: modifiers.iter().map(|m| (*m).to_owned()).collect(),
        language: language.map(str::to_owned),
    }
}

#[test]
fn parse_classifier_string_reads_type_modifiers_and_language() {
    assert_eq!(parse_classifier_string("type", None), classifier("type", &[], None));
    assert_eq!(parse_classifier_string("*", None), classifier("*", &[], None));
    // os modificadores saem na ordem inversa à do texto
    assert_eq!(
        parse_classifier_string("type.modifier1.modifier2:lang", None),
        classifier("type", &["modifier2", "modifier1"], Some("lang"))
    );
    assert_eq!(parse_classifier_string("type", Some("ts")), classifier("type", &[], Some("ts")));
    assert_eq!(parse_classifier_string("type:js", Some("ts")), classifier("type", &[], Some("js")));
    assert_eq!(parse_classifier_string(".modifier", None), classifier("", &["modifier"], None));
}

#[test]
fn id_pattern_constants_and_validation() {
    assert_eq!(TYPE_AND_MODIFIER_ID_PATTERN.as_str(), r"^\w+[-_\w+]*$");
    assert_eq!(TOKEN_STYLING_SCHEMA_ID, "vscode://schemas/token-styling");

    let registry = TokenClassificationRegistry::new();
    assert_eq!(registry.register_token_type("a-b+c_1", "d", None, None), Ok(()));
    for invalid in ["", "-x", "a b", "a.b", "a:b", "ação"] {
        assert_eq!(registry.register_token_type(invalid, "d", None, None), Err(TokenClassificationError::InvalidTokenTypeId));
        assert_eq!(
            registry.register_token_modifier(invalid, "d", None),
            Err(TokenClassificationError::InvalidTokenModifierId)
        );
    }
    assert_eq!(
        registry.register_token_type("ok", "d", Some("not valid"), None),
        Err(TokenClassificationError::InvalidTokenSuperTypeId)
    );
    // um superType vazio é falso e não é validado
    assert_eq!(registry.register_token_type("ok", "d", Some(""), None), Ok(()));
    assert_eq!(TokenClassificationError::InvalidTokenTypeId.to_string(), "Invalid token type id.");
}

#[test]
fn selector_ids_sort_modifiers_and_keep_language() {
    let registry = TokenClassificationRegistry::new();
    assert_eq!(registry.parse_token_selector("variable.static.async", None).id, "variable.async.static");
    assert_eq!(registry.parse_token_selector("variable.readonly:ts", None).id, "variable.readonly:ts");
    assert_eq!(registry.parse_token_selector("variable", Some("js")).id, "variable:js");
    assert_eq!(registry.parse_token_selector("*.declaration", None).id, "*.declaration");
    assert_eq!(registry.parse_token_selector("", None).id, "$invalid");
    assert_eq!(registry.parse_token_selector(".foo", None).id, "$invalid");
}

#[test]
fn invalid_selector_matches_nothing() {
    let registry = get_token_classification_registry();
    let selector = registry.parse_token_selector("", None);
    assert_eq!(selector.matches("function", &NO_MODIFIERS, "ts"), -1);
}

#[test]
fn selector_scores_follow_the_type_hierarchy_modifiers_and_language() {
    let registry = get_token_classification_registry();
    let score = |selector: &str, token_type: &str, modifiers: &[&str], language: &str| {
        registry.parse_token_selector(selector, None).matches(token_type, modifiers, language)
    };

    assert_eq!(score("*", "anything", &NO_MODIFIERS, "ts"), 0);
    assert_eq!(score("function", "function", &NO_MODIFIERS, "ts"), 100);
    assert_eq!(score("function", "method", &NO_MODIFIERS, "ts"), -1);
    // `member` tem `method` como superType: o nível 1 vale 99
    assert_eq!(score("member", "member", &NO_MODIFIERS, "ts"), 100);
    assert_eq!(score("method", "member", &NO_MODIFIERS, "ts"), 99);
    assert_eq!(score("function.declaration", "function", &["declaration", "static"], "ts"), 200);
    assert_eq!(score("function.declaration", "function", &["static"], "ts"), -1);
    assert_eq!(score("*.declaration.static", "variable", &["static", "declaration"], "ts"), 200);
    assert_eq!(score("function:ts", "function", &NO_MODIFIERS, "ts"), 110);
    assert_eq!(score("function:ts", "function", &NO_MODIFIERS, "js"), -1);
    assert_eq!(score("*:ts", "variable", &NO_MODIFIERS, "ts"), 10);
}

#[test]
fn selector_uses_the_registry_that_parsed_it() {
    let registry = TokenClassificationRegistry::new();
    registry.register_token_type("base", "d", None, None).unwrap();
    registry.register_token_type("derived", "d", Some("base"), None).unwrap();
    let selector = registry.parse_token_selector("base", None);
    assert_eq!(selector.matches("derived", &NO_MODIFIERS, "x"), 99);

    // o cache de hierarquia é descartado quando os tipos mudam
    registry.deregister_token_type("base");
    assert_eq!(selector.matches("derived", &NO_MODIFIERS, "x"), 99);
    registry.register_token_type("derived", "d", None, None).unwrap();
    assert_eq!(selector.matches("derived", &NO_MODIFIERS, "x"), -1);
}

#[test]
fn hierarchy_stops_on_a_cycle() {
    let registry = TokenClassificationRegistry::new();
    registry.register_token_type("a", "d", Some("b"), None).unwrap();
    registry.register_token_type("b", "d", Some("a"), None).unwrap();
    assert_eq!(registry.parse_token_selector("b", None).matches("a", &NO_MODIFIERS, "x"), 99);
}

#[test]
fn default_registry_has_the_vscode_types_and_modifiers() {
    let registry = get_token_classification_registry();
    let types = registry.get_token_types();
    assert_eq!(types.len(), 24);
    assert_eq!((types[0].id.as_str(), types[0].num), ("comment", 0));
    assert_eq!((types[23].id.as_str(), types[23].num), ("label", 23));
    let member = types.iter().find(|t| t.id == "member").unwrap();
    assert_eq!(member.super_type.as_deref(), Some("method"));
    assert_eq!(member.deprecation_message.as_deref(), Some("Deprecated use `method` instead"));
    assert_eq!(member.description, "Style for member functions");

    let modifiers = registry.get_token_modifiers();
    let ids: Vec<&str> = modifiers.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        ["declaration", "documentation", "static", "abstract", "deprecated", "modification", "async", "readonly"]
    );
    let bits: Vec<u64> = modifiers.iter().map(|m| m.num).collect();
    assert_eq!(bits, [1, 2, 4, 8, 16, 32, 64, 128]);
}

#[test]
fn default_rules_follow_registration_order_with_scopes_to_probe() {
    let rules = get_token_classification_registry().get_token_styling_default_rules();
    assert_eq!(rules.len(), 24 + 11);
    assert_eq!(rules[0].selector.id, "comment");
    assert_eq!(rules[0].defaults.scopes_to_probe, Some(vec![vec!["comment".to_owned()]]));
    let type_rule = rules.iter().find(|r| r.selector.id == "type").unwrap();
    assert_eq!(
        type_rule.defaults.scopes_to_probe,
        Some(vec![vec!["entity.name.type".to_owned()], vec!["support.type".to_owned()]])
    );
    let member = rules.iter().find(|r| r.selector.id == "member").unwrap();
    assert_eq!(member.defaults.scopes_to_probe, Some(Vec::new()));
    let last = rules.last().unwrap();
    assert_eq!(last.selector.id, "member.defaultLibrary");
    assert_eq!(last.defaults.scopes_to_probe, Some(vec![vec!["support.function".to_owned()]]));
    assert!(rules.iter().any(|r| r.selector.id == "variable.defaultLibrary.readonly"));
    assert!(last.defaults.light.is_none() && last.defaults.dark.is_none());
}

#[test]
fn register_and_deregister_style_defaults() {
    let registry = TokenClassificationRegistry::new();
    registry.register_token_type("a", "d", None, None).unwrap();
    let selector = registry.parse_token_selector("a.static", None);
    registry.register_token_style_default(selector.clone(), Default::default());
    registry.register_token_style_default(registry.parse_token_selector("a", None), Default::default());
    assert_eq!(registry.get_token_styling_default_rules().len(), 2);
    registry.deregister_token_style_default(&selector);
    let rules = registry.get_token_styling_default_rules();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].selector.id, "a");
}

#[test]
fn token_types_and_modifiers_keep_registration_position_and_object_key_order() {
    let registry = TokenClassificationRegistry::new();
    registry.register_token_type("b", "first", None, None).unwrap();
    registry.register_token_type("10", "ten", None, None).unwrap();
    registry.register_token_type("a", "a", None, None).unwrap();
    registry.register_token_type("2", "two", None, None).unwrap();
    // re-registrar mantém a posição e troca a contribuição (o `num` avança)
    registry.register_token_type("b", "second", None, None).unwrap();

    let types = registry.get_token_types();
    let ids: Vec<&str> = types.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["2", "10", "b", "a"]);
    let b = types.iter().find(|t| t.id == "b").unwrap();
    assert_eq!((b.description.as_str(), b.num), ("second", 4));

    registry.deregister_token_type("10");
    assert_eq!(registry.get_token_types().len(), 3);

    registry.register_token_modifier("m", "d", Some("old")).unwrap();
    assert_eq!(registry.get_token_modifiers()[0].deprecation_message.as_deref(), Some("old"));
    registry.deregister_token_modifier("m");
    assert!(registry.get_token_modifiers().is_empty());
}

#[test]
fn display_lists_types_without_dots_first_then_alphabetically() {
    let registry = TokenClassificationRegistry::new();
    registry.register_token_type("zeta", "z", None, None).unwrap();
    registry.register_token_type("alpha", "a", None, None).unwrap();
    registry.register_token_type("alpha-b", "ab", None, None).unwrap();
    assert_eq!(registry.to_string(), "- `alpha`: a\n- `alpha-b`: ab\n- `zeta`: z");

    let all = get_token_classification_registry().to_string();
    assert!(all.starts_with("- `class`: Style for classes."));
    assert!(all.contains("- `typeParameter`: Style for type parameters."));
}

#[test]
fn schema_has_the_registered_properties_and_definitions() {
    let registry = get_token_classification_registry();
    let schema = registry.get_token_styling_schema();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"]["comment"]["description"], "Style for comments.");
    assert_eq!(schema["properties"]["member"]["deprecationMessage"], "Deprecated use `method` instead");
    assert_eq!(schema["properties"]["*.declaration"]["description"], "Style for all symbol declarations.");
    assert_eq!(schema["properties"]["comment"]["anyOf"][1]["$ref"], "#/definitions/style");
    assert_eq!(schema["properties"]["comment"]["defaultSnippets"][0]["body"], "${1:#ff0000}");

    let pattern_properties = schema["patternProperties"].as_object().unwrap();
    assert_eq!(pattern_properties.len(), 1);
    let (pattern, entry) = pattern_properties.iter().next().unwrap();
    assert_eq!(pattern, r"^(\w+[-_\w+]*|\*)(\.\w+[-_\w+]*)*(:\w+[-_\w+]*)?$");
    assert!(entry.get("description").is_none());

    let style = &schema["definitions"]["style"];
    assert_eq!(style["description"], "Colors and styles for the token.");
    assert_eq!(style["properties"]["foreground"]["default"], "#ff0000");
    assert_eq!(style["properties"]["fontStyle"]["pattern"], r"^(\s*(italic|bold|underline|strikethrough))*\s*$");
    let snippets = style["properties"]["fontStyle"]["defaultSnippets"].as_array().unwrap();
    assert_eq!(snippets.len(), 16);
    assert_eq!(snippets[0]["label"], "None (clear inherited style)");
    assert_eq!(snippets[0]["bodyText"], "\"\"");
    assert_eq!(snippets[15]["body"], "italic bold underline strikethrough");
    assert_eq!(style["defaultSnippets"][0]["body"]["fontStyle"], "${2:bold}");
}

#[test]
fn schema_follows_registrations_and_deregistrations() {
    let registry = TokenClassificationRegistry::new();
    assert!(registry.get_token_styling_schema()["properties"].as_object().unwrap().is_empty());
    registry.register_token_type("t", "type t", None, None).unwrap();
    registry.register_token_modifier("m", "modifier m", None).unwrap();
    let schema = registry.get_token_styling_schema();
    assert_eq!(schema["properties"]["t"]["description"], "type t");
    assert_eq!(schema["properties"]["*.m"]["description"], "modifier m");
    registry.deregister_token_type("t");
    registry.deregister_token_modifier("m");
    assert!(registry.get_token_styling_schema()["properties"].as_object().unwrap().is_empty());
}

fn style(foreground: Option<&str>, font_style: Option<&str>) -> TokenStyle {
    TokenStyle::from_settings(foreground, font_style, None, None, None, None)
}

#[test]
fn from_settings_parses_the_font_style_words() {
    let s = style(Some("#ff0000"), Some("italic bold"));
    assert!(Color::equals_opt(s.foreground.as_ref(), Some(&Color::from_hex("#ff0000"))));
    assert_eq!((s.bold, s.italic, s.underline, s.strikethrough), (Some(true), Some(true), Some(false), Some(false)));

    let s = style(None, Some("underline strikethrough"));
    assert!(s.foreground.is_none());
    assert_eq!((s.bold, s.italic, s.underline, s.strikethrough), (Some(false), Some(false), Some(true), Some(true)));

    // o texto vazio zera todos os estilos herdados
    let s = style(None, Some(""));
    assert_eq!((s.bold, s.italic, s.underline, s.strikethrough), (Some(false), Some(false), Some(false), Some(false)));

    // palavras casam em qualquer posição, como o regex global do original
    let s = style(None, Some("xxbold,italic!"));
    assert_eq!((s.bold, s.italic, s.underline, s.strikethrough), (Some(true), Some(true), Some(false), Some(false)));

    // sem fontStyle, os booleanos passam como vieram
    let s = TokenStyle::from_settings(None, None, Some(true), None, Some(false), Some(true));
    assert_eq!((s.bold, s.underline, s.strikethrough, s.italic), (Some(true), None, Some(false), Some(true)));

    // fontStyle vence os booleanos
    let s = TokenStyle::from_settings(None, Some("bold"), Some(false), Some(true), Some(true), Some(true));
    assert_eq!((s.bold, s.underline, s.strikethrough, s.italic), (Some(true), Some(false), Some(false), Some(false)));

    // cor inválida vira vermelho, como `Color.fromHex`
    let s = style(Some("not a color"), None);
    assert!(Color::equals_opt(s.foreground.as_ref(), Some(&Color::from_hex("#ff0000"))));
}

#[test]
fn token_style_equals_compares_color_and_flags() {
    let a = style(Some("#00ff00"), Some("bold"));
    let b = style(Some("#00ff00"), Some("bold"));
    assert!(TokenStyle::equals(Some(&a), Some(&a)));
    assert!(TokenStyle::equals(Some(&a), Some(&b)));
    assert!(TokenStyle::equals(None, None));
    assert!(!TokenStyle::equals(Some(&a), None));
    assert!(!TokenStyle::equals(None, Some(&a)));
    assert!(!TokenStyle::equals(Some(&a), Some(&style(Some("#00ff01"), Some("bold")))));
    assert!(!TokenStyle::equals(Some(&a), Some(&style(Some("#00ff00"), Some("italic")))));
    assert!(!TokenStyle::equals(Some(&a), Some(&style(None, Some("bold")))));
    assert!(!TokenStyle::equals(Some(&style(None, None)), Some(&a)));
    assert!(TokenStyle::equals(Some(&style(None, None)), Some(&TokenStyle::default())));
}

#[test]
fn token_style_json_round_trip() {
    let original = TokenStyle::from_settings(Some("#336699"), None, Some(true), None, Some(false), None);
    let json = original.to_json_object();
    assert!(json["_foreground"].is_string());
    assert_eq!(json["_bold"], true);
    assert_eq!(json["_underline"], Value::Null);
    assert_eq!(json["_strikethrough"], false);
    assert_eq!(json["_italic"], Value::Null);
    let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["_foreground", "_bold", "_underline", "_italic", "_strikethrough"]);

    let restored = TokenStyle::from_json_object(&json).unwrap();
    assert!(TokenStyle::equals(Some(&original), Some(&restored)));

    assert!(TokenStyle::from_json_object(&Value::Null).is_none());
    assert!(TokenStyle::from_json_object(&json!(false)).is_none());
    assert!(TokenStyle::from_json_object(&json!("")).is_none());
    // valores de tipo errado viram `undefined`
    let loose = TokenStyle::from_json_object(&json!({ "_foreground": 1, "_bold": "yes", "_italic": true })).unwrap();
    assert!(loose.foreground.is_none() && loose.bold.is_none());
    assert_eq!(loose.italic, Some(true));
}

#[test]
fn semantic_token_rule_json_round_trip_and_equals() {
    let registry = get_token_classification_registry();
    let rule = SemanticTokenRule {
        selector: registry.parse_token_selector("variable.static.readonly", None),
        style: style(Some("#112233"), Some("italic")),
    };
    let json = rule.to_json_object();
    assert_eq!(json["_selector"], "variable.readonly.static");

    let restored = SemanticTokenRule::from_json_object(registry, &json).unwrap();
    assert!(SemanticTokenRule::equals(Some(&rule), Some(&restored)));
    assert!(SemanticTokenRule::equals(Some(&rule), Some(&rule)));
    assert!(SemanticTokenRule::equals(None, None));
    assert!(!SemanticTokenRule::equals(Some(&rule), None));

    let other_selector = SemanticTokenRule { selector: registry.parse_token_selector("variable", None), style: rule.style.clone() };
    assert!(!SemanticTokenRule::equals(Some(&rule), Some(&other_selector)));
    let other_style = SemanticTokenRule { selector: rule.selector.clone(), style: style(Some("#112233"), Some("bold")) };
    assert!(!SemanticTokenRule::equals(Some(&rule), Some(&other_style)));

    assert!(SemanticTokenRule::from_json_object(registry, &Value::Null).is_none());
    assert!(SemanticTokenRule::from_json_object(registry, &json!({ "_selector": 1, "_style": {} })).is_none());
    assert!(SemanticTokenRule::from_json_object(registry, &json!({ "_selector": "a" })).is_none());
    assert!(SemanticTokenRule::from_json_object(registry, &json!({ "_selector": "a", "_style": null })).is_none());
}
