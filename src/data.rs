use serde::Serialize;
use sqlx::FromRow;

/// This struct represents a file in the database.
/// It contains fields for the file's ID, content type,
/// upload time, download limit, download count,
/// and file size.
/// It derives the `FromRow` trait from `sqlx`
/// to allow it to be created from a database row.
/// It also derives the `Serialize` trait
/// from `serde`
/// to allow it to be serialized into JSON.
#[derive(FromRow, Serialize)]
pub struct File {
    pub id: String,
    pub content_type: String,
    pub upload_time: i64,
    pub download_limit: i32,
    pub download_count: i32,
    pub file_size: i64,
}

/// This struct is used to represent the configuration settings for the application.
/// It contains various fields that are used to configure the database connection,
/// data path, server port, and logging settings.
/// It derives the `Clone` trait
/// to allow it to be cloned.
#[derive(Clone)]
pub struct Config {
    pub db_type: String,
    pub database_url: String,
    pub data_path: String,
    pub port: String,
    pub listener_addr: String,
    pub log_level: String,
    pub log_location: String,
}
