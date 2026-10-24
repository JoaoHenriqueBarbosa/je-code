//! Teste de ponta a ponta de `vscode.authentication` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada pelo host
//! local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), pede sessões e contas, escuta `onDidChangeSessions` e
//! registra um provedor de autenticação próprio; o efeito aparece no cliente `MainThreadAuthentication` do workbench.
//!
//! O lado do ExtHost é o `ExtHostAuthentication` REAL (portado de `extHostAuthentication.ts`), ligado pelo host em processo. Não existe
//! ainda um `MainThreadAuthentication` em `crates/je-workbench/src/api/browser/` (ele depende do `AuthenticationService`, do
//! armazenamento de segredos e das notificações do workbench), então o lado do workbench é um dublê do `MainThreadAuthenticationShape`
//! ([`Panel`]) que anota cada chamada (`$ensureProvider`, `$getSession`, `$getAccounts`, `$registerAuthenticationProvider`,
//! `$sendDidChangeSessions`, `$unregisterAuthenticationProvider`) e faz o papel da thread principal quando o teste manda: pede ao ExtHost
//! o que a thread principal real pediria (`$getSessions`, `$createSession`, `$removeSession`, `$onDidChangeAuthenticationSessions`) pelo
//! proxy `ExtHostAuthenticationShape` que o contexto em processo entrega.

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::RefCell;
use std::rc::Rc;

use harness::{run_extension, settled_ok, Workbench};
use je_base::dynamic::Dynamic;
use je_base::js_string::JsString;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::UriComponents;
use je_exthost::in_process::IMainThreadBinder;
use je_workbench::api::common::ext_host_authentication::IAuthorizationToken;
use je_workbench::api::common::ext_host_protocol::{
    AuthenticationGetSessionOptions, AuthenticationInteractive, AuthenticationSessionAccountDto, AuthenticationSessionDto, AuthenticationSessionsChangeEventDto,
    ClientRegistration, DynamicProviderInfoChange, ExtHostAuthenticationShape, ExtHostContext, IMainContext, IRegisterAuthenticationProviderDetails,
    IRegisterDynamicAuthenticationProviderDetails, MainContext, MainThreadAuthenticationShape,
};
use je_workbench::services::authentication::authentication::{
    AuthenticationScopeListOrRequest, AuthenticationSessionAccount, IAuthenticationCreateSessionOptions, IAuthenticationGetSessionsOptions,
};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const log = (text) => globalThis.__events.push(text);
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  const describe = (session) => JSON.stringify({
    id: session.id,
    token: session.accessToken,
    account: session.account.label + ':' + session.account.id,
    icon: session.account.icon.path,
    scopes: session.scopes,
    idToken: session.idToken,
    server: session.authorizationServer.authority,
  });

  command('t.getSession', async () => {
    const session = await vscode.authentication.getSession('github', ['repo', 'user'], {
      createIfNone: { detail: 'Preciso do GitHub' },
      clearSessionPreference: true,
      account: { id: 'a1', label: 'Ana' },
    });
    return describe(session);
  });

  command('t.getSessionTwice', async () => {
    const [first, second] = await Promise.all([
      vscode.authentication.getSession('github', ['b', 'a'], { silent: true }),
      vscode.authentication.getSession('github', ['a', 'b'], { silent: true }),
    ]);
    return first.id + ',' + second.id;
  });

  command('t.getSessionNone', async () => String(await vscode.authentication.getSession('github', ['repo'], { silent: true })));

  command('t.getAccounts', async () => {
    const accounts = await vscode.authentication.getAccounts('github');
    return JSON.stringify(accounts.map((account) => account.label + ':' + account.id + ':' + (account.icon ? account.icon.path : '-')));
  });

  command('t.proposals', async () => {
    const attempt = async (call) => {
      try { await call(); return 'sem erro'; } catch (error) { return error.message.split('\n')[0]; }
    };
    const provider = { onDidChangeSessions: new vscode.EventEmitter().event, getSessions: async () => [], createSession: async () => ({}), removeSession: async () => {} };
    return JSON.stringify([
      await attempt(() => vscode.authentication.getSession('github', ['repo'], { clientId: 'x' })),
      await attempt(() => vscode.authentication.getSession('github', ['repo'], { audience: 'y' })),
      await attempt(() => vscode.authentication.getSession('github', ['repo'], { createIfNone: { learnMore: vscode.Uri.parse('https://example.com/mais') } })),
      await attempt(() => vscode.authentication.registerAuthenticationProvider('p', 'P', provider, { supportedAuthorizationServers: [vscode.Uri.parse('https://a.example.com')] })),
      await attempt(() => vscode.authentication.registerAuthenticationProvider('p', 'P', {})),
    ]);
  });

  command('t.listen', () => {
    context.subscriptions.push(vscode.authentication.onDidChangeSessions((event) => log('change:' + event.provider.id + ':' + event.provider.label)));
    return 'ok';
  });

  const makeProvider = (name, emitter) => {
    const sessions = [{ id: 'p1', accessToken: 'secret-1', account: { id: 'u1', label: 'Uma' }, scopes: ['read'] }];
    class Provider {
      constructor() {
        this.onDidChangeSessions = emitter.event;
        this.prefix = 'cls';
      }
      async getSessions(scopes, options) {
        log(name + ':getSessions:' + (scopes ? scopes.join(',') : 'todas') + ':' + (options.account ? options.account.id : '-'));
        return name === 'first' ? sessions : [];
      }
      async createSession(scopes, options) {
        log(name + ':createSession:' + scopes.join(',') + ':' + this.prefix);
        const session = { id: 'p2', accessToken: 'secret-2', account: { id: 'u2', label: 'Dois', icon: vscode.Uri.parse('https://example.com/dois.png') }, scopes: [...scopes] };
        sessions.push(session);
        return session;
      }
      async removeSession(id) {
        log(name + ':removeSession:' + id);
      }
    }
    return new Provider();
  };

  command('t.register', () => {
    globalThis.__emitter = new vscode.EventEmitter();
    globalThis.__registration = vscode.authentication.registerAuthenticationProvider('je-provider', 'Je Provider', makeProvider('first', globalThis.__emitter), { supportsMultipleAccounts: true });
    return 'ok';
  });

  command('t.registerAndFire', () => {
    const emitter = new vscode.EventEmitter();
    globalThis.__registration = vscode.authentication.registerAuthenticationProvider('je-provider', 'Je Provider', makeProvider('first', emitter));
    emitter.fire({ added: [{ id: 'p0', accessToken: 'secret-0', account: { id: 'u0', label: 'Zero' }, scopes: [] }], removed: undefined, changed: undefined });
    return 'ok';
  });

  command('t.registerTwice', () => {
    globalThis.__registration = vscode.authentication.registerAuthenticationProvider('je-provider', 'Je Provider', makeProvider('first', new vscode.EventEmitter()));
    globalThis.__second = vscode.authentication.registerAuthenticationProvider('je-provider', 'Je Provider 2', makeProvider('second', new vscode.EventEmitter()));
    return 'ok';
  });

  command('t.fire', () => {
    globalThis.__emitter.fire({ added: [{ id: 'p3', accessToken: 'secret-3', account: { id: 'u3', label: 'Tres' }, scopes: ['read'] }], removed: [], changed: undefined });
    return 'ok';
  });

  command('t.unregister', () => {
    globalThis.__registration.dispose();
    return 'ok';
  });

  command('t.unregisterSecond', () => {
    globalThis.__second.dispose();
    return 'ok';
  });
};
"#;

/// `{ scheme, authority, path }` com a consulta e o fragmento ausentes.
fn components(scheme: &str, authority: &str, path: &str) -> UriComponents {
    UriComponents {
        scheme: Some(JsString::from(scheme)),
        authority: Some(JsString::from(authority)),
        path: Some(JsString::from(path)),
        query: None,
        fragment: None,
    }
}

fn github_session() -> AuthenticationSessionDto {
    AuthenticationSessionDto {
        id: "s1".to_owned(),
        access_token: "tok-1".to_owned(),
        account: AuthenticationSessionAccountDto { label: "Ana".to_owned(), id: "a1".to_owned(), icon: Some(components("https", "example.com", "/avatar.png")) },
        scopes: vec!["repo".to_owned(), "user".to_owned()],
        id_token: Some("idt".to_owned()),
        expires_after: None,
        authorization_server: Some(components("https", "auth.example.com", "")),
    }
}

/// Um resumo (em texto) das opções que `$getSession` carregou: só as chaves presentes, na ordem do protocolo.
fn describe_options(options: &AuthenticationGetSessionOptions) -> String {
    let interactive = |name: &str, value: &Option<AuthenticationInteractive>| match value {
        None => None,
        Some(AuthenticationInteractive::Flag(flag)) => Some(format!("{name}={flag}")),
        Some(AuthenticationInteractive::Options(options)) => Some(format!("{name}=detail:{}", options.detail.clone().unwrap_or_default())),
    };
    let mut parts: Vec<String> = Vec::new();
    parts.extend(options.silent.map(|silent| format!("silent={silent}")));
    parts.extend(interactive("createIfNone", &options.create_if_none));
    parts.extend(interactive("forceNewSession", &options.force_new_session));
    parts.extend(options.clear_session_preference.map(|clear| format!("clearSessionPreference={clear}")));
    parts.extend(options.account.as_ref().map(|account| format!("account={}/{}", account.id, account.label)));
    parts.join(",")
}

fn describe_ids(sessions: &Option<Vec<AuthenticationSessionDto>>) -> String {
    sessions.as_ref().map_or_else(|| "-".to_owned(), |sessions| sessions.iter().map(|session| session.id.clone()).collect::<Vec<_>>().join(","))
}

/// O papel da thread principal: o `MainThreadAuthenticationShape` que anota as chamadas.
struct Panel {
    ext_host: RefCell<Option<Rc<dyn ExtHostAuthenticationShape>>>,
    log: RefCell<Vec<String>>,
    /// O que `$getSession` responde.
    session: RefCell<Option<AuthenticationSessionDto>>,
    /// O que `$getAccounts` responde.
    accounts: RefCell<Vec<AuthenticationSessionAccountDto>>,
}

impl Panel {
    fn new() -> Rc<Self> {
        Rc::new(Self { ext_host: RefCell::new(None), log: RefCell::new(Vec::new()), session: RefCell::new(None), accounts: RefCell::new(Vec::new()) })
    }

    fn ext_host(&self) -> Rc<dyn ExtHostAuthenticationShape> {
        self.ext_host.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    fn note(&self, line: String) {
        self.log.borrow_mut().push(line);
    }

    fn lines(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

impl IDisposable for Panel {
    fn dispose(&self) {}
}

impl MainThreadAuthenticationShape for Panel {
    fn rpc_register_authentication_provider(&self, details: IRegisterAuthenticationProviderDetails) -> Promise<()> {
        self.note(format!("register:{}:{}:multiple={}", details.id, details.label, details.supports_multiple_accounts));
        Promise::resolved(())
    }

    fn rpc_unregister_authentication_provider(&self, id: &str) -> Promise<()> {
        self.note(format!("unregister:{id}"));
        Promise::resolved(())
    }

    fn rpc_ensure_provider(&self, id: &str) -> Promise<()> {
        self.note(format!("ensure:{id}"));
        Promise::resolved(())
    }

    fn rpc_send_did_change_sessions(&self, provider_id: &str, event: AuthenticationSessionsChangeEventDto) -> Promise<()> {
        self.note(format!(
            "sessionsChanged:{provider_id}:added={},removed={},changed={}",
            describe_ids(&event.added),
            describe_ids(&event.removed),
            describe_ids(&event.changed)
        ));
        Promise::resolved(())
    }

    fn rpc_get_session(
        &self,
        provider_id: &str,
        scope_list_or_request: AuthenticationScopeListOrRequest,
        extension_id: &str,
        extension_name: &str,
        options: AuthenticationGetSessionOptions,
    ) -> Promise<Option<AuthenticationSessionDto>> {
        let scopes = match &scope_list_or_request {
            AuthenticationScopeListOrRequest::Scopes(scopes) => scopes.join(","),
            AuthenticationScopeListOrRequest::Request(request) => format!("www:{}", request.www_authenticate),
        };
        self.note(format!("getSession:{provider_id}:{scopes}:{extension_id}:{extension_name}:{}", describe_options(&options)));
        Promise::resolved(self.session.borrow().clone())
    }

    fn rpc_get_accounts(&self, provider_id: &str) -> Promise<Vec<AuthenticationSessionAccountDto>> {
        self.note(format!("getAccounts:{provider_id}"));
        Promise::resolved(self.accounts.borrow().clone())
    }

    fn rpc_remove_session(&self, _provider_id: &str, _session_id: &str) -> Promise<()> {
        panic!("o teste não remove sessões pela thread principal")
    }

    fn rpc_wait_for_uri_handler(&self, _expected_uri: UriComponents) -> Promise<UriComponents> {
        panic!("o teste não espera o tratador de URI")
    }

    fn rpc_show_continue_notification(&self, _message: &str) -> Promise<bool> {
        panic!("o teste não mostra a notificação de continuar")
    }

    fn rpc_show_device_code_modal(&self, _user_code: &str, _verification_uri: &str) -> Promise<bool> {
        panic!("o teste não mostra o modal de código de dispositivo")
    }

    fn rpc_prompt_for_client_registration(&self, _authorization_server_url: &str) -> Promise<Option<ClientRegistration>> {
        panic!("o teste não pede o registro do cliente")
    }

    fn rpc_prompt_for_resource_client_secret(&self, _resource_client_id: &str, _resource: &str) -> Promise<Option<String>> {
        panic!("o teste não pede o segredo do cliente do recurso")
    }

    fn rpc_register_dynamic_authentication_provider(&self, _details: IRegisterDynamicAuthenticationProviderDetails) -> Promise<()> {
        panic!("o teste não registra provedores dinâmicos")
    }

    fn rpc_set_sessions_for_dynamic_auth_provider(&self, _auth_provider_id: &str, _client_id: &str, _sessions: Vec<IAuthorizationToken>) -> Promise<()> {
        panic!("o teste não grava sessões de provedores dinâmicos")
    }

    fn rpc_send_did_change_dynamic_provider_info(&self, _info: DynamicProviderInfoChange) -> Promise<()> {
        panic!("o teste não muda a informação de provedores dinâmicos")
    }
}

/// Liga o dublê da thread principal ao host.
struct Binder {
    panel: Rc<Panel>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        *self.panel.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_AUTHENTICATION));
        let panel: Rc<dyn MainThreadAuthenticationShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_AUTHENTICATION, panel);
        Vec::new()
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Rc<Panel>) {
    let panel = Panel::new();
    (Rc::new(Binder { panel: panel.clone() }), panel)
}

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

fn events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events)")
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn get_session_forwards_the_request_and_revives_the_session() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        panel.session.replace(Some(github_session()));
        assert_eq!(
            run(workbench, "t.getSession"),
            Dynamic::string(r#"{"id":"s1","token":"tok-1","account":"Ana:a1","icon":"/avatar.png","scopes":["repo","user"],"idToken":"idt","server":"auth.example.com"}"#)
        );
        assert_eq!(
            panel.lines(),
            strings(&[
                "ensure:github",
                "getSession:github:repo,user:je.exthost:exthost:createIfNone=detail:Preciso do GitHub,clearSessionPreference=true,account=a1/Ana",
            ])
        );
    });
}

#[test]
fn get_session_shares_the_in_flight_request_with_the_same_key() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        panel.session.replace(Some(github_session()));
        assert_eq!(run(workbench, "t.getSessionTwice"), Dynamic::string("s1,s1"));
        assert_eq!(
            panel.lines(),
            strings(&["ensure:github", "getSession:github:b,a:je.exthost:exthost:silent=true"]),
            "os escopos em outra ordem são a mesma chave: uma só chamada à thread principal"
        );

        run(workbench, "t.getSessionTwice");
        assert_eq!(panel.lines().len(), 4, "terminada a tarefa, a mesma chave pede de novo");
    });
}

#[test]
fn get_session_resolves_undefined_when_the_main_thread_has_no_session() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(run(workbench, "t.getSessionNone"), Dynamic::string("undefined"));
        assert_eq!(panel.lines(), strings(&["ensure:github", "getSession:github:repo:je.exthost:exthost:silent=true"]));
    });
}

#[test]
fn get_session_and_register_reject_options_that_need_a_proposal() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(
            run(workbench, "t.proposals"),
            Dynamic::string(concat!(
                r#"["Extension 'je.exthost' CANNOT use API proposal: authIssuers.","#,
                r#""Extension 'je.exthost' CANNOT use API proposal: authSessionAudience.","#,
                r#""Extension 'je.exthost' CANNOT use API proposal: authLearnMore.","#,
                r#""Extension 'je.exthost' CANNOT use API proposal: authIssuers.","#,
                r#""registerAuthenticationProvider: o provedor precisa de getSessions"]"#
            ))
        );
        assert_eq!(panel.lines(), Vec::<String>::new(), "nada chega à thread principal");
    });
}

#[test]
fn get_accounts_revives_the_account_icons() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        panel.accounts.replace(vec![
            AuthenticationSessionAccountDto { label: "Ana".to_owned(), id: "a1".to_owned(), icon: Some(components("https", "example.com", "/avatar.png")) },
            AuthenticationSessionAccountDto { label: "Bia".to_owned(), id: "b2".to_owned(), icon: None },
        ]);
        assert_eq!(run(workbench, "t.getAccounts"), Dynamic::string(r#"["Ana:a1:/avatar.png","Bia:b2:-"]"#));
        assert_eq!(panel.lines(), strings(&["ensure:github", "getAccounts:github"]));
    });
}

#[test]
fn on_did_change_sessions_honors_the_extension_filter_and_skips_internal_providers() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.listen");
        let host = panel.ext_host();
        let notices: [(&str, &str, Option<Vec<String>>); 4] = [
            ("github", "GitHub", None),
            ("github", "GitHub", Some(strings(&["other.ext"]))),
            ("github2", "GitHub 2", Some(strings(&["je.exthost"]))),
            ("__internal", "Interno", None),
        ];
        for (id, label, filter) in notices {
            let notified = host.rpc_on_did_change_authentication_sessions(id, label, filter);
            workbench.pump();
            settled_ok(&notified, "onDidChangeAuthenticationSessions");
        }
        assert_eq!(events(workbench), r#"["change:github:GitHub","change:github2:GitHub 2"]"#);
    });
}

#[test]
fn registered_provider_answers_the_main_thread_and_forwards_its_session_changes() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.register");
        assert_eq!(panel.lines(), strings(&["register:je-provider:Je Provider:multiple=true"]));
        let host = panel.ext_host();

        let options = IAuthenticationGetSessionsOptions { account: Some(AuthenticationSessionAccount::new("Uma", "u1")), ..IAuthenticationGetSessionsOptions::default() };
        let listed = host.rpc_get_sessions("je-provider", Some(strings(&["read"])), options);
        workbench.pump();
        let listed = settled_ok(&listed, "getSessions");
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].id.as_str(), listed[0].access_token.as_str(), listed[0].account.label.as_str()), ("p1", "secret-1", "Uma"));
        assert_eq!(listed[0].scopes, strings(&["read"]));

        let created = host.rpc_create_session("je-provider", strings(&["read", "write"]), IAuthenticationCreateSessionOptions::default());
        workbench.pump();
        let created = settled_ok(&created, "createSession");
        assert_eq!((created.id.as_str(), created.account.id.as_str()), ("p2", "u2"));
        let icon = created.account.icon.as_ref().expect("o ícone da conta chega como URI");
        assert_eq!(icon.to_string(false).to_string_lossy(), "https://example.com/dois.png");

        let removed = host.rpc_remove_session("je-provider", "p2");
        workbench.pump();
        settled_ok(&removed, "removeSession");
        assert_eq!(events(workbench), r#"["first:getSessions:read:u1","first:createSession:read,write:cls","first:removeSession:p2"]"#, "o this da classe do provedor vale");

        run(workbench, "t.fire");
        assert_eq!(panel.lines()[1], "sessionsChanged:je-provider:added=p3,removed=,changed=-");

        run(workbench, "t.unregister");
        assert_eq!(panel.lines()[2..].to_vec(), strings(&["unregister:je-provider"]));
        run(workbench, "t.fire");
        assert_eq!(panel.lines().len(), 3, "descartado o registro, o provedor deixa de avisar a thread principal");

        let missing = host.rpc_get_sessions("je-provider", None, IAuthenticationGetSessionsOptions::default());
        workbench.pump();
        match missing.result() {
            Some(Err(error)) => assert_eq!(error.message, "Unable to find authentication provider with handle: je-provider"),
            _ => panic!("pedir sessões de um provedor descartado deveria rejeitar"),
        }
    });
}

#[test]
fn session_changes_fired_right_after_registering_wait_for_the_main_thread_acknowledgement() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.registerAndFire");
        assert_eq!(
            panel.lines(),
            strings(&["register:je-provider:Je Provider:multiple=false", "sessionsChanged:je-provider:added=p0,removed=-,changed=-"]),
            "o evento disparado antes do `$registerAuthenticationProvider` ser reconhecido sai depois dele"
        );
    });
}

#[test]
fn a_second_registration_with_the_same_id_is_ignored() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.registerTwice");
        assert_eq!(panel.lines(), strings(&["register:je-provider:Je Provider:multiple=false"]), "o primeiro vence");

        let listed = panel.ext_host().rpc_get_sessions("je-provider", None, IAuthenticationGetSessionsOptions::default());
        workbench.pump();
        assert_eq!(settled_ok(&listed, "getSessions").len(), 1);
        assert_eq!(events(workbench), r#"["first:getSessions:todas:-"]"#);

        run(workbench, "t.unregisterSecond");
        assert_eq!(panel.lines().len(), 1, "descartar o registro ignorado não desregistra o provedor que venceu");
    });
}
