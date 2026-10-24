//! Porte da lógica de `src/vs/editor/contrib/codeAction`: os tipos (`common/types.ts`, [`types`]), a coleta e a
//! aplicação das ações (`browser/codeAction.ts`, [`code_action`]), o modelo e o oráculo
//! (`browser/codeActionModel.ts`, [`code_action_model`]) e o resolvedor de atalhos
//! (`browser/codeActionKeybindingResolver.ts`, [`code_action_keybinding_resolver`]).
//!
//! Ficam como especificação, sem porte: `codeActionMenu.ts`, `lightBulbWidget.ts` e a parte de `codeActionController.ts`
//! que desenha o menu e a lâmpada (DOM e `ActionWidget`), `codeActionCommands.ts` e `codeActionContributions.ts` (as
//! ações do editor, o registro de comandos e de contribuições, que dependem do controlador).
//!
//! Os tipos de `src/vs/editor/common/languages.ts` que a lógica consome moram em [`languages`], e os recortes de
//! `platform` e de `editor/browser/services` que o porte ainda não tem (progresso, `ActionSet`, edição em lote e sinal
//! de acessibilidade) em [`platform`]. O registro de provedores é o
//! [`LanguageFeatureRegistry`](crate::language_feature_registry::LanguageFeatureRegistry).

pub mod code_action;
pub mod code_action_keybinding_resolver;
pub mod code_action_model;
pub mod languages;
pub mod platform;
pub mod types;
