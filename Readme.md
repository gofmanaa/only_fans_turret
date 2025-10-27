# OnlyFansTurret ;)

## Description

This project is a platform for managing and streaming video from various devices. It includes a Rust-based backend with gRPC services for device communication and control, as well as a web-based frontend for user interaction. The "only_fans_turret" name suggests a specific application or target for the platform, possibly related to remote monitoring or surveillance.

## Features

*   Video streaming: Real-time video streaming from devices.
*   Remote control: Controlling devices remotely.
*   Web interface: A user-friendly web interface for managing devices and viewing streams.

## Prerequisites

*   Rust toolchain
*   Docker (optional, for containerization)

## Build and Run

1.  **Build the project:**

    ```bash
    cargo build --release
    ```

2.  **Run the server:**

    ```bash
    ./target/release/only_fans_turret
    ```

3.  **Access the web interface:**

    Open your web browser and navigate to the appropriate address (e.g., `http://localhost`).

## Project Structure

*   `.dockerignore`, `Dockerfile`, `docker-compose.yml`: Docker-related files for containerization.
*   `Cargo.lock`, `Cargo.toml`: Rust project configuration and dependencies.
*   `proto/`: Protocol buffer definitions for gRPC.
*   `src/`: Source code.
    *   `src/action_service.rs`, `src/actions.rs`: Action-related services and definitions.
    *   `src/app_state.rs`: Application state management.
    *   `src/device_server.rs`: gRPC server for device communication.
    *   `src/gst_v8_stream.rs`: GStreamer and V8 integration for streaming.
    *   `src/handler.rs`: Request handlers.
    *   `src/main.rs`: Main application entry point.
    *   `src/message.rs`: Message handling.
    *   `src/rtp.rs`: RTP-related functionality.
    *   `src/sdp_handler.rs`: SDP handling.
    *   `src/devices/`: Device-specific implementations.
    *   `src/devices/grpc_server.rs`: gRPC server for devices.
    *   `src/devices/mod.rs`: Device module definition.
*   `web/`: Web interface files.
    *   `web/index.html`: Main HTML file.
    *   `web/script.js`: JavaScript file.
    *   `web/logo.png`: Logo image.

## Contributing

Contributions are welcome! Please submit pull requests with detailed descriptions of your changes.

