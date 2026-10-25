//! Porte de `src/vs/editor/contrib/hover/browser`.
//!
//! O hover de conteúdo do editor é portado em fatias; esta pasta tem as seguintes:
//!
//! - H1 (a base que as outras usam), pronta: `hoverOperation.ts` ([`hover_operation`]), `contentHoverComputer.ts`
//!   ([`content_hover_computer`]), `contentHoverTypes.ts` ([`content_hover_types`]), `hoverActionIds.ts`
//!   ([`hover_action_ids`]), `contentHoverStatusBar.ts` ([`content_hover_status_bar`]) e `resizableContentWidget.ts`
//!   ([`resizable_content_widget`], sobre o `ResizableHTMLElement` de `je-widgets`), além de `hoverTypes.ts`
//!   ([`hover_types`]), `getHover.ts` ([`get_hover`]), `hoverUtils.ts` ([`hover_utils`]), `hoverCopyButton.ts`
//!   ([`hover_copy_button`]) e a parte estática de `contentHoverRendered.ts` ([`content_hover_rendered`]).
//! - H2: `contentHoverWidget.ts`.
//! - H3: `contentHoverWidgetWrapper.ts`.
//! - H4: `contentHoverController.ts` e `hoverContribution.ts`.
//! - H5: `markdownHoverParticipant.ts`.
//! - H6: `markerHoverParticipant.ts`.
//! - H7: o hover da margem (`glyphHover*`: `marginHoverWidget.ts`, `marginHoverController.ts` e o que mais houver).
//! - H8: `hoverActions.ts` e `hoverAccessibleViews.ts`.
//! - Os casos de `src/vs/editor/contrib/hover/test/browser/contentHover.test.ts` (o caso do `computeHoverPositions` já
//!   está em `tests/content_hover.rs`) e os demais testes da pasta, portados caso a caso quando as peças que eles
//!   exercitam existirem.

pub mod content_hover_computer;
pub mod content_hover_rendered;
pub mod content_hover_status_bar;
pub mod content_hover_types;
pub mod get_hover;
pub mod hover_action_ids;
pub mod hover_copy_button;
pub mod hover_operation;
pub mod hover_types;
pub mod hover_utils;
pub mod resizable_content_widget;
