//! Porte parcial de `src/vs/workbench/contrib/update/browser/updateTitleBarEntry.ts`: `getAdditionalUpdateTitleBarMenuWhen`.
//!
//! Fora do recorte (dependem de DOM, do hover e da árvore de ações, ainda não portados): o `UpdateTitleBarEntry` (item de
//! ação que renderiza o indicador, trata o `Tab` e abre o hover ao `workbench.action.showHover`), o
//! `UpdateTitleBarContribution`, o `registerUpdateTitleBarMenuPlacement` e o registro da ação
//! `workbench.actions.updateIndicator`.

use je_platform::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression};

use crate::contrib::update::common::update::update_title_bar_context;

/// `getAdditionalUpdateTitleBarMenuWhen(when?)`.
pub fn get_additional_update_title_bar_menu_when(when: Option<ContextKeyExpression>) -> ContextKeyExpression {
    ContextKeyExpr::and([Some(update_title_bar_context().expr()), when]).expect("a conjunção tem termos")
}
