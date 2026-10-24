//! Porte de `src/vs/editor/contrib/multicursor/browser/multicursor.ts`: adicionar cursor acima e abaixo
//! (`InsertCursorAbove`, `InsertCursorBelow`), cursores no fim de cada linha selecionada, no fundo e no topo
//! (`InsertCursorAtEndOfEachLineSelected`, `InsertCursorAtEndOfLineSelected`, `InsertCursorAtTopOfLineSelected`),
//! a sessão de ocorrências da seleção (`MultiCursorSessionResult`, `MultiCursorSession`), a contribuição
//! `MultiCursorSelectionController`, as ações que a usam (`AddSelectionToNextFindMatchAction`,
//! `AddSelectionToPreviousFindMatchAction`, `MoveSelectionToNextFindMatchAction`,
//! `MoveSelectionToPreviousFindMatchAction`, `SelectHighlightsAction`, `CompatChangeAll`), o destaque das
//! ocorrências da seleção (`SelectionHighlighterState`, `SelectionHighlighter`) e a troca de foco entre cursores
//! (`FocusNextCursor`, `FocusPreviousCursor`). O que o carregamento do módulo faz no original (os
//! `registerEditorContribution` e `registerEditorAction` do fim do arquivo) é [`register_multi_cursor`].
//!
//! Representação:
//! - `ICodeEditor`/`IActiveCodeEditor` é o `Rc<CodeEditorWidget>`; o `hasModel()` que o original confere antes de usar o
//!   modelo vira o `Option` de `get_model()`/`get_view_model()`. As contribuições guardam o editor como `Weak` (o editor é
//!   quem as guarda) e os ouvintes que o original liga a `this` guardam um `Weak` da própria contribuição
//!   ([`listen_weak`]).
//! - `InsertCursorAbove`/`InsertCursorBelow` (que só diferem no `addCursorUp`/`addCursorDown`, no `reveal...MostCursor` e
//!   nos dados) são a struct [`InsertCursor`] com a direção ([`InsertDirection`]), e as funções [`insert_cursor_above`] e
//!   [`insert_cursor_below`] montam os dois. `FocusNextCursor`/`FocusPreviousCursor` são a [`FocusCursor`], do mesmo
//!   jeito. As três ações privadas que trocam as seleções por uma lista calculada (`InsertCursorAtEndOfEachLineSelected`,
//!   `InsertCursorAtEndOfLineSelected` e `InsertCursorAtTopOfLineSelected`) são a [`SelectionCursorsAction`].
//! - `abstract class MultiCursorSelectionControllerAction` com o `_run` abstrato é a struct
//!   [`MultiCursorSelectionControllerAction`]: o `_run` de cada subclasse só chama um método do controlador com os mesmos
//!   argumentos, então a struct guarda esse método como ponteiro de função ([`ControllerRun`]) e as subclasses viram as
//!   funções [`add_selection_to_next_find_match_action`] e as suas irmãs.
//! - `EditorContextKeys.focus` e `editorTextFocus` são as expressões `has("editorFocus")` e `has("editorTextFocus")`,
//!   como as demais chaves de contexto do porte (ver `core_commands`).
//! - `MultiCursorSelectionController.get(editor)` e `SelectionHighlighter` leem a contribuição do editor por tipo, pelo
//!   [`IEditorContribution::as_any`].
//! - `string | undefined` de `args.source` é `Option<String>`; `args.logicalLine === false` só vale com `false` mesmo.
//! - `MultiCursorSession.currentMatch` (um campo mutável) é um `Cell<Option<Selection>>`.
//! - `Constants.MAX_SAFE_SMALL_INTEGER` é o limite de resultados de `selectAll`; o `findMatches` sem limite do
//!   `SelectionHighlighterState` usa o padrão do modelo ([`LIMIT_FIND_COUNT`]).
//! - `RunOnceScheduler(..., 300)` é o `RunOnceScheduler` de `je_base::async_`.
//!
//! Divergências:
//! - **FindController.** O `CommonFindController` e o `FindReplaceState` (`contrib/find`) ainda não foram portados. O que o
//!   multicursor chama deles é o mínimo, definido aqui: [`ICommonFindController`] (`getState`, `highlightFindOptions`),
//!   [`IFindReplaceState`] (os campos lidos, `change` e `onFindReplaceStateChange`), [`INewFindReplaceState`] e
//!   [`FindReplaceStateChangedEvent`] (só com os campos que o multicursor e o seu teste usam: `filters`, que é genérico,
//!   fica de fora) e [`FindOptionOverride`]. `CommonFindController.get(editor)` e o
//!   `createInstance(CommonFindController, editor)` que a ação usa quando o editor não tem o controlador são o
//!   [`ICommonFindControllerSource`], entregue pelo registro e pelos construtores (o original os acha em um singleton e no
//!   `accessor`). `getGlobalBufferTerm` e `setSearchString` não são chamados por este módulo e ficam fora.
//! - **`ILanguageFeaturesService`.** O `SelectionHighlighter` só pergunta `documentHighlightProvider.has(model)`: é o
//!   [`IDocumentHighlightProviderRegistry`], entregue pelo construtor.
//! - **`aria.status`.** `announceCursorChange` monta a mensagem e a entrega ao `status` do aria (`base/browser/ui/aria`),
//!   que não tem porte (é DOM): [`announce_cursor_change`] devolve a mensagem e as ações a descartam.
//! - **Decorações.** `getSelectionHighlightDecorationOptions` é do `highlightDecorations.ts` do `wordHighlighter`, que
//!   ainda não tem porte: as duas `ModelDecorationOptions` de seleção ficam aqui, com as cores do tema por id.
//! - **`_expandEmptyToWord(model, selection)`** ignora o `model` no original; aqui o parâmetro não existe.
//! - **`SelectionHighlighter` e `MultiCursorSelectionController` (`IEditorContribution`).** O `EditorContributionCtor` do
//!   registro não recebe serviços, então [`register_multi_cursor`] recebe os dois serviços e os captura nos construtores.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::RunOnceScheduler;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;
use je_platform::action::localize2;
use je_platform::actions::menu_ids;
use je_platform::commands::CommandMetadata;
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression};
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};

use crate::editor_decorations_collection::EditorDecorationsCollection;
use crate::code_editor_widget::{
    CodeEditorWidget, EditorContributionCtor, EditorContributionInstantiation, ICodeEditor,
    IEditorContribution,
};
use crate::config::editor_options::registry::{
    SELECTED_TEXT_MATCH_MODE, SELECTION_HIGHLIGHT, SELECTION_HIGHLIGHT_MAX_LENGTH, SELECTION_HIGHLIGHT_MULTILINE,
    OCCURRENCES_HIGHLIGHT, WORD_SEPARATORS,
};
use crate::core_commands::{editor_text_focus, key, platform_keybindings, writable};
use crate::cursor_common::{CursorSimpleModel, CursorState, PartialCursorState};
use crate::cursor_move_commands::{add_cursor_down, add_cursor_up};
use crate::editor_common::ScrollType;
use crate::editor_extensions::{
    ActionLabel, ActionOptions, CommandKeybindingsOptions, CommandMenuOptions, CommandOptions, EditorAction,
    EditorActionBase, EditorActionContextMenuOptions, ServicesAccessor, register_editor_action,
    register_editor_contribution,
};
use crate::model::{
    DecorationColor, FindMatch, MinimapPosition, ModelDecorationMinimapInit, ModelDecorationOptionsInit,
    ModelDecorationOverviewRulerInit, ModelDeltaDecoration, OverviewRulerLane, TrackedRangeStickiness,
};
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::{FindScope, ModelDecorationOptions, TextModel};
use crate::text_model_search::LIMIT_FIND_COUNT;
use crate::view_model_types::{CursorChangeReason, IViewModel};

// #region Serviços (recortes do FindController e dos recursos de linguagem)

/// `FindOptionOverride` (`findState.ts`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FindOptionOverride {
    #[default]
    NotSet = 0,
    True = 1,
    False = 2,
}

/// `INewFindReplaceState` (`findState.ts`), sem `filters` (ver "Divergências"). Cada campo `None` é o `undefined`
/// do original, que não muda o estado.
#[derive(Clone, Debug, Default)]
pub struct INewFindReplaceState {
    pub search_string: Option<JsString>,
    pub replace_string: Option<JsString>,
    pub is_revealed: Option<bool>,
    pub is_replace_revealed: Option<bool>,
    pub is_regex: Option<bool>,
    pub is_regex_override: Option<FindOptionOverride>,
    pub whole_word: Option<bool>,
    pub whole_word_override: Option<FindOptionOverride>,
    pub match_case: Option<bool>,
    pub match_case_override: Option<FindOptionOverride>,
    pub preserve_case: Option<bool>,
    pub preserve_case_override: Option<FindOptionOverride>,
    /// `searchScope?: Range[] | null`: `None` é `undefined`, `Some(None)` é `null`.
    pub search_scope: Option<Option<Vec<Range>>>,
    pub r#loop: Option<bool>,
    pub is_searching: Option<bool>,
}

/// `FindReplaceStateChangedEvent` (`findState.ts`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindReplaceStateChangedEvent {
    pub move_cursor: bool,
    pub update_history: bool,

    pub search_string: bool,
    pub replace_string: bool,
    pub is_revealed: bool,
    pub is_replace_revealed: bool,
    pub is_regex: bool,
    pub whole_word: bool,
    pub match_case: bool,
    pub preserve_case: bool,
    pub search_scope: bool,
    pub matches_position: bool,
    pub matches_count: bool,
    pub current_match: bool,
    pub r#loop: bool,
    pub is_searching: bool,
    pub filters: bool,
}

/// A fatia do `FindReplaceState` que o multicursor usa (ver "Divergências"). Os valores são os efetivos (com o
/// `Override` aplicado), como os acessos `findState.matchCase`, `findState.wholeWord` e `findState.isRegex` do original.
pub trait IFindReplaceState {
    /// `searchString`.
    fn search_string(&self) -> JsString;
    /// `isRevealed`.
    fn is_revealed(&self) -> bool;
    /// `isRegex`.
    fn is_regex(&self) -> bool;
    /// `wholeWord`.
    fn whole_word(&self) -> bool;
    /// `matchCase`.
    fn match_case(&self) -> bool;
    /// `searchScope`.
    fn search_scope(&self) -> Option<Vec<Range>>;
    /// `change(newState, moveCursor)` (o `updateHistory` fica no padrão).
    fn change(&self, new_state: &INewFindReplaceState, move_cursor: bool);
    /// `onFindReplaceStateChange`.
    fn on_find_replace_state_change(&self) -> Event<FindReplaceStateChangedEvent>;
}

/// A fatia do `CommonFindController` que o multicursor usa (ver "Divergências"). O `dispose` é o do
/// `IDisposable`: a ação descarta o controlador que ela mesma criou.
pub trait ICommonFindController: IDisposable {
    /// `getState()`.
    fn get_state(&self) -> Rc<dyn IFindReplaceState>;
    /// `highlightFindOptions(ignoreWhenVisible)`.
    fn highlight_find_options(&self, ignore_when_visible: bool);
}

/// `CommonFindController.get(editor)` e `createInstance(CommonFindController, editor)` (ver "Divergências").
pub trait ICommonFindControllerSource {
    /// `CommonFindController.get(editor)`: o controlador do editor, se ele tem.
    fn get(&self, editor: &CodeEditorWidget) -> Option<Rc<dyn ICommonFindController>>;
    /// `accessor.get(IInstantiationService).createInstance(CommonFindController, editor)`: um controlador novo, que
    /// quem o pede descarta.
    fn create_instance(&self, editor: &Rc<CodeEditorWidget>) -> Rc<dyn ICommonFindController>;
}

/// `ILanguageFeaturesService.documentHighlightProvider`, só com o `has(model)` (ver "Divergências").
pub trait IDocumentHighlightProviderRegistry {
    /// `has(model)`.
    fn has(&self, model: &TextModel) -> bool;
}

// #endregion

// #region Ajudas

/// O `Weak` de uma contribuição (o `this` do original) no ouvinte: a chamada some se a contribuição já foi solta.
fn listen_weak<T: 'static, E: 'static>(owner: &Weak<T>, event: Event<E>, handler: impl Fn(&T, &E) + 'static) -> DisposableRef {
    let owner = owner.clone();
    event.listen(move |e| {
        if let Some(owner) = owner.upgrade() {
            handler(&*owner, e);
        }
    })
}

/// `args.source` (`string | undefined`).
fn source_arg(args: &Dynamic) -> Option<String> {
    match args {
        Dynamic::Object(_) => match args.get_str("source") {
            Dynamic::String(source) => Some(source.to_string_lossy()),
            _ => None,
        },
        _ => None,
    }
}

/// `EditorContextKeys.focus` (a chave `editorFocus`).
fn editor_focus() -> ContextKeyExpression {
    ContextKeyExpr::has("editorFocus")
}

/// O `kbOpts` de uma ação deste módulo: peso `EditorContrib`, com o `kbExpr` e os atalhos.
fn contrib_keybindings(kb_expr: ContextKeyExpression, keybindings: Keybindings) -> Vec<CommandKeybindingsOptions> {
    vec![CommandKeybindingsOptions::new(KeybindingWeight::EDITOR_CONTRIB, Some(kb_expr), keybindings)]
}

/// O `menuOpts` de uma ação deste módulo: o menu Seleção, grupo `3_multi`.
fn selection_menu(order: f64, key: &str, title: &str) -> Vec<CommandMenuOptions> {
    vec![CommandMenuOptions::new(menu_ids::menubar_selection_menu(), "3_multi", order, localize(key, title, &[]))]
}

/// O `label: nls.localize2(key, message)` de uma ação.
fn localized_label(key: &str, message: &str) -> ActionLabel {
    ActionLabel::Localized { label: localize2(key, message), alias: None }
}

/// As opções de uma ação deste módulo sem menu de contexto.
fn action_options(command: CommandOptions, label: ActionLabel) -> ActionOptions {
    ActionOptions { command, context_menu_opts: Vec::new(), label }
}

/// `announceCursorChange(previousCursorState, cursorState)`: devolve a mensagem que o original entrega ao `status` do aria
/// (ver "Divergências"), ou `None` se nenhum cursor é novo.
pub fn announce_cursor_change(previous_cursor_state: &[CursorState], cursor_state: &[CursorState]) -> Option<String> {
    let cursor_diff: Vec<&CursorState> =
        cursor_state.iter().filter(|cs| !previous_cursor_state.iter().any(|pcs| pcs.equals(cs))).collect();
    if cursor_diff.is_empty() {
        return None;
    }
    let cursor_positions = cursor_diff
        .iter()
        .map(|cs| format!("line {} column {}", cs.view_state.position.line_number, cs.view_state.position.column))
        .collect::<Vec<_>>()
        .join(", ");
    Some(if cursor_diff.len() == 1 {
        localize("cursorAdded", "Cursor added: {0}", &[cursor_positions.as_str()])
    } else {
        localize("cursorsAdded", "Cursors added: {0}", &[cursor_positions.as_str()])
    })
}

// #endregion

// #region Adicionar cursor acima e abaixo

/// Para onde [`InsertCursor`] adiciona o cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertDirection {
    Above,
    Below,
}

/// `InsertCursorAbove` e `InsertCursorBelow`.
pub struct InsertCursor {
    action: EditorActionBase,
    direction: InsertDirection,
}

/// `InsertCursorAbove`.
pub fn insert_cursor_above() -> InsertCursor {
    InsertCursor {
        action: EditorActionBase::new(ActionOptions {
            command: CommandOptions {
                kb_opts: contrib_keybindings(
                    editor_text_focus(),
                    Keybindings {
                        primary: Some(key(KeyMod::CTRL_CMD | KeyMod::ALT, KeyCode::UpArrow)),
                        linux: platform_keybindings(
                            key(KeyMod::SHIFT | KeyMod::ALT, KeyCode::UpArrow),
                            Some(vec![key(KeyMod::CTRL_CMD | KeyMod::SHIFT, KeyCode::UpArrow)]),
                        ),
                        ..Keybindings::default()
                    },
                ),
                menu_opts: selection_menu(2.0, "miInsertCursorAbove", "&&Add Cursor Above"),
                ..CommandOptions::new("editor.action.insertCursorAbove")
            },
            context_menu_opts: Vec::new(),
            label: localized_label("mutlicursor.insertAbove", "Add Cursor Above"),
        }),
        direction: InsertDirection::Above,
    }
}

/// `InsertCursorBelow`.
pub fn insert_cursor_below() -> InsertCursor {
    InsertCursor {
        action: EditorActionBase::new(ActionOptions {
            command: CommandOptions {
                kb_opts: contrib_keybindings(
                    editor_text_focus(),
                    Keybindings {
                        primary: Some(key(KeyMod::CTRL_CMD | KeyMod::ALT, KeyCode::DownArrow)),
                        linux: platform_keybindings(
                            key(KeyMod::SHIFT | KeyMod::ALT, KeyCode::DownArrow),
                            Some(vec![key(KeyMod::CTRL_CMD | KeyMod::SHIFT, KeyCode::DownArrow)]),
                        ),
                        ..Keybindings::default()
                    },
                ),
                menu_opts: selection_menu(3.0, "miInsertCursorBelow", "A&&dd Cursor Below"),
                ..CommandOptions::new("editor.action.insertCursorBelow")
            },
            context_menu_opts: Vec::new(),
            label: localized_label("mutlicursor.insertBelow", "Add Cursor Below"),
        }),
        direction: InsertDirection::Below,
    }
}

impl EditorAction for InsertCursor {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        let Some(view_model) = editor.get_view_model() else {
            return Promise::resolved(());
        };

        let use_logical_line =
            !(matches!(args, Dynamic::Object(_)) && matches!(args.get_str("logicalLine"), Dynamic::Bool(false)));

        if view_model.cursor_config().read_only {
            return Promise::resolved(());
        }

        let source = source_arg(args);
        view_model.model().push_stack_element();
        let previous_cursor_state = view_model.get_cursor_states();
        let new_states = match self.direction {
            InsertDirection::Above => add_cursor_up(&*view_model, &previous_cursor_state, use_logical_line),
            InsertDirection::Below => add_cursor_down(&*view_model, &previous_cursor_state, use_logical_line),
        };
        view_model.set_cursor_states(source.as_deref(), CursorChangeReason::Explicit, Some(new_states));
        match self.direction {
            InsertDirection::Above => view_model.reveal_top_most_cursor(source.as_deref()),
            InsertDirection::Below => view_model.reveal_bottom_most_cursor(source.as_deref()),
        }
        let _announcement = announce_cursor_change(&previous_cursor_state, &view_model.get_cursor_states());
        Promise::resolved(())
    }
}

// #endregion

// #region Cursores a partir das seleções

/// Qual das três ações privadas de [`SelectionCursorsAction`] roda.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionCursorsKind {
    /// `InsertCursorAtEndOfEachLineSelected`.
    EndOfEachLineSelected,
    /// `InsertCursorAtEndOfLineSelected` (`editor.action.addCursorsToBottom`).
    ToBottom,
    /// `InsertCursorAtTopOfLineSelected` (`editor.action.addCursorsToTop`).
    ToTop,
}

/// `InsertCursorAtEndOfEachLineSelected`, `InsertCursorAtEndOfLineSelected` e `InsertCursorAtTopOfLineSelected`.
struct SelectionCursorsAction {
    action: EditorActionBase,
    kind: SelectionCursorsKind,
}

impl SelectionCursorsAction {
    fn end_of_each_line_selected() -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    kb_opts: contrib_keybindings(
                        editor_text_focus(),
                        Keybindings {
                            primary: Some(key(KeyMod::SHIFT | KeyMod::ALT, KeyCode::KeyI)),
                            ..Keybindings::default()
                        },
                    ),
                    menu_opts: selection_menu(4.0, "miInsertCursorAtEndOfEachLineSelected", "Add C&&ursors to Line Ends"),
                    ..CommandOptions::new("editor.action.insertCursorAtEndOfEachLineSelected")
                },
                context_menu_opts: Vec::new(),
                label: localized_label("mutlicursor.insertAtEndOfEachLineSelected", "Add Cursors to Line Ends"),
            }),
            kind: SelectionCursorsKind::EndOfEachLineSelected,
        }
    }

    fn to_bottom() -> Self {
        Self {
            action: EditorActionBase::new(action_options(
                CommandOptions::new("editor.action.addCursorsToBottom"),
                localized_label("mutlicursor.addCursorsToBottom", "Add Cursors to Bottom"),
            )),
            kind: SelectionCursorsKind::ToBottom,
        }
    }

    fn to_top() -> Self {
        Self {
            action: EditorActionBase::new(action_options(
                CommandOptions::new("editor.action.addCursorsToTop"),
                localized_label("mutlicursor.addCursorsToTop", "Add Cursors to Top"),
            )),
            kind: SelectionCursorsKind::ToTop,
        }
    }

    /// `getCursorsForSelection(selection, model, result)`.
    fn cursors_for_selection(selection: &Selection, model: &TextModel, result: &mut Vec<Selection>) {
        if selection.is_empty() {
            return;
        }

        for i in selection.start_line_number..selection.end_line_number {
            let current_line_max_column = CursorSimpleModel::get_line_max_column(model, i);
            result.push(Selection::new(i, current_line_max_column, i, current_line_max_column));
        }
        if selection.end_column > 1 {
            result.push(Selection::new(
                selection.end_line_number,
                selection.end_column,
                selection.end_line_number,
                selection.end_column,
            ));
        }
    }
}

impl EditorAction for SelectionCursorsAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(model), Some(selections), Some(view_model)) =
            (editor.get_model(), editor.get_selections(), editor.get_view_model())
        else {
            return Promise::resolved(());
        };

        let previous_cursor_state = view_model.get_cursor_states();
        let mut new_selections: Vec<Selection> = Vec::new();
        match self.kind {
            SelectionCursorsKind::EndOfEachLineSelected => {
                for selection in &selections {
                    Self::cursors_for_selection(selection, &model, &mut new_selections);
                }
            }
            SelectionCursorsKind::ToBottom => {
                let line_count = CursorSimpleModel::get_line_count(&*model);
                for i in selections[0].start_line_number..=line_count {
                    new_selections.push(Selection::new(i, selections[0].start_column, i, selections[0].end_column));
                }
            }
            SelectionCursorsKind::ToTop => {
                for i in (1..=selections[0].start_line_number).rev() {
                    new_selections.push(Selection::new(i, selections[0].start_column, i, selections[0].end_column));
                }
            }
        }

        if !new_selections.is_empty() {
            editor.set_selections(&new_selections, None, None);
        }
        let _announcement = announce_cursor_change(&previous_cursor_state, &view_model.get_cursor_states());
        Promise::resolved(())
    }
}

// #endregion

// #region Sessão

/// `MultiCursorSessionResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiCursorSessionResult {
    pub selections: Vec<Selection>,
    pub reveal_range: Range,
    pub reveal_scroll_type: ScrollType,
}

/// `isFindWidgetSearch(editor, findController)`: o widget de busca é dono do que se procura se o foco não está no editor,
/// o widget está visível e o texto da busca não é vazio.
fn is_find_widget_search(editor: &CodeEditorWidget, find_controller: &dyn ICommonFindController) -> bool {
    let find_state = find_controller.get_state();
    !editor.has_text_focus() && find_state.is_revealed() && !find_state.search_string().is_empty()
}

/// O resultado de `getSelectionSearchOptions`.
struct SelectionSearchOptions {
    whole_word: bool,
    match_case: bool,
    uses_find_options: bool,
}

/// `getSelectionSearchOptions(editor, findController)`.
fn get_selection_search_options(editor: &CodeEditorWidget, find_controller: &dyn ICommonFindController) -> SelectionSearchOptions {
    let selected_text_match_mode = editor.get_option(SELECTED_TEXT_MATCH_MODE);
    if selected_text_match_mode == "findOptions" {
        let find_state = find_controller.get_state();
        return SelectionSearchOptions {
            whole_word: find_state.whole_word(),
            match_case: find_state.match_case(),
            uses_find_options: true,
        };
    }
    SelectionSearchOptions { whole_word: false, match_case: selected_text_match_mode == "caseSensitive", uses_find_options: false }
}

/// `text.replace(/\r\n/g, '\n')`.
fn normalize_crlf(text: &JsString) -> JsString {
    let units = text.units();
    let mut normalized = JsString::default();
    let mut i = 0;
    while i < units.len() {
        if units[i] == 13 && units.get(i + 1) == Some(&10) {
            normalized.push_unit(10);
            i += 2;
        } else {
            normalized.push_unit(units[i]);
            i += 1;
        }
    }
    normalized
}

/// `MultiCursorSession`.
pub struct MultiCursorSession {
    editor: Rc<CodeEditorWidget>,
    pub find_controller: Rc<dyn ICommonFindController>,
    pub is_disconnected_from_find_controller: bool,
    pub uses_find_options: bool,
    pub search_text: JsString,
    pub whole_word: bool,
    pub match_case: bool,
    current_match: Cell<Option<Selection>>,
}

impl MultiCursorSession {
    /// `MultiCursorSession.create(editor, findController)`.
    pub fn create(editor: &Rc<CodeEditorWidget>, find_controller: &Rc<dyn ICommonFindController>) -> Option<MultiCursorSession> {
        let model = editor.get_model()?;
        let find_state = find_controller.get_state();

        if is_find_widget_search(editor, &**find_controller) {
            // Find widget owns what is searched for
            return Some(MultiCursorSession {
                editor: editor.clone(),
                find_controller: find_controller.clone(),
                is_disconnected_from_find_controller: false,
                uses_find_options: true,
                search_text: find_state.search_string(),
                whole_word: find_state.whole_word(),
                match_case: find_state.match_case(),
                current_match: Cell::new(None),
            });
        }

        let mut is_disconnected_from_find_controller = false;
        let whole_word;
        let match_case;
        let uses_find_options;
        let selections = editor.get_selections()?;
        if selections.len() == 1 && selections[0].is_empty() {
            is_disconnected_from_find_controller = true;
            whole_word = true;
            match_case = true;
            uses_find_options = true;
        } else {
            let selection_search_options = get_selection_search_options(editor, &**find_controller);
            whole_word = selection_search_options.whole_word;
            match_case = selection_search_options.match_case;
            uses_find_options = selection_search_options.uses_find_options;
        }

        // Selection owns what is searched for
        let s = editor.get_selection()?;

        let search_text;
        let mut current_match = None;

        if s.is_empty() {
            // selection is empty => expand to current word
            let word = editor.get_configured_word_at_position(s.get_start_position())?;
            search_text = word.word;
            current_match = Some(Selection::new(s.start_line_number, word.start_column, s.start_line_number, word.end_column));
        } else {
            search_text = normalize_crlf(&model.get_value_in_range(&s));
        }

        Some(MultiCursorSession {
            editor: editor.clone(),
            find_controller: find_controller.clone(),
            is_disconnected_from_find_controller,
            uses_find_options,
            search_text,
            whole_word,
            match_case,
            current_match: Cell::new(current_match),
        })
    }

    /// `currentMatch`.
    pub fn current_match(&self) -> Option<Selection> {
        self.current_match.get()
    }

    /// `this.wholeWord ? this._editor.getOption(EditorOption.wordSeparators) : null`.
    fn word_separators(&self) -> Option<&str> {
        self.whole_word.then(|| self.editor.get_option(WORD_SEPARATORS).as_str())
    }

    /// O resultado que adiciona `selection` às seleções do editor.
    fn result_with(selections: Vec<Selection>, selection: Selection) -> MultiCursorSessionResult {
        MultiCursorSessionResult { selections, reveal_range: *selection, reveal_scroll_type: ScrollType::Smooth }
    }

    /// `addSelectionToNextFindMatch()`.
    pub fn add_selection_to_next_find_match(&self) -> Option<MultiCursorSessionResult> {
        let next_match = self.get_next_match()?;
        let mut selections = self.editor.get_selections()?;
        selections.push(next_match);
        Some(Self::result_with(selections, next_match))
    }

    /// `moveSelectionToNextFindMatch()`.
    pub fn move_selection_to_next_find_match(&self) -> Option<MultiCursorSessionResult> {
        let next_match = self.get_next_match()?;
        let mut selections = self.editor.get_selections()?;
        selections.pop();
        selections.push(next_match);
        Some(Self::result_with(selections, next_match))
    }

    /// `_getNextMatch()`.
    fn get_next_match(&self) -> Option<Selection> {
        let model = self.editor.get_model()?;

        if let Some(result) = self.current_match.take() {
            return Some(result);
        }

        self.highlight_find_options();

        let all_selections = self.editor.get_selections()?;
        let last_added_selection = all_selections.last()?;
        let next_match = model.find_next_match(
            self.search_text.clone(),
            last_added_selection.get_end_position(),
            false,
            self.match_case,
            self.word_separators(),
            false,
        )?;
        Some(Selection::new(
            next_match.range.start_line_number,
            next_match.range.start_column,
            next_match.range.end_line_number,
            next_match.range.end_column,
        ))
    }

    /// `addSelectionToPreviousFindMatch()`.
    pub fn add_selection_to_previous_find_match(&self) -> Option<MultiCursorSessionResult> {
        let previous_match = self.get_previous_match()?;
        let mut selections = self.editor.get_selections()?;
        selections.push(previous_match);
        Some(Self::result_with(selections, previous_match))
    }

    /// `moveSelectionToPreviousFindMatch()`.
    pub fn move_selection_to_previous_find_match(&self) -> Option<MultiCursorSessionResult> {
        let previous_match = self.get_previous_match()?;
        let mut selections = self.editor.get_selections()?;
        selections.pop();
        selections.push(previous_match);
        Some(Self::result_with(selections, previous_match))
    }

    /// `_getPreviousMatch()`.
    fn get_previous_match(&self) -> Option<Selection> {
        let model = self.editor.get_model()?;

        if let Some(result) = self.current_match.take() {
            return Some(result);
        }

        self.highlight_find_options();

        let all_selections = self.editor.get_selections()?;
        let last_added_selection = all_selections.last()?;
        let previous_match = model.find_previous_match(
            self.search_text.clone(),
            last_added_selection.get_start_position(),
            false,
            self.match_case,
            self.word_separators(),
            false,
        )?;
        Some(Selection::new(
            previous_match.range.start_line_number,
            previous_match.range.start_column,
            previous_match.range.end_line_number,
            previous_match.range.end_column,
        ))
    }

    /// `selectAll(searchScope)`.
    pub fn select_all(&self, search_scope: Option<Vec<Range>>) -> Vec<FindMatch> {
        let Some(editor_model) = self.editor.get_model() else {
            return Vec::new();
        };

        self.highlight_find_options();

        let scope = match search_scope {
            Some(ranges) => FindScope::Ranges(ranges),
            None => FindScope::Whole,
        };
        editor_model.find_matches(
            self.search_text.clone(),
            scope,
            false,
            self.match_case,
            self.word_separators(),
            false,
            MAX_SAFE_SMALL_INTEGER as usize,
        )
    }

    /// `_highlightFindOptions()`.
    fn highlight_find_options(&self) {
        if self.uses_find_options {
            self.find_controller.highlight_find_options(false);
        }
    }
}

// #endregion

// #region MultiCursorSelectionController

/// `MultiCursorSelectionController`.
pub struct MultiCursorSelectionController {
    me: Weak<MultiCursorSelectionController>,
    base: Rc<Disposable>,
    editor: Weak<CodeEditorWidget>,
    ignore_selection_change: Cell<bool>,
    session: RefCell<Option<Rc<MultiCursorSession>>>,
    session_dispose: Rc<DisposableStore>,
}

impl MultiCursorSelectionController {
    pub const ID: &'static str = "editor.contrib.multiCursorController";

    pub fn new(editor: &Rc<CodeEditorWidget>) -> Rc<Self> {
        let base = Disposable::new();
        let session_dispose = DisposableStore::new();
        base.register(session_dispose.clone());
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            editor: Rc::downgrade(editor),
            ignore_selection_change: Cell::new(false),
            session: RefCell::new(None),
            session_dispose,
        })
    }

    /// `MultiCursorSelectionController.get(editor)`.
    pub fn get(editor: &CodeEditorWidget) -> Option<Rc<MultiCursorSelectionController>> {
        let contribution = editor.get_contribution(Self::ID)?;
        contribution.as_any()?.downcast_ref::<MultiCursorSelectionController>()?.me.upgrade()
    }

    /// O editor dono da contribuição (que a descarta antes de ser solto).
    fn editor(&self) -> Rc<CodeEditorWidget> {
        self.editor.upgrade().expect("o editor vive mais que as suas contribuições")
    }

    /// `_beginSessionIfNeeded(findController)`.
    fn begin_session_if_needed(&self, find_controller: &Rc<dyn ICommonFindController>) {
        if self.session.borrow().is_some() {
            return;
        }
        let editor = self.editor();

        // Create a new session
        let Some(session) = MultiCursorSession::create(&editor, find_controller) else {
            return;
        };
        let session = Rc::new(session);
        *self.session.borrow_mut() = Some(session.clone());

        let mut new_state = INewFindReplaceState { search_string: Some(session.search_text.clone()), ..Default::default() };
        if session.is_disconnected_from_find_controller {
            new_state.whole_word_override = Some(FindOptionOverride::True);
            new_state.match_case_override = Some(FindOptionOverride::True);
            new_state.is_regex_override = Some(FindOptionOverride::False);
        }
        find_controller.get_state().change(&new_state, false);

        self.session_dispose.add(listen_weak(&self.me, editor.on_did_change_cursor_selection(), |controller, _e| {
            if controller.ignore_selection_change.get() {
                return;
            }
            controller.end_session();
        }));
        self.session_dispose.add(listen_weak(&self.me, editor.on_did_blur_editor_text(), |controller, _e| {
            controller.end_session();
        }));
        if editor.get_option(SELECTED_TEXT_MATCH_MODE) != "findOptions" {
            self.session_dispose.add(listen_weak(&self.me, editor.on_did_focus_editor_text(), |controller, _e| {
                controller.end_session();
            }));
        }
        self.session_dispose.add(listen_weak(&self.me, editor.on_did_change_configuration(), |controller, e| {
            if e.has_changed(SELECTED_TEXT_MATCH_MODE.id()) {
                controller.end_session();
            }
        }));
        let uses_find_options = session.uses_find_options;
        self.session_dispose.add(listen_weak(
            &self.me,
            find_controller.get_state().on_find_replace_state_change(),
            move |controller, e| {
                if uses_find_options && (e.match_case || e.whole_word) {
                    controller.end_session();
                }
            },
        ));
    }

    /// `_endSession()`.
    fn end_session(&self) {
        self.session_dispose.clear();
        let session = self.session.borrow().clone();
        if let Some(session) = session
            && session.is_disconnected_from_find_controller
        {
            let new_state = INewFindReplaceState {
                whole_word_override: Some(FindOptionOverride::NotSet),
                match_case_override: Some(FindOptionOverride::NotSet),
                is_regex_override: Some(FindOptionOverride::NotSet),
                ..Default::default()
            };
            session.find_controller.get_state().change(&new_state, false);
        }
        *self.session.borrow_mut() = None;
    }

    /// `_setSelections(selections)`.
    fn set_selections(&self, selections: &[Selection]) {
        self.ignore_selection_change.set(true);
        self.editor().set_selections(selections, None, None);
        self.ignore_selection_change.set(false);
    }

    /// `_expandEmptyToWord(model, selection)`.
    fn expand_empty_to_word(&self, selection: Selection) -> Selection {
        if !selection.is_empty() {
            return selection;
        }
        let Some(word) = self.editor().get_configured_word_at_position(selection.get_start_position()) else {
            return selection;
        };
        Selection::new(selection.start_line_number, word.start_column, selection.start_line_number, word.end_column)
    }

    /// `_applySessionResult(result)`.
    fn apply_session_result(&self, result: Option<MultiCursorSessionResult>) {
        let Some(result) = result else {
            return;
        };
        self.set_selections(&result.selections);
        self.editor().reveal_range_in_center_if_outside_viewport(result.reveal_range, Some(result.reveal_scroll_type));
    }

    /// `getSession(findController)`.
    pub fn get_session(&self) -> Option<Rc<MultiCursorSession>> {
        self.session.borrow().clone()
    }

    /// `addSelectionToNextFindMatch(findController)`.
    pub fn add_selection_to_next_find_match(&self, find_controller: &Rc<dyn ICommonFindController>) {
        let editor = self.editor();
        let Some(model) = editor.get_model() else {
            return;
        };
        if self.session.borrow().is_none() {
            // If there are multiple cursors, handle the case where they do not all select the same text.
            let Some(all_selections) = editor.get_selections() else {
                return;
            };
            if all_selections.len() > 1 {
                let match_case = if is_find_widget_search(&editor, &**find_controller) {
                    find_controller.get_state().match_case()
                } else {
                    get_selection_search_options(&editor, &**find_controller).match_case
                };
                let selections_contain_same_text = model_ranges_contain_same_text(&model, &all_selections, match_case);
                if !selections_contain_same_text {
                    let resulting_selections: Vec<Selection> =
                        all_selections.iter().map(|selection| self.expand_empty_to_word(*selection)).collect();
                    editor.set_selections(&resulting_selections, None, None);
                    return;
                }
            }
        }
        self.begin_session_if_needed(find_controller);
        if let Some(session) = self.get_session() {
            self.apply_session_result(session.add_selection_to_next_find_match());
        }
    }

    /// `addSelectionToPreviousFindMatch(findController)`.
    pub fn add_selection_to_previous_find_match(&self, find_controller: &Rc<dyn ICommonFindController>) {
        self.begin_session_if_needed(find_controller);
        if let Some(session) = self.get_session() {
            self.apply_session_result(session.add_selection_to_previous_find_match());
        }
    }

    /// `moveSelectionToNextFindMatch(findController)`.
    pub fn move_selection_to_next_find_match(&self, find_controller: &Rc<dyn ICommonFindController>) {
        self.begin_session_if_needed(find_controller);
        if let Some(session) = self.get_session() {
            self.apply_session_result(session.move_selection_to_next_find_match());
        }
    }

    /// `moveSelectionToPreviousFindMatch(findController)`.
    pub fn move_selection_to_previous_find_match(&self, find_controller: &Rc<dyn ICommonFindController>) {
        self.begin_session_if_needed(find_controller);
        if let Some(session) = self.get_session() {
            self.apply_session_result(session.move_selection_to_previous_find_match());
        }
    }

    /// `selectAll(findController)`.
    pub fn select_all(&self, find_controller: &Rc<dyn ICommonFindController>) {
        let editor = self.editor();
        let Some(model) = editor.get_model() else {
            return;
        };

        let find_state = find_controller.get_state();

        // Special case: find widget owns entirely what we search for if:
        // - focus is not in the editor (i.e. it is in the find widget)
        // - and the search widget is visible
        // - and the search string is non-empty
        // - and we're searching for a regex
        let allow_find_regex = editor.get_option(SELECTED_TEXT_MATCH_MODE) == "findOptions" || !editor.has_text_focus();
        let mut matches: Vec<FindMatch> = if allow_find_regex
            && find_state.is_revealed()
            && !find_state.search_string().is_empty()
            && find_state.is_regex()
        {
            let search_scope = match find_state.search_scope() {
                Some(ranges) => FindScope::Ranges(ranges),
                None => FindScope::Whole,
            };
            let word_separators = find_state.whole_word().then(|| editor.get_option(WORD_SEPARATORS).as_str());
            model.find_matches(
                find_state.search_string(),
                search_scope,
                find_state.is_regex(),
                find_state.match_case(),
                word_separators,
                false,
                MAX_SAFE_SMALL_INTEGER as usize,
            )
        } else {
            self.begin_session_if_needed(find_controller);
            let Some(session) = self.get_session() else {
                return;
            };
            session.select_all(find_state.search_scope())
        };

        if !matches.is_empty() {
            let Some(editor_selection) = editor.get_selection() else {
                return;
            };
            // Have the primary cursor remain the one where the action was invoked
            if let Some(index) = matches.iter().position(|found| found.range.intersect_ranges(&editor_selection).is_some()) {
                // bingo!
                matches.swap(0, index);
            }

            let selections: Vec<Selection> = matches
                .iter()
                .map(|m| Selection::new(m.range.start_line_number, m.range.start_column, m.range.end_line_number, m.range.end_column))
                .collect();
            self.set_selections(&selections);
        }
    }

    /// `selectAllUsingSelections(selections)`.
    pub fn select_all_using_selections(&self, selections: &[Selection]) {
        if !selections.is_empty() {
            self.set_selections(selections);
        }
    }
}

impl IDisposable for MultiCursorSelectionController {
    fn dispose(&self) {
        self.end_session();
        self.base.dispose();
    }
}

impl IEditorContribution for MultiCursorSelectionController {
    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

// #endregion

// #region MultiCursorSelectionControllerAction

/// O `_run(multiCursorController, findController)` de uma subclasse de `MultiCursorSelectionControllerAction`: um
/// método do controlador.
pub type ControllerRun = fn(&MultiCursorSelectionController, &Rc<dyn ICommonFindController>);

/// `abstract class MultiCursorSelectionControllerAction` e as suas subclasses (ver o topo do módulo).
pub struct MultiCursorSelectionControllerAction {
    action: EditorActionBase,
    find_controllers: Rc<dyn ICommonFindControllerSource>,
    run_in_controller: ControllerRun,
}

impl EditorAction for MultiCursorSelectionControllerAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let Some(multi_cursor_controller) = MultiCursorSelectionController::get(editor) else {
            return Promise::resolved(());
        };
        if let Some(view_model) = editor.get_view_model() {
            let previous_cursor_state = view_model.get_cursor_states();
            match self.find_controllers.get(editor) {
                Some(find_controller) => (self.run_in_controller)(&multi_cursor_controller, &find_controller),
                None => {
                    let new_find_controller = self.find_controllers.create_instance(editor);
                    (self.run_in_controller)(&multi_cursor_controller, &new_find_controller);
                    new_find_controller.dispose();
                }
            }

            let _announcement = announce_cursor_change(&previous_cursor_state, &view_model.get_cursor_states());
        }
        Promise::resolved(())
    }
}

/// `AddSelectionToNextFindMatchAction`.
pub fn add_selection_to_next_find_match_action(find_controllers: Rc<dyn ICommonFindControllerSource>) -> MultiCursorSelectionControllerAction {
    MultiCursorSelectionControllerAction {
        action: EditorActionBase::new(ActionOptions {
            command: CommandOptions {
                kb_opts: contrib_keybindings(
                    editor_focus(),
                    Keybindings { primary: Some(key(KeyMod::CTRL_CMD, KeyCode::KeyD)), ..Keybindings::default() },
                ),
                menu_opts: selection_menu(5.0, "miAddSelectionToNextFindMatch", "Add &&Next Occurrence"),
                ..CommandOptions::new("editor.action.addSelectionToNextFindMatch")
            },
            context_menu_opts: Vec::new(),
            label: localized_label("addSelectionToNextFindMatch", "Add Selection to Next Find Match"),
        }),
        find_controllers,
        run_in_controller: MultiCursorSelectionController::add_selection_to_next_find_match,
    }
}

/// `AddSelectionToPreviousFindMatchAction`.
pub fn add_selection_to_previous_find_match_action(find_controllers: Rc<dyn ICommonFindControllerSource>) -> MultiCursorSelectionControllerAction {
    MultiCursorSelectionControllerAction {
        action: EditorActionBase::new(action_options(
            CommandOptions {
                menu_opts: selection_menu(6.0, "miAddSelectionToPreviousFindMatch", "Add P&&revious Occurrence"),
                ..CommandOptions::new("editor.action.addSelectionToPreviousFindMatch")
            },
            localized_label("addSelectionToPreviousFindMatch", "Add Selection to Previous Find Match"),
        )),
        find_controllers,
        run_in_controller: MultiCursorSelectionController::add_selection_to_previous_find_match,
    }
}

/// `MoveSelectionToNextFindMatchAction`.
pub fn move_selection_to_next_find_match_action(find_controllers: Rc<dyn ICommonFindControllerSource>) -> MultiCursorSelectionControllerAction {
    MultiCursorSelectionControllerAction {
        action: EditorActionBase::new(action_options(
            CommandOptions {
                kb_opts: contrib_keybindings(
                    editor_focus(),
                    Keybindings {
                        primary: Some(key_chord(
                            key(KeyMod::CTRL_CMD, KeyCode::KeyK),
                            key(KeyMod::CTRL_CMD, KeyCode::KeyD),
                        )),
                        ..Keybindings::default()
                    },
                ),
                ..CommandOptions::new("editor.action.moveSelectionToNextFindMatch")
            },
            localized_label("moveSelectionToNextFindMatch", "Move Last Selection to Next Find Match"),
        )),
        find_controllers,
        run_in_controller: MultiCursorSelectionController::move_selection_to_next_find_match,
    }
}

/// `MoveSelectionToPreviousFindMatchAction`.
pub fn move_selection_to_previous_find_match_action(find_controllers: Rc<dyn ICommonFindControllerSource>) -> MultiCursorSelectionControllerAction {
    MultiCursorSelectionControllerAction {
        action: EditorActionBase::new(action_options(
            CommandOptions::new("editor.action.moveSelectionToPreviousFindMatch"),
            localized_label("moveSelectionToPreviousFindMatch", "Move Last Selection to Previous Find Match"),
        )),
        find_controllers,
        run_in_controller: MultiCursorSelectionController::move_selection_to_previous_find_match,
    }
}

/// `SelectHighlightsAction`.
pub fn select_highlights_action(find_controllers: Rc<dyn ICommonFindControllerSource>) -> MultiCursorSelectionControllerAction {
    MultiCursorSelectionControllerAction {
        action: EditorActionBase::new(ActionOptions {
            command: CommandOptions {
                kb_opts: contrib_keybindings(
                    editor_focus(),
                    Keybindings {
                        primary: Some(key(KeyMod::CTRL_CMD | KeyMod::SHIFT, KeyCode::KeyL)),
                        ..Keybindings::default()
                    },
                ),
                menu_opts: selection_menu(7.0, "miSelectHighlights", "Select All &&Occurrences"),
                ..CommandOptions::new("editor.action.selectHighlights")
            },
            context_menu_opts: Vec::new(),
            label: localized_label("selectAllOccurrencesOfFindMatch", "Select All Occurrences of Find Match"),
        }),
        find_controllers,
        run_in_controller: MultiCursorSelectionController::select_all,
    }
}

/// `CompatChangeAll`.
pub fn compat_change_all(find_controllers: Rc<dyn ICommonFindControllerSource>) -> MultiCursorSelectionControllerAction {
    MultiCursorSelectionControllerAction {
        action: EditorActionBase::new(ActionOptions {
            command: CommandOptions {
                precondition: ContextKeyExpr::and([Some(writable()), Some(editor_text_focus())]),
                kb_opts: contrib_keybindings(
                    editor_text_focus(),
                    Keybindings { primary: Some(key(KeyMod::CTRL_CMD, KeyCode::F2)), ..Keybindings::default() },
                ),
                ..CommandOptions::new("editor.action.changeAll")
            },
            context_menu_opts: vec![EditorActionContextMenuOptions {
                group: "1_modification".to_owned(),
                order: 1.2,
                when: None,
                menu_id: None,
            }],
            label: localized_label("changeAll.label", "Change All Occurrences"),
        }),
        find_controllers,
        run_in_controller: MultiCursorSelectionController::select_all,
    }
}

// #endregion

// #region SelectionHighlighter

/// `ModelDecorationOptions.register(...)` das duas decorações de `getSelectionHighlightDecorationOptions`
/// (`highlightDecorations.ts`, ver "Divergências").
struct SelectionHighlightDecorationOptions {
    with_overview: Rc<ModelDecorationOptions>,
    no_overview: Rc<ModelDecorationOptions>,
}

thread_local! {
    static SELECTION_HIGHLIGHT_OPTIONS: SelectionHighlightDecorationOptions = SelectionHighlightDecorationOptions {
        with_overview: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "selection-highlight-overview".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            class_name: Some("selectionHighlight".to_owned()),
            overview_ruler: Some(ModelDecorationOverviewRulerInit {
                color: Some(DecorationColor::Theme("editorOverviewRuler.selectionHighlightForeground".to_owned())),
                dark_color: None,
                position: Some(OverviewRulerLane::Center),
            }),
            minimap: Some(ModelDecorationMinimapInit {
                color: Some(DecorationColor::Theme("minimap.selectionOccurrenceHighlight".to_owned())),
                dark_color: None,
                position: MinimapPosition::Inline,
                section_header_style: None,
                section_header_text: None,
            }),
            ..Default::default()
        }),
        no_overview: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "selection-highlight".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            class_name: Some("selectionHighlight".to_owned()),
            ..Default::default()
        }),
    };
}

/// `getSelectionHighlightDecorationOptions(hasSemanticHighlights)`: mostra na régua de visão geral só se o modelo não tem
/// destaque semântico.
fn get_selection_highlight_decoration_options(has_semantic_highlights: bool) -> Rc<ModelDecorationOptions> {
    SELECTION_HIGHLIGHT_OPTIONS.with(|options| {
        if has_semantic_highlights { options.no_overview.clone() } else { options.with_overview.clone() }
    })
}

/// `SelectionHighlighterState`.
struct SelectionHighlighterState {
    model: Rc<TextModel>,
    search_text: JsString,
    match_case: bool,
    word_separators: Option<String>,
    model_version_id: i64,
    cached_find_matches: RefCell<Option<Rc<Vec<Range>>>>,
}

impl SelectionHighlighterState {
    fn new(
        model: Rc<TextModel>,
        search_text: JsString,
        match_case: bool,
        word_separators: Option<String>,
        prev_state: Option<&SelectionHighlighterState>,
    ) -> Self {
        let model_version_id = model.get_version_id();
        let cached_find_matches = match prev_state {
            Some(prev_state)
                if Rc::ptr_eq(&model, &prev_state.model)
                    && search_text == prev_state.search_text
                    && match_case == prev_state.match_case
                    && word_separators == prev_state.word_separators
                    && model_version_id == prev_state.model_version_id =>
            {
                prev_state.cached_find_matches.borrow().clone()
            }
            _ => None,
        };
        Self { model, search_text, match_case, word_separators, model_version_id, cached_find_matches: RefCell::new(cached_find_matches) }
    }

    /// `findMatches()`.
    fn find_matches(&self) -> Rc<Vec<Range>> {
        let cached = self.cached_find_matches.borrow().clone();
        if let Some(cached) = cached {
            return cached;
        }
        let mut find_matches: Vec<Range> = self
            .model
            .find_matches(
                self.search_text.clone(),
                FindScope::Whole,
                false,
                self.match_case,
                self.word_separators.as_deref(),
                false,
                LIMIT_FIND_COUNT,
            )
            .into_iter()
            .map(|m| m.range)
            .collect();
        find_matches.sort_by(|a, b| Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0));
        let find_matches = Rc::new(find_matches);
        *self.cached_find_matches.borrow_mut() = Some(find_matches.clone());
        find_matches
    }
}

/// `SelectionHighlighter`.
pub struct SelectionHighlighter {
    me: Weak<SelectionHighlighter>,
    base: Rc<Disposable>,
    editor: Weak<CodeEditorWidget>,
    document_highlight_providers: Rc<dyn IDocumentHighlightProviderRegistry>,
    find_controllers: Rc<dyn ICommonFindControllerSource>,
    is_enabled: Cell<bool>,
    is_enabled_multiline: Cell<bool>,
    max_length: Cell<f64>,
    decorations: EditorDecorationsCollection,
    update_soon: Rc<RunOnceScheduler>,
    state: RefCell<Option<Rc<SelectionHighlighterState>>>,
}

impl SelectionHighlighter {
    pub const ID: &'static str = "editor.contrib.selectionHighlighter";

    pub fn new(
        editor: &Rc<CodeEditorWidget>,
        document_highlight_providers: Rc<dyn IDocumentHighlightProviderRegistry>,
        find_controllers: Rc<dyn ICommonFindControllerSource>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let this = Rc::new_cyclic(|me: &Weak<SelectionHighlighter>| {
            let scheduled = me.clone();
            let update_soon = RunOnceScheduler::new(
                move || {
                    if let Some(highlighter) = scheduled.upgrade() {
                        highlighter.update();
                    }
                },
                300,
            );
            base.register(update_soon.clone());
            Self {
                me: me.clone(),
                base: base.clone(),
                editor: Rc::downgrade(editor),
                document_highlight_providers,
                find_controllers,
                is_enabled: Cell::new(*editor.get_option(SELECTION_HIGHLIGHT)),
                is_enabled_multiline: Cell::new(*editor.get_option(SELECTION_HIGHLIGHT_MULTILINE)),
                max_length: Cell::new(*editor.get_option(SELECTION_HIGHLIGHT_MAX_LENGTH)),
                decorations: editor.create_decorations_collection(None),
                update_soon,
                state: RefCell::new(None),
            }
        });

        this.base.register(listen_weak(&this.me, editor.on_did_change_configuration(), |highlighter, e| {
            let editor = highlighter.editor();
            highlighter.is_enabled.set(*editor.get_option(SELECTION_HIGHLIGHT));
            highlighter.is_enabled_multiline.set(*editor.get_option(SELECTION_HIGHLIGHT_MULTILINE));
            highlighter.max_length.set(*editor.get_option(SELECTION_HIGHLIGHT_MAX_LENGTH));
            if e.has_changed(SELECTED_TEXT_MATCH_MODE.id()) {
                highlighter.update_soon.schedule(Some(0));
            }
        }));
        this.base.register(listen_weak(&this.me, editor.on_did_change_cursor_selection(), |highlighter, e| {
            if !highlighter.is_enabled.get() {
                // Early exit if nothing needs to be done!
                // Leave some form of early exit check here if you wish to continue being a cursor position change listener ;)
                return;
            }

            if e.selection.is_empty() {
                if e.reason == CursorChangeReason::Explicit {
                    if highlighter.state.borrow().is_some() {
                        // no longer valid
                        highlighter.set_state(None);
                    }
                    highlighter.update_soon.schedule(None);
                } else {
                    highlighter.set_state(None);
                }
            } else {
                highlighter.update();
            }
        }));
        this.base.register(listen_weak(&this.me, editor.on_did_change_model(), |highlighter, _e| {
            highlighter.set_state(None);
        }));
        this.base.register(listen_weak(&this.me, editor.on_did_change_model_content(), |highlighter, _e| {
            if highlighter.is_enabled.get() {
                highlighter.update_soon.schedule(None);
            }
        }));
        if let Some(find_controller) = this.find_controllers.get(editor) {
            this.base.register(listen_weak(
                &this.me,
                find_controller.get_state().on_find_replace_state_change(),
                |highlighter, _e| highlighter.update(),
            ));
        }
        this.update_soon.schedule(None);
        this
    }

    /// O editor dono da contribuição (que a descarta antes de ser solto).
    fn editor(&self) -> Rc<CodeEditorWidget> {
        self.editor.upgrade().expect("o editor vive mais que as suas contribuições")
    }

    /// `_update()`.
    fn update(&self) {
        let old_state = self.state.borrow().clone();
        self.set_state(self.create_state(old_state.as_deref()));
    }

    /// `_createState(oldState, isEnabled, isEnabledMultiline, maxLength, editor)`.
    fn create_state(&self, old_state: Option<&SelectionHighlighterState>) -> Option<SelectionHighlighterState> {
        if !self.is_enabled.get() {
            return None;
        }
        let editor = self.editor();
        let model = editor.get_model()?;
        if !self.is_enabled_multiline.get() {
            let s = editor.get_selection()?;
            if s.start_line_number != s.end_line_number {
                // multiline forbidden for perf reasons
                return None;
            }
        }
        let multi_cursor_controller = MultiCursorSelectionController::get(&editor)?;
        let find_controller = self.find_controllers.get(&editor)?;
        let mut r = multi_cursor_controller.get_session();
        if r.is_none() {
            let all_selections = editor.get_selections()?;
            if all_selections.len() > 1 {
                let find_state = find_controller.get_state();
                let match_case = find_state.match_case();
                let selections_contain_same_text = model_ranges_contain_same_text(&model, &all_selections, match_case);
                if !selections_contain_same_text {
                    return None;
                }
            }

            r = MultiCursorSession::create(&editor, &find_controller).map(Rc::new);
        }
        let r = r?;

        if r.current_match().is_some() {
            // This is an empty selection
            // Do not interfere with semantic word highlighting in the no selection case
            return None;
        }
        if !r.search_text.is_empty() && r.search_text.units().iter().all(|&unit| unit == 0x20 || unit == 0x09) {
            // whitespace only selection
            return None;
        }
        let max_length = self.max_length.get();
        if max_length > 0.0 && r.search_text.len() as f64 > max_length {
            // very long selection
            return None;
        }

        // TODO: better handling of this case
        let find_state = find_controller.get_state();
        let case_sensitive = find_state.match_case();

        // Return early if the find widget shows the exact same matches
        if find_state.is_revealed() {
            let mut find_state_search_string = find_state.search_string();
            if !case_sensitive {
                find_state_search_string = find_state_search_string.to_lower_case();
            }

            let mut my_search_string = r.search_text.clone();
            if !case_sensitive {
                my_search_string = my_search_string.to_lower_case();
            }

            if find_state_search_string == my_search_string
                && r.match_case == find_state.match_case()
                && r.whole_word == find_state.whole_word()
                && !find_state.is_regex()
            {
                return None;
            }
        }

        Some(SelectionHighlighterState::new(
            model,
            r.search_text.clone(),
            r.match_case,
            r.whole_word.then(|| editor.get_option(WORD_SEPARATORS).clone()),
            old_state,
        ))
    }

    /// `_setState(newState)`.
    fn set_state(&self, new_state: Option<SelectionHighlighterState>) {
        let state = new_state.map(Rc::new);
        *self.state.borrow_mut() = state.clone();

        let Some(state) = state else {
            self.decorations.clear();
            return;
        };

        let editor = self.editor();
        let Some(model) = editor.get_model() else {
            return;
        };
        if model.is_too_large_for_tokenization() {
            // the file is too large, so searching word under cursor in the whole document would be blocking the UI.
            return;
        }

        let all_matches = state.find_matches();

        let Some(selections) = editor.get_selections() else {
            return;
        };
        let mut selections: Vec<Range> = selections.into_iter().map(|selection| *selection).collect();
        selections.sort_by(|a, b| Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0));

        // do not overlap with selection (issue #64 and #512)
        let mut matches: Vec<Range> = Vec::new();
        let (mut i, mut j) = (0, 0);
        while i < all_matches.len() {
            let match_range = all_matches[i];

            if j >= selections.len() {
                // finished all editor selections
                matches.push(match_range);
                i += 1;
            } else {
                let cmp = Range::compare_ranges_using_starts(Some(&match_range), Some(&selections[j]));
                if cmp < 0 {
                    // match is before sel
                    if selections[j].is_empty() || !Range::are_intersecting(&match_range, &selections[j]) {
                        matches.push(match_range);
                    }
                    i += 1;
                } else if cmp > 0 {
                    // sel is before match
                    j += 1;
                } else {
                    // sel is equal to match
                    i += 1;
                    j += 1;
                }
            }
        }

        let occurrence_highlighting = editor.get_option(OCCURRENCES_HIGHLIGHT) != "off";
        let has_semantic_highlights = self.document_highlight_providers.has(&model) && occurrence_highlighting;
        let decorations: Vec<ModelDeltaDecoration> = matches
            .into_iter()
            .map(|range| ModelDeltaDecoration { range, options: get_selection_highlight_decoration_options(has_semantic_highlights) })
            .collect();

        self.decorations.set(&decorations);
    }
}

impl IDisposable for SelectionHighlighter {
    fn dispose(&self) {
        self.set_state(None);
        self.base.dispose();
    }
}

impl IEditorContribution for SelectionHighlighter {
    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

/// `modelRangesContainSameText(model, ranges, matchCase)`.
fn model_ranges_contain_same_text(model: &TextModel, ranges: &[Selection], match_case: bool) -> bool {
    let selected_text = get_value_in_range(model, &ranges[0], !match_case);
    for range in &ranges[1..] {
        if range.is_empty() {
            return false;
        }
        let this_selected_text = get_value_in_range(model, range, !match_case);
        if selected_text != this_selected_text {
            return false;
        }
    }
    true
}

/// `getValueInRange(model, range, toLowerCase)`.
fn get_value_in_range(model: &TextModel, range: &Range, to_lower_case: bool) -> JsString {
    let text = model.get_value_in_range(range);
    if to_lower_case { text.to_lower_case() } else { text }
}

// #endregion

// #region FocusNextCursor e FocusPreviousCursor

/// Para onde [`FocusCursor`] anda o foco.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusDirection {
    Next,
    Previous,
}

/// `FocusNextCursor` e `FocusPreviousCursor`.
pub struct FocusCursor {
    action: EditorActionBase,
    direction: FocusDirection,
}

/// `FocusNextCursor`.
pub fn focus_next_cursor() -> FocusCursor {
    FocusCursor {
        action: EditorActionBase::new(action_options(
            CommandOptions {
                metadata: Some(CommandMetadata {
                    description: localize("mutlicursor.focusNextCursor.description", "Focuses the next cursor", &[]).as_str().into(),
                    args: Some(Vec::new()),
                    returns: None,
                }),
                ..CommandOptions::new("editor.action.focusNextCursor")
            },
            localized_label("mutlicursor.focusNextCursor", "Focus Next Cursor"),
        )),
        direction: FocusDirection::Next,
    }
}

/// `FocusPreviousCursor`.
pub fn focus_previous_cursor() -> FocusCursor {
    FocusCursor {
        action: EditorActionBase::new(action_options(
            CommandOptions {
                metadata: Some(CommandMetadata {
                    description: localize("mutlicursor.focusPreviousCursor.description", "Focuses the previous cursor", &[])
                        .as_str()
                        .into(),
                    args: Some(Vec::new()),
                    returns: None,
                }),
                ..CommandOptions::new("editor.action.focusPreviousCursor")
            },
            localized_label("mutlicursor.focusPreviousCursor", "Focus Previous Cursor"),
        )),
        direction: FocusDirection::Previous,
    }
}

impl EditorAction for FocusCursor {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        let Some(view_model) = editor.get_view_model() else {
            return Promise::resolved(());
        };

        if view_model.cursor_config().read_only {
            return Promise::resolved(());
        }

        let source = source_arg(args);
        view_model.model().push_stack_element();
        let mut previous_cursor_state = view_model.get_cursor_states();
        if previous_cursor_state.is_empty() {
            return Promise::resolved(());
        }
        match self.direction {
            FocusDirection::Next => previous_cursor_state.rotate_left(1),
            FocusDirection::Previous => previous_cursor_state.rotate_right(1),
        }

        let states = previous_cursor_state.iter().map(|state| PartialCursorState::Full(*state)).collect();
        view_model.set_cursor_states(source.as_deref(), CursorChangeReason::Explicit, Some(states));
        view_model.reveal_primary_cursor(source.as_deref(), true, None);
        let _announcement = announce_cursor_change(&previous_cursor_state, &view_model.get_cursor_states());
        Promise::resolved(())
    }
}

// #endregion

thread_local! {
    /// O carregamento do módulo acontece uma vez; o registro de comandos é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// Os `registerEditorContribution` e `registerEditorAction` do fim do arquivo (o que o carregamento do módulo faz no
/// original). É idempotente: o registro acontece uma vez por thread, com os serviços da primeira chamada.
pub fn register_multi_cursor(
    find_controllers: Rc<dyn ICommonFindControllerSource>,
    document_highlight_providers: Rc<dyn IDocumentHighlightProviderRegistry>,
) {
    if REGISTERED.replace(true) {
        return;
    }
    let multi_cursor_ctor: EditorContributionCtor =
        Rc::new(|editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> { MultiCursorSelectionController::new(editor) });
    register_editor_contribution(MultiCursorSelectionController::ID, multi_cursor_ctor, EditorContributionInstantiation::Lazy);
    let highlighter_find_controllers = find_controllers.clone();
    let selection_highlighter_ctor: EditorContributionCtor = Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> {
        SelectionHighlighter::new(editor, document_highlight_providers.clone(), highlighter_find_controllers.clone())
    });
    register_editor_contribution(
        SelectionHighlighter::ID,
        selection_highlighter_ctor,
        EditorContributionInstantiation::AfterFirstRender,
    );

    register_editor_action(insert_cursor_above);
    register_editor_action(insert_cursor_below);
    register_editor_action(SelectionCursorsAction::end_of_each_line_selected);
    register_editor_action(|| add_selection_to_next_find_match_action(find_controllers.clone()));
    register_editor_action(|| add_selection_to_previous_find_match_action(find_controllers.clone()));
    register_editor_action(|| move_selection_to_next_find_match_action(find_controllers.clone()));
    register_editor_action(|| move_selection_to_previous_find_match_action(find_controllers.clone()));
    register_editor_action(|| select_highlights_action(find_controllers.clone()));
    register_editor_action(|| compat_change_all(find_controllers.clone()));
    register_editor_action(SelectionCursorsAction::to_bottom);
    register_editor_action(SelectionCursorsAction::to_top);
    register_editor_action(focus_next_cursor);
    register_editor_action(focus_previous_cursor);
}
