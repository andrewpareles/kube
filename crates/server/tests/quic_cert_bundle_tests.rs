use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use quinn::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use shared::env::env_vars::QUIC_SERVER_KEY_DER_BASE64;
use shared::protocol::quic_cert::QUIC_SERVER_CERT_DER_BYTES;

// Needs the real key: infisical run --env=dev -- cargo test -p server --test quic_cert_bundle_tests
#[test]
fn quic_cert_and_key_build_a_server_config() {
    let cert_chain = vec![CertificateDer::from(QUIC_SERVER_CERT_DER_BYTES)];
    let key_der_bytes = BASE64_STANDARD
        .decode(QUIC_SERVER_KEY_DER_BASE64.as_str())
        .expect("QUIC_SERVER_KEY_DER_BASE64 should be base64");
    let private_key = PrivateKeyDer::try_from(key_der_bytes).expect("the private key should be valid der");
    ServerConfig::with_single_cert(cert_chain, private_key).expect("the quic cert and key should build a server config");
}
