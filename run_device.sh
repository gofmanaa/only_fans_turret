#!/usr/bin/env bash

docker run -it --rm \
  --name device_server_tmp \
  --device /dev/ttyUSB0:/dev/ttyUSB0 \
  --device /dev/video0:/dev/video0 \
  -e GRPC_ADDR="0.0.0.0:5001" \
  -e STTY_PATH="/dev/ttyUSB0" \
  -e BAUD_RATE="9600" \
  -e VIDEO_DEV="/dev/video0" \
  -e V8STREAM_ADDR="172.17.0.1:5004" \
  -e RUST_LOG="info" \
  -p 5004:5004 \
  -p 5004:5004/udp \
  tmp_local \
  bash
  #device_server