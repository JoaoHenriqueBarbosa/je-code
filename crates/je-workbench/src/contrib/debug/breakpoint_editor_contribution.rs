//! Porte parcial de `src/vs/workbench/contrib/debug/browser/breakpointEditorContribution.ts`: `createBreakpointDecorations`
//! e `getBreakpointDecorationOptions`, que transformam os breakpoints de um modelo de texto nas decorações da margem do
//! editor (o ícone, o hover com a condição e o aviso do adaptador, a marca do `overviewRuler` e o marcador em linha).
//!
//! Notas de tradução:
//! - O `ServicesAccessor` (`accessor.get(IDebugService)`, `ILanguageService`, `ILabelService`) é o
//!   [`BreakpointDecorationServices`]: o `DebugModel` (`debugService.getModel()`), o [`IDebuggerLookup`]
//!   (`debugService.getAdapterManager().getDebugger`), o `ILabelService` e o `ILanguageService`.
//! - O `ITextModel` é o [`TextModel`] do je-editor (`getLineCount`, `getLineFirstNonWhitespaceColumn` e `getLanguageId`).
//! - O `IModelDecorationOptions` é o [`ModelDecorationOptionsInit`]; `themeColorFromId(debugIconBreakpointForeground)` é a
//!   [`DecorationColor::Theme`] do id registrado (`debugIcon.breakpointForeground`).
//!
//! Divergências:
//! - O `IMarkdownString` do `ModelDecorationOptionsInit` do je-editor guarda só o `value` (é um substituto até o
//!   `htmlContent` entrar lá). O hover completo, o `MarkdownString` do je-base com `isTrusted` e `supportThemeIcons`, vai ao
//!   lado em [`BreakpointDecoration::glyph_margin_hover_message`]; as `options` levam o mesmo texto.
//! - O resto do arquivo (`BreakpointEditorContribution`, `BreakpointEditorContribution.createBreakpointDecorations` sobre o
//!   editor, os widgets de condição, o menu de contexto da margem e os candidatos em linha) pinta widgets e ouve o mouse e
//!   fica com a camada nativa do editor.

use std::rc::Rc;

use je_base::JsString;
use je_base::html_content::{IsTrusted, MarkdownString, MarkdownStringOptions, TrustedOrOptions};
use je_base::strings::NO_BREAK_WHITESPACE;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::language_service::ILanguageService;
use je_editor::model::{
    DecorationColor, GlyphMarginLane, InjectedTextOptions, ModelDecorationGlyphMarginInit, ModelDecorationOptionsInit, ModelDecorationOverviewRulerInit,
    OverviewRulerLane, TrackedRangeStickiness,
};
use je_editor::model::MarkdownString as DecorationMarkdownString;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_platform::label::label::ILabelService;

use super::breakpoints::Breakpoint;
use super::breakpoints_view::get_breakpoint_message_and_icon;
use super::debug::IDebugSession;
use super::debug_model::{BreakpointElement, DebugModel};
use super::debug_types::State;
use super::debugger::Debugger;

/// `debugIcon.breakpointForeground`, o `debugIconBreakpointForeground` registrado no fim do arquivo original.
pub const DEBUG_ICON_BREAKPOINT_FOREGROUND: &str = "debugIcon.breakpointForeground";

/// `DebuggerString.UnverifiedBreakpoints`.
const UNVERIFIED_BREAKPOINTS: &str = "unverifiedBreakpoints";

/// `debugService.getAdapterManager().getDebugger(type)`.
pub trait IDebuggerLookup {
    fn get_debugger(&self, debug_type: &str) -> Option<Rc<Debugger>>;
}

impl IDebuggerLookup for super::adapter_manager::AdapterManager {
    fn get_debugger(&self, debug_type: &str) -> Option<Rc<Debugger>> {
        super::adapter_manager::AdapterManager::get_debugger(self, debug_type)
    }
}

/// Os serviços que `getBreakpointDecorationOptions` lê do `ServicesAccessor`.
#[derive(Clone)]
pub struct BreakpointDecorationServices {
    pub debug_model: Rc<DebugModel>,
    pub adapter_manager: Rc<dyn IDebuggerLookup>,
    pub label_service: Rc<dyn ILabelService>,
    pub language_service: Rc<dyn ILanguageService>,
}

/// `{ range, options }` de uma decoração de breakpoint.
#[derive(Clone, Debug)]
pub struct BreakpointDecoration {
    pub range: Range,
    pub options: ModelDecorationOptionsInit,
    /// O `glyphMarginHoverMessage` completo (ver o cabeçalho do módulo).
    pub glyph_margin_hover_message: Option<MarkdownString>,
}

fn js(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `createBreakpointDecorations(accessor, model, breakpoints, state, breakpointsActivated, showBreakpointsInOverviewRuler)`.
pub fn create_breakpoint_decorations(
    services: &BreakpointDecorationServices,
    model: &TextModel,
    breakpoints: &[Rc<Breakpoint>],
    state: State,
    breakpoints_activated: bool,
    show_breakpoints_in_overview_ruler: bool,
) -> Vec<BreakpointDecoration> {
    let mut result: Vec<BreakpointDecoration> = Vec::new();
    for breakpoint in breakpoints {
        if breakpoint.line_number() > i64::from(model.get_line_count()) {
            continue;
        }
        let line_number = breakpoint.line_number() as i32;
        let has_other_breakpoints_on_line = breakpoints.iter().any(|bp| !Rc::ptr_eq(bp, breakpoint) && bp.line_number() == breakpoint.line_number());
        let column = model.get_line_first_non_whitespace_column(line_number);
        let range = model.validate_range(&match breakpoint.column().filter(|column| *column != 0) {
            Some(breakpoint_column) => Range::new(line_number, breakpoint_column as i32, line_number, breakpoint_column as i32 + 1),
            None => Range::new(line_number, column, line_number, column + 1), // Decoration has to have a width #20688
        });

        let (options, glyph_margin_hover_message) = get_breakpoint_decoration_options(
            services,
            model,
            breakpoint,
            state,
            breakpoints_activated,
            show_breakpoints_in_overview_ruler,
            has_other_breakpoints_on_line,
        );
        result.push(BreakpointDecoration { range, options, glyph_margin_hover_message });
    }

    result
}

fn get_breakpoint_decoration_options(
    services: &BreakpointDecorationServices,
    model: &TextModel,
    breakpoint: &Rc<Breakpoint>,
    state: State,
    breakpoints_activated: bool,
    show_breakpoints_in_overview_ruler: bool,
    has_other_breakpoints_on_line: bool,
) -> (ModelDecorationOptionsInit, Option<MarkdownString>) {
    let message_and_icon = get_breakpoint_message_and_icon(
        state,
        breakpoints_activated,
        &BreakpointElement::Breakpoint(breakpoint.clone()),
        &*services.label_service,
        &services.debug_model,
    );
    let mut glyph_margin_hover_message: Option<MarkdownString> = None;

    let mut unverified_message: Option<String> = None;
    if message_and_icon.show_adapter_unverified_message {
        let mut lang_id: Option<String> = None;
        unverified_message = services
            .debug_model
            .get_sessions(false)
            .iter()
            .map(|session| {
                let debug_type = session.configuration().get("type").and_then(|value| value.as_str()).unwrap_or_default().to_owned();
                let dbg = services.adapter_manager.get_debugger(&debug_type);
                let message = dbg
                    .as_ref()
                    .and_then(|dbg| dbg.strings())
                    .and_then(|strings| strings.get(UNVERIFIED_BREAKPOINTS).and_then(|value| value.as_str().map(str::to_owned)))
                    .filter(|message| !message.is_empty());
                let message = message?;
                if lang_id.is_none() {
                    // Lazily compute this, only if needed for some debug adapter
                    lang_id = services.language_service.guess_language_id_by_filepath_or_first_line(Some(&breakpoint.uri()), None);
                }
                match (&lang_id, &dbg) {
                    (Some(lang_id), Some(dbg)) if !lang_id.is_empty() && dbg.interested_in_language(lang_id) => Some(message),
                    _ => None,
                }
            })
            .find(Option::is_some)
            .flatten();
    }

    let new_markdown = || {
        MarkdownString::new(
            None,
            TrustedOrOptions::Options(MarkdownStringOptions {
                is_trusted: Some(IsTrusted::Bool(true)),
                support_theme_icons: Some(true),
                ..MarkdownStringOptions::default()
            }),
        )
    };
    if let Some(message) = &message_and_icon.message {
        let mut markdown = new_markdown();
        if breakpoint.condition().is_some_and(|condition| !condition.is_empty()) || breakpoint.hit_condition().is_some_and(|condition| !condition.is_empty()) {
            let language_id = model.get_language_id();
            markdown.append_codeblock(&js(&language_id), &js(message));
            if let Some(unverified_message) = &unverified_message {
                markdown.append_markdown(&js(&format!("$(warning) {unverified_message}")));
            }
        } else {
            markdown.append_text(&js(message), Default::default());
            if let Some(unverified_message) = &unverified_message {
                markdown.append_markdown(&js(&format!("\n\n$(warning) {unverified_message}")));
            }
        }
        glyph_margin_hover_message = Some(markdown);
    } else if let Some(unverified_message) = &unverified_message {
        let mut markdown = new_markdown();
        markdown.append_markdown(&js(unverified_message));
        glyph_margin_hover_message = Some(markdown);
    }

    let overview_ruler_decoration = show_breakpoints_in_overview_ruler.then(|| ModelDecorationOverviewRulerInit {
        color: Some(DecorationColor::Theme(DEBUG_ICON_BREAKPOINT_FOREGROUND.to_owned())),
        dark_color: None,
        position: Some(OverviewRulerLane::Left),
    });

    let line_number = breakpoint.line_number() as i32;
    let render_inline = breakpoint
        .column()
        .filter(|column| *column != 0)
        .is_some_and(|column| has_other_breakpoints_on_line || column as i32 > model.get_line_first_non_whitespace_column(line_number));
    let options = ModelDecorationOptionsInit {
        description: "breakpoint-decoration".to_owned(),
        glyph_margin: Some(ModelDecorationGlyphMarginInit { position: GlyphMarginLane::Right, persist_lane: None }),
        glyph_margin_class_name: Some(message_and_icon.icon.as_class_name()),
        glyph_margin_hover_message: glyph_margin_hover_message.as_ref().map(|markdown| vec![DecorationMarkdownString { value: js_string_to_string(&markdown.value) }]),
        stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
        before: render_inline.then(|| InjectedTextOptions {
            content: String::from_utf16_lossy(&NO_BREAK_WHITESPACE),
            inline_class_name: Some("debug-breakpoint-placeholder".to_owned()),
            inline_class_name_affects_letter_spacing: true,
            width_in_em: Some(0.9),
            ..InjectedTextOptions::default()
        }),
        overview_ruler: overview_ruler_decoration,
        z_index: 9999,
        ..ModelDecorationOptionsInit::default()
    };
    (options, glyph_margin_hover_message)
}

fn js_string_to_string(value: &JsString) -> String {
    String::from_utf16_lossy(value.units())
}
