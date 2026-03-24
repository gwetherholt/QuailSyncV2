use std::sync::{atomic::AtomicBool, Arc, Mutex};

use quailsync_common::AlertConfig;
use quailsync_server::{auto_backup_if_needed, build_app, init_db, AppState};
use rusqlite::Connection;
use tokio::sync::broadcast;

fn ensure_tls_certs() {
    let cert_path = std::path::Path::new("certs/quailsync.crt");
    let key_path = std::path::Path::new("certs/quailsync.key");
    if cert_path.exists() && key_path.exists() {
        println!("[tls] using existing certs in certs/");
        return;
    }
    println!("[tls] generating self-signed certificate...");
    std::fs::create_dir_all("certs").expect("failed to create certs directory");

    let mut params = rcgen::CertificateParams::new(vec!["localhost".to_string()])
        .expect("failed to create cert params");
    params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
            std::net::Ipv4Addr::new(127, 0, 0, 1),
        )));
    // Add common LAN addresses
    params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
            std::net::Ipv4Addr::new(0, 0, 0, 0),
        )));

    let key_pair = rcgen::KeyPair::generate().expect("failed to generate key pair");
    let cert = params
        .self_signed(&key_pair)
        .expect("failed to generate self-signed cert");

    std::fs::write(cert_path, cert.pem()).expect("failed to write cert");
    std::fs::write(key_path, key_pair.serialize_pem()).expect("failed to write key");
    println!("[tls] saved certs/quailsync.crt and certs/quailsync.key");
}

fn load_tls_config() -> Arc<rustls::ServerConfig> {
    let cert_pem = std::fs::read("certs/quailsync.crt").expect("failed to read cert");
    let key_pem = std::fs::read("certs/quailsync.key").expect("failed to read key");

    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut &cert_pem[..])
            .filter_map(|r| r.ok())
            .collect();
    let key = rustls_pemfile::pkcs8_private_keys(&mut &key_pem[..])
        .next()
        .expect("no private key found")
        .expect("failed to parse private key");

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, rustls::pki_types::PrivateKeyDer::Pkcs8(key))
        .expect("failed to build TLS config");

    Arc::new(config)
}

#[tokio::main]
async fn main() {
    auto_backup_if_needed();

    let conn = Connection::open("quailsync.db").expect("failed to open database");
    init_db(&conn);
    println!("[db] SQLite initialized (quailsync.db)");

    let alert_config = AlertConfig::default();
    println!(
        "[alerts] thresholds: temp {:.0}-{:.0}\u{00b0}F, humidity {:.0}-{:.0}%",
        alert_config.brooder_temp_min,
        alert_config.brooder_temp_max,
        alert_config.humidity_min,
        alert_config.humidity_max,
    );

    let (live_tx, _) = broadcast::channel::<String>(64);

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        agent_connected: Arc::new(AtomicBool::new(false)),
        alert_config,
        live_tx,
    };

    let app = build_app(state);

    // Generate TLS certs if needed
    ensure_tls_certs();
    let tls_config = load_tls_config();
    let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_config);

    // HTTP server on port 3000
    let http_listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    // HTTPS listener on port 3443
    let https_listener = tokio::net::TcpListener::bind("0.0.0.0:3443").await.unwrap();

    println!("HTTP:  http://0.0.0.0:3000");
    println!("HTTPS: https://0.0.0.0:3443");

    let http_app = app.clone();
    let https_app = app;

    // Spawn HTTP server
    let http_handle = tokio::spawn(async move {
        axum::serve(http_listener, http_app).await.unwrap();
    });

    // Spawn HTTPS server with manual TLS accept loop
    let https_handle = tokio::spawn(async move {
        loop {
            let (tcp_stream, _addr) = match https_listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    eprintln!("[tls] accept error: {e}");
                    continue;
                }
            };

            let acceptor = tls_acceptor.clone();
            let app = https_app.clone();

            tokio::spawn(async move {
                let tls_stream = match acceptor.accept(tcp_stream).await {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("[tls] handshake error: {e}");
                        return;
                    }
                };

                let io = hyper_util::rt::TokioIo::new(tls_stream);
                let hyper_service = hyper::service::service_fn(
                    move |req: hyper::Request<hyper::body::Incoming>| {
                        let mut app = app.clone();
                        async move {
                            use tower::Service;
                            let req = req.map(axum::body::Body::new);
                            app.call(req).await
                        }
                    },
                );

                if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection_with_upgrades(io, hyper_service)
                .await
                {
                    eprintln!("[tls] connection error: {e}");
                }
            });
        }
    });

    tokio::select! {
        r = http_handle => { if let Err(e) = r { eprintln!("HTTP server error: {e}"); } }
        r = https_handle => { if let Err(e) = r { eprintln!("HTTPS server error: {e}"); } }
    }
}
