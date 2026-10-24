//! Porte de `src/vs/editor/contrib/suggest/browser/wordContextKey.ts`: a chave de contexto `atEndOfWord`
//! (`WordContextKey`), verdadeira quando o cursor está no fim de uma palavra e as completações por Tab estão ligadas.
//!
//! Representação:
//! - `static readonly AtEnd` é o `thread_local` lido por [`WordContextKey::at_end`] (o `RawContextKey` guarda o
//!   nome, o valor padrão e a expressão `Defined`; `bindTo(service)` é o do `RawContextKey`). O `{ type, description }`
//!   é o [`ContextKeyMeta::Info`].
//! - `@IContextKeyService` do construtor é o argumento `context_key_service`. O editor é o `Rc<CodeEditorWidget>`
//!   (`ICodeEditor` não é objeto de trait, tem métodos genéricos) e os ouvintes do editor guardam só um [`Weak`] do
//!   que precisam, porque o editor é dono deles e um `Rc` formaria um ciclo.
//! - `WordContextKey` é criada por [`WordContextKey::new`], que devolve `Rc<Self>` (o ouvinte da configuração guarda um
//!   [`Weak`] dela). `e.hasChanged(EditorOption.tabCompletion) && this._update()` é o `if` do ouvinte.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::nls::localize;
use je_platform::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::EditorOption;
use crate::config::editor_options::registry::TAB_COMPLETION;

thread_local! {
    static AT_END: RawContextKey = RawContextKey::new(
        "atEndOfWord",
        false,
        ContextKeyMeta::Info {
            ty: "boolean".to_string(),
            description: localize(
                "desc",
                "A context key that is true when at the end of a word. Note that this is only defined when tab-completions are enabled",
                &[],
            ),
        },
    );
}

/// `checkForWordEnd`: a chave vale verdadeiro quando o começo da seleção está no fim de uma palavra e a seleção
/// cabe numa linha.
fn check_for_word_end(editor: &CodeEditorWidget, ck_at_end: &dyn IContextKey) {
    if !editor.has_model() {
        ck_at_end.set(Dynamic::Bool(false));
        return;
    }
    let model = editor.get_model().expect("o editor tem modelo");
    let selection = editor.get_selection().expect("o editor com modelo tem seleção");
    let start = selection.get_start_position();
    let Some(word) = model.get_word_at_position(start) else {
        ck_at_end.set(Dynamic::Bool(false));
        return;
    };
    ck_at_end.set(Dynamic::Bool(
        word.end_column == start.column && selection.start_line_number == selection.end_line_number,
    ));
}

/// `WordContextKey`.
pub struct WordContextKey {
    editor: Rc<CodeEditorWidget>,
    ck_at_end: Rc<dyn IContextKey>,
    config_listener: DisposableRef,
    enabled: Cell<bool>,
    selection_listener: RefCell<Option<DisposableRef>>,
}

impl WordContextKey {
    /// `WordContextKey.AtEnd`.
    pub fn at_end() -> RawContextKey {
        AT_END.with(Clone::clone)
    }

    pub fn new(editor: &Rc<CodeEditorWidget>, context_key_service: &dyn IContextKeyService) -> Rc<Self> {
        let key = Rc::new_cyclic(|me: &Weak<WordContextKey>| {
            let weak = me.clone();
            let config_listener = editor.on_did_change_configuration().listen(move |e| {
                if e.has_changed(EditorOption::TabCompletion)
                    && let Some(key) = weak.upgrade()
                {
                    key.update();
                }
            });
            Self {
                editor: Rc::clone(editor),
                ck_at_end: Self::at_end().bind_to(context_key_service),
                config_listener,
                enabled: Cell::new(false),
                selection_listener: RefCell::new(None),
            }
        });
        key.update();
        key
    }

    fn update(&self) {
        // only update this when tab completions are enabled
        let enabled = self.editor.get_option(TAB_COMPLETION) == "on";
        if self.enabled.get() == enabled {
            return;
        }
        self.enabled.set(enabled);

        if enabled {
            let weak_editor = Rc::downgrade(&self.editor);
            let ck_at_end = Rc::clone(&self.ck_at_end);
            let listener = self.editor.on_did_change_cursor_selection().listen(move |_| {
                if let Some(editor) = weak_editor.upgrade() {
                    check_for_word_end(&editor, &*ck_at_end);
                }
            });
            *self.selection_listener.borrow_mut() = Some(listener);
            check_for_word_end(&self.editor, &*self.ck_at_end);
        } else {
            let listener = self.selection_listener.borrow_mut().take();
            if let Some(listener) = listener {
                self.ck_at_end.reset();
                listener.dispose();
            }
        }
    }
}

impl IDisposable for WordContextKey {
    fn dispose(&self) {
        self.config_listener.dispose();
        let listener = self.selection_listener.borrow_mut().take();
        if let Some(listener) = listener {
            listener.dispose();
        }
        self.ck_at_end.reset();
    }
}
