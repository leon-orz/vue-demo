mod api;
mod collector;
mod store;

#[tokio::main]
async fn main() {
    // TODO(S2-1)：生产环境仅监听 Tailscale 网卡，开发期先绑回环地址
    let addr = "127.0.0.1:7130";
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind hub address");
    println!("toogl-hub listening on http://{addr}");
    axum::serve(listener, api::router()).await.expect("serve");
}
