//! Porte de `src/vs/workbench/contrib/externalUriOpener`: a configuração `workbench.externalUriOpeners`, o armazém dos openers
//! contribuídos por extensões e o serviço que escolhe o opener de um endereço (`common`). O `externalUriOpener.contribution.ts`
//! (registro do serviço e do nó de configuração) entra com o registro de contribuições do workbench.

pub mod common;
