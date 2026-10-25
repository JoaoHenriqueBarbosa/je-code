//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/hintsWidget/hoverParticipant.ts`: `InlineCompletionsHover`
//! e `InlineCompletionsHoverParticipant`, o participante que mostra o [`InlineSuggestionHintsContentWidget`] dentro do
//! hover do editor quando o ponteiro para sobre o texto fantasma (`showToolbar: 'onHover'`).
//!
//! Notas de tradução:
//! - **O que `IEditorHoverParticipant` não tem.** O trait do porte (`hover/hover_types`) só traz os membros que não
//!   dependem de DOM: `computeSync` e `renderHoverParts` ficam como métodos próprios do participante
//!   ([`InlineCompletionsHoverParticipant::compute_sync`] e [`InlineCompletionsHoverParticipant::render_hover_parts`]), com
//!   a mesma assinatura e o mesmo corpo, e o hover widget os chama quando for portado. O `IEditorHoverRenderContext` não tem
//!   porte: o participante só usa o `onContentsChanged` e o `fragment`, e o `fragment` é o retorno
//!   ([`RenderedInlineCompletionsHoverPart`]), então o contexto é o trait [`IInlineCompletionsHoverRenderContext`];
//! - **Controller.** O `InlineCompletionsController` (fatia 4c-6) ainda não tem porte; o `InlineCompletionsController.get(editor)`
//!   e o `GhostTextView.getWarningWidgetContext(element)` (o nó DOM do aviso é do pintor) entram pelo
//!   [`IInlineCompletionsHoverHost`], e o que o participante lê do controller (`model`, `shouldShowHoverAt` e
//!   `shouldShowHoverAtViewZone`) é o [`IInlineCompletionsHoverController`];
//! - **DOM vira dado.** `widgetNode.replaceChildren(w.getDomNode())` é o [`InlineSuggestionHintsContentWidget`] do retorno
//!   (o pintor lê o `visual()` dele). `renderScreenReaderText` monta a linha `div.hover-row.markdown-hover` com
//!   `div.hover-contents` (`aria-live: assertive`, [`SCREEN_READER_ARIA_LIVE`]) e renderiza o markdown pelo
//!   `IMarkdownRendererService`; o dado é o `IMarkdownString` observável ([`RenderedInlineCompletionsHoverPart::screen_reader_text`],
//!   `None` é o `dom.reset`), e o pintor renderiza e troca a classe do conteúdo para
//!   [`SCREEN_READER_CODE_HOVER_CONTENTS_CLASS_NAME`] quando a renderização assíncrona termina (o `asyncRenderCallback`), avisando o
//!   `onContentsChanged`. A linha do leitor de tela vem antes do widget, como no `fragment`;
//! - o `autorunWithStore` de `renderHoverParts` só lia `InlineSuggestionHintsContentWidget.hot` (recarga a quente), então roda
//!   uma vez: é a criação direta do widget;
//! - `@IInstantiationService` é o [`InlineSuggestionHintsServices`]; `@IMarkdownRendererService` é do pintor;
//! - `MouseTargetType.CONTENT_*` e o `target.detail` são as variantes de [`MouseTarget`]; `Range.fromPositions(p)` é
//!   `Range::from_positions(p, p)`; `this._editor.getModel()!` e `part.controller.model.get()!` (a asserção de não nulo)
//!   são o `?` de `suggest_hover_anchor` e o `expect` de `render_hover_parts`.

use std::rc::{Rc, Weak};

use je_base::html_content::{IMarkdownString, MarkdownString, MarkdownStringTextNewlineStyle, TrustedOrOptions};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::observable::{IObservable, Observable, SettableExt, autorun, const_observable, observable_value};
use je_platform::accessibility::IAccessibilityService;
use je_platform::telemetry::ITelemetryService;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::inline_suggest::{INLINE_SUGGEST, InlineSuggestShowToolbar};
use crate::config::editor_options::registry::SCREEN_READER_ANNOUNCE_INLINE_SUGGESTION;
use crate::cursor_common::CursorSimpleModel;
use crate::editor_browser::{EditorMouseEvent, ElementId, MouseTarget, MouseTargetType};
use crate::hover::hover_types::{HoverAnchor, HoverAnchorType, HoverForeignElementAnchor, IEditorHoverParticipant, IHoverPart};
use crate::inline_completions::inline_completions_model::InlineCompletionsModel;
use crate::model::ModelDecoration;
use crate::range::Range;

use super::inline_completions_hints_widget::{InlineSuggestionHintsContentWidget, InlineSuggestionHintsServices};

/// `aria-live: 'assertive'` do `div.hover-contents` do texto para leitores de tela.
pub const SCREEN_READER_ARIA_LIVE: &str = "assertive";
/// A classe que o `div.hover-contents` ganha quando a renderização assíncrona do markdown termina.
pub const SCREEN_READER_CODE_HOVER_CONTENTS_CLASS_NAME: &str = "hover-contents code-hover-contents";
/// A prioridade do `HoverForeignElementAnchor` que o participante sugere.
const HOVER_ANCHOR_PRIORITY: i32 = 1000;

/// O que o participante lê do `InlineCompletionsController`.
pub trait IInlineCompletionsHoverController {
    /// `controller.model`.
    fn model(&self) -> IObservable<Option<Rc<InlineCompletionsModel>>>;
    /// `shouldShowHoverAt(range)`.
    fn should_show_hover_at(&self, range: Range) -> bool;
    /// `shouldShowHoverAtViewZone(viewZoneId)`.
    fn should_show_hover_at_view_zone(&self, view_zone_id: &str) -> bool;
}

/// O que o participante pede do mundo de fora do porte: o `InlineCompletionsController.get(editor)` e o
/// `GhostTextView.getWarningWidgetContext(element)`.
pub trait IInlineCompletionsHoverHost {
    /// `InlineCompletionsController.get(editor)`.
    fn controller(&self) -> Option<Rc<dyn IInlineCompletionsHoverController>>;
    /// `GhostTextView.getWarningWidgetContext(element)?.range`: o intervalo do aviso desenhado no elemento, se ele é o
    /// widget de aviso do texto fantasma.
    fn warning_widget_range(&self, element: ElementId) -> Option<Range>;
}

/// O que o participante usa do `IEditorHoverRenderContext`: o aviso de que o conteúdo mudou de tamanho.
pub trait IInlineCompletionsHoverRenderContext {
    /// `onContentsChanged()`.
    fn on_contents_changed(&self);
}

/// `InlineCompletionsHover`.
#[derive(Clone)]
pub struct InlineCompletionsHover {
    pub owner: Rc<dyn IEditorHoverParticipant>,
    pub range: Range,
    pub controller: Rc<dyn IInlineCompletionsHoverController>,
}

impl IHoverPart for InlineCompletionsHover {
    fn owner(&self) -> Rc<dyn IEditorHoverParticipant> {
        Rc::clone(&self.owner)
    }

    fn range(&self) -> Range {
        self.range
    }

    fn is_valid_for_hover_anchor(&self, anchor: &HoverAnchor) -> bool {
        anchor.anchor_type() == HoverAnchorType::Range
            && self.range.start_column <= anchor.range().start_column
            && self.range.end_column >= anchor.range().end_column
    }
}

/// O `IRenderedHoverPart` de `renderHoverParts`: a parte, o conteúdo do hover e o descarte.
pub struct RenderedInlineCompletionsHoverPart {
    /// `hoverPart`.
    pub hover_part: InlineCompletionsHover,
    /// `hoverElement`: o conteúdo do hover (o pintor lê o `visual()` do widget).
    pub widget: Rc<InlineSuggestionHintsContentWidget>,
    /// A linha de texto para leitores de tela (só existe quando o leitor de tela está otimizado e a opção
    /// `screenReaderAnnounceInlineSuggestion` está desligada): o markdown a renderizar, ou `None` sem sugestão.
    pub screen_reader_text: Option<IObservable<Option<Rc<IMarkdownString>>>>,
    disposables: Rc<DisposableStore>,
}

impl IDisposable for RenderedInlineCompletionsHoverPart {
    /// `dispose() { disposables.dispose(); }`.
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// `InlineCompletionsHoverParticipant`.
pub struct InlineCompletionsHoverParticipant {
    me: Weak<Self>,
    editor: Rc<CodeEditorWidget>,
    host: Rc<dyn IInlineCompletionsHoverHost>,
    accessibility_service: Rc<dyn IAccessibilityService>,
    hints_services: InlineSuggestionHintsServices,
    telemetry_service: Rc<dyn ITelemetryService>,
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// O `IMarkdownString` de um `MarkdownString`.
fn markdown_dto(markdown: MarkdownString) -> IMarkdownString {
    IMarkdownString {
        value: markdown.value,
        is_trusted: markdown.is_trusted,
        support_theme_icons: markdown.support_theme_icons,
        support_html: markdown.support_html,
        support_alert_syntax: markdown.support_alert_syntax,
        base_uri: markdown.base_uri,
        uris: markdown.uris,
    }
}

impl InlineCompletionsHoverParticipant {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        host: Rc<dyn IInlineCompletionsHoverHost>,
        accessibility_service: Rc<dyn IAccessibilityService>,
        hints_services: InlineSuggestionHintsServices,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me| Self { me: me.clone(), editor, host, accessibility_service, hints_services, telemetry_service })
    }

    /// O participante como dono das partes e dos ancoradores (`this`).
    fn as_owner(&self) -> Rc<dyn IEditorHoverParticipant> {
        self.me.upgrade().expect("o participante vive enquanto o hover o usa")
    }

    /// O `HoverForeignElementAnchor(1000, this, range, posx, posy, false)`.
    fn foreign_anchor(&self, mouse_event: &EditorMouseEvent, range: Range) -> HoverAnchor {
        HoverAnchor::ForeignElement(HoverForeignElementAnchor {
            priority: HOVER_ANCHOR_PRIORITY,
            owner: self.as_owner(),
            range,
            initial_mouse_pos_x: Some(mouse_event.event.posx),
            initial_mouse_pos_y: Some(mouse_event.event.posy),
            supports_marker_hover: Some(false),
        })
    }

    /// `computeSync(anchor, lineDecorations)`.
    pub fn compute_sync(&self, anchor: &HoverAnchor, _line_decorations: &[ModelDecoration]) -> Vec<InlineCompletionsHover> {
        if self.editor.get_option(INLINE_SUGGEST).show_toolbar != InlineSuggestShowToolbar::OnHover {
            return Vec::new();
        }

        match self.host.controller() {
            Some(controller) if controller.should_show_hover_at(anchor.range()) => {
                vec![InlineCompletionsHover { owner: self.as_owner(), range: anchor.range(), controller }]
            }
            _ => Vec::new(),
        }
    }

    /// `renderHoverParts(context, hoverParts)`: o hover mostra o widget de dicas (sem borda própria, que é a do hover) e pede o
    /// disparo explícito ao modelo.
    pub fn render_hover_parts(
        &self,
        context: Rc<dyn IInlineCompletionsHoverRenderContext>,
        hover_parts: &[InlineCompletionsHover],
    ) -> RenderedInlineCompletionsHoverPart {
        let disposables = DisposableStore::new();
        let part = &hover_parts[0];

        // inlineCompletionHover.shown: 'This event tracks whenever an inline completion hover is shown.' (owner: hediet)
        self.telemetry_service.public_log2("inlineCompletionHover.shown", None);

        let screen_reader_text = (self.accessibility_service.is_screen_reader_optimized()
            && !*self.editor.get_option(SCREEN_READER_ANNOUNCE_INLINE_SUGGESTION))
        .then(|| self.render_screen_reader_text(&disposables, part));

        let model = part.controller.model().get().expect("o hover só é calculado com um modelo ativo");

        let widget = disposables.add_rc(InlineSuggestionHintsContentWidget::new(
            Rc::clone(&self.editor),
            false,
            const_observable(None),
            Rc::clone(&model.selected_inline_completion_index),
            Rc::clone(&model.inline_completions_count),
            Rc::clone(&model.active_commands),
            Rc::clone(&model.warning),
            Rc::new(move || context.on_contents_changed()),
            self.hints_services.clone(),
        ));

        let _ = model.trigger_explicitly(None, false);

        RenderedInlineCompletionsHoverPart { hover_part: part.clone(), widget, screen_reader_text, disposables }
    }

    /// `renderScreenReaderText(context, part)`: o markdown `Suggestion:` com o bloco de código do texto fantasma, relido
    /// quando o texto fantasma muda.
    fn render_screen_reader_text(&self, disposables: &Rc<DisposableStore>, part: &InlineCompletionsHover) -> IObservable<Option<Rc<IMarkdownString>>> {
        let content = observable_value::<Option<Rc<IMarkdownString>>, ()>("screenReaderHoverContent", None);

        disposables.add(autorun({
            let (editor, controller, content) = (Rc::clone(&self.editor), Rc::clone(&part.controller), Rc::clone(&content));
            move |reader| {
                /* @description update hover */
                let ghost_text = controller.model().read(reader).and_then(|model| model.primary_ghost_text.read(reader));
                let Some(ghost_text) = ghost_text else {
                    content.set(None, None);
                    return;
                };

                let text_model = editor.get_model().expect("o hover só é calculado com um modelo no editor");
                let line_text = CursorSimpleModel::get_line_content(&*text_model, ghost_text.line_number());
                let code = ghost_text.render_for_screen_reader(line_text.units());

                let inline_suggestion_available = localize("inlineSuggestionFollows", "Suggestion:", &[]);
                let mut markdown = MarkdownString::new(None, TrustedOrOptions::default());
                markdown
                    .append_text(&utf16(&inline_suggestion_available), MarkdownStringTextNewlineStyle::Paragraph)
                    .append_codeblock(&utf16("text"), code.units());
                content.set(Some(Rc::new(markdown_dto(markdown))), None);
            }
        }));

        content.to_observable()
    }
}

impl IEditorHoverParticipant for InlineCompletionsHoverParticipant {
    /// `hoverOrdinal = 4`.
    fn hover_ordinal(&self) -> i32 {
        4
    }

    /// `hideCopyButton = true`.
    fn hide_copy_button(&self) -> bool {
        true
    }

    fn suggest_hover_anchor(&self, mouse_event: &EditorMouseEvent) -> Option<HoverAnchor> {
        let controller = self.host.controller()?;

        match &mouse_event.target {
            MouseTarget::ViewZone(target) if target.target_type == MouseTargetType::ContentViewZone => {
                // handle the case where the mouse is over the view zone
                let view_zone_data = &target.detail;
                if controller.should_show_hover_at_view_zone(&view_zone_data.view_zone_id) {
                    let text_model = self.editor.get_model()?;
                    let position = text_model.validate_position(view_zone_data.position_before.unwrap_or(view_zone_data.position));
                    return Some(self.foreign_anchor(mouse_event, Range::from_positions(position, position)));
                }
            }
            MouseTarget::ContentEmpty(target) => {
                // handle the case where the mouse is over the empty portion of a line following ghost text
                if controller.should_show_hover_at(target.range) {
                    return Some(self.foreign_anchor(mouse_event, target.range));
                }
            }
            MouseTarget::ContentText(target) => {
                // handle the case where the mouse is directly over ghost text
                if target.detail.might_be_foreign_element && controller.should_show_hover_at(target.range) {
                    return Some(self.foreign_anchor(mouse_event, target.range));
                }
            }
            MouseTarget::ContentWidget(target) => {
                if let Some(range) = target.element.and_then(|element| self.host.warning_widget_range(element))
                    && controller.should_show_hover_at(range)
                {
                    return Some(self.foreign_anchor(mouse_event, range));
                }
            }
            _ => {}
        }
        None
    }

    fn get_accessible_content(&self, _hover_part: &dyn IHoverPart) -> String {
        localize("hoverAccessibilityStatusBar", "There are inline completions here", &[])
    }
}
