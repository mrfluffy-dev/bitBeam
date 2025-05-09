use axum::{
    body::Bytes,
    extract::ConnectInfo,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::Utc;
use rand::Rng;
use sqlx::AnyPool;
use std::path::Path;
use tokio::fs;
use uuid::Uuid;
use log::{info, warn, error};

use std::net::SocketAddr;
use crate::data;


/// Handler to return all files as JSON
/// This function retrieves all files from the database
/// and returns them as a JSON response.
/// It also logs the IP address of the client making the request.
pub async fn all_files(Extension(pool): Extension<AnyPool>, ConnectInfo(addr): ConnectInfo<SocketAddr>) -> impl IntoResponse {
    //log the IP address of the client and the call
    let ip = addr.ip().to_string();
    info!("Received an all_files request from IP: {}", ip);
    // build the query and map the result to the File struct
    // and return the result as JSON if successful
    // or return an error message if not
    match sqlx::query_as::<_, data::File>(
        r#"
        SELECT *
        FROM files
        "#,
    )
    .fetch_all(&pool)
    .await
    {
        Ok(files) => {
            info!("DB select all success");
            (StatusCode::OK, Json(files)).into_response()
        },
        Err(e) => {
            warn!("DB select all error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database select all error",
            )
                .into_response()
        }
    }
}

/// Handler to upload a file
/// This function handles the file upload process.
/// It receives the file data in the request body,
/// saves it to the server's file system,
/// and stores the file metadata in the database.
/// It also logs the IP address of the client making the request.
pub async fn upload(Extension(pool): Extension<AnyPool>,
                ConnectInfo(addr): ConnectInfo<SocketAddr>,
                Extension(config): Extension<data::Config> ,
                headers: HeaderMap,
                body: Bytes,
                ) -> Response {
    //log the IP address of the client and the call
    let ip = addr.ip().to_string();
    info!("Received update from IP: {}", ip);

    // gets the content type from the headers
    let content_type = headers
        .get("content-type")
        .and_then(|hv| hv.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    // gets the download limit from the headers
    let download_limit = headers
        .get("download_limit") // Option<&HeaderValue>
        .and_then(|hv| hv.to_str().ok()) // Option<&str>
        .and_then(|s| s.parse::<i32>().ok()) // Option<u32>
        .unwrap_or(1); // u32
    //generate a random UUID for the file ID
    let id = {
        // Fallback to random UUID if body is too small
        let mut rng = rand::rng();
        Uuid::from_u128(rng.random::<u128>()).to_string()
    };
    //create the directory if it doesn't exist
    let dir = Path::new(&config.data_path);
    if let Err(e) = fs::create_dir_all(dir).await {
        warn!("could not make dir at {} error: {}", &config.data_path ,e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Directory creation error",
        )
            .into_response();
    }
    //create the file path
    // the file path is the directory + the file ID + file type if file type is not application/x-executable
    if content_type == "application/x-executable" {
        info!("File type is application/x-executable");
    } else {
        info!("File type is {}", content_type);
    }
    let file_path = dir.join(
        if content_type == "application/x-executable" {
            format!("{}",id)
        } else {
            format!("{}.{}",
        id,
        content_type.split('/').last().unwrap_or("bin"))
        },
    );

    if let Err(e) = fs::write(&file_path, &body).await {
        warn!("write error {}: {}", id, e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "File write error",
        )
            .into_response();
    }
    let file_size = body.len() as i64;

    let upload_time = Utc::now().timestamp(); // i64

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
        error!("DB insert error {}: {}", id, e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Database insert error",
        )
            .into_response();
    }

    let uploaded_file = data::File {
        id,
        content_type,
        upload_time,
        download_limit,
        download_count,
        file_size,
    };
    Json(uploaded_file).into_response()
}
