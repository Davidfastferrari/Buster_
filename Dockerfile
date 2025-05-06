# Use nightly Rust that supports edition2024
FROM rustlang/rust:nightly AS builder

# Set working directory
WORKDIR /usr/src/app

# Install minimal required system tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential clang cmake git libclang-dev libssl-dev llvm-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*

# Copy the whole repo
COPY . .

# Optional: ensure nightly is used explicitly (safe measure)
RUN rustup override set nightly

# Build the project in release mode
#RUN cargo build --release
# Build the project in release mode
RUN cargo build --release -p Buster_

# --- Optional: Create smaller runtime image if needed ---
FROM debian:bullseye-slim

# Install libssl because some Rust apps need it at runtime (optional, safe to add)
RUN apt-get update && apt-get install -y --no-install-recommends libssl1.1 ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy compiled binary
COPY --from=builder /usr/src/app/target/release/Buster_ /usr/local/bin/Buster_

# Set the startup command
CMD ["Buster_"]
