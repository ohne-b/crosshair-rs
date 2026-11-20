FROM rust:1.97.1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked --workspace --bins

FROM debian:bookworm-slim
RUN install -d -m 0700 -o 10001 -g 10001 /run/crosshair
COPY --from=build /src/target/release/community-bot /src/target/release/showcase-worker /usr/local/bin/
COPY assets/OFL.txt /usr/share/doc/community-bot/Manrope-OFL.txt
COPY welcome/LICENSE.md welcome/NOTICE.md /usr/share/doc/community-bot/welcome/
COPY showcase/LICENSE.md showcase/NOTICE.md /usr/share/doc/community-bot/showcase/
USER 10001:10001
CMD ["community-bot"]
