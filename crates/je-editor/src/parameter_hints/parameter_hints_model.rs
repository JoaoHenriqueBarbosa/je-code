//! Porte de `src/vs/editor/contrib/parameterHints/browser/parameterHintsModel.ts`: o modelo das dicas de parâmetro
//! (`ParameterHintsModel`), que escuta o editor, dispara (com atraso e junção de gatilhos) o pedido de ajuda de
//! assinatura aos provedores e guarda o estado (`Default`, `Pending`, `Active`).
//!
//! Representação:
//! - `ParameterHintsModel extends Disposable` é a struct [`ParameterHintsModel`] (criada por
//!   [`ParameterHintsModel::new`], que devolve `Rc<Self>`: os ouvintes do editor guardam um [`Weak`] do modelo, e o
//!   editor é dono deles). O `this._register(...)` é o `DisposableStore` do modelo, e `onChangedHints` é o
//!   [`ParameterHintsModel::on_changed_hints`].
//! - `namespace ParameterHintState` é o enum [`ParameterHintState`]; o `Pending` guarda a `CancelablePromise` do
//!   pedido e as dicas que estavam ativas, e o `Active` as dicas (`Rc<SignatureHelp>`: o objeto compartilhado do
//!   original, de identidade observável pelo provedor). O setter `state` cancela o pedido do `Pending` que sai.
//! - O `delay` do construtor é opcional (padrão 120 ms) e o `delay` de `trigger` é opcional, em milissegundos.
//! - `async doTrigger` é [`async_fn`]: o corpo roda já, de forma síncrona, até o primeiro `await`, de modo que o
//!   provedor é chamado (dentro do `createCancelablePromise`) antes de o estado virar `Pending`, como no original.
//! - `_lastSignatureHelpResult` é o `MutableDisposable<SignatureHelpResult>` de `je-base` e guarda o `Rc` do resultado.
//! - `CharacterSet` guarda as unidades UTF-16 (`ch.charCodeAt(0)` e `text.charCodeAt(n)`).
//! - O `.catch(onUnexpectedError)` do `throttledDelayer.trigger` é o `then_result` que repassa o erro a
//!   `on_unexpected_error` (que ignora o cancelamento).
//! - `ICodeEditor` não é objeto de trait (tem métodos genéricos), então o editor é o `Rc<CodeEditorWidget>`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{Delayer, async_fn, create_cancelable_promise};
use je_base::errors::{Error, on_unexpected_error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable, MutableDisposable};
use je_base::promise::{CancelablePromise, MaybePromise, Promise};
use je_base::symbols::Delay;

use super::languages::{SignatureHelp, SignatureHelpContext, SignatureHelpProvider, SignatureHelpResult, SignatureHelpTriggerKind};
use super::provide_signature_help::provide_signature_help;
use crate::character_classifier::CharacterSet;
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::parameter_hints::PARAMETER_HINTS;
use crate::cursor_events::ICursorSelectionChangedEvent;
use crate::language_feature_registry::LanguageFeatureRegistry;

/// O atraso padrão, em milissegundos, do `throttledDelayer`.
const DEFAULT_DELAY: u64 = 120;

/// `TriggerContext`.
#[derive(Clone, Debug)]
pub struct TriggerContext {
    pub trigger_kind: SignatureHelpTriggerKind,
    pub trigger_character: Option<String>,
}

/// `ParameterHintState`: `Default`, `Active` e `Pending`.
enum ParameterHintState {
    Default,
    Active(Rc<SignatureHelp>),
    Pending {
        request: CancelablePromise<Option<Rc<SignatureHelpResult>>>,
        previously_active_hints: Option<Rc<SignatureHelp>>,
    },
}

/// O estado mutável do modelo.
struct ModelState {
    trigger_on_type: bool,
    state: ParameterHintState,
    pending_triggers: Vec<TriggerContext>,
    trigger_chars: CharacterSet,
    retrigger_chars: CharacterSet,
    trigger_id: u64,
}

/// `mergeTriggerContexts(previous, current)`: `Invoke` e o caractere de gatilho vencem o gatilho anterior; a
/// mudança de conteúdo é ignorada.
fn merge_trigger_contexts(previous: TriggerContext, current: TriggerContext) -> TriggerContext {
    match current.trigger_kind {
        // Invoke overrides previous triggers.
        SignatureHelpTriggerKind::Invoke => current,
        // Ignore content changes triggers
        SignatureHelpTriggerKind::ContentChange => previous,
        SignatureHelpTriggerKind::TriggerCharacter => current,
    }
}

/// `ParameterHintsModel`.
pub struct ParameterHintsModel {
    editor: Rc<CodeEditorWidget>,
    providers: Rc<LanguageFeatureRegistry<dyn SignatureHelpProvider>>,
    disposables: Rc<DisposableStore>,
    on_changed_hints: Rc<Emitter<Option<Rc<SignatureHelp>>>>,
    inner: RefCell<ModelState>,
    last_signature_help_result: Rc<MutableDisposable<SignatureHelpResult>>,
    throttled_delayer: Delayer<bool>,
    this: Weak<ParameterHintsModel>,
}

impl ParameterHintsModel {
    pub fn new(
        editor: &Rc<CodeEditorWidget>,
        providers: &Rc<LanguageFeatureRegistry<dyn SignatureHelpProvider>>,
        delay: Option<u64>,
    ) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let on_changed_hints = disposables.add_rc(Emitter::new());
        let last_signature_help_result = disposables.add_rc(MutableDisposable::new());
        let model = Rc::new_cyclic(|this| Self {
            editor: Rc::clone(editor),
            providers: Rc::clone(providers),
            disposables,
            on_changed_hints,
            inner: RefCell::new(ModelState {
                trigger_on_type: false,
                state: ParameterHintState::Default,
                pending_triggers: Vec::new(),
                trigger_chars: CharacterSet::new(),
                retrigger_chars: CharacterSet::new(),
                trigger_id: 0,
            }),
            last_signature_help_result,
            throttled_delayer: Delayer::new(Delay::Millis(delay.unwrap_or(DEFAULT_DELAY))),
            this: this.clone(),
        });

        Self::listen(&model, editor.on_did_blur_editor_widget(), |model, _| model.cancel(false));
        Self::listen(&model, editor.on_did_change_configuration(), |model, _| model.on_editor_configuration_change());
        Self::listen(&model, editor.on_did_change_model(), |model, _| model.on_model_changed());
        Self::listen(&model, editor.on_did_change_model_language(), |model, _| model.on_model_changed());
        Self::listen(&model, editor.on_did_change_cursor_selection(), |model, e| model.on_cursor_change(e));
        Self::listen(&model, editor.on_did_change_model_content(), |model, _| model.on_model_content_change());
        Self::listen(&model, providers.on_did_change(), |model, _| model.on_model_changed());
        Self::listen(&model, editor.on_did_type(), |model, text| model.on_did_type(text));

        model.on_editor_configuration_change();
        model.on_model_changed();
        model
    }

    /// `this._register(event(handler))`: o ouvinte guarda só um [`Weak`] do modelo.
    fn listen<E: 'static>(model: &Rc<Self>, event: Event<E>, handler: impl Fn(&Self, &E) + 'static) {
        let weak = Rc::downgrade(model);
        event.listen_in(
            move |e| {
                if let Some(model) = weak.upgrade() {
                    handler(&*model, e);
                }
            },
            &model.disposables,
        );
    }

    /// `onChangedHints`.
    pub fn on_changed_hints(&self) -> Event<Option<Rc<SignatureHelp>>> {
        self.on_changed_hints.event()
    }

    /// `set state(value)`: cancela o pedido do `Pending` que sai.
    fn set_state(&self, value: ParameterHintState) {
        let previous = std::mem::replace(&mut self.inner.borrow_mut().state, value);
        if let ParameterHintState::Pending { request, .. } = previous {
            request.cancel();
        }
    }

    pub fn cancel(&self, silent: bool) {
        self.set_state(ParameterHintState::Default);

        self.throttled_delayer.cancel();

        if !silent {
            self.on_changed_hints.fire(None);
        }
    }

    pub fn trigger(&self, context: TriggerContext, delay: Option<u64>) {
        let Some(model) = self.editor.get_model() else {
            return;
        };
        if !self.providers.has(&model) {
            return;
        }

        let trigger_id = {
            let mut inner = self.inner.borrow_mut();
            inner.trigger_id += 1;
            inner.pending_triggers.push(context);
            inner.trigger_id
        };
        let weak = self.this.clone();
        self.throttled_delayer
            .trigger(
                move || match weak.upgrade() {
                    Some(model) => MaybePromise::Promise(model.do_trigger(trigger_id)),
                    None => MaybePromise::Value(false),
                },
                delay.map(Delay::Millis),
            )
            .then_result(|result| {
                if let Err(error) = result {
                    on_unexpected_error(&error);
                }
                Ok(())
            });
    }

    pub fn next(&self) {
        let Some(hints) = self.active_hints() else {
            return;
        };

        let length = hints.signatures.len() as i32;
        let active_signature = hints.active_signature;
        let last = (active_signature % length) == (length - 1);
        let cycle = self.editor.get_option(PARAMETER_HINTS).cycle;

        // If there is only one signature, or we're on last signature of list
        if (length < 2 || last) && !cycle {
            self.cancel(false);
            return;
        }

        self.update_active_signature(if last && cycle { 0 } else { active_signature + 1 });
    }

    pub fn previous(&self) {
        let Some(hints) = self.active_hints() else {
            return;
        };

        let length = hints.signatures.len() as i32;
        let active_signature = hints.active_signature;
        let first = active_signature == 0;
        let cycle = self.editor.get_option(PARAMETER_HINTS).cycle;

        // If there is only one signature, or we're on first signature of list
        if (length < 2 || first) && !cycle {
            self.cancel(false);
            return;
        }

        self.update_active_signature(if first && cycle { length - 1 } else { active_signature - 1 });
    }

    /// As dicas do estado `Active`, se for o estado atual.
    fn active_hints(&self) -> Option<Rc<SignatureHelp>> {
        match &self.inner.borrow().state {
            ParameterHintState::Active(hints) => Some(Rc::clone(hints)),
            _ => None,
        }
    }

    fn update_active_signature(&self, active_signature: i32) {
        let Some(hints) = self.active_hints() else {
            return;
        };

        let hints = Rc::new(SignatureHelp { active_signature, ..(*hints).clone() });
        self.set_state(ParameterHintState::Active(Rc::clone(&hints)));
        self.on_changed_hints.fire(Some(hints));
    }

    fn do_trigger(self: &Rc<Self>, trigger_id: u64) -> Promise<bool> {
        let this = Rc::clone(self);
        async_fn(async move {
            let is_retrigger = matches!(this.inner.borrow().state, ParameterHintState::Active(_) | ParameterHintState::Pending { .. });
            let active_signature_help = this.get_last_active_hints();
            this.cancel(true);

            let pending_triggers = std::mem::take(&mut this.inner.borrow_mut().pending_triggers);
            let Some(context) = pending_triggers.into_iter().reduce(merge_trigger_contexts) else {
                return Ok(false);
            };

            let trigger_context = SignatureHelpContext {
                trigger_kind: context.trigger_kind,
                trigger_character: context.trigger_character,
                is_retrigger,
                active_signature_help: active_signature_help.clone(),
            };

            if !this.editor.has_model() {
                return Ok(false);
            }

            let model = this.editor.get_model().expect("o editor tem modelo");
            let position = this.editor.get_position().expect("o editor com modelo tem posição");

            let request = create_cancelable_promise(|token| {
                provide_signature_help(&this.providers, &model, position, trigger_context, &token)
            });
            this.set_state(ParameterHintState::Pending { request: request.clone(), previously_active_hints: active_signature_help });

            match request.await {
                Ok(result) => {
                    // Check that we are still resolving the correct signature help
                    if trigger_id != this.inner.borrow().trigger_id {
                        if let Some(result) = result {
                            result.dispose();
                        }

                        return Ok(false);
                    }

                    match result {
                        Some(result) if !result.value.signatures.is_empty() => {
                            this.set_state(ParameterHintState::Active(Rc::clone(&result.value)));
                            let hints = Rc::clone(&result.value);
                            this.last_signature_help_result.set(Some(result));
                            this.on_changed_hints.fire(Some(hints));
                            Ok(true)
                        }
                        result => {
                            if let Some(result) = result {
                                result.dispose();
                            }
                            this.last_signature_help_result.clear();
                            this.cancel(false);
                            Ok(false)
                        }
                    }
                }
                Err(error) => {
                    if trigger_id == this.inner.borrow().trigger_id {
                        this.set_state(ParameterHintState::Default);
                    }
                    on_unexpected_error(&error);
                    Ok::<bool, Error>(false)
                }
            }
        })
    }

    fn get_last_active_hints(&self) -> Option<Rc<SignatureHelp>> {
        match &self.inner.borrow().state {
            ParameterHintState::Active(hints) => Some(Rc::clone(hints)),
            ParameterHintState::Pending { previously_active_hints, .. } => previously_active_hints.clone(),
            ParameterHintState::Default => None,
        }
    }

    fn is_triggered(&self) -> bool {
        matches!(self.inner.borrow().state, ParameterHintState::Active(_) | ParameterHintState::Pending { .. })
            || self.throttled_delayer.is_triggered()
    }

    fn on_model_changed(&self) {
        self.cancel(false);

        {
            let mut inner = self.inner.borrow_mut();
            inner.trigger_chars.clear();
            inner.retrigger_chars.clear();
        }

        let Some(model) = self.editor.get_model() else {
            return;
        };

        let mut inner = self.inner.borrow_mut();
        for support in self.providers.ordered(&model, false) {
            for ch in support.signature_help_trigger_characters() {
                if let Some(char_code) = ch.encode_utf16().next() {
                    inner.trigger_chars.add(i32::from(char_code));

                    // All trigger characters are also considered retrigger characters
                    inner.retrigger_chars.add(i32::from(char_code));
                }
            }

            for ch in support.signature_help_retrigger_characters() {
                if let Some(char_code) = ch.encode_utf16().next() {
                    inner.retrigger_chars.add(i32::from(char_code));
                }
            }
        }
    }

    fn on_did_type(&self, text: &JsString) {
        if !self.inner.borrow().trigger_on_type {
            return;
        }

        let Some(&trigger_char_code) = text.units().last() else {
            return;
        };
        let trigger_char_code = i32::from(trigger_char_code);

        let triggers = {
            let inner = self.inner.borrow();
            inner.trigger_chars.has(trigger_char_code)
        } || (self.is_triggered() && self.inner.borrow().retrigger_chars.has(trigger_char_code));
        if triggers {
            self.trigger(
                TriggerContext {
                    trigger_kind: SignatureHelpTriggerKind::TriggerCharacter,
                    trigger_character: Some(String::from_utf16_lossy(&text.units()[text.len() - 1..])),
                },
                None,
            );
        }
    }

    fn on_cursor_change(&self, e: &ICursorSelectionChangedEvent) {
        if e.source == "mouse" {
            self.cancel(false);
        } else if self.is_triggered() {
            self.trigger(TriggerContext { trigger_kind: SignatureHelpTriggerKind::ContentChange, trigger_character: None }, None);
        }
    }

    fn on_model_content_change(&self) {
        if self.is_triggered() {
            self.trigger(TriggerContext { trigger_kind: SignatureHelpTriggerKind::ContentChange, trigger_character: None }, None);
        }
    }

    fn on_editor_configuration_change(&self) {
        let trigger_on_type = self.editor.get_option(PARAMETER_HINTS).enabled;
        self.inner.borrow_mut().trigger_on_type = trigger_on_type;

        if !trigger_on_type {
            self.cancel(false);
        }
    }
}

impl IDisposable for ParameterHintsModel {
    fn dispose(&self) {
        self.cancel(true);
        self.disposables.dispose();
    }
}
