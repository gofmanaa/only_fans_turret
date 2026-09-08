use crate::actions::Action;
use anyhow::anyhow;
use std::{marker::PhantomData, path::Path, sync::Arc, thread, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{Mutex, oneshot},
    time::{Instant, sleep},
};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{error, info, warn};

const ACTION_COOL_DOWN: Duration = Duration::from_millis(300);

pub struct Turret;
pub struct CameraOnly;

impl Turret {
    fn action_to_command(action: Action) -> &'static str {
        match action {
            Action::Right => "H-1",
            Action::Left => "H1",
            Action::Up => "V-1",
            Action::Down => "V1",
            Action::Fire => "FIRE",
        }
    }
}

struct DeviceState {
    writer: Option<tokio::io::WriteHalf<SerialStream>>,
    last_action: Option<Instant>,
    stream_handler: Option<VideoStreamerHandle>,
    stream_factory: Option<Box<dyn Fn() -> VideoStreamerHandle + Send + Sync>>,
}

pub struct ActionService<D> {
    state: Arc<Mutex<DeviceState>>,
    device: PhantomData<D>,
}

impl<D> ActionService<D> {
    /// Create service with serial device.
    pub async fn new(
        path: &Path,
        baud_rate: u32,
        stream_factory: Option<Box<dyn Fn() -> VideoStreamerHandle + Send + Sync>>,
    ) -> anyhow::Result<Self> {
        info!("Opening serial port at {}", path.display());

        let port_stream = connect_device_retry(path, baud_rate).await?;
        let (reader, writer) = tokio::io::split(port_stream);

        tokio::spawn(async move {
            info!("Starting Arduino reader");

            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            loop {
                line.clear();

                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        info!("Arduino serial port closed");
                        break;
                    }

                    Ok(_) => {
                        info!("Arduino: {}", line.trim());
                    }

                    Err(e) => {
                        warn!("Serial read error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            state: Arc::new(Mutex::new(DeviceState {
                writer: Some(writer),
                last_action: None,
                stream_handler: None,
                stream_factory,
            })),
            device: PhantomData,
        })
    }

    /// Create service without serial device.
    ///
    /// Useful for CameraOnly.
    pub fn new_stream(stream_factory: Box<dyn Fn() -> VideoStreamerHandle + Send + Sync>) -> Self {
        Self {
            state: Arc::new(Mutex::new(DeviceState {
                writer: None,
                last_action: None,
                stream_handler: None,
                stream_factory: Some(stream_factory),
            })),
            device: PhantomData,
        }
    }

    pub async fn start_stream(&self) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;

        if state.stream_handler.is_some() {
            info!("Stream already running");
            return Ok(());
        }

        let Some(factory) = state.stream_factory.as_ref() else {
            warn!("No stream factory configured");
            return Ok(());
        };

        let handle = factory();

        state.stream_handler = Some(handle);

        info!("Video stream started");

        Ok(())
    }

    pub async fn stop_stream(&self) -> anyhow::Result<()> {
        let handle = {
            let mut state = self.state.lock().await;

            match state.stream_handler.take() {
                Some(handle) => handle,

                None => {
                    warn!("No stream running to stop");
                    return Ok(());
                }
            }
        };

        // tokio::task::spawn_blocking(move || {
        drop(handle);
        // })
        // .await?;

        info!("Video streamer stopped");

        Ok(())
    }
}

impl ActionService<Turret> {
    pub async fn send_action(&self, action: Action) -> anyhow::Result<()> {
        let now = Instant::now();

        let mut state = self.state.lock().await;

        if let Some(last_time) = state.last_action
            && now.duration_since(last_time) < ACTION_COOL_DOWN
        {
            warn!("Action {:?} rejected: cooldown active", action);

            return Err(anyhow!("Action {:?} rejected due to cooldown", action));
        }

        let writer = state
            .writer
            .as_mut()
            .ok_or_else(|| anyhow!("Serial device is not connected"))?;

        let command_str = Turret::action_to_command(action);
        let command = format!("{command_str}\n");

        info!("Sending action {:?} as command {}", action, command_str);

        writer.write_all(command.as_bytes()).await?;
        writer.flush().await?;

        state.last_action = Some(now);

        Ok(())
    }
}

async fn connect_device_retry(path: &Path, baud_rate: u32) -> anyhow::Result<SerialStream> {
    const MAX_RETRIES: u32 = 100;
    const RETRY_DELAY: Duration = Duration::from_secs(2);

    for attempt in 1..=MAX_RETRIES {
        match tokio_serial::new(path.display().to_string(), baud_rate).open_native_async() {
            Ok(client) => {
                info!("Connected to device at {}", path.display());
                return Ok(client);
            }

            Err(e) => {
                warn!(
                    "Failed to connect to device {} (attempt {}): {}",
                    path.display(),
                    attempt,
                    e
                );

                if attempt < MAX_RETRIES {
                    sleep(RETRY_DELAY).await;
                }
            }
        }
    }

    anyhow::bail!(
        "Failed to connect after {MAX_RETRIES} attempts to {}",
        path.display()
    );
}

pub struct VideoStreamerHandle {
    pub stop_tx: Option<oneshot::Sender<()>>,
    pub handle: Option<thread::JoinHandle<()>>,
}

impl VideoStreamerHandle {
    fn stop_video_stream(&mut self) {
        info!("Sending stop signal to video thread...");

        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }

        if let Some(handle) = self.handle.take() {
            if let Err(e) = handle.join() {
                error!("Failed to join video thread: {:?}", e);
            } else {
                info!("Video thread stopped cleanly.");
            }
        }
    }
}

impl Drop for VideoStreamerHandle {
    fn drop(&mut self) {
        self.stop_video_stream();
    }
}
