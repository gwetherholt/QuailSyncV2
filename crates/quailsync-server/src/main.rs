use axum::{routing::get, Router};

async fn health() -> &'static str {
    "quailsync-server ok"
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/health", get(health));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    println!("quailsync-server listening on 0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
