//! Porte de `src/vs/platform/userDataSync/test/common/userDataSyncStoreService.test.ts` (suítes
//! `UserDataSyncStoreService` e `UserDataSyncRequestsSession`), caso a caso e na mesma ordem.
//!
//! Divergências: os testes que o original roda sem `runWithFakedTimers` rodam também sob o relógio virtual de
//! [`async_test`] (nenhum depende de tempo real: os que esperam o fim do `retry-after` usam o `timeout` virtual
//! ou o evento). `isWeb` é falso, então o `X-Client-Name` é só o `applicationName`. `createInstance(UserDataSyncStoreService)`
//! é [`UserDataSyncStoreService::new_service`] com os serviços do cliente. O `IRequestService` mudo da suíte de
//! sessão devolve uma resposta vazia (`newWriteableBufferStream()` é um fluxo de zero bytes). `target.clear()` é
//! [`UserDataSyncTestServer::clear_data`] e o `retryAfter` fracionário é [`UserDataSyncTestServer::with_rate_limit_seconds`].

mod common;

use std::rc::Rc;

use common::files::async_test;
use common::user_data_sync_client::{RecordedRequestWithAllHeaders, UserDataSyncClient, UserDataSyncTestServer};
use je_base::async_::timeout;
use je_base::buffer::{VSBuffer, buffer_to_stream};
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::DisposableStore;
use je_base::parts::request::{HeaderValue, IHeaders, IRequestContext, IRequestOptions, IRequestResponse};
use je_base::promise::Promise;
use je_platform::log::NullLogService;
use je_platform::product::IProductService;
use je_platform::request::{AuthInfo, Credentials, IRequestService, RequestCompleteEvent};
use je_platform::user_data_sync::user_data_sync::{IUserDataSyncStoreService, ServerResource, SyncResource, UserDataSyncErrorCode, UserDataSyncStoreError};
use je_platform::user_data_sync::user_data_sync_store_service::{RequestsSession, UserDataSyncStoreClientServices, UserDataSyncStoreService};

const MACHINE_SESSION: &str = "X-Machine-Session-Id";
const USER_SESSION: &str = "X-User-Session-Id";

/// `target.requestsWithAllHeaders[0].headers![name]`.
fn header_of(request: &RecordedRequestWithAllHeaders, name: &str) -> Option<String> {
    request.headers.as_ref().and_then(|headers| headers.get(name)).and_then(|value| match value {
        HeaderValue::One(text) => Some(text.clone()),
        HeaderValue::Many(_) => None,
    })
}

fn first_request(target: &UserDataSyncTestServer) -> RecordedRequestWithAllHeaders {
    target.requests_with_all_headers().into_iter().next().expect("há ao menos uma requisição")
}

/// `new UserDataSyncTestServer()`, o cliente e o `setUp()`.
async fn set_up(d: &Rc<DisposableStore>, target: Rc<UserDataSyncTestServer>) -> Rc<UserDataSyncClient> {
    d.add_rc(UserDataSyncClient::set_up(target, false).await.unwrap())
}

async fn manifest(client: &UserDataSyncClient) {
    client.user_data_sync_store_service.manifest(None, None).await.unwrap();
}

async fn write_settings(client: &UserDataSyncClient) {
    client.user_data_sync_store_service.write_resource(ServerResource::from(SyncResource::Settings), "some content", None, None, None).await.unwrap();
}

/// `client.instantiationService.createInstance(UserDataSyncStoreService)`.
fn create_store_service(client: &UserDataSyncClient) -> Rc<UserDataSyncStoreService> {
    UserDataSyncStoreService::new_service(
        client.user_data_sync_store_management_service.clone(),
        UserDataSyncStoreClientServices {
            product_service: client.product_service.clone(),
            request_service: client.test_server.clone(),
            log_service: NullLogService::shared(),
            environment_service: client.environment_service.clone(),
            file_service: client.file_service.clone(),
            storage_service: client.storage_service.clone(),
        },
    )
}

#[test]
fn read_manifest_for_the_first_time() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_eq!(header_of(&request, "X-Client-Name"), Some(client.product_service.application_name().to_string()));
        assert_eq!(header_of(&request, "X-Client-Version"), Some(client.product_service.version().to_string()));
        assert_ne!(header_of(&request, MACHINE_SESSION), None);
        assert_eq!(header_of(&request, USER_SESSION), None);
    });
}

#[test]
fn read_manifest_for_the_second_time_when_session_is_not_yet_created() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;
        let machine_session_id = header_of(&first_request(&target), MACHINE_SESSION);

        target.reset();
        manifest(&client).await;

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_eq!(header_of(&request, MACHINE_SESSION), machine_session_id);
        assert_eq!(header_of(&request, USER_SESSION), None);
    });
}

#[test]
fn session_id_header_is_not_set_in_the_first_manifest_request_after_session_is_created() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;
        let machine_session_id = header_of(&first_request(&target), MACHINE_SESSION);
        write_settings(&client).await;

        target.reset();
        manifest(&client).await;

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_eq!(header_of(&request, MACHINE_SESSION), machine_session_id);
        assert_eq!(header_of(&request, USER_SESSION), None);
    });
}

#[test]
fn session_id_header_is_set_from_the_second_manifest_request_after_session_is_created() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;
        let machine_session_id = header_of(&first_request(&target), MACHINE_SESSION);
        write_settings(&client).await;
        manifest(&client).await;

        target.reset();
        manifest(&client).await;

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_eq!(header_of(&request, MACHINE_SESSION), machine_session_id);
        assert_ne!(header_of(&request, USER_SESSION), None);
    });
}

#[test]
fn headers_are_send_for_write_request() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;
        let machine_session_id = header_of(&first_request(&target), MACHINE_SESSION);
        write_settings(&client).await;
        manifest(&client).await;
        manifest(&client).await;

        target.reset();
        write_settings(&client).await;

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_eq!(header_of(&request, MACHINE_SESSION), machine_session_id);
        assert_ne!(header_of(&request, USER_SESSION), None);
    });
}

#[test]
fn headers_are_send_for_read_request() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;
        let machine_session_id = header_of(&first_request(&target), MACHINE_SESSION);
        write_settings(&client).await;
        manifest(&client).await;
        manifest(&client).await;

        target.reset();
        client.user_data_sync_store_service.read_resource(ServerResource::from(SyncResource::Settings), None, None, None).await.unwrap();

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_eq!(header_of(&request, MACHINE_SESSION), machine_session_id);
        assert_ne!(header_of(&request, USER_SESSION), None);
    });
}

#[test]
fn headers_are_reset_after_session_is_cleared() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;
        let machine_session_id = header_of(&first_request(&target), MACHINE_SESSION);
        write_settings(&client).await;
        manifest(&client).await;
        manifest(&client).await;
        client.user_data_sync_store_service.clear().await.unwrap();

        target.reset();
        manifest(&client).await;

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_ne!(header_of(&request, MACHINE_SESSION), None);
        assert_ne!(header_of(&request, MACHINE_SESSION), machine_session_id);
        assert_eq!(header_of(&request, USER_SESSION), None);
    });
}

#[test]
fn old_headers_are_sent_after_session_is_changed_on_server() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;
        write_settings(&client).await;
        manifest(&client).await;
        target.reset();
        manifest(&client).await;
        let machine_session_id = header_of(&first_request(&target), MACHINE_SESSION);
        let user_session_id = header_of(&first_request(&target), USER_SESSION);
        target.clear_data();

        // client 2
        let client2 = set_up(&d, target.clone()).await;
        write_settings(&client2).await;

        target.reset();
        manifest(&client).await;

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_ne!(header_of(&request, MACHINE_SESSION), None);
        assert_eq!(header_of(&request, MACHINE_SESSION), machine_session_id);
        assert_ne!(header_of(&request, USER_SESSION), None);
        assert_eq!(header_of(&request, USER_SESSION), user_session_id);
    });
}

#[test]
fn old_headers_are_reset_from_second_request_after_session_is_changed_on_server() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;
        write_settings(&client).await;
        manifest(&client).await;
        target.reset();
        manifest(&client).await;
        let machine_session_id = header_of(&first_request(&target), MACHINE_SESSION);
        let user_session_id = header_of(&first_request(&target), USER_SESSION);
        target.clear_data();

        // client 2
        let client2 = set_up(&d, target.clone()).await;
        write_settings(&client2).await;

        manifest(&client).await;
        target.reset();
        manifest(&client).await;

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_ne!(header_of(&request, MACHINE_SESSION), None);
        assert_ne!(header_of(&request, MACHINE_SESSION), machine_session_id);
        assert_ne!(header_of(&request, USER_SESSION), None);
        assert_ne!(header_of(&request, USER_SESSION), user_session_id);
    });
}

#[test]
fn old_headers_are_sent_after_session_is_cleared_from_another_server() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;
        write_settings(&client).await;
        manifest(&client).await;
        target.reset();
        manifest(&client).await;
        let machine_session_id = header_of(&first_request(&target), MACHINE_SESSION);
        let user_session_id = header_of(&first_request(&target), USER_SESSION);

        // client 2
        let client2 = set_up(&d, target.clone()).await;
        client2.user_data_sync_store_service.clear().await.unwrap();

        target.reset();
        manifest(&client).await;

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_ne!(header_of(&request, MACHINE_SESSION), None);
        assert_eq!(header_of(&request, MACHINE_SESSION), machine_session_id);
        assert_ne!(header_of(&request, USER_SESSION), None);
        assert_eq!(header_of(&request, USER_SESSION), user_session_id);
    });
}

#[test]
fn headers_are_reset_after_session_is_cleared_from_another_server() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;
        write_settings(&client).await;
        manifest(&client).await;
        target.reset();
        manifest(&client).await;
        let machine_session_id = header_of(&first_request(&target), MACHINE_SESSION);

        // client 2
        let client2 = set_up(&d, target.clone()).await;
        client2.user_data_sync_store_service.clear().await.unwrap();

        manifest(&client).await;
        target.reset();
        manifest(&client).await;

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_ne!(header_of(&request, MACHINE_SESSION), None);
        assert_ne!(header_of(&request, MACHINE_SESSION), machine_session_id);
        assert_eq!(header_of(&request, USER_SESSION), None);
    });
}

#[test]
fn headers_are_reset_after_session_is_cleared_from_another_server_started_syncing_again() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target.clone()).await;

        manifest(&client).await;
        write_settings(&client).await;
        manifest(&client).await;
        target.reset();
        manifest(&client).await;
        let machine_session_id = header_of(&first_request(&target), MACHINE_SESSION);
        let user_session_id = header_of(&first_request(&target), USER_SESSION);

        // client 2
        let client2 = set_up(&d, target.clone()).await;
        client2.user_data_sync_store_service.clear().await.unwrap();

        manifest(&client).await;
        write_settings(&client).await;
        manifest(&client).await;
        target.reset();
        manifest(&client).await;

        assert_eq!(target.requests_with_all_headers().len(), 1);
        let request = first_request(&target);
        assert_ne!(header_of(&request, MACHINE_SESSION), None);
        assert_ne!(header_of(&request, MACHINE_SESSION), machine_session_id);
        assert_ne!(header_of(&request, USER_SESSION), user_session_id);
        assert_ne!(header_of(&request, USER_SESSION), None);
    });
}

#[test]
fn rate_limit_on_server_with_retry_after() {
    async_test(|d| async move {
        let target = Rc::new(UserDataSyncTestServer::with_rate_limit(1, Some(1)));
        let client = set_up(&d, target).await;
        let test_object = &client.user_data_sync_store_service;

        manifest(&client).await;

        let promise = test_object.on_did_change_donot_make_requests_until().to_promise(None);
        let error = test_object.manifest(None, None).await.expect_err("should fail");
        let store_error = UserDataSyncStoreError::from_error(&error).expect("é um UserDataSyncStoreError");
        assert_eq!(store_error.error.code, UserDataSyncErrorCode::TooManyRequestsAndRetryAfter);
        promise.await.unwrap();
        assert!(test_object.donot_make_requests_until().is_some());
    });
}

#[test]
fn donot_make_requests_until_is_reset_after_retry_time_is_finished() {
    async_test(|d| async move {
        let client = set_up(&d, Rc::new(UserDataSyncTestServer::with_rate_limit_seconds(1, Some(0.25)))).await;
        let test_object = &client.user_data_sync_store_service;

        manifest(&client).await;
        assert!(test_object.manifest(None, None).await.is_err(), "should fail");

        let promise = test_object.on_did_change_donot_make_requests_until().to_promise(None);
        timeout(300).await.unwrap();
        promise.await.unwrap();
        assert!(test_object.donot_make_requests_until().is_none());
    });
}

#[test]
fn donot_make_requests_until_is_retrieved() {
    async_test(|d| async move {
        let client = set_up(&d, Rc::new(UserDataSyncTestServer::with_rate_limit(1, Some(1)))).await;
        let test_object = &client.user_data_sync_store_service;

        manifest(&client).await;
        let _ = test_object.manifest(None, None).await;

        let target = d.add_rc(create_store_service(&client));
        assert_eq!(target.donot_make_requests_until(), test_object.donot_make_requests_until());
    });
}

#[test]
fn donot_make_requests_until_is_checked_and_reset_after_retreived() {
    async_test(|d| async move {
        let client = set_up(&d, Rc::new(UserDataSyncTestServer::with_rate_limit_seconds(1, Some(0.25)))).await;
        let test_object = &client.user_data_sync_store_service;

        manifest(&client).await;
        assert!(test_object.manifest(None, None).await.is_err(), "should fail");

        timeout(300).await.unwrap();
        let target = d.add_rc(create_store_service(&client));
        assert!(target.donot_make_requests_until().is_none());
    });
}

#[test]
fn read_resource_request_handles_304() {
    async_test(|d| async move {
        // Setup the client
        let target = Rc::new(UserDataSyncTestServer::new());
        let client = set_up(&d, target).await;
        client.sync().await.unwrap();

        let test_object = &client.user_data_sync_store_service;
        let resource = ServerResource::from(SyncResource::Settings);
        let expected = test_object.read_resource(resource, None, None, None).await.unwrap();
        let actual = test_object.read_resource(resource, Some(expected.clone()), None, None).await.unwrap();

        assert_eq!(actual, expected);
    });
}

/// O `IRequestService` da suíte `UserDataSyncRequestsSession`: responde sempre com uma resposta vazia.
struct EmptyRequestService;

impl IRequestService for EmptyRequestService {
    fn on_did_complete_request(&self) -> Event<RequestCompleteEvent> {
        Event::none()
    }

    fn request(&self, _options: IRequestOptions, _token: CancellationToken) -> Promise<IRequestContext> {
        Promise::resolved(IRequestContext { res: IRequestResponse { headers: IHeaders::new(), status_code: None }, stream: buffer_to_stream(VSBuffer::from_string("")), timings: None })
    }

    fn resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None)
    }

    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        Promise::resolved(None)
    }

    fn lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None)
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        Promise::resolved(Vec::new())
    }
}

/// `{ callSite: 'test' }`.
fn test_options() -> IRequestOptions {
    IRequestOptions { call_site: "test".to_string(), ..IRequestOptions::default() }
}

/// `testObject.request('url', { callSite: 'test' }, CancellationToken.None)`.
async fn request(test_object: &RequestsSession) -> Result<IRequestContext, je_base::errors::Error> {
    test_object.request("url", test_options(), CancellationToken::None)?.await
}

fn assert_local_too_many_requests(error: &je_base::errors::Error) {
    let store_error = UserDataSyncStoreError::from_error(error).expect("é um UserDataSyncStoreError");
    assert_eq!(store_error.error.code, UserDataSyncErrorCode::LocalTooManyRequests);
}

#[test]
fn too_many_requests_are_thrown_when_limit_exceeded() {
    async_test(|_d| async move {
        let test_object = RequestsSession::new(1, 500, Rc::new(EmptyRequestService), NullLogService::shared());
        request(&test_object).await.unwrap();

        match request(&test_object).await {
            Err(error) => assert_local_too_many_requests(&error),
            Ok(_) => panic!("Should fail with limit exceeded"),
        }
    });
}

#[test]
fn requests_are_handled_after_session_is_expired() {
    async_test(|_d| async move {
        let test_object = RequestsSession::new(1, 100, Rc::new(EmptyRequestService), NullLogService::shared());
        request(&test_object).await.unwrap();
        timeout(125).await.unwrap();
        request(&test_object).await.unwrap();
    });
}

#[test]
fn too_many_requests_are_thrown_after_session_is_expired() {
    async_test(|_d| async move {
        let test_object = RequestsSession::new(1, 100, Rc::new(EmptyRequestService), NullLogService::shared());
        request(&test_object).await.unwrap();
        timeout(125).await.unwrap();
        request(&test_object).await.unwrap();

        match request(&test_object).await {
            Err(error) => assert_local_too_many_requests(&error),
            Ok(_) => panic!("Should fail with limit exceeded"),
        }
    });
}
