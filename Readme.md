# OnlyFansTurret ;)

## Description

**OnlyFansTurret** is an interactive web service that allows users to remotely control a toy turret and watch its live video stream.

At any given time, **one user has full control** over the turret, including rotation, tilt, and firing rockets.  
Other users can **only watch the camera feed**.

Access is managed through a **queue system**: the next user in line gains control once the current session ends.

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

## Contributing

Contributions are welcome! Please submit pull requests with detailed descriptions of your changes.

