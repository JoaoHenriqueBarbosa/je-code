//! Porte de `src/vs/workbench/contrib/search/browser/searchTreeModel/rangeDecorations.ts`: a decoração de destaque de
//! um intervalo em um modelo (`RangeHighlightDecorations`), que some sozinha quando as decorações do modelo mudam.
//!
//! Divergências:
//! - `createInstance(RangeHighlightDecorations)` é o [`Injectable`] sem argumentos e com o `IModelService` como
//!   dependência. O `URI | ITextModel` de `highlightRange` é [`HighlightResource`]; o `ownerId` (que o original não lê) não
//!   existe.
//! - `ModelDecorationOptions.register(...)` estático é um `thread_local` de [`Rc<ModelDecorationOptions>`].

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::uri::URI;
use je_editor::model::{ModelDecorationOptionsInit, TrackedRangeStickiness};
use je_editor::range::Range;
use je_editor::services_model::IModelService;
use je_editor::text_model::{ModelDecorationOptions, TextModel};
use je_instantiation::Injectable;

thread_local! {
    /// `_RANGE_HIGHLIGHT_DECORATION`.
    static RANGE_HIGHLIGHT_DECORATION: Rc<ModelDecorationOptions> = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: "search-range-highlight".to_owned(),
        stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
        class_name: Some("rangeHighlight".to_owned()),
        is_whole_line: true,
        ..ModelDecorationOptionsInit::default()
    });
}

/// `URI | ITextModel`.
pub enum HighlightResource {
    Uri(URI),
    Model(Rc<TextModel>),
}

/// `RangeHighlightDecorations`: pode acrescentar uma decoração de destaque de intervalo a um modelo e a remove sozinha
/// quando as decorações do modelo mudam.
pub struct RangeHighlightDecorations {
    this: Weak<RangeHighlightDecorations>,
    model_service: Rc<dyn IModelService>,
    decoration_id: RefCell<Option<String>>,
    model: RefCell<Option<Rc<TextModel>>>,
    model_disposables: Rc<DisposableStore>,
}

impl Injectable for RangeHighlightDecorations {
    type Args = ();
    type Deps = (Rc<dyn IModelService>,);

    fn create(_: (), (model_service,): Self::Deps) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            model_service,
            decoration_id: RefCell::new(None),
            model: RefCell::new(None),
            model_disposables: DisposableStore::new(),
        })
    }
}

impl RangeHighlightDecorations {
    pub fn remove_highlight_range(&self) {
        let model = self.model.borrow().clone();
        let decoration_id = self.decoration_id.borrow().clone();
        if let (Some(model), Some(decoration_id)) = (model, decoration_id) {
            model.change_decorations(|accessor| accessor.remove_decoration(&decoration_id), 0);
        }
        *self.decoration_id.borrow_mut() = None;
    }

    pub fn highlight_range(&self, resource: HighlightResource, range: &Range) {
        let model = match resource {
            HighlightResource::Uri(uri) => self.model_service.get_model(&uri),
            HighlightResource::Model(model) => Some(model),
        };

        if let Some(model) = model {
            self.do_highlight_range(&model, range);
        }
    }

    fn do_highlight_range(&self, model: &Rc<TextModel>, range: &Range) {
        self.remove_highlight_range();
        let options = RANGE_HIGHLIGHT_DECORATION.with(Rc::clone);
        let decoration_id = model.change_decorations(|accessor| accessor.add_decoration(range, options), 0);
        *self.decoration_id.borrow_mut() = decoration_id;
        self.set_model(model);
    }

    /// O corpo comum dos dois ouvintes de `setModel`: o modelo mudou as decorações ou vai ser descartado.
    fn on_model_changed(&self) {
        self.clear_model_listeners();
        self.remove_highlight_range();
        *self.model.borrow_mut() = None;
    }

    fn set_model(&self, model: &Rc<TextModel>) {
        let is_same_model = self.model.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, model));
        if !is_same_model {
            self.clear_model_listeners();
            *self.model.borrow_mut() = Some(model.clone());
            let this = self.this.clone();
            self.model_disposables.add(model.on_did_change_decorations().listen(move |_| {
                if let Some(this) = this.upgrade() {
                    this.on_model_changed();
                }
            }));
            let this = self.this.clone();
            self.model_disposables.add(model.on_will_dispose().listen(move |_| {
                if let Some(this) = this.upgrade() {
                    this.on_model_changed();
                }
            }));
        }
    }

    fn clear_model_listeners(&self) {
        self.model_disposables.clear();
    }
}

impl IDisposable for RangeHighlightDecorations {
    fn dispose(&self) {
        let has_model = self.model.borrow().is_some();
        if has_model {
            self.remove_highlight_range();
            *self.model.borrow_mut() = None;
        }
        self.model_disposables.dispose();
    }
}
