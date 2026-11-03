// accessed by both server and client
// if you change this, you must regenerate the certs
pub const QUIC_CERT_SERVER_NAME: &str = "kube-new-certname";

// public (accessed by client)
pub const QUIC_SERVER_CERT_DER_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../certs/quic_server_cert.der"));

// the matching private key is a secret, servers read it secretly from QUIC_SERVER_KEY_DER_BASE64
