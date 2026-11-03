/// Optional KCP session settings. KCP is plaintext and does not authenticate peers.
#[derive(Clone, Debug)]
pub struct KcpConfig {
    /// Maximum silence without a valid peer packet; must be nonzero.
    pub idle_timeout_secs: u64,
    /// Low-level KCP settings. Message mode and MTU 1400 are required by this adapter.
    pub protocol: kcp_ovo::KcpConfig,
}

impl Default for KcpConfig {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 60,
            protocol: kcp_ovo::KcpConfig {
                interval: 10,
                ..kcp_ovo::KcpConfig::fast_mode()
            },
        }
    }
}

impl KcpConfig {
    /// Uses the upstream normal preset instead of the default custom fast preset.
    pub fn normal_mode() -> Self {
        Self {
            protocol: kcp_ovo::KcpConfig::normal_mode(),
            ..Self::default()
        }
    }

    pub(crate) fn validate(&self) -> crate::Result<()> {
        if self.idle_timeout_secs == 0 {
            return Err(crate::LynnError::config("KCP idle timeout must be nonzero"));
        }
        if self.protocol.stream_mode {
            return Err(crate::LynnError::config(
                "KCP stream_mode removes message boundaries required by the Lynn session envelope",
            ));
        }
        if self.protocol.mtu != 1400
            || !(1..=5000).contains(&self.protocol.interval)
            || self.protocol.fastresend < 0
            || self.protocol.snd_wnd == 0
            || usize::from(self.protocol.rcv_wnd) < (super::CHUNK + 1).div_ceil(1372)
        {
            return Err(crate::LynnError::config(
                "KCP requires MTU 1400, interval 1..=5000 ms, fastresend >= 0, snd_wnd > 0 and rcv_wnd >= 12",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_normal_and_custom_options_are_explicit_and_validated() {
        let fast = KcpConfig::default();
        assert!(fast.protocol.nodelay && fast.protocol.nocwnd);
        assert_eq!(fast.protocol.interval, 10);
        assert_eq!(fast.protocol.fastresend, 2);
        assert!(!fast.protocol.stream_mode);
        fast.validate().unwrap();
        let normal = KcpConfig::normal_mode();
        assert!(!normal.protocol.nodelay && !normal.protocol.nocwnd);
        assert_eq!(normal.protocol.interval, 100);
        assert_eq!(normal.protocol.fastresend, 0);
        normal.validate().unwrap();
        for field in 0..7 {
            let mut config = fast.clone();
            match field {
                0 => config.protocol.stream_mode = true,
                1 => config.protocol.interval = 0,
                2 => config.protocol.interval = 5001,
                3 => config.protocol.mtu = 1500,
                4 => config.protocol.fastresend = -1,
                5 => config.protocol.snd_wnd = 0,
                _ => config.protocol.rcv_wnd = 11,
            }
            assert!(config.validate().is_err());
        }
        let mut custom = fast;
        custom.protocol.interval = 1;
        custom.protocol.snd_wnd = 256;
        custom.protocol.rcv_wnd = 256;
        custom.validate().unwrap();
    }
}
