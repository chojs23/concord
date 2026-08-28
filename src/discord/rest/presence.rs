use crate::{Result, discord::PresenceStatus};

use super::DiscordRest;

impl DiscordRest {
    pub async fn update_current_user_status(&self, status: PresenceStatus) -> Result<()> {
        self.update_status_settings(status).await
    }
}
