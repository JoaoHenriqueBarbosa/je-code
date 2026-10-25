//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineSuggestionsView.ts`: `InlineSuggestionsView`, a view
//! que junta o texto fantasma (um `GhostTextView` por trecho), a `InlineEditsView` (pelo
//! `InlineEditsViewAndDiffProducer`) e o indicador da margem, e escolhe quando cada um aparece.
//!
//! O `InlineCompletionsHintsWidget` (`hintsWidget/inlineCompletionsHintsWidget.ts`) é criado direto, como no original: depois
//! da folha de estilo, com `new InlineCompletionsHintsWidget(editor, model, instantiationService)`. O
//! `@IInstantiationService` é o [`InlineEditsViewServices`], de onde saem os serviços do widget
//! ([`InlineSuggestionHintsServices::from_view_services`]).
//!
//! Pendente, ligado por contrato (a peça do construtor original que ainda não tem porte):
//! - `InlineEditsOnboardingExperience` (`inlineEditsNewUsers.ts`): o original o cria num `derived` ao fim do construtor, a cada
//!   `InlineEditsViewAndDiffProducer`, com `w._inlineEditModel` ([`InlineEditsViewAndDiffProducer::inline_edit_model`]), o
//!   indicador ([`InlineSuggestionsView::indicator`]) e `w.view._inlineCollapsedView`
//!   ([`InlineEditsView::inline_collapsed_view`]). Depende do armazenamento e da configuração do usuário, e do `timeout`
//!   assíncrono, e entra junto do controller.
//!
//! Notas de tradução:
//! - `createHotClass(this)` (recarga a quente do desenvolvimento do VS Code) e a folha de estilo
//!   (`createStyleSheetFromObservable`) não existem: a folha só aplicava `font-family` aos textos fantasmas, e o pintor nativo
//!   lê a fonte em [`InlineSuggestionsView::font_family`] (`"default"` quando não há fonte própria);
//! - `@IInstantiationService` (que criava o `GhostTextView`, o indicador, o produtor e a experiência de novos usuários) é o
//!   [`InlineEditsViewServices`], que a `InlineEditsView` já usa; o indicador usa os serviços de tema e de acessibilidade e o
//!   `gutter_indicator_host` dele;
//! - os itens de `_ghostTexts` são `Rc<GhostTextOrReplacement>`: o `===` que `convertItemsToStableObservables` usa no original
//!   compara objetos novos a cada cálculo, e o `Rc` guarda a mesma identidade;
//! - `_inlineEdit` só era lido como booleano em `_everHadInlineEdit` (`!!this._inlineEdit.read(reader)`), então é o booleano
//!   `inline_edit_exists` (a presença da sugestão);
//! - `_indicatorIsHoverVisible` (o observável que quebra a dependência cíclica) é um `ObservableValue` com o observável do
//!   indicador, preenchido depois que o indicador existe;
//! - `derivedDisposable` de `_inlineEditWidget` é o `derived` que entrega o descarte do produtor ao `store` do leitor;
//! - `GhostTextView` não tem `IInstantiationService`: o `@ILanguageService` do original serve ao `languageIdCodec`.

use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{
    IObservable, IReaderWithStore, Observable, ObservableExt, SettableExt, const_observable, derived, derived_observable_with_cache,
    map_observable_array_cached_by, observable_value,
};
use je_base::runtime::date_now;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::inline_suggest::INLINE_SUGGEST;
use crate::inline_completions::ghost_text::GhostTextOrReplacement;
use crate::inline_completions::hints_widget::inline_completions_hints_widget::{InlineCompletionsHintsWidget, InlineSuggestionHintsServices};
use crate::inline_completions::inline_completions_model::{InlineCompletionsModel, InlineCompletionsModelState};
use crate::inline_completions::inline_suggestion_item::{InlineCompletionItem, InlineSuggestionAction, InlineSuggestionItem, InlineSuggestionItemBase};
use crate::inline_completions::utils::{convert_items_to_stable_observables, dispose_handle};
use crate::inline_completions::view::ghost_text_view::{GhostTextView, GhostTextViewOptions, GhostTextWidgetData, GhostTextWidgetWarning, IGhostTextWidgetData};
use crate::language_service::ILanguageService;
use crate::line_range::LineRange;
use crate::observable_code_editor::{ObservableCodeEditor, observable_code_editor};

use super::inline_edits::components::gutter_indicator_view::{
    GutterIndicatorServices, InlineEditsGutterIndicator, InlineEditsGutterIndicatorData, InlineSuggestionGutterMenuData, SimpleInlineSuggestModel,
};
use super::inline_edits::inline_edits_view::InlineEditsViewServices;
use super::inline_edits::inline_edits_view_interface::{InlineCompletionViewData, InlineCompletionViewKind, InlineEditTabAction};
use super::inline_edits::inline_edits_view_producer::InlineEditsViewAndDiffProducer;

/// O `_gutterIndicatorState` do original: `{ displayRange, tabAction, gutterIndicatorOffset, inlineSuggestion, model }`.
struct GutterIndicatorState {
    display_range: LineRange,
    tab_action: IObservable<InlineEditTabAction>,
    gutter_indicator_offset: IObservable<f64>,
    inline_suggestion: InlineSuggestionItem,
    model: Rc<InlineCompletionsModel>,
}

/// `InlineSuggestionsView`.
pub struct InlineSuggestionsView {
    base: Rc<Disposable>,
    ghost_text_widgets: IObservable<Rc<Vec<Rc<GhostTextView>>>>,
    inline_edit_widget: IObservable<Option<Rc<InlineEditsViewAndDiffProducer>>>,
    indicator: Rc<InlineEditsGutterIndicator>,
    font_family: IObservable<String>,
}

impl InlineSuggestionsView {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        model: IObservable<Option<Rc<InlineCompletionsModel>>>,
        focus_is_in_menu: Rc<ObservableValue<bool, ()>>,
        services: InlineEditsViewServices,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let editor_obs = observable_code_editor(&editor);

        let ghost_texts: IObservable<Rc<Vec<Rc<GhostTextOrReplacement>>>> = {
            let model = Rc::clone(&model);
            derived(move |reader| {
                let Some(model) = model.read(reader) else {
                    return Rc::new(Vec::new());
                };
                Rc::new(model.ghost_texts.read(reader).map_or_else(Vec::new, |ghost_texts| ghost_texts.iter().cloned().map(Rc::new).collect()))
            })
        };

        let stabilized_ghost_texts = convert_items_to_stable_observables(&ghost_texts, base.store());

        let ghost_text_widgets: IObservable<Rc<Vec<Rc<GhostTextView>>>> = {
            let (editor, model, editor_obs, language_service) =
                (Rc::clone(&editor), Rc::clone(&model), Rc::clone(&editor_obs), Rc::clone(&services.language_service));
            map_observable_array_cached_by(
                None,
                stabilized_ghost_texts,
                move |ghost_text: &IObservable<Rc<GhostTextOrReplacement>>, store| {
                    store.add_rc(create_ghost_text(&editor, &model, &editor_obs, ghost_text, &language_service))
                },
                |ghost_text| Rc::as_ptr(ghost_text).cast::<()>() as usize,
            )
            .recompute_initially_and_on_change(base.store(), None)
        };

        let inline_edit_exists: IObservable<bool> = {
            let model = Rc::clone(&model);
            derived(move |reader| model.read(reader).is_some_and(|model| model.inline_edit_state.read(reader).and_then(|state| state.inline_suggestion()).is_some()))
        };
        let ever_had_inline_edit: IObservable<bool> = {
            let model = Rc::clone(&model);
            derived_observable_with_cache::<bool>(None, move |reader, last| {
                last.copied().unwrap_or(false)
                    || inline_edit_exists.read(reader)
                    || model.read(reader).is_some_and(|model| {
                        model
                            .inline_completion_state
                            .read(reader)
                            .and_then(|state| state.inline_suggestion())
                            .is_some_and(|inline_suggestion| inline_suggestion.show_inline_edit_menu())
                    })
            })
        };

        // To break a cyclic dependency
        let indicator_is_hover_visible = observable_value::<Option<IObservable<bool>>, ()>("indicatorIsHoverVisible", None);

        let show_inline_edit_collapsed: IObservable<bool> = {
            let (model, indicator_is_hover_visible) = (Rc::clone(&model), indicator_is_hover_visible.to_observable());
            derived(move |reader| {
                let s = model.read(reader).is_some_and(|model| model.show_collapsed.read(reader));
                s && !indicator_is_hover_visible.read(reader).is_some_and(|is_hover_visible| is_hover_visible.read(reader))
            })
        };

        let inline_edit_widget: IObservable<Option<Rc<InlineEditsViewAndDiffProducer>>> = {
            let (editor, model, services) = (Rc::clone(&editor), Rc::clone(&model), services.clone());
            derived(move |reader| {
                if !ever_had_inline_edit.read(reader) {
                    return None;
                }
                let producer =
                    InlineEditsViewAndDiffProducer::new(Rc::clone(&editor), Rc::clone(&model), Rc::clone(&show_inline_edit_collapsed), services.clone());
                reader.store().add(dispose_handle(&producer));
                Some(producer)
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        let font_family: IObservable<String> = editor_obs.get_option(INLINE_SUGGEST).map(|options, _| options.font_family.clone());

        base.register(InlineCompletionsHintsWidget::new(Rc::clone(&editor), Rc::clone(&model), InlineSuggestionHintsServices::from_view_services(&services)));

        let gutter_indicator_state: IObservable<Option<Rc<GutterIndicatorState>>> = {
            let (model, editor, editor_obs, inline_edit_widget) =
                (Rc::clone(&model), Rc::clone(&editor), Rc::clone(&editor_obs), Rc::clone(&inline_edit_widget));
            derived(move |reader| {
                let model = model.read(reader)?;

                let state = model.state.read(reader);
                match state.as_deref() {
                    Some(InlineCompletionsModelState::GhostText(ghost_text_state)) => {
                        let inline_suggestion = ghost_text_state.inline_suggestion.as_ref().filter(|item| item.show_inline_edit_menu())?;
                        let tab_action: IObservable<InlineEditTabAction> = {
                            let editor_obs = Rc::clone(&editor_obs);
                            derived(move |reader| if editor_obs.is_focused.read(reader) { InlineEditTabAction::Accept } else { InlineEditTabAction::Inactive })
                        };
                        Some(Rc::new(GutterIndicatorState {
                            display_range: LineRange::of_length(ghost_text_state.primary_ghost_text.line_number(), 1),
                            tab_action,
                            gutter_indicator_offset: const_observable(get_ghost_text_top_offset(inline_suggestion, &editor)),
                            inline_suggestion: InlineSuggestionItem::Completion(inline_suggestion.clone()),
                            model,
                        }))
                    }
                    Some(InlineCompletionsModelState::InlineEdit(inline_edit_state)) => {
                        let inline_edit_widget = inline_edit_widget.read(reader)?;
                        let inline_edit_widget = inline_edit_widget.view();

                        let display_range = inline_edit_widget.display_range.read(reader)?;
                        let tab_action: IObservable<InlineEditTabAction> = {
                            let (model, editor_obs) = (Rc::clone(&model), Rc::clone(&editor_obs));
                            derived(move |reader| {
                                if editor_obs.is_focused.read(reader) {
                                    if model.tab_should_jump_to_inline_edit.read(reader) {
                                        return InlineEditTabAction::Jump;
                                    }
                                    if model.tab_should_accept_inline_edit.read(reader) {
                                        return InlineEditTabAction::Accept;
                                    }
                                }
                                InlineEditTabAction::Inactive
                            })
                        };
                        Some(Rc::new(GutterIndicatorState {
                            display_range,
                            tab_action,
                            gutter_indicator_offset: Rc::clone(&inline_edit_widget.gutter_indicator_offset),
                            inline_suggestion: InlineSuggestionItem::Edit(inline_edit_state.inline_suggestion.clone()),
                            model,
                        }))
                    }
                    None => None,
                }
            })
        };

        let indicator = InlineEditsGutterIndicator::new(
            Rc::clone(&editor_obs),
            {
                let gutter_indicator_state = Rc::clone(&gutter_indicator_state);
                derived(move |reader| {
                    let s = gutter_indicator_state.read(reader)?;
                    let alternative_action = match s.inline_suggestion.action() {
                        Some(InlineSuggestionAction::Edit(edit)) => edit.alternative_action,
                        _ => None,
                    };
                    Some(Rc::new(InlineEditsGutterIndicatorData::new(
                        Rc::new(InlineSuggestionGutterMenuData::from_inline_suggestion(&s.inline_suggestion)),
                        s.display_range,
                        SimpleInlineSuggestModel::from_inline_completion_model(&s.model),
                        alternative_action,
                        None,
                    )))
                })
            },
            gutter_indicator_state.map(|s, reader| s.map_or(InlineEditTabAction::Inactive, |s| s.tab_action.read(reader))),
            gutter_indicator_state.map(|s, reader| s.map_or(0.0, |s| s.gutter_indicator_offset.read(reader))),
            inline_edit_widget.map(|w, reader| w.is_some_and(|w| w.view().inline_edits_is_hovered.read(reader))),
            focus_is_in_menu,
            GutterIndicatorServices {
                theme_service: Rc::clone(&services.theme_service),
                accessibility_service: Rc::clone(&services.accessibility_service),
                host: Rc::clone(&services.gutter_indicator_host),
            },
        );
        base.register(indicator.clone());
        indicator_is_hover_visible.set(Some(indicator.is_hover_visible()), None);

        // Ponto de ligação do `InlineEditsOnboardingExperience`: ver o contrato no topo do módulo.

        Rc::new(Self { base, ghost_text_widgets, inline_edit_widget, indicator, font_family })
    }

    /// `shouldShowHoverAtViewZone(viewZoneId)`.
    pub fn should_show_hover_at_view_zone(&self, view_zone_id: &str) -> bool {
        self.ghost_text_widgets.get().first().is_some_and(|widget| widget.owns_view_zone(view_zone_id))
    }

    /// `_ghostTextWidgets`: um `GhostTextView` por trecho de texto fantasma.
    pub fn ghost_text_widgets(&self) -> IObservable<Rc<Vec<Rc<GhostTextView>>>> {
        Rc::clone(&self.ghost_text_widgets)
    }

    /// `_inlineEditWidget`: o produtor da `InlineEditsView` (`None` até a primeira edição ou menu de edição).
    pub fn inline_edit_widget(&self) -> IObservable<Option<Rc<InlineEditsViewAndDiffProducer>>> {
        Rc::clone(&self.inline_edit_widget)
    }

    /// `_indicator`.
    pub fn indicator(&self) -> &Rc<InlineEditsGutterIndicator> {
        &self.indicator
    }

    /// A fonte dos textos fantasmas (`.ghost-text-decoration`, `.ghost-text-decoration-preview` e `.ghost-text`), que a
    /// folha de estilo do original aplicava como `font-family`. `"default"` é a fonte do editor.
    pub fn font_family(&self) -> IObservable<String> {
        Rc::clone(&self.font_family)
    }
}

impl IDisposable for InlineSuggestionsView {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `_createGhostText(ghostText)`.
fn create_ghost_text(
    editor: &Rc<CodeEditorWidget>,
    model: &IObservable<Option<Rc<InlineCompletionsModel>>>,
    editor_obs: &Rc<ObservableCodeEditor>,
    ghost_text: &IObservable<Rc<GhostTextOrReplacement>>,
    language_service: &Rc<dyn ILanguageService>,
) -> Rc<GhostTextView> {
    let data: IObservable<Option<Rc<dyn IGhostTextWidgetData>>> = {
        let (model, ghost_text) = (Rc::clone(model), Rc::clone(ghost_text));
        derived(move |reader| -> Option<Rc<dyn IGhostTextWidgetData>> {
            let model = model.read(reader);
            let inline_completion = model.as_ref().and_then(|model| model.inline_completion_state.read(reader)).and_then(|state| state.inline_suggestion());
            let ghost_text = (*ghost_text.read(reader)).clone();
            let (Some(model), Some(inline_completion)) = (model, inline_completion) else {
                // editor.suggest.preview: true causes situations where we have ghost text, but no suggest preview.
                return Some(Rc::new(GhostTextWidgetData { ghost_text, warning: None, on_inline_completion_shown: None }));
            };

            let warning = model.warning.read(reader);
            let warning = GhostTextWidgetWarning::from(warning.as_deref());
            let shown_model = Rc::clone(&model);
            let on_inline_completion_shown: Rc<dyn Fn(&InlineCompletionViewData)> = Rc::new(move |view_data: &InlineCompletionViewData| {
                let _ = shown_model.handle_inline_suggestion_shown(&inline_completion, InlineCompletionViewKind::GhostText, view_data.clone(), date_now() as f64);
            });
            Some(Rc::new(GhostTextWidgetData { ghost_text, warning, on_inline_completion_shown: Some(on_inline_completion_shown) }))
        })
    };

    GhostTextView::new(
        Rc::clone(editor),
        data,
        GhostTextViewOptions {
            use_syntax_highlighting: Some(editor_obs.get_option(INLINE_SUGGEST).map(|options, _| options.syntax_highlighting_enabled)),
            highlight_short_suggestions: true,
            ..GhostTextViewOptions::default()
        },
        language_service.language_id_codec(),
    )
}

/// `getGhostTextTopOffset(inlineCompletion, editor)`.
fn get_ghost_text_top_offset(inline_completion: &InlineCompletionItem, editor: &Rc<CodeEditorWidget>) -> f64 {
    let replacement = inline_completion.get_single_text_edit();
    let Some(text_model) = editor.get_model() else {
        return 0.0;
    };

    let eol = text_model.get_eol();
    let eol = eol.as_units();
    if replacement.range.is_empty() && replacement.text.units().starts_with(eol) {
        let line_height = editor.get_line_height_for_position(replacement.range.get_start_position());
        return count_prefix_repeats(replacement.text.units(), eol) as f64 * line_height;
    }

    0.0
}

/// `countPrefixRepeats(str, prefix)`.
fn count_prefix_repeats(text: &[u16], prefix: &[u16]) -> usize {
    if prefix.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut i = 0;
    while text[i..].starts_with(prefix) {
        count += 1;
        i += prefix.len();
    }
    count
}
