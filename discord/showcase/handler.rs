use anyhow::{Context as _, Result, bail, ensure};
use serenity::all::*;
use tokio::sync::Semaphore;

pub fn command() -> CreateCommand {
    CreateCommand::new("showcase")
        .description(
            "Publish a crosshair preview and its original preset JSON in the showcase channel.",
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::Attachment,
                "preset",
                "A single exported crosshair-rs preset (.json)",
            )
            .required(true),
        )
}

pub async fn handle(
    ctx: &Context,
    cmd: &CommandInteraction,
    channel: ChannelId,
    http: &reqwest::Client,
    slot: &Semaphore,
) -> Result<()> {
    cmd.defer_ephemeral(&ctx.http).await?;
    let reply = if cmd.channel_id != channel {
        format!("Use /showcase in <#{channel}>.")
    } else if let Ok(_permit) = slot.try_acquire() {
        publish(ctx, cmd, http)
            .await
            .unwrap_or_else(|error| error.to_string())
    } else {
        "Another preset is rendering. Try again in a moment.".to_owned()
    };
    cmd.edit_response(
        &ctx.http,
        EditInteractionResponse::new()
            .content(reply)
            .allowed_mentions(CreateAllowedMentions::new()),
    )
    .await?;
    Ok(())
}

async fn publish(
    ctx: &Context,
    cmd: &CommandInteraction,
    http: &reqwest::Client,
) -> Result<String> {
    let file = cmd
        .data
        .options()
        .into_iter()
        .find_map(|option| match option.value {
            ResolvedValue::Attachment(file) if option.name == "preset" => Some(file),
            _ => None,
        })
        .context("Attach one exported preset JSON file.")?;
    ensure!(
        file.filename.to_ascii_lowercase().ends_with(".json"),
        "Choose a .json preset export."
    );
    let limit = cmd.attachment_size_limit.min(16 * 1024 * 1024) as usize;
    ensure!(
        file.size as usize <= limit,
        "That file exceeds the showcase upload limit."
    );
    let json = crate::cdn_bytes(http, &file.url, limit).await.context(
        "Could not download the preset within the upload limits. Try attaching it again.",
    )?;
    let author = cmd
        .member
        .as_ref()
        .map_or_else(|| cmd.user.display_name(), |m| m.display_name());
    let (filename, png) = match render(author, &cmd.user.name, &json)
        .await
        .context("The preview worker is unavailable. Try again shortly.")?
    {
        WorkerReply::Preview(filename, png) => (filename, png),
        WorkerReply::Rejected(message) => bail!("{message}"),
    };
    ensure!(
        png.len() <= cmd.attachment_size_limit as usize,
        "The preview exceeds Discord's upload limit."
    );
    let message = cmd
        .channel_id
        .send_message(
            &ctx.http,
            CreateMessage::new()
                .add_file(
                    CreateAttachment::bytes(png, "showcase.png")
                        .description("Crosshair preset preview"),
                )
                .add_file(CreateAttachment::bytes(json, filename))
                .allowed_mentions(CreateAllowedMentions::new()),
        )
        .await
        .context("Could not confirm the post. Check the showcase channel before trying again.")?;
    Ok(format!(
        "Published your preset: {}",
        message.id.link(cmd.channel_id, cmd.guild_id)
    ))
}

#[cfg_attr(not(unix), allow(dead_code))]
enum WorkerReply {
    Preview(String, Vec<u8>),
    Rejected(String),
}

#[cfg(unix)]
async fn render(author: &str, username: &str, json: &[u8]) -> Result<WorkerReply> {
    use std::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixStream,
    };

    async fn read_field(socket: &mut UnixStream, limit: usize) -> Result<Vec<u8>> {
        let len = socket.read_u32().await? as usize;
        ensure!(len > 0 && len <= limit, "Invalid worker response length");
        let mut bytes = vec![0; len];
        socket.read_exact(&mut bytes).await?;
        Ok(bytes)
    }

    tokio::time::timeout(Duration::from_secs(35), async {
        let mut socket = UnixStream::connect("/run/crosshair/showcase.sock").await?;
        for data in [author.as_bytes(), username.as_bytes(), json] {
            socket.write_u32(data.len().try_into()?).await?;
            socket.write_all(data).await?;
        }
        match socket.read_u8().await? {
            0 => {
                let filename = String::from_utf8(read_field(&mut socket, 128).await?)?;
                ensure!(
                    filename.ends_with(".json")
                        && filename
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.')),
                    "Invalid filename"
                );
                Ok(WorkerReply::Preview(
                    filename,
                    read_field(&mut socket, 4 * 1024 * 1024).await?,
                ))
            }
            1 => Ok(WorkerReply::Rejected(String::from_utf8(
                read_field(&mut socket, 256).await?,
            )?)),
            _ => bail!("Invalid worker response"),
        }
    })
    .await?
}

#[cfg(not(unix))]
async fn render(_: &str, _: &str, _: &[u8]) -> Result<WorkerReply> {
    bail!("Showcase rendering requires Linux containers.")
}
