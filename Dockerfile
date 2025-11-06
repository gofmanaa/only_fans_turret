FROM rust:1.90.0-bookworm AS builder

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
COPY Cargo.lock Cargo.lock ./
COPY device ./device
COPY device_server ./device_server
COPY web_server ./web_server

RUN cargo build -r --features gstream

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y curl bash ca-certificates \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libgstreamer-plugins-bad1.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav gstreamer1.0-tools gstreamer1.0-x gstreamer1.0-alsa gstreamer1.0-gl gstreamer1.0-gtk3 gstreamer1.0-qt5 gstreamer1.0-pulseaudio \
    v4l-utils \
    iputils-ping \
    netcat-openbsd \
    iproute2 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/web_server/web /app/web
COPY --from=builder /app/target/release/web_server /app/web_server
COPY --from=builder /app/target/release/device_server /usr/local/bin/device_server

CMD ["./web_server"]
# For http_server container
# CMD ["device_server"]
