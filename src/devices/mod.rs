pub mod grpc_server;

pub mod pb {
    tonic::include_proto!("devices");
}
