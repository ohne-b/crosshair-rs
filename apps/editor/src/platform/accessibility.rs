use anyhow::{Context, Result};
use std::sync::Arc;
use windows::{
    Foundation::TypedEventHandler,
    UI::ViewManagement::{AccessibilitySettings, UISettings},
    Win32::Graphics::Gdi::{
        COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor,
    },
    core::IInspectable,
};

#[derive(Clone)]
pub struct Appearance {
    pub text_scale: f32,
    pub animations: bool,
    pub colors: Option<[u32; 4]>,
}

pub struct Watcher {
    settings: UISettings,
    _accessibility: AccessibilitySettings,
    text_token: i64,
    color_token: Option<i64>,
}

impl Watcher {
    pub fn new(changed: Arc<dyn Fn(Appearance) + Send + Sync>) -> Result<Self> {
        let settings = UISettings::new().context("UISettings activation")?;
        let accessibility =
            AccessibilitySettings::new().context("AccessibilitySettings activation")?;
        changed(read(&settings, &accessibility).context("Reading initial appearance")?);
        let callback = changed.clone();
        let contrast = accessibility.clone();
        let text_token = settings
            .TextScaleFactorChanged(&TypedEventHandler::<UISettings, IInspectable>::new(
                move |sender, _| {
                    if let Some(sender) = sender.as_ref()
                        && let Ok(appearance) = read(sender, &contrast)
                    {
                        callback(appearance);
                    }
                    Ok(())
                },
            ))
            .context("Subscribing to text scaling")?;
        let contrast = accessibility.clone();
        let color_token = settings
            .ColorValuesChanged(&TypedEventHandler::<UISettings, IInspectable>::new(
                move |sender, _| {
                    if let Some(sender) = sender.as_ref()
                        && let Ok(appearance) = read(sender, &contrast)
                    {
                        changed(appearance);
                    }
                    Ok(())
                },
            ))
            .ok();
        Ok(Self {
            settings,
            _accessibility: accessibility,
            text_token,
            color_token,
        })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.settings.RemoveTextScaleFactorChanged(self.text_token);
        if let Some(token) = self.color_token {
            let _ = self.settings.RemoveColorValuesChanged(token);
        }
    }
}

pub fn current() -> Result<Appearance> {
    read(&UISettings::new()?, &AccessibilitySettings::new()?)
}

fn read(settings: &UISettings, accessibility: &AccessibilitySettings) -> Result<Appearance> {
    let colors = if accessibility
        .HighContrast()
        .context("Reading high contrast")?
    {
        Some(unsafe {
            [
                GetSysColor(COLOR_WINDOW),
                GetSysColor(COLOR_WINDOWTEXT),
                GetSysColor(COLOR_HIGHLIGHT),
                GetSysColor(COLOR_HIGHLIGHTTEXT),
            ]
        })
    } else {
        None
    };
    Ok(Appearance {
        animations: settings
            .AnimationsEnabled()
            .context("Reading animation preference")?,
        text_scale: (settings.TextScaleFactor().context("Reading text scale")? as f32)
            .clamp(1.0, 2.25),
        colors,
    })
}
