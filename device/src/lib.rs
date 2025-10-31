pub mod grpc_server;
pub mod action_service;

pub mod actions;
pub mod pb {
    tonic::include_proto!("devices");
}