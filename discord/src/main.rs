use std::{env, num::NonZeroU64, time::Duration};

use anyhow::{Context as _, Result, ensure};
use serenity::{all::*, async_trait};
use tokio::sync::Semaphore;

#[path = "../showcase/handler.rs"]
mod showcase;
#[path = "../welcome/handler.rs"]
mod welcome;

struct Bot {
    guild: GuildId,
    welcome_channel: ChannelId,
    showcase_channel: ChannelId,
    welcome: welcome::Welcome,
    showcase_slot: Semaphore,
    cdn: reqwest::Client,
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, _: Ready) {
        match self
            .guild
            .create_command(&ctx.http, showcase::command())
            .await
        {
            Ok(_) => println!("Community bot ready."),
            Err(error) => eprintln!("Could not register /showcase: {error}"),
        }
    }

    async fn guild_member_addition(&self, ctx: Context, member: Member) {
        if member.guild_id != self.guild || member.user.bot {
            return;
        }
        if self
            .welcome
            .joined(&ctx, &member, self.welcome_channel, &self.cdn)
            .await
            .is_err()
        {
            eprintln!("Could not send a welcome message.");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Interaction::Command(command) = interaction else {
            return;
        };
        if command.guild_id != Some(self.guild) || command.data.name != "showcase" {
            return;
        }
        if showcase::handle(
            &ctx,
            &command,
            self.showcase_channel,
            &self.cdn,
            &self.showcase_slot,
        )
        .await
        .is_err()
        {
            eprintln!("Could not acknowledge the showcase submission.");
        }
    }
}

fn id(name: &str) -> Result<NonZeroU64> {
    env::var(name)
        .with_context(|| format!("Set {name}"))?
        .parse()
        .with_context(|| format!("{name} must be a nonzero Discord ID"))
}

pub(crate) async fn cdn_bytes(
    http: &reqwest::Client,
    url: &str,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let url = reqwest::Url::parse(url)?;
    ensure!(
        url.scheme() == "https"
            && matches!(
                url.host_str(),
                Some("cdn.discordapp.com" | "media.discordapp.net")
            ),
        "invalid Discord CDN URL"
    );
    let mut response = http.get(url).send().await?.error_for_status()?;
    ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= max_bytes as u64),
        "file exceeds size limit"
    );
    let mut data = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            chunk.len() <= max_bytes - data.len(),
            "file exceeds size limit"
        );
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}

#[cfg(test)]
#[tokio::test]
async fn downloads_require_a_discord_https_address() {
    let http = reqwest::Client::new();
    for url in ["http://cdn.discordapp.com/file", "https://example.org/file"] {
        assert_eq!(
            cdn_bytes(&http, url, 1024).await.unwrap_err().to_string(),
            "invalid Discord CDN URL"
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let bot = Bot {
        guild: GuildId::new(id("DISCORD_GUILD_ID")?.get()),
        welcome_channel: ChannelId::new(id("WELCOME_CHANNEL_ID")?.get()),
        showcase_channel: ChannelId::new(id("SHOWCASE_CHANNEL_ID")?.get()),
        welcome: welcome::Welcome::new()?,
        showcase_slot: Semaphore::new(1),
        cdn: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
    };
    let token = env::var("DISCORD_TOKEN").context("Set DISCORD_TOKEN")?;
    let intents = GatewayIntents::GUILDS | GatewayIntents::GUILD_MEMBERS;
    let mut client = Client::builder(token, intents).event_handler(bot).await?;
    client.start().await?;
    Ok(())
}
