//! Shared detection of Discord's "you must solve a CAPTCHA" response.
//!
//! Discord answers privileged user-account requests (sending the first message
//! to a stranger, logging in from a new location, ...) with an HTTP 400 whose
//! body carries an hCaptcha challenge instead of a normal error. The challenge
//! must be solved and replayed; we cannot do that in a terminal. Every caller
//! that talks to the REST/auth API therefore needs to recognise this shape so
//! it can stop (never retry) and tell the user to finish the action in an
//! official client.

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub(in crate::discord) struct CaptchaChallenge {
    captcha_key: Vec<String>,
    captcha_service: Option<String>,
}

/// Returns the challenge when `body` is Discord's hCaptcha gate for `status`.
///
/// Only HTTP 400 bodies that explicitly list `captcha-required` for the
/// hCaptcha service count; any other 400 is an ordinary error and returns
/// `None` so callers keep their existing handling.
pub(in crate::discord) fn parse_captcha_challenge(
    status: reqwest::StatusCode,
    body: &str,
) -> Option<CaptchaChallenge> {
    if status != reqwest::StatusCode::BAD_REQUEST {
        return None;
    }

    let challenge = serde_json::from_str::<CaptchaChallenge>(body).ok()?;
    let is_hcaptcha = challenge
        .captcha_service
        .as_deref()
        .is_none_or(|service| service.eq_ignore_ascii_case("hcaptcha"));
    let requires_captcha = challenge
        .captcha_key
        .iter()
        .any(|key| key == "captcha-required");

    (is_hcaptcha && requires_captcha).then_some(challenge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn detects_hcaptcha_required_only_on_bad_request() {
        let body = r#"{"captcha_key":["captcha-required"],"captcha_service":"hcaptcha","captcha_sitekey":"abc"}"#;
        assert!(parse_captcha_challenge(StatusCode::BAD_REQUEST, body).is_some());
        // Same body under a different status is not a captcha gate.
        assert!(parse_captcha_challenge(StatusCode::FORBIDDEN, body).is_none());
        // An ordinary 400 error must not be mistaken for a captcha.
        assert!(
            parse_captcha_challenge(StatusCode::BAD_REQUEST, r#"{"message":"bad"}"#).is_none()
        );
    }
}
