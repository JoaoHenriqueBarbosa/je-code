//! Porte de `src/vs/editor/common/languages/modesRegistry.ts`: o registro global das linguagens
//! (`ModesRegistry`), com a linguagem `plaintext` já registrada.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::DisposableRef;
use je_base::mime::mimes as Mimes;
use je_base::uri::URI;

/// `PLAINTEXT_LANGUAGE_ID`.
pub const PLAINTEXT_LANGUAGE_ID: &str = "plaintext";
/// `PLAINTEXT_EXTENSION`.
pub const PLAINTEXT_EXTENSION: &str = ".txt";

/// `ILanguageIcon`.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageIcon {
    pub light: URI,
    pub dark: URI,
}

/// `ILanguageExtensionPoint`.
#[derive(Clone, Debug, Default)]
pub struct LanguageExtensionPoint {
    pub id: String,
    pub extensions: Option<Vec<String>>,
    pub filenames: Option<Vec<String>>,
    pub filename_patterns: Option<Vec<String>>,
    pub first_line: Option<String>,
    pub aliases: Option<Vec<String>>,
    pub mimetypes: Option<Vec<String>>,
    pub configuration: Option<URI>,
    pub icon: Option<LanguageIcon>,
}

impl LanguageExtensionPoint {
    pub fn new(id: &str) -> Self {
        Self { id: id.to_owned(), ..Self::default() }
    }
}

/// `EditorModesRegistry`.
pub struct EditorModesRegistry {
    languages: RefCell<Vec<(u64, LanguageExtensionPoint)>>,
    next_id: Cell<u64>,
    on_did_change_languages: Rc<Emitter<()>>,
}

impl EditorModesRegistry {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { languages: RefCell::new(Vec::new()), next_id: Cell::new(0), on_did_change_languages: Emitter::new() })
    }

    pub fn on_did_change_languages(&self) -> Event<()> {
        self.on_did_change_languages.event()
    }

    pub fn register_language(self: &Rc<Self>, def: LanguageExtensionPoint) -> DisposableRef {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.languages.borrow_mut().push((id, def));
        self.on_did_change_languages.fire(());
        let weak = Rc::downgrade(self);
        // Objeto literal `{ dispose() { ... } }` no original: não é rastreado.
        je_base::lifecycle::plain_disposable(move || {
            if let Some(registry) = weak.upgrade() {
                let mut languages = registry.languages.borrow_mut();
                if let Some(index) = languages.iter().position(|(i, _)| *i == id) {
                    languages.remove(index);
                }
            }
        })
    }

    pub fn get_languages(&self) -> Vec<LanguageExtensionPoint> {
        self.languages.borrow().iter().map(|(_, l)| l.clone()).collect()
    }
}

thread_local! {
    static MODES_REGISTRY: Rc<EditorModesRegistry> = {
        let registry = EditorModesRegistry::new();
        // `ModesRegistry.registerLanguage({ id: PLAINTEXT_LANGUAGE_ID, ... })`: a primeira linguagem.
        let _ = registry.register_language(LanguageExtensionPoint {
            id: PLAINTEXT_LANGUAGE_ID.to_owned(),
            extensions: Some(vec![PLAINTEXT_EXTENSION.to_owned()]),
            aliases: Some(vec!["Plain Text".to_owned(), "text".to_owned()]),
            mimetypes: Some(vec![Mimes::TEXT.to_owned()]),
            ..LanguageExtensionPoint::default()
        });
        registry
    };
}

/// `ModesRegistry`.
pub fn modes_registry() -> Rc<EditorModesRegistry> {
    MODES_REGISTRY.with(Rc::clone)
}

