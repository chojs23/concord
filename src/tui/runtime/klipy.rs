use crate::tui::{
    media::{
        DecodedMediaImage, clipped_media_protocol, decode_media_image_bytes,
        fixed_media_protocol_render_spec, media_image_job_permits, media_image_work_permits,
    },
    state::DashboardState,
    ui,
};
use crate::{
    config::{AnimatePreviews, KlipyOptions},
    klipy::{GifPage, KlipyClient},
};
use image::DynamicImage;
use ratatui::layout::Rect;
use ratatui_image::{picker::Picker, protocol::Protocol};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{OwnedSemaphorePermit, mpsc},
    task::JoinHandle,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PreviewKey {
    generation: u64,
    url: String,
    width: u16,
    height: u16,
}

pub(super) enum KlipyResult {
    Search(u64, Result<GifPage, String>),
    Preview(PreviewKey, Result<KlipyPreview, String>),
    Frame(PreviewKey, usize, Result<Protocol, String>),
}

pub(super) struct KlipyPreview {
    image: DecodedMediaImage,
    protocol: Protocol,
    rendered_frame_index: usize,
}

#[derive(Default)]
pub(super) struct KlipyRuntime {
    client: Option<Result<KlipyClient, String>>,
    search_generation: Option<u64>,
    search_task: Option<JoinHandle<()>>,
    preview_key: Option<PreviewKey>,
    preview_task: Option<JoinHandle<()>>,
    preview: Option<Result<KlipyPreview, String>>,
    pending_frame: Option<usize>,
}

impl Drop for KlipyRuntime {
    fn drop(&mut self) {
        self.cancel_search();
        self.cancel_preview();
    }
}

impl KlipyRuntime {
    pub(super) fn configure(&mut self, options: &KlipyOptions) {
        self.client = Some(KlipyClient::new(options));
    }

    fn cancel_search(&mut self) {
        if let Some(task) = self.search_task.take() {
            task.abort();
        }
    }
    fn cancel_preview(&mut self) {
        if let Some(task) = self.preview_task.take() {
            task.abort();
        }
        self.pending_frame = None;
    }

    pub(super) fn sync(
        &mut self,
        state: &mut DashboardState,
        area: Rect,
        picker: Option<Picker>,
        tx: &mpsc::UnboundedSender<KlipyResult>,
    ) {
        let shares = state.take_klipy_shares();
        if !shares.is_empty()
            && let Some(Ok(client)) = self.client.as_ref()
        {
            let client = client.clone();
            tokio::spawn(async move {
                for (slug, query) in shares {
                    if let Err(error) = client.share(&slug, &query).await {
                        crate::logging::error("klipy", error);
                    }
                }
            });
        }
        let Some(view) = state.gif_picker() else {
            self.cancel_search();
            self.cancel_preview();
            self.search_generation = None;
            self.preview_key = None;
            self.preview = None;
            return;
        };
        if self.search_generation != Some(view.generation) {
            self.cancel_search();
            let generation = view.generation;
            self.search_generation = Some(generation);
            let query = view.query.value().to_owned();
            let page = view.page;
            let client = self
                .client
                .clone()
                .unwrap_or_else(|| Err("KLIPY is not configured".to_owned()));
            let tx = tx.clone();
            self.search_task = Some(tokio::spawn(async move {
                // Debounce typed searches, but do not delay a requested next page.
                if page == 1 {
                    tokio::time::sleep(Duration::from_millis(350)).await;
                }
                let result = match client {
                    Ok(client) => client.search(&query, page).await,
                    Err(error) => Err(error),
                };
                let _ = tx.send(KlipyResult::Search(generation, result));
            }));
        }
        let preview_area = ui::gif_picker_preview_area(area);
        let key = view
            .selected_gif()
            .and_then(|gif| gif.media_url(true))
            .filter(|_| state.show_images() && !preview_area.is_empty())
            .map(|url| PreviewKey {
                generation: view.preview_generation,
                url: url.to_owned(),
                width: preview_area.width,
                height: preview_area.height,
            });
        if self.preview_key != key {
            self.cancel_preview();
            self.preview = None;
            self.preview_key = key.clone();
            if let Some(key) = key {
                let tx = tx.clone();
                let client = self
                    .client
                    .clone()
                    .unwrap_or_else(|| Err("KLIPY is not configured".to_owned()));
                self.preview_task = Some(tokio::spawn(async move {
                    let result = load_preview(client, picker, &key).await;
                    let _ = tx.send(KlipyResult::Preview(key, result));
                }));
            }
        }
    }

    pub(super) fn store(&mut self, state: &mut DashboardState, result: KlipyResult) -> bool {
        match result {
            KlipyResult::Search(generation, result) => state.store_gif_results(generation, result),
            KlipyResult::Preview(key, result) if self.preview_key.as_ref() == Some(&key) => {
                self.preview_task = None;
                self.preview = Some(result);
                true
            }
            KlipyResult::Frame(key, frame_index, result)
                if self.preview_key.as_ref() == Some(&key)
                    && self.pending_frame == Some(frame_index) =>
            {
                self.preview_task = None;
                self.pending_frame = None;
                match result {
                    Ok(protocol) => {
                        let Some(Ok(preview)) = self.preview.as_mut() else {
                            return false;
                        };
                        preview.protocol = protocol;
                        preview.rendered_frame_index = frame_index;
                    }
                    Err(error) => self.preview = Some(Err(error)),
                }
                true
            }
            _ => false,
        }
    }

    pub(super) fn preview(&self) -> Option<Result<&Protocol, &str>> {
        self.preview.as_ref().map(|result| {
            result
                .as_ref()
                .map(|preview| &preview.protocol)
                .map_err(String::as_str)
        })
    }

    pub(super) fn placement(&self, area: Rect) -> Option<(String, Rect)> {
        let key = self.preview_key.as_ref()?;
        let preview = self.preview.as_ref()?.as_ref().ok()?;
        Some((
            format!(
                "{}:{}:{}",
                key.generation, key.url, preview.rendered_frame_index
            ),
            ui::gif_picker_preview_area(area),
        ))
    }

    pub(super) fn sync_animation_visibility(&mut self, now: Instant, animate: AnimatePreviews) {
        let Some(Ok(preview)) = self.preview.as_mut() else {
            return;
        };
        if animate == AnimatePreviews::Never {
            preview.image.pause_animation();
        } else {
            preview.image.start_animation(now);
        }
    }

    pub(super) fn pause_animation(&mut self) {
        if let Some(Ok(preview)) = self.preview.as_mut() {
            preview.image.pause_animation();
        }
    }

    pub(super) fn next_animation_deadline(&self) -> Option<Instant> {
        if self.pending_frame.is_some() {
            return None;
        }
        self.preview
            .as_ref()?
            .as_ref()
            .ok()?
            .image
            .next_frame_deadline()
    }

    pub(super) fn advance_animation(
        &mut self,
        now: Instant,
        picker: Option<Picker>,
        tx: &mpsc::UnboundedSender<KlipyResult>,
    ) {
        if self.pending_frame.is_some() {
            return;
        }
        let Some(key) = self.preview_key.clone() else {
            return;
        };
        let Some(Ok(preview)) = self.preview.as_mut() else {
            return;
        };
        if !preview.image.advance_frame(now) {
            return;
        }
        let Some(picker) = picker else {
            preview.image.pause_animation();
            return;
        };
        let frame_index = preview.image.current_frame_index();
        let frame = preview.image.current_frame_shared();
        self.pending_frame = Some(frame_index);
        let tx = tx.clone();
        self.preview_task = Some(tokio::spawn(async move {
            let result = build_preview_frame(frame, picker, key.width, key.height).await;
            let _ = tx.send(KlipyResult::Frame(key, frame_index, result));
        }));
    }
}

async fn load_preview(
    client: Result<KlipyClient, String>,
    picker: Option<Picker>,
    key: &PreviewKey,
) -> Result<KlipyPreview, String> {
    let picker = picker.ok_or_else(|| "Inline preview unavailable in this terminal".to_owned())?;
    // A queued-job permit bounds downloaded bytes. The active-worker permit
    // stays in the blocking closure because aborting its async parent cannot
    // interrupt image decoding.
    let job_permit = media_image_job_permits()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "Image worker stopped".to_owned())?;
    let bytes = client?.preview(&key.url).await?;
    decode_preview_bytes(bytes, picker, key.width, key.height, job_permit).await
}

async fn decode_preview_bytes(
    bytes: Vec<u8>,
    picker: Picker,
    width: u16,
    height: u16,
    job_permit: OwnedSemaphorePermit,
) -> Result<KlipyPreview, String> {
    let work_permit = media_image_work_permits()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "Image worker stopped".to_owned())?;
    tokio::task::spawn_blocking(move || {
        let (_job_permit, _work_permit) = (job_permit, work_permit);
        let image = decode_media_image_bytes(&bytes).map_err(|error| error.to_string())?;
        let protocol = preview_protocol(
            &picker,
            image.current_frame_shared().as_ref(),
            width,
            height,
        )?;
        Ok(KlipyPreview {
            image,
            protocol,
            rendered_frame_index: 0,
        })
    })
    .await
    .map_err(|_| "Preview worker failed".to_owned())?
}

async fn build_preview_frame(
    frame: Arc<DynamicImage>,
    picker: Picker,
    width: u16,
    height: u16,
) -> Result<Protocol, String> {
    let job_permit = media_image_job_permits()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "Image worker stopped".to_owned())?;
    let work_permit = media_image_work_permits()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "Image worker stopped".to_owned())?;
    tokio::task::spawn_blocking(move || {
        let (_job_permit, _work_permit) = (job_permit, work_permit);
        preview_protocol(&picker, frame.as_ref(), width, height)
    })
    .await
    .map_err(|_| "Preview worker failed".to_owned())?
}

fn preview_protocol(
    picker: &Picker,
    frame: &DynamicImage,
    width: u16,
    height: u16,
) -> Result<Protocol, String> {
    clipped_media_protocol(
        picker,
        frame,
        fixed_media_protocol_render_spec(width, height),
    )
    .ok_or_else(|| "Preview dimensions unavailable".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{
        Delay, Frame as ImageFrame, ImageBuffer, Rgba,
        codecs::gif::{GifEncoder, Repeat},
    };

    fn animated_gif() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            encoder
                .set_repeat(Repeat::Infinite)
                .expect("test GIF repeat should encode");
            for color in [Rgba([255, 0, 0, 255]), Rgba([0, 0, 255, 255])] {
                encoder
                    .encode_frame(ImageFrame::from_parts(
                        ImageBuffer::from_pixel(2, 2, color),
                        0,
                        0,
                        Delay::from_numer_denom_ms(60, 1),
                    ))
                    .expect("test GIF frame should encode");
            }
        }
        bytes
    }

    #[tokio::test]
    async fn klipy_preview_ignores_stale_results_and_drops_work_on_close() {
        let mut runtime = KlipyRuntime::default();
        let mut state = DashboardState::new();
        let key = PreviewKey {
            generation: 2,
            url: "https://static.klipy.com/a.gif".to_owned(),
            width: 20,
            height: 10,
        };
        runtime.preview_key = Some(key.clone());
        let stale = PreviewKey {
            generation: 1,
            ..key.clone()
        };
        assert!(!runtime.store(
            &mut state,
            KlipyResult::Preview(stale, Err("old".to_owned()))
        ));
        assert!(runtime.preview.is_none());
        assert!(runtime.store(
            &mut state,
            KlipyResult::Preview(key.clone(), Err("current".to_owned()))
        ));
        assert!(matches!(runtime.preview(), Some(Err("current"))));
        let (tx, _rx) = mpsc::unbounded_channel();
        runtime.sync(&mut state, Rect::new(0, 0, 100, 30), None, &tx);
        assert!(runtime.preview.is_none());
        assert!(runtime.preview_key.is_none());
        assert!(!runtime.store(
            &mut state,
            KlipyResult::Preview(key, Err("late".to_owned()))
        ));
    }

    #[tokio::test]
    async fn klipy_preview_animates_selected_gif_and_pauses_when_disabled() {
        let picker = Picker::halfblocks();
        let image = decode_media_image_bytes(&animated_gif()).expect("test GIF should decode");
        assert_eq!(image.frame_count(), 2);
        let protocol = preview_protocol(&picker, image.current_frame_shared().as_ref(), 20, 10)
            .expect("first frame should render");
        let key = PreviewKey {
            generation: 1,
            url: "https://static.klipy.com/animated.gif".to_owned(),
            width: 20,
            height: 10,
        };
        let mut runtime = KlipyRuntime::default();
        runtime.preview_key = Some(key.clone());
        runtime.preview = Some(Ok(KlipyPreview {
            image,
            protocol,
            rendered_frame_index: 0,
        }));
        let now = Instant::now();
        runtime.sync_animation_visibility(now, AnimatePreviews::Selected);
        let deadline = runtime
            .next_animation_deadline()
            .expect("selected GIF should schedule a frame");
        let (tx, mut rx) = mpsc::unbounded_channel();
        runtime.advance_animation(deadline, Some(picker), &tx);
        assert_eq!(runtime.pending_frame, Some(1));
        assert_eq!(runtime.next_animation_deadline(), None);
        let result = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("frame should finish")
            .expect("frame should be sent");
        assert!(runtime.store(&mut DashboardState::new(), result));
        assert!(
            runtime
                .placement(Rect::new(0, 0, 100, 30))
                .expect("rendered GIF should have placement")
                .0
                .ends_with(":1")
        );
        runtime.sync_animation_visibility(deadline, AnimatePreviews::Never);
        assert_eq!(runtime.next_animation_deadline(), None);
        runtime.sync_animation_visibility(deadline, AnimatePreviews::Always);
        assert!(runtime.next_animation_deadline().is_some());
        runtime.pause_animation();
        assert_eq!(runtime.next_animation_deadline(), None);
    }

    #[tokio::test]
    async fn klipy_preview_work_waits_for_shared_image_worker_capacity() {
        let workers = media_image_work_permits().clone();
        let first = workers
            .clone()
            .acquire_owned()
            .await
            .expect("image workers stay open");
        let second = workers
            .acquire_owned()
            .await
            .expect("image workers stay open");
        let job_permit = media_image_job_permits()
            .clone()
            .acquire_owned()
            .await
            .expect("image job slots stay open");
        let mut decode = tokio::spawn(decode_preview_bytes(
            animated_gif(),
            Picker::halfblocks(),
            20,
            10,
            job_permit,
        ));
        let mut frame = tokio::spawn(build_preview_frame(
            Arc::new(DynamicImage::new_rgba8(2, 2)),
            Picker::halfblocks(),
            20,
            10,
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut decode)
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut frame)
                .await
                .is_err()
        );
        drop(first);
        drop(second);
        let preview = tokio::time::timeout(Duration::from_secs(5), decode)
            .await
            .expect("released worker should run")
            .expect("decode task should join")
            .expect("GIF should render");
        assert_eq!(preview.image.frame_count(), 2);
        tokio::time::timeout(Duration::from_secs(5), frame)
            .await
            .expect("released worker should run")
            .expect("frame task should join")
            .expect("frame should render");
    }
}
