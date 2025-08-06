# Use official Rust image as the build environment
FROM rust:1.88 as builder

# Create app directory
WORKDIR /usr/src/link_monitor

# Copy source code and Cargo files
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY config.toml ./

# Build the application in release mode
RUN cargo build --release

# Use a minimal base image for running the app
FROM debian:bookworm-slim

# Install necessary dependencies for running the app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/src/link_monitor/target/release/link_monitor /usr/local/bin/link_monitor

# Copy the config file (can be overridden by mounting)
COPY config.toml /etc/link_monitor/config.toml

# Set working directory
WORKDIR /etc/link_monitor

# Expose no ports (app is CLI)

# Run the application
CMD ["link_monitor"]
