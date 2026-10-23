//! `TestDecorationProvider` de `src/vs/editor/contrib/folding/test/browser/foldingModel.test.ts`, que o
//! `hiddenRangeModel.test.ts` também importa. Fica aqui porque cada arquivo de teste do Rust é um programa
//! separado e não importa o outro.
//!
//! As três opções (`collapsedDecoration`, `expandedDecoration`, `hiddenDecoration`) são `static` no original,
//! registradas uma vez; aqui são campos do provedor, criados com `ModelDecorationOptions::register`. Os
//! três têm o mesmo conteúdo e só a identidade (o `===` do original, `Rc::ptr_eq`) os distingue.

use std::rc::Rc;

use je_editor::folding::folding_model::IDecorationProvider;
use je_editor::model::{ModelDecorationOptionsInit, TrackedRangeStickiness};
use je_editor::text_model::{ModelDecorationOptions, ModelDecorationsChangeAccessor, TextModel};

/// `ExpectedDecoration.type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpectedDecorationType {
    Hidden,
    Collapsed,
    Expanded,
}

/// `ExpectedDecoration`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpectedDecoration {
    pub line: i32,
    pub r#type: ExpectedDecorationType,
}

fn test_decoration() -> Rc<ModelDecorationOptions> {
    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: "test".to_owned(),
        stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
        lines_decorations_class_name: Some("folding".to_owned()),
        ..Default::default()
    })
}

/// `TestDecorationProvider`.
pub struct TestDecorationProvider {
    model: Rc<TextModel>,
    collapsed_decoration: Rc<ModelDecorationOptions>,
    expanded_decoration: Rc<ModelDecorationOptions>,
    hidden_decoration: Rc<ModelDecorationOptions>,
}

impl TestDecorationProvider {
    pub fn new(model: Rc<TextModel>) -> Rc<TestDecorationProvider> {
        Rc::new(TestDecorationProvider {
            model,
            collapsed_decoration: test_decoration(),
            expanded_decoration: test_decoration(),
            hidden_decoration: test_decoration(),
        })
    }

    pub fn get_decorations(&self) -> Vec<ExpectedDecoration> {
        let decorations = self.model.get_all_decorations(0, false, false);
        let mut res: Vec<ExpectedDecoration> = Vec::new();
        for decoration in &decorations {
            let r#type = if Rc::ptr_eq(&decoration.options, &self.hidden_decoration) {
                ExpectedDecorationType::Hidden
            } else if Rc::ptr_eq(&decoration.options, &self.collapsed_decoration) {
                ExpectedDecorationType::Collapsed
            } else if Rc::ptr_eq(&decoration.options, &self.expanded_decoration) {
                ExpectedDecorationType::Expanded
            } else {
                continue;
            };
            res.push(ExpectedDecoration { line: decoration.range.start_line_number, r#type });
        }
        res
    }
}

impl IDecorationProvider for TestDecorationProvider {
    fn get_decoration_option(&self, is_collapsed: bool, is_hidden: bool, _is_manual: bool) -> Rc<ModelDecorationOptions> {
        if is_hidden {
            return self.hidden_decoration.clone();
        }
        if is_collapsed {
            return self.collapsed_decoration.clone();
        }
        self.expanded_decoration.clone()
    }

    fn change_decorations(&self, callback: &mut dyn FnMut(&ModelDecorationsChangeAccessor<'_>)) -> Option<()> {
        self.model.change_decorations(|accessor| callback(accessor), 0)
    }

    fn remove_decorations(&self, decoration_ids: &[String]) {
        self.model.change_decorations(
            |change_accessor| {
                change_accessor.delta_decorations(decoration_ids, &[]);
            },
            0,
        );
    }
}
