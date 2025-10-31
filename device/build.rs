fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &["proto/device.proto"],
            &["proto"], // specify the root location to search proto dependencies
        )?;

    Ok(())
}
