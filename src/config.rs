use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::Result;

const CONFIG_DIR: &str = ".concord";
const CONFIG_FILE: &str = "config.toml";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct DisplayOptions {
    pub disable_image_preview: bool,
    pub show_avatars: bool,
    pub show_images: bool,
    pub image_preview_quality: ImagePreviewQualityPreset,
    pub show_custom_emoji: bool,
    pub desktop_notifications: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImagePreviewQualityPreset {
    Efficient,
    #[default]
    Balanced,
    High,
    Original,
}

impl ImagePreviewQualityPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Efficient => "efficient",
            Self::Balanced => "balanced",
            Self::High => "high",
            Self::Original => "original",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Efficient => Self::Balanced,
            Self::Balanced => Self::High,
            Self::High => Self::Original,
            Self::Original => Self::Efficient,
        }
    }
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            disable_image_preview: false,
            show_avatars: true,
            show_images: true,
            image_preview_quality: ImagePreviewQualityPreset::default(),
            show_custom_emoji: true,
            desktop_notifications: true,
        }
    }
}

impl DisplayOptions {
    pub fn avatars_visible(self) -> bool {
        !self.disable_image_preview && self.show_avatars
    }

    pub fn images_visible(self) -> bool {
        !self.disable_image_preview && self.show_images
    }

    pub fn custom_emoji_visible(self) -> bool {
        !self.disable_image_preview && self.show_custom_emoji
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct AppConfig {
    display: DisplayOptions,
}

pub fn load_display_options() -> Result<DisplayOptions> {
    let path = config_path()?;
    load_display_options_from_path(&path)
}

fn load_display_options_from_path(path: &Path) -> Result<DisplayOptions> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(toml::from_str::<AppConfig>(&content)?.display),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DisplayOptions::default()),
        Err(error) => Err(error.into()),
    }
}

pub fn save_display_options(options: &DisplayOptions) -> Result<()> {
    let path = config_path()?;
    save_display_options_to_path(&path, options)
}

fn save_display_options_to_path(path: &Path, options: &DisplayOptions) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        set_private_dir_permissions(parent)?;
    }

    let config = AppConfig { display: *options };
    write_private_file(&path, &toml::to_string_pretty(&config)?)
}

fn config_path() -> Result<PathBuf> {
    let home = env::var_os("HOME").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "HOME environment variable is not set",
        )
    })?;

    Ok(PathBuf::from(home).join(CONFIG_DIR).join(CONFIG_FILE))
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn write_private_file(path: &Path, content: &str) -> Result<()> {
    use std::{
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };

    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content.as_bytes())?;

    let mut permissions = file.metadata()?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        AppConfig, DisplayOptions, ImagePreviewQualityPreset, load_display_options_from_path,
        save_display_options_to_path,
    };

    #[test]
    fn display_options_default_to_all_media_enabled() {
        let options = DisplayOptions::default();

        assert!(options.avatars_visible());
        assert!(options.images_visible());
        assert!(options.custom_emoji_visible());
        assert_eq!(
            options.image_preview_quality,
            ImagePreviewQualityPreset::Balanced
        );
    }

    #[test]
    fn global_disable_overrides_individual_toggles() {
        let options = DisplayOptions {
            disable_image_preview: true,
            show_avatars: true,
            show_images: true,
            image_preview_quality: ImagePreviewQualityPreset::Balanced,
            show_custom_emoji: true,
            desktop_notifications: true,
        };

        assert!(!options.avatars_visible());
        assert!(!options.images_visible());
        assert!(!options.custom_emoji_visible());
    }

    #[test]
    fn missing_toml_fields_use_defaults() {
        let config: AppConfig = toml::from_str("[display]\ndisable_image_preview = true\n")
            .expect("partial config should parse");

        assert!(config.display.disable_image_preview);
        assert!(config.display.show_avatars);
        assert!(config.display.show_images);
        assert_eq!(
            config.display.image_preview_quality,
            ImagePreviewQualityPreset::Balanced
        );
        assert!(config.display.show_custom_emoji);
        assert!(config.display.desktop_notifications);
    }

    #[test]
    fn parses_image_preview_quality_preset() {
        let config: AppConfig = toml::from_str("[display]\nimage_preview_quality = \"original\"\n")
            .expect("quality preset config should parse");

        assert_eq!(
            config.display.image_preview_quality,
            ImagePreviewQualityPreset::Original
        );
    }

    #[test]
    fn display_options_save_and_load_round_trip() {
        let path = test_config_path();
        let options = DisplayOptions {
            disable_image_preview: true,
            show_avatars: false,
            show_images: false,
            image_preview_quality: ImagePreviewQualityPreset::Original,
            show_custom_emoji: false,
            desktop_notifications: false,
        };

        save_display_options_to_path(&path, &options).expect("config should save");
        let loaded = load_display_options_from_path(&path).expect("config should load");

        assert_eq!(loaded, options);
        let _ = fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    fn test_config_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("concord-config-test-{unique}"))
            .join("config.toml")
    }
}
