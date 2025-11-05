use super::pb::device_server::{Device, DeviceServer};
use crate::action_service::{ActionService, Turret};
use crate::actions::Action;
use crate::pb::{CommandRequest, CommandResponse};
use crate::pb::{StartStreamRequest, StartStreamResponse, StopStreamRequest, StopStreamResponse};
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::service::Interceptor;
use tonic::{Request, Response, Status};
use tracing::info;

#[allow(dead_code)]
pub struct GrpcDeviceServer {
    pub action_service: Arc<Mutex<ActionService<Turret>>>,
}

#[tonic::async_trait]
impl Device for GrpcDeviceServer {
    async fn do_action(
        &self,
        request: Request<CommandRequest>,
    ) -> Result<Response<CommandResponse>, Status> {
        let command = request.into_inner();
        info!("Received action: {:?}", command);
        let action: Action = command.action().into();
        let service = self.action_service.lock().await;
        service
            .send_action(action)
            .await
            .map_err(|e| Status::internal(format!("Action service error: {}", e)))?;

        let reply = CommandResponse {
            action: command.action,
        };

        Ok(Response::new(reply))
    }

    async fn start_stream(
        &self,
        _request: Request<StartStreamRequest>,
    ) -> Result<Response<StartStreamResponse>, Status> {
        let service = self.action_service.lock().await;
        service
            .start_stream()
            .await
            .map_err(|e| Status::internal(format!("Action service error: {}", e)))?;
        Ok(Response::new(StartStreamResponse {}))
    }

    async fn stop_stream(
        &self,
        _request: Request<StopStreamRequest>,
    ) -> Result<Response<StopStreamResponse>, Status> {
        let service = self.action_service.lock().await;
        service
            .stop_stream()
            .await
            .map_err(|e| Status::internal(format!("Action service error: {}", e)))?;

        Ok(Response::new(StopStreamResponse {}))
    }
}

impl Interceptor for GrpcDeviceServer {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        let peer_addr = request
            .extensions()
            .get::<std::net::SocketAddr>()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        info!("Received RPC from: {}", peer_addr);

        Ok(request)
    }
}

#[allow(dead_code)]
impl GrpcDeviceServer {
    pub fn new(action_service: ActionService<Turret>) -> Self {
        Self {
            action_service: Arc::new(Mutex::new(action_service)),
        }
    }

    pub fn into_service(self) -> DeviceServer<Self> {
        DeviceServer::new(self)
    }
}
