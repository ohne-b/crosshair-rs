use std::sync::Arc;

use anyhow::{Context as _, Result};
use crosshair_welcome::{Error, MAX_AVATAR_BYTES, WelcomeCard, WelcomeRenderer};
use serenity::all::{
    ChannelId, Context, CreateAllowedMentions, CreateAttachment, CreateMessage, Member, Nonce,
};
use tokio::sync::Mutex;

pub struct Welcome {
    renderer: Arc<Mutex<WelcomeRenderer<'static>>>,
}

impl Welcome {
    pub fn new() -> Result<Self> {
        let renderer = WelcomeRenderer::new(include_bytes!("../assets/Manrope.ttf"))?;
        Ok(Self {
            renderer: Arc::new(Mutex::new(renderer)),
        })
    }

    pub async fn joined(
        &self,
        ctx: &Context,
        member: &Member,
        channel: ChannelId,
        http: &reqwest::Client,
    ) -> Result<()> {
        let cached_count = ctx
            .cache
            .guild(member.guild_id)
            .map(|guild| guild.member_count);
        let member_number = match cached_count {
            Some(count) => count,
            None => member
                .guild_id
                .to_partial_guild_with_counts(&ctx.http)
                .await?
                .approximate_member_count
                .context("Discord omitted the server member count")?,
        };
        // Reserve the renderer before downloading so a burst of joins cannot buffer many avatars.
        let renderer = Arc::clone(&self.renderer).lock_owned().await;
        let url = match (&member.avatar, &member.user.avatar) {
            (Some(hash), _) => format!(
                "https://cdn.discordapp.com/guilds/{}/users/{}/avatars/{hash}.png?size=256",
                member.guild_id, member.user.id
            ),
            (_, Some(hash)) => format!(
                "https://cdn.discordapp.com/avatars/{}/{hash}.png?size=256",
                member.user.id
            ),
            _ => member.user.default_avatar_url(),
        };
        let avatar = crate::cdn_bytes(http, &url, MAX_AVATAR_BYTES).await.ok();
        let name = member.display_name().to_owned();
        let png = tokio::task::spawn_blocking(move || {
            let mut card = WelcomeCard {
                display_name: &name,
                member_number,
                avatar: avatar.as_deref(),
            };
            loop {
                match renderer.render_png(card) {
                    Err(Error::InvalidName(_)) if card.display_name != "Member" => {
                        card.display_name = "Member"
                    }
                    Err(
                        Error::Image(_)
                        | Error::InvalidImageDimensions
                        | Error::UnsupportedAvatarFormat,
                    ) if card.avatar.is_some() => card.avatar = None,
                    result => return result,
                }
            }
        })
        .await??;
        let id = member.user.id;
        let message = CreateMessage::new()
            .content(format!("Welcome, <@{id}>!"))
            .allowed_mentions(CreateAllowedMentions::new().users([id]))
            .nonce(Nonce::String(id.to_string()))
            .enforce_nonce(true)
            .add_file(
                CreateAttachment::bytes(png, "welcome.png").description(format!(
                    "Welcome, {}! Member #{member_number}.",
                    member.display_name()
                )),
            );
        channel.send_message(&ctx.http, message).await?;
        Ok(())
    }
}
