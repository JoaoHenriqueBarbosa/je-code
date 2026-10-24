//! Porte parcial de `src/vs/server/node/serverEnvironmentService.ts`: o `ServerParsedArgs` e o
//! `getRedactedServerParsedArgs`. O `serverOptions` (a descrição de cada opção da linha de comando) e o
//! `ServerEnvironmentService` entram com o porte do servidor e do `parseArgs` do servidor.
//!
//! Divergências:
//! - `ServerParsedArgs extends NativeParsedArgs` é o próprio [`NativeParsedArgs`], o envoltório do objeto dinâmico do
//!   `parseArgs`: as opções do servidor (`connection-token`, `without-connection-token`, `user-data-dir`...) são chaves
//!   do mesmo objeto, lidas com `get_string` e `get_bool`.
//! - `{ ...args }` (a cópia rasa) é [`copy_args`]: um objeto novo com as mesmas chaves, na mesma ordem.

use je_base::JsString;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_platform::environment::native_parsed_args::NativeParsedArgs;

/// `ServerParsedArgs`.
pub type ServerParsedArgs = NativeParsedArgs;

/// `{ ...args }`: a cópia rasa dos argumentos.
pub fn copy_args(args: &ServerParsedArgs) -> ServerParsedArgs {
    let Dynamic::Object(object) = args.to_dynamic() else {
        return ServerParsedArgs::default();
    };
    ServerParsedArgs::from_dynamic(&Dynamic::Object(ObjectRef::from_entries(object.entries())))
}

/// Returns server arguments with connection tokens redacted for logging.
pub fn get_redacted_server_parsed_args(args: &ServerParsedArgs) -> ServerParsedArgs {
    let redacted_args = copy_args(args);
    for key in ["connection-token", "agent-host-bridge-connection-token"] {
        if !matches!(redacted_args.get(key), Dynamic::Undefined) {
            redacted_args.set(key, Dynamic::from(JsString::from("<redacted>")));
        }
    }
    redacted_args
}
