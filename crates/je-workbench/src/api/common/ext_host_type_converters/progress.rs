//! `extHostTypeConverters.ts`: o namespace `ProgressLocation`, a conversão do local de progresso da API para o do
//! workbench.
//!
//! Divergências: `vscode.ProgressLocation | { viewId: string }` é [`ProgressLocationArg`] (o valor numérico da API ou
//! o objeto com o `viewId`); o `throw new Error(...)` do original é o `Err` de [`ProgressLocation::from`].

use je_base::errors::Error;
use je_platform::progress::progress::{ProgressLocation as MainProgressLocation, ProgressLocationId};

use crate::api::common::ext_host_types as types;

/// `vscode.ProgressLocation | { viewId: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressLocationArg {
    /// O valor numérico de `vscode.ProgressLocation`.
    Value(i32),
    /// `{ viewId }`.
    ViewId(String),
}

impl From<types::ProgressLocation> for ProgressLocationArg {
    fn from(location: types::ProgressLocation) -> Self {
        ProgressLocationArg::Value(location.value())
    }
}

/// `typeConverters.ProgressLocation`.
pub struct ProgressLocation;

impl ProgressLocation {
    /// `ProgressLocation.from`.
    pub fn from(loc: &ProgressLocationArg) -> Result<ProgressLocationId, Error> {
        let value = match loc {
            ProgressLocationArg::ViewId(view_id) => return Ok(ProgressLocationId::View(view_id.clone())),
            ProgressLocationArg::Value(value) => *value,
        };

        match types::ProgressLocation::from_value(value) {
            Some(types::ProgressLocation::SourceControl) => Ok(MainProgressLocation::Scm.into()),
            Some(types::ProgressLocation::Window) => Ok(MainProgressLocation::Window.into()),
            Some(types::ProgressLocation::Notification) => Ok(MainProgressLocation::Notification.into()),
            None => Err(Error::new("Unknown 'ProgressLocation'")),
        }
    }
}
