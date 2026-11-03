#![cfg(all(feature = "kcp", any(feature = "client", feature = "server")))]

use kcp_ovo::core::kcp::Kcp;
use lynn_tcp::lynn_kcp::KcpConfig;

// The final application, not the transport dependency, owns allocator selection.
#[global_allocator]
static ALLOCATOR: std::alloc::System = std::alloc::System;

async fn assert_timeout_schedule(mut config: KcpConfig, expected: [u64; 6]) {
    config.protocol.interval = 1;
    let fast = config.protocol.nodelay;
    let mut kcp = Kcp::new(1, config.protocol).unwrap();
    kcp.set_output(|packet| async move { Ok(packet.len()) });
    kcp.send(b"unacknowledged").unwrap();
    kcp.update(0).await.unwrap();
    assert_eq!(kcp.snd_buf.front().unwrap().rto, expected[0]);
    assert_eq!(
        kcp.snd_buf.front().unwrap().resendts,
        if fast { 200 } else { 225 }
    );
    for (retry, rto) in expected.into_iter().enumerate().skip(1) {
        let due = kcp.snd_buf.front().unwrap().resendts;
        kcp.update(due - 1).await.unwrap();
        assert_eq!(kcp.snd_buf.front().unwrap().xmit, retry as u32);
        kcp.update(due).await.unwrap();
        let segment = kcp.snd_buf.front().unwrap();
        assert_eq!(segment.xmit, retry as u32 + 1);
        assert_eq!(segment.rto, rto);
        assert_eq!(segment.resendts, due + rto);
    }
}

#[tokio::test]
async fn fast_timeouts_back_off_by_half_and_use_the_new_deadline() {
    assert_timeout_schedule(KcpConfig::default(), [200, 300, 450, 675, 1012, 1518]).await;
}

#[tokio::test]
async fn normal_timeouts_double_and_use_the_new_deadline() {
    assert_timeout_schedule(KcpConfig::normal_mode(), [200, 400, 800, 1600, 3200, 6400]).await;
}
