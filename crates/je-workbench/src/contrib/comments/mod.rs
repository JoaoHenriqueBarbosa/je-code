//! Porte de `src/vs/workbench/contrib/comments` (comentários de revisão: threads de controladores, o painel de comentários, os
//! widgets no editor e no notebook).
//!
//! Feito (fatia 1):
//! - `common/commentModel.ts` ([`common::comment_model`]), `common/commentContextKeys.ts` ([`common::comment_context_keys`]) e
//!   `common/commentsConfiguration.ts` ([`common::comments_configuration`]);
//! - `browser/commentService.ts` ([`browser::comment_service`]: `ICommentService`, `CommentService`, os eventos,
//!   `ICommentController`, `ICommentInfo`, `INotebookCommentInfo`, o registro de controladores e o estado
//!   `commentsEnabled` e `continueOnComments`);
//! - `browser/commentMenus.ts` ([`browser::comment_menus`]) e `browser/commentsModel.ts` ([`browser::comments_model`]), que o
//!   `CommentService` constrói por conta própria (`createInstance(CommentMenus)` e `new CommentsModel()`), então vieram junto.
//!
//! Fatias seguintes, na ordem de dependência:
//! 1. `browser/commentsFilterOptions.ts`;
//! 2. `browser/commentColors.ts`;
//! 3. `browser/commentsTreeViewer.ts`;
//! 4. `browser/commentsView.ts`, com `test/browser/commentsView.test.ts`;
//! 5. `browser/commentReply.ts`, com `test/browser/commentReply.test.ts`;
//! 6. `browser/commentNode.ts`;
//! 7. `browser/commentThreadBody.ts`, `commentThreadHeader.ts`, `commentThreadWidget.ts` e `commentThreadZoneWidget.ts`;
//! 8. `browser/commentsController.ts`;
//! 9. `browser/commentsEditorContribution.ts`;
//! 10. o resto: `common/commentCommandIds.ts`, `common/commentThreadWidget.ts`, `browser/comments.ts`,
//!     `browser/comments.contribution.ts`, `browser/commentFormActions.ts`, `browser/commentGlyphWidget.ts`,
//!     `browser/commentThreadAdditionalActions.ts`, `browser/commentThreadRangeDecorator.ts`,
//!     `browser/commentsAccessibility.ts`, `browser/commentsAccessibleView.ts`, `browser/commentsInputContentProvider.ts`,
//!     `browser/commentsViewActions.ts`, `browser/reactionsAction.ts`, `browser/simpleCommentEditor.ts` e
//!     `browser/timestamp.ts`.
//!
//! Os tipos de `editor/common/languages.ts` que o módulo troca com os controladores (`CommentThread`, `Comment`,
//! `CommentingRanges`, `PendingCommentThread`...) estão no fim de `je_editor::languages`.

pub mod browser;
pub mod common;
