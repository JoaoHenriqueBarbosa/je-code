//! Porte de `EditorRulers` e `IRulerOption` (`editorOptions.ts`, linhas 4007 a 4071).
//!
//! `schema()` devolve `None`: o esquema do original (`array` de `anyOf` com `format: 'color-hex'`)
//! não cabe em [`ConfigurationPropertySchema`] (`// TODO(config)`). O `validate` do trait só enxerga
//! `Other` para um array e não lê os elementos, então devolve o padrão (a lista vazia, que é o resultado
//! de `[]` e de qualquer não-array); a validação dos elementos está em [`EditorRulers::validate_rulers`].

use crate::config::editor_options::{clamped_int, EditorOption, EditorOptionImpl, Opt, OptionInput};

/// Chave tipada de `rulers`.
pub const RULERS: Opt<Vec<IRulerOption>> = Opt::new(EditorOption::Rulers);

/// `IRulerOption`. `color` ausente (`undefined` ou `null` no original) é `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct IRulerOption {
    pub column: f64,
    pub color: Option<String>,
}

/// Elemento de `(number | IRulerOption)[]`.
#[derive(Clone, Debug, PartialEq)]
pub enum RulerInput {
    Column(f64),
    Option(IRulerOption),
}

/// `EditorRulers`.
pub struct EditorRulers;

impl EditorRulers {
    pub fn new() -> Self {
        EditorRulers
    }

    /// `validate` com os elementos já montados; `None` é "não é array" (vale o padrão).
    pub fn validate_rulers(&self, input: Option<&[RulerInput]>) -> Vec<IRulerOption> {
        let Some(input) = input else {
            return self.default_value();
        };
        let mut rulers: Vec<IRulerOption> = input
            .iter()
            .map(|element| match element {
                RulerInput::Column(column) => {
                    IRulerOption { column: clamped_int(&OptionInput::Number(*column), 0.0, 0.0, 10000.0), color: None }
                }
                RulerInput::Option(option) => {
                    IRulerOption { column: clamped_int(&OptionInput::Number(option.column), 0.0, 0.0, 10000.0), color: option.color.clone() }
                }
            })
            .collect();
        // `Array.prototype.sort` é estável, como `sort_by`.
        rulers.sort_by(|a, b| a.column.total_cmp(&b.column));
        rulers
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

    fn validate(&self, _input: &OptionInput) -> Vec<IRulerOption> {
        self.default_value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ruler(column: f64, color: Option<&str>) -> IRulerOption {
        IRulerOption { column, color: color.map(str::to_string) }
    }

    #[test]
    fn default_is_empty() {
        assert!(EditorRulers::new().default_value().is_empty());
    }

    #[test]
    fn non_arrays_give_the_default() {
        let option = EditorRulers::new();
        assert!(option.validate_rulers(None).is_empty());
        for input in [OptionInput::Undefined, OptionInput::Null, OptionInput::Number(80.0), OptionInput::string("80"), OptionInput::Other] {
            assert!(option.validate(&input).is_empty());
        }
        assert!(option.validate_rulers(Some(&[])).is_empty());
    }

    #[test]
    fn numbers_become_rulers_without_color() {
        let option = EditorRulers::new();
        let rulers = option.validate_rulers(Some(&[RulerInput::Column(80.0), RulerInput::Column(120.0)]));
        assert_eq!(rulers, vec![ruler(80.0, None), ruler(120.0, None)]);
    }

    #[test]
    fn objects_keep_their_color() {
        let option = EditorRulers::new();
        let rulers = option.validate_rulers(Some(&[RulerInput::Option(ruler(100.0, Some("#ff0000"))), RulerInput::Option(ruler(60.0, None))]));
        assert_eq!(rulers, vec![ruler(60.0, None), ruler(100.0, Some("#ff0000"))]);
    }

    #[test]
    fn columns_are_clamped_integers() {
        let option = EditorRulers::new();
        let rulers = option.validate_rulers(Some(&[
            RulerInput::Column(-5.0),
            RulerInput::Column(99999.0),
            RulerInput::Column(80.9),
            RulerInput::Option(ruler(f64::NAN, Some("#fff"))),
            RulerInput::Option(ruler(20000.0, None)),
        ]));
        assert_eq!(
            rulers,
            vec![ruler(0.0, None), ruler(0.0, Some("#fff")), ruler(80.0, None), ruler(10000.0, None), ruler(10000.0, None)]
        );
    }

    #[test]
    fn sort_is_by_column_and_stable() {
        let option = EditorRulers::new();
        let rulers = option.validate_rulers(Some(&[
            RulerInput::Option(ruler(80.0, Some("#a"))),
            RulerInput::Column(40.0),
            RulerInput::Option(ruler(80.0, Some("#b"))),
            RulerInput::Column(5.0),
        ]));
        let columns: Vec<f64> = rulers.iter().map(|r| r.column).collect();
        assert_eq!(columns, vec![5.0, 40.0, 80.0, 80.0]);
        assert_eq!(rulers[2].color.as_deref(), Some("#a"));
        assert_eq!(rulers[3].color.as_deref(), Some("#b"));
    }

    #[test]
    fn identity() {
        let option = EditorRulers::new();
        assert_eq!(option.id(), EditorOption::Rulers);
        assert_eq!(option.name(), "rulers");
        assert!(option.schema().is_none());
    }
}
