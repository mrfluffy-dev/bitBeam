use axum::{
    body::Bytes,
    extract::DefaultBodyLimit,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::Serialize;
use sqlx::{any::AnyPoolOptions, migrate::MigrateDatabase, AnyPool, Encode, FromRow, Sqlite};
use std::path::Path;
use tokio::fs;
use uuid::Uuid;

#[derive(FromRow, Serialize)]
struct File {
    id: String,
    content_type: String,
    upload_time: i64,
    download_limit: i32,
    download_count: i32,
    file_size: i64,
}

struct Config {
    db_type: String,
    database_url: String,
}

#[tokio::main]
async fn main() {
    sqlx::any::install_default_drivers();
    // Read and normalize DB type and connection URL
    let config = Config {
        db_type: std::env::var("BITBEEM_DB_TYPE").unwrap_or_else(|_| "postgres".to_string()),
        database_url: match std::env::var("BITBEEM_DB_TYPE").unwrap().as_str() {
            "postgres" => std::env::var("BITBEEM_DATABASE_URL")
                .expect("BITBEEM_DATABASE_URL must be set for Postgres"),
            "sqlite" => std::env::var("BITBEEM_DATABASE_URL")
                .expect("BITBEEM_DATABASE_URL must be set for SQLite"),
            other => panic!("Unsupported BITBEEM_DB_TYPE: {}", other),
        },
    };

    if config.db_type == "sqlite" {
        if !Sqlite::database_exists(&config.database_url)
            .await
            .unwrap_or(false)
        {
            println!("Creating database {}", config.database_url);
            match Sqlite::create_database(&config.database_url).await {
                Ok(_) => println!("Create db success"),
                Err(error) => panic!("error: {}", error),
            }
        } else {
            println!("Database already exists");
        }
    }

    // Create a generic AnyPool
    let pool: AnyPool = AnyPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await
        .expect("could not connect to database");

    // Migration SQL
    if let Err(_e) = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS files (
            id TEXT PRIMARY KEY,
            content_type TEXT NOT NULL,
            upload_time BIGINT NOT NULL,
            download_limit INTEGER NOT NULL,
            download_count INTEGER NOT NULL,
            file_size BIGINT NOT NULL
        );
    "#,
    )
    .execute(&pool)
    .await
    {
        eprintln!("DB created");
    };

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/upload", post(upload))
        .route("/all_files", get(all_files))
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(Extension(pool));

    axum::serve(
        tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap(),
        app,
    )
    .await
    .unwrap();
}

/// Handler to return all files as JSON
async fn all_files(Extension(pool): Extension<AnyPool>) -> impl IntoResponse {
    // Run the query and map each row into a File
    match sqlx::query_as::<_, File>(
        r#"
        SELECT *
        FROM files
        "#,
    )
    .fetch_all(&pool)
    .await
    {
        Ok(files) => (StatusCode::OK, Json(files)).into_response(),
        Err(e) => {
            eprintln!("DB select all error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database select all error",
            )
                .into_response()
        }
    }
}

async fn upload(Extension(pool): Extension<AnyPool>, headers: HeaderMap, body: Bytes) -> Response {
    let content_type = headers
        .get("content-type")
        .and_then(|hv| hv.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let id = {
        // Fallback to random UUID if body is too small
        let mut rng = rand::rng();
        Uuid::from_u128(rng.random::<u128>()).to_string()
    };
    let dir = Path::new("./media_store");
    if let Err(e) = fs::create_dir_all(dir).await {
        eprintln!("mkdir error: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Directory creation error",
        )
            .into_response();
    }

    let file_path = dir.join(&id);
    if let Err(e) = fs::write(&file_path, &body).await {
        eprintln!("write error {}: {}", id, e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "File write error",
        )
            .into_response();
    }
    let file_size = body.len() as i64;

    let upload_time = Utc::now().timestamp(); // i64
    let download_limit = headers
        .get("download_limit") // Option<&HeaderValue>
        .and_then(|hv| hv.to_str().ok()) // Option<&str>
        .and_then(|s| s.parse::<i32>().ok()) // Option<u32>
        .unwrap_or(2); // u32    let download_count = 0;
    let download_count = 0;

    if let Err(e) = sqlx::query(
        r#"
        INSERT INTO files
            (id, content_type, upload_time, download_limit, download_count, file_size)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&content_type)
    .bind(&upload_time)
    .bind(download_limit)
    .bind(download_count)
    .bind(file_size as i64)
    .execute(&pool)
    .await
    {
        eprintln!("DB insert error {}: {}", id, e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Database insert error",
        )
            .into_response();
    }

    let uploaded_file = File {
        id,
        content_type,
        upload_time,
        download_limit,
        download_count,
        file_size,
    };
    Json(uploaded_file).into_response()
}
