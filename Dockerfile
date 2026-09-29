# ---------- Build stage ----------
# Official Docker Hub images. Builder and runtime must use the same Debian
# release (glibc compatibility): trixie in both stages. The Rust version is
# bumped by Dependabot; `debian:trixie-slim` has no version number and gets
# security rebuilds via `docker build --pull`. Move both to the next Debian
# release together; the runtime stage verifies below that the binary's
# libraries are satisfied.
FROM rust:1.98.1-slim-trixie AS builder

WORKDIR /app

# 1. Compile dependencies in their own layer. This layer is only invalidated
#    when Cargo.toml / Cargo.lock change, so day-to-day source edits reuse the
#    (expensive) dependency build instead of recompiling everything from scratch.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
 && echo 'fn main() {}' > src/main.rs \
 && cargo build --release --locked \
 && rm -rf src

# 2. Build the real binary, reusing the cached dependency artifacts above.
#    --locked ensures the exact versions in Cargo.lock are used, never
#    silently re-resolved during the image build. `touch` makes sure cargo
#    rebuilds the crate even if the copied sources are older than the stub.
COPY src ./src
RUN touch src/main.rs src/lib.rs \
 && cargo build --release --locked

# Stage the binary (stripped via `[profile.release] strip = true`) and an
# empty log directory for the runtime stage.
RUN cp target/release/link_monitor /app/monitor \
 && mkdir -p /app/logs

# ---------- Runtime stage ----------
FROM debian:trixie-slim

# CA certificates for HTTPS targets and a dedicated non-root user.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --user-group --no-create-home \
      --shell /usr/sbin/nologin appuser

WORKDIR /app

# Copy only the compiled binary, the default config and a log directory
# writable by the non-root user.
COPY --from=builder /app/monitor /app/monitor
COPY config.toml /app/config.toml
COPY --from=builder --chown=appuser:appuser /app/logs /app/logs

# Fail the build (instead of the container at startup) if the builder used a
# newer Debian/glibc than this image, e.g. when only one of the base images
# above was moved to a new Debian release.
RUN if ldd /app/monitor | grep 'not found'; then \
      echo 'ERROR: /app/monitor needs libraries or glibc versions missing in the runtime image;' \
           'use the same Debian release in both FROM lines' >&2; \
      exit 1; \
    fi

USER appuser

# Run the binary
CMD ["/app/monitor"]
