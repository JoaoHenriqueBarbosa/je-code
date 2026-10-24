//! Porte de `src/vs/workbench/services/policies/test/browser/managedSettingsUpdate.test.ts` (suíte `Managed settings update presentation`), caso a caso e na mesma ordem.
//!
//! **Nenhum dos 14 casos está portado** (nada vira `#[test]`, para o placar não contar o que não bate com o oráculo):
//! - Todos testam a apresentação do bloqueio "your organization requires an update to use AI features" do cliente
//!   (`IManagedSettingsCompatibilityError.minimumClientVersion`): a `ManagedSettingsUpdateContribution`, que publica um banner
//!   e controla a view do Chat (`ManagedSettingsUpdateRequiredContext`, `MANAGED_SETTINGS_UPDATE_VIEW_ID =
//!   'workbench.panel.chat.updateRequired'`), o `ChatAIDisabledSettingId`, a janela de Agents e a ajuda de acessibilidade do
//!   Chat e do overlay de Agents. Chat, agentes de IA e Agents estão fora do escopo do projeto (CLAUDE.md), e o que sobra
//!   (`getManagedSettingsUpdateInfo`, `ManagedSettingsUpdateService`) só existe para alimentar esses consumidores.
//! - Os quatro primeiros casos (`identifies the requesting product...`, `uses actionable update states...`, `all disabled
//!   updater reasons...`, `reads initially blocked state and recomputes...`) usam só `getManagedSettingsUpdateInfo` e o
//!   `ManagedSettingsUpdateService`, mas dependem de `IObservable`/`autorun` do `base/common/observable`, do `State` e do
//!   `DisablementReason` de `platform/update/common/update.ts` e do `IManagedSettingsCompatibilityError` de
//!   `platform/defaultAccount`, e a mensagem é a de "AI features". Sem o contrato de update portado e sem o chat, a
//!   fidelidade não é verificável, e a suíte entra junto com o porte do `update.ts`.
