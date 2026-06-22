//! alva-app-gateway binary: load a YAML routing config and serve the protocol gateway.
use alva_app_gateway::config::GatewayConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Config path: first CLI arg, else $ALVA_GATEWAY_CONFIG, else ./gateway.yml
    let path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("ALVA_GATEWAY_CONFIG").ok())
        .unwrap_or_else(|| "gateway.yml".to_string());
    let yaml =
        std::fs::read_to_string(&path).map_err(|e| format!("read gateway config {path}: {e}"))?;
    let cfg = GatewayConfig::from_yaml(&yaml)?;
    let router = cfg.build_router()?;
    println!("alva-app-gateway listening on {}", cfg.listen);
    alva_app_gateway::serve(router, &cfg.listen)
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    Ok(())
}
