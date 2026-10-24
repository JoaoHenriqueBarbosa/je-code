//! Porte de `src/vs/editor/contrib/smartSelect/browser/smartSelect.ts`: o controlador que expande e encolhe a seleção
//! (`SmartSelectController`), as ações `editor.action.smartSelect.expand` e `editor.action.smartSelect.shrink`, a função
//! [`provide_selection_ranges`] que junta os provedores de intervalos de seleção, e as chamadas de registro do arquivo.
//!
//! Os provedores embutidos moram nos submódulos [`bracket_selections`] (`bracketSelections.ts`) e [`word_selections`]
//! (`wordSelections.ts`).
//!
//! Representação:
//! - `SelectionRanges` (privada no original) é [`SelectionRanges`]; os intervalos ficam num `Rc<Vec<Range>>`, o
//!   `readonly ranges` que `mov` repassa ao novo valor sem copiar.
//! - `SmartSelectController implements IEditorContribution` é a struct [`SmartSelectController`]. O `ILanguageFeaturesService`
//!   do construtor é o `LanguageFeatureRegistry<dyn SelectionRangeProvider>` de `selectionRangeProvider` (o único recurso
//!   que o controlador lê), entregue pelo construtor e por [`register_smart_select`], como o `multicursor` faz com os
//!   provedores de destaque. O estado mutável fica em `RefCell` e `Cell`, e o ouvinte de cursor guarda um [`Weak`] do
//!   controlador.
//! - `async run(forward)` é [`SmartSelectController::run`], que devolve a `Promise<void>`: o `await` do original é o
//!   `then` que continua depois dos intervalos e só existe quando ainda não há estado (sem estado o corpo roda inteiro,
//!   de forma síncrona, como numa função `async` sem `await` executado).
//! - `AbstractSmartSelect`, `GrowSelectionAction` e `ShrinkSelectionAction` são uma struct só, [`SmartSelectAction`], que
//!   guarda o `_forward`; as duas ações diferem em dados e saem de [`expand_selection_action`] e [`shrink_selection_action`].
//! - `SelectionRangesOptions` tem o mesmo formato de `SmartSelectOptions` (a opção `smartSelect` do editor) e é o
//!   mesmo tipo.
//!
//! Divergências:
//! - O comando `_executeSelectionRangeProvider` (que lê o recurso e as posições dos argumentos, abre o modelo pelo
//!   `ITextModelService` e devolve os intervalos numa promessa) não é registrado: o retorno do `CommandHandler` é um
//!   `Dynamic`, que não carrega a promessa nem os intervalos. O corpo do comando é [`provide_selection_ranges`] chamada
//!   com `selectLeadingAndTrailingWhitespace` e `selectSubwords` verdadeiros.
//! - O `Range.isIRange` que filtra a saída de cada provedor é sempre verdadeiro (o provedor devolve [`Range`]) e o
//!   `Range.lift` é a identidade.
//! - O `onUnexpectedExternalError` do `then` de cada provedor recebe a rejeição da promessa dele.

pub mod bracket_selections;
pub mod word_selections;

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::rc::{Rc, Weak};

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::on_unexpected_external_error;
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::action::localize2;
use je_platform::actions::menu_ids;
use je_platform::commands::commands_registry;
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};

use crate::code_editor_widget::{
    CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution,
};
use crate::config::editor_options::smart_select::{SMART_SELECT, SmartSelectOptions};
use crate::core_commands::{editor_text_focus, key, platform_keybindings};
use crate::cursor_common::CursorSimpleModel;
use crate::editor_extensions::{
    ActionLabel, ActionOptions, CommandKeybindingsOptions, CommandMenuOptions, CommandOptions, EditorAction,
    EditorActionBase, ServicesAccessor, register_editor_action, register_editor_contribution,
};
use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_providers::SelectionRangeProvider;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

use self::bracket_selections::BracketSelectionRangeProvider;
use self::word_selections::WordSelectionRangeProvider;

/// `SelectionRangesOptions`: o mesmo formato de [`SmartSelectOptions`].
pub type SelectionRangesOptions = SmartSelectOptions;

/// `SelectionRanges`: os intervalos de uma seleção e em qual deles ela está.
#[derive(Clone)]
struct SelectionRanges {
    index: usize,
    ranges: Rc<Vec<Range>>,
}

impl SelectionRanges {
    fn mov(&self, fwd: bool) -> SelectionRanges {
        let index = self.index as i64 + if fwd { 1 } else { -1 };
        if index < 0 || index >= self.ranges.len() as i64 {
            return self.clone();
        }
        let res = SelectionRanges { index: index as usize, ranges: Rc::clone(&self.ranges) };
        if res.ranges[res.index] == self.ranges[self.index] {
            // next range equals this range, retry with next-next
            return res.mov(fwd);
        }
        res
    }
}

thread_local! {
    /// O carregamento do módulo acontece uma vez; o registro de ações e contribuição é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// `SmartSelectController`.
pub struct SmartSelectController {
    editor: Weak<CodeEditorWidget>,
    language_features: Rc<LanguageFeatureRegistry<dyn SelectionRangeProvider>>,
    me: Weak<SmartSelectController>,
    state: RefCell<Option<Vec<SelectionRanges>>>,
    selection_listener: RefCell<Option<DisposableRef>>,
    ignore_selection: Cell<bool>,
}

impl SmartSelectController {
    pub const ID: &'static str = "editor.contrib.smartSelectController";

    /// `SmartSelectController.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<SmartSelectController>> {
        editor.get_contribution_as::<SmartSelectController>(Self::ID)
    }

    pub fn new(
        editor: &Rc<CodeEditorWidget>,
        language_features: Rc<LanguageFeatureRegistry<dyn SelectionRangeProvider>>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            editor: Rc::downgrade(editor),
            language_features,
            me: me.clone(),
            state: RefCell::new(None),
            selection_listener: RefCell::new(None),
            ignore_selection: Cell::new(false),
        })
    }

    /// `this._selectionListener?.dispose()`.
    fn dispose_selection_listener(&self) {
        let listener = self.selection_listener.borrow_mut().take();
        if let Some(listener) = listener {
            listener.dispose();
        }
    }

    /// `async run(forward)`.
    pub fn run(self: &Rc<Self>, forward: bool) -> Promise<()> {
        let Some(editor) = self.editor.upgrade().filter(|editor| editor.has_model()) else {
            return Promise::resolved(());
        };
        let (Some(selections), Some(model)) = (editor.get_selections(), editor.get_model()) else {
            return Promise::resolved(());
        };

        if self.state.borrow().is_some() {
            self.step(forward);
            return Promise::resolved(());
        }

        let positions: Vec<Position> = selections.iter().map(|selection| selection.get_position()).collect();
        let options = *editor.get_option(SMART_SELECT);
        let this = Rc::clone(self);
        provide_selection_ranges(&self.language_features, &model, &positions, options, &CancellationToken::None).then(
            move |ranges| {
                this.adopt_ranges(ranges, &selections);
                this.step(forward);
            },
        )
    }

    /// O `then(ranges => { ... })` de `run`: guarda os intervalos como estado e passa a ouvir o cursor.
    fn adopt_ranges(&self, ranges: Vec<Vec<Range>>, selections: &[Selection]) {
        if ranges.is_empty() || ranges.len() != selections.len() {
            // invalid result
            return;
        }
        let Some(editor) = self.editor.upgrade().filter(|editor| editor.has_model()) else {
            // invalid editor state
            return;
        };
        if editor.get_selections().as_deref() != Some(selections) {
            // invalid editor state
            return;
        }

        let state: Vec<SelectionRanges> = ranges
            .into_iter()
            .zip(selections)
            .map(|(ranges, selection)| {
                // filter ranges inside the selection
                let mut ranges: Vec<Range> = ranges
                    .into_iter()
                    .filter(|range| {
                        range.contains_position(&selection.get_start_position())
                            && range.contains_position(&selection.get_end_position())
                    })
                    .collect();
                // prepend current selection
                ranges.insert(0, **selection);
                SelectionRanges { index: 0, ranges: Rc::new(ranges) }
            })
            .collect();
        *self.state.borrow_mut() = Some(state);

        // listen to caret move and forget about state
        self.dispose_selection_listener();
        let me = self.me.clone();
        let listener = editor.on_did_change_cursor_position().listen(move |_| {
            if let Some(this) = me.upgrade()
                && !this.ignore_selection.get()
            {
                this.dispose_selection_listener();
                *this.state.borrow_mut() = None;
            }
        });
        *self.selection_listener.borrow_mut() = Some(listener);
    }

    /// O trecho de `run` depois do `if (!this._state)`: anda um intervalo e aplica as seleções.
    fn step(&self, forward: bool) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let new_selections: Vec<Selection> = {
            let mut state = self.state.borrow_mut();
            let Some(current) = state.as_ref() else {
                // no state
                return;
            };
            let moved: Vec<SelectionRanges> = current.iter().map(|state| state.mov(forward)).collect();
            let selections = moved
                .iter()
                .map(|state| {
                    let range = state.ranges[state.index];
                    Selection::from_positions(range.get_start_position(), range.get_end_position())
                })
                .collect();
            *state = Some(moved);
            selections
        };
        self.ignore_selection.set(true);
        editor.set_selections(&new_selections, None, None);
        self.ignore_selection.set(false);
    }
}

impl IDisposable for SmartSelectController {
    fn dispose(&self) {
        self.dispose_selection_listener();
    }
}

impl IEditorContribution for SmartSelectController {}

/// `AbstractSmartSelect` com `GrowSelectionAction` e `ShrinkSelectionAction`.
pub struct SmartSelectAction {
    action: EditorActionBase,
    forward: bool,
}

impl SmartSelectAction {
    fn new(forward: bool, id: &str, label: ActionLabel, keybindings: Keybindings, menu_opts: CommandMenuOptions) -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    kb_opts: vec![CommandKeybindingsOptions::new(
                        KeybindingWeight::EDITOR_CONTRIB,
                        Some(editor_text_focus()),
                        keybindings,
                    )],
                    menu_opts: vec![menu_opts],
                    ..CommandOptions::new(id)
                },
                context_menu_opts: Vec::new(),
                label,
            }),
            forward,
        }
    }
}

/// `GrowSelectionAction`.
pub fn expand_selection_action() -> SmartSelectAction {
    SmartSelectAction::new(
        true,
        "editor.action.smartSelect.expand",
        ActionLabel::Localized { label: localize2("smartSelect.expand", "Expand Selection"), alias: None },
        Keybindings {
            primary: Some(key(KeyMod::SHIFT | KeyMod::ALT, KeyCode::RightArrow)),
            mac: platform_keybindings(
                key(KeyMod::CTRL_CMD | KeyMod::WIN_CTRL | KeyMod::SHIFT, KeyCode::RightArrow),
                Some(vec![key(KeyMod::WIN_CTRL | KeyMod::SHIFT, KeyCode::RightArrow)]),
            ),
            ..Keybindings::default()
        },
        CommandMenuOptions::new(
            menu_ids::menubar_selection_menu(),
            "1_basic",
            2.0,
            localize("miSmartSelectGrow", "&&Expand Selection", &[]),
        ),
    )
}

/// `ShrinkSelectionAction`.
pub fn shrink_selection_action() -> SmartSelectAction {
    SmartSelectAction::new(
        false,
        "editor.action.smartSelect.shrink",
        ActionLabel::Localized { label: localize2("smartSelect.shrink", "Shrink Selection"), alias: None },
        Keybindings {
            primary: Some(key(KeyMod::SHIFT | KeyMod::ALT, KeyCode::LeftArrow)),
            mac: platform_keybindings(
                key(KeyMod::CTRL_CMD | KeyMod::WIN_CTRL | KeyMod::SHIFT, KeyCode::LeftArrow),
                Some(vec![key(KeyMod::WIN_CTRL | KeyMod::SHIFT, KeyCode::LeftArrow)]),
            ),
            ..Keybindings::default()
        },
        CommandMenuOptions::new(
            menu_ids::menubar_selection_menu(),
            "1_basic",
            3.0,
            localize("miSmartSelectShrink", "&&Shrink Selection", &[]),
        ),
    )
}

impl EditorAction for SmartSelectAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        match SmartSelectController::get(editor) {
            Some(controller) => controller.run(self.forward),
            None => Promise::resolved(()),
        }
    }
}

/// As chamadas `registerCommandAlias`, `registerEditorContribution` e `registerEditorAction` do arquivo (o que o
/// carregamento do módulo faz no original). É idempotente: o registro acontece uma vez por thread. O registro de
/// `selectionRangeProvider` é o `ILanguageFeaturesService` que o construtor do controlador recebe (ver a nota do módulo).
pub fn register_smart_select(language_features: Rc<LanguageFeatureRegistry<dyn SelectionRangeProvider>>) {
    if REGISTERED.replace(true) {
        return;
    }
    // renamed command id
    commands_registry().register_command_alias("editor.action.smartSelect.grow", "editor.action.smartSelect.expand");

    register_editor_contribution(
        SmartSelectController::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> {
            SmartSelectController::new(editor, Rc::clone(&language_features))
        }),
        EditorContributionInstantiation::Lazy,
    );
    register_editor_action(expand_selection_action);
    register_editor_action(shrink_selection_action);
}

/// `provideSelectionRanges(registry, model, positions, options, token)`: para cada posição, os intervalos que a contêm,
/// do menor para o maior, vindos de todos os provedores do registro mais o de palavras (que sempre participa) e, sem
/// nenhum outro provedor, o de colchetes.
pub fn provide_selection_ranges(
    registry: &LanguageFeatureRegistry<dyn SelectionRangeProvider>,
    model: &Rc<TextModel>,
    positions: &[Position],
    options: SelectionRangesOptions,
    token: &CancellationToken,
) -> Promise<Vec<Vec<Range>>> {
    let mut providers = registry.all(model);
    // ALWAYS have word based selection range
    providers.push(Rc::new(WordSelectionRangeProvider::new(options.select_subwords)));

    if providers.len() == 1 {
        // add word selection and bracket selection when no provider exists
        providers.insert(0, Rc::new(BracketSelectionRangeProvider));
    }

    let all_raw_ranges: Rc<RefCell<Vec<Vec<Range>>>> = Rc::default();
    let mut work: Vec<Promise<()>> = Vec::new();

    for provider in &providers {
        let all_raw_ranges = Rc::clone(&all_raw_ranges);
        let positions_of_work = positions.to_vec();
        work.push(provider.provide_selection_ranges(model, positions, token).then_result(move |result| {
            match result {
                Ok(Some(all_provider_ranges))
                    if !all_provider_ranges.is_empty() && all_provider_ranges.len() == positions_of_work.len() =>
                {
                    let mut all_raw_ranges = all_raw_ranges.borrow_mut();
                    if all_raw_ranges.len() < positions_of_work.len() {
                        all_raw_ranges.resize_with(positions_of_work.len(), Vec::new);
                    }
                    for (i, one_provider_ranges) in all_provider_ranges.iter().enumerate() {
                        for selection_range in one_provider_ranges {
                            if selection_range.range.contains_position(&positions_of_work[i]) {
                                all_raw_ranges[i].push(selection_range.range);
                            }
                        }
                    }
                }
                Err(error) => on_unexpected_external_error(&error),
                Ok(_) => {}
            }
            Ok(())
        }));
    }

    let model = Rc::clone(model);
    Promise::all(work).then(move |_| {
        let all_raw_ranges = all_raw_ranges.take();
        all_raw_ranges.into_iter().map(|one_raw_ranges| expand_with_trivia(&model, one_raw_ranges, options)).collect()
    })
}

/// O `map` final de `provideSelectionRanges` para uma posição: ordena, tira os intervalos que não contêm o anterior ou
/// que são iguais a ele e, com `selectLeadingAndTrailingWhitespace`, acrescenta as linhas e os blocos sem e com espaço
/// nas pontas quando um intervalo passa a outra linha.
fn expand_with_trivia(model: &TextModel, mut one_raw_ranges: Vec<Range>, options: SelectionRangesOptions) -> Vec<Range> {
    if one_raw_ranges.is_empty() {
        return Vec::new();
    }

    // sort all by start/end position
    one_raw_ranges.sort_by(|a, b| {
        if a.get_start_position().is_before(&b.get_start_position()) {
            Ordering::Greater
        } else if b.get_start_position().is_before(&a.get_start_position()) {
            Ordering::Less
        } else if a.get_end_position().is_before(&b.get_end_position()) {
            Ordering::Less
        } else if b.get_end_position().is_before(&a.get_end_position()) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });

    // remove ranges that don't contain the former range or that are equal to the
    // former range
    let mut one_ranges: Vec<Range> = Vec::new();
    let mut last: Option<Range> = None;
    for range in one_raw_ranges {
        if last.is_none_or(|last| range.contains_range(&last) && range != last) {
            one_ranges.push(range);
            last = Some(range);
        }
    }

    if !options.select_leading_and_trailing_whitespace {
        return one_ranges;
    }

    // add ranges that expand trivia at line starts and ends whenever a range
    // wraps onto the a new line
    let mut one_ranges_with_trivia: Vec<Range> = vec![one_ranges[0]];
    for i in 1..one_ranges.len() {
        let prev = one_ranges[i - 1];
        let cur = one_ranges[i];
        if cur.start_line_number != prev.start_line_number || cur.end_line_number != prev.end_line_number {
            // add line/block range without leading/failing whitespace
            let range_no_whitespace = Range::new(
                prev.start_line_number,
                model.get_line_first_non_whitespace_column(prev.start_line_number),
                prev.end_line_number,
                model.get_line_last_non_whitespace_column(prev.end_line_number),
            );
            if range_no_whitespace.contains_range(&prev)
                && range_no_whitespace != prev
                && cur.contains_range(&range_no_whitespace)
                && cur != range_no_whitespace
            {
                one_ranges_with_trivia.push(range_no_whitespace);
            }
            // add line/block range
            let range_full =
                Range::new(prev.start_line_number, 1, prev.end_line_number, model.get_line_max_column(prev.end_line_number));
            if range_full.contains_range(&prev)
                && range_full != range_no_whitespace
                && cur.contains_range(&range_full)
                && cur != range_full
            {
                one_ranges_with_trivia.push(range_full);
            }
        }
        one_ranges_with_trivia.push(cur);
    }
    one_ranges_with_trivia
}
