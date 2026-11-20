# crosshair-rs community bot

The bot welcomes new members with a personal mention and banner. In the showcase channel, `/showcase preset:<file.json>` posts the preview and the original downloadable preset together. Rejected uploads receive a private explanation.

## Run with Docker Compose

1. Create a Discord application in the [Developer Portal](https://discord.com/developers/applications). Under **Bot**, enable **Server Members Intent** and obtain the bot token.
2. Install the app in your server with the `bot` and `applications.commands` scopes. Grant **View Channel**, **Send Messages** and **Attach Files** in the welcome and showcase channels. Members need **Use Application Commands** in the showcase channel.
3. In Discord, enable **Developer Mode** and use **Copy ID** for the server and each channel. From this directory, copy `.env.example` to `.env` and fill in the four values.
4. Start both services with `docker compose up --build -d`. Use `docker compose logs bot` to see when the slash command is registered.

The welcome number is the server's current member count. Showcase accepts one exported preset and renders its base state. Presets with native text layers need outlined SVG text for Linux. The showcase worker runs offline, processes one upload at a time, and shares a local socket with the bot.

Manrope uses the [SIL Open Font License](assets/OFL.txt). The renderer artwork credits are in the [welcome notice](welcome/NOTICE.md) and [showcase notice](showcase/NOTICE.md).
