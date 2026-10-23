//! Porte de `.../bracketPairsTextModelPart/colorizedBracketPairsDecorationProvider.ts`: as decorações de
//! colorização de pares de colchetes, que o `TextModel` soma às decorações dele.
//!
//! Divergências:
//! - O `registerThemingParticipant` (as regras de CSS `.bracket-highlighting-N` e o
//!   `unexpected-closing-bracket`) não existe no porte: a UI não usa CSS (ver `CLAUDE.md`). O nome de classe
//!   de cada colchete é o do original e o tema resolve a cor pelo nome.
//! - As opções da decoração (`{ description, inlineClassName }`) viram um `ModelDecorationOptions` dinâmico.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use crate::model::{
    BracketPairColorizationOptions, ModelDecoration, ModelDecorationOptionsInit,
    ModelOptionsChangedEvent,
};
use crate::range::Range;
use crate::text_model::{ModelDecorationOptions, TextModel};
use crate::text_model_bracket_pairs::BracketInfo;

/// `ColorizedBracketPairsDecorationProvider`.
pub struct ColorizedBracketPairsDecorationProvider {
    base: Rc<Disposable>,
    text_model: Weak<TextModel>,
    colorization_options: Cell<BracketPairColorizationOptions>,
    on_did_change_emitter: Rc<Emitter<()>>,
}

impl ColorizedBracketPairsDecorationProvider {
    pub fn new(
        text_model: Weak<TextModel>,
        colorization_options: BracketPairColorizationOptions,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_emitter = Emitter::new();
        base.register(on_did_change_emitter.clone());
        Rc::new(Self {
            base,
            text_model,
            colorization_options: Cell::new(colorization_options),
            on_did_change_emitter,
        })
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change_emitter.event()
    }

    /// Dispara `onDidChange` (o `TextModel` liga isto ao `bracketPairs.onDidChange`).
    pub fn fire_did_change(&self) {
        self.on_did_change_emitter.fire(());
    }

    // #region TextModel events

    pub fn handle_did_change_options(&self, _e: &ModelOptionsChangedEvent) {
        if let Some(model) = self.text_model.upgrade() {
            self.colorization_options
                .set(model.get_options().bracket_pair_colorization_options);
        }
    }

    // #endregion

    pub fn get_decorations_in_range(
        &self,
        range: &Range,
        owner_id: Option<i32>,
        _filter_out_validation: bool,
        _filter_font_decorations: bool,
        only_minimap_decorations: bool,
    ) -> Vec<ModelDecoration> {
        if only_minimap_decorations {
            // As decorações de colorização de pares de colchetes não aparecem no minimapa
            return Vec::new();
        }
        if owner_id.is_none() {
            return Vec::new();
        }
        let options = self.colorization_options.get();
        if !options.enabled {
            return Vec::new();
        }

        let Some(model) = self.text_model.upgrade() else {
            return Vec::new();
        };
        model
            .bracket_pairs()
            .get_brackets_in_range(*range, true)
            .map(move |bracket| ModelDecoration {
                id: format!("bracket{}-{}", bracket.range, bracket.nesting_level),
                options: ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                    description: "BracketPairColorization".to_owned(),
                    inline_class_name: Some(get_inline_class_name(
                        &bracket,
                        options.independent_color_pool_per_bracket_type,
                    )),
                    ..Default::default()
                }),
                owner_id: 0,
                range: bracket.range,
            })
            .to_array()
    }

    pub fn get_all_decorations(
        &self,
        owner_id: Option<i32>,
        filter_out_validation: bool,
        filter_font_decorations: bool,
    ) -> Vec<ModelDecoration> {
        if owner_id.is_none() {
            return Vec::new();
        }
        if !self.colorization_options.get().enabled {
            return Vec::new();
        }
        let Some(model) = self.text_model.upgrade() else {
            return Vec::new();
        };
        self.get_decorations_in_range(
            &Range::new(1, 1, model.get_line_count(), 1),
            owner_id,
            filter_out_validation,
            filter_font_decorations,
            false,
        )
    }
}

impl IDisposable for ColorizedBracketPairsDecorationProvider {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}

/// `ColorProvider.unexpectedClosingBracketClassName`.
pub const UNEXPECTED_CLOSING_BRACKET_CLASS_NAME: &str = "unexpected-closing-bracket";

/// `ColorProvider.getInlineClassName(bracket, independentColorPoolPerBracketType)`.
pub fn get_inline_class_name(
    bracket: &BracketInfo,
    independent_color_pool_per_bracket_type: bool,
) -> String {
    if bracket.is_invalid {
        return UNEXPECTED_CLOSING_BRACKET_CLASS_NAME.to_owned();
    }
    get_inline_class_name_of_level(if independent_color_pool_per_bracket_type {
        bracket.nesting_level_of_equal_bracket_type
    } else {
        bracket.nesting_level
    })
}

/// `ColorProvider.getInlineClassNameOfLevel(level)`: para suportar uma quantidade dinâmica de até 6 cores,
/// usa-se um número que é o mínimo múltiplo comum de 1 a 6.
pub fn get_inline_class_name_of_level(level: i64) -> String {
    format!("bracket-highlighting-{}", level % 30)
}
