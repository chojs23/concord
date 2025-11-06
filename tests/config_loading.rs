use discord_client_terminal::{AppError, Config};

#[test]
fn rejects_missing_discord_token() {
    let error = Config::from_pairs([] as [(&str, &str); 0]).expect_err("token must be required");
    assert!(matches!(error, AppError::MissingDiscordToken));
}

#[test]
fn rejects_invalid_channel_id() {
    let error = Config::from_pairs([
        ("DISCORD_TOKEN", "token-value"),
        ("DISCORD_DEFAULT_CHANNEL_ID", "not-a-number"),
    ])
    .expect_err("channel ids must be numeric");

    assert!(matches!(error, AppError::InvalidChannelId { .. }));
}

#[test]
fn rejects_invalid_message_content_flag() {
    let error = Config::from_pairs([
        ("DISCORD_TOKEN", "token-value"),
        ("DISCORD_ENABLE_MESSAGE_CONTENT", "maybe"),
    ])
    .expect_err("message content flag must be a known boolean");

    assert!(matches!(error, AppError::InvalidMessageContentFlag { .. }));
}
