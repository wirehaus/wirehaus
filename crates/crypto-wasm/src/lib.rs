#![allow(unsafe_code)]

mod core;
mod qr;

use js_sys::Array;
use wasm_bindgen::prelude::*;
use wirehaus_crypto::handshake::ViewerHandshake;
use wirehaus_crypto::session::Session as CoreSession;

fn failure(error: &impl std::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}

#[wasm_bindgen(js_name = linkGenerate)]
pub fn link_generate() -> Result<Vec<u8>, JsError> {
    let (private, public) = wirehaus_crypto::link::generate().map_err(|e| failure(&e))?;
    let mut out = Vec::with_capacity(private.len() + public.len());
    out.extend_from_slice(&private);
    out.extend_from_slice(&public);
    Ok(out)
}

#[wasm_bindgen(js_name = linkOpen)]
pub fn link_open(private: &[u8], sealed: &[u8]) -> Result<String, JsError> {
    let plain = wirehaus_crypto::link::open(private, sealed).map_err(|e| failure(&e))?;
    String::from_utf8(plain).map_err(|e| failure(&e))
}

#[wasm_bindgen(js_name = linkVerification)]
#[must_use]
pub fn link_verification(public: &[u8], id: &[u8]) -> String {
    wirehaus_crypto::link::verification_code(public, id)
}

#[wasm_bindgen(js_name = qrMatrix)]
pub fn qr_matrix(text: &str) -> Result<Vec<u8>, JsError> {
    qr::matrix(text).map_err(|e| JsError::new(&e))
}

#[wasm_bindgen(js_name = qrDecode)]
#[must_use]
pub fn qr_decode(width: u32, height: u32, luma: &[u8]) -> Option<String> {
    qr::decode(width as usize, height as usize, luma)
}

#[wasm_bindgen(js_name = generateDeviceKey)]
pub fn generate_device_key() -> Result<Vec<u8>, JsError> {
    core::generate_device_key().map_err(|e| failure(&e))
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct Handshake {
    pending: Option<ViewerHandshake>,
    first: Vec<u8>,
}

#[wasm_bindgen]
impl Handshake {
    pub fn reconnect(device_key: &[u8], agent_public: &[u8]) -> Result<Handshake, JsError> {
        let (pending, first) =
            core::reconnect(device_key, agent_public).map_err(|e| failure(&e))?;
        Ok(Handshake {
            pending: Some(pending),
            first,
        })
    }

    pub fn pairing(device_key: &[u8], code: &str, name: &str) -> Result<Handshake, JsError> {
        let (pending, first) = core::pairing(device_key, code, name).map_err(|e| failure(&e))?;
        Ok(Handshake {
            pending: Some(pending),
            first,
        })
    }

    #[wasm_bindgen(getter, js_name = firstMessage)]
    #[must_use]
    pub fn first_message(&self) -> Vec<u8> {
        self.first.clone()
    }

    pub fn complete(&mut self, reply: &[u8]) -> Result<Outcome, JsError> {
        let pending = self
            .pending
            .take()
            .ok_or_else(|| JsError::new("the handshake was already completed"))?;
        let done = core::complete(pending, reply).map_err(|e| failure(&e))?;
        Ok(Outcome {
            session: Some(Session {
                inner: done.session,
            }),
            last_message: done.last_message,
        })
    }
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct Outcome {
    session: Option<Session>,
    last_message: Option<Vec<u8>>,
}

#[wasm_bindgen]
impl Outcome {
    #[wasm_bindgen(getter, js_name = lastMessage)]
    #[must_use]
    pub fn last_message(&self) -> Option<Vec<u8>> {
        self.last_message.clone()
    }

    #[wasm_bindgen(js_name = takeSession)]
    pub fn take_session(&mut self) -> Result<Session, JsError> {
        self.session
            .take()
            .ok_or_else(|| JsError::new("the session was already taken"))
    }
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct Session {
    inner: CoreSession,
}

#[wasm_bindgen]
impl Session {
    pub fn seal(&mut self, message: &[u8]) -> Result<Array, JsError> {
        let records = self.inner.seal(message).map_err(|e| failure(&e))?;
        Ok(records
            .iter()
            .map(|record| js_sys::Uint8Array::from(record.as_slice()))
            .collect())
    }

    pub fn open(&mut self, record: &[u8]) -> Result<Option<Vec<u8>>, JsError> {
        self.inner.open(record).map_err(|e| failure(&e))
    }

    #[wasm_bindgen(getter, js_name = remotePublicKey)]
    #[must_use]
    pub fn remote_public_key(&self) -> Vec<u8> {
        self.inner.remote().as_bytes().to_vec()
    }
}
