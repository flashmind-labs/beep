//! Android JNI boundary. TLS verification is always enabled, including development.
use anyhow::{Context, Result};
use bytes::Bytes;
use jni::{
    JNIEnv,
    objects::{JClass, JObject, JString, JValue},
    sys::{jint, jlong},
};
use neobeep_quic_client::Client;
use std::{
    collections::HashMap,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

static CALLS: OnceLock<Mutex<HashMap<i64, CancellationToken>>> = OnceLock::new();
static NEXT_ID: AtomicI64 = AtomicI64::new(1);
fn calls() -> &'static Mutex<HashMap<i64, CancellationToken>> {
    CALLS.get_or_init(Default::default)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_flashmind_neobeep_NativeQuic_createCall(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    calls().lock().unwrap().insert(id, CancellationToken::new());
    id
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_flashmind_neobeep_NativeQuic_cancel(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
) {
    if let Some(token) = calls().lock().unwrap().get(&id) {
        token.cancel();
    }
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_com_flashmind_neobeep_NativeQuic_request(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    address: JString,
    hostname: JString,
    ca: JString,
    token: JString,
    method: JString,
    path: JString,
    body: JString,
    callback: JObject,
) -> jint {
    let result: Result<u16> = (|| {
        let address: String = env.get_string(&address)?.into();
        let hostname: String = env.get_string(&hostname)?.into();
        let ca: String = env.get_string(&ca)?.into();
        let token: String = env.get_string(&token)?.into();
        let method: String = env.get_string(&method)?.into();
        let path: String = env.get_string(&path)?.into();
        let body: String = env.get_string(&body)?.into();
        let cancel = calls()
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .context("unknown request")?;
        let _ = rustls::crypto::ring::default_provider().install_default();
        static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
        let runtime = RUNTIME.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("Tokio runtime")
        });
        runtime.block_on(async {
            let operation = async {
                let certs = rustls_pemfile::certs(&mut ca.as_bytes()).collect::<std::io::Result<Vec<_>>>()?;
                anyhow::ensure!(ca.trim().is_empty() || !certs.is_empty(),"certificate file contains no PEM certificates");
                let addr = tokio::net::lookup_host(&address).await?.next().context("server address did not resolve")?;
                let mut client = Client::connect(addr,&hostname,certs).await?;
                let mut callback_error = None;
                let status = client.request(&method,&path,&token,(!body.is_empty()).then(||Bytes::from(body)),|chunk| {
                    if callback_error.is_some() { return; }
                    let result: jni::errors::Result<()> = (|| {
                        let bytes = env.byte_array_from_slice(chunk)?;
                        let result = env.call_method(&callback,"onChunk","([B)V",&[JValue::Object(&bytes)]);
                        env.delete_local_ref(bytes)?;
                        result?;
                        Ok(())
                    })();
                    if let Err(error) = result { callback_error = Some(error); }
                }).await?;
                if let Some(error) = callback_error { return Err(error.into()); }
                Ok(status.as_u16())
            };
            tokio::select! {
                biased;
                _ = cancel.cancelled() => anyhow::bail!("request cancelled"),
                result = tokio::time::timeout(Duration::from_secs(45),operation) => result.context("QUIC request timed out")?,
            }
        })
    })();
    calls().lock().unwrap().remove(&id);
    match result {
        Ok(status) => i32::from(status),
        Err(error) => {
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/io/IOException", format!("{error:#}"));
            }
            -1
        }
    }
}
