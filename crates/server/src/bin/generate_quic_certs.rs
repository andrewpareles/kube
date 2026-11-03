use anyhow::Context;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use rcgen::generate_simple_self_signed;
use shared::protocol::quic_cert::QUIC_CERT_SERVER_NAME;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let certs_dir_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../certs");
    std::fs::create_dir_all(&certs_dir_path).with_context(|| format!("Failed to create {}", certs_dir_path.display()))?;

    let cert_der_path = certs_dir_path.join("quic_server_cert.der");

    let rcgen::CertifiedKey { cert, key_pair } = generate_simple_self_signed(vec![QUIC_CERT_SERVER_NAME.to_string()])?;
    std::fs::write(&cert_der_path, cert.der().as_ref()).with_context(|| format!("Failed to write {}", cert_der_path.display()))?;

    // The key is a secret, so it is printed instead of written into the repo.
    println!("Wrote {}", cert_der_path.display());
    println!("Set this as QUIC_SERVER_KEY_DER_BASE64 in Infisical's Production /vercel:");
    println!("{}", BASE64_STANDARD.encode(key_pair.serialize_der()));
    println!("The desktop client trusts the bundled cert, so ship a client update after rotating it.");
    Ok(())
}
