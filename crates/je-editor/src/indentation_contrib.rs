//! Porte de `src/vs/editor/contrib/indentation/browser/indentation.ts` (as ações de indentação, o comando e a
//! contribuição `AutoIndentOnPaste`) e de `src/vs/editor/contrib/indentation/common/indentation.ts`
//! (`getReindentEditOperations`). O `indentation.rs` do crate é o de `common/core/misc/indentation.ts`
//! (`normalizeIndentation`), não este.
//!
//! Representação:
//! - `IndentationToSpacesCommand` e `IndentationToTabsCommand` só diferem no sentido da conversão (o
//!   `getIndentationEditOperations` recebe um `bool`): são o [`IndentationCommand`] com o [`IndentationConversion`]
//!   como parâmetro. Do mesmo modo, `IndentationToSpacesAction` e `IndentationToTabsAction` são a
//!   [`IndentationConversionAction`], criada por [`IndentationConversionAction::to_spaces`] e
//!   [`IndentationConversionAction::to_tabs`].
//! - `IndentUsingTabs`, `IndentUsingSpaces` e `ChangeTabDisplaySize` são subclasses de
//!   `ChangeIndentationSizeAction` que só passam argumentos ao construtor: viram as funções
//!   [`indent_using_tabs`], [`indent_using_spaces`] e [`change_tab_display_size`], que devolvem a própria
//!   [`ChangeIndentationSizeAction`]. Os `static readonly ID` são as constantes `*_ID`.
//! - `EditorAction` é o trait de `editor_extensions`; a precondição `EditorContextKeys.writable` é `writable()`.
//! - `AutoIndentOnPaste` guarda o editor como `Weak`: o editor é dono da contribuição, e a referência forte
//!   formaria um ciclo (o coletor de lixo do original o resolve). O `@ILanguageConfigurationService` do
//!   construtor é o parâmetro `language_configuration_service`.
//! - `IVirtualModel` de `trigger` (o `virtualModel` com a primeira linha trocada) é o [`PastedVirtualModel`]; sem
//!   `forceTokenization`, como no original. O `IIndentConverter` é o [`MoveLinesIndentConverter`] de
//!   `move_lines_command` (o mesmo `shiftIndent`/`unshiftIndent` com `indentation.length + 1`).
//! - `AutoIndentOnPasteCommand` recebe `TextEdit[]` (`languages.TextEdit`, ainda não portado em `languages.rs`):
//!   é o [`TextEdit`] local. O filtro `edit.range && typeof edit.text === 'string'` do construtor é garantido
//!   pelos tipos.
//!
//! Divergências:
//! - **Serviços.** `IQuickInputService` ainda não existe no porte (só os tipos de `quickInput.ts` e os
//!   utilitários): o recorte é o [`IQuickInputService`] daqui (só o `pick`), com o id [`I_QUICK_INPUT_SERVICE`], e as
//!   opções do `pick` são o [`QuickPickOptions`] (`placeHolder` e `activeItem`). O item de lista é o `IQuickItem`
//!   de `je_platform` (`id`, `label` e `description`, como o `IQuickPickItem`). `IModelService.getCreationOptions`
//!   foi acrescentado ao recorte de `services_model`. `ILanguageConfigurationService` não tem identificador no
//!   porte: as ações de reindentação o leem do editor ([`CodeEditorWidget::language_configuration_service`]), que
//!   é o serviço que o `accessor` entregaria.
//! - **`setTimeout` de 50 ms** do `ChangeIndentationSizeAction` é o `set_timeout` de `je_base::runtime`.
//! - O `accessor` de uma ação de editor é opcional na assinatura do porte e sempre presente quando o registro a
//!   chama: as ações que o usam o exigem.
//! - `parseInt(pick.label, 10)` de um rótulo que não é número (nunca acontece: os rótulos são de 1 a 8) ignora a
//!   escolha em vez de gravar `NaN`.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::js_string::{substring, trim};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::runtime;
use je_platform::action::localize2;
use je_platform::commands::{CommandDescription, CommandMetadata};
use je_platform::contextkey::ContextKeyExpression;
use je_platform::quickinput::quick_input::IQuickItem;

use crate::auto_indent::{
    IIndentConverter, IVirtualModel, get_good_indent_for_line, get_indent_metadata, leading_whitespace,
};
use crate::code_editor_widget::{CodeEditorWidget, ExecuteEditsSource, ICodeEditor, IEditorContribution};
use crate::config::editor_options::EditorAutoIndentStrategy;
use crate::config::editor_options::registry::{AUTO_INDENT, AUTO_INDENT_ON_PASTE, AUTO_INDENT_ON_PASTE_WITHIN_STRING};
use crate::core_commands::writable;
use crate::cursor_common::CursorSimpleModel;
use crate::edit_operation::{EditOperation, SingleEditOperation};
use crate::editor_common::{Command as EditCommand, CursorStateComputerData, EditOperationBuilder};
use crate::editor_extensions::{
    ActionLabel, ActionOptions, CommandOptions, EditorAction, EditorActionBase, EditorContributionInstantiation,
    ServicesAccessor, register_editor_action, register_editor_contribution,
};
use crate::encoded_token_attributes::standard_token_type;
use crate::indent_rules::indent_consts;
use crate::indent_utils::{generate_indent, get_space_cnt};
use crate::indentation::normalize_indentation;
use crate::indentation_line_processor::ProcessedIndentRulesSupport;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::line_tokens::IViewLineTokens;
use crate::model::{EndOfLineSequence, IndentSize, TextModelResolvedOptions, TextModelUpdateOptions};
use crate::move_lines_command::MoveLinesIndentConverter;
use crate::range::Range;
use crate::selection::Selection;
use crate::services_model::I_MODEL_SERVICE;
use crate::text_model::TextModel;

const TAB: u16 = 9;
const SPACE: u16 = 32;

// #region Serviços (recortes)

/// `IPickOptions`: só o que `ChangeIndentationSizeAction` usa.
pub struct QuickPickOptions {
    /// `placeHolder`.
    pub place_holder: String,
    /// `activeItem`: o item que já nasce em foco.
    pub active_item: Option<IQuickItem>,
}

/// `IQuickInputService`, só com o `pick` (ver "Divergências" no topo do módulo).
pub trait IQuickInputService {
    /// `pick(picks, options)`: resolve com o item escolhido, ou `None` se a escolha foi cancelada.
    fn pick(&self, picks: Vec<IQuickItem>, options: QuickPickOptions) -> Promise<Option<IQuickItem>>;
}

je_instantiation::service_id!(pub I_QUICK_INPUT_SERVICE: dyn IQuickInputService = "quickInputService");

// #endregion

// #region Ações

/// O `id` de `IndentationToSpacesAction`.
pub const INDENTATION_TO_SPACES_ID: &str = "editor.action.indentationToSpaces";
/// O `id` de `IndentationToTabsAction`.
pub const INDENTATION_TO_TABS_ID: &str = "editor.action.indentationToTabs";
/// O `id` de `IndentUsingTabs`.
pub const INDENT_USING_TABS_ID: &str = "editor.action.indentUsingTabs";
/// O `id` de `IndentUsingSpaces`.
pub const INDENT_USING_SPACES_ID: &str = "editor.action.indentUsingSpaces";
/// O `id` de `ChangeTabDisplaySize`.
pub const CHANGE_TAB_DISPLAY_SIZE_ID: &str = "editor.action.changeTabDisplaySize";
/// O `id` de `DetectIndentation`.
pub const DETECT_INDENTATION_ID: &str = "editor.action.detectIndentation";
/// O `id` de `ReindentLinesAction`.
pub const REINDENT_LINES_ID: &str = "editor.action.reindentlines";
/// O `id` de `ReindentSelectedLinesAction`.
pub const REINDENT_SELECTED_LINES_ID: &str = "editor.action.reindentselectedlines";

/// As opções de uma ação: o `id`, o rótulo e a descrição localizados, a precondição e `canTriggerInlineEdits`.
fn action_options(
    id: &str,
    label: (&str, &str),
    description: (&str, &str),
    precondition: Option<ContextKeyExpression>,
    can_trigger_inline_edits: bool,
) -> ActionOptions {
    ActionOptions {
        command: CommandOptions {
            precondition,
            metadata: Some(CommandMetadata::new(CommandDescription::Localized(localize2(description.0, description.1)))),
            can_trigger_inline_edits,
            ..CommandOptions::new(id)
        },
        context_menu_opts: Vec::new(),
        label: ActionLabel::Localized { label: localize2(label.0, label.1), alias: None },
    }
}

/// O sentido da conversão de indentação.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndentationConversion {
    /// `IndentationToSpacesCommand`: tabulações viram espaços.
    ToSpaces,
    /// `IndentationToTabsCommand`: espaços viram tabulações.
    ToTabs,
}

/// `IndentationToSpacesAction` e `IndentationToTabsAction`.
pub struct IndentationConversionAction {
    action: EditorActionBase,
    conversion: IndentationConversion,
}

impl IndentationConversionAction {
    /// `IndentationToSpacesAction`.
    pub fn to_spaces() -> Self {
        Self {
            action: EditorActionBase::new(action_options(
                INDENTATION_TO_SPACES_ID,
                ("indentationToSpaces", "Convert Indentation to Spaces"),
                ("indentationToSpacesDescription", "Convert the tab indentation to spaces."),
                Some(writable()),
                false,
            )),
            conversion: IndentationConversion::ToSpaces,
        }
    }

    /// `IndentationToTabsAction`.
    pub fn to_tabs() -> Self {
        Self {
            action: EditorActionBase::new(action_options(
                INDENTATION_TO_TABS_ID,
                ("indentationToTabs", "Convert Indentation to Tabs"),
                ("indentationToTabsDescription", "Convert the spaces indentation to tabs."),
                Some(writable()),
                false,
            )),
            conversion: IndentationConversion::ToTabs,
        }
    }
}

impl EditorAction for IndentationConversionAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let Some(model) = editor.get_model() else {
            return Promise::resolved(());
        };
        let model_opts = model.get_options();
        let Some(selection) = editor.get_selection() else {
            return Promise::resolved(());
        };
        let command = IndentationCommand::new(selection, model_opts.tab_size, self.conversion);

        editor.push_undo_stop();
        editor.execute_commands(Some(self.action.command.id.as_str()), vec![Some(Box::new(command) as Box<dyn EditCommand>)]);
        editor.push_undo_stop();

        model.update_options(TextModelUpdateOptions {
            insert_spaces: Some(self.conversion == IndentationConversion::ToSpaces),
            ..TextModelUpdateOptions::default()
        });
        Promise::resolved(())
    }
}

/// `ChangeIndentationSizeAction`: pergunta o tamanho da tabulação e o aplica ao modelo.
pub struct ChangeIndentationSizeAction {
    action: EditorActionBase,
    insert_spaces: bool,
    display_size_only: bool,
}

impl ChangeIndentationSizeAction {
    pub fn new(insert_spaces: bool, display_size_only: bool, opts: ActionOptions) -> Self {
        Self { action: EditorActionBase::new(opts), insert_spaces, display_size_only }
    }
}

/// `IndentUsingTabs`.
pub fn indent_using_tabs() -> ChangeIndentationSizeAction {
    ChangeIndentationSizeAction::new(
        false,
        false,
        action_options(
            INDENT_USING_TABS_ID,
            ("indentUsingTabs", "Indent Using Tabs"),
            ("indentUsingTabsDescription", "Use indentation with tabs."),
            None,
            false,
        ),
    )
}

/// `IndentUsingSpaces`.
pub fn indent_using_spaces() -> ChangeIndentationSizeAction {
    ChangeIndentationSizeAction::new(
        true,
        false,
        action_options(
            INDENT_USING_SPACES_ID,
            ("indentUsingSpaces", "Indent Using Spaces"),
            ("indentUsingSpacesDescription", "Use indentation with spaces."),
            None,
            false,
        ),
    )
}

/// `ChangeTabDisplaySize`.
pub fn change_tab_display_size() -> ChangeIndentationSizeAction {
    ChangeIndentationSizeAction::new(
        true,
        true,
        action_options(
            CHANGE_TAB_DISPLAY_SIZE_ID,
            ("changeTabDisplaySize", "Change Tab Display Size"),
            ("changeTabDisplaySizeDescription", "Change the space size equivalent of the tab."),
            None,
            false,
        ),
    )
}

impl EditorAction for ChangeIndentationSizeAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let accessor = accessor.expect("uma ação de editor roda com o accessor do registro");
        let quick_input_service = accessor.get(I_QUICK_INPUT_SERVICE);
        let model_service = accessor.get(I_MODEL_SERVICE);

        let Some(model) = editor.get_model() else {
            return Promise::resolved(());
        };

        let creation_opts = model_service.get_creation_options(&model.get_language_id(), &model.uri(), model.is_for_simple_widget());
        let model_opts = model.get_options();
        let picks: Vec<IQuickItem> = (1..=8)
            .map(|n: i32| IQuickItem {
                id: Some(n.to_string()),
                label: JsString::from(n.to_string().as_str()),
                // add description for tabSize value set in the configuration
                description: if n == creation_opts.tab_size && n == model_opts.tab_size {
                    Some(JsString::from(localize("configuredTabSize", "Configured Tab Size", &[]).as_str()))
                } else if n == creation_opts.tab_size {
                    Some(JsString::from(localize("defaultTabSize", "Default Tab Size", &[]).as_str()))
                } else if n == model_opts.tab_size {
                    Some(JsString::from(localize("currentTabSize", "Current Tab Size", &[]).as_str()))
                } else {
                    None
                },
                ..IQuickItem::default()
            })
            .collect();

        // auto focus the tabSize set for the current editor
        let auto_focus_index = (model_opts.tab_size - 1).clamp(0, 7) as usize;

        let (insert_spaces, display_size_only) = (self.insert_spaces, self.display_size_only);
        // quick input is sensitive to being opened so soon after another
        runtime::set_timeout(
            move || {
                let options = QuickPickOptions {
                    place_holder: localize("selectTabWidth", "Select Tab Size for Current File", &[]),
                    active_item: Some(picks[auto_focus_index].clone()),
                };
                let _ = quick_input_service.pick(picks, options).then(move |pick| {
                    let Some(pick) = pick else {
                        return;
                    };
                    if model.is_disposed() {
                        return;
                    }
                    let Ok(picked_val) = pick.label.to_string_lossy().parse::<i32>() else {
                        return;
                    };
                    if display_size_only {
                        model.update_options(TextModelUpdateOptions {
                            tab_size: Some(picked_val),
                            ..TextModelUpdateOptions::default()
                        });
                    } else {
                        model.update_options(TextModelUpdateOptions {
                            tab_size: Some(picked_val),
                            indent_size: Some(IndentSize::Size(picked_val)),
                            insert_spaces: Some(insert_spaces),
                            ..TextModelUpdateOptions::default()
                        });
                    }
                });
            },
            50,
        );
        Promise::resolved(())
    }
}

/// `DetectIndentation`.
pub struct DetectIndentation {
    action: EditorActionBase,
}

impl DetectIndentation {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(action_options(
                DETECT_INDENTATION_ID,
                ("detectIndentation", "Detect Indentation from Content"),
                ("detectIndentationDescription", "Detect the indentation from content."),
                None,
                false,
            )),
        }
    }
}

impl Default for DetectIndentation {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for DetectIndentation {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let accessor = accessor.expect("uma ação de editor roda com o accessor do registro");
        let model_service = accessor.get(I_MODEL_SERVICE);

        let Some(model) = editor.get_model() else {
            return Promise::resolved(());
        };

        let creation_opts = model_service.get_creation_options(&model.get_language_id(), &model.uri(), model.is_for_simple_widget());
        model.detect_indentation(creation_opts.insert_spaces, creation_opts.tab_size);
        Promise::resolved(())
    }
}

/// `ReindentLinesAction`.
pub struct ReindentLinesAction {
    action: EditorActionBase,
}

impl ReindentLinesAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(action_options(
                REINDENT_LINES_ID,
                ("editor.reindentlines", "Reindent Lines"),
                ("editor.reindentlinesDescription", "Reindent the lines of the editor."),
                Some(writable()),
                true,
            )),
        }
    }
}

impl Default for ReindentLinesAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for ReindentLinesAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let language_configuration_service = editor.language_configuration_service();

        let Some(model) = editor.get_model() else {
            return Promise::resolved(());
        };
        let edits = get_reindent_edit_operations(&model, &*language_configuration_service, 1, model.get_line_count());
        execute_reindent_edits(editor, &self.action.command.id, edits);
        Promise::resolved(())
    }
}

/// `ReindentSelectedLinesAction`.
pub struct ReindentSelectedLinesAction {
    action: EditorActionBase,
}

impl ReindentSelectedLinesAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(action_options(
                REINDENT_SELECTED_LINES_ID,
                ("editor.reindentselectedlines", "Reindent Selected Lines"),
                ("editor.reindentselectedlinesDescription", "Reindent the selected lines of the editor."),
                Some(writable()),
                true,
            )),
        }
    }
}

impl Default for ReindentSelectedLinesAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for ReindentSelectedLinesAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let language_configuration_service = editor.language_configuration_service();

        let Some(model) = editor.get_model() else {
            return Promise::resolved(());
        };

        let Some(selections) = editor.get_selections() else {
            return Promise::resolved(());
        };

        let mut edits: Vec<SingleEditOperation> = Vec::new();

        for selection in selections {
            let mut start_line_number = selection.start_line_number;
            let mut end_line_number = selection.end_line_number;

            if start_line_number != end_line_number && selection.end_column == 1 {
                end_line_number -= 1;
            }

            if start_line_number == 1 {
                if start_line_number == end_line_number {
                    continue;
                }
            } else {
                start_line_number -= 1;
            }

            edits.extend(get_reindent_edit_operations(&model, &*language_configuration_service, start_line_number, end_line_number));
        }

        execute_reindent_edits(editor, &self.action.command.id, edits);
        Promise::resolved(())
    }
}

/// O fim comum das duas ações de reindentação: se há edições, aplica-as entre dois pontos de desfazer.
fn execute_reindent_edits(editor: &Rc<CodeEditorWidget>, action_id: &str, edits: Vec<SingleEditOperation>) {
    if !edits.is_empty() {
        editor.push_undo_stop();
        editor.execute_edits(ExecuteEditsSource::Name(Some(action_id)), edits, None);
        editor.push_undo_stop();
    }
}

// #endregion

// #region AutoIndentOnPasteCommand

/// `languages.TextEdit`: o intervalo a trocar, o texto novo e, opcionalmente, o fim de linha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range,
    pub text: JsString,
    pub eol: Option<EndOfLineSequence>,
}

/// `AutoIndentOnPasteCommand`.
pub struct AutoIndentOnPasteCommand {
    edits: Vec<TextEdit>,
    initial_selection: Selection,
    selection_id: Option<String>,
}

impl AutoIndentOnPasteCommand {
    pub fn new(edits: Vec<TextEdit>, initial_selection: Selection) -> Self {
        Self { edits, initial_selection, selection_id: None }
    }
}

impl EditCommand for AutoIndentOnPasteCommand {
    fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        for edit in &self.edits {
            builder.add_edit_operation(edit.range, Some(edit.text.clone()), false);
        }

        let mut selection_is_set = false;
        if self.edits.len() == 1 && self.initial_selection.is_empty() {
            let range = self.edits[0].range;
            if range.start_column == self.initial_selection.end_column && range.start_line_number == self.initial_selection.end_line_number {
                selection_is_set = true;
                self.selection_id = Some(builder.track_selection(self.initial_selection, Some(true)));
            } else if range.end_column == self.initial_selection.start_column
                && range.end_line_number == self.initial_selection.start_line_number
            {
                selection_is_set = true;
                self.selection_id = Some(builder.track_selection(self.initial_selection, Some(false)));
            }
        }

        if !selection_is_set {
            self.selection_id = Some(builder.track_selection(self.initial_selection, None));
        }
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        helper.get_tracked_selection(self.selection_id.as_deref().expect("getEditOperations roda antes de computeCursorState"))
    }
}

// #endregion

// #region AutoIndentOnPaste

/// O `virtualModel` de `trigger`: o modelo com o conteúdo da primeira linha colada trocado pelo já reindentado.
struct PastedVirtualModel<'a> {
    model: &'a TextModel,
    first_line_number: i32,
    first_line_text: &'a JsString,
}

impl IVirtualModel for PastedVirtualModel<'_> {
    fn get_line_tokens(&self, line_number: i32) -> Box<dyn IViewLineTokens> {
        Box::new(self.model.tokenization().get_line_tokens(line_number))
    }

    fn get_language_id(&self) -> String {
        self.model.get_language_id()
    }

    fn get_language_id_at_position(&self, line_number: i32, column: i32) -> String {
        self.model.get_language_id_at_position(line_number, column)
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        if line_number == self.first_line_number {
            self.first_line_text.clone()
        } else {
            CursorSimpleModel::get_line_content(self.model, line_number)
        }
    }
}

/// `AutoIndentOnPaste`: reindenta o texto colado conforme as regras de indentação da linguagem.
pub struct AutoIndentOnPaste {
    editor: Weak<CodeEditorWidget>,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    call_on_dispose: Rc<DisposableStore>,
    call_on_model: Rc<DisposableStore>,
    this: Weak<AutoIndentOnPaste>,
}

impl AutoIndentOnPaste {
    pub const ID: &'static str = "editor.contrib.autoIndentOnPaste";

    pub fn new(editor: &Rc<CodeEditorWidget>, language_configuration_service: Rc<dyn ILanguageConfigurationService>) -> Rc<Self> {
        let call_on_dispose = DisposableStore::new();
        let this = Rc::new_cyclic(|this| Self {
            editor: Rc::downgrade(editor),
            language_configuration_service,
            call_on_dispose: call_on_dispose.clone(),
            call_on_model: DisposableStore::new(),
            this: this.clone(),
        });

        let update: Rc<dyn Fn()> = {
            let this = Rc::downgrade(&this);
            Rc::new(move || {
                if let Some(this) = this.upgrade() {
                    this.update();
                }
            })
        };
        let on_configuration = Rc::clone(&update);
        call_on_dispose.add(editor.on_did_change_configuration().listen(move |_| on_configuration()));
        let on_model = Rc::clone(&update);
        call_on_dispose.add(editor.on_did_change_model().listen(move |_| on_model()));
        call_on_dispose.add(editor.on_did_change_model_language().listen(move |_| update()));
        this
    }

    fn update(&self) {
        // clean up
        self.call_on_model.clear();

        let Some(editor) = self.editor.upgrade() else {
            return;
        };

        // we are disabled
        if !*editor.get_option(AUTO_INDENT_ON_PASTE) || *editor.get_option(AUTO_INDENT) < EditorAutoIndentStrategy::Full {
            return;
        }

        // no model
        if !editor.has_model() {
            return;
        }

        let this = self.this.clone();
        self.call_on_model.add(editor.on_did_paste().listen(move |event| {
            if let Some(this) = this.upgrade() {
                this.trigger(event.range);
            }
        }));
    }

    pub fn trigger(&self, range: Range) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let Some(selections) = editor.get_selections() else {
            return;
        };
        if selections.len() > 1 {
            return;
        }

        let Some(model) = editor.get_model() else {
            return;
        };
        let contains_only_whitespace = range_contains_only_whitespace_characters(&model, &range);
        if contains_only_whitespace {
            return;
        }
        if !*editor.get_option(AUTO_INDENT_ON_PASTE_WITHIN_STRING) && is_start_or_end_in_string(&model, &range) {
            return;
        }
        if !model.tokenization().is_cheap_to_tokenize(range.get_start_position().line_number) {
            return;
        }
        let auto_indent = *editor.get_option(AUTO_INDENT);
        let TextModelResolvedOptions { tab_size, indent_size, insert_spaces, .. } = model.get_options();
        let mut text_edits: Vec<TextEdit> = Vec::new();

        let indent_converter = MoveLinesIndentConverter { tab_size, indent_size, insert_spaces };
        let language_configuration_service = &*self.language_configuration_service;

        let mut start_line_number = range.start_line_number;

        let mut first_line_text = CursorSimpleModel::get_line_content(&*model, start_line_number);
        if trim(substring(first_line_text.units(), 0, i64::from(range.start_column - 1))).is_empty() {
            let indent_of_first_line = get_good_indent_for_line(
                auto_indent,
                &*model,
                &model.get_language_id(),
                start_line_number,
                &indent_converter,
                language_configuration_service,
            );

            if let Some(indent_of_first_line) = indent_of_first_line {
                let old_indentation = leading_whitespace(first_line_text.units());
                let new_space_cnt = get_space_cnt(indent_of_first_line.units(), tab_size);
                let old_space_cnt = get_space_cnt(old_indentation.units(), tab_size);

                if new_space_cnt != old_space_cnt {
                    let new_indent = generate_indent(new_space_cnt, tab_size, insert_spaces);
                    text_edits.push(TextEdit {
                        range: Range::new(start_line_number, 1, start_line_number, old_indentation.len() as i32 + 1),
                        text: new_indent.clone(),
                        eol: None,
                    });
                    let mut new_first_line_text = new_indent;
                    new_first_line_text.push_units(&first_line_text.units()[old_indentation.len()..]);
                    first_line_text = new_first_line_text;
                } else {
                    let indent_metadata = get_indent_metadata(&model, start_line_number, language_configuration_service);

                    if matches!(indent_metadata, Some(0) | Some(indent_consts::UNINDENT_MASK)) {
                        // we paste content into a line where only contains whitespaces
                        // after pasting, the indentation of the first line is already correct
                        // the first line doesn't match any indentation rule
                        // then no-op.
                        return;
                    }
                }
            }
        }

        let first_line_number = start_line_number;

        // ignore empty or ignored lines
        while start_line_number < range.end_line_number {
            if trim(CursorSimpleModel::get_line_content(&*model, start_line_number + 1).units()).is_empty() {
                start_line_number += 1;
                continue;
            }
            break;
        }

        if start_line_number != range.end_line_number {
            let virtual_model = PastedVirtualModel { model: &model, first_line_number, first_line_text: &first_line_text };
            let indent_of_second_line = get_good_indent_for_line(
                auto_indent,
                &virtual_model,
                &model.get_language_id(),
                start_line_number + 1,
                &indent_converter,
                language_configuration_service,
            );
            if let Some(indent_of_second_line) = indent_of_second_line {
                let new_space_cnt_of_second_line = get_space_cnt(indent_of_second_line.units(), tab_size);
                let old_space_cnt_of_second_line = get_space_cnt(
                    leading_whitespace(CursorSimpleModel::get_line_content(&*model, start_line_number + 1).units()).units(),
                    tab_size,
                );

                if new_space_cnt_of_second_line != old_space_cnt_of_second_line {
                    let space_cnt_offset = new_space_cnt_of_second_line - old_space_cnt_of_second_line;
                    for i in (start_line_number + 1)..=range.end_line_number {
                        let line_content = CursorSimpleModel::get_line_content(&*model, i);
                        let original_indent = leading_whitespace(line_content.units());
                        let original_spaces_cnt = get_space_cnt(original_indent.units(), tab_size);
                        let new_spaces_cnt = original_spaces_cnt + space_cnt_offset;
                        let new_indent = generate_indent(new_spaces_cnt, tab_size, insert_spaces);

                        if new_indent != original_indent {
                            text_edits.push(TextEdit {
                                range: Range::new(i, 1, i, original_indent.len() as i32 + 1),
                                text: new_indent,
                                eol: None,
                            });
                        }
                    }
                }
            }
        }

        if !text_edits.is_empty() {
            editor.push_undo_stop();
            let selection = editor.get_selection().expect("o editor tem modelo, então tem seleção");
            let cmd = AutoIndentOnPasteCommand::new(text_edits, selection);
            editor.execute_command(Some("autoIndentOnPaste"), Box::new(cmd));
            editor.push_undo_stop();
        }
    }
}

/// `rangeContainsOnlyWhitespaceCharacters`.
fn range_contains_only_whitespace_characters(model: &TextModel, range: &Range) -> bool {
    let line_contains_only_whitespace = |content: &[u16]| trim(content).is_empty();
    let mut contains_only_whitespace = true;
    if range.start_line_number == range.end_line_number {
        let line_content = CursorSimpleModel::get_line_content(model, range.start_line_number);
        let line_part = substring(line_content.units(), i64::from(range.start_column - 1), i64::from(range.end_column - 1));
        contains_only_whitespace = line_contains_only_whitespace(line_part);
    } else {
        for i in range.start_line_number..=range.end_line_number {
            let line_content = CursorSimpleModel::get_line_content(model, i);
            if i == range.start_line_number {
                let line_part = substring(line_content.units(), i64::from(range.start_column - 1), line_content.len() as i64);
                contains_only_whitespace = line_contains_only_whitespace(line_part);
            } else if i == range.end_line_number {
                let line_part = substring(line_content.units(), 0, i64::from(range.end_column - 1));
                contains_only_whitespace = line_contains_only_whitespace(line_part);
            } else {
                contains_only_whitespace = model.get_line_first_non_whitespace_column(i) == 0;
            }
            if !contains_only_whitespace {
                break;
            }
        }
    }
    contains_only_whitespace
}

impl IDisposable for AutoIndentOnPaste {
    fn dispose(&self) {
        self.call_on_dispose.dispose();
        self.call_on_model.dispose();
    }
}

impl IEditorContribution for AutoIndentOnPaste {}

/// `isStartOrEndInString`.
fn is_start_or_end_in_string(model: &TextModel, range: &Range) -> bool {
    let is_position_in_string = |position| {
        model.tokenization().get_standard_token_type_at_position(position) == Some(standard_token_type::STRING)
    };
    is_position_in_string(range.get_start_position()) || is_position_in_string(range.get_end_position())
}

// #endregion

// #region Comandos de conversão

/// `getIndentationEditOperations`: troca a indentação de cada linha, tabulações por espaços ou o inverso.
fn get_indentation_edit_operations(
    model: &TextModel,
    builder: &mut dyn EditOperationBuilder,
    tab_size: i32,
    conversion: IndentationConversion,
) {
    if model.get_line_count() == 1 && model.get_line_max_column(1) == 1 {
        // Model is empty
        return;
    }

    debug_assert!(tab_size >= 1, "o tamanho da tabulação do modelo é no mínimo 1");
    let spaces = vec![SPACE; tab_size.max(1) as usize];

    for line_number in 1..=model.get_line_count() {
        let mut last_indentation_column = model.get_line_first_non_whitespace_column(line_number);
        if last_indentation_column == 0 {
            last_indentation_column = model.get_line_max_column(line_number);
        }

        if last_indentation_column == 1 {
            continue;
        }

        let original_indentation_range = Range::new(line_number, 1, line_number, last_indentation_column);
        let original_indentation = model.get_value_in_range(&original_indentation_range);
        let new_indentation = match conversion {
            IndentationConversion::ToSpaces => replace_tabs_with_spaces(original_indentation.units(), &spaces),
            IndentationConversion::ToTabs => replace_spaces_with_tabs(original_indentation.units(), &spaces),
        };

        builder.add_edit_operation(original_indentation_range, Some(new_indentation), false);
    }
}

/// `indentation.replace(/\t/ig, spaces)`.
fn replace_tabs_with_spaces(indentation: &[u16], spaces: &[u16]) -> JsString {
    let mut result = JsString::new();
    for &unit in indentation {
        if unit == TAB {
            result.push_units(spaces);
        } else {
            result.push_unit(unit);
        }
    }
    result
}

/// `indentation.replace(new RegExp(spaces, 'gi'), '\t')`: troca cada trecho de `spaces`, da esquerda para a direita
/// e sem sobreposição.
fn replace_spaces_with_tabs(indentation: &[u16], spaces: &[u16]) -> JsString {
    let mut result = JsString::new();
    let mut index = 0;
    while index < indentation.len() {
        if indentation[index..].starts_with(spaces) {
            result.push_unit(TAB);
            index += spaces.len();
        } else {
            result.push_unit(indentation[index]);
            index += 1;
        }
    }
    result
}

/// `IndentationToSpacesCommand` e `IndentationToTabsCommand`.
pub struct IndentationCommand {
    selection: Selection,
    tab_size: i32,
    conversion: IndentationConversion,
    selection_id: Option<String>,
}

impl IndentationCommand {
    pub fn new(selection: Selection, tab_size: i32, conversion: IndentationConversion) -> Self {
        Self { selection, tab_size, conversion, selection_id: None }
    }
}

impl EditCommand for IndentationCommand {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        self.selection_id = Some(builder.track_selection(self.selection, None));
        get_indentation_edit_operations(model, builder, self.tab_size, self.conversion);
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        helper.get_tracked_selection(self.selection_id.as_deref().expect("getEditOperations roda antes de computeCursorState"))
    }
}

// #endregion

// #region getReindentEditOperations

/// `getReindentEditOperations`: as edições que reindentam as linhas de `start_line_number` a `end_line_number`
/// conforme as regras de indentação da linguagem do modelo.
pub fn get_reindent_edit_operations(
    model: &TextModel,
    language_configuration_service: &dyn ILanguageConfigurationService,
    start_line_number: i32,
    end_line_number: i32,
) -> Vec<SingleEditOperation> {
    if model.get_line_count() == 1 && model.get_line_max_column(1) == 1 {
        // Model is empty
        return Vec::new();
    }

    let language_configuration = language_configuration_service.get_language_configuration(&model.get_language_id());
    let Some(indentation_rules_support) = language_configuration.indent_rules_support.as_ref() else {
        return Vec::new();
    };

    let processed_indent_rules_support =
        ProcessedIndentRulesSupport::new(model, indentation_rules_support, language_configuration_service);
    let end_line_number = end_line_number.min(model.get_line_count());

    // Skip `unIndentedLinePattern` lines
    let mut start_line_number = start_line_number;
    while start_line_number <= end_line_number {
        if !processed_indent_rules_support.should_ignore(start_line_number, None) {
            break;
        }

        start_line_number += 1;
    }

    if start_line_number > end_line_number - 1 {
        return Vec::new();
    }

    let TextModelResolvedOptions { tab_size, indent_size, insert_spaces, .. } = model.get_options();
    let indent_converter = MoveLinesIndentConverter { tab_size, indent_size, insert_spaces };
    let mut indent_edits: Vec<SingleEditOperation> = Vec::new();

    // indentation being passed to lines below

    // Calculate indentation for the first line
    // If there is no passed-in indentation, we use the indentation of the first line as base.
    let current_line_text = CursorSimpleModel::get_line_content(model, start_line_number);
    let mut global_indent = leading_whitespace(current_line_text.units());
    // idealIndentForNextLine doesn't equal globalIndent when there is a line matching `indentNextLinePattern`.
    let mut ideal_indent_for_next_line = global_indent.clone();

    if processed_indent_rules_support.should_increase(start_line_number, None) {
        ideal_indent_for_next_line = indent_converter.shift_indent(ideal_indent_for_next_line.units());
        global_indent = indent_converter.shift_indent(global_indent.units());
    } else if processed_indent_rules_support.should_indent_next_line(start_line_number, None) {
        ideal_indent_for_next_line = indent_converter.shift_indent(ideal_indent_for_next_line.units());
    }

    start_line_number += 1;

    // Calculate indentation adjustment for all following lines
    for line_number in start_line_number..=end_line_number {
        if does_line_start_with_string(model, line_number) {
            continue;
        }
        let text = CursorSimpleModel::get_line_content(model, line_number);
        let old_indentation = leading_whitespace(text.units());
        let current_ideal_indent = ideal_indent_for_next_line.clone();

        if processed_indent_rules_support.should_decrease(line_number, Some(current_ideal_indent.units())) {
            ideal_indent_for_next_line = indent_converter.unshift_indent(ideal_indent_for_next_line.units());
            global_indent = indent_converter.unshift_indent(global_indent.units());
        }

        if old_indentation != ideal_indent_for_next_line {
            indent_edits.push(EditOperation::replace_move(
                Range::new(line_number, 1, line_number, old_indentation.len() as i32 + 1),
                Some(normalize_indentation(ideal_indent_for_next_line.units(), indent_size, insert_spaces)),
            ));
        }

        // calculate idealIndentForNextLine
        if processed_indent_rules_support.should_ignore(line_number, None) {
            // In reindent phase, if the line matches `unIndentedLinePattern` we inherit indentation from above lines
            // but don't change globalIndent and idealIndentForNextLine.
            continue;
        } else if processed_indent_rules_support.should_increase(line_number, Some(current_ideal_indent.units())) {
            global_indent = indent_converter.shift_indent(global_indent.units());
            ideal_indent_for_next_line = global_indent.clone();
        } else if processed_indent_rules_support.should_indent_next_line(line_number, Some(current_ideal_indent.units())) {
            ideal_indent_for_next_line = indent_converter.shift_indent(ideal_indent_for_next_line.units());
        } else {
            ideal_indent_for_next_line = global_indent.clone();
        }
    }

    indent_edits
}

/// `doesLineStartWithString`.
fn does_line_start_with_string(model: &TextModel, line_number: i32) -> bool {
    if !model.tokenization().is_cheap_to_tokenize(line_number) {
        return false;
    }
    let line_tokens = model.tokenization().get_line_tokens(line_number);
    line_tokens.get_standard_token_type(0) == standard_token_type::STRING
}

// #endregion

thread_local! {
    /// O carregamento do módulo acontece uma vez; o registro de ações e contribuições é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// As chamadas `registerEditorContribution(...)` e `registerEditorAction(...)` do fim do arquivo (o que o
/// carregamento do módulo faz no original). É idempotente: o registro acontece uma vez por thread.
pub fn register_indentation() {
    if REGISTERED.replace(true) {
        return;
    }
    register_editor_contribution(
        AutoIndentOnPaste::ID,
        Rc::new(|editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> {
            AutoIndentOnPaste::new(editor, editor.language_configuration_service())
        }),
        EditorContributionInstantiation::BeforeFirstInteraction,
    );
    register_editor_action(IndentationConversionAction::to_spaces);
    register_editor_action(IndentationConversionAction::to_tabs);
    register_editor_action(indent_using_tabs);
    register_editor_action(indent_using_spaces);
    register_editor_action(change_tab_display_size);
    register_editor_action(DetectIndentation::new);
    register_editor_action(ReindentLinesAction::new);
    register_editor_action(ReindentSelectedLinesAction::new);
}
