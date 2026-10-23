//! Porte de `src/vs/base/test/common/jsonSchema.test.ts` (suíte `JSON Schema`), caso a caso e na mesma ordem.

use je_base::dynamic::Dynamic;
use je_base::json::{self, Value};
use je_base::json_schema::get_compressed_content;

fn schema(text: &str) -> Dynamic {
    let mut errors = Vec::new();
    let value: Value = json::parse_str(text, &mut errors).expect("JSON válido");
    assert!(errors.is_empty());
    Dynamic::from_json(&value)
}


#[test]
fn get_compressed_content_1() {
    let schema_value = schema(
        "{\"type\":\"object\",\"properties\":{\"a\":{\"type\":\"object\",\"description\":\"a\",\"properties\":{\"b\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}}},\"e\":{\"type\":\"object\",\"description\":\"e\",\"properties\":{\"b\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}}}}}",
    );
    let expected = "{\"type\":\"object\",\"properties\":{\"a\":{\"type\":\"object\",\"description\":\"a\",\"properties\":{\"b\":{\"$ref\":\"#/$defs/_0\"}}},\"e\":{\"type\":\"object\",\"description\":\"e\",\"properties\":{\"b\":{\"$ref\":\"#/$defs/_0\"}}}},\"$defs\":{\"_0\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}}}";
    assert_eq!(get_compressed_content(&schema_value), expected);
}

#[test]
fn get_compressed_content_2() {
    let schema_value = schema(
        "{\"type\":\"object\",\"properties\":{\"a\":{\"type\":\"object\",\"properties\":{\"b\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}}},\"e\":{\"type\":\"object\",\"properties\":{\"b\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}}}}}",
    );
    let expected = "{\"type\":\"object\",\"properties\":{\"a\":{\"$ref\":\"#/$defs/_0\"},\"e\":{\"$ref\":\"#/$defs/_0\"}},\"$defs\":{\"_0\":{\"type\":\"object\",\"properties\":{\"b\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}}}}}";
    assert_eq!(get_compressed_content(&schema_value), expected);
}

#[test]
fn get_compressed_content_3() {
    let schema_value = schema(
        "{\"type\":\"object\",\"properties\":{\"a\":{\"type\":\"object\",\"oneOf\":[{\"allOf\":[{\"properties\":{\"name\":{\"type\":\"string\"},\"description\":{\"type\":\"string\"}}},{\"properties\":{\"street\":{\"type\":\"string\"}}}]},{\"allOf\":[{\"properties\":{\"name\":{\"type\":\"string\"},\"description\":{\"type\":\"string\"}}},{\"properties\":{\"river\":{\"type\":\"string\"}}}]},{\"allOf\":[{\"properties\":{\"name\":{\"type\":\"string\"},\"description\":{\"type\":\"string\"}}},{\"properties\":{\"mountain\":{\"type\":\"string\"}}}]}]},\"b\":{\"type\":\"object\",\"properties\":{\"street\":{\"properties\":{\"street\":{\"type\":\"string\"}}}}}}}",
    );
    let expected = "{\"type\":\"object\",\"properties\":{\"a\":{\"type\":\"object\",\"oneOf\":[{\"allOf\":[{\"$ref\":\"#/$defs/_0\"},{\"$ref\":\"#/$defs/_1\"}]},{\"allOf\":[{\"$ref\":\"#/$defs/_0\"},{\"properties\":{\"river\":{\"type\":\"string\"}}}]},{\"allOf\":[{\"$ref\":\"#/$defs/_0\"},{\"properties\":{\"mountain\":{\"type\":\"string\"}}}]}]},\"b\":{\"type\":\"object\",\"properties\":{\"street\":{\"$ref\":\"#/$defs/_1\"}}}},\"$defs\":{\"_0\":{\"properties\":{\"name\":{\"type\":\"string\"},\"description\":{\"type\":\"string\"}}},\"_1\":{\"properties\":{\"street\":{\"type\":\"string\"}}}}}";
    assert_eq!(get_compressed_content(&schema_value), expected);
}

#[test]
fn get_compressed_content_4() {
    let schema_value = schema(
        "{\"type\":\"object\",\"properties\":{\"a\":{\"type\":\"object\",\"properties\":{\"b\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}}},\"e\":{\"type\":\"object\",\"properties\":{\"b\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}}},\"f\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}",
    );
    let expected = "{\"type\":\"object\",\"properties\":{\"a\":{\"$ref\":\"#/$defs/_0\"},\"e\":{\"$ref\":\"#/$defs/_0\"},\"f\":{\"$ref\":\"#/$defs/_1\"}},\"$defs\":{\"_0\":{\"type\":\"object\",\"properties\":{\"b\":{\"type\":\"object\",\"properties\":{\"c\":{\"$ref\":\"#/$defs/_1\"}}}}},\"_1\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}";
    assert_eq!(get_compressed_content(&schema_value), expected);
}

#[test]
fn get_compressed_content_5() {
    let schema_value = schema(
        "{\"type\":\"object\",\"properties\":{\"a\":{\"type\":\"array\",\"items\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}},\"e\":{\"type\":\"array\",\"items\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}},\"f\":{\"type\":\"object\",\"properties\":{\"b\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}}},\"g\":{\"type\":\"object\",\"properties\":{\"b\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}}}}}",
    );
    let expected = "{\"type\":\"object\",\"properties\":{\"a\":{\"$ref\":\"#/$defs/_0\"},\"e\":{\"$ref\":\"#/$defs/_0\"},\"f\":{\"$ref\":\"#/$defs/_1\"},\"g\":{\"$ref\":\"#/$defs/_1\"}},\"$defs\":{\"_0\":{\"type\":\"array\",\"items\":{\"$ref\":\"#/$defs/_2\"}},\"_1\":{\"type\":\"object\",\"properties\":{\"b\":{\"$ref\":\"#/$defs/_2\"}}},\"_2\":{\"type\":\"object\",\"properties\":{\"c\":{\"type\":\"object\",\"properties\":{\"d\":{\"type\":\"string\"}}}}}}}";
    assert_eq!(get_compressed_content(&schema_value), expected);
}
