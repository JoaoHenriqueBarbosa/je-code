//! Porte parcial de `src/vs/workbench/api/common/extHostTypeConverters.ts`.
//!
//! Divergências: só o espaço de nomes `ViewColumn`, que `extHostBrowsers.ts` usa. `EditorGroupColumn`
//! (`workbench/services/editor/common/editorGroupColumn.ts`) e as constantes `ACTIVE_GROUP` e
//! `SIDE_GROUP` (`editorService.ts`) ainda não têm porte próprio: ficam definidos aqui, com os mesmos
//! valores do original, até esses arquivos serem portados.

use je_base::errors::{self, Error};

use super::ext_host_types::ViewColumn as ApiViewColumn;

/// `EditorGroupColumn`.
pub type EditorGroupColumn = f64;

/// `ACTIVE_GROUP` de `editorService.ts`.
pub const ACTIVE_GROUP: EditorGroupColumn = -1.0;

/// `SIDE_GROUP` de `editorService.ts`.
pub const SIDE_GROUP: EditorGroupColumn = -2.0;

/// `typeConverters.ViewColumn`.
pub struct ViewColumn;

impl ViewColumn {
    /// `ViewColumn.from`: a coluna da API (base 1) para a coluna do grupo de editores (base 0).
    pub fn from(column: Option<f64>) -> EditorGroupColumn {
        match column {
            Some(column) if column >= ApiViewColumn::ONE => column - 1.0, // ajusta o índice zero (ViewColumn.ONE => 0)
            Some(column) if column == ApiViewColumn::BESIDE => SIDE_GROUP,
            _ => ACTIVE_GROUP, // o padrão é sempre o grupo ativo
        }
    }

    /// `ViewColumn.to`: levanta um erro para uma posição que não é de coluna.
    pub fn to(position: EditorGroupColumn) -> f64 {
        if position >= 0.0 {
            return position + 1.0; // ajusta para o índice (ViewColumn.ONE => 1)
        }

        errors::throw(Error::new("invalid 'EditorGroupColumn'"))
    }
}
