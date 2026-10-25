//! Porte de `src/vs/editor/browser/widget/diffEditor/registrations.contribution.ts`: os ícones `diff-insert` e `diff-remove` e as
//! opções de decoração registradas (`ModelDecorationOptions.register`) que o editor de diff põe nas linhas e nos trechos
//! alterados (`line-insert`, `line-delete`, `char-insert`, `char-delete` e as variantes).
//!
//! Divergências:
//! - **Cores.** `diffEditor.move.border`, `diffEditor.moveActive.border` e `diffEditor.unchangedRegionShadow` (os três `registerColor`
//!   do arquivo) já vivem no registro de cores de `je-theme`, e não são repetidos aqui.
//! - **Ícones.** `registerIcon('diff-insert', Codicon.add, ...)` devolve o `ThemeIcon` de id `diff-insert`; o mapeamento do id para o
//!   glifo do codicon `add` é do registro de ícones, que o pintor consulta. Aqui só existe o `ThemeIcon`, de onde sai a classe
//!   `codicon codicon-diff-insert` do `ThemeIcon.asClassName`.
//! - As opções são registradas uma vez por thread (`thread_local`), como as constantes de módulo do original, e
//!   [`diff_decoration_options`] entrega um clone dos `Rc`.

use std::rc::Rc;

use je_base::themables::ThemeIcon;

use crate::model::ModelDecorationOptionsInit;
use crate::text_model::ModelDecorationOptions;

/// `diffInsertIcon`.
pub fn diff_insert_icon() -> ThemeIcon {
    ThemeIcon::from_id("diff-insert")
}

/// `diffRemoveIcon`.
pub fn diff_remove_icon() -> ThemeIcon {
    ThemeIcon::from_id("diff-remove")
}

/// As opções de decoração registradas do arquivo, na ordem do original.
#[derive(Clone)]
pub struct DiffDecorationOptions {
    pub diff_line_add_decoration_background_with_indicator: Rc<ModelDecorationOptions>,
    pub diff_line_delete_decoration_background_with_indicator: Rc<ModelDecorationOptions>,
    pub diff_line_add_decoration_background: Rc<ModelDecorationOptions>,
    pub diff_line_delete_decoration_background: Rc<ModelDecorationOptions>,
    pub diff_add_decoration: Rc<ModelDecorationOptions>,
    pub diff_whole_line_add_decoration: Rc<ModelDecorationOptions>,
    pub diff_add_decoration_empty: Rc<ModelDecorationOptions>,
    pub diff_delete_decoration: Rc<ModelDecorationOptions>,
    pub diff_whole_line_delete_decoration: Rc<ModelDecorationOptions>,
    pub diff_delete_decoration_empty: Rc<ModelDecorationOptions>,
}

/// `ModelDecorationOptions.register({ className, description: className, ...rest })`: nas dez opções do arquivo a descrição é
/// o próprio nome da classe.
fn register(class_name: &str, rest: ModelDecorationOptionsInit) -> Rc<ModelDecorationOptions> {
    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        class_name: Some(class_name.to_owned()),
        description: class_name.to_owned(),
        ..rest
    })
}

impl DiffDecorationOptions {
    fn register_all() -> Self {
        let whole_line = || ModelDecorationOptionsInit { is_whole_line: true, ..ModelDecorationOptionsInit::default() };
        let margin = |margin_class_name: &str| ModelDecorationOptionsInit {
            margin_class_name: Some(margin_class_name.to_owned()),
            ..whole_line()
        };
        let fill_line_on_line_break = ModelDecorationOptionsInit { should_fill_line_on_line_break: Some(true), ..ModelDecorationOptionsInit::default() };

        Self {
            diff_line_add_decoration_background_with_indicator: register(
                "line-insert",
                ModelDecorationOptionsInit {
                    lines_decorations_class_name: Some(format!("insert-sign {}", diff_insert_icon().as_class_name())),
                    ..margin("gutter-insert")
                },
            ),
            diff_line_delete_decoration_background_with_indicator: register(
                "line-delete",
                ModelDecorationOptionsInit {
                    lines_decorations_class_name: Some(format!("delete-sign {}", diff_remove_icon().as_class_name())),
                    ..margin("gutter-delete")
                },
            ),
            diff_line_add_decoration_background: register("line-insert", margin("gutter-insert")),
            diff_line_delete_decoration_background: register("line-delete", margin("gutter-delete")),
            diff_add_decoration: register("char-insert", fill_line_on_line_break.clone()),
            diff_whole_line_add_decoration: register("char-insert", whole_line()),
            diff_add_decoration_empty: register("char-insert diff-range-empty", ModelDecorationOptionsInit::default()),
            diff_delete_decoration: register("char-delete", fill_line_on_line_break),
            diff_whole_line_delete_decoration: register("char-delete", whole_line()),
            diff_delete_decoration_empty: register("char-delete diff-range-empty", ModelDecorationOptionsInit::default()),
        }
    }
}

thread_local! {
    static DIFF_DECORATION_OPTIONS: DiffDecorationOptions = DiffDecorationOptions::register_all();
}

/// As opções de decoração registradas do editor de diff (as constantes `diff*Decoration*` do original).
pub fn diff_decoration_options() -> DiffDecorationOptions {
    DIFF_DECORATION_OPTIONS.with(DiffDecorationOptions::clone)
}
