#![cfg(all(feature = "kcp", any(feature = "client", feature = "server")))]

use kcp_ovo::{KcpCmd, KcpError, core::kcp::Kcp, helper::KcpPacketHeader};
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

fn push_packet(sn: u32, frg: u8, payload: &[u8]) -> Vec<u8> {
    let header = KcpPacketHeader::new(1, KcpCmd::Push, frg, 128, 0, sn, 0, payload.len() as u32);
    let mut packet = header.to_bytes().unwrap();
    packet.extend_from_slice(payload);
    packet
}

#[test]
fn received_messages_survive_reuse_of_single_and_fragmented_input_buffers() {
    for config in [KcpConfig::default(), KcpConfig::normal_mode()] {
        for len in [1, 1372, 1373, 16 * 1024 + 1] {
            let mut kcp = Kcp::new(1, config.protocol.clone()).unwrap();
            let payload: Vec<_> = (0..len).map(|index| (index % 251 + 1) as u8).collect();
            let fragments = payload.chunks(kcp.mss as usize);
            let count = fragments.len();
            for (index, fragment) in fragments.enumerate() {
                let mut packet = push_packet(index as u32, (count - index - 1) as u8, fragment);
                kcp.input(&packet).unwrap();
                packet.fill(0);
                drop(packet);
            }
            assert_eq!(kcp.recv().unwrap(), payload);
            assert_eq!(kcp.recv().unwrap_err(), KcpError::QueueEmpty);
        }
    }
}

#[test]
fn reordered_duplicate_messages_preserve_order_and_recover_a_full_receive_window() {
    for mut config in [KcpConfig::default(), KcpConfig::normal_mode()] {
        config.protocol.rcv_wnd = 12;
        let mut kcp = Kcp::new(1, config.protocol).unwrap();
        for sn in (1..12_u32).rev().chain([6, 11, 1, 0, 12, 12]) {
            kcp.input(&push_packet(sn, 0, &sn.to_be_bytes())).unwrap();
        }
        assert_eq!((kcp.nrcv_que, kcp.nrcv_buf), (12, 1));
        for sn in 0..13_u32 {
            assert_eq!(kcp.recv().unwrap(), sn.to_be_bytes());
            if sn == 0 {
                assert_eq!((kcp.nrcv_que, kcp.nrcv_buf), (12, 0));
            }
        }
        assert_eq!((kcp.nrcv_que, kcp.nrcv_buf), (0, 0));
        assert_eq!(kcp.recv().unwrap_err(), KcpError::QueueEmpty);
        kcp.input(&push_packet(13, 0, b"next")).unwrap();
        assert_eq!(kcp.recv().unwrap(), b"next");
    }
}
