fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protos = &[
        "../../proto/prism/v1/auth.proto",
        "../../proto/prism/v1/admin.proto",
        "../../proto/prism/v1/org.proto",
        "../../proto/prism/v1/config.proto",
        "../../proto/prism/v1/handlers.proto",
        "../../proto/prism/v1/metrics.proto",
        "../../proto/prism/v1/reasoning.proto",
        "../../proto/prism/v1/insights.proto",
    ];

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(protos, &["../../proto"])?;

    Ok(())
}
