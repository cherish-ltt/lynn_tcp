/// Optional KCP session settings. KCP is plaintext and does not authenticate peers.
#[derive(Clone, Debug)]
pub struct KcpConfig {
    /// Maximum silence without a valid peer packet; must be nonzero.
    pub idle_timeout_secs: u64,
}

impl Default for KcpConfig {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 60,
        }
    }
}

impl KcpConfig {
    pub(crate) fn validate(&self) -> crate::Result<()> {
        if self.idle_timeout_secs == 0 {
            return Err(crate::LynnError::config("KCP idle timeout must be nonzero"));
        }
        Ok(())
    }
}
