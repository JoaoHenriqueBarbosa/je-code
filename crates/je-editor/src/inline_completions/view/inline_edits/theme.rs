//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/theme.ts`: os identificadores das cores
//! do inline edits, as funções de cor e a medida de raio de borda.
//!
//! Notas de tradução:
//! - as chamadas `registerColor` do original (com os padrões por tipo de tema e as descrições) já estão no registro do
//!   `je-theme` (`registered_colors`, gerado de todas as chamadas de `ref/vscode/src/vs`): cada constante daqui é o
//!   identificador da cor registrada, como `OVERVIEW_RULER_BRACKET_MATCH_FOREGROUND` do `bracket_matching`. Quem pinta
//!   resolve o valor pelo tema (`IColorTheme::get_color`);
//! - `ColorIdentifier` é `&'static str`: os identificadores registrados são estáticos;
//! - `Color` é `Rc<Color>`: o `===` do original compara a identidade de um objeto novo a cada cálculo, e a igualdade
//!   das cores observadas (`equalsFn: a.equals(b)`) é a de valor;
//! - `string | IObservable<string>` de `getEditorBlendedColor` é o [`ValueOrObservable`];
//! - `getEditorBackgroundColor` devolve o identificador da cor (o `asCssVariable` do original monta um `var(--vscode-...)`
//!   para o CSS, que não existe aqui). O `assertNever` do original é a exaustividade do `match`;
//! - `observeColor` usa o `IThemeService` de `services::theme_service`: sem a cor no tema o original lança um
//!   `BugIndicatingError`, aqui `errors::throw` com a mesma mensagem.

use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::observable::base::{IObservable, Observable};
use je_base::observable::debug_name::{DebugNameOptions, DebugNameSource};
use je_base::observable::deps::EqualityComparer;
use je_base::observable::observables::observable_from_event::{FromEventOptions, observable_from_event_opts};
use je_base::observable::ObservableExt;
use je_theme::color::Color;

use super::inline_edits_view_interface::InlineEditTabAction;
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::observable_code_editor::ValueOrObservable;
use crate::services::theme_service::IThemeService;

/// `editorBackground` (`editor.background`).
const EDITOR_BACKGROUND: &str = "editor.background";

/// A cor de fundo das células de notebook.
const NOTEBOOK_CELL_EDITOR_BACKGROUND: &str = "notebook.cellEditorBackground";

pub const ORIGINAL_BACKGROUND_COLOR: &str = "inlineEdit.originalBackground";
pub const MODIFIED_BACKGROUND_COLOR: &str = "inlineEdit.modifiedBackground";
pub const ORIGINAL_CHANGED_LINE_BACKGROUND_COLOR: &str = "inlineEdit.originalChangedLineBackground";
pub const ORIGINAL_CHANGED_TEXT_OVERLAY_COLOR: &str = "inlineEdit.originalChangedTextBackground";
pub const MODIFIED_CHANGED_LINE_BACKGROUND_COLOR: &str = "inlineEdit.modifiedChangedLineBackground";
pub const MODIFIED_CHANGED_TEXT_OVERLAY_COLOR: &str = "inlineEdit.modifiedChangedTextBackground";

// ------- GUTTER INDICATOR -------

pub const INLINE_EDIT_INDICATOR_PRIMARY_FOREGROUND: &str = "inlineEdit.gutterIndicator.primaryForeground";
pub const INLINE_EDIT_INDICATOR_PRIMARY_BORDER: &str = "inlineEdit.gutterIndicator.primaryBorder";
pub const INLINE_EDIT_INDICATOR_PRIMARY_BACKGROUND: &str = "inlineEdit.gutterIndicator.primaryBackground";

pub const INLINE_EDIT_INDICATOR_SECONDARY_FOREGROUND: &str = "inlineEdit.gutterIndicator.secondaryForeground";
pub const INLINE_EDIT_INDICATOR_SECONDARY_BORDER: &str = "inlineEdit.gutterIndicator.secondaryBorder";
pub const INLINE_EDIT_INDICATOR_SECONDARY_BACKGROUND: &str = "inlineEdit.gutterIndicator.secondaryBackground";

pub const INLINE_EDIT_INDICATOR_SUCCESSFUL_FOREGROUND: &str = "inlineEdit.gutterIndicator.successfulForeground";
pub const INLINE_EDIT_INDICATOR_SUCCESSFUL_BORDER: &str = "inlineEdit.gutterIndicator.successfulBorder";
pub const INLINE_EDIT_INDICATOR_SUCCESSFUL_BACKGROUND: &str = "inlineEdit.gutterIndicator.successfulBackground";

pub const INLINE_EDIT_INDICATOR_BACKGROUND: &str = "inlineEdit.gutterIndicator.background";

// ------- BORDER COLORS -------

const ORIGINAL_BORDER: &str = "inlineEdit.originalBorder";
const MODIFIED_BORDER: &str = "inlineEdit.modifiedBorder";
const TAB_WILL_ACCEPT_MODIFIED_BORDER: &str = "inlineEdit.tabWillAcceptModifiedBorder";
const TAB_WILL_ACCEPT_ORIGINAL_BORDER: &str = "inlineEdit.tabWillAcceptOriginalBorder";

/// `getModifiedBorderColor(tabAction)`.
pub fn get_modified_border_color(tab_action: &IObservable<InlineEditTabAction>) -> IObservable<&'static str> {
    tab_action.map(|a, _| if a == InlineEditTabAction::Accept { TAB_WILL_ACCEPT_MODIFIED_BORDER } else { MODIFIED_BORDER })
}

/// `getOriginalBorderColor(tabAction)`.
pub fn get_original_border_color(tab_action: &IObservable<InlineEditTabAction>) -> IObservable<&'static str> {
    tab_action.map(|a, _| if a == InlineEditTabAction::Accept { TAB_WILL_ACCEPT_ORIGINAL_BORDER } else { ORIGINAL_BORDER })
}

/// `getEditorBlendedColor(colorIdentifier, themeService)`: a cor sobre o fundo do editor, já opaca.
pub fn get_editor_blended_color(color_identifier: ValueOrObservable<&'static str>, theme_service: &Rc<dyn IThemeService>) -> IObservable<Rc<Color>> {
    let color: IObservable<Rc<Color>> = match color_identifier {
        ValueOrObservable::Value(identifier) => observe_color(identifier, theme_service),
        ValueOrObservable::Observable(identifiers) => {
            let theme_service = Rc::clone(theme_service);
            identifiers.map(move |identifier, reader| observe_color(identifier, &theme_service).read(reader))
        }
    };

    let background_color = observe_color(EDITOR_BACKGROUND, theme_service);

    // makeOpaque
    color.map(move |c, reader| Rc::new(c.make_opaque(&background_color.read(reader))))
}

/// `getEditorBackgroundColor(editorType)`: o identificador da cor de fundo do tipo de editor.
pub fn get_editor_background_color(editor_type: InlineCompletionEditorType) -> &'static str {
    match editor_type {
        InlineCompletionEditorType::TextEditor | InlineCompletionEditorType::DiffEditor => EDITOR_BACKGROUND,
        InlineCompletionEditorType::Notebook => NOTEBOOK_CELL_EDITOR_BACKGROUND,
    }
}

/// `observeColor(colorIdentifier, themeService)`: a cor do tema atual, que muda com `onDidColorThemeChange`.
pub fn observe_color(color_identifier: &'static str, theme_service: &Rc<dyn IThemeService>) -> IObservable<Rc<Color>> {
    let themes = Rc::clone(theme_service);
    let debug_name = format!("observeColor({color_identifier})");
    observable_from_event_opts(
        FromEventOptions {
            debug: DebugNameOptions { owner: None, debug_name: Some(DebugNameSource::from(debug_name.as_str())) },
            equals_fn: Some(Rc::new(|a: &Rc<Color>, b: &Rc<Color>| **a == **b) as EqualityComparer<Rc<Color>>),
            get_transaction: None,
        },
        theme_service.on_did_color_theme_change(),
        move |_| match themes.get_color_theme().get_color(color_identifier, None) {
            Some(color) => Rc::new(color),
            None => errors::throw(Error::bug_indicating(Some(&format!("Missing color: {color_identifier}")))),
        },
    )
}

// Styles

/// `INLINE_EDITS_BORDER_RADIUS`: o raio de borda das views (também usado no CSS do original).
pub const INLINE_EDITS_BORDER_RADIUS: f64 = 3.0;
