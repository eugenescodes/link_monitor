# ---------- Build stage ----------
FROM rust:1.93 AS builder

WORKDIR /app
COPY . .

# Build the project in release mode
RUN cargo build --release

# ---------- Runtime stage ----------
FROM debian:stable-slim

# Install only required system packages
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy only the compiled binary and config file
COPY --from=builder /app/target/release/link_monitor /app/monitor
COPY config.toml /app/config.toml

# Run the binary
CMD ["/app/monitor"]
