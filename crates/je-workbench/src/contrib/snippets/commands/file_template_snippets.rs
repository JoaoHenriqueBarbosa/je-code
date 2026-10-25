//! Porte de `src/vs/workbench/contrib/snippets/browser/commands/fileTemplateSnippets.ts`: a ação
//! `workbench.action.populateFileFromSnippet` ("Fill File with Snippet"), que troca o conteúdo do editor ativo pelo corpo de um
//! snippet de modelo de arquivo escolhido no seletor.
//!
//! Notas de tradução:
//! - `class ApplyFileSnippetAction extends SnippetsAction` é a struct [`ApplyFileSnippetAction`], que implementa `Action2` e
//!   monta o `desc` com [`snippets_action_options`] (o `super({ ...defaultOptions, ...desc })`). O `static readonly Id` é
//!   [`ApplyFileSnippetAction::ID`].
//! - `async run(accessor): Promise<void>` é [`apply_file_snippet`], que devolve a `Promise` do corpo; o `Action2::run` do porte
//!   não devolve valor, então o `run` a descarta (como em `url_contribution`). Quem confere o efeito espera por ela.
//! - `getCodeEditor(editorService.activeTextEditorControl)` é [`get_code_editor`]: o editor de código, o lado modificado de um
//!   editor de diff, ou nada.
//! - `editor.hasModel()` é o `get_model()` com valor. `editor.getModel().setLanguage(langService.createById(langId), Id)`
//!   resolve o `ILanguageSelection` na hora (`create_by_id(...).language_id()`), porque o `set_language` do modelo recebe o
//!   identificador: uma linguagem não registrada vira `plaintext`, como no `createById`.
//! - `type SnippetAndLanguagePick = IQuickPickItem & { snippet: SnippetAndLanguage }` não cabe no `IQuickPickItem` do porte
//!   (o `extra` é um objeto dinâmico): o item leva em `id` o índice do par (linguagem, snippet) na lista achatada, e o par
//!   escolhido é recuperado por esse índice.
//! - `groupBy(all, (a, b) => compare(a.langId, b.langId))` é o `group_by` de `je_base::arrays` sobre `strings::compare`
//!   (unidades UTF-16), e `snippet.prefix || snippet.name` escolhe o nome quando o prefixo é vazio.

use std::rc::Rc;

use je_base::JsString;
use je_base::arrays::group_by;
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::DisposableRef;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::strings::compare;
use je_editor::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE};
use je_editor::snippet::snippet_controller2::SnippetController2;
use je_editor::snippet::snippet_session::SnippetEdit;
use je_instantiation::ServicesAccessor;
use je_platform::action::{CommandAction, CommandActionTitle, Title, localize2};
use je_platform::actions::actions::{Action2, Action2Options, register_action2};
use je_platform::quickinput::quick_input::{
    I_QUICK_INPUT_SERVICE, IPickOptions, IQuickInputService, IQuickPickItem, IQuickPickSeparator, QuickPickInput, QuickPicks,
};

use super::abstract_snippets_actions::snippets_action_options;
use crate::contrib::snippets::snippets::{I_SNIPPETS_SERVICE, ISnippetGetOptions};
use crate::contrib::snippets::snippets_file::Snippet;
use crate::services::editor::editor_service::{ActiveTextEditorControl, I_EDITOR_SERVICE, IEditorService};

/// `ApplyFileSnippetAction`.
pub struct ApplyFileSnippetAction;

impl ApplyFileSnippetAction {
    /// `static readonly Id`.
    pub const ID: &'static str = "workbench.action.populateFileFromSnippet";
}

impl Action2 for ApplyFileSnippetAction {
    fn desc(&self) -> Action2Options {
        let title = localize2("label", "Fill File with Snippet");
        snippets_action_options(Action2Options {
            command: CommandAction::new(
                Self::ID,
                Title::Localized(CommandActionTitle { value: title.value, original: title.original, mnemonic_title: None }),
            ),
            menu: Vec::new(),
            keybinding: Vec::new(),
            f1: true,
        })
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let _ = apply_file_snippet(accessor);
    }
}

/// `registerAction2(ApplyFileSnippetAction)`.
pub fn register_apply_file_snippet_action() -> DisposableRef {
    register_action2(Rc::new(ApplyFileSnippetAction))
}

/// `getCodeEditor(editorService.activeTextEditorControl)`.
fn get_code_editor(control: Option<ActiveTextEditorControl>) -> Option<Rc<CodeEditorWidget>> {
    match control? {
        ActiveTextEditorControl::Code(editor) => editor.code_editor_widget(),
        ActiveTextEditorControl::Diff { modified, .. } => modified.code_editor_widget(),
    }
}

/// `SnippetAndLanguage`.
#[derive(Clone)]
struct SnippetAndLanguage {
    lang_id: String,
    snippet: Rc<Snippet>,
}

/// O corpo de `ApplyFileSnippetAction.run`.
pub fn apply_file_snippet(accessor: &ServicesAccessor) -> Promise<()> {
    let snippet_service = accessor.get(I_SNIPPETS_SERVICE);
    let quick_input_service = accessor.get(I_QUICK_INPUT_SERVICE);
    let editor_service = accessor.get(I_EDITOR_SERVICE);
    let lang_service = accessor.get(I_LANGUAGE_SERVICE);

    let Some(editor) = get_code_editor(editor_service.active_text_editor_control()) else {
        return Promise::resolved(());
    };
    let Some(model) = ICodeEditor::get_model(&*editor) else {
        return Promise::resolved(());
    };

    let resource_uri = model.uri();
    async_fn(async move {
        let options = ISnippetGetOptions {
            file_template_snippets: Some(true),
            no_recency_sort: Some(true),
            include_no_prefix_snippets: Some(true),
            ..ISnippetGetOptions::default()
        };
        let snippets = snippet_service.get_snippets(None, Some(&resource_uri), Some(&options)).await?;
        if snippets.is_empty() {
            return Ok(());
        }

        let Some(selection) = pick(&*quick_input_service, &*lang_service, &snippets).await? else {
            return Ok(());
        };

        if let Some(model) = ICodeEditor::get_model(&*editor) {
            // set language before applying so snippet comment variables resolve against this language
            let language_id = lang_service.create_by_id(Some(&selection.lang_id)).language_id();
            model.set_language(&language_id, Some(ApplyFileSnippetAction::ID));

            // apply snippet edit -> replaces everything
            if let Some(controller) = SnippetController2::get(&editor) {
                controller.apply(
                    vec![SnippetEdit {
                        range: model.get_full_model_range(),
                        template: JsString::from(selection.snippet.body.as_str()),
                        keep_whitespace: None,
                    }],
                    None,
                );
            }

            ICodeEditor::focus(&*editor);
        }
        Ok(())
    })
}

/// `_pick(quickInputService, langService, snippets)`.
async fn pick(
    quick_input_service: &dyn IQuickInputService,
    lang_service: &dyn ILanguageService,
    snippets: &[Rc<Snippet>],
) -> Result<Option<SnippetAndLanguage>, Error> {
    // spread snippet onto each language it supports
    let mut all: Vec<SnippetAndLanguage> = Vec::new();
    for snippet in snippets {
        if snippet.scopes.is_empty() {
            all.push(SnippetAndLanguage { lang_id: String::new(), snippet: snippet.clone() });
        } else {
            for lang_id in snippet.scopes.to_vec() {
                all.push(SnippetAndLanguage { lang_id, snippet: snippet.clone() });
            }
        }
    }

    let mut picks: Vec<QuickPickInput> = Vec::new();
    let mut ordered: Vec<SnippetAndLanguage> = Vec::new();

    let groups = group_by(&all, |a, b| {
        let a_units: Vec<u16> = a.lang_id.encode_utf16().collect();
        let b_units: Vec<u16> = b.lang_id.encode_utf16().collect();
        f64::from(compare(&a_units, &b_units))
    });

    for group in groups {
        let mut first = true;
        for item in group {
            if first {
                picks.push(QuickPickInput::Separator(IQuickPickSeparator {
                    label: Some(lang_service.get_language_name(&item.lang_id).unwrap_or_else(|| item.lang_id.clone())),
                    ..IQuickPickSeparator::default()
                }));
                first = false;
            }

            let label = if item.snippet.prefix.is_empty() { &item.snippet.name } else { &item.snippet.prefix };
            let mut pick_item = IQuickPickItem { detail: Some(item.snippet.description.clone()), ..IQuickPickItem::default() };
            pick_item.base.id = Some(ordered.len().to_string());
            pick_item.base.label = JsString::from(label.as_str());
            picks.push(QuickPickInput::Item(pick_item));
            ordered.push(item);
        }
    }

    let options = IPickOptions {
        place_holder: Some(localize("placeholder", "Select a snippet", &[])),
        match_on_detail: Some(true),
        ..IPickOptions::default()
    };
    let picked = quick_input_service.pick(QuickPicks::Items(picks), Some(&options), None).await?;
    Ok(picked
        .and_then(|item| item.base.id)
        .and_then(|id| id.parse::<usize>().ok())
        .and_then(|index| ordered.get(index).cloned()))
}
