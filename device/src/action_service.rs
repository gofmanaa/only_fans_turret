use crate::actions::Action;
use anyhow::anyhow;
use std::marker::PhantomData;
use std::{path::Path, sync::Arc, thread, time::Duration};
use tokio::time::sleep;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Mutex,
    time::Instant,
};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{error, info, warn};
use tokio::sync::{oneshot};

pub struct Turret;

const ACTION_COOL_DOWN: Duration = Duration::from_millis(300);

impl Turret {
    /// Actions are converted into serial port commands
    fn action_to_command(action: Action) -> String {
        match action {
            Action::Right => "H-1".to_string(),
            Action::Left => "H1".to_string(),
            Action::Up => "V-1".to_string(),
            Action::Down => "V1".to_string(),
            Action::Fire => "FIRE".to_string(),
        }
    }
}

pub struct ActionService<D> {
    writer: Arc<Mutex<tokio::io::WriteHalf<SerialStream>>>,
    last_action: Arc<Mutex<Option<Instant>>>,
    device: PhantomData<D>,
    stream_handler: Arc<Mutex<Option<VideoStreamerHandle>>>,
    stream_factory: Option<Box<dyn Fn() -> VideoStreamerHandle + Send + Sync>>
}

impl ActionService<Turret> {
    /// Create a new ActionService and start reading Arduino output
    #[allow(dead_code)]
    pub async fn new(path: &Path, baud_rate: u32, stream_factory: Option<Box<dyn Fn() -> VideoStreamerHandle + Send + Sync>>) -> anyhow::Result<Self> {
        info!("Open serial port at {}", path.display());

        let port_stream = connect_devic_retry(path, baud_rate).await?;

        // Split serial stream into reader and writer
        let (reader, writer) = tokio::io::split(port_stream);
        let writer = Arc::new(Mutex::new(writer));

        // Spawn background task to read Arduino output
        tokio::spawn(async move {
            info!("Starting Arduino Reader");
            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // port closed
                    Ok(_) => info!("Arduino: {}", line.trim()),
                    Err(e) => {
                        warn!("Serial read error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            writer,
            last_action: Arc::new(Mutex::new(None)),
            device: PhantomData,
            stream_handler: Arc::new(Mutex::new(None)),
            stream_factory,
        })
    }

    /// Try to send an action if cooldown passed
    pub async fn send_action(&self, action: Action) -> anyhow::Result<()> {
        let mut last = self.last_action.lock().await;
        let now = Instant::now();

        if let Some(last_time) = *last
            && now.duration_since(last_time) < ACTION_COOL_DOWN
        {
            warn!("Action {:?} rejected: cooldown active", action);
            return Err(anyhow!("Action {:?} rejected due to cooldown", action));
        }

        *last = Some(now);

        let command_str = Self::action_to_command(action);
        info!("Sending action {:?} as command {}", action, command_str);

        // Write command to Arduino
        let mut writer = self.writer.lock().await;
        let command = format!("{}\n", command_str);
        writer.write_all(command.as_bytes()).await?;
        writer.flush().await?; // ensure immediate send

        Ok(())
    }

    fn action_to_command(action: Action) -> String {
        Turret::action_to_command(action)
    }

    pub async fn start_stream(&self) -> anyhow::Result<()> {
        let mut handler = self.stream_handler.lock().await;
        if handler.is_some() {
            info!("Stream already running");
            return Ok(());
        }

        if let Some(factory) = &self.stream_factory {
            let nwe_handle = factory(); // call closure
            *handler = Some(nwe_handle);
            info!("Video stream started via closure");
        } else {
            warn!("No stream factory configured");
        }

        Ok(())
    }

    pub async fn stop_stream(&self) -> anyhow::Result<()> {
        let mut handler = self.stream_handler.lock().await;
        if let Some(handle) = handler.take() {
                drop(handle);
            info!("Video streamer stopped and handle dropped");
        } else {
            warn!("No stream running to stop");
        }
        Ok(())
    }
}

#[allow(dead_code)]
async fn connect_devic_retry(path: &Path, baud_rate: u32) -> anyhow::Result<SerialStream> {
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

                sleep(RETRY_DELAY).await;
            }
        }
    }

    anyhow::bail!("Failed to connect after {MAX_RETRIES} attempts to {}", path.display());
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
        } else {
            warn!("Video thread handle already taken or stopped.");
        }
    }
}

impl Drop for VideoStreamerHandle {
    fn drop(&mut self) {
        self.stop_video_stream();
    }
}