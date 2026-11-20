use image::RgbaImage;
use tiny_skia::{Mask, Pixmap};

use crate::{drawing, images, style, text::Typography, Error, Result};

/// The caller supplies the member number. The renderer does not count members,
/// fetch avatars, retain user data, or connect to a Discord account.
#[derive(Clone, Copy)]
pub struct WelcomeCard<'a> {
    pub display_name: &'a str,
    pub member_number: u64,
    /// PNG, JPEG or WebP bytes. `None` selects the bundled sample avatar.
    pub avatar: Option<&'a [u8]>,
}

/// A reusable, CPU-only renderer for the approved fixed banner design.
///
/// Construct once and keep the font bytes alive. Prepared artwork and parsed
/// font faces are reused. Rendering takes `&self`, with no shared mutable state.
/// Returned images have **straight** RGBA, suitable for PNG encoding.
pub struct WelcomeRenderer<'font> {
    typography: Typography<'font>,
    background: Pixmap,
    logo: Pixmap,
    default_avatar: Pixmap,
    avatar_mask: Mask,
}

impl<'font> WelcomeRenderer<'font> {
    /// Supply the bundled variable Manrope font.
    pub fn new(manrope: &'font [u8]) -> Result<Self> {
        let typography = Typography::new(manrope)?;
        let avatar_mask = images::avatar_mask()?;
        Ok(Self {
            typography,
            background: drawing::background()?,
            logo: images::logo()?,
            default_avatar: images::default_avatar(&avatar_mask)?,
            avatar_mask,
        })
    }

    /// Render a 1200 × 400 image. No files are created by this method.
    pub fn render(&self, card: WelcomeCard<'_>) -> Result<RgbaImage> {
        if !(1..1_000_000_000).contains(&card.member_number) {
            return Err(Error::InvalidMemberNumber);
        }
        // Reject invalid text before doing potentially expensive image work.
        let name = crate::text::normalize_name(card.display_name)?;
        let decoded_avatar = card
            .avatar
            .map(|bytes| images::avatar(bytes, &self.avatar_mask))
            .transpose()?;
        let avatar = decoded_avatar.as_ref().unwrap_or(&self.default_avatar);
        let mut canvas = self.background.clone();
        drawing::over_at(&mut canvas, avatar, style::AVATAR_CENTER);
        drawing::cover_bolt_holes(&mut canvas)?;
        drawing::over_at(&mut canvas, &self.logo, style::AVATAR_CENTER);
        self.typography.draw(&mut canvas, &name, card.member_number);
        images::finish(canvas)
    }

    /// Convenience API for an attachment, HTTP response, or local PNG file.
    pub fn render_png(&self, card: WelcomeCard<'_>) -> Result<Vec<u8>> {
        images::encode_png(&self.render(card)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_can_be_shared_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WelcomeRenderer<'static>>();
    }

    #[test]
    fn malformed_font_is_rejected() {
        assert!(WelcomeRenderer::new(b"invalid font").is_err());
    }
}
