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
const HELLO: &[u8] = b"\0lynn-kcp/1";

#[cfg(test)]
mod tests;
