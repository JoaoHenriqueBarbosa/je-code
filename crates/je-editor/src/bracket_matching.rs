//! Porte de `src/vs/editor/contrib/bracketMatching/browser/bracketMatching.ts`: o controlador que marca o par do
//! colchete sob o cursor (`BracketMatchingController`) e as ações `editor.action.jumpToBracket`,
//! `editor.action.selectToBracket` e `editor.action.removeBrackets`, com as chamadas de registro do fim do arquivo.
//!
//! Representação:
//! - `BracketMatchingController extends Disposable implements IEditorContribution` é a struct
//!   [`BracketMatchingController`], que guarda um [`Disposable`] (o `_register`) e implementa [`IEditorContribution`].
//!   `BracketMatchingController.get(editor)` é [`BracketMatchingController::get`] (o `getContribution<T>` tipado de
//!   [`ICodeEditor::get_contribution_as`]).
//! - As três ações (`extends EditorAction`) são as structs [`JumpToBracketAction`], [`SelectToBracketAction`] e
//!   [`RemoveBracketsAction`], que implementam [`EditorAction`].
//! - `type Brackets = [Range, Range]` é a dupla `(Range, Range)` que o `BracketPairsTextModelPart` devolve.
//! - O estado mutável do controlador (`_lastBracketsData`, `_lastVersionId`, `_matchBrackets`) fica num `RefCell`,
//!   porque os métodos e os ouvintes de evento trabalham por `&self`. Os ouvintes e o `RunOnceScheduler` guardam um
//!   [`Weak`] do controlador e o `editor` do controlador é um [`Weak`] do widget (o editor é dono da contribuição, um
//!   `Rc` formaria um ciclo).
//! - `_DECORATION_OPTIONS_WITH_OVERVIEW_RULER` e `_DECORATION_OPTIONS_WITHOUT_OVERVIEW_RULER` (`static readonly`)
//!   são os `thread_local` [`DECORATION_OPTIONS_WITH_OVERVIEW_RULER`] e [`DECORATION_OPTIONS_WITHOUT_OVERVIEW_RULER`].
//!   A cor `themeColorFromId(overviewRulerBracketMatchForeground)` é `DecorationColor::Theme` com o id
//!   `editorOverviewRuler.bracketMatchForeground`, que o `je-theme` já registra (o `registerColor` do topo do original).
//! - `createDecorationsCollection()` do editor é [`EditorDecorationsCollection::new`] (ver `editor_decorations_collection`).
//!
//! Divergências:
//! - O `registerThemingParticipant` do fim do arquivo (a regra CSS `.bracket-match-inline { color: ... }`) não existe:
//!   a UI nativa não usa CSS, e a cor `editorBracketMatchForeground` é lida pelo desenho dos colchetes.
//! - O `import './bracketMatching.css'` também não existe, pelo mesmo motivo.
//! - `jumpToBracket`, `selectToBracket` e `removeBrackets` leem `getSelections()` com o editor já sabidamente com
//!   modelo (`hasModel()`), então a lista de seleções nunca é `None`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::RunOnceScheduler;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::action::{CommandAction, Title, localize2};
use je_platform::actions::{AnyMenuItem, MenuItem, menu_ids, menu_registry};
use je_platform::commands::{CommandArgument, CommandDescription, CommandMetadata};
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};

use crate::code_editor_widget::{
    CodeEditorWidget, EditorContributionInstantiation, ExecuteEditsSource, ICodeEditor, IEditorContribution,
};
use crate::config::editor_options::registry::MATCH_BRACKETS;
use crate::core_commands::{editor_text_focus, key};
use crate::edit_operation::EditOperation;
use crate::editor_decorations_collection::EditorDecorationsCollection;
use crate::editor_extensions::{
    ActionLabel, ActionOptions, CommandKeybindingsOptions, CommandOptions, EditorAction, EditorActionBase,
    ServicesAccessor, register_editor_action, register_editor_contribution,
};
use crate::model::{
    DecorationColor, ModelDecorationOptionsInit, ModelDecorationOverviewRulerInit, ModelDeltaDecoration,
    OverviewRulerLane, TrackedRangeStickiness,
};
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::ModelDecorationOptions;

/// O id da cor `editorOverviewRuler.bracketMatchForeground` (`overviewRulerBracketMatchForeground`).
const OVERVIEW_RULER_BRACKET_MATCH_FOREGROUND: &str = "editorOverviewRuler.bracketMatchForeground";

/// `[Range, Range]`: o colchete de abrir e o de fechar.
type Brackets = (Range, Range);

// #region Ações

/// `JumpToBracketAction`.
pub struct JumpToBracketAction {
    action: EditorActionBase,
}

impl JumpToBracketAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    kb_opts: vec![CommandKeybindingsOptions::new(
                        KeybindingWeight::EDITOR_CONTRIB,
                        Some(editor_text_focus()),
                        Keybindings {
                            primary: Some(key(KeyMod::CTRL_CMD | KeyMod::SHIFT, KeyCode::Backslash)),
                            ..Keybindings::default()
                        },
                    )],
                    ..CommandOptions::new("editor.action.jumpToBracket")
                },
                context_menu_opts: Vec::new(),
                label: ActionLabel::Localized { label: localize2("smartSelect.jumpBracket", "Go to Bracket"), alias: None },
            }),
        }
    }
}

impl Default for JumpToBracketAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for JumpToBracketAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        if let Some(controller) = BracketMatchingController::get(editor) {
            controller.jump_to_bracket();
        }
        Promise::resolved(())
    }
}

/// `SelectToBracketAction`.
pub struct SelectToBracketAction {
    action: EditorActionBase,
}

impl SelectToBracketAction {
    pub fn new() -> Self {
        let select_brackets_property =
            Dynamic::object(vec![("type", Dynamic::from("boolean")), ("default", Dynamic::from(true))]);
        let schema = Dynamic::object(vec![
            ("type", Dynamic::from("object")),
            ("properties", Dynamic::object(vec![("selectBrackets", select_brackets_property)])),
        ]);
        let metadata = CommandMetadata {
            description: CommandDescription::Localized(localize2(
                "smartSelect.selectToBracketDescription",
                "Select the text inside and including the brackets or curly braces",
            )),
            args: Some(vec![CommandArgument { schema: Some(schema), ..CommandArgument::new("args") }]),
            returns: None,
        };
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions { metadata: Some(metadata), ..CommandOptions::new("editor.action.selectToBracket") },
                context_menu_opts: Vec::new(),
                label: ActionLabel::Localized {
                    label: localize2("smartSelect.selectToBracket", "Select to Bracket"),
                    alias: None,
                },
            }),
        }
    }
}

impl Default for SelectToBracketAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for SelectToBracketAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        let mut select_brackets = true;
        if matches!(args, Dynamic::Object(_)) && args.get_str("selectBrackets").strict_equals(&Dynamic::Bool(false)) {
            select_brackets = false;
        }
        if let Some(controller) = BracketMatchingController::get(editor) {
            controller.select_to_bracket(select_brackets);
        }
        Promise::resolved(())
    }
}

/// `RemoveBracketsAction`.
pub struct RemoveBracketsAction {
    action: EditorActionBase,
}

impl RemoveBracketsAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    kb_opts: vec![CommandKeybindingsOptions::new(
                        KeybindingWeight::EDITOR_CONTRIB,
                        Some(editor_text_focus()),
                        Keybindings {
                            primary: Some(key(KeyMod::CTRL_CMD | KeyMod::ALT, KeyCode::Backspace)),
                            ..Keybindings::default()
                        },
                    )],
                    can_trigger_inline_edits: true,
                    ..CommandOptions::new("editor.action.removeBrackets")
                },
                context_menu_opts: Vec::new(),
                label: ActionLabel::Localized { label: localize2("smartSelect.removeBrackets", "Remove Brackets"), alias: None },
            }),
        }
    }
}

impl Default for RemoveBracketsAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for RemoveBracketsAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        if let Some(controller) = BracketMatchingController::get(editor) {
            controller.remove_brackets(Some(&self.action.command.id));
        }
        Promise::resolved(())
    }
}

// #endregion

/// `BracketsData`.
#[derive(Clone)]
struct BracketsData {
    position: Position,
    brackets: Option<Brackets>,
    options: Rc<ModelDecorationOptions>,
}

/// O estado mutável do controlador.
struct ControllerState {
    last_brackets_data: Vec<BracketsData>,
    last_version_id: i64,
    /// `'never' | 'near' | 'always'`.
    match_brackets: String,
}

thread_local! {
    /// `_DECORATION_OPTIONS_WITH_OVERVIEW_RULER`.
    static DECORATION_OPTIONS_WITH_OVERVIEW_RULER: Rc<ModelDecorationOptions> =
        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "bracket-match-overview".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            class_name: Some("bracket-match".to_owned()),
            inline_class_name: Some("bracket-match-inline".to_owned()),
            overview_ruler: Some(ModelDecorationOverviewRulerInit {
                color: Some(DecorationColor::Theme(OVERVIEW_RULER_BRACKET_MATCH_FOREGROUND.to_owned())),
                dark_color: None,
                position: Some(OverviewRulerLane::Center),
            }),
            ..ModelDecorationOptionsInit::default()
        });

    /// `_DECORATION_OPTIONS_WITHOUT_OVERVIEW_RULER`.
    static DECORATION_OPTIONS_WITHOUT_OVERVIEW_RULER: Rc<ModelDecorationOptions> =
        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "bracket-match-no-overview".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            class_name: Some("bracket-match".to_owned()),
            inline_class_name: Some("bracket-match-inline".to_owned()),
            ..ModelDecorationOptionsInit::default()
        });

    /// O carregamento do módulo acontece uma vez; o registro de ações e contribuição é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// `BracketMatchingController`.
pub struct BracketMatchingController {
    base: Rc<Disposable>,
    editor: Weak<CodeEditorWidget>,
    state: RefCell<ControllerState>,
    decorations: EditorDecorationsCollection,
    update_brackets_soon: Rc<RunOnceScheduler>,
}

impl BracketMatchingController {
    pub const ID: &'static str = "editor.contrib.bracketMatchingController";

    /// `BracketMatchingController.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<BracketMatchingController>> {
        editor.get_contribution_as::<BracketMatchingController>(Self::ID)
    }

    pub fn new(editor: &Rc<CodeEditorWidget>) -> Rc<Self> {
        let this = Rc::new_cyclic(|me: &Weak<Self>| {
            let base = Disposable::new();
            let runner_me = me.clone();
            let update_brackets_soon = RunOnceScheduler::new(
                move || {
                    if let Some(this) = runner_me.upgrade() {
                        this.update_brackets();
                    }
                },
                50,
            );
            base.register(update_brackets_soon.clone());
            Self {
                base,
                editor: Rc::downgrade(editor),
                state: RefCell::new(ControllerState {
                    last_brackets_data: Vec::new(),
                    last_version_id: 0,
                    match_brackets: editor.get_option(MATCH_BRACKETS).clone(),
                }),
                decorations: EditorDecorationsCollection::new(editor, None),
                update_brackets_soon,
            }
        });

        this.update_brackets_soon.schedule(None);
        this.register_listener(editor.on_did_change_cursor_position(), |this, _| {
            if this.state.borrow().match_brackets == "never" {
                // Early exit if nothing needs to be done!
                // Leave some form of early exit check here if you wish to continue being a cursor position change listener ;)
                return;
            }

            this.update_brackets_soon.schedule(None);
        });
        this.register_listener(editor.on_did_change_model_content(), |this, _| {
            this.update_brackets_soon.schedule(None);
        });
        this.register_listener(editor.on_did_change_model(), |this, _| {
            this.reset_brackets_data();
            this.update_brackets_soon.schedule(None);
        });
        this.register_listener(editor.on_did_change_model_language_configuration(), |this, _| {
            this.reset_brackets_data();
            this.update_brackets_soon.schedule(None);
        });
        this.register_listener(editor.on_did_change_configuration(), |this, e| {
            if e.has_changed(MATCH_BRACKETS.id()) {
                if let Some(editor) = this.editor.upgrade() {
                    this.state.borrow_mut().match_brackets = editor.get_option(MATCH_BRACKETS).clone();
                }
                this.decorations.clear();
                this.reset_brackets_data();
                this.state.borrow_mut().last_version_id = 0;
                this.update_brackets_soon.schedule(None);
            }
        });

        this.register_listener(editor.on_did_blur_editor_widget(), |this, _| {
            this.update_brackets_soon.schedule(None);
        });

        this.register_listener(editor.on_did_focus_editor_widget(), |this, _| {
            this.update_brackets_soon.schedule(None);
        });
        this
    }

    /// `this._register(event(listener))`, com o callback recebendo o controlador (o `this` do original) em vez de
    /// capturá-lo: o `Weak` sobe a cada chamada, e a chamada some se o controlador já foi solto.
    fn register_listener<E: 'static>(self: &Rc<Self>, event: Event<E>, handler: impl Fn(&Self, &E) + 'static) {
        let me = Rc::downgrade(self);
        self.base.register(event.listen(move |e| {
            if let Some(this) = me.upgrade() {
                handler(&this, e);
            }
        }));
    }

    /// `this._lastBracketsData = []`.
    fn reset_brackets_data(&self) {
        self.state.borrow_mut().last_brackets_data = Vec::new();
    }

    pub fn jump_to_bracket(&self) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let (Some(model), Some(selections)) = (editor.get_model(), editor.get_selections()) else {
            return;
        };

        let bracket_pairs = model.bracket_pairs();
        let new_selections: Vec<Selection> = selections
            .iter()
            .map(|selection| {
                let position = selection.get_start_position();

                // find matching brackets if position is on a bracket
                let brackets = bracket_pairs.match_bracket(position, None);
                let mut new_cursor_position: Option<Position> = None;
                if let Some((opening, closing)) = brackets {
                    if opening.contains_position(&position) && !closing.contains_position(&position) {
                        new_cursor_position = Some(closing.get_start_position());
                    } else if closing.contains_position(&position) {
                        new_cursor_position = Some(opening.get_start_position());
                    }
                } else {
                    // find the enclosing brackets if the position isn't on a matching bracket
                    let enclosing_brackets = bracket_pairs.find_enclosing_brackets(position, None);
                    if let Some((_, closing)) = enclosing_brackets {
                        new_cursor_position = Some(closing.get_start_position());
                    } else {
                        // no enclosing brackets, try the very first next bracket
                        let next_bracket = bracket_pairs.find_next_bracket(position);
                        if let Some(next_bracket) = next_bracket {
                            new_cursor_position = Some(next_bracket.range.get_start_position());
                        }
                    }
                }

                if let Some(new_cursor_position) = new_cursor_position {
                    return Selection::new(
                        new_cursor_position.line_number,
                        new_cursor_position.column,
                        new_cursor_position.line_number,
                        new_cursor_position.column,
                    );
                }
                Selection::new(position.line_number, position.column, position.line_number, position.column)
            })
            .collect();

        editor.set_selections(&new_selections, None, None);
        editor.reveal_range(*new_selections[0], None, None, None);
    }

    pub fn select_to_bracket(&self, select_brackets: bool) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let (Some(model), Some(selections)) = (editor.get_model(), editor.get_selections()) else {
            return;
        };

        let bracket_pairs = model.bracket_pairs();
        let mut new_selections: Vec<Selection> = Vec::new();

        for selection in &selections {
            let position = selection.get_start_position();
            let mut brackets = bracket_pairs.match_bracket(position, None);

            if brackets.is_none() {
                brackets = bracket_pairs.find_enclosing_brackets(position, None);
                if brackets.is_none() {
                    let next_bracket = bracket_pairs.find_next_bracket(position);
                    if let Some(next_bracket) = next_bracket {
                        brackets = bracket_pairs.match_bracket(next_bracket.range.get_start_position(), None);
                    }
                }
            }

            if let Some((first, second)) = brackets {
                let mut sorted = [first, second];
                sorted.sort_by(|a, b| Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0));
                let [open, close] = sorted;
                let mut select_from =
                    if select_brackets { open.get_start_position() } else { open.get_end_position() };
                let mut select_to =
                    if select_brackets { close.get_end_position() } else { close.get_start_position() };

                if close.contains_position(&position) {
                    // select backwards if the cursor was on the closing bracket
                    std::mem::swap(&mut select_from, &mut select_to);
                }

                new_selections.push(Selection::new(
                    select_from.line_number,
                    select_from.column,
                    select_to.line_number,
                    select_to.column,
                ));
            }
        }

        if !new_selections.is_empty() {
            editor.set_selections(&new_selections, None, None);
            editor.reveal_range(*new_selections[0], None, None, None);
        }
    }

    pub fn remove_brackets(&self, edit_source: Option<&str>) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let (Some(model), Some(selections)) = (editor.get_model(), editor.get_selections()) else {
            return;
        };

        let bracket_pairs = model.bracket_pairs();
        for selection in &selections {
            let position = selection.get_position();

            let mut brackets = bracket_pairs.match_bracket(position, None);
            if brackets.is_none() {
                brackets = bracket_pairs.find_enclosing_brackets(position, None);
            }
            if let Some((opening, closing)) = brackets {
                editor.push_undo_stop();
                editor.execute_edits(
                    ExecuteEditsSource::Name(edit_source),
                    vec![
                        EditOperation::replace(opening, Some(JsString::new())),
                        EditOperation::replace(closing, Some(JsString::new())),
                    ],
                    None,
                );
                editor.push_undo_stop();
            }
        }
    }

    /// `_updateBrackets()`.
    fn update_brackets(&self) {
        if self.state.borrow().match_brackets == "never" {
            return;
        }
        self.recompute_brackets();

        let mut new_decorations: Vec<ModelDeltaDecoration> = Vec::new();
        for bracket_data in &self.state.borrow().last_brackets_data {
            if let Some((opening, closing)) = &bracket_data.brackets {
                new_decorations.push(ModelDeltaDecoration { range: *opening, options: Rc::clone(&bracket_data.options) });
                new_decorations.push(ModelDeltaDecoration { range: *closing, options: Rc::clone(&bracket_data.options) });
            }
        }

        self.decorations.set(&new_decorations);
    }

    /// `_recomputeBrackets()`.
    fn recompute_brackets(&self) {
        let editor = self.editor.upgrade();
        let Some(editor) = editor.filter(|editor| editor.has_model() && editor.has_widget_focus()) else {
            // no model or no focus => no brackets!
            self.reset_state();
            return;
        };
        let (Some(model), Some(selections)) = (editor.get_model(), editor.get_selections()) else {
            self.reset_state();
            return;
        };

        if selections.len() > 100 {
            // no bracket matching for high numbers of selections
            self.reset_state();
            return;
        }

        let version_id = model.get_version_id();
        let match_brackets = self.state.borrow().match_brackets.clone();
        let mut previous_data: Vec<BracketsData> = Vec::new();
        {
            let state = self.state.borrow();
            if state.last_version_id == version_id {
                // use the previous data only if the model is at the same version id
                previous_data = state.last_brackets_data.clone();
            }
        }

        // will bracket match a cursor only if the selection is collapsed
        let mut positions: Vec<Position> =
            selections.iter().filter(|selection| selection.is_empty()).map(|selection| selection.get_start_position()).collect();

        // sort positions for `previousData` cache hits
        if positions.len() > 1 {
            positions.sort_by(|a, b| Position::compare(a, b).cmp(&0));
        }

        let bracket_pairs = model.bracket_pairs();
        let mut new_data: Vec<BracketsData> = Vec::new();
        let mut previous_index = 0;
        let previous_len = previous_data.len();
        for position in positions {
            while previous_index < previous_len && previous_data[previous_index].position.is_before(&position) {
                previous_index += 1;
            }

            if previous_index < previous_len && previous_data[previous_index].position == position {
                new_data.push(previous_data[previous_index].clone());
            } else {
                // give at most 20ms to compute
                let mut brackets = bracket_pairs.match_bracket(position, Some(20));
                let mut options = DECORATION_OPTIONS_WITH_OVERVIEW_RULER.with(Rc::clone);
                if brackets.is_none() && match_brackets == "always" {
                    // give at most 20ms to compute
                    brackets = bracket_pairs.find_enclosing_brackets(position, Some(20));
                    options = DECORATION_OPTIONS_WITHOUT_OVERVIEW_RULER.with(Rc::clone);
                }
                new_data.push(BracketsData { position, brackets, options });
            }
        }

        let mut state = self.state.borrow_mut();
        state.last_brackets_data = new_data;
        state.last_version_id = version_id;
    }

    /// `this._lastBracketsData = []; this._lastVersionId = 0;`
    fn reset_state(&self) {
        let mut state = self.state.borrow_mut();
        state.last_brackets_data = Vec::new();
        state.last_version_id = 0;
    }
}

impl IDisposable for BracketMatchingController {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorContribution for BracketMatchingController {}

/// As chamadas `registerEditorContribution(...)`, `registerEditorAction(...)` e `MenuRegistry.appendMenuItem(...)` do
/// fim do arquivo (o que o carregamento do módulo faz no original). É idempotente: o registro acontece uma vez por thread.
pub fn register_bracket_matching() {
    if REGISTERED.with(|registered| registered.replace(true)) {
        return;
    }
    register_editor_contribution(
        BracketMatchingController::ID,
        Rc::new(|editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> { BracketMatchingController::new(editor) }),
        EditorContributionInstantiation::AfterFirstRender,
    );
    register_editor_action(SelectToBracketAction::new);
    register_editor_action(JumpToBracketAction::new);
    register_editor_action(RemoveBracketsAction::new);

    // Go to menu
    let mut go_to_bracket = MenuItem::new(CommandAction::new(
        "editor.action.jumpToBracket",
        Title::Plain(localize("miGoToBracket", "Go to &&Bracket", &[])),
    ));
    go_to_bracket.group = Some("5_infile_nav".to_owned());
    go_to_bracket.order = Some(2.0);
    menu_registry().append_menu_item(&menu_ids::menubar_go_menu(), AnyMenuItem::Menu(go_to_bracket));
}
