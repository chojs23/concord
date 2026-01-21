use discord_client_terminal::{AppError, Config};

#[test]
fn allows_missing_discord_token() {
    let config = Config::from_pairs([] as [(&str, &str); 0]).expect("token is stored separately");
    assert!(config.default_channel_id.is_none());
}

#[test]
fn rejects_invalid_channel_id() {
    let error = Config::from_pairs([("DISCORD_DEFAULT_CHANNEL_ID", "not-a-number")])
        .expect_err("channel ids must be numeric");

    assert!(matches!(error, AppError::InvalidChannelId { .. }));
}
