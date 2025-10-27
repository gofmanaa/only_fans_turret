
#[cfg(feature = "gstream")]
pub mod gstream {
    use anyhow::Context;
    use gst::prelude::*;
    use gstreamer as gst;
    use std::net::{SocketAddr, ToSocketAddrs};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::thread;
    use std::thread::available_parallelism;
    use std::time::Duration;

    use tracing::{error, info};

    /// Struct representing the VP8 UDP streamer
    struct Vp8Streamer {
        pipeline: gst::Pipeline,
    }

    /// broadcast: gst-launch-1.0 v4l2src device=/dev/video0 ! videoconvert ! vp8enc deadline=1 threads=4 ! rtpvp8pay pt=96 ! udpsink host=127.0.0.1 port=5004
    /// read: gst-launch-1.0 udpsrc port=5004 caps="application/x-rtp, media=video, encoding-name=VP8, payload=96" ! rtpvp8depay ! vp8dec ! videoconvert ! autovideosink

    impl Vp8Streamer {
        /// Create a new streamer
        fn new(device: &str, host: SocketAddr) -> anyhow::Result<Vp8Streamer> {
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
                .property("host", host.ip().to_string())
                .property("port", host.port() as i32)
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
        #[allow(dead_code)]
        fn stop(&self) -> anyhow::Result<()> {
            self.pipeline.set_state(gst::State::Null)?;
            info!("Vp8Streamer stop.");
            Ok(())
        }

        /// Access the GStreamer bus for event handling
        #[allow(dead_code)]
        fn bus(&self) -> gst::Bus {
            self.pipeline.bus().unwrap()
        }
    }

    pub fn video_stream_start(video_dev: PathBuf, v8stream_addr: &str) -> thread::JoinHandle<()> {
        let stream_add = resolve_with_retry(v8stream_addr);

        info!(
            "Video device: {}, stream to {}",
            video_dev.display(),
            stream_add
        );

        // -------------------------
        // Vp8VideoStream
        // -------------------------

        let streamer = Arc::new(
            Vp8Streamer::new(video_dev.to_str().unwrap(), stream_add)
                .expect("Failed to create video streamer"),
        );

        thread::spawn(move || {
            if let Err(e) = streamer.start() {
                error!("Failed to start streamer: {e}");
                return;
            }

            info!("Streamer started...");

            loop {
                thread::sleep(Duration::from_secs(1));
            }
        })
    }
    fn resolve_with_retry(addr: &str) -> SocketAddr {
        let mut retries = 0;
        loop {
            match addr.to_socket_addrs() {
                Ok(mut iter) => {
                    if let Some(a) = iter.next() {
                        return a;
                    }
                }
                Err(e) => {
                    retries += 1;
                    error!(
                        "Failed to connect to device {} (attempt {}): {}",
                        addr, retries, e
                    );
                }
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
}
