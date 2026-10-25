//! Porte de `src/vs/editor/contrib/folding/browser/foldingDecorations.ts`: o provedor das opções de decoração das
//! regiões de dobra (`FoldingDecorationProvider`), com as treze opções estáticas e os ícones do gutter.
//!
//! Representação:
//! - As `ModelDecorationOptions.register(...)` estáticas são um `thread_local` de [`Rc<ModelDecorationOptions>`]; a
//!   identidade das opções (que `foldingModel` e os testes comparam com `===`) é a do `Rc`.
//! - `registerIcon('folding-expanded', ...)` e as três irmãs são os ids de [`ThemeIcon`] `folding-expanded`,
//!   `folding-collapsed`, `folding-manual-collapsed` e `folding-manual-expanded`; `ThemeIcon.asClassName(icon)` é
//!   `codicon codicon-<id>`, como o registro de ícones devolve o ícone com o id registrado.
//! - `themeColorFromId(foldBackground)` é [`DecorationColor::Theme`] com o id `editor.foldBackground`. O registro das
//!   cores `editor.foldBackground`, `editor.foldPlaceholderForeground` e `editorGutter.foldingControlForeground`
//!   mora em `je-theme` (tabela de cores registradas), não aqui.
//! - O `ICodeEditor` é um [`Weak`] do [`CodeEditorWidget`]: o editor é dono do controlador que é dono do provedor.
//! - `changeDecorations<T>(callback): T | null` é o `change_decorations` de [`IDecorationProvider`] (callback
//!   `FnMut`, retorno `Option<()>`, ver `folding_model`).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::themables::ThemeIcon;

use super::folding_model::IDecorationProvider;
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::simple_keys::ShowFoldingControls;
use crate::model::{
    DecorationColor, MinimapPosition, ModelDecorationMinimapInit, ModelDecorationOptionsInit, TrackedRangeStickiness,
};
use crate::text_model::{ModelDecorationOptions, ModelDecorationsChangeAccessor};

/// O id da cor `editor.foldBackground` (`foldBackground`).
const FOLD_BACKGROUND: &str = "editor.foldBackground";

/// `foldingExpandedIcon`.
pub const FOLDING_EXPANDED_ICON: &str = "folding-expanded";
/// `foldingCollapsedIcon`.
pub const FOLDING_COLLAPSED_ICON: &str = "folding-collapsed";
/// `foldingManualCollapsedIcon`.
pub const FOLDING_MANUAL_COLLAPSED_ICON: &str = "folding-manual-collapsed";
/// `foldingManualExpandedIcon`.
pub const FOLDING_MANUAL_EXPANDED_ICON: &str = "folding-manual-expanded";

/// `collapsed`: a dica do ícone de uma região dobrada.
const COLLAPSED_TOOLTIP: &str = "Click to expand the range.";
/// `expanded`: a dica do ícone de uma região aberta.
const EXPANDED_TOOLTIP: &str = "Click to collapse the range.";

/// `foldedBackgroundMinimap`.
fn folded_background_minimap() -> ModelDecorationMinimapInit {
    ModelDecorationMinimapInit {
        color: Some(DecorationColor::Theme(FOLD_BACKGROUND.to_owned())),
        dark_color: None,
        position: MinimapPosition::Inline,
        section_header_style: None,
        section_header_text: None,
    }
}

/// `ThemeIcon.asClassName(icon)` do ícone registrado com o id dado.
fn icon_class(id: &str) -> String {
    ThemeIcon::from_id(id).as_class_name()
}

/// As treze opções estáticas de `FoldingDecorationProvider`.
struct FoldingDecorationOptions {
    collapsed_visual: Rc<ModelDecorationOptions>,
    collapsed_highlighted_visual: Rc<ModelDecorationOptions>,
    manually_collapsed_visual: Rc<ModelDecorationOptions>,
    manually_collapsed_highlighted_visual: Rc<ModelDecorationOptions>,
    no_controls_collapsed_range: Rc<ModelDecorationOptions>,
    no_controls_collapsed_highlighted_range: Rc<ModelDecorationOptions>,
    expanded_visual: Rc<ModelDecorationOptions>,
    expanded_auto_hide_visual: Rc<ModelDecorationOptions>,
    manually_expanded_visual: Rc<ModelDecorationOptions>,
    manually_expanded_auto_hide_visual: Rc<ModelDecorationOptions>,
    no_controls_expanded_range: Rc<ModelDecorationOptions>,
    hidden_range: Rc<ModelDecorationOptions>,
}

impl FoldingDecorationOptions {
    /// Uma região dobrada: cresce nas bordas, termina em `inline-folded` e ocupa a linha inteira.
    fn collapsed(description: &str, highlighted: bool, icon: Option<&str>) -> Rc<ModelDecorationOptions> {
        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: description.to_owned(),
            stickiness: Some(TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges),
            after_content_class_name: Some("inline-folded".to_owned()),
            class_name: highlighted.then(|| "folded-background".to_owned()),
            minimap: highlighted.then(folded_background_minimap),
            is_whole_line: true,
            lines_decorations_tooltip: Some(COLLAPSED_TOOLTIP.to_owned()),
            first_line_decoration_class_name: icon.map(icon_class),
            ..ModelDecorationOptionsInit::default()
        })
    }

    /// Uma região aberta, com o ícone e a dica de recolher.
    fn expanded(
        description: &str,
        stickiness: TrackedRangeStickiness,
        always_show: bool,
        icon: &str,
    ) -> Rc<ModelDecorationOptions> {
        let icon_classes = icon_class(icon);
        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: description.to_owned(),
            stickiness: Some(stickiness),
            is_whole_line: true,
            first_line_decoration_class_name: Some(if always_show { format!("alwaysShowFoldIcons {icon_classes}") } else { icon_classes }),
            lines_decorations_tooltip: Some(EXPANDED_TOOLTIP.to_owned()),
            ..ModelDecorationOptionsInit::default()
        })
    }

    fn new() -> Self {
        use TrackedRangeStickiness::{AlwaysGrowsWhenTypingAtEdges, NeverGrowsWhenTypingAtEdges};
        Self {
            collapsed_visual: Self::collapsed("folding-collapsed-visual-decoration", false, Some(FOLDING_COLLAPSED_ICON)),
            collapsed_highlighted_visual: Self::collapsed(
                "folding-collapsed-highlighted-visual-decoration",
                true,
                Some(FOLDING_COLLAPSED_ICON),
            ),
            manually_collapsed_visual: Self::collapsed(
                "folding-manually-collapsed-visual-decoration",
                false,
                Some(FOLDING_MANUAL_COLLAPSED_ICON),
            ),
            manually_collapsed_highlighted_visual: Self::collapsed(
                "folding-manually-collapsed-highlighted-visual-decoration",
                true,
                Some(FOLDING_MANUAL_COLLAPSED_ICON),
            ),
            no_controls_collapsed_range: Self::collapsed("folding-no-controls-range-decoration", false, None),
            no_controls_collapsed_highlighted_range: Self::collapsed("folding-no-controls-range-decoration", true, None),
            expanded_visual: Self::expanded(
                "folding-expanded-visual-decoration",
                NeverGrowsWhenTypingAtEdges,
                true,
                FOLDING_EXPANDED_ICON,
            ),
            expanded_auto_hide_visual: Self::expanded(
                "folding-expanded-auto-hide-visual-decoration",
                NeverGrowsWhenTypingAtEdges,
                false,
                FOLDING_EXPANDED_ICON,
            ),
            manually_expanded_visual: Self::expanded(
                "folding-manually-expanded-visual-decoration",
                AlwaysGrowsWhenTypingAtEdges,
                true,
                FOLDING_MANUAL_EXPANDED_ICON,
            ),
            manually_expanded_auto_hide_visual: Self::expanded(
                "folding-manually-expanded-auto-hide-visual-decoration",
                AlwaysGrowsWhenTypingAtEdges,
                false,
                FOLDING_MANUAL_EXPANDED_ICON,
            ),
            no_controls_expanded_range: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                description: "folding-no-controls-range-decoration".to_owned(),
                stickiness: Some(AlwaysGrowsWhenTypingAtEdges),
                is_whole_line: true,
                ..ModelDecorationOptionsInit::default()
            }),
            hidden_range: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                description: "folding-hidden-range-decoration".to_owned(),
                stickiness: Some(NeverGrowsWhenTypingAtEdges),
                ..ModelDecorationOptionsInit::default()
            }),
        }
    }
}

thread_local! {
    static OPTIONS: FoldingDecorationOptions = FoldingDecorationOptions::new();
}

/// `FoldingDecorationProvider`.
pub struct FoldingDecorationProvider {
    editor: Weak<CodeEditorWidget>,
    show_folding_controls: Cell<ShowFoldingControls>,
    show_folding_highlights: Cell<bool>,
}

impl FoldingDecorationProvider {
    pub fn new(editor: &Rc<CodeEditorWidget>) -> Self {
        Self {
            editor: Rc::downgrade(editor),
            show_folding_controls: Cell::new(ShowFoldingControls::Mouseover),
            show_folding_highlights: Cell::new(true),
        }
    }

    /// `showFoldingControls`.
    pub fn show_folding_controls(&self) -> ShowFoldingControls {
        self.show_folding_controls.get()
    }

    pub fn set_show_folding_controls(&self, value: ShowFoldingControls) {
        self.show_folding_controls.set(value);
    }

    /// `showFoldingHighlights`.
    pub fn show_folding_highlights(&self) -> bool {
        self.show_folding_highlights.get()
    }

    pub fn set_show_folding_highlights(&self, value: bool) {
        self.show_folding_highlights.set(value);
    }
}

impl IDecorationProvider for FoldingDecorationProvider {
    fn get_decoration_option(&self, is_collapsed: bool, is_hidden: bool, is_manual: bool) -> Rc<ModelDecorationOptions> {
        let highlights = self.show_folding_highlights.get();
        let controls = self.show_folding_controls.get();
        OPTIONS.with(|options| {
            if is_hidden {
                // is inside another collapsed region
                return Rc::clone(&options.hidden_range);
            }
            if controls == ShowFoldingControls::Never {
                let option = match (is_collapsed, highlights) {
                    (true, true) => &options.no_controls_collapsed_highlighted_range,
                    (true, false) => &options.no_controls_collapsed_range,
                    (false, _) => &options.no_controls_expanded_range,
                };
                return Rc::clone(option);
            }
            let option = if is_collapsed {
                match (is_manual, highlights) {
                    (true, true) => &options.manually_collapsed_highlighted_visual,
                    (true, false) => &options.manually_collapsed_visual,
                    (false, true) => &options.collapsed_highlighted_visual,
                    (false, false) => &options.collapsed_visual,
                }
            } else if controls == ShowFoldingControls::Mouseover {
                if is_manual { &options.manually_expanded_auto_hide_visual } else { &options.expanded_auto_hide_visual }
            } else if is_manual {
                &options.manually_expanded_visual
            } else {
                &options.expanded_visual
            };
            Rc::clone(option)
        })
    }

    fn change_decorations(&self, callback: &mut dyn FnMut(&ModelDecorationsChangeAccessor<'_>)) -> Option<()> {
        self.editor.upgrade()?.change_decorations(|accessor| callback(accessor))
    }

    fn remove_decorations(&self, decoration_ids: &[String]) {
        if let Some(editor) = self.editor.upgrade() {
            editor.remove_decorations(decoration_ids);
        }
    }
}
