use super::pb::device_server::{Device, DeviceServer};
use crate::action_service::ActionService;
use crate::actions::Action;
use crate::devices::pb::{CommandRequest, CommandResponse};
use tonic::service::Interceptor;
use tonic::{Request, Response, Status};
use tracing::info;

pub struct GrpcDeviceServer {
    pub action_service: ActionService,
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

        self.action_service
            .send_action(action)
            .await
            .map_err(|e| Status::internal(format!("Action service error: {}", e)))?;

        let reply = CommandResponse {
            action: command.action,
        };

        Ok(Response::new(reply))
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

impl GrpcDeviceServer {
    pub fn new(action_service: ActionService) -> Self {
        Self { action_service }
    }

    pub fn into_service(self) -> DeviceServer<Self> {
        DeviceServer::new(self)
    }
}
