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
//! Fatia 2 (feita):
//! - `browser/commentsFilterOptions.ts` ([`browser::comments_filter_options`]) e `browser/commentColors.ts`
//!   ([`browser::comment_colors`]: os ids das cores, que já estão no registro do `je-theme`, e as cores por estado da thread);
//! - `browser/timestamp.ts` ([`browser::timestamp`]: `TimestampWidget`);
//! - `browser/commentsTreeViewer.ts` ([`browser::comments_tree_viewer`]: `CommentsList`, `ResourceWithCommentsRenderer`,
//!   `CommentNodeRenderer`, `CommentsMenus`, `Filter`, o delegado de alturas e o provedor de identidade). O que a árvore divide com a
//!   do painel de problemas (rótulo de recurso, hover, auxiliares de layout, desfecho do filtro por texto) vem de
//!   `markers_tree_viewer`.
//!
//! Fatias seguintes, na ordem de dependência:
//! 1. `browser/commentsView.ts`, com `test/browser/commentsView.test.ts`;
//! 2. `browser/commentReply.ts`, com `test/browser/commentReply.test.ts`;
//! 3. `browser/commentNode.ts`;
//! 4. `browser/commentThreadBody.ts`, `commentThreadHeader.ts`, `commentThreadWidget.ts` e `commentThreadZoneWidget.ts`;
//! 5. `browser/commentsController.ts`;
//! 6. `browser/commentsEditorContribution.ts`;
//! 7. o resto: `common/commentCommandIds.ts`, `common/commentThreadWidget.ts`, `browser/comments.ts`,
//!    `browser/comments.contribution.ts`, `browser/commentFormActions.ts`, `browser/commentGlyphWidget.ts`,
//!    `browser/commentThreadAdditionalActions.ts`, `browser/commentThreadRangeDecorator.ts`,
//!    `browser/commentsAccessibility.ts`, `browser/commentsAccessibleView.ts`, `browser/commentsInputContentProvider.ts`,
//!    `browser/commentsViewActions.ts`, `browser/reactionsAction.ts` e `browser/simpleCommentEditor.ts`.
//!
//! Os tipos de `editor/common/languages.ts` que o módulo troca com os controladores (`CommentThread`, `Comment`,
//! `CommentingRanges`, `PendingCommentThread`...) estão no fim de `je_editor::languages`.

pub mod browser;
pub mod common;
