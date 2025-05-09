use axum::{
    extract::DefaultBodyLimit,
    //response::IntoResponse,
    routing::{get, post},
    Extension, Router,
};
use log::{debug, error, info, warn};
use sqlx::{any::AnyPoolOptions, migrate::MigrateDatabase, AnyPool, Sqlite};

use std::path::Path;
use tokio::fs;

use std::net::SocketAddr;
mod api;
mod data;

/// This is the main function of the application.
/// It sets up the database connection,
/// initializes the logging system,
/// and starts the web server.
/// It uses the Axum framework to handle HTTP requests.
/// It also uses SQLx for database interactions.
/// It uses the Fern library for logging.
/// It uses the Tokio runtime for asynchronous programming.
/// It uses the Chrono library for date and time handling.
/// It uses the UUID library for generating unique identifiers.
/// It uses the Bytes library for handling byte arrays.
/// It uses the Serde library for serialization and deserialization.
#[tokio::main]
async fn main() {
    sqlx::any::install_default_drivers();
    // Load the configuration from environment variables
    let config = data::Config {
        db_type: std::env::var("BITBEAM_DB_TYPE").unwrap_or_else(|_| "sqlite".to_string()),
        // Determine the correct database URL
        database_url: match std::env::var("BITBEAM_DB_TYPE")
            .unwrap_or_else(|_| "sqlite".to_string())
            .as_str()
        {
            "postgres" => {
                // For Postgres, BITBEAM_DATABASE_URL must be set
                std::env::var("BITBEAM_DATABASE_URL")
                    .expect("BITBEAM_DATABASE_URL must be set for Postgres")
            }

            "sqlite" => {
                // For SQLite, use BITBEAM_DATABASE_URL if set, otherwise default
                std::env::var("BITBEAM_DATABASE_URL")
                    .unwrap_or_else(|_| "sqlite://./bitbeam.sqlite".to_string())
            }

            other => {
                panic!("Unsupported BITBEAM_DB_TYPE: {}", other);
            }
        },
        data_path: std::env::var("BITBEAM_DATA_PATH")
            .unwrap_or_else(|_| "./media_store".to_string()),
        port: std::env::var("BITBEAM_PORT").unwrap_or_else(|_| "3000".to_string()),
        listener_addr: std::env::var("BITBEAM_ADDR").unwrap_or_else(|_| "127.0.0.1".to_string()),
        log_level: std::env::var("BITBEAM_LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
        log_location: std::env::var("BITBEAM_LOG_LOCATION")
            .unwrap_or_else(|_| "./bitbeam.log".to_string()),
        use_tls: std::env::var("BITBEAM_USE_TLS")
            .unwrap_or_else(|_| "false".to_string())
            .parse()
            .unwrap_or(false),
        base_url: std::env::var("BITBEAM_BASE_URL").unwrap_or_else(|_| {
            format!(
                "localhost:{}",
                std::env::var("BITBEAM_PORT").unwrap_or_else(|_| "3000".to_string())
            )
            .to_string()
        }),
        allow_register: std::env::var("BITBEAM_ALLOW_REGISTER")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true),
    };
    // Setting up the logging system
    // The log level is set based on the environment variable BITBEAM_LOG_LEVEL
    let level = match config.log_level.as_str() {
        "debug" => log::LevelFilter::Debug,
        "info" => log::LevelFilter::Info,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        _ => log::LevelFilter::Info,
    };
    // Initialize the logging system
    let log_path = &config.log_location;
    let _logs = init_logging(log_path, level);
    info!("done loading config");

    // Create the data path if it doesn't exist
    // only if the db type is sqlite
    // otherwise, the data path is not used
    if config.db_type == "sqlite" {
        if !Sqlite::database_exists(&config.database_url)
            .await
            .unwrap_or(false)
        {
            println!("Creating database {}", config.database_url);
            match Sqlite::create_database(&config.database_url).await {
                Ok(_) => info!("Create db success"),
                Err(error) => {
                    error!("Error creating database: {}", error);
                    panic!("error: {}", error)
                }
            }
        } else {
            info!("Database already exists");
        }
    }

    // Create the database connection any pool
    // The connection pool is created using the database URL from the configuration
    let pool: AnyPool = AnyPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await
        .expect("could not connect to database");

    // Setting up the database schema
    // The database schema is created if it doesn't exist
    if let Err(_e) = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS files (
            id TEXT PRIMARY KEY,
            file_name TEXT NOT NULL,
            content_type TEXT NOT NULL,
            upload_time BIGINT NOT NULL,
            download_limit INTEGER NOT NULL,
            download_count INTEGER NOT NULL,
            file_size BIGINT NOT NULL,
            download_url TEXT NOT NULL,
            owner TEXT NOT NULL
        );
    "#,
    )
    .execute(&pool)
    .await
    {
        info!("DB created");
    };
    // create the user table
    if let Err(_e) = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            key TEXT PRIMARY KEY,
            username TEXT NOT NULL,
            password TEXT NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await
    {
        info!("DB created");
    };
    //create the directory if it doesn't exist
    let dir = Path::new(&config.data_path);
    if let Err(e) = fs::create_dir_all(dir).await {
        warn!("could not make dir at {} error: {}", &config.data_path, e);
    }
    //let file_path = dir.join(&id);

    // Setting up the web server
    // The web server is created using the Axum framework
    // these are the routes
    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/upload", post(api::upload))
        .route("/all_files", get(api::all_files))
        .route("/download/{uuid}", get(api::download_file))
        .route("/user/register", post(api::register_user))
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(Extension(pool))
        .layer(Extension(config.clone()))
        .into_make_service_with_connect_info::<SocketAddr>();

    // The web server is started using the Axum framework
    // The server listens on the address and port specified in the configuration
    axum::serve(
        match tokio::net::TcpListener::bind(format!("{}:{}", &config.listener_addr, &config.port))
            .await
        {
            Ok(listener) => listener,
            Err(e) => {
                error!(
                    "Error binding to address {}:{} : {}",
                    &config.listener_addr, &config.port, e
                );
                return;
            }
        },
        app,
    )
    .await
    .unwrap();
}

/// This function initializes the logging system.
/// It sets up a logger that writes to both stdout and a log file.
/// It uses the Fern library for logging.
/// It formats the log messages to include the date, time, log level, target, and message.
/// It also sets the log level based on the provided level filter.
/// It takes the log file path and log level as parameters.
fn init_logging(
    log_file_path: &str,
    level: log::LevelFilter,
) -> Result<(), Box<dyn std::error::Error>> {
    // Build a Dispatch for stdout
    let stdout_dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{date}][{lvl}][{target}] {msg}",
                date = chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                lvl = record.level(),
                target = record.target(),
                msg = message,
            ))
        })
        .level(level)
        .chain(std::io::stdout());

    // Build a Dispatch for a rolling log file
    let file_dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{date}][{lvl}][{target}] {msg}",
                date = chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                lvl = record.level(),
                target = record.target(),
                msg = message,
            ))
        })
        .level(level)
        .chain(fern::log_file(log_file_path)?);

    // Combine the stdout and file dispatches
    // and apply them
    // This sets up the logger to write to both stdout and the log file
    fern::Dispatch::new()
        .chain(stdout_dispatch)
        .chain(file_dispatch)
        .apply()?;

    Ok(())
}
