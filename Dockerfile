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
COPY --from=web /app/web/dist ./web/dist
RUN cargo build --release -p server

# --- Stage 3: minimal runtime (glibc + libgcc only). Same Debian release (13, "trixie") as the build
#     stage, so the binary meets the glibc it was linked against.
FROM gcr.io/distroless/cc-debian13:nonroot
COPY --from=build /app/target/release/fetchbox /usr/local/bin/fetchbox
# Tells the app how it was installed, so its update notice shows the Docker Compose steps.
ENV FETCHBOX_INSTALL_METHOD=docker
EXPOSE 8080
VOLUME ["/data", "/downloads"]
ENTRYPOINT ["/usr/local/bin/fetchbox", "--data-dir", "/data", "--download-dir", "/downloads", "--port", "8080"]
