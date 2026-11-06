mod config;
mod driver;
#[cfg(feature = "server")]
pub(crate) mod listener;
pub(crate) mod stream;

pub use config::KcpConfig;

const CHUNK: usize = 16 * 1024;
const BUFFER: usize = 64 * 1024;
#[cfg(any(feature = "server", test))]
const PACKETS: usize = 128;
#[cfg(test)]
const HELLO: &[u8] = b"\0lynn-kcp/1";

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(crate) enum Mode {
    Plain,
    #[cfg(feature = "kcp-tls")]
    Tls,
}

impl Mode {
    fn hello(self) -> &'static [u8] {
        match self {
            Self::Plain => b"\0lynn-kcp/1",
            #[cfg(feature = "kcp-tls")]
            Self::Tls => b"\0lynn-kcp/2",
        }
    }
}

#[cfg(all(test, feature = "kcp-tls"))]
mod tls_tests;

#[cfg(feature = "kcp-tls")]
pub(crate) const TLS_READY: &[u8] = b"\0lynn-kcp-tls/ready";
