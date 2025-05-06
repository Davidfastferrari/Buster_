FROM rust:latest AS builder

# Set working directory
WORKDIR /usr/src/app

# Install minimal required system tools
RUN apt-get update && apt-get install -y --no-install-recommends \
build-essential clang cmake git libclang-dev libssl-dev llvm-dev pkg-config && \
rm -rf /var/lib/apt/lists/*

# Copy the whole repo
COPY . .

# Tell Cargo to use path dependencies correctly
RUN cargo build --release

# --- Optional: Create smaller runtime image if needed ---
FROM debian:bullseye-slim

# Copy compiled binary
COPY --from=builder /usr/src/app/target/release/Buster_ /usr/local/bin/Buster_

# Set the startup command
CMD ["Buster_"]
