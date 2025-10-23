use crate::actions::Action;
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Mutex,
    time::Instant,
};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{error, info, warn};

pub struct ActionService {
    writer: Arc<Mutex<tokio::io::WriteHalf<SerialStream>>>,
    last_action: Arc<Mutex<Option<Instant>>>,
}

impl ActionService {
    /// Create a new ActionService and start reading Arduino output
    pub async fn new(path: &Path, baud_rate: u32) -> anyhow::Result<Self> {
        info!("Open serial port at {}", path.display());
        let port = tokio_serial::new(path.display().to_string(), baud_rate)
            .open_native_async()
            .expect("Could not open serial port");

        // Split serial stream into reader and writer
        let (reader, writer) = tokio::io::split(port);
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
        })
    }

    /// Try to send an action if cooldown passed
    pub async fn send_action(&self, action: Action) -> anyhow::Result<()> {
        let mut last = self.last_action.lock().await;
        let now = Instant::now();

        if let Some(last_time) = *last
            && now.duration_since(last_time) < Duration::from_millis(300)
        {
            warn!("Action {:?} rejected: cooldown active", action);
            return Ok(());
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
        match action {
            Action::Right => "H1".to_string(),
            Action::Left => "H-1".to_string(),
            Action::Up => "V-1".to_string(),
            Action::Down => "V1".to_string(),
            Action::Fire => "FIRE".to_string(),
        }
    }
}
