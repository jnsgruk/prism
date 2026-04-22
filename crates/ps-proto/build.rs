fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protos = &[
        "../../proto/canonical/prism/v1/common.proto",
        "../../proto/canonical/prism/v1/auth.proto",
        "../../proto/canonical/prism/v1/admin.proto",
        "../../proto/canonical/prism/v1/backup.proto",
        "../../proto/canonical/prism/v1/org.proto",
        "../../proto/canonical/prism/v1/config.proto",
        "../../proto/canonical/prism/v1/handlers.proto",
        "../../proto/canonical/prism/v1/metrics.proto",
        "../../proto/canonical/prism/v1/reasoning.proto",
        "../../proto/canonical/prism/v1/insights.proto",
    ];

    // Ensure cargo reruns this script when any proto file changes.
    // Without this, BuildKit's persistent target/ cache mount can serve
    // stale generated code when only proto content changes.
    for proto in protos {
        println!("cargo:rerun-if-changed={proto}");
    }

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(protos, &["../../proto"])?;

    Ok(())
}
