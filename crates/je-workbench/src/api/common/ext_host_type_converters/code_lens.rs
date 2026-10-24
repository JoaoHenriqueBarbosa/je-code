//! `extHostApiCommands.ts` (`vscode.executeCodeLensProvider`): a ponte do `languages.CodeLens` do editor com o valor que o
//! RPC entrega, o objeto `{ range, id?, command? }` (o `ICodeLensDto` sem o `cacheId`).
//!
//! Divergências:
//! - O original não tem um namespace `CodeLens` em `extHostTypeConverters.ts`: a conversão para `types.CodeLens` mora no
//!   resultado do comando de API (ver `ext_host_api_commands`). Aqui só estão as duas pontas do DTO.
//! - O `languages.Command` do editor não carrega o `$ident`: o comando cruza como `{ id, title, tooltip?, arguments? }`.

use je_base::dynamic::Dynamic;
use je_editor::code_action::languages::Command;
use je_editor::language_features_service::CodeLens as EditorCodeLens;

use super::geometry::Range;

/// A lente de código do editor como DTO.
pub struct CodeLens;

impl CodeLens {
    /// O `languages.Command` como o DTO do RPC.
    fn command_to_dto(command: &Command) -> Dynamic {
        let entries = [
            ("id", Some(Dynamic::string(&command.id))),
            ("title", Some(Dynamic::string(&command.title))),
            ("tooltip", command.tooltip.as_deref().map(Dynamic::string)),
            ("arguments", command.arguments.as_ref().map(|arguments| Dynamic::array(arguments.iter().cloned()))),
        ];
        Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item))))
    }

    /// O `languages.CodeLens` como o DTO do RPC.
    pub fn to_dto(lens: &EditorCodeLens) -> Dynamic {
        let entries = [
            ("range", Some(Range::to_dto(&lens.range))),
            ("id", lens.id.as_deref().map(Dynamic::string)),
            ("command", lens.command.as_ref().map(CodeLens::command_to_dto)),
        ];
        Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item))))
    }

    /// A leitura do DTO de [`CodeLens::to_dto`]; `None` se o valor não tem um `range` válido.
    pub fn from_dto(value: &Dynamic) -> Option<EditorCodeLens> {
        let command = match value.get_str("command") {
            command @ Dynamic::Object(_) => {
                let Dynamic::String(id) = command.get_str("id") else {
                    return None;
                };
                let text = |key: &str| match command.get_str(key) {
                    Dynamic::String(text) => Some(text.to_string_lossy()),
                    _ => None,
                };
                Some(Command {
                    id: id.to_string_lossy(),
                    title: text("title").unwrap_or_default(),
                    tooltip: text("tooltip"),
                    arguments: match command.get_str("arguments") {
                        Dynamic::Array(arguments) => Some(arguments.to_vec()),
                        _ => None,
                    },
                })
            }
            _ => None,
        };
        let id = match value.get_str("id") {
            Dynamic::String(id) => Some(id.to_string_lossy()),
            _ => None,
        };
        Some(EditorCodeLens { range: Range::from_dto(&value.get_str("range"))?, id, command, cache_id: None })
    }
}
