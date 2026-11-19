//! HTTPS for `ragon web` with the node's own certificate (specs/DREAMFERENCE_AUTH.md §6).
//!
//! `ragon-admin` makes the node's CA and a server certificate signed by it in
//! `~/.local/share/dreamference/tls` (the gate's `ModelGateTls`); the gate on port 8000 and this
//! server serve the same certificate. One port serves both: the first byte of a connection tells a
//! TLS ClientHello (0x16, a handshake record) from plain HTTP, which stays served in 0.9.9 as on the
//! gate. Without the files the server is plain HTTP, exactly as before.

use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::extract::ConnectInfo;
use axum::extract::Request;
use axum::serve::{IncomingStream, Listener};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

/// The node's certificate files, under the home directory (`model_gate_tls.py`'s `TLS_DIR`).
pub const TLS_DIR: &str = ".local/share/dreamference/tls";
const SERVER_CERT: &str = "server.pem";
const SERVER_KEY: &str = "server.key";
/// The first byte of a TLS handshake record.
const HANDSHAKE_RECORD: u8 = 0x16;
/// How long a connection may stay silent before its first byte, and how long a handshake may take.
const SNIFF_TIMEOUT: Duration = Duration::from_secs(30);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// Connections sorted but not yet taken by the server.
const BACKLOG: usize = 64;

/// The server's TLS configuration from `dir`, or None when the node has no certificate there.
pub fn load(dir: &Path) -> io::Result<Option<Arc<rustls::ServerConfig>>> {
    let (cert, key) = (dir.join(SERVER_CERT), dir.join(SERVER_KEY));
    if !cert.is_file() || !key.is_file() {
        return Ok(None);
    }
    let invalid = |err: &dyn std::fmt::Display| io::Error::new(io::ErrorKind::InvalidData, err.to_string());
    let chain = CertificateDer::pem_file_iter(&cert)
        .map_err(|err| invalid(&err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| invalid(&err))?;
    let key = PrivateKeyDer::from_pem_file(&key).map_err(|err| invalid(&err))?;
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|err| invalid(&err))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|err| invalid(&err))?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Some(Arc::new(config)))
}

/// A connection the server reads: plain HTTP or HTTPS.
pub enum Io {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Io {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Io::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            Io::Tls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Io {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Io::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            Io::Tls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Io::Plain(stream) => Pin::new(stream).poll_flush(cx),
            Io::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Io::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            Io::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
        }
    }
}

/// A listener that hands the server each connection after its first byte has said what it is.
///
/// Sorting and handshakes run in their own tasks, so a silent client or a broken handshake holds
/// up no other connection; the accept loop ends when the listener is dropped.
pub struct SniffingListener {
    ready: mpsc::Receiver<(Io, SocketAddr)>,
    local: SocketAddr,
    accepting: tokio::task::JoinHandle<()>,
}

impl SniffingListener {
    pub fn new(listener: TcpListener, config: Arc<rustls::ServerConfig>) -> io::Result<Self> {
        let local = listener.local_addr()?;
        let acceptor = TlsAcceptor::from(config);
        let (sorted, ready) = mpsc::channel(BACKLOG);
        let accepting = tokio::spawn(async move {
            loop {
                let (stream, peer) = match listener.accept().await {
                    Ok(accepted) => accepted,
                    Err(_) => {
                        // Out of descriptors, or a connection reset before it was taken: as axum does.
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                if sorted.is_closed() {
                    return;
                }
                let (sorted, acceptor) = (sorted.clone(), acceptor.clone());
                tokio::spawn(async move {
                    if let Some(io) = sort(stream, &acceptor).await {
                        let _ = sorted.send((io, peer)).await;
                    }
                });
            }
        });
        Ok(Self { ready, local, accepting })
    }
}

impl Drop for SniffingListener {
    fn drop(&mut self) {
        self.accepting.abort();
    }
}

/// The connection as plain HTTP or after its handshake; None if it closed, stayed silent or failed.
async fn sort(stream: TcpStream, acceptor: &TlsAcceptor) -> Option<Io> {
    let mut first = [0u8; 1];
    match tokio::time::timeout(SNIFF_TIMEOUT, stream.peek(&mut first)).await {
        Ok(Ok(1..)) => {}
        _ => return None,
    }
    if first[0] != HANDSHAKE_RECORD {
        return Some(Io::Plain(stream));
    }
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
        Ok(Ok(tls)) => Some(Io::Tls(Box::new(tls))),
        _ => None,
    }
}

impl Listener for SniffingListener {
    type Io = Io;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(connection) => connection,
            // The accept loop never ends while the listener lives.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local)
    }
}

/// The peer's address for the router, as `ConnectInfo<SocketAddr>` gives it on a plain listener.
#[derive(Clone, Copy, Debug)]
pub struct Peer(pub SocketAddr);

impl axum::extract::connect_info::Connected<IncomingStream<'_, SniffingListener>> for Peer {
    fn connect_info(stream: IncomingStream<'_, SniffingListener>) -> Self {
        Peer(*stream.remote_addr())
    }
}

/// Copies the peer's address into the `ConnectInfo<SocketAddr>` a plain listener would give.
pub async fn peer_as_socket_addr(mut request: Request) -> Request {
    if let Some(ConnectInfo(Peer(addr))) = request.extensions().get::<ConnectInfo<Peer>>().copied() {
        request.extensions_mut().insert(ConnectInfo(addr));
    }
    request
}
