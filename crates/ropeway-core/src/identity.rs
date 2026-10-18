//! Device identity: a persistent self-signed certificate.
//!
//! The device id is a SHA-256 fingerprint of the certificate. Peers pin it, so
//! a trusted device cannot be impersonated by another machine on the network:
//! the TLS handshake proves possession of the matching private key.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};

const CERT_FILE: &str = "identity.cert.der";
const KEY_FILE: &str = "identity.key.der";

pub struct Identity {
    pub cert: CertificateDer<'static>,
    pub key: PrivatePkcs8KeyDer<'static>,
    pub id: String,
}

impl Identity {
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        let cert_path = dir.join(CERT_FILE);
        let key_path = dir.join(KEY_FILE);

        let (cert, key) = if cert_path.exists() && key_path.exists() {
            (
                fs::read(&cert_path).context("reading device certificate")?,
                fs::read(&key_path).context("reading device key")?,
            )
        } else {
            let key_pair = rcgen::KeyPair::generate()?;
            let mut params = rcgen::CertificateParams::new(vec!["ropeway.local".to_string()])?;
            params
                .distinguished_name
                .push(rcgen::DnType::CommonName, "Ropeway device");
            let cert = params.self_signed(&key_pair)?;
            let cert = cert.der().to_vec();
            let key = key_pair.serialize_der();
            fs::write(&key_path, &key).context("saving device key")?;
            fs::write(&cert_path, &cert).context("saving device certificate")?;
            (cert, key)
        };

        let id = fingerprint(&cert);
        Ok(Self {
            cert: CertificateDer::from(cert),
            key: PrivatePkcs8KeyDer::from(key),
            id,
        })
    }
}

/// Device id: first 128 bits of the certificate's SHA-256, hex encoded.
pub fn fingerprint(cert_der: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, cert_der);
    hex::encode(&digest.as_ref()[..16])
}

/// Human-friendly code that two people can compare to verify a device,
/// e.g. `3FA2-91C0-7B1E-04D9` (64 bits: too many to forge a matching one).
pub fn short_code(id: &str) -> String {
    let upper = id.to_uppercase();
    let chars: Vec<char> = upper.chars().take(16).collect();
    chars
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// Ids arrive from the network and are used in file names, so they must be
/// plain lowercase hex.
pub fn is_valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
