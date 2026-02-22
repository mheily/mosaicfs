use std::fs;
use std::io::BufReader;
use std::path::Path;

use rcgen::{CertificateParams, KeyPair, IsCa, BasicConstraints};
use rustls::ServerConfig;
use tracing::info;

/// Ensure TLS certificates exist, generating self-signed ones if needed.
/// Returns a rustls ServerConfig.
pub fn ensure_tls_certs(data_dir: &Path) -> anyhow::Result<ServerConfig> {
    let ca_cert_path = data_dir.join("ca.crt");
    let server_cert_path = data_dir.join("server.crt");
    let server_key_path = data_dir.join("server.key");

    if !ca_cert_path.exists() || !server_cert_path.exists() || !server_key_path.exists() {
        info!("Generating self-signed TLS certificates");
        generate_certs(data_dir)?;
    }

    // Load server cert and key
    let cert_pem = fs::read(&server_cert_path)?;
    let key_pem = fs::read(&server_key_path)?;

    let certs: Vec<rustls::pki_types::CertificateDer<'static>> = {
        let mut reader = BufReader::new(&cert_pem[..]);
        rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()?
    };

    let key = {
        let mut reader = BufReader::new(&key_pem[..]);
        rustls_pemfile::private_key(&mut reader)?
            .ok_or_else(|| anyhow::anyhow!("No private key found in server.key"))?
    };

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(config)
}

fn generate_certs(data_dir: &Path) -> anyhow::Result<()> {
    // Generate CA
    let ca_key_pair = KeyPair::generate()?;
    let mut ca_params = CertificateParams::new(vec!["MosaicFS CA".to_string()])?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca_params.self_signed(&ca_key_pair)?;

    // Generate server cert signed by CA
    let server_key_pair = KeyPair::generate()?;
    let mut server_params = CertificateParams::new(vec![
        "localhost".to_string(),
    ])?;
    server_params.subject_alt_names = vec![
        rcgen::SanType::DnsName("localhost".try_into()?),
        rcgen::SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
    ];
    let server_cert = server_params.signed_by(&server_key_pair, &ca_cert, &ca_key_pair)?;

    // Write files
    fs::write(data_dir.join("ca.crt"), ca_cert.pem())?;
    fs::write(data_dir.join("ca.key"), ca_key_pair.serialize_pem())?;
    fs::write(data_dir.join("server.crt"), server_cert.pem())?;
    fs::write(data_dir.join("server.key"), server_key_pair.serialize_pem())?;

    info!("TLS certificates generated");
    Ok(())
}
