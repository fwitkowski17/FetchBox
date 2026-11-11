# syntax=docker/dockerfile:1

# --- Stage 1: frontend (Node exists only here, never in the final image)
FROM node:22-trixie-slim AS web
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# --- Stage 2: backend (embeds web/dist at compile time)
FROM rust:1-trixie AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml ./
COPY crates ./crates
# Plugins that ship inside the binary (embedded at compile time).
COPY plugins ./plugins
COPY --from=web /app/web/dist ./web/dist
RUN cargo build --release -p server

# --- Stage 3: minimal runtime (glibc + libgcc only). Same Debian release (13, "trixie") as the build
#     stage, so the binary meets the glibc it was linked against.
FROM gcr.io/distroless/cc-debian13:nonroot
COPY --from=build /app/target/release/fetchbox /usr/local/bin/fetchbox
# Tells the app how it was installed, so its update notice shows the Docker Compose steps.
ENV FETCHBOX_INSTALL_METHOD=docker
# Inside the container the web interface is on port 80; docker-compose.yml publishes it as 8080 on
# the host. It is an environment variable, not a flag, so that `-e FETCHBOX_PORT=...` changes it. (A
# normal user listens below 1024 because Docker sets net.ipv4.ip_unprivileged_port_start=0 in
# containers; docker-compose.yml says so explicitly.)
ENV FETCHBOX_PORT=80
EXPOSE 80 6881/tcp 6881/udp
VOLUME ["/data", "/downloads"]
ENTRYPOINT ["/usr/local/bin/fetchbox", "--data-dir", "/data", "--download-dir", "/downloads"]
