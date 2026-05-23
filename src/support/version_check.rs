use std::{cmp::Ordering, time::Duration};

use serde::Deserialize;

const CRATE_INDEX_URL: &str = "https://index.crates.io/co/nc/concord";
const VERSION_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
struct CrateIndexVersion {
    vers: String,
    yanked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComparableVersion {
    parts: [u64; 3],
    prerelease: bool,
}

impl Ord for ComparableVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts
            .cmp(&other.parts)
            .then_with(|| other.prerelease.cmp(&self.prerelease))
    }
}

impl PartialOrd for ComparableVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub async fn check_latest_version() -> std::result::Result<Option<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(VERSION_CHECK_TIMEOUT)
        .user_agent(format!("concord/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("build version-check HTTP client: {error}"))?;
    let body = client
        .get(CRATE_INDEX_URL)
        .send()
        .await
        .map_err(|error| format!("request crates.io sparse index: {error}"))?
        .error_for_status()
        .map_err(|error| format!("crates.io sparse index returned error: {error}"))?
        .text()
        .await
        .map_err(|error| format!("read crates.io sparse index response: {error}"))?;

    latest_available_version_from_index(&body, env!("CARGO_PKG_VERSION"))
}

fn latest_available_version_from_index(
    body: &str,
    current_version: &str,
) -> std::result::Result<Option<String>, String> {
    let current = parse_version(current_version)
        .ok_or_else(|| format!("current version is not semver: {current_version}"))?;
    let include_prerelease = current.prerelease;
    let mut latest: Option<(ComparableVersion, String)> = None;

    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        let version: CrateIndexVersion = serde_json::from_str(line)
            .map_err(|error| format!("parse crates.io sparse index line: {error}"))?;
        if version.yanked {
            continue;
        }
        let Some(parsed) = parse_version(&version.vers) else {
            continue;
        };
        if parsed.prerelease && !include_prerelease {
            continue;
        }
        if latest
            .as_ref()
            .is_none_or(|(latest_version, _)| parsed > *latest_version)
        {
            latest = Some((parsed, version.vers));
        }
    }

    Ok(latest
        .filter(|(latest_version, _)| latest_version > &current)
        .map(|(_, version)| version))
}

fn parse_version(value: &str) -> Option<ComparableVersion> {
    let value = value.strip_prefix('v').unwrap_or(value);
    let core = value.split_once('+').map_or(value, |(core, _)| core);
    let (core, prerelease) = match core.split_once('-') {
        Some((core, _)) => (core, true),
        None => (core, false),
    };
    let mut parts = [0; 3];
    let mut iter = core.split('.');
    for part in &mut parts {
        *part = iter.next()?.parse().ok()?;
    }
    if iter.next().is_some() {
        return None;
    }
    Some(ComparableVersion { parts, prerelease })
}

#[cfg(test)]
mod tests {
    use super::latest_available_version_from_index;

    #[test]
    fn sparse_index_latest_version_ignores_yanked_and_prerelease() {
        let body = r#"
{"name":"concord","vers":"1.2.0","yanked":false}
{"name":"concord","vers":"1.3.0-alpha.1","yanked":false}
{"name":"concord","vers":"1.3.0","yanked":true}
{"name":"concord","vers":"1.2.1","yanked":false}
"#;

        let latest = latest_available_version_from_index(body, "1.2.0")
            .expect("sparse index body should parse");

        assert_eq!(latest.as_deref(), Some("1.2.1"));
    }

    #[test]
    fn sparse_index_latest_version_returns_none_for_current_version() {
        let body = r#"{"name":"concord","vers":"1.2.0","yanked":false}"#;

        let latest = latest_available_version_from_index(body, "1.2.0")
            .expect("sparse index body should parse");

        assert_eq!(latest, None);
    }
}
