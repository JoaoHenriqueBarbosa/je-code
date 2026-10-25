//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/suggestWidgetAdapter.ts`: o `SuggestWidgetAdaptor`
//! (acompanha o item em foco do widget de sugestões e pré-seleciona o item que a sugestão em linha completa), a fábrica
//! `SuggestItemInfo.fromSuggestion` e o `ObservableSuggestWidgetAdapter` (o mesmo adaptador visto como observável). O
//! `SuggestItemInfo` e a função `suggestItemInfoEquals` vivem com o resto do estado em `inline_completions_model` (a
//! fábrica é um `impl` daquele tipo, aqui).
//!
//! Notas de tradução:
//! - `SuggestWidgetAdaptor` é um `Rc<Self>` criado por `Rc::new_cyclic`: os ouvintes dos eventos do editor e do widget
//!   guardam só um `Weak` dele, e o `SuggestController` é pedido a cada uso por `SuggestController::get(editor)` (como o
//!   original faz em `getSuggestItemInfo` e nos `...ForceRenderingAbove`), para o seletor registrado no controller não
//!   prender o próprio controller;
//! - `private constructor` de `SuggestItemInfo` é o [`SuggestItemInfo::new`] público do modelo (o porte não tem
//!   construtor privado);
//! - `suggestItemInfoEquals(a, b)` compara `a === b` primeiro: o item atual é um `Rc` que só é substituído quando muda, então a
//!   identidade é `Rc::ptr_eq`;
//! - os ouvintes `this._register(editor.onKeyDown(...))` do original são as inscrições registradas no `base`;
//! - `new Event.once(suggestController.model.onDidTrigger)` é [`Event::once`]; `bindToSuggestWidget` roda uma vez só;
//! - o `ObservableSuggestWidgetAdapter` não repete `stopForceRenderingAbove` e `forceRenderingAbove`, que no original só
//!   repassam ao adaptador com os mesmos argumentos: quem os usa chama o `suggest_widget_adaptor` público.
//!
//! Divergências:
//! - o `SuggestWidget` e a parte do `SuggestController` que depende dele (`registerSelector`, `widget.value`,
//!   `forceRenderingAbove` e `stopForceRenderingAbove`) ainda não têm porte (ver `suggest_widget_contract.rs` e o topo de
//!   `suggest_controller.rs`). Esses quatro membros entram como o [`SuggestWidgetHost`], que o `SuggestController` passa a
//!   implementar com o porte do `suggestWidget.ts`; o resto do que o adaptador lê do controller (`getOverwriteInfo`,
//!   `model.onDidTrigger` e `onWillInsertSuggestItem`) é o `SuggestController` real;
//! - a `select` do seletor recebe o `ITextModel` (que o adaptador não usa: ele lê o modelo do editor, como o original) e a
//!   posição já como [`Position`] (o `Position.lift(pos)` do original).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::arrays::{compare_by, number_comparator};
use je_base::arrays_find::find_first_max;
use je_base::event::{Emitter, Event, Listener};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::observable::base::IObservable;
use je_base::observable::observable_from_event;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::edits::text_edit::TextReplacement;
use crate::inline_completions::compute_ghost_text::{single_text_edit_augments, single_text_remove_common_prefix};
use crate::inline_completions::inline_completions_model::SuggestItemInfo;
use crate::languages::completion_item_insert_text_rule;
use crate::observable_code_editor::ObservableCodeEditor;
use crate::position::Position;
use crate::range::Range;
use crate::snippet::snippet_parser::SnippetParser;
use crate::snippet::snippet_session::SnippetSession;
use crate::suggest::suggest::CompletionItem;
use crate::suggest::suggest_controller::{SuggestController, WillInsertSuggestItem};
use crate::suggest::suggest_widget_contract::ISuggestWidget;
use crate::text_model::TextModel;

/// O argumento de `suggestController.registerSelector(...)`: `select` devolve o índice do item a pré-selecionar, ou `-1`.
#[derive(Clone)]
pub struct SuggestItemSelector {
    pub priority: i32,
    pub select: Rc<dyn Fn(&Rc<TextModel>, Position, &[CompletionItem]) -> i32>,
}

/// O que o adaptador pede ao `SuggestController` e que depende do `SuggestWidget` (ver as divergências do módulo).
pub trait SuggestWidgetHost {
    /// `registerSelector(selector)`.
    fn register_selector(&self, selector: SuggestItemSelector) -> DisposableRef;

    /// `widget.value`.
    fn widget(&self) -> Rc<dyn ISuggestWidget>;

    /// `forceRenderingAbove()`.
    fn force_rendering_above(&self);

    /// `stopForceRenderingAbove()`.
    fn stop_force_rendering_above(&self);
}

impl SuggestItemInfo {
    /// `SuggestItemInfo.fromSuggestion(suggestController, model, position, item, toggleMode)`.
    pub fn from_suggestion(
        suggest_controller: &SuggestController,
        model: &TextModel,
        position: Position,
        item: &CompletionItem,
        toggle_mode: bool,
    ) -> SuggestItemInfo {
        let (insert_text, insert_text_rules, kind) = {
            let completion = item.completion.borrow();
            (completion.insert_text.clone(), completion.insert_text_rules.unwrap_or(0), completion.kind)
        };

        let mut insert_text = JsString::from(insert_text);
        let mut is_snippet_text = false;
        if insert_text_rules & completion_item_insert_text_rule::INSERT_AS_SNIPPET != 0 {
            let snippet = SnippetParser::new().parse(insert_text.units(), false, false);

            if snippet.children().len() < 100 {
                // Adjust whitespace is expensive.
                SnippetSession::adjust_whitespace(model, position, true, &snippet, None);
            }

            insert_text = snippet.to_js_string();
            is_snippet_text = true;
        }

        let info = suggest_controller.get_overwrite_info(item, toggle_mode);

        SuggestItemInfo::new(
            Range::from_positions(position.delta(0, -info.overwrite_before), position.delta(0, info.overwrite_after.max(0))),
            insert_text,
            kind,
            is_snippet_text,
            item.container.incomplete.unwrap_or(false),
        )
    }
}

/// `suggestItemInfoEquals(a, b)`.
fn suggest_item_info_equals(a: &Option<Rc<SuggestItemInfo>>, b: &Option<Rc<SuggestItemInfo>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b) || a.equals(b),
        _ => false,
    }
}

/// Um item que o seletor considera: o índice na lista e o tamanho do texto que ele acrescenta à sugestão pré-selecionada.
struct PreselectCandidate {
    index: usize,
    prefix_length: usize,
}

/// `SuggestWidgetAdaptor`.
pub struct SuggestWidgetAdaptor {
    me: Weak<SuggestWidgetAdaptor>,
    base: Rc<Disposable>,
    editor: Rc<CodeEditorWidget>,
    widget_host: Rc<dyn SuggestWidgetHost>,
    suggest_controller_preselector: Rc<dyn Fn() -> Option<TextReplacement>>,
    on_will_accept: Rc<dyn Fn(&SuggestItemInfo)>,

    is_suggest_widget_visible: Cell<bool>,
    is_shift_key_pressed: Cell<bool>,
    is_active: Cell<bool>,
    is_bound_to_suggest_widget: Cell<bool>,
    current_suggest_item_info: RefCell<Option<Rc<SuggestItemInfo>>>,
    on_did_selected_item_change: Rc<Emitter<()>>,
}

impl SuggestWidgetAdaptor {
    pub fn new(
        editor: &Rc<CodeEditorWidget>,
        widget_host: Rc<dyn SuggestWidgetHost>,
        suggest_controller_preselector: Rc<dyn Fn() -> Option<TextReplacement>>,
        on_will_accept: Rc<dyn Fn(&SuggestItemInfo)>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_selected_item_change = Emitter::<()>::new();
        base.register(on_did_selected_item_change.clone());

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            editor: Rc::clone(editor),
            widget_host,
            suggest_controller_preselector,
            on_will_accept,
            is_suggest_widget_visible: Cell::new(false),
            is_shift_key_pressed: Cell::new(false),
            is_active: Cell::new(false),
            is_bound_to_suggest_widget: Cell::new(false),
            current_suggest_item_info: RefCell::new(None),
            on_did_selected_item_change,
        });
        this.bind_to_editor_and_controller();
        this
    }

    /// `selectedItem`.
    pub fn selected_item(&self) -> Option<Rc<SuggestItemInfo>> {
        self.current_suggest_item_info.borrow().clone()
    }

    /// `onDidSelectedItemChange`.
    pub fn on_did_selected_item_change(&self) -> Event<()> {
        self.on_did_selected_item_change.event()
    }

    /// O corpo do construtor depois dos campos: as teclas do editor, o seletor, os eventos do widget e a aceitação.
    fn bind_to_editor_and_controller(self: &Rc<Self>) {
        // See the command acceptAlternativeSelectedSuggestion that is bound to shift+tab
        let weak = self.me.clone();
        self.base.register(self.editor.on_key_down().listen(move |e| {
            if let Some(this) = weak.upgrade()
                && e.shift_key
                && !this.is_shift_key_pressed.get()
            {
                this.is_shift_key_pressed.set(true);
                this.update(this.is_active.get());
            }
        }));
        let weak = self.me.clone();
        self.base.register(self.editor.on_key_up().listen(move |e| {
            if let Some(this) = weak.upgrade()
                && e.shift_key
                && this.is_shift_key_pressed.get()
            {
                this.is_shift_key_pressed.set(false);
                this.update(this.is_active.get());
            }
        }));

        if let Some(suggest_controller) = SuggestController::get(&self.editor) {
            let weak = self.me.clone();
            let selector = SuggestItemSelector {
                priority: 100,
                select: Rc::new(move |_model: &Rc<TextModel>, pos: Position, suggest_items: &[CompletionItem]| -> i32 {
                    let Some(this) = weak.upgrade() else {
                        return -1;
                    };
                    let Some(suggest_controller) = SuggestController::get(&this.editor) else {
                        return -1;
                    };
                    this.select_item_to_preselect(&suggest_controller, pos, suggest_items)
                }),
            };
            self.base.register(self.widget_host.register_selector(selector));

            let weak = self.me.clone();
            self.base.register(suggest_controller.model.on_did_trigger().once().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.bind_to_suggest_widget();
                }
            }));

            let weak = self.me.clone();
            self.base.register(suggest_controller.on_will_insert_suggest_item().listen(move |e: &WillInsertSuggestItem| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                let Some(suggest_controller) = SuggestController::get(&this.editor) else {
                    return;
                };
                let (Some(position), Some(model)) = (this.editor.get_position(), this.editor.get_model()) else {
                    return;
                };

                let suggest_item_info =
                    SuggestItemInfo::from_suggestion(&suggest_controller, &model, position, &e.item, this.is_shift_key_pressed.get());

                (this.on_will_accept)(&suggest_item_info);
            }));
        }
        self.update(self.is_active.get());
    }

    /// O `select` do seletor: o item da lista que mais completa a sugestão em linha pré-selecionada, ou `-1`.
    fn select_item_to_preselect(&self, suggest_controller: &SuggestController, pos: Position, suggest_items: &[CompletionItem]) -> i32 {
        let Some(text_model) = self.editor.get_model() else {
            // Should not happen
            return -1;
        };

        let preselected = (self.suggest_controller_preselector)();
        let item_to_preselect = preselected.map(|edit| single_text_remove_common_prefix(&edit, &text_model, None));
        let Some(item_to_preselect) = item_to_preselect else {
            return -1;
        };

        let candidates: Vec<PreselectCandidate> = suggest_items
            .iter()
            .enumerate()
            .filter_map(|(index, suggest_item)| {
                let suggest_item_info =
                    SuggestItemInfo::from_suggestion(suggest_controller, &text_model, pos, suggest_item, self.is_shift_key_pressed.get());
                let suggest_item_text_edit =
                    single_text_remove_common_prefix(&suggest_item_info.get_single_text_edit(), &text_model, None);
                let valid = single_text_edit_augments(&item_to_preselect, &suggest_item_text_edit);
                let prefix_length = suggest_item_text_edit.text.units().len();
                (valid && prefix_length > 0).then_some(PreselectCandidate { index, prefix_length })
            })
            .collect();

        let result = find_first_max(&candidates, compare_by(|candidate: &PreselectCandidate| candidate.prefix_length as f64, number_comparator));
        result.map_or(-1, |candidate| candidate.index as i32)
    }

    /// `bindToSuggestWidget()`: liga os eventos do widget na primeira vez que o controller dispara uma sugestão.
    fn bind_to_suggest_widget(&self) {
        if self.is_bound_to_suggest_widget.replace(true) {
            return;
        }

        let widget = self.widget_host.widget();
        self.listen_widget_visibility(widget.on_did_show(), true);
        self.listen_widget_visibility(widget.on_did_hide(), false);
        self.listen_widget_visibility(widget.on_did_focus(), true);
    }

    /// Os três ouvintes de `bindToSuggestWidget`: `isSuggestWidgetVisible = visible; update(visible)`.
    fn listen_widget_visibility<T: 'static>(&self, event: Event<T>, visible: bool) {
        let weak = self.me.clone();
        self.base.register(event.listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.is_suggest_widget_visible.set(visible);
                this.update(visible);
            }
        }));
    }

    fn update(&self, new_active: bool) {
        let new_inline_completion = self.get_suggest_item_info();

        let changed = self.is_active.get() != new_active || !suggest_item_info_equals(&self.current_suggest_item_info.borrow(), &new_inline_completion);
        if changed {
            self.is_active.set(new_active);
            *self.current_suggest_item_info.borrow_mut() = new_inline_completion;

            self.on_did_selected_item_change.fire(());
        }
    }

    fn get_suggest_item_info(&self) -> Option<Rc<SuggestItemInfo>> {
        let suggest_controller = SuggestController::get(&self.editor)?;
        if !self.is_suggest_widget_visible.get() {
            return None;
        }

        let focused_item = self.widget_host.widget().get_focused_item();
        let position = self.editor.get_position();
        let model = self.editor.get_model();

        let (focused_item, position, model) = (focused_item?, position?, model?);

        Some(Rc::new(SuggestItemInfo::from_suggestion(
            &suggest_controller,
            &model,
            position,
            &focused_item.item,
            self.is_shift_key_pressed.get(),
        )))
    }

    /// `stopForceRenderingAbove()`: só age se o editor tem o `SuggestController`.
    pub fn stop_force_rendering_above(&self) {
        if SuggestController::get(&self.editor).is_some() {
            self.widget_host.stop_force_rendering_above();
        }
    }

    /// `forceRenderingAbove()`: só age se o editor tem o `SuggestController`.
    pub fn force_rendering_above(&self) {
        if SuggestController::get(&self.editor).is_some() {
            self.widget_host.force_rendering_above();
        }
    }
}

impl IDisposable for SuggestWidgetAdaptor {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `ObservableSuggestWidgetAdapter`.
pub struct ObservableSuggestWidgetAdapter {
    base: Rc<Disposable>,
    /// `_suggestWidgetAdaptor`: público porque o `stopForceRenderingAbove` e o `forceRenderingAbove` do original só o
    /// repassam.
    pub suggest_widget_adaptor: Rc<SuggestWidgetAdaptor>,
    /// `selectedItem`.
    pub selected_item: IObservable<Option<Rc<SuggestItemInfo>>>,
}

impl ObservableSuggestWidgetAdapter {
    pub fn new(
        editor_obs: &Rc<ObservableCodeEditor>,
        widget_host: Rc<dyn SuggestWidgetHost>,
        handle_suggest_accepted: Rc<dyn Fn(&SuggestItemInfo)>,
        suggest_controller_preselector: Rc<dyn Fn() -> Option<TextReplacement>>,
    ) -> Rc<Self> {
        let base = Disposable::new();

        let preselector: Rc<dyn Fn() -> Option<TextReplacement>> = {
            let editor_obs = Rc::clone(editor_obs);
            Rc::new(move || {
                editor_obs.force_update();
                suggest_controller_preselector()
            })
        };
        let on_will_accept: Rc<dyn Fn(&SuggestItemInfo)> = {
            let editor_obs = Rc::clone(editor_obs);
            Rc::new(move |item: &SuggestItemInfo| {
                // @description InlineCompletionsController.handleSuggestAccepted
                editor_obs.force_update_with(|_tx| handle_suggest_accepted(item));
            })
        };
        let suggest_widget_adaptor = SuggestWidgetAdaptor::new(&editor_obs.editor, widget_host, preselector, on_will_accept);
        base.register(suggest_widget_adaptor.clone());

        let selected_item_changed = {
            let (adaptor, editor_obs) = (Rc::clone(&suggest_widget_adaptor), Rc::clone(editor_obs));
            Event::<()>::new(move |listener: Listener<()>| {
                let editor_obs = Rc::clone(&editor_obs);
                adaptor.on_did_selected_item_change().listen(move |_| {
                    editor_obs.force_update_with(|_tx| listener(&()));
                })
            })
        };
        let selected_item = {
            let adaptor = Rc::clone(&suggest_widget_adaptor);
            observable_from_event(selected_item_changed, move |_| adaptor.selected_item())
        };

        Rc::new(Self { base, suggest_widget_adaptor, selected_item })
    }
}

impl IDisposable for ObservableSuggestWidgetAdapter {
    fn dispose(&self) {
        self.base.dispose();
    }
}
