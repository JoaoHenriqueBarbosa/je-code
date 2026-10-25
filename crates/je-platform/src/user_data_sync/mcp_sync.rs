//! Porte de `src/vs/platform/userDataSync/common/mcpSync.ts`: o sincronizador de `mcp.json`.
//!
//! Decisões de tradução (as da base estão em [`super::abstract_json_synchronizer`]):
//! - `McpSynchroniser extends AbstractJsonSynchronizer` é [`AbstractJsonSynchronizer`] sobre [`McpSyncKind`], que
//!   dá `getContentFromSyncContent` e `toSyncContent`. O construtor é [`McpSynchroniser::for_profile`] (o `new` da
//!   base é genérico).
//! - `IMcpSyncContent` (`{ mcp?: string }`) é um objeto JSON com a chave `mcp` ou vazio.

use std::rc::Rc;

use je_base::JsString;
use je_base::json::{Object, Value};

use super::abstract_json_synchronizer::{AbstractJsonSynchronizer, JsonSyncKind, get_sync_content_property};
use super::abstract_synchronizer::SynchroniserServices;
use super::user_data_sync::{IUserDataSyncLogService, IUserDataSyncResource, SyncResource};
use crate::user_data_profile::UserDataProfile;

/// `getMcpContentFromSyncContent`: `JSON.parse(syncContent).mcp ?? null`; o erro vai para o log.
pub fn get_mcp_content_from_sync_content(sync_content: &str, log_service: &IUserDataSyncLogService) -> Option<String> {
    get_sync_content_property(sync_content, "mcp", log_service)
}

/// O que `McpSynchroniser` dá à base: o conteúdo da chave `mcp` e o objeto `{ mcp }`.
pub struct McpSyncKind;

impl JsonSyncKind for McpSyncKind {
    fn get_content_from_sync_content(sync_content: &str, log_service: &IUserDataSyncLogService) -> Option<String> {
        get_mcp_content_from_sync_content(sync_content, log_service)
    }

    fn to_sync_content(mcp: Option<&str>) -> Value {
        let mut content = Object::new();
        if let Some(mcp) = mcp.filter(|mcp| !mcp.is_empty()) {
            content.insert(JsString::from("mcp"), Value::from(mcp));
        }
        Value::Object(content)
    }
}

/// `McpSynchroniser`.
pub type McpSynchroniser = AbstractJsonSynchronizer<McpSyncKind>;

impl McpSynchroniser {
    pub fn for_profile(profile: UserDataProfile, collection: Option<String>, services: SynchroniserServices) -> Rc<Self> {
        let mcp_resource = profile.mcp_resource.clone();
        Self::new(mcp_resource, IUserDataSyncResource { sync_resource: SyncResource::Mcp, profile }, collection, "mcp.json", services)
    }
}
