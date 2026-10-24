//! Porte de `src/vs/editor/contrib/find/browser/findDecorations.ts`: as decorações que marcam as ocorrências da busca
//! (`FindDecorations`), a ocorrência atual, o realce da linha e o escopo da busca.
//!
//! Representação:
//! - `IActiveCodeEditor` é um [`Weak`] do [`CodeEditorWidget`] (o editor é dono do controlador que é dono do modelo de
//!   busca, um `Rc` formaria um ciclo). O editor com modelo é o que existe enquanto as decorações existem.
//! - As seis `ModelDecorationOptions.register(...)` estáticas são `thread_local` de [`Rc<ModelDecorationOptions>`], e a
//!   igualdade de referência (`candidateOpts === FindDecorations._FIND_MATCH_DECORATION`) é [`Rc::ptr_eq`].
//! - `themeColorFromId(overviewRulerFindMatchForeground)` e `themeColorFromId(minimapFindMatch)` são
//!   [`DecorationColor::Theme`] com os ids `editorOverviewRuler.findMatchForeground` e `minimap.findMatchHighlight`.
//! - `getDecorationsInRange` do modelo (todos os donos, `ownerId` 0) é a leitura de `getCurrentMatchesPosition`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::lifecycle::IDisposable;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::cursor_common::CursorSimpleModel;
use crate::model::{
    DecorationColor, FindMatch, MinimapPosition, ModelDecorationMinimapInit, ModelDecorationOptionsInit,
    ModelDecorationOverviewRulerInit, ModelDeltaDecoration, OverviewRulerLane, TrackedRangeStickiness,
};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::ModelDecorationOptions;

/// O id da cor `editorOverviewRuler.findMatchForeground` (`overviewRulerFindMatchForeground`).
const OVERVIEW_RULER_FIND_MATCH_FOREGROUND: &str = "editorOverviewRuler.findMatchForeground";
/// O id da cor `minimap.findMatchHighlight` (`minimapFindMatch`).
const MINIMAP_FIND_MATCH: &str = "minimap.findMatchHighlight";

fn overview_ruler_find_match() -> ModelDecorationOverviewRulerInit {
    ModelDecorationOverviewRulerInit {
        color: Some(DecorationColor::Theme(OVERVIEW_RULER_FIND_MATCH_FOREGROUND.to_owned())),
        dark_color: None,
        position: Some(OverviewRulerLane::Center),
    }
}

fn minimap_find_match() -> ModelDecorationMinimapInit {
    ModelDecorationMinimapInit {
        color: Some(DecorationColor::Theme(MINIMAP_FIND_MATCH.to_owned())),
        dark_color: None,
        position: MinimapPosition::Inline,
        section_header_style: None,
        section_header_text: None,
    }
}

thread_local! {
    /// `_CURRENT_FIND_MATCH_DECORATION`.
    static CURRENT_FIND_MATCH_DECORATION: Rc<ModelDecorationOptions> =
        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "current-find-match".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            z_index: 13,
            class_name: Some("currentFindMatch".to_owned()),
            inline_class_name: Some("currentFindMatchInline".to_owned()),
            show_if_collapsed: true,
            overview_ruler: Some(overview_ruler_find_match()),
            minimap: Some(minimap_find_match()),
            ..ModelDecorationOptionsInit::default()
        });

    /// `_FIND_MATCH_DECORATION`.
    static FIND_MATCH_DECORATION: Rc<ModelDecorationOptions> =
        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "find-match".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            z_index: 10,
            class_name: Some("findMatch".to_owned()),
            inline_class_name: Some("findMatchInline".to_owned()),
            show_if_collapsed: true,
            overview_ruler: Some(overview_ruler_find_match()),
            minimap: Some(minimap_find_match()),
            ..ModelDecorationOptionsInit::default()
        });

    /// `_FIND_MATCH_NO_OVERVIEW_DECORATION`.
    static FIND_MATCH_NO_OVERVIEW_DECORATION: Rc<ModelDecorationOptions> =
        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "find-match-no-overview".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            class_name: Some("findMatch".to_owned()),
            show_if_collapsed: true,
            ..ModelDecorationOptionsInit::default()
        });

    /// `_FIND_MATCH_ONLY_OVERVIEW_DECORATION`.
    static FIND_MATCH_ONLY_OVERVIEW_DECORATION: Rc<ModelDecorationOptions> =
        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "find-match-only-overview".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            overview_ruler: Some(overview_ruler_find_match()),
            ..ModelDecorationOptionsInit::default()
        });

    /// `_RANGE_HIGHLIGHT_DECORATION`.
    static RANGE_HIGHLIGHT_DECORATION: Rc<ModelDecorationOptions> =
        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "find-range-highlight".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            class_name: Some("rangeHighlight".to_owned()),
            is_whole_line: true,
            ..ModelDecorationOptionsInit::default()
        });

    /// `_FIND_SCOPE_DECORATION`.
    static FIND_SCOPE_DECORATION: Rc<ModelDecorationOptions> =
        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "find-scope".to_owned(),
            class_name: Some("findScope".to_owned()),
            is_whole_line: true,
            ..ModelDecorationOptionsInit::default()
        });
}

/// O estado mutável de [`FindDecorations`].
struct DecorationsState {
    decorations: Vec<String>,
    overview_ruler_approximate_decorations: Vec<String>,
    find_scope_decoration_ids: Vec<String>,
    range_highlight_decoration_id: Option<String>,
    highlighted_decoration_id: Option<String>,
    start_position: Position,
}

/// `FindDecorations`.
pub struct FindDecorations {
    editor: Weak<CodeEditorWidget>,
    state: RefCell<DecorationsState>,
}

impl FindDecorations {
    pub fn new(editor: &Rc<CodeEditorWidget>) -> Self {
        Self {
            editor: Rc::downgrade(editor),
            state: RefCell::new(DecorationsState {
                decorations: Vec::new(),
                overview_ruler_approximate_decorations: Vec::new(),
                find_scope_decoration_ids: Vec::new(),
                range_highlight_decoration_id: None,
                highlighted_decoration_id: None,
                start_position: editor.get_position().expect("o editor com modelo tem posição"),
            }),
        }
    }

    /// O editor, que vive enquanto o modelo de busca existe.
    fn editor(&self) -> Rc<CodeEditorWidget> {
        self.editor.upgrade().expect("o editor vive enquanto as decorações da busca existem")
    }

    /// `this._editor.getModel().getDecorationRange(id)`.
    fn decoration_range(&self, decoration_id: &str) -> Option<Range> {
        self.editor().get_model().and_then(|model| model.get_decoration_range(decoration_id))
    }

    pub fn reset(&self) {
        let mut state = self.state.borrow_mut();
        state.decorations = Vec::new();
        state.overview_ruler_approximate_decorations = Vec::new();
        state.find_scope_decoration_ids = Vec::new();
        state.range_highlight_decoration_id = None;
        state.highlighted_decoration_id = None;
    }

    pub fn get_count(&self) -> usize {
        self.state.borrow().decorations.len()
    }

    /// `getFindScope()` (obsoleto: `getFindScopes` suporta várias seleções).
    pub fn get_find_scope(&self) -> Option<Range> {
        let id = self.state.borrow().find_scope_decoration_ids.first().cloned()?;
        self.decoration_range(&id)
    }

    pub fn get_find_scopes(&self) -> Option<Vec<Range>> {
        let ids = self.state.borrow().find_scope_decoration_ids.clone();
        if !ids.is_empty() {
            let scopes: Vec<Range> = ids.iter().filter_map(|id| self.decoration_range(id)).collect();
            if !scopes.is_empty() {
                return Some(scopes);
            }
        }
        None
    }

    pub fn get_start_position(&self) -> Position {
        self.state.borrow().start_position
    }

    pub fn set_start_position(&self, new_start_position: Position) {
        self.state.borrow_mut().start_position = new_start_position;
        self.set_current_find_match(None);
    }

    fn get_decoration_index(&self, decoration_id: &str) -> usize {
        match self.state.borrow().decorations.iter().position(|id| id == decoration_id) {
            Some(index) => index + 1,
            None => 1,
        }
    }

    pub fn get_decoration_range_at(&self, index: usize) -> Option<Range> {
        let decoration_id = self.state.borrow().decorations.get(index).cloned()?;
        self.decoration_range(&decoration_id)
    }

    pub fn get_current_matches_position(&self, desired_range: Range) -> usize {
        let Some(model) = self.editor().get_model() else {
            return 0;
        };
        let candidates = model.get_decorations_in_range(&desired_range, 0, false, false, false, false);
        for candidate in candidates {
            let is_find_decoration = CURRENT_FIND_MATCH_DECORATION.with(|current| Rc::ptr_eq(&candidate.options, current))
                || FIND_MATCH_DECORATION.with(|find_match| Rc::ptr_eq(&candidate.options, find_match))
                || FIND_MATCH_NO_OVERVIEW_DECORATION.with(|no_overview| Rc::ptr_eq(&candidate.options, no_overview));
            if is_find_decoration {
                return self.get_decoration_index(&candidate.id);
            }
        }
        // We don't know the current match position, so returns zero to show '?' in find widget
        0
    }

    pub fn set_current_find_match(&self, next_match: Option<Range>) -> usize {
        let editor = self.editor();
        let Some(model) = editor.get_model() else {
            return 0;
        };
        let mut new_current_decoration_id: Option<String> = None;
        let mut match_position = 0;
        if let Some(next_match) = next_match {
            let decorations = self.state.borrow().decorations.clone();
            for (i, decoration_id) in decorations.iter().enumerate() {
                let range = model.get_decoration_range(decoration_id);
                if range == Some(next_match) {
                    new_current_decoration_id = Some(decoration_id.clone());
                    match_position = i + 1;
                    break;
                }
            }
        }

        let highlighted_decoration_id = self.state.borrow().highlighted_decoration_id.clone();
        if highlighted_decoration_id.is_some() || new_current_decoration_id.is_some() {
            let range_highlight_decoration_id = self.state.borrow().range_highlight_decoration_id.clone();
            let result = editor.change_decorations(|change_accessor| {
                let mut highlighted = None;
                if let Some(highlighted_decoration_id) = &highlighted_decoration_id {
                    change_accessor.change_decoration_options(
                        highlighted_decoration_id,
                        FIND_MATCH_DECORATION.with(Rc::clone),
                    );
                }
                if let Some(new_current_decoration_id) = &new_current_decoration_id {
                    highlighted = Some(new_current_decoration_id.clone());
                    change_accessor.change_decoration_options(
                        new_current_decoration_id,
                        CURRENT_FIND_MATCH_DECORATION.with(Rc::clone),
                    );
                }
                if let Some(range_highlight_decoration_id) = &range_highlight_decoration_id {
                    change_accessor.remove_decoration(range_highlight_decoration_id);
                }
                let mut new_range_highlight = None;
                if let Some(new_current_decoration_id) = &new_current_decoration_id {
                    let mut rng = model
                        .get_decoration_range(new_current_decoration_id)
                        .expect("a decoração atual da busca existe");
                    if rng.start_line_number != rng.end_line_number && rng.end_column == 1 {
                        let line_before_end = rng.end_line_number - 1;
                        let line_before_end_max_column = model.get_line_max_column(line_before_end);
                        rng = Range::new(rng.start_line_number, rng.start_column, line_before_end, line_before_end_max_column);
                    }
                    new_range_highlight = Some(change_accessor.add_decoration(&rng, RANGE_HIGHLIGHT_DECORATION.with(Rc::clone)));
                }
                (highlighted, new_range_highlight)
            });
            if let Some((highlighted, new_range_highlight)) = result {
                let mut state = self.state.borrow_mut();
                state.highlighted_decoration_id = highlighted;
                state.range_highlight_decoration_id = new_range_highlight;
            }
        }

        match_position
    }

    pub fn set(&self, find_matches: &[FindMatch], find_scopes: Option<&[Range]>) {
        let editor = self.editor();
        let Some(model) = editor.get_model() else {
            return;
        };
        let (old_decorations, old_overview, old_scopes, old_range_highlight) = {
            let state = self.state.borrow();
            (
                state.decorations.clone(),
                state.overview_ruler_approximate_decorations.clone(),
                state.find_scope_decoration_ids.clone(),
                state.range_highlight_decoration_id.clone(),
            )
        };
        let layout_height = editor.get_layout_info().height;

        let result = editor.change_decorations(|accessor| {
            let mut find_matches_options = FIND_MATCH_DECORATION.with(Rc::clone);
            let mut new_overview_ruler_approximate_decorations: Vec<ModelDeltaDecoration> = Vec::new();

            if find_matches.len() > 1000 {
                // we go into a mode where the overview ruler gets "approximate" decorations
                // the reason is that the overview ruler paints all the decorations in the file and we don't want to cause freezes
                find_matches_options = FIND_MATCH_NO_OVERVIEW_DECORATION.with(Rc::clone);

                // approximate a distance in lines where matches should be merged (um `number` do JS, que vira `Infinity`
                // quando a altura do editor é zero)
                let line_count = model.get_line_count();
                let approx_pixels_per_line = layout_height / f64::from(line_count);
                let merge_lines_delta = 2.0_f64.max((3.0 / approx_pixels_per_line).ceil());

                // merge decorations as much as possible
                let mut prev_start_line_number = find_matches[0].range.start_line_number;
                let mut prev_end_line_number = find_matches[0].range.end_line_number;
                for find_match in &find_matches[1..] {
                    let range = find_match.range;
                    if f64::from(prev_end_line_number) + merge_lines_delta >= f64::from(range.start_line_number) {
                        if range.end_line_number > prev_end_line_number {
                            prev_end_line_number = range.end_line_number;
                        }
                    } else {
                        new_overview_ruler_approximate_decorations.push(ModelDeltaDecoration {
                            range: Range::new(prev_start_line_number, 1, prev_end_line_number, 1),
                            options: FIND_MATCH_ONLY_OVERVIEW_DECORATION.with(Rc::clone),
                        });
                        prev_start_line_number = range.start_line_number;
                        prev_end_line_number = range.end_line_number;
                    }
                }

                new_overview_ruler_approximate_decorations.push(ModelDeltaDecoration {
                    range: Range::new(prev_start_line_number, 1, prev_end_line_number, 1),
                    options: FIND_MATCH_ONLY_OVERVIEW_DECORATION.with(Rc::clone),
                });
            }

            // Find matches
            let new_find_matches_decorations: Vec<ModelDeltaDecoration> = find_matches
                .iter()
                .map(|find_match| ModelDeltaDecoration { range: find_match.range, options: Rc::clone(&find_matches_options) })
                .collect();
            let decorations = accessor.delta_decorations(&old_decorations, &new_find_matches_decorations);

            // Overview ruler approximate decorations
            let overview_ruler_approximate_decorations =
                accessor.delta_decorations(&old_overview, &new_overview_ruler_approximate_decorations);

            // Range highlight
            if let Some(range_highlight_decoration_id) = &old_range_highlight {
                accessor.remove_decoration(range_highlight_decoration_id);
            }

            // Find scope
            for find_scope_decoration_id in &old_scopes {
                accessor.remove_decoration(find_scope_decoration_id);
            }
            let find_scope_decoration_ids: Vec<String> = match find_scopes {
                Some(find_scopes) => find_scopes
                    .iter()
                    .map(|find_scope| accessor.add_decoration(find_scope, FIND_SCOPE_DECORATION.with(Rc::clone)))
                    .collect(),
                None => Vec::new(),
            };

            (decorations, overview_ruler_approximate_decorations, find_scope_decoration_ids)
        });

        if let Some((decorations, overview_ruler_approximate_decorations, find_scope_decoration_ids)) = result {
            let mut state = self.state.borrow_mut();
            state.decorations = decorations;
            state.overview_ruler_approximate_decorations = overview_ruler_approximate_decorations;
            state.find_scope_decoration_ids = find_scope_decoration_ids;
            state.range_highlight_decoration_id = None;
        }
    }

    pub fn match_before_position(&self, position: Position) -> Option<Range> {
        let decorations = self.state.borrow().decorations.clone();
        if decorations.is_empty() {
            return None;
        }
        for decoration_id in decorations.iter().rev() {
            let Some(r) = self.decoration_range(decoration_id) else {
                continue;
            };
            if r.end_line_number > position.line_number {
                continue;
            }
            if r.end_line_number < position.line_number {
                return Some(r);
            }
            if r.end_column > position.column {
                continue;
            }
            return Some(r);
        }

        self.decoration_range(&decorations[decorations.len() - 1])
    }

    pub fn match_after_position(&self, position: Position) -> Option<Range> {
        let decorations = self.state.borrow().decorations.clone();
        if decorations.is_empty() {
            return None;
        }
        for decoration_id in &decorations {
            let Some(r) = self.decoration_range(decoration_id) else {
                continue;
            };
            if r.start_line_number < position.line_number {
                continue;
            }
            if r.start_line_number > position.line_number {
                return Some(r);
            }
            if r.start_column < position.column {
                continue;
            }
            return Some(r);
        }

        self.decoration_range(&decorations[0])
    }

    fn all_decorations(&self) -> Vec<String> {
        let state = self.state.borrow();
        let mut result: Vec<String> = state.decorations.clone();
        result.extend(state.overview_ruler_approximate_decorations.iter().cloned());
        result.extend(state.find_scope_decoration_ids.iter().cloned());
        if let Some(range_highlight_decoration_id) = &state.range_highlight_decoration_id {
            result.push(range_highlight_decoration_id.clone());
        }
        result
    }
}

impl IDisposable for FindDecorations {
    fn dispose(&self) {
        if let Some(editor) = self.editor.upgrade() {
            editor.remove_decorations(&self.all_decorations());
        }

        self.reset();
    }
}
