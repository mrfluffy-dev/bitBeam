use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};

use chrono::Utc;
use log::{error, info, warn};
use rand::Rng;
use sqlx::AnyPool;
use std::path::Path as PathBuf;
use tokio::fs;
use uuid::Uuid;

use crate::data;
use std::net::SocketAddr;
use serde_json::json;

/// Handler to return all files as JSON
/// This function retrieves all files from the database
/// and returns them as a JSON response.
/// It also logs the IP address of the client making the request.
/// example request: curl -X GET http://localhost:3000/all_files
/// requires no parameters
/// returns a JSON array of files
/// TODO: add user authentication
pub async fn all_files(
    Extension(pool): Extension<AnyPool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
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
        }
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
/// example request: curl -X POST -H "key: <key>" -H "file_name: <file_name>" -H "content-type: <content_type>" -H "download_limit: <download_limit>" --data-binary @<file_path> http://localhost:3000/upload
/// requires the following headers:
/// - key: the key of the user (not optional)
/// - file_name: the name of the file (optional)
/// - content-type: the content type of the file (optional)
/// - download_limit: the download limit of the file (optional)
pub async fn upload(
    Extension(pool): Extension<AnyPool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Extension(config): Extension<data::Config>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    //log the IP address of the client and the call
    let ip = addr.ip().to_string();
    info!("Received update from IP: {}", ip);


        //get the key from the headers
    let key = match headers.get("key") {
        Some(hv) => hv.to_str().unwrap_or("unknown").to_string(),
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "Key header not supplied",
            )
                .into_response();
        }
    };

    //check if the user exists
    let owner = sqlx::query_as::<_, data::user>(
        r#"
        SELECT *
        FROM users
        WHERE key = ?
        "#,
    )
    .bind(&key)
    .fetch_one(&pool)
    .await;
    let owner = match owner {
        Ok(user) => {
            info!("User found in DB: {}", key);
            user.username
        }
        Err(e) => {
            error!("DB select error {}: {} Most likely because the Key is not valid", key, e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Your key is not valid",
            )
                .into_response();
        }
    };

    // gets the content type from the headers
    let content_type = headers
        .get("content-type")
        .and_then(|hv| hv.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    // gets the download limit from the headers
    let download_limit = headers
        .get("download_limit") // Option<&HeaderValue>
        .and_then(|hv| hv.to_str().ok()) // Option<&str>
        .and_then(|s| s.parse::<i32>().ok()) // Option<u32>
        .unwrap_or(1); // u32
    //get filename from the headers
    let file_name = headers
        .get("file_name")
        .and_then(|hv| hv.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    //generate a random UUID for the file ID
    let id = {
        // Fallback to random UUID if body is too small
        let mut rng = rand::rng();
        Uuid::from_u128(rng.random::<u128>()).to_string()
    };
    //create the directory if it doesn't exist
    let dir = PathBuf::new(&config.data_path);
    if let Err(e) = fs::create_dir_all(dir).await {
        warn!("could not make dir at {} error: {}", &config.data_path, e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Directory creation error",
        )
            .into_response();
    }
    //create the file path
    // the file path is the directory + the file ID + file type if file type is not application/x-executable
    info!("File type is {}", content_type);
    let file_path = dir.join(&id);

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

    let download_url = match config.use_tls {
        true => format!("https://{}/download/{}", config.base_url, id),
        false => format!("http://{}/download/{}", config.base_url, id),
    };


    if let Err(e) = sqlx::query(
        r#"
        INSERT INTO files
            (id, content_type, upload_time, download_limit, download_count, file_size, download_url, file_name, owner)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&content_type)
    .bind(&upload_time)
    .bind(download_limit)
    .bind(download_count)
    .bind(file_size as i64)
    .bind(&download_url)
    .bind(&file_name)
    .bind(&owner)
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
        file_name,
        content_type,
        upload_time,
        download_limit,
        download_count,
        file_size,
        download_url,
        owner,
    };
    Json(uploaded_file).into_response()
}

/// This is The file Download handler
/// This function handles the file download process.
/// It retrieves the file metadata from the database
/// and returns the file as a response.
/// It also logs the IP address of the client making the request.
/// example request: curl -X GET http://localhost:3000/download/<uuid>
/// requires the following path parameter:
/// - uuid: the UUID of the file (not optional)
pub async fn download_file(
    Path(uuid): Path<String>, // Add this extractor
    Extension(pool): Extension<AnyPool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Extension(config): Extension<data::Config>,
    // Remove body: Bytes,         // <-- GET handler shouldn't have a body
) -> Response {

    // Get UUID directly from path
    info!("Download request for UUID: {}", uuid);
    // Log the IP address of the client and the call
    let ip = addr.ip().to_string();
    info!("Received download request for {} from IP: {}", uuid, ip);

    // find file by uuid in the config.data_path
    let file_path = PathBuf::new(&config.data_path).join(&uuid);

    if !file_path.exists() {
        error!("File not found: {}", file_path.display());
        return (
            axum::http::StatusCode::NOT_FOUND,
            "File not found",
        )
            .into_response();
    }
    // Check if the file exists in the database
    let file = sqlx::query_as::<_, data::File>(
        r#"
        SELECT *
        FROM files
        WHERE id = ?
        "#,
    )
    .bind(&uuid)
    .fetch_one(&pool)
    .await;
    let file = match file {
        Ok(file) => {
            info!("File found in DB: {}", uuid);
            file
        }
        Err(e) => {
            error!("DB select error {}: {}", uuid, e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Database select error",
            )
                .into_response();
        }
    };

    //update download count
    if let Err(e) = sqlx::query(
        r#"
        UPDATE files
        SET download_count = download_count + 1
        WHERE id = ?
        "#,
    )
    .bind(&uuid)
    .execute(&pool)

    .await
    {
        error!("DB update error {}: {}", uuid, e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Database update error",
        )
            .into_response();
    }
    info!("Update Download Count Sucess for UUID: {}", uuid);

    //rutn file to axum::body::Bytes
    let file_bytes = match fs::read(&file_path).await {
        Ok(file) => file,
        Err(e) => {
            error!("File read error {}: {}", uuid, e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "File read error",
            )
                .into_response();
        }
    };

    //if download count is greater or equal to download limit delete the file and remove it from the database
    if (file.download_count) >= file.download_limit {
        if let Err(e) = fs::remove_file(&file_path).await {
            error!("File delete error {}: {}", uuid, e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "File delete error",
            )
                .into_response();
        }
        if let Err(e) = sqlx::query(
            r#"
            DELETE FROM files
            WHERE id = ?
            "#,
        )
        .bind(&uuid)
        .execute(&pool)
        .await
        {
            error!("DB delete error {}: {}", uuid, e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Database delete error",
            )
                .into_response();
        }
        info!("File deleted from DB because max download limit was reached: {}", uuid);
    }

    // return the file as a response
    return (
        axum::http::StatusCode::OK,
        axum::response::IntoResponse::into_response(
            axum::response::Response::builder()
                .header("Content-Disposition", format!("attachment; filename=\"{}\"", uuid))
                .header("Content-Type", format!("{}", &file.content_type ))
                .header("Content-Length", file.file_size)
                .header("filename", file.file_name)
                .body(axum::body::Body::from(file_bytes))
                .unwrap(),
        ),
    )
        .into_response()
}

/// Handler to upload a file
/// This function registers a new user.
/// It receives the user data in the request headers,
/// saves it to the database,
/// and returns the user data as a JSON response.
/// It also logs the IP address of the client making the request.
///  example request: curl -X POST -H "username: <username>" -H "password: <password>" http://localhost:3000/register
///  requires the following headers:
///  - username: the username of the user (not optional)
///  - password: the password of the user (not optional)
pub async fn register_user (
    Extension(pool): Extension<AnyPool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Extension(config): Extension<data::Config>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    //log the IP address of the client and the call
    let ip = addr.ip().to_string();
    info!("Received update from IP: {}", ip);

    //check if registration is allowed
    if !config.allow_register {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "Registration is not allowed",
        )
            .into_response();
    }

    // gets the content type from the headers return error if header is not suplyde
    let username = match headers .get("username") {
        Some(hv) => hv.to_str().unwrap_or("unknown").to_string(),
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "Username header not supplied",
            )
                .into_response();
        }
    };
    let password = match headers .get("password") {
        Some(hv) => hv.to_str().unwrap_or("unknown").to_string(),
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "Password header not supplied",
            )
                .into_response();
        }
    };

    //generate a random UUID for the user key
    let key = {
        // Fallback to random UUID if body is too small
        let mut rng = rand::rng();
        Uuid::from_u128(rng.random::<u128>()).to_string()
    };

    // check if the user already exists
    let user = sqlx::query_as::<_, data::user>(
        r#"
        SELECT *
        FROM users
        WHERE username = ?
        "#,
    )
    .bind(&username)
    .fetch_one(&pool)
    .await;
    match user {
        Ok(_) => {
            info!("User already exists: {}", username);
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "User already exists",
            )
                .into_response();
        }
        Err(e) => {
            warn!("DB select error {}: {}", username, e);
        }
    }

    //add the user to the database
    if let Err(e) = sqlx::query(
        r#"
        INSERT INTO users
            (key, username, password)
        VALUES (?, ?, ?)
        "#,
    )
    .bind(&key)
    .bind(&username)
    .bind(&password)
    .execute(&pool)
    .await
    {
        error!("DB insert error {}: {}", key, e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Database insert error",
        )
            .into_response();
    }
    info!("User registered: {}", username);

    //return the user as a response
    let registered_user = json!({
        "key": key,
        "username": username,
    });
    Json(registered_user)
        .into_response()
}
