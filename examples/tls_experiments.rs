use std::sync::Arc;

use rcgen::{CertifiedKey, generate_simple_self_signed};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{
    TlsAcceptor, TlsConnector,
    rustls::{
        ClientConfig, RootCertStore, ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, ServerName},
    },
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["127.0.0.1".to_string()])?;
    let server_cert_der = CertificateDer::from(cert);
    let chain = vec![server_cert_der.clone()];
    let key = PrivateKeyDer::from(signing_key);

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(chain, key)?;
    let acceptor = TlsAcceptor::from(Arc::new(config));

    let addr = "127.0.0.1:6969";
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();

            let stream = acceptor.accept(stream).await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0; 10];
                let (mut r_stream, mut w_stream) = tokio::io::split(stream);
                let Ok(n) = r_stream.read(&mut buf[..]).await else {
                    return;
                };
                let _ = w_stream.write_all(&buf[..n]).await;
            });
        }
    });

    let mut store = RootCertStore::empty();
    store.add(server_cert_der)?;

    let client_config = ClientConfig::builder()
        .with_root_certificates(store)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(client_config));
    let ip: std::net::IpAddr = "127.0.0.1".parse().unwrap();

    let stream = tokio::net::TcpStream::connect(addr).await?;
    let mut tls_stream = connector
        .connect(ServerName::IpAddress(ip.into()), stream)
        .await?;

    tls_stream.write_all(b"ping").await?;
    let mut buf = [0; 10];
    let n = tls_stream.read(&mut buf[..]).await?;
    println!("{}", String::from_utf8_lossy(&buf[..n]));
    Ok(())
}
