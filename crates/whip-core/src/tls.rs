//! A rustls connector for ureq that offers ALPN `http/1.1`.
//!
//! ureq's own rustls connector sends no ALPN extension, and servers that
//! require one refuse the handshake. Apart from ALPN this mirrors ureq's
//! default: webpki roots, ring.

use std::io::{Read, Write};
use std::sync::{Arc, LazyLock};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use ureq::config::Config;
use ureq::unversioned::resolver::DefaultResolver;
use ureq::unversioned::transport::{
    Buffers, ConnectProxyConnector, ConnectionDetails, Connector, Either, LazyBuffers, NextTimeout,
    TcpConnector, Transport, TransportAdapter,
};
use ureq::{Agent, Error};

/// The only protocol ureq speaks.
const ALPN_HTTP11: &[u8] = b"http/1.1";

static CONFIG: LazyLock<Arc<ClientConfig>> = LazyLock::new(|| {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN_HTTP11.to_vec()];
    Arc::new(config)
});

/// An agent using `config` whose HTTPS connections offer ALPN `http/1.1`.
pub(crate) fn agent(config: Config) -> Agent {
    let connector =
        ().chain(ConnectProxyConnector::default())
            .chain(TcpConnector::default())
            .chain(AlpnRustlsConnector);
    Agent::with_parts(config, connector, DefaultResolver::default())
}

#[derive(Debug)]
struct AlpnRustlsConnector;

impl<In: Transport> Connector<In> for AlpnRustlsConnector {
    type Out = Either<In, RustlsTransport>;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, Error> {
        let Some(transport) = chained else {
            return Ok(None);
        };
        if !details.needs_tls() || transport.is_tls() {
            return Ok(Some(Either::A(transport)));
        }

        let host = details.uri.host().ok_or(Error::Tls("no host in URL"))?;
        // IPv6 literals arrive bracketed.
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let name = ServerName::try_from(host.to_string())
            .map_err(|_| Error::Tls("invalid TLS server name"))?;

        let mut conn = ClientConnection::new(CONFIG.clone(), name)?;
        let mut sock = TransportAdapter::new(transport.boxed(), details.current_time.clone());
        sock.set_timeout(details.timeout);
        conn.complete_io(&mut sock)?;

        let buffers = LazyBuffers::new(
            details.config.input_buffer_size(),
            details.config.output_buffer_size(),
        );
        Ok(Some(Either::B(RustlsTransport {
            buffers,
            stream: StreamOwned { conn, sock },
        })))
    }
}

struct RustlsTransport {
    buffers: LazyBuffers,
    stream: StreamOwned<ClientConnection, TransportAdapter>,
}

impl std::fmt::Debug for RustlsTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RustlsTransport").finish_non_exhaustive()
    }
}

impl Transport for RustlsTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), Error> {
        self.stream.get_mut().set_timeout(timeout);
        let output = &self.buffers.output()[..amount];
        self.stream.write_all(output)?;
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, Error> {
        self.stream.get_mut().set_timeout(timeout);
        let input = self.buffers.input_append_buf();
        let amount = self.stream.read(input)?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.stream.get_mut().get_mut().is_open()
    }

    fn is_tls(&self) -> bool {
        true
    }
}
