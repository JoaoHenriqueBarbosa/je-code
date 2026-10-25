//! Porte de `src/vs/editor/browser/widget/diffEditor/components/diffEditorSash.ts`: a posição do sash entre o editor original e o
//! modificado (`SashLayout`) e o sash em si (`DiffEditorSash`).
//!
//! Divergências:
//! - **`Sash` é o de `je-widgets`**, e o `HTMLElement` onde ele entra é o `NodeId` da árvore de widgets de `je-ui` (com o [`Ui`]
//!   dono dela). O `{ getVerticalSashTop, getVerticalSashLeft, getVerticalSashHeight }` do original é o
//!   [`DiffSashLayoutProvider`], que implementa o `VerticalSashLayoutProvider` do porte.
//! - `{ height: IObservable<number>; width: IObservable<number> }` (que o original escreve inline nos dois construtores) é o
//!   [`ObservableDimension`].
//! - `IObservable<IBoundarySashes | undefined>` é `IObservable<Option<Rc<BoundarySashes>>>` (a igualdade estrita do `Rc` é a
//!   identidade do objeto, como no `===` do original).
//! - `() => void` do `resetSash` é um `Rc<dyn Fn()>`.
//! - O sash do original é um membro privado só usado nos ouvintes; aqui os ouvintes guardam um `Weak` dele e o `base` do
//!   `DiffEditorSash` o descarta.

use std::cell::Cell;
use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::observables::observable_value::{ObservableValue, observable_value_owned};
use je_base::observable::{DebugOwner, IObservable, IReader, ISettableObservable, Observable, SettableExt, autorun, derived_with_setter};
use je_ui::{NodeId, Ui};
use je_widgets::sash::{BoundarySashes, Sash, SashEvent, SashOptions, SashState, VerticalSashLayoutProvider};

use crate::diff_editor_options::DiffEditorOptions;

/// `{ height: IObservable<number>; width: IObservable<number> }`.
#[derive(Clone)]
pub struct ObservableDimension {
    pub height: IObservable<f64>,
    pub width: IObservable<f64>,
}

/// `SashLayout`.
pub struct SashLayout {
    pub sash_left: ISettableObservable<f64>,
    sash_ratio: Rc<ObservableValue<Option<f64>, ()>>,
    pub dimensions: ObservableDimension,
}

impl SashLayout {
    pub fn new(options: Rc<DiffEditorOptions>, dimensions: ObservableDimension) -> Self {
        let owner = DebugOwner::new("SashLayout");
        let sash_ratio = observable_value_owned::<Option<f64>, ()>(owner.clone(), None);

        let sash_left = {
            let (options, width, sash_ratio_read) = (options, dimensions.width.clone(), sash_ratio.clone());
            let (width_for_setter, sash_ratio_write) = (width.clone(), sash_ratio.clone());
            derived_with_setter(
                Some(owner),
                move |reader| {
                    let ratio = sash_ratio_read.read(reader).unwrap_or_else(|| options.split_view_default_ratio.read(reader));
                    compute_sash_left(&options, &width, ratio, reader)
                },
                move |value, tx| {
                    let content_width = width_for_setter.get();
                    sash_ratio_write.set(Some(value / content_width), tx);
                },
            )
        };

        Self { sash_left, sash_ratio, dimensions }
    }

    pub fn reset_sash(&self) {
        self.sash_ratio.set(None, None);
    }
}

/// `_computeSashLeft(desiredRatio, reader)` (`@pure`).
fn compute_sash_left(options: &DiffEditorOptions, width: &IObservable<f64>, desired_ratio: f64, reader: &dyn IReader) -> f64 {
    let content_width = width.read(reader);
    let mid_point = (options.split_view_default_ratio.read(reader) * content_width).floor();
    let sash_left = if options.enable_split_view_resizing.read(reader) { (desired_ratio * content_width).floor() } else { mid_point };

    const MINIMUM_EDITOR_WIDTH: f64 = 100.0;
    if content_width <= MINIMUM_EDITOR_WIDTH * 2.0 {
        return mid_point;
    }
    if sash_left < MINIMUM_EDITOR_WIDTH {
        return MINIMUM_EDITOR_WIDTH;
    }
    if sash_left > content_width - MINIMUM_EDITOR_WIDTH {
        return content_width - MINIMUM_EDITOR_WIDTH;
    }
    sash_left
}

/// O `{ getVerticalSashTop: () => 0, getVerticalSashLeft: () => this.sashLeft.get(), getVerticalSashHeight: () => this._dimensions.height.get() }`
/// do original.
pub struct DiffSashLayoutProvider {
    sash_left: ISettableObservable<f64>,
    height: IObservable<f64>,
}

impl VerticalSashLayoutProvider for DiffSashLayoutProvider {
    fn get_vertical_sash_left(&self, _sash: &Sash) -> f64 {
        self.sash_left.get()
    }

    fn get_vertical_sash_top(&self, _sash: &Sash) -> Option<f64> {
        Some(0.0)
    }

    fn get_vertical_sash_height(&self, _sash: &Sash) -> Option<f64> {
        Some(self.height.get())
    }
}

/// `DiffEditorSash`.
pub struct DiffEditorSash {
    base: Rc<Disposable>,
    pub sash_left: ISettableObservable<f64>,
}

impl DiffEditorSash {
    /// O construtor: `dom_node` é o nó da árvore de `ui` onde o sash entra (o `_domNode` do original).
    pub fn new(
        ui: &Ui,
        dom_node: NodeId,
        dimensions: ObservableDimension,
        enabled: IObservable<bool>,
        boundary_sashes: IObservable<Option<Rc<BoundarySashes>>>,
        sash_left: ISettableObservable<f64>,
        reset_sash: Rc<dyn Fn()>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let sash = Sash::new_vertical(
            ui,
            dom_node,
            Rc::new(DiffSashLayoutProvider { sash_left: sash_left.clone(), height: dimensions.height.clone() }),
            SashOptions::default(),
        );
        base.register(sash.clone());
        let start_sash_position: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));

        {
            let (sash_left, start_sash_position) = (sash_left.clone(), start_sash_position.clone());
            base.register(sash.on_did_start().listen(move |_| {
                start_sash_position.set(Some(sash_left.get()));
            }));
        }
        {
            let (sash_left, start_sash_position) = (sash_left.clone(), start_sash_position);
            base.register(sash.on_did_change().listen(move |e: &SashEvent| {
                let start = start_sash_position.get().expect("o arraste começa com onDidStart");
                sash_left.set(start + (e.current_x - e.start_x), None);
            }));
        }
        {
            let sash_weak = Rc::downgrade(&sash);
            base.register(sash.on_did_end().listen(move |_| {
                if let Some(sash) = sash_weak.upgrade() {
                    sash.layout();
                }
            }));
        }
        base.register(sash.on_did_reset().listen(move |_| reset_sash()));

        {
            let sash_weak = Rc::downgrade(&sash);
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let sashes = boundary_sashes.read(reader);
                if let (Some(sashes), Some(sash)) = (sashes, sash_weak.upgrade()) {
                    sash.set_orthogonal_end_sash(sashes.bottom.clone());
                }
            }));
        }

        {
            let (sash_weak, sash_left, height) = (Rc::downgrade(&sash), sash_left.clone(), dimensions.height);
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                // DiffEditorSash.layoutSash
                let enabled = enabled.read(reader);
                let Some(sash) = sash_weak.upgrade() else { return };
                sash.set_state(if enabled { SashState::Enabled } else { SashState::Disabled });
                sash_left.read(reader);
                height.read(reader);
                sash.layout();
            }));
        }

        Rc::new(Self { base, sash_left })
    }
}

impl IDisposable for DiffEditorSash {
    fn dispose(&self) {
        self.base.dispose();
    }
}
