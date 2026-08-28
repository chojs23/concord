use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::{sync::oneshot, task::JoinHandle};

use super::RTP_VERSION;

#[derive(Default)]
pub(super) struct GatewayChildTasks {
    heartbeat: Option<GatewayChildTask>,
    udp_ping: Option<GatewayChildTask>,
    media: Option<GatewayChildTask>,
}

struct GatewayChildTask {
    task: JoinHandle<()>,
    graceful_stop: Option<oneshot::Sender<()>>,
}

impl GatewayChildTask {
    fn abort(mut self) {
        self.graceful_stop.take();
        self.task.abort();
    }

    fn request_shutdown(&mut self) {
        if let Some(stop_tx) = self.graceful_stop.take() {
            let _ = stop_tx.send(());
        } else {
            self.task.abort();
        }
    }

    async fn shutdown(mut self) {
        self.request_shutdown();
        let _ = self.task.await;
    }
}

impl GatewayChildTasks {
    pub(super) fn has_media(&self) -> bool {
        self.media.is_some()
    }

    pub(super) async fn replace_heartbeat(&mut self, task: JoinHandle<()>) {
        Self::replace(
            &mut self.heartbeat,
            GatewayChildTask {
                task,
                graceful_stop: None,
            },
        )
        .await;
    }

    pub(super) async fn replace_udp_ping(&mut self, task: JoinHandle<()>) {
        Self::replace(
            &mut self.udp_ping,
            GatewayChildTask {
                task,
                graceful_stop: None,
            },
        )
        .await;
    }

    pub(super) async fn replace_media(&mut self, task: JoinHandle<()>) {
        Self::replace(
            &mut self.media,
            GatewayChildTask {
                task,
                graceful_stop: None,
            },
        )
        .await;
    }

    pub(super) async fn shutdown_media(&mut self) {
        if let Some(media) = self.media.take() {
            media.shutdown().await;
        }
    }

    pub(super) fn install_media_gracefully(
        &mut self,
        task: JoinHandle<()>,
        stop_tx: oneshot::Sender<()>,
    ) {
        assert!(
            self.media.is_none(),
            "media task must be stopped before install"
        );
        self.media = Some(GatewayChildTask {
            task,
            graceful_stop: Some(stop_tx),
        });
    }

    async fn replace(slot: &mut Option<GatewayChildTask>, task: GatewayChildTask) {
        if let Some(previous) = slot.take() {
            previous.shutdown().await;
        }
        *slot = Some(task);
    }

    pub(super) async fn shutdown(&mut self) {
        let mut tasks = [
            self.heartbeat.take(),
            self.udp_ping.take(),
            self.media.take(),
        ];
        for task in tasks.iter_mut().flatten() {
            task.request_shutdown();
        }
        for task in tasks.into_iter().flatten() {
            let _ = task.task.await;
        }
    }

    fn abort_all(&mut self) {
        for task in [
            self.heartbeat.take(),
            self.udp_ping.take(),
            self.media.take(),
        ]
        .into_iter()
        .flatten()
        {
            task.abort();
        }
    }
}

impl Drop for GatewayChildTasks {
    fn drop(&mut self) {
        // Callers normally use `shutdown` so graceful media cleanup can run.
        // This fallback prevents detached work if the owning future is aborted.
        self.abort_all();
    }
}

pub(super) fn packetize_h264_payloads(frame: &[u8], max_payload_bytes: usize) -> Vec<Vec<u8>> {
    if max_payload_bytes <= 2 {
        return Vec::new();
    }

    let mut payloads = Vec::new();
    for nal in annex_b_nals(frame) {
        if nal.len() <= max_payload_bytes {
            payloads.push(nal.to_vec());
            continue;
        }
        let Some((&nal_header, body)) = nal.split_first() else {
            continue;
        };
        let fu_indicator = (nal_header & 0xe0) | 28;
        let nal_type = nal_header & 0x1f;
        let chunks = body.chunks(max_payload_bytes - 2);
        let chunk_count = chunks.len();
        for (index, chunk) in chunks.enumerate() {
            let mut payload = Vec::with_capacity(chunk.len() + 2);
            payload.push(fu_indicator);
            payload.push(
                nal_type
                    | if index == 0 { 0x80 } else { 0 }
                    | if index + 1 == chunk_count { 0x40 } else { 0 },
            );
            payload.extend_from_slice(chunk);
            payloads.push(payload);
        }
    }
    payloads
}

pub(super) fn annex_b_nals(frame: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut index = 0usize;
    while index + 3 <= frame.len() {
        let start_len = if frame.get(index..index + 4) == Some(&[0, 0, 0, 1]) {
            4
        } else if frame.get(index..index + 3) == Some(&[0, 0, 1]) {
            3
        } else {
            index += 1;
            continue;
        };
        starts.push((index, start_len));
        index += start_len;
    }
    if starts.is_empty() {
        return (!frame.is_empty()).then_some(frame).into_iter().collect();
    }
    starts
        .iter()
        .enumerate()
        .filter_map(|(position, (start, start_len))| {
            let nal_start = start + start_len;
            let nal_end = starts
                .get(position + 1)
                .map(|(next, _)| *next)
                .unwrap_or(frame.len());
            (nal_start < nal_end).then_some(&frame[nal_start..nal_end])
        })
        .collect()
}

pub(super) fn current_unix_time() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

pub(super) fn build_rtcp_sender_report(
    sender_ssrc: u32,
    unix_time: Duration,
    rtp_timestamp: u32,
    packet_count: u32,
    octet_count: u32,
) -> [u8; 28] {
    const NTP_UNIX_EPOCH_OFFSET_SECONDS: u64 = 2_208_988_800;
    const RTCP_SENDER_REPORT_PACKET_TYPE: u8 = 200;
    const RTCP_SENDER_REPORT_LENGTH_WORDS_MINUS_ONE: u16 = 6;

    let ntp_seconds = unix_time
        .as_secs()
        .wrapping_add(NTP_UNIX_EPOCH_OFFSET_SECONDS) as u32;
    let ntp_fraction = ((u64::from(unix_time.subsec_nanos()) << 32) / 1_000_000_000) as u32;
    let mut report = [0u8; 28];
    report[0] = RTP_VERSION << 6;
    report[1] = RTCP_SENDER_REPORT_PACKET_TYPE;
    report[2..4].copy_from_slice(&RTCP_SENDER_REPORT_LENGTH_WORDS_MINUS_ONE.to_be_bytes());
    report[4..8].copy_from_slice(&sender_ssrc.to_be_bytes());
    report[8..12].copy_from_slice(&ntp_seconds.to_be_bytes());
    report[12..16].copy_from_slice(&ntp_fraction.to_be_bytes());
    report[16..20].copy_from_slice(&rtp_timestamp.to_be_bytes());
    report[20..24].copy_from_slice(&packet_count.to_be_bytes());
    report[24..28].copy_from_slice(&octet_count.to_be_bytes());
    report
}
