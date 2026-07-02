use serde::Deserialize;

use crate::Result;

use super::DiscordRest;

/// Resolves an RPC app's `client_id` to a display name. `SET_ACTIVITY` omits it.
#[derive(Debug, Deserialize)]
pub(in crate::discord) struct ApplicationRpcInfo {
    pub name: String,
}

/// A registered art asset, referenced by `name`. The `id` resolves on the CDN as
/// `app-assets/{app_id}/{id}.png`.
#[derive(Debug, Deserialize)]
pub(in crate::discord) struct ApplicationAsset {
    pub id: String,
    pub name: String,
}

/// Media-proxy path for an image referenced by raw URL. Carried as
/// `mp:{external_asset_path}` because a raw URL alone does not render.
#[derive(Debug, Deserialize)]
pub(in crate::discord) struct ExternalAsset {
    pub external_asset_path: String,
}

impl DiscordRest {
    pub(in crate::discord) async fn application_rpc(
        &self,
        application_id: &str,
    ) -> Result<ApplicationRpcInfo> {
        self.send_json(
            self.raw_http.get(format!(
                "https://discord.com/api/v9/applications/{application_id}/rpc"
            )),
            "application rpc info",
        )
        .await
    }

    /// Uses the `oauth2/applications/...` path: it returns the public asset list for
    /// any app, while `applications/{id}/assets` is owner-scoped and `401`s.
    pub(in crate::discord) async fn application_assets(
        &self,
        application_id: &str,
    ) -> Result<Vec<ApplicationAsset>> {
        self.send_json(
            self.raw_http.get(format!(
                "https://discord.com/api/v9/oauth2/applications/{application_id}/assets"
            )),
            "application assets",
        )
        .await
    }

    /// Register external image URLs and get back their media-proxy paths, one per
    /// input URL in order.
    pub(in crate::discord) async fn application_external_assets(
        &self,
        application_id: &str,
        urls: &[&str],
    ) -> Result<Vec<ExternalAsset>> {
        self.send_json(
            self.raw_http
                .post(format!(
                    "https://discord.com/api/v9/applications/{application_id}/external-assets"
                ))
                .json(&serde_json::json!({ "urls": urls })),
            "application external assets",
        )
        .await
    }
}
