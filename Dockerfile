# Build stage
FROM rust:1.95-bookworm AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    libudev-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy source
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build release
RUN cargo build --release --locked

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    libserialport-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary
COPY --from=builder /build/target/release/iot__host ./

# Create non-root user
RUN useradd -m -u 1000 iotuser
USER iotuser

# Default configuration
ENV SERIAL_PORT=/dev/ttyUSB0
ENV BAUD_RATE=115200
ENV MQTT_HOST=localhost
ENV MQTT_PORT=1883

# Run the application
CMD ["./iot__host"]
