# syntax=docker/dockerfile:1

# --- Stage 1: frontend (Node exists only here, never in the final image)
FROM node:22-bookworm-slim AS web
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# --- Stage 2: backend (embeds web/dist at compile time)
FROM rust:1-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml ./
COPY crates ./crates
COPY --from=web /app/web/dist ./web/dist
RUN cargo build --release -p server

# --- Stage 3: minimal runtime (glibc + libgcc only)
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /app/target/release/dlmanager /usr/local/bin/dlmanager
EXPOSE 8080
VOLUME ["/data", "/downloads"]
ENTRYPOINT ["/usr/local/bin/dlmanager", "--data-dir", "/data", "--download-dir", "/downloads", "--port", "8080"]
