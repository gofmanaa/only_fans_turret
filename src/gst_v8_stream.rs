use anyhow::{Context, Result};
use gst::prelude::*;
use gstreamer as gst;
use std::error::Error;
use tracing::debug;

/// Struct representing the VP8 UDP streamer
pub struct Vp8Streamer {
    pipeline: gst::Pipeline,
}

// broadcast: gst-launch-1.0 v4l2src device=/dev/video0 ! videoconvert ! vp8enc deadline=1 threads=4 ! rtpvp8pay pt=96 ! udpsink host=127.0.0.1 port=5004
// read(test): gst-launch-1.0 udpsrc port=5004 caps="application/x-rtp, media=video, encoding-name=VP8, payload=96" ! rtpvp8depay ! vp8dec ! videoconvert ! autovideosink

impl Vp8Streamer {
    /// Create a new streamer
    pub fn new(device: &str, host: &str, port: u32) -> Result<Self> {
        gst::init().context("Failed to initialize GStreamer")?;

        let pipeline = gst::Pipeline::new();

        let src = gst::ElementFactory::make("v4l2src")
            .property("device", device)
            .build()
            .context("Failed to create v4l2src")?;

        let convert = gst::ElementFactory::make("videoconvert")
            .build()
            .context("Failed to create videoconvert")?;

        let encoder = gst::ElementFactory::make("vp8enc")
            .property("deadline", 1i64)
            .property("threads", 4i32)
            .build()
            .context("Failed to create vp8enc")?;

        let payloader = gst::ElementFactory::make("rtpvp8pay")
            .property("pt", 96u32)
            .build()
            .context("Failed to create rtpvp8pay")?;

        let sink = gst::ElementFactory::make("udpsink")
            .property("host", host)
            .property("port", port as i32)
            .build()
            .context("Failed to create udpsink")?;

        pipeline
            .add_many([&src, &convert, &encoder, &payloader, &sink])
            .context("Failed to add elements to pipeline")?;
        gst::Element::link_many([&src, &convert, &encoder, &payloader, &sink])
            .context("Failed to link elements")?;
        Ok(Self { pipeline })
    }

    /// Start streaming
    pub fn start(&self) -> Result<(), Box<dyn Error>> {
        self.pipeline.set_state(gst::State::Playing)?;

        Ok(())
    }

    /// Stop streaming
    pub fn stop(&self) -> Result<(), Box<dyn Error>> {
        self.pipeline.set_state(gst::State::Null)?;
        debug!("Vp8Streamer stop.");
        Ok(())
    }

    /// Access the GStreamer bus for event handling
    pub fn bus(&self) -> gst::Bus {
        self.pipeline.bus().unwrap()
    }
}
