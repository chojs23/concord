use crate::tui::{
    media::{
        clipped_media_protocol, decode_image_bytes, fixed_media_protocol_render_spec,
        media_image_job_permits,
    },
    state::DashboardState,
    ui,
};
use crate::{
    config::KlipyOptions,
    klipy::{GifPage, KlipyClient},
};
use ratatui::layout::Rect;
use ratatui_image::{picker::Picker, protocol::Protocol};
use std::time::Duration;
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PreviewKey {
    generation: u64,
    url: String,
    width: u16,
    height: u16,
}

pub(super) enum KlipyResult {
    Search(u64, Result<GifPage, String>),
    Preview(PreviewKey, Result<Protocol, String>),
}

#[derive(Default)]
pub(super) struct KlipyRuntime {
    client: Option<Result<KlipyClient, String>>,
    search_generation: Option<u64>,
    search_task: Option<JoinHandle<()>>,
    preview_key: Option<PreviewKey>,
    preview_task: Option<JoinHandle<()>>,
    preview: Option<Result<Protocol, String>>,
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
                // Abort/restart while typing: no request per keystroke.
                tokio::time::sleep(Duration::from_millis(350)).await;
                let result = match client {
                    Ok(client) => client.search(&query, page).await,
                    Err(error) => Err(error),
                };
                let _ = tx.send(KlipyResult::Search(generation, result));
            }));
        }
        let preview_area = ui::gif_picker_preview_area(area);
        let key = view
            .results
            .get(view.selected)
            .and_then(|gif| gif.media_url(true))
            .filter(|_| state.show_images() && !preview_area.is_empty())
            .map(|url| PreviewKey {
                generation: view.generation,
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
                self.preview = Some(result);
                true
            }
            _ => false,
        }
    }

    pub(super) fn preview(&self) -> Option<Result<&Protocol, &str>> {
        self.preview
            .as_ref()
            .map(|result| result.as_ref().map_err(String::as_str))
    }

    pub(super) fn placement(&self, area: Rect) -> Option<(String, Rect)> {
        let key = self.preview_key.as_ref()?;
        self.preview.as_ref()?.as_ref().ok()?;
        Some((
            format!("{}:{}", key.generation, key.url),
            ui::gif_picker_preview_area(area),
        ))
    }
}

async fn load_preview(
    client: Result<KlipyClient, String>,
    picker: Option<Picker>,
    key: &PreviewKey,
) -> Result<Protocol, String> {
    let picker = picker.ok_or_else(|| "Inline preview unavailable in this terminal".to_owned())?;
    // Share Concord's image-work limit, including bytes waiting to be decoded.
    let permit = media_image_job_permits()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "Image worker stopped".to_owned())?;
    let bytes = client?.preview(&key.url).await?;
    let (width, height) = (key.width, key.height);
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let image = decode_image_bytes(&bytes)?;
        clipped_media_protocol(
            &picker,
            &image,
            fixed_media_protocol_render_spec(width, height),
        )
        .ok_or_else(|| "Preview dimensions unavailable".to_owned())
    })
    .await
    .map_err(|_| "Preview worker failed".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
