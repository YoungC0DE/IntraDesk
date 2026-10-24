use crate::ResultType;
use rustls_pki_types::{ServerName, UnixTime};
use std::sync::Arc;
use tokio_rustls::rustls::{self, client::WebPkiServerVerifier, ClientConfig};
use tokio_rustls::rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    DigitallySignedStruct, Error as TLSError, SignatureScheme,
};

// https://github.com/seanmonstar/reqwest/blob/fd61bc93e6f936454ce0b978c6f282f06eee9287/src/tls.rs#L608
#[derive(Debug)]
pub(crate) struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls_pki_types::CertificateDer,
        _intermediates: &[rustls_pki_types::CertificateDer],
        _server_name: &ServerName,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TLSError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls_pki_types::CertificateDer,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TLSError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls_pki_types::CertificateDer,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TLSError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA1,
            SignatureScheme::ECDSA_SHA1_Legacy,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}




fn webpki_server_verifier(
    provider: Arc<rustls::crypto::CryptoProvider>,
) -> ResultType<Arc<WebPkiServerVerifier>> {
    // Load root certificates from both bundled webpki_roots and system-native certificate stores.
    // This approach is consistent with how reqwest and tokio-tungstenite handle root certificates.
    // https://github.com/snapview/tokio-tungstenite/blob/35d110c24c9d030d1608ec964d70c789dfb27452/src/tls.rs#L95
    // https://github.com/seanmonstar/reqwest/blob/b126ca49da7897e5d676639cdbf67a0f6838b586/src/async_impl/client.rs#L643
    let mut root_cert_store = rustls::RootCertStore::empty();
    root_cert_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let rustls_native_certs::CertificateResult { certs, errors, .. } =
        rustls_native_certs::load_native_certs();
    if !errors.is_empty() {
        log::warn!("native root CA certificate loading errors: {errors:?}");
    }
    root_cert_store.add_parsable_certificates(certs);

    // Build verifier using with_root_certificates behavior (WebPkiServerVerifier without CRLs).
    // Both reqwest and tokio-tungstenite use this approach.
    // https://github.com/seanmonstar/reqwest/blob/b126ca49da7897e5d676639cdbf67a0f6838b586/src/async_impl/client.rs#L749
    // https://github.com/snapview/tokio-tungstenite/blob/35d110c24c9d030d1608ec964d70c789dfb27452/src/tls.rs#L127
    // https://github.com/rustls/rustls/blob/1ee126adb3352a2dcd72420dcd6040351a6ddc1e/rustls/src/client/builder.rs#L47
    // with_root_certificates creates a WebPkiServerVerifier without revocation checking:
    // https://github.com/rustls/rustls/blob/1ee126adb3352a2dcd72420dcd6040351a6ddc1e/rustls/src/webpki/server_verifier.rs#L177
    // https://github.com/rustls/rustls/blob/1ee126adb3352a2dcd72420dcd6040351a6ddc1e/rustls/src/webpki/server_verifier.rs#L168
    // Since no CRL is provided (as is the case here), we must explicitly set allow_unknown_revocation_status()
    // to match the behavior of with_root_certificates, which allows unknown revocation status by default.
    // https://github.com/rustls/rustls/blob/1ee126adb3352a2dcd72420dcd6040351a6ddc1e/rustls/src/webpki/server_verifier.rs#L37
    // Note: build() only returns an error if the root certificate store is empty, which won't happen here.
    let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
        Arc::new(root_cert_store),
        provider.clone(),
    )
    .allow_unknown_revocation_status()
    .build()
    .map_err(|e| anyhow::anyhow!(e))?;
    Ok(verifier)
}

pub fn client_config(danger_accept_invalid_cert: bool) -> ResultType<ClientConfig> {
    if danger_accept_invalid_cert {
        client_config_danger()
    } else {
        client_config_safe()
    }
}

pub fn client_config_safe() -> ResultType<ClientConfig> {
    // Use the default builder which uses the default protocol versions and crypto provider.
    // The with_protocol_versions API has been removed in rustls master branch:
    // https://github.com/rustls/rustls/pull/2599
    // This approach is consistent with tokio-tungstenite's usage:
    // https://github.com/snapview/tokio-tungstenite/blob/35d110c24c9d030d1608ec964d70c789dfb27452/src/tls.rs#L126
    let config_builder = rustls::ClientConfig::builder();
    let provider = config_builder.crypto_provider().clone();
    let webpki_verifier = webpki_server_verifier(provider.clone())?;
    {
        let config = config_builder
            .with_webpki_verifier(webpki_verifier)
            .with_no_client_auth();
        Ok(config)
    }
}

pub fn client_config_danger() -> ResultType<ClientConfig> {
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();
    Ok(config)
}
