#[cfg(feature = "gstream")]
pub mod gstream {
    use anyhow::Context;
    use gst::prelude::*;
    use gstreamer as gst;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::thread::available_parallelism;
    use std::time::Duration;

    use tracing::{debug, error, info};

    /// Struct representing the VP8 UDP streamer
    struct Vp8Streamer {
        pipeline: gst::Pipeline,
    }

    // broadcast: gst-launch-1.0 v4l2src device=/dev/video0 ! videoconvert ! vp8enc deadline=1 threads=4 ! rtpvp8pay pt=96 ! udpsink host=127.0.0.1 port=5004
    // read(test): gst-launch-1.0 udpsrc port=5004 caps="application/x-rtp, media=video, encoding-name=VP8, payload=96" ! rtpvp8depay ! vp8dec ! videoconvert ! autovideosink

    impl Vp8Streamer {
        /// Create a new streamer
        fn new(device: &str, url: SocketAddr) -> anyhow::Result<Vp8Streamer> {
            gst::init()
                .context("Failed to initialize GStreamer")
                .expect("Failed to initialize GStreamer");

            let pipeline = gst::Pipeline::new();

            let src = gst::ElementFactory::make("v4l2src")
                .property("device", device)
                .build()
                .context("Failed to create v4l2src")?;

            let convert = gst::ElementFactory::make("videoconvert")
                .build()
                .context("Failed to create videoconvert")
                .expect("Failed to create video convert");

            let default_parallelism_approx = available_parallelism()?.get();

            let encoder = gst::ElementFactory::make("vp8enc")
                .property("deadline", 1i64)
                .property("threads", default_parallelism_approx as i32)
                .build()
                .context("Failed to create vp8enc")
                .expect("Failed to create vp8enc");

            let payloader = gst::ElementFactory::make("rtpvp8pay")
                .property("pt", 96u32)
                .build()
                .context("Failed to create rtpvp8pay")
                .expect("Failed to create rtpvp8pay");

            let sink = gst::ElementFactory::make("udpsink")
                .property("host", url.ip().to_string())
                .property("port", url.port() as i32)
                .build()
                .context("Failed to create udpsink")
                .expect("Failed to create udpsink");

            pipeline
                .add_many([&src, &convert, &encoder, &payloader, &sink])
                .context("Failed to add elements to pipeline")
                .expect("Failed to add elements to pipeline");
            gst::Element::link_many([&src, &convert, &encoder, &payloader, &sink])
                .context("Failed to link elements")
                .expect("Failed to link elements");
            Ok(Self { pipeline })
        }

        /// Start streaming
        fn start(&self) -> anyhow::Result<()> {
            self.pipeline.set_state(gst::State::Playing)?;
            info!("Vp8Streamer start.");
            Ok(())
        }

        /// Stop streaming
        fn stop(&self) -> anyhow::Result<()> {
            self.pipeline.set_state(gst::State::Null)?;
            info!("Vp8Streamer stop.");
            Ok(())
        }

        /// Access the GStreamer bus for event handling
        fn bus(&self) -> gst::Bus {
            self.pipeline.bus().unwrap()
        }
    }

    pub fn video_stream_start(
        video_dev: PathBuf,
        v8stream_url: SocketAddr,
    ) -> std::thread::JoinHandle<()> {
        info!(
            "Video device: {}, stream to {}",
            video_dev.display(),
            v8stream_url
        );

        // -------------------------
        // Vp8VideoStream
        // -------------------------

        let streamer = Arc::new(
            Vp8Streamer::new(video_dev.to_str().unwrap(), v8stream_url)
                .expect("Failed to create video streamer"),
        );

        std::thread::spawn(move || {
            if let Err(e) = streamer.start() {
                error!("Failed to start streamer: {e}");
                return;
            }

            info!("Streamer started...");

            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        })
    }
}
