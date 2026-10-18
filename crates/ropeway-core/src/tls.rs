//! QUIC/TLS configuration with certificate pinning instead of a CA.
//!
//! Every device presents its self-signed certificate (mutual TLS). The
//! verifiers below accept any certificate but still check the handshake
//! signatures, so the peer must own the private key. The caller then derives
//! the peer's id from the certificate and, when dialing a known device,
//! requires that it matches.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};

use crate::identity::{fingerprint, Identity};
use crate::protocol::ALPN;

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn transport() -> Arc<quinn::TransportConfig> {
    let mut t = quinn::TransportConfig::default();
    t.keep_alive_interval(Some(Duration::from_secs(4)));
    t.max_idle_timeout(Some(
        Duration::from_secs(15)
            .try_into()
            .expect("idle timeout fits"),
    ));
    t.max_concurrent_bidi_streams(256u32.into());
    t.max_concurrent_uni_streams(256u32.into());
    // Room for LAN-speed file transfers (the defaults cap throughput).
    t.stream_receive_window((16u32 * 1024 * 1024).into());
    t.receive_window((64u32 * 1024 * 1024).into());
    t.send_window(64 * 1024 * 1024);
    Arc::new(t)
}

pub fn server_config(identity: &Identity, provider: Arc<CryptoProvider>) -> Result<quinn::ServerConfig> {
    let mut crypto = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(Arc::new(AnyClient { provider }))
        .with_single_cert(
            vec![identity.cert.clone()],
            PrivateKeyDer::Pkcs8(identity.key.clone_key()),
        )?;
    crypto.alpn_protocols = vec![ALPN.to_vec()];
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(crypto)?));
    config.transport_config(transport());
    Ok(config)
}

/// Client config for one outgoing connection. With `expected_id` set, the
/// handshake fails unless the server's certificate has that fingerprint.
pub fn client_config(
    identity: &Identity,
    provider: Arc<CryptoProvider>,
    expected_id: Option<&str>,
) -> Result<quinn::ClientConfig> {
    let verifier = PinnedServer {
        provider: provider.clone(),
        expected_id: expected_id.map(str::to_owned),
    };
    let mut crypto = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_client_auth_cert(
            vec![identity.cert.clone()],
            PrivateKeyDer::Pkcs8(identity.key.clone_key()),
        )?;
    crypto.alpn_protocols = vec![ALPN.to_vec()];
    let mut config = quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?));
    config.transport_config(transport());
    Ok(config)
}

/// Device id of the remote side of an established connection.
pub fn peer_id(conn: &quinn::Connection) -> Result<String> {
    let identity = conn
        .peer_identity()
        .ok_or_else(|| anyhow!("peer presented no certificate"))?;
    let certs = identity
        .downcast::<Vec<CertificateDer<'static>>>()
        .map_err(|_| anyhow!("unexpected peer identity type"))?;
    let cert = certs.first().ok_or_else(|| anyhow!("empty certificate chain"))?;
    Ok(fingerprint(cert))
}

#[derive(Debug)]
struct PinnedServer {
    provider: Arc<CryptoProvider>,
    expected_id: Option<String>,
}

impl ServerCertVerifier for PinnedServer {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if let Some(expected) = &self.expected_id {
            if fingerprint(end_entity) != *expected {
                return Err(rustls::Error::General("device fingerprint mismatch".into()));
            }
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

#[derive(Debug)]
struct AnyClient {
    provider: Arc<CryptoProvider>,
}

impl ClientCertVerifier for AnyClient {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}
