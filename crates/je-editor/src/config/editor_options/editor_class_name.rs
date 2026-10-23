//! Porte de `EditorClassName` (`editorOptions.ts`, região `editorClassName`): a lista de classes do
//! elemento raiz do editor, calculada a partir de outras opções. Como opção só calculada, é um
//! [`ComputedEditorOption`] (`name = "_never_"`, sem esquema).

use crate::config::editor_options::{ComputedEditorOption, EditorOption, Opt};

/// Chave tipada da opção `editorClassName`.
pub const EDITOR_CLASS_NAME: Opt<String> = Opt::new(EditorOption::EditorClassName);

const EXTRA_EDITOR_CLASS_NAME: Opt<String> = Opt::new(EditorOption::ExtraEditorClassName);
const MOUSE_STYLE: Opt<String> = Opt::new(EditorOption::MouseStyle);
const SHOW_UNUSED: Opt<bool> = Opt::new(EditorOption::ShowUnused);
const SHOW_DEPRECATED: Opt<bool> = Opt::new(EditorOption::ShowDeprecated);

/// `EditorClassName`.
pub fn editor_class_name() -> ComputedEditorOption<String> {
    ComputedEditorOption::new(EditorOption::EditorClassName, String::new(), |env, options, _| {
        let mut class_names = vec!["monaco-editor".to_string()];
        let extra_editor_class_name = options.get(EXTRA_EDITOR_CLASS_NAME);
        if !extra_editor_class_name.is_empty() {
            class_names.push(extra_editor_class_name.clone());
        }
        if !env.extra_editor_class_name.is_empty() {
            class_names.push(env.extra_editor_class_name.clone());
        }
        match options.get(MOUSE_STYLE).as_str() {
            "default" => class_names.push("mouse-default".to_string()),
            "copy" => class_names.push("mouse-copy".to_string()),
            _ => {}
        }

        if *options.get(SHOW_UNUSED) {
            class_names.push("showUnused".to_string());
        }

        if *options.get(SHOW_DEPRECATED) {
            class_names.push("showDeprecated".to_string());
        }

        class_names.join(" ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::testing;
    use crate::config::editor_options::EditorOptionImpl;

    fn compute(extra: &str, env_extra: &str, mouse_style: &str, show_unused: bool, show_deprecated: bool) -> String {
        let option = editor_class_name();
        let mut env = testing::environment(testing::font_info(true, 14.0, "normal"));
        env.extra_editor_class_name = env_extra.to_string();
        let options = testing::options_with(vec![
            (EditorOption::ExtraEditorClassName, testing::value(extra.to_string())),
            (EditorOption::MouseStyle, testing::value(mouse_style.to_string())),
            (EditorOption::ShowUnused, testing::value(show_unused)),
            (EditorOption::ShowDeprecated, testing::value(show_deprecated)),
        ]);
        EditorOptionImpl::compute(&option, &env, &options, &String::new())
    }

    #[test]
    fn minimal() {
        assert_eq!(compute("", "", "text", false, false), "monaco-editor");
    }

    #[test]
    fn everything_in_the_original_order() {
        assert_eq!(
            compute("extra", "env-extra", "default", true, true),
            "monaco-editor extra env-extra mouse-default showUnused showDeprecated"
        );
    }

    #[test]
    fn mouse_style_copy_and_individual_flags() {
        assert_eq!(compute("", "", "copy", false, false), "monaco-editor mouse-copy");
        assert_eq!(compute("", "only-env", "text", true, false), "monaco-editor only-env showUnused");
        assert_eq!(compute("only-option", "", "text", false, true), "monaco-editor only-option showDeprecated");
    }

    #[test]
    fn computed_option_shape() {
        let option = editor_class_name();
        assert_eq!(EditorOptionImpl::name(&option), "_never_");
        assert_eq!(EditorOptionImpl::id(&option), EditorOption::EditorClassName);
        assert_eq!(EditorOptionImpl::default_value(&option), "");
        assert!(EditorOptionImpl::schema(&option).is_none());
    }
}
