//! Porte de `src/vs/editor/contrib/gotoSymbol/browser/symbolNavigation.ts`: a navegação pelo teclado entre os resultados
//! de "ir para definição/referências" (`ISymbolNavigationService`), a chave de contexto `hasSymbols` e os comandos
//! `editor.gotoNextSymbolFromResult` (F12) e `editor.gotoNextSymbolFromResult.cancel` (Escape).
//!
//! Divergências:
//! - **`ICodeEditorService`.** É o [`ICodeEditorService`] de `opener_service` (`onCodeEditorAdd`, `onCodeEditorRemove`,
//!   `listCodeEditors`, `getActiveCodeEditor` e `openCodeEditor`), com o id `I_CODE_EDITOR_SERVICE`. O
//!   `openCodeEditor({ resource, options: { selection, selectionRevealType } }, source)` do original monta o
//!   [`TextResourceEditorInput`] com a seleção do início da referência e o
//!   `TextEditorSelectionRevealType.NearTopIfOutsideViewport`.
//! - **`revealNext`** devolve `Promise<()>` em vez de `Promise<any>`: o único consumidor (o comando F12) descarta o valor.
//! - **`registerSingleton(ISymbolNavigationService, SymbolNavigationService, InstantiationType.Delayed)`**, o
//!   `registerEditorCommand` e o `KeybindingsRegistry.registerCommandAndKeybindingRule` do carregamento do módulo são
//!   [`register`], chamada por quem monta o workbench (e pelos testes), idempotente por thread, como nos demais
//!   serviços do porte. O proxy da instanciação atrasada é [`SymbolNavigationServiceProxy`].
//! - **`put` não descarta o estado anterior.** `this._currentState = combinedDisposable(...)` do original sobrescreve sem
//!   descartar, e o porte preserva isso (só `reset` descarta).
//! - **`Map<ICodeEditor, IDisposable>`** do `EditorState` é um `HashMap` pelo `getId()` do editor, e os ouvintes seguram o
//!   editor por `Weak` (o coletor de lixo do original desfaz o ciclo editor, ouvinte, editor).
//! - `kb.getLabel()` nulo entra na mensagem como o texto `null`, como a interpolação do original.
//!
//! A fatia seguinte do módulo, `goToCommands.ts` (`DefinitionAction` e companhia), é [`super::go_to_commands`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, combined_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_instantiation::{Injectable, InstantiationType, ServicesAccessor, register_singleton, service_id, service_impl};
use je_platform::contextkey::{IContextKey, IContextKeyService, RawContextKey};
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::keybinding::keybindings_registry::{
    CommandAndKeybindingRule, KeybindingRule, KeybindingWeight, Keybindings, keybindings_registry,
};
use je_platform::notification::{INotificationService, NotificationMessage, StatusHandle};
use je_platform::opener::editor_types::TextEditorSelectionRevealType;

use super::references_model::{OneReference, ReferencesModel, ext_uri};
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor, IEditorCommand};
use crate::editor_extensions::{Command, CommandKeybindingsOptions, CommandOptions, EditorCommand, register_editor_command};
use crate::opener_service::{ICodeEditorService, TextEditorOptions, TextResourceEditorInput, range_to_text_editor_selection};

thread_local! {
    static CTX_HAS_SYMBOLS: RawContextKey = RawContextKey::new(
        "hasSymbols",
        false,
        localize("hasSymbols", "Whether there are symbol locations that can be navigated via keyboard-only.", &[]),
    );
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// `ctxHasSymbols`.
pub fn ctx_has_symbols() -> RawContextKey {
    CTX_HAS_SYMBOLS.with(RawContextKey::clone)
}

const GOTO_NEXT_SYMBOL_FROM_RESULT: &str = "editor.gotoNextSymbolFromResult";

/// `ISymbolNavigationService`.
pub trait ISymbolNavigationService {
    fn reset(&self);
    fn put(&self, anchor: &Rc<OneReference>);
    fn reveal_next(&self, source: &Rc<CodeEditorWidget>) -> Promise<()>;
}

je_instantiation::delayed_proxy!(pub SymbolNavigationServiceProxy for dyn ISymbolNavigationService {
    methods {
        fn reset(&self);
        fn put(&self, anchor: &Rc<OneReference>);
        fn reveal_next(&self, source: &Rc<CodeEditorWidget>) -> Promise<()>;
    }
    events {}
});

service_id!(pub I_SYMBOL_NAVIGATION_SERVICE: dyn ISymbolNavigationService = "ISymbolNavigationService", proxy SymbolNavigationServiceProxy);

/// `SymbolNavigationService`.
pub struct SymbolNavigationService {
    me: Weak<SymbolNavigationService>,
    ctx_has_symbols: Rc<dyn IContextKey>,
    editor_service: Rc<dyn ICodeEditorService>,
    notification_service: Rc<dyn INotificationService>,
    keybinding_service: Rc<dyn IKeybindingService>,
    current_model: RefCell<Option<Rc<ReferencesModel>>>,
    current_idx: Cell<isize>,
    current_state: RefCell<Option<DisposableRef>>,
    current_message: RefCell<Option<Rc<dyn StatusHandle>>>,
    ignore_editor_change: Cell<bool>,
}

impl Injectable for SymbolNavigationService {
    type Args = ();
    type Deps = (Rc<dyn IContextKeyService>, Rc<dyn ICodeEditorService>, Rc<dyn INotificationService>, Rc<dyn IKeybindingService>);

    fn create(_: (), (context_key_service, editor_service, notification_service, keybinding_service): Self::Deps) -> Rc<Self> {
        let ctx_has_symbols = ctx_has_symbols().bind_to(&*context_key_service);
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            ctx_has_symbols,
            editor_service,
            notification_service,
            keybinding_service,
            current_model: RefCell::new(None),
            current_idx: Cell::new(-1),
            current_state: RefCell::new(None),
            current_message: RefCell::new(None),
            ignore_editor_change: Cell::new(false),
        })
    }
}

service_impl!(SymbolNavigationService => dyn ISymbolNavigationService);

impl SymbolNavigationService {
    /// `_showMessage()`.
    fn show_message(&self) {
        let previous = self.current_message.borrow_mut().take();
        if let Some(previous) = previous {
            previous.close();
        }

        let total = self.current_model.borrow().as_ref().map_or(0, |model| model.references().len());
        let position = (self.current_idx.get() + 1).to_string();
        let total = total.to_string();
        let message = match self.keybinding_service.lookup_keybinding(GOTO_NEXT_SYMBOL_FROM_RESULT, None, false) {
            Some(kb) => {
                let label = kb.get_label().unwrap_or_else(|| "null".to_string());
                localize("location.kb", "Symbol {0} of {1}, {2} for next", &[position.as_str(), total.as_str(), label.as_str()])
            }
            None => localize("location", "Symbol {0} of {1}", &[position.as_str(), total.as_str()]),
        };

        let handle = self.notification_service.status(NotificationMessage::Text(message), None);
        *self.current_message.borrow_mut() = Some(handle);
    }
}

impl ISymbolNavigationService for SymbolNavigationService {
    fn reset(&self) {
        self.ctx_has_symbols.reset();
        let state = self.current_state.borrow_mut().take();
        if let Some(state) = state {
            state.dispose();
        }
        let message = self.current_message.borrow_mut().take();
        if let Some(message) = message {
            message.close();
        }
        *self.current_model.borrow_mut() = None;
        self.current_idx.set(-1);
    }

    fn put(&self, anchor: &Rc<OneReference>) {
        let ref_model = anchor.parent().parent();

        if ref_model.references().len() <= 1 {
            self.reset();
            return;
        }

        *self.current_model.borrow_mut() = Some(Rc::clone(&ref_model));
        self.current_idx.set(ref_model.references().iter().position(|reference| Rc::ptr_eq(reference, anchor)).map_or(-1, |i| i as isize));
        self.ctx_has_symbols.set(Dynamic::Bool(true));
        self.show_message();

        let editor_state = EditorState::new(&self.editor_service);
        let me = self.me.clone();
        let listener = editor_state.on_did_change().listen(move |_| {
            let Some(this) = me.upgrade() else {
                return;
            };
            if this.ignore_editor_change.get() {
                return;
            }

            let Some(editor) = this.editor_service.get_active_code_editor() else {
                return;
            };
            let (Some(model), Some(position)) = (editor.get_model(), editor.get_position()) else {
                return;
            };

            let ext_uri = ext_uri();
            let model_uri = model.uri();
            let mut seen_uri = false;
            let mut seen_position = false;
            for reference in ref_model.references() {
                if ext_uri.is_equal(Some(reference.uri()), Some(&model_uri), false) {
                    seen_uri = true;
                    seen_position = seen_position || reference.range().contains_position(&position);
                } else if seen_uri {
                    break;
                }
            }
            if !seen_uri || !seen_position {
                this.reset();
            }
        });

        let editor_state: DisposableRef = editor_state;
        *self.current_state.borrow_mut() = Some(combined_disposable(vec![editor_state, listener]));
    }

    fn reveal_next(&self, source: &Rc<CodeEditorWidget>) -> Promise<()> {
        let Some(model) = self.current_model.borrow().clone() else {
            return Promise::resolved(());
        };

        // get next result and advance
        let idx = (self.current_idx.get() + 1) % model.references().len() as isize;
        self.current_idx.set(idx);
        let reference = Rc::clone(&model.references()[idx as usize]);

        // status
        self.show_message();

        // open editor, ignore events while that happens
        self.ignore_editor_change.set(true);
        let me = self.me.clone();
        let input = TextResourceEditorInput {
            resource: reference.uri().clone(),
            options: Some(TextEditorOptions {
                selection: Some(range_to_text_editor_selection(&reference.range().collapse_to_start())),
                selection_reveal_type: Some(TextEditorSelectionRevealType::NearTopIfOutsideViewport),
                ..TextEditorOptions::default()
            }),
        };
        self.editor_service
            .open_code_editor(input, Some(Rc::clone(source)), None)
            .finally(move || {
                if let Some(this) = me.upgrade() {
                    this.ignore_editor_change.set(false);
                }
            })
            .then(|_| ())
    }
}

/// `registerSingleton(...)`, `registerEditorCommand(...)` e `KeybindingsRegistry.registerCommandAndKeybindingRule(...)`
/// do módulo (ver "Divergências").
pub fn register() {
    if REGISTERED.with(|registered| registered.replace(true)) {
        return;
    }

    register_singleton::<SymbolNavigationService>(I_SYMBOL_NAVIGATION_SERVICE, InstantiationType::Delayed);

    register_editor_command(Rc::new(GotoNextSymbolFromResultCommand {
        base: Command::new(CommandOptions {
            precondition: Some(ctx_has_symbols().expr()),
            kb_opts: vec![CommandKeybindingsOptions::new(
                KeybindingWeight::EDITOR_CONTRIB,
                None,
                Keybindings { primary: Some(KeyCode::F12.bits()), ..Keybindings::default() },
            )],
            ..CommandOptions::new(GOTO_NEXT_SYMBOL_FROM_RESULT)
        }),
    }));

    keybindings_registry().register_command_and_keybinding_rule(CommandAndKeybindingRule {
        rule: KeybindingRule {
            keybindings: Keybindings { primary: Some(KeyCode::Escape.bits()), ..Keybindings::default() },
            id: format!("{GOTO_NEXT_SYMBOL_FROM_RESULT}.cancel"),
            weight: KeybindingWeight::EDITOR_CONTRIB,
            args: Dynamic::Undefined,
            when: Some(ctx_has_symbols().expr()),
        },
        handler: Rc::new(|accessor: &ServicesAccessor, _args: &[Dynamic]| {
            accessor.get(I_SYMBOL_NAVIGATION_SERVICE).reset();
            Dynamic::Undefined
        }),
        metadata: None,
    });
}

/// O `registerEditorCommand(new class extends EditorCommand { ... })` do `editor.gotoNextSymbolFromResult`.
struct GotoNextSymbolFromResultCommand {
    base: Command,
}

impl IEditorCommand for GotoNextSymbolFromResultCommand {
    /// `runEditorCommand(accessor, editor)`: `accessor.get(ISymbolNavigationService).revealNext(editor)`.
    fn run_editor_command(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        match accessor {
            Some(accessor) => accessor.get(I_SYMBOL_NAVIGATION_SERVICE).reveal_next(editor),
            None => Promise::rejected(Error::named("TypeError", "o comando precisa do ServicesAccessor")),
        }
    }
}

impl EditorCommand for GotoNextSymbolFromResultCommand {
    fn base(&self) -> &Command {
        &self.base
    }
}

//

type Listeners = RefCell<HashMap<String, DisposableRef>>;

/// `EditorState`: dispara quando o cursor ou o conteúdo de qualquer editor muda.
struct EditorState {
    listeners: Rc<Listeners>,
    disposables: Rc<DisposableStore>,
    on_did_change: Rc<Emitter<Rc<CodeEditorWidget>>>,
}

impl EditorState {
    fn new(editor_service: &Rc<dyn ICodeEditorService>) -> Rc<Self> {
        let listeners: Rc<Listeners> = Rc::new(RefCell::new(HashMap::new()));
        let on_did_change: Rc<Emitter<Rc<CodeEditorWidget>>> = Emitter::new();
        let disposables = DisposableStore::new();

        let remove_from = Rc::clone(&listeners);
        disposables.add(editor_service.on_code_editor_remove().listen(move |editor| Self::on_did_remove_editor(&remove_from, editor)));
        let (add_to, add_emitter) = (Rc::clone(&listeners), Rc::clone(&on_did_change));
        disposables.add(editor_service.on_code_editor_add().listen(move |editor| Self::on_did_add_editor(&add_to, &add_emitter, editor)));
        for editor in editor_service.list_code_editors() {
            Self::on_did_add_editor(&listeners, &on_did_change, &editor);
        }

        Rc::new(Self { listeners, disposables, on_did_change })
    }

    /// `onDidChange`.
    fn on_did_change(&self) -> Event<Rc<CodeEditorWidget>> {
        self.on_did_change.event()
    }

    /// `_onDidAddEditor(editor)`.
    fn on_did_add_editor(listeners: &Listeners, on_did_change: &Rc<Emitter<Rc<CodeEditorWidget>>>, editor: &Rc<CodeEditorWidget>) {
        let (emitter, target) = (Rc::clone(on_did_change), Rc::downgrade(editor));
        let on_cursor = editor.on_did_change_cursor_position().listen(move |_| {
            if let Some(editor) = target.upgrade() {
                emitter.fire(editor);
            }
        });
        let (emitter, target) = (Rc::clone(on_did_change), Rc::downgrade(editor));
        let on_content = editor.on_did_change_model_content().listen(move |_| {
            if let Some(editor) = target.upgrade() {
                emitter.fire(editor);
            }
        });
        listeners.borrow_mut().insert(editor.get_id(), combined_disposable(vec![on_cursor, on_content]));
    }

    /// `_onDidRemoveEditor(editor)`.
    fn on_did_remove_editor(listeners: &Listeners, editor: &Rc<CodeEditorWidget>) {
        let removed = listeners.borrow_mut().remove(&editor.get_id());
        if let Some(removed) = removed {
            removed.dispose();
        }
    }
}

impl IDisposable for EditorState {
    fn dispose(&self) {
        self.disposables.dispose();
        self.on_did_change.dispose();
        let listeners: Vec<DisposableRef> = self.listeners.borrow().values().cloned().collect();
        for listener in &listeners {
            listener.dispose();
        }
    }
}
