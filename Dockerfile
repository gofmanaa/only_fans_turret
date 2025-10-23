FROM  rust:1.90.0-bookworm AS builder

RUN apt-get update && apt-get install -y protobuf-compiler pkg-config libssl-dev \
        libgstreamer1.0-dev \
        libgstreamer-plugins-base1.0-dev \
        gstreamer1.0-plugins-base \
        gstreamer1.0-plugins-good \
        gstreamer1.0-plugins-bad \
        gstreamer1.0-plugins-ugly \
        gstreamer1.0-libav \
        gstreamer1.0-tools \
        gstreamer1.0-x \
        gstreamer1.0-gl \
        gstreamer1.0-alsa \
        gstreamer1.0-pulseaudio \
     && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY proto ./proto

RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release || true

COPY . .

RUN cargo build -r --features gstream

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y curl bash ca-certificates \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-0 \
     && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/main /usr/local/bin/http_server
COPY --from=builder /app/target/release/device_server /usr/local/bin/device_server

CMD ["device_server"]
# For http_server container
# CMD ["http_server"]