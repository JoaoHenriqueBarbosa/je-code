//! Porte de `src/vs/editor/contrib/wordHighlighter/browser/highlightDecorations.ts`: as opções de decoração dos destaques de
//! palavra (leitura, escrita e texto) e de seleção ([`get_highlight_decoration_options`] e
//! [`get_selection_highlight_decoration_options`]).
//!
//! Divergências:
//! - Os `registerColor(...)` do começo do arquivo já existem no `je-theme` (`registered_colors`), e o
//!   `registerThemingParticipant` do fim (a regra CSS `.monaco-editor .selectionHighlight { background-color: ... }`) não
//!   existe: a UI nativa não usa CSS, e a cor `editor.selectionHighlightBackground` é lida pelo desenho da decoração.
//! - O `import './highlightDecorations.css'` também não existe, pelo mesmo motivo.
//! - `themeColorFromId(id)` é `DecorationColor::Theme` com o id da cor.

use std::rc::Rc;

use crate::language_providers::DocumentHighlightKind;
use crate::model::{
    DecorationColor, MinimapPosition, ModelDecorationMinimapInit, ModelDecorationOptionsInit,
    ModelDecorationOverviewRulerInit, OverviewRulerLane, TrackedRangeStickiness,
};
use crate::text_model::ModelDecorationOptions;

/// O id da cor do minimapa de uma ocorrência da seleção (`minimapSelectionOccurrenceHighlight`).
const MINIMAP_SELECTION_OCCURRENCE_HIGHLIGHT: &str = "minimap.selectionOccurrenceHighlight";

/// `ModelDecorationOptions.register({ description, stickiness, className, overviewRuler, minimap })` com a cor de régua
/// de visão geral `overview_ruler_color`.
fn register_with_overview(description: &str, class_name: &str, overview_ruler_color: &str) -> Rc<ModelDecorationOptions> {
    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: description.to_owned(),
        stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
        class_name: Some(class_name.to_owned()),
        overview_ruler: Some(ModelDecorationOverviewRulerInit {
            color: Some(DecorationColor::Theme(overview_ruler_color.to_owned())),
            dark_color: None,
            position: Some(OverviewRulerLane::Center),
        }),
        minimap: Some(ModelDecorationMinimapInit {
            color: Some(DecorationColor::Theme(MINIMAP_SELECTION_OCCURRENCE_HIGHLIGHT.to_owned())),
            dark_color: None,
            position: MinimapPosition::Inline,
            section_header_style: None,
            section_header_text: None,
        }),
        ..ModelDecorationOptionsInit::default()
    })
}

struct HighlightDecorationOptions {
    write: Rc<ModelDecorationOptions>,
    text: Rc<ModelDecorationOptions>,
    selection_highlight: Rc<ModelDecorationOptions>,
    selection_highlight_no_overview: Rc<ModelDecorationOptions>,
    regular: Rc<ModelDecorationOptions>,
}

thread_local! {
    static OPTIONS: HighlightDecorationOptions = HighlightDecorationOptions {
        // _WRITE_OPTIONS
        write: register_with_overview(
            "word-highlight-strong",
            "wordHighlightStrong",
            "editorOverviewRuler.wordHighlightStrongForeground",
        ),
        // _TEXT_OPTIONS
        text: register_with_overview(
            "word-highlight-text",
            "wordHighlightText",
            "editorOverviewRuler.wordHighlightTextForeground",
        ),
        // _SELECTION_HIGHLIGHT_OPTIONS
        selection_highlight: register_with_overview(
            "selection-highlight-overview",
            "selectionHighlight",
            "editorOverviewRuler.selectionHighlightForeground",
        ),
        // _SELECTION_HIGHLIGHT_OPTIONS_NO_OVERVIEW
        selection_highlight_no_overview: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "selection-highlight".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            class_name: Some("selectionHighlight".to_owned()),
            ..ModelDecorationOptionsInit::default()
        }),
        // _REGULAR_OPTIONS
        regular: register_with_overview(
            "word-highlight",
            "wordHighlight",
            "editorOverviewRuler.wordHighlightForeground",
        ),
    };
}

/// `getHighlightDecorationOptions(kind)`.
pub fn get_highlight_decoration_options(kind: Option<DocumentHighlightKind>) -> Rc<ModelDecorationOptions> {
    OPTIONS.with(|options| match kind {
        Some(DocumentHighlightKind::Write) => Rc::clone(&options.write),
        Some(DocumentHighlightKind::Text) => Rc::clone(&options.text),
        _ => Rc::clone(&options.regular),
    })
}

/// `getSelectionHighlightDecorationOptions(hasSemanticHighlights)`: mostra na régua de visão geral só se o modelo não tem
/// destaque semântico.
pub fn get_selection_highlight_decoration_options(has_semantic_highlights: bool) -> Rc<ModelDecorationOptions> {
    OPTIONS.with(|options| {
        if has_semantic_highlights {
            Rc::clone(&options.selection_highlight_no_overview)
        } else {
            Rc::clone(&options.selection_highlight)
        }
    })
}
