# OnlyFansTurret ;)


<div style="text-align: center;">
  <img src="web/mascot_200.png" alt="OnlyFansTurret" width="200">
</div>

## Description

**OnlyFansTurret** is an interactive web service that allows users to remotely control a toy turret and watch its live video stream.

At any given time, **one user has full control** over the turret, including rotation, tilt, and firing rockets.  
Other users can **only watch the camera feed**.

Access is managed through a **queue system**: the next user in line gains control once the current session ends.

## Features

*   Video streaming: Real-time video streaming from devices.
*   Remote control: Controlling devices remotely.
*   Web interface: A user-friendly web interface for managing devices and viewing streams.

## How this works:
The service consists of three parts:
- **Device server** – manages the device, accepts commands via [gRPC](https://grpc.io/), and streams video via [GStreamer](https://gstreamer.freedesktop.org/).
- **Web server** – renders the web page and video, using WebSocket (WS) for server communication.
- **Device** - communicates and receives commands via the serial port.

## Prerequisites

*   Rust toolchain
*   Docker (optional, for containerization)

## Build and Run

1.  **Build the project and run the server:**

    ```bash
    docker compose up
    ```
    
## Demo video
[🎬 Watch demo video](web/demo.mp4)


## Contributing

Contributions are welcome! Please submit pull requests with detailed descriptions of your changes.

