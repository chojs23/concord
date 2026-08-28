use serde_json::{Value, json};

use super::DiscordRest;
use crate::Result;

impl DiscordRest {
    pub(in crate::discord) async fn upload_stream_preview(
        &self,
        stream_key: &str,
        thumbnail_data_uri: &str,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post(stream_preview_endpoint(stream_key))
                .json(&stream_preview_request_body(thumbnail_data_uri)),
            "upload stream preview",
        )
        .await
    }
}

fn stream_preview_endpoint(stream_key: &str) -> String {
    format!("https://discord.com/api/v9/streams/{stream_key}/preview")
}

fn stream_preview_request_body(thumbnail_data_uri: &str) -> Value {
    json!({ "thumbnail": thumbnail_data_uri })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_preview_request_matches_discord_endpoint_and_body() {
        let stream_key = "guild:123:456:789";
        let thumbnail = "data:image/jpeg;base64,cHJldmlldw==";

        assert_eq!(
            stream_preview_endpoint(stream_key),
            "https://discord.com/api/v9/streams/guild:123:456:789/preview"
        );
        assert_eq!(
            stream_preview_request_body(thumbnail),
            json!({ "thumbnail": thumbnail })
        );
    }
}
