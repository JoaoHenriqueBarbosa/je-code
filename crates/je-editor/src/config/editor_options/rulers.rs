//! Porte de `EditorRulers` e `IRulerOption` (`editorOptions.ts`, linhas 4007 a 4071).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{clamped_int, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `rulers`.
pub const RULERS: Opt<Vec<IRulerOption>> = Opt::new(EditorOption::Rulers);

/// `IRulerOption`. `color` que não é texto (`null`, `undefined`) é `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct IRulerOption {
    pub column: f64,
    pub color: Option<String>,
}

/// `EditorRulers`.
pub struct EditorRulers {
    schema: Dynamic,
}

impl EditorRulers {
    pub fn new() -> Self {
        let column_schema = Dynamic::object([
            ("type", Dynamic::string("number")),
            ("description", Dynamic::string("Number of monospace characters at which this editor ruler will render.")),
        ]);
        let schema = Dynamic::object([
            ("type", Dynamic::string("array")),
            (
                "items",
                Dynamic::object([(
                    "anyOf",
                    Dynamic::array([
                        column_schema.clone(),
                        Dynamic::object([
                            ("type", Dynamic::array([Dynamic::string("object")])),
                            (
                                "properties",
                                Dynamic::object([
                                    ("column", column_schema),
                                    (
                                        "color",
                                        Dynamic::object([
                                            ("type", Dynamic::string("string")),
                                            ("description", Dynamic::string("Color of this editor ruler.")),
                                            ("format", Dynamic::string("color-hex")),
                                        ]),
                                    ),
                                ]),
                            ),
                        ]),
                    ]),
                )]),
            ),
            ("default", Dynamic::array([])),
            (
                "description",
                Dynamic::string("Render vertical rulers after a certain number of monospace characters. Use multiple values for multiple rulers. No rulers are drawn if array is empty."),
            ),
        ]);
        Self { schema }
    }
}

impl Default for EditorRulers {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorRulers {
    type Value = Vec<IRulerOption>;

    fn id(&self) -> EditorOption {
        EditorOption::Rulers
    }

    fn name(&self) -> &str {
        "rulers"
    }

    fn default_value(&self) -> Vec<IRulerOption> {
        Vec::new()
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> Vec<IRulerOption> {
        if !input.is_array() {
            return self.default_value();
        }
        let mut rulers = Vec::new();
        for (_, element) in input.own_entries() {
            if let Dynamic::Number(_) = element {
                rulers.push(IRulerOption { column: clamped_int(&element, 0.0, 0.0, 10000.0), color: None });
            } else if element.is_truthy() && element.type_of() == "object" {
                let color = match element.get_str("color") {
                    Dynamic::String(text) => Some(text.to_string_lossy()),
                    _ => None,
                };
                rulers.push(IRulerOption { column: clamped_int(&element.get_str("column"), 0.0, 0.0, 10000.0), color });
            }
        }
        // `Array.prototype.sort` é estável, como `sort_by`.
        rulers.sort_by(|a, b| a.column.total_cmp(&b.column));
        rulers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ruler(column: f64, color: Option<&str>) -> IRulerOption {
        IRulerOption { column, color: color.map(str::to_string) }
    }

    fn num(n: f64) -> Dynamic {
        Dynamic::Number(n)
    }

    fn object(column: Dynamic, color: Option<&str>) -> Dynamic {
        match color {
            Some(color) => Dynamic::object([("column", column), ("color", Dynamic::string(color))]),
            None => Dynamic::object([("column", column)]),
        }
    }

    fn validate(items: Vec<Dynamic>) -> Vec<IRulerOption> {
        EditorRulers::new().validate(&Dynamic::array(items))
    }

    #[test]
    fn default_is_empty() {
        assert!(EditorRulers::new().default_value().is_empty());
    }

    #[test]
    fn non_arrays_give_the_default() {
        let option = EditorRulers::new();
        for input in [Dynamic::Undefined, Dynamic::Null, num(80.0), Dynamic::string("80"), Dynamic::object([("0", num(80.0))])] {
            assert!(option.validate(&input).is_empty());
        }
        assert!(validate(vec![]).is_empty());
    }

    #[test]
    fn numbers_become_rulers_without_color() {
        assert_eq!(validate(vec![num(80.0), num(120.0)]), vec![ruler(80.0, None), ruler(120.0, None)]);
    }

    #[test]
    fn objects_keep_their_color() {
        let rulers = validate(vec![object(num(100.0), Some("#ff0000")), object(num(60.0), None)]);
        assert_eq!(rulers, vec![ruler(60.0, None), ruler(100.0, Some("#ff0000"))]);
    }

    #[test]
    fn elements_that_are_neither_numbers_nor_objects_are_skipped() {
        let rulers = validate(vec![Dynamic::string("80"), Dynamic::Null, Dynamic::Undefined, Dynamic::Bool(true), num(5.0)]);
        assert_eq!(rulers, vec![ruler(5.0, None)]);
    }

    #[test]
    fn columns_are_clamped_integers() {
        let rulers = validate(vec![
            num(-5.0),
            num(99999.0),
            num(80.9),
            object(num(f64::NAN), Some("#fff")),
            object(num(20000.0), None),
            object(Dynamic::string("42"), None),
            object(Dynamic::Undefined, None),
        ]);
        assert_eq!(
            rulers,
            vec![
                ruler(0.0, None),
                ruler(0.0, Some("#fff")),
                ruler(0.0, None),
                ruler(42.0, None),
                ruler(80.0, None),
                ruler(10000.0, None),
                ruler(10000.0, None)
            ]
        );
    }

    #[test]
    fn sort_is_by_column_and_stable() {
        let rulers = validate(vec![object(num(80.0), Some("#a")), num(40.0), object(num(80.0), Some("#b")), num(5.0)]);
        let columns: Vec<f64> = rulers.iter().map(|r| r.column).collect();
        assert_eq!(columns, vec![5.0, 40.0, 80.0, 80.0]);
        assert_eq!(rulers[2].color.as_deref(), Some("#a"));
        assert_eq!(rulers[3].color.as_deref(), Some("#b"));
    }

    #[test]
    fn schema_keeps_the_original_shape_and_order() {
        let option = EditorRulers::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["type", "items", "default", "description"]);
        let any_of = schema.get_str("items").get_str("anyOf");
        assert_eq!(any_of.get_str("length"), num(2.0));
        let object_schema = any_of.get_str("1");
        assert_eq!(object_schema.own_keys(), vec!["type", "properties"]);
        assert_eq!(object_schema.get_str("type").get_str("0"), Dynamic::string("object"));
        let properties = object_schema.get_str("properties");
        assert_eq!(properties.own_keys(), vec!["column", "color"]);
        assert_eq!(properties.get_str("color").own_keys(), vec!["type", "description", "format"]);
        // `columnSchema` é a mesma referência nos dois lugares.
        assert!(any_of.get_str("0").strict_equals(&properties.get_str("column")));
        assert_eq!(schema.get_str("default").get_str("length"), num(0.0));
    }
}
