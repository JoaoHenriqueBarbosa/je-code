//! Porte de `src/vs/editor/contrib/stickyScroll/browser/stickyScrollProvider.ts`: o provedor dos candidatos a linha
//! fixa (`StickyLineCandidateProvider`), que mantém o modelo de escopos do documento e responde quais linhas de início
//! de escopo cruzam um intervalo.
//!
//! Representação:
//! - `IStickyLineCandidateProvider` é o trait [`IStickyLineCandidateProvider`], com `dispose` herdado de
//!   [`IDisposable`]. Os métodos `getVersionId`, `update`, `getCandidateStickyLinesIntersecting` e
//!   `onDidChangeStickyScroll` da classe moram na implementação do trait, que é a única definição deles (a classe
//!   original só os repete da interface).
//! - `StickyLineCandidate` é a struct [`StickyLineCandidate`] (`top` e `height` em `f64`, as linhas em `i32`).
//! - `StickyLineCandidateProvider extends Disposable` é a struct [`StickyLineCandidateProvider`], criada por
//!   [`StickyLineCandidateProvider::new`] (devolve `Rc<Self>`: os ouvintes e as promessas guardam um [`Weak`]). O
//!   `editor` é um [`Weak`] do widget. O `async update()` é uma cadeia de [`Promise`].
//! - `_editor._getViewModel()?.getHiddenAreas()` é `CodeEditorWidget::get_view_model` mais
//!   [`IViewModel::get_hidden_areas`].
//! - O `RunOnceScheduler` de 50 ms e o `DisposableStore` da sessão são os de `je_base`; o `binarySearch` é
//!   `je_base::arrays::binary_search`.
//!
//! Divergências:
//! - O parâmetro `depth` de `getCandidateStickyLinesIntersectingFromStickyModel` só era repassado na recursão e nunca
//!   lido, e não existe aqui.
//! - O `StickyLineCandidateProvider.ID` é a constante associada [`StickyLineCandidateProvider::ID`].

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::arrays::binary_search;
use je_base::async_::RunOnceScheduler;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;

use super::sticky_scroll_element::{StickyElement, StickyModel, StickyRange};
use super::sticky_scroll_model_provider::{IStickyModelProvider, StickyModelProvider};
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::EditorOption;
use crate::config::editor_options::sticky_scroll::STICKY_SCROLL;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::language_features_service::LanguageFeaturesService;
use crate::multicursor::listen_weak;
use crate::position::Position;
use crate::range::Range;
use crate::view_model_types::IViewModel;

/// `StickyLineCandidate`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickyLineCandidate {
    pub start_line_number: i32,
    pub end_line_number: i32,
    pub top: f64,
    pub height: f64,
}

/// `IStickyLineCandidateProvider`.
pub trait IStickyLineCandidateProvider: IDisposable {
    /// Get the version ID of the sticky model.
    fn get_version_id(&self) -> Option<i64>;

    /// Update the sticky line candidates.
    fn update(&self) -> Promise<()>;

    /// Get sticky line candidates intersecting a given range.
    fn get_candidate_sticky_lines_intersecting(&self, range: StickyRange) -> Vec<StickyLineCandidate>;

    /// Event triggered when sticky scroll changes.
    fn on_did_change_sticky_scroll(&self) -> Event<()>;
}

/// `StickyLineCandidateProvider`.
pub struct StickyLineCandidateProvider {
    me: Weak<StickyLineCandidateProvider>,
    base: Rc<Disposable>,
    editor: Weak<CodeEditorWidget>,
    language_features_service: Rc<LanguageFeaturesService>,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,

    did_change_sticky_scroll: Rc<Emitter<()>>,
    update_soon: Rc<RunOnceScheduler>,
    session_store: Rc<DisposableStore>,

    model: RefCell<Option<StickyModel>>,
    cts: RefCell<Option<CancellationTokenSource>>,
    sticky_model_provider: RefCell<Option<Rc<dyn IStickyModelProvider>>>,
}

impl StickyLineCandidateProvider {
    pub const ID: &'static str = "store.contrib.stickyScrollController";

    /// `new StickyLineCandidateProvider(editor, languageFeaturesService, languageConfigurationService)`.
    pub fn new(
        editor: &Rc<CodeEditorWidget>,
        language_features_service: Rc<LanguageFeaturesService>,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let did_change_sticky_scroll = Emitter::new();
        base.register(did_change_sticky_scroll.clone());
        let session_store = DisposableStore::new();
        base.register(session_store.clone());

        let this = Rc::new_cyclic(|me: &Weak<StickyLineCandidateProvider>| {
            let scheduler_owner = me.clone();
            let update_soon = RunOnceScheduler::new(
                move || {
                    if let Some(this) = scheduler_owner.upgrade() {
                        let _ = this.update();
                    }
                },
                50,
            );
            base.register(update_soon.clone());

            base.register(listen_weak(me, editor.on_did_change_configuration(), |this: &StickyLineCandidateProvider, e| {
                if e.has_changed(EditorOption::StickyScroll) {
                    this.read_configuration();
                }
            }));

            StickyLineCandidateProvider {
                me: me.clone(),
                base: base.clone(),
                editor: Rc::downgrade(editor),
                language_features_service,
                language_configuration_service,
                did_change_sticky_scroll,
                update_soon,
                session_store,
                model: RefCell::new(None),
                cts: RefCell::new(None),
                sticky_model_provider: RefCell::new(None),
            }
        });
        this.read_configuration();
        this
    }

    /// Read and apply the sticky scroll configuration.
    fn read_configuration(&self) {
        self.session_store.clear();
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let options = *editor.get_options().get(STICKY_SCROLL);
        if !options.enabled {
            return;
        }
        let session_store = &self.session_store;
        session_store.add(listen_weak(&self.me, editor.on_did_change_model(), |this: &StickyLineCandidateProvider, _| {
            *this.model.borrow_mut() = None;
            this.update_sticky_model_provider();
            this.did_change_sticky_scroll.fire(());
            let _ = this.update();
        }));
        session_store.add(listen_weak(&self.me, editor.on_did_change_hidden_areas(), |this: &StickyLineCandidateProvider, _| {
            let _ = this.update();
        }));
        session_store.add(listen_weak(&self.me, editor.on_did_change_model_content(), |this: &StickyLineCandidateProvider, _| {
            this.update_soon.schedule(None);
        }));
        session_store.add(listen_weak(
            &self.me,
            self.language_features_service.document_symbol_provider.on_did_change(),
            |this: &StickyLineCandidateProvider, _| {
                let _ = this.update();
            },
        ));
        let me = self.me.clone();
        session_store.add(to_disposable(move || {
            if let Some(this) = me.upgrade() {
                let provider = this.sticky_model_provider.borrow_mut().take();
                if let Some(provider) = provider {
                    provider.dispose();
                }
            }
        }));
        self.update_sticky_model_provider();
        let _ = self.update();
    }

    /// Update the sticky model provider.
    fn update_sticky_model_provider(&self) {
        let previous = self.sticky_model_provider.borrow_mut().take();
        if let Some(previous) = previous {
            previous.dispose();
        }
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        if !editor.has_model() {
            return;
        }
        let me = self.me.clone();
        let on_provider_update: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(this) = me.upgrade() {
                this.update_soon.schedule(None);
            }
        });
        let provider: Rc<dyn IStickyModelProvider> = Rc::new(StickyModelProvider::new(
            &editor,
            on_provider_update,
            Rc::clone(&self.language_configuration_service),
            Rc::clone(&self.language_features_service),
        ));
        *self.sticky_model_provider.borrow_mut() = Some(provider);
    }

    /// Update the sticky model based on the current editor state.
    fn update_sticky_model(&self, token: CancellationToken) -> Promise<()> {
        let text_model = self.editor.upgrade().and_then(|editor| editor.get_model());
        let sticky_model_provider = self.sticky_model_provider.borrow().clone();
        let (Some(text_model), Some(sticky_model_provider)) = (text_model, sticky_model_provider) else {
            *self.model.borrow_mut() = None;
            return Promise::resolved(());
        };
        if text_model.is_too_large_for_tokenization() {
            *self.model.borrow_mut() = None;
            return Promise::resolved(());
        }
        let me = self.me.clone();
        let update_token = token.clone();
        sticky_model_provider.update(&token).then(move |model| {
            if !update_token.is_cancellation_requested() {
                if let Some(this) = me.upgrade() {
                    *this.model.borrow_mut() = model;
                }
            }
        })
    }

    /// `getCandidateStickyLinesIntersectingFromStickyModel(range, outlineModel, result, depth, top, lastStartLineNumber)`.
    fn collect_candidates(
        &self,
        range: StickyRange,
        outline_model: &StickyElement,
        result: &mut Vec<StickyLineCandidate>,
        top: f64,
        last_start_line_number: i32,
    ) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let Some(text_model) = editor.get_model() else {
            return;
        };
        let children = outline_model.children.borrow().clone();
        if children.is_empty() {
            return;
        }
        let mut last_line = last_start_line_number;
        let children_start_lines: Vec<i32> = children.iter().filter_map(|child| child.range).map(|range| range.start_line_number).collect();

        let find_index = |line_number: i32| {
            update_index(binary_search(&children_start_lines, &line_number, |a, b| f64::from(*a) - f64::from(*b)))
        };
        let lower_bound = find_index(range.start_line_number);
        let upper_bound = find_index(range.end_line_number);

        for i in lower_bound..=upper_bound {
            let Some(child) = children.get(i) else {
                continue;
            };
            let Some(child_range) = child.range else {
                continue;
            };
            let StickyRange { start_line_number, end_line_number } = child_range;
            if end_line_number > start_line_number + 1
                && range.start_line_number <= end_line_number + 1
                && start_line_number - 1 <= range.end_line_number
                && start_line_number != last_line
                && text_model.is_valid_range(&Range::new(start_line_number, 1, end_line_number, 1))
            {
                last_line = start_line_number;
                let line_height = editor.get_line_height_for_position(Position::new(start_line_number, 1));
                result.push(StickyLineCandidate { start_line_number, end_line_number: end_line_number - 1, top, height: line_height });
                self.collect_candidates(range, child, result, top + line_height, start_line_number);
            }
        }
    }

    /// Filter out sticky line candidates that are within hidden ranges.
    fn filter_hidden_ranges(&self, sticky_line_candidates: Vec<StickyLineCandidate>) -> Vec<StickyLineCandidate> {
        let hidden_ranges = self.editor.upgrade().and_then(|editor| editor.get_view_model()).map(|view_model| view_model.get_hidden_areas());
        let Some(hidden_ranges) = hidden_ranges else {
            return sticky_line_candidates;
        };
        sticky_line_candidates
            .into_iter()
            .filter(|candidate| {
                !hidden_ranges.iter().any(|hidden_range| {
                    candidate.start_line_number >= hidden_range.start_line_number
                        && candidate.end_line_number <= hidden_range.end_line_number + 1
                })
            })
            .collect()
    }
}

/// Update the binary search index.
fn update_index(index: i64) -> usize {
    if index == -1 {
        0
    } else if index < 0 {
        (-index - 2) as usize
    } else {
        index as usize
    }
}

impl IDisposable for StickyLineCandidateProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IStickyLineCandidateProvider for StickyLineCandidateProvider {
    fn get_version_id(&self) -> Option<i64> {
        self.model.borrow().as_ref().map(|model| model.version)
    }

    fn update(&self) -> Promise<()> {
        let previous = self.cts.borrow_mut().take();
        if let Some(previous) = previous {
            previous.dispose_with(true);
        }
        let cts = CancellationTokenSource::new(None);
        *self.cts.borrow_mut() = Some(cts.clone());
        let me = self.me.clone();
        self.update_sticky_model(cts.token()).then(move |()| {
            if let Some(this) = me.upgrade() {
                this.did_change_sticky_scroll.fire(());
            }
        })
    }

    fn get_candidate_sticky_lines_intersecting(&self, range: StickyRange) -> Vec<StickyLineCandidate> {
        let element = self.model.borrow().as_ref().and_then(|model| model.element.clone());
        let Some(element) = element else {
            return Vec::new();
        };
        let mut sticky_line_candidates: Vec<StickyLineCandidate> = Vec::new();
        self.collect_candidates(range, &element, &mut sticky_line_candidates, 0.0, -1);
        self.filter_hidden_ranges(sticky_line_candidates)
    }

    fn on_did_change_sticky_scroll(&self) -> Event<()> {
        self.did_change_sticky_scroll.event()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_search_results_become_child_indexes() {
        // Achou: o próprio índice.
        assert_eq!(update_index(3), 3);
        // `-1` (inserção no começo): o primeiro filho.
        assert_eq!(update_index(-1), 0);
        // `~n` com n = 1 (inserção depois do primeiro): o primeiro filho, que ainda contém a linha.
        assert_eq!(update_index(-2), 0);
        // `~n` com n = 4: o quarto filho.
        assert_eq!(update_index(-5), 3);
    }

    #[test]
    fn the_search_over_start_lines_finds_the_enclosing_child() {
        let start_lines = [1, 10, 20];
        let index_of = |line: i32| update_index(binary_search(&start_lines, &line, |a, b| f64::from(*a) - f64::from(*b)));
        assert_eq!(index_of(10), 1);
        assert_eq!(index_of(15), 1);
        assert_eq!(index_of(25), 2);
        assert_eq!(index_of(0), 0);
    }
}
