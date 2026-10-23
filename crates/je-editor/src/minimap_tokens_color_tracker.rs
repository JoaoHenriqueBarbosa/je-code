//! Porte de `src/vs/editor/common/viewModel/minimapTokensColorTracker.ts`: `MinimapTokensColorTracker`, o
//! singleton que traduz o mapa de cores do registro de tokenização para `RGBA8` e diz se o fundo é claro.
//!
//! Divergências:
//! - `Disposable` (a classe base) vira a composição de um `je_base::lifecycle::Disposable`; `_register`
//!   vira `register` nele. O singleton é um `thread_local` (um por thread de UI, como o
//!   `tokenization_registry()`), e `markAsSingleton` marca o `Disposable` interno, que é o objeto que o
//!   rastreador de vazamento conhece.
//! - O mapa de cores do registro guarda texto CSS (ver `tokenization_registry.rs`); cada entrada passa por
//!   `Color::from_hex` (`je_theme`) para virar o `Color` que o original lê. A posição 0 do mapa não é lida,
//!   como no original.
//! - `onUnexpectedError(new BugIndicatingError(...))` vira `errors::on_unexpected_error` com
//!   `Error::bug_indicating`.
//! - O construtor é privado (só `getInstance` cria) e devolve `Rc<Self>`, porque o ouvinte do registro guarda
//!   uma referência fraca ao objeto.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, mark_as_singleton};
use je_theme::color::Color;

use crate::encoded_token_attributes::color_id;
use crate::rgba::Rgba8;
use crate::tokenization_registry::tokenization_registry;

thread_local! {
    static INSTANCE: RefCell<Option<Rc<MinimapTokensColorTracker>>> = const { RefCell::new(None) };
}

struct ColorState {
    colors: Vec<Rgba8>,
    background_is_light: bool,
}

/// `MinimapTokensColorTracker`.
pub struct MinimapTokensColorTracker {
    base: Rc<Disposable>,
    state: RefCell<ColorState>,
    on_did_change: Rc<Emitter<()>>,
}

impl MinimapTokensColorTracker {
    /// `getInstance()`.
    pub fn get_instance() -> Rc<MinimapTokensColorTracker> {
        if let Some(instance) = INSTANCE.with(|i| i.borrow().clone()) {
            return instance;
        }
        let instance = Self::new();
        mark_as_singleton(instance.base.clone());
        INSTANCE.with(|i| *i.borrow_mut() = Some(instance.clone()));
        instance
    }

    fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone() as DisposableRef);
        let this = Rc::new(Self {
            base,
            state: RefCell::new(ColorState { colors: Vec::new(), background_is_light: true }),
            on_did_change,
        });
        this.update_color_map();
        let weak = Rc::downgrade(&this);
        let listener = tokenization_registry().on_did_change().listen(move |e| {
            if e.changed_color_map
                && let Some(this) = weak.upgrade()
            {
                this.update_color_map();
            }
        });
        this.base.register(listener);
        this
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn update_color_map(&self) {
        let Some(color_map) = tokenization_registry().get_color_map() else {
            *self.state.borrow_mut() =
                ColorState { colors: vec![Rgba8::EMPTY; color_id::DEFAULT_BACKGROUND as usize + 1], background_is_light: true };
            return;
        };
        let colors = std::iter::once(Rgba8::EMPTY)
            .chain(color_map.iter().skip(1).map(|css| {
                let source = Color::from_hex(css).rgba();
                // Usa um tipo amigável à VM.
                Rgba8::new(f64::from(source.r), f64::from(source.g), f64::from(source.b), (source.a * 255.0).round())
            }))
            .collect();
        let background_luminosity = Color::from_hex(&color_map[color_id::DEFAULT_BACKGROUND as usize]).get_relative_luminance();
        *self.state.borrow_mut() = ColorState { colors, background_is_light: background_luminosity >= 0.5 };
        self.on_did_change.fire(());
    }

    pub fn get_color(&self, id: u32) -> Rgba8 {
        let state = self.state.borrow();
        let id = if id < 1 || id as usize >= state.colors.len() {
            // cor de fundo (basicamente invisível)
            color_id::DEFAULT_BACKGROUND
        } else {
            id
        };
        match state.colors.get(id as usize) {
            Some(color) => *color,
            None => {
                errors::on_unexpected_error(&Error::bug_indicating(Some(&format!("Missing color for colorId {id}"))));
                Rgba8::EMPTY
            }
        }
    }

    pub fn background_is_light(&self) -> bool {
        self.state.borrow().background_is_light
    }
}

impl IDisposable for MinimapTokensColorTracker {
    fn dispose(&self) {
        self.base.dispose();
    }
}
