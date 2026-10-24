//! Porte de `src/vs/workbench/contrib/webview/test/browser/overlayWebview.test.ts` (suíte `OverlayWebview`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo do caso; a sobreposição vai para o
//!   `store` do guarda (`store.add`).
//! - **Sem DOM.** Os `div` do original são os modelos de `overlay_webview` ([`OverlayPart`], [`OverlayAnchor`],
//!   [`OverlayRoot`]); `className`, `appendChild` e `closest('.part')` viram o `class_name` do elemento e a cadeia de
//!   ancestrais da âncora. `document.body.appendChild(root)` e o `root.remove()` do `toDisposable` não têm efeito no
//!   modelo (não há documento), então o `toDisposable` não aparece.
//! - **`instantiationService.stub(...)` + `createInstance(OverlayWebview, initInfo)`.** O único serviço que o corpo lê é o
//!   `IWorkbenchLayoutService.getContainer`, que devolve a raiz: o dublê é a própria [`OverlayRoot`] passada a
//!   [`OverlayWebview::new`]. `IWebviewService` e `IContextKeyService` (stubs vazios) só servem a `claim`/`_show`, fora do
//!   recorte.
//! - **`await timeout(0)`.** É [`OverlayPart::deliver_mutation_records`], a entrega do `MutationObserver` que o `timeout`
//!   espera.
//! - **Geometria do segundo caso.** `position: relative; width: 400px; height: 400px; border: 1px; box-sizing: border-box`
//!   é `set_geometry(0, 0, 400, 400, 1)`; cada âncora `position: absolute` com porcentagens é `place_absolute`.
//!   `getComputedStyle(...).borderBottom{Left,Right}Radius` é [`OverlayWebview::computed_bottom_corner_radii`].
//! - **`assert.deepStrictEqual({ ... }, { ... })`.** É um `assert_eq!` sobre as duas estruturas.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::webview::browser_webview::{WebviewContentOptions, WebviewInitInfo, WebviewOptions};
use je_workbench::contrib::webview::overlay_webview::{OverlayAnchor, OverlayPart, OverlayRoot, OverlayWebview};

#[derive(Debug, PartialEq)]
struct OuterEdges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

const fn edges(left: bool, right: bool, top: bool, bottom: bool) -> OuterEdges {
    OuterEdges { left, right, top, bottom }
}

/// `createOverlay(root)`.
fn create_overlay(root: &Rc<OverlayRoot>, store: &common::LeakGuard) -> Rc<OverlayWebview> {
    store.add_rc(OverlayWebview::new(
        WebviewInitInfo {
            title: None,
            options: WebviewOptions::default(),
            content_options: WebviewContentOptions::default(),
            extension: None,
            ..WebviewInitInfo::default()
        },
        root.clone(),
    ))
}

/// `getOuterEdges(element)`.
fn get_outer_edges(overlay: &OverlayWebview) -> OuterEdges {
    let container = overlay.container();
    OuterEdges {
        left: container.contains_class("webview-overlay-outer-left"),
        right: container.contains_class("webview-overlay-outer-right"),
        top: container.contains_class("webview-overlay-outer-top"),
        bottom: container.contains_class("webview-overlay-outer-bottom"),
    }
}

#[derive(Debug, PartialEq)]
struct ModalOverlay {
    outer_edges: OuterEdges,
    modal: bool,
}

#[derive(Debug, PartialEq)]
struct AnchorPartReport {
    initial_edges: OuterEdges,
    updated_first_edges: OuterEdges,
    reanchored_edges: OuterEdges,
    after_old_part_changed: OuterEdges,
    updated_second_edges: OuterEdges,
    modal_overlay: ModalOverlay,
    modal_removed_after_reanchor: bool,
}

#[test]
fn keeps_overlay_classes_synchronized_with_the_current_anchor_part() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let root = OverlayRoot::new("");
    let first_part = OverlayPart::new("part floating-editor-outer-left floating-editor-outer-top");
    let first_anchor = OverlayAnchor::new(vec![first_part.clone()]);

    let second_part = OverlayPart::new("part floating-part-outer-left floating-part-outer-bottom");
    let second_anchor = OverlayAnchor::new(vec![second_part.clone()]);

    let modal_part = OverlayPart::new("part modal-editor-part");
    let modal_anchor = OverlayAnchor::new(vec![modal_part.clone()]);

    let overlay = create_overlay(&root, &store);

    overlay.set_anchor_element(&first_anchor, None);
    let initial_edges = get_outer_edges(&overlay);

    first_part.set_class_name("part floating-editor-outer-right floating-editor-outer-bottom");
    first_part.deliver_mutation_records();
    let updated_first_edges = get_outer_edges(&overlay);

    overlay.set_anchor_element(&second_anchor, None);
    let reanchored_edges = get_outer_edges(&overlay);

    first_part.set_class_name("part floating-editor-outer-right floating-editor-outer-top");
    first_part.deliver_mutation_records();
    let after_old_part_changed = get_outer_edges(&overlay);

    second_part.set_class_name("part floating-part-outer-right floating-part-outer-top");
    second_part.deliver_mutation_records();
    let updated_second_edges = get_outer_edges(&overlay);

    overlay.set_anchor_element(&modal_anchor, None);
    let modal_overlay = ModalOverlay { outer_edges: get_outer_edges(&overlay), modal: overlay.container().contains_class("webview-overlay-modal") };

    overlay.set_anchor_element(&first_anchor, None);

    assert_eq!(
        AnchorPartReport {
            initial_edges,
            updated_first_edges,
            reanchored_edges,
            after_old_part_changed,
            updated_second_edges,
            modal_overlay,
            modal_removed_after_reanchor: !overlay.container().contains_class("webview-overlay-modal"),
        },
        AnchorPartReport {
            initial_edges: edges(true, false, true, false),
            updated_first_edges: edges(false, true, false, true),
            reanchored_edges: edges(true, false, false, true),
            after_old_part_changed: edges(true, false, false, true),
            updated_second_edges: edges(false, true, true, false),
            modal_overlay: ModalOverlay { outer_edges: edges(false, false, false, false), modal: true },
            modal_removed_after_reanchor: true,
        }
    );
}

type Corners = Vec<(&'static str, (String, String))>;

fn radii(bottom_left: &str, bottom_right: &str) -> (String, String) {
    (bottom_left.to_string(), bottom_right.to_string())
}

#[test]
fn rounds_only_webview_anchors_that_reach_a_native_corner_across_editor_splits_and_moves() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let root = OverlayRoot::new("monaco-workbench modern-ui floating-panels mac macos-tahoe nostatusbar");
    root.set_css_text("--vscode-cornerRadius-large: 8px; --vscode-spacing-size40: 4px; --vscode-strokeThickness: 1px; --window-zoom-factor: 1;");
    let part = OverlayPart::new("part floating-editor-outer-left floating-editor-outer-right floating-editor-outer-top floating-editor-outer-bottom");
    part.set_geometry(0.0, 0.0, 400.0, 400.0, 1.0);
    let anchors: Vec<(&'static str, Rc<OverlayAnchor>)> = ["topLeft", "topRight", "bottomLeft", "bottomRight"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let anchor = OverlayAnchor::new(vec![part.clone()]);
            anchor.place_absolute((index % 2) as f64 * 50.0, (index / 2) as f64 * 50.0, 50.0, 50.0);
            (name, anchor)
        })
        .collect();
    let overlay = create_overlay(&root, &store);
    let corners = || -> Corners {
        anchors
            .iter()
            .map(|(name, anchor)| {
                overlay.set_anchor_element(anchor, None);
                (*name, overlay.computed_bottom_corner_radii())
            })
            .collect()
    };
    let default_zoom = corners();
    root.set_property("--window-zoom-factor", "0.5");
    let zoomed_out = corners();

    let moved_anchor = &anchors[1].1;
    overlay.set_anchor_element(moved_anchor, None);
    moved_anchor.set_top_percent(50.0);
    overlay.set_anchor_element(moved_anchor, None);

    assert_eq!(
        (default_zoom, zoomed_out, get_outer_edges(&overlay)),
        (
            vec![
                ("topLeft", radii("8px", "8px")),
                ("topRight", radii("8px", "8px")),
                ("bottomLeft", radii("11px", "8px")),
                ("bottomRight", radii("8px", "11px")),
            ],
            vec![
                ("topLeft", radii("8px", "8px")),
                ("topRight", radii("8px", "8px")),
                ("bottomLeft", radii("27px", "8px")),
                ("bottomRight", radii("8px", "27px")),
            ],
            edges(false, true, false, true),
        )
    );
}
