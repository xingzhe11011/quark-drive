use quark_testkit::FakeQuarkServer;

#[tokio::main]
async fn main() {
    if let Ok(port) = std::env::var("FAKE_PORT") {
        if let Ok(port) = port.parse::<u16>() {
            std::env::set_var("FAKE_BIND_PORT", port.to_string());
        }
    }
    let server = FakeQuarkServer::start().await;
    println!("API_BASE={}", server.api_base());
    println!("OSS_BASE={}", server.oss_base());
    println!("按 Ctrl+C 退出");
    let _ = tokio::signal::ctrl_c().await;
}
