//! Porte de `src/vs/platform/sign/common/sign.ts`: o contrato do serviço que assina o desafio do handshake com o
//! agente remoto (`ISignService`) e a mensagem que ele cria (`IMessage`).
//!
//! O original só declara a interface; quem a implementa (o `SignService` do Electron, que usa o módulo nativo `vsda`,
//! e o da web) fica fora deste porte.

use je_base::promise::Promise;
use je_instantiation::service_id;

/// `SIGN_SERVICE_ID`.
pub const SIGN_SERVICE_ID: &str = "signService";

/// `IMessage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IMessage {
    pub id: String,
    pub data: String,
}

/// `ISignService`.
pub trait ISignService {
    /// `createNewMessage(value)`.
    fn create_new_message(&self, value: &str) -> Promise<IMessage>;
    /// `validate(message, value)`.
    fn validate(&self, message: &IMessage, value: &str) -> Promise<bool>;
    /// `sign(value)`.
    fn sign(&self, value: &str) -> Promise<String>;
}

service_id!(pub I_SIGN_SERVICE: dyn ISignService = "signService");
