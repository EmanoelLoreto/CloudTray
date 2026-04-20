use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use crate::config::load_or_create_config;
use crate::auth::get_tokens;

use tauri::command;
use tauri::State;
use crate::GoogleCredentials;
use crate::CancellationSet;

const APP_FOLDER_NAME: &str = "CloudTray";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GoogleTokens {
	pub access_token: String,
	pub refresh_token: String,
	pub expires_in: i32,
	pub token_type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DriveFolder {
	pub id: String,
	pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DriveFile {
	pub id: String,
	pub name: String,
	#[serde(rename = "webViewLink")]
	pub web_view_link: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct UploadProgressEvent {
    pub file_name: String,
    pub bytes_sent: u64,
    pub total_bytes: u64,
    pub percent: u32,
    pub speed_bps: u64,
}

fn mime_type_for(file_name: &str) -> &'static str {
    let lower = file_name.to_lowercase();
    if lower.ends_with(".png") { "image/png" }
    else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") { "image/jpeg" }
    else if lower.ends_with(".gif") { "image/gif" }
    else if lower.ends_with(".webp") { "image/webp" }
    else if lower.ends_with(".svg") { "image/svg+xml" }
    else if lower.ends_with(".mp4") { "video/mp4" }
    else if lower.ends_with(".mov") { "video/quicktime" }
    else if lower.ends_with(".avi") { "video/x-msvideo" }
    else if lower.ends_with(".mkv") { "video/x-matroska" }
    else if lower.ends_with(".mp3") { "audio/mpeg" }
    else if lower.ends_with(".pdf") { "application/pdf" }
    else { "application/octet-stream" }
}

async fn set_public_permission(file_id: &str, access_token: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", access_token)).unwrap(),
    );
    let permission_body = serde_json::json!({ "role": "reader", "type": "anyone" });

    let response = client
        .post(&format!(
            "https://www.googleapis.com/drive/v3/files/{}/permissions",
            file_id
        ))
        .headers(headers)
        .json(&permission_body)
        .send()
        .await
        .map_err(|e| format!("Erro ao definir permissões: {}", e))?;

    if !response.status().is_success() {
        return Err("Falha ao definir permissões do arquivo".to_string());
    }
    Ok(())
}

async fn initiate_resumable_session(
    client: &reqwest::Client,
    access_token: &str,
    file_name: &str,
    folder_id: &str,
    file_size: u64,
    mime_type: &str,
) -> Result<String, String> {
    let metadata = serde_json::json!({
        "name": file_name,
        "parents": [folder_id],
    });

    let response = client
        .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&fields=id,name,webViewLink")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .header("X-Upload-Content-Type", mime_type)
        .header("X-Upload-Content-Length", file_size.to_string())
        .json(&metadata)
        .send()
        .await
        .map_err(|e| format!("Failed to initiate resumable session: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Resumable session initiation failed: {} - {}",
            status, body
        ));
    }

    response
        .headers()
        .get("Location")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| "No Location header in resumable upload response".to_string())
}

async fn upload_chunk_with_retry(
    client: &reqwest::Client,
    session_uri: &str,
    chunk: &[u8],
    start: u64,
    total: u64,
    max_retries: u32,
) -> Result<reqwest::Response, String> {
    let end = start + chunk.len() as u64 - 1;
    let content_range = format!("bytes {}-{}/{}", start, end, total);

    for attempt in 0..=max_retries {
        let result = client
            .put(session_uri)
            .header("Content-Range", &content_range)
            .header("Content-Length", chunk.len().to_string())
            .body(chunk.to_vec())
            .send()
            .await;

        match result {
            Ok(response) => {
                let status = response.status().as_u16();
                if status == 200 || status == 201 || status == 308 {
                    return Ok(response);
                }
                if attempt == max_retries {
                    let body = response.text().await.unwrap_or_default();
                    return Err(format!(
                        "Chunk upload failed with status {} after {} retries: {}",
                        status, max_retries, body
                    ));
                }
                // Query session status before retry so Google can confirm bytes received
                let _ = client
                    .put(session_uri)
                    .header("Content-Range", format!("bytes */{}", total))
                    .header("Content-Length", "0")
                    .send()
                    .await;
            }
            Err(e) => {
                if attempt == max_retries {
                    return Err(format!(
                        "Chunk upload network error after {} retries: {}",
                        max_retries, e
                    ));
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2u64.pow(attempt))).await;
    }

    unreachable!()
}

async fn upload_resumable(
    window: &tauri::Window,
    file_path: &str,
    file_name: &str,
    folder_id: &str,
    access_token: &str,
    cancellation: &CancellationSet,
) -> Result<DriveFile, String> {
    let metadata = tokio::fs::metadata(file_path)
        .await
        .map_err(|e| format!("Failed to read file metadata: {}", e))?;
    let file_size = metadata.len();

    if file_size == 0 {
        return Err("Cannot upload empty file".to_string());
    }

    let mime_type = mime_type_for(file_name);

    let client = reqwest::Client::new();
    let session_uri =
        initiate_resumable_session(&client, access_token, file_name, folder_id, file_size, mime_type)
            .await?;

    let mut file = tokio::fs::File::open(file_path)
        .await
        .map_err(|e| format!("Failed to open file: {}", e))?;

    const CHUNK_SIZE: u64 = 8 * 1024 * 1024; // 8MB
    let mut bytes_sent: u64 = 0;
    let mut speed_history: Vec<f64> = Vec::new();

    let _ = window.emit(
        "upload-progress",
        UploadProgressEvent {
            file_name: file_name.to_string(),
            bytes_sent: 0,
            total_bytes: file_size,
            percent: 0,
            speed_bps: 0,
        },
    );

    loop {
        if cancellation.cancelled.lock().unwrap().remove(file_name) {
            return Err("cancelled".to_string());
        }

        let chunk_size = std::cmp::min(CHUNK_SIZE, file_size - bytes_sent) as usize;
        let mut chunk = vec![0u8; chunk_size];
        file.read_exact(&mut chunk)
            .await
            .map_err(|e| format!("Failed to read chunk at offset {}: {}", bytes_sent, e))?;

        let chunk_start = std::time::Instant::now();

        let response =
            upload_chunk_with_retry(&client, &session_uri, &chunk, bytes_sent, file_size, 3)
                .await?;

        let elapsed = chunk_start.elapsed().as_secs_f64().max(0.001);
        let speed = chunk_size as f64 / elapsed;
        speed_history.push(speed);
        if speed_history.len() > 3 {
            speed_history.remove(0);
        }
        let avg_speed = speed_history.iter().sum::<f64>() / speed_history.len() as f64;

        bytes_sent += chunk_size as u64;
        let percent = ((bytes_sent as f64 / file_size as f64) * 100.0) as u32;

        let _ = window.emit(
            "upload-progress",
            UploadProgressEvent {
                file_name: file_name.to_string(),
                bytes_sent,
                total_bytes: file_size,
                percent,
                speed_bps: avg_speed as u64,
            },
        );

        let status = response.status().as_u16();
        if status == 200 || status == 201 {
            let drive_file: DriveFile = response
                .json()
                .await
                .map_err(|e| format!("Failed to parse upload response: {}", e))?;
            return Ok(drive_file);
        }
        // 308 Resume Incomplete — continue to next chunk
        if bytes_sent >= file_size {
            return Err("Upload finished reading file but server did not return 200/201".to_string());
        }
    }
}

async fn upload_multipart(
    window: &tauri::Window,
    file_content: Vec<u8>,
    file_name: &str,
    folder_id: &str,
    access_token: &str,
) -> Result<DriveFile, String> {
    let client = reqwest::Client::new();
    let mime_type = mime_type_for(file_name);

    let metadata = serde_json::json!({
        "name": file_name,
        "parents": [folder_id],
    });

    let boundary = "cloudtray_boundary_abc123";
    let metadata_part = format!(
        "--{}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{}\r\n",
        boundary,
        serde_json::to_string(&metadata).unwrap()
    );
    let file_part = format!(
        "--{}\r\nContent-Type: {}\r\n\r\n",
        boundary, mime_type
    );
    let end_boundary = format!("\r\n--{}--", boundary);

    let _ = window.emit(
        "upload-progress",
        UploadProgressEvent {
            file_name: file_name.to_string(),
            bytes_sent: 0,
            total_bytes: file_content.len() as u64,
            percent: 10,
            speed_bps: 0,
        },
    );

    let mut body = Vec::new();
    body.extend_from_slice(metadata_part.as_bytes());
    body.extend_from_slice(file_part.as_bytes());
    body.extend_from_slice(&file_content);
    body.extend_from_slice(end_boundary.as_bytes());

    let _ = window.emit(
        "upload-progress",
        UploadProgressEvent {
            file_name: file_name.to_string(),
            bytes_sent: file_content.len() as u64 / 2,
            total_bytes: file_content.len() as u64,
            percent: 50,
            speed_bps: 0,
        },
    );

    let response = client
        .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id,name,webViewLink")
        .header("Authorization", format!("Bearer {}", access_token))
        .header(
            "Content-Type",
            format!("multipart/related; boundary={}", boundary),
        )
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let _ = window.emit(
        "upload-progress",
        UploadProgressEvent {
            file_name: file_name.to_string(),
            bytes_sent: file_content.len() as u64,
            total_bytes: file_content.len() as u64,
            percent: 100,
            speed_bps: 0,
        },
    );

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Upload failed with status {}: {}", status, body));
    }

    let response_text = response.text().await.map_err(|e| e.to_string())?;
    serde_json::from_str::<DriveFile>(&response_text)
        .map_err(|_| format!("Failed to parse upload response: {}", response_text))
}

#[command]
pub async fn get_or_create_app_folder(credentials: State<'_, GoogleCredentials>) -> Result<DriveFolder, String> {
	let tokens = get_tokens(credentials).await?;
	let client = reqwest::Client::new();
	
	let mut headers = HeaderMap::new();
	headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", tokens.access_token)).unwrap());
	
	let query = format!("name = '{}' and mimeType = 'application/vnd.google-apps.folder' and trashed = false", APP_FOLDER_NAME);
	
	let response = client
		.get("https://www.googleapis.com/drive/v3/files")
		.headers(headers.clone())
		.query(&[
			("q", &query),
			("fields", &"files(id, name)".to_string()),
		])
		.send()
		.await
		.map_err(|e| {
			e.to_string()
		})?;

	let response_text = response.text().await.map_err(|e| e.to_string())?;

	#[derive(Debug, Deserialize)]
	struct FileList {
		files: Vec<DriveFolder>,
	}

	let file_list: FileList = serde_json::from_str(&response_text)
		.map_err(|e| format!("Erro ao parsear lista de arquivos: {}. Resposta: {}", e, response_text))?;

	if let Some(folder) = file_list.files.first() {
		return Ok(DriveFolder {
			id: folder.id.clone(),
			name: folder.name.clone(),
		});
	}

	headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

	let folder_metadata = serde_json::json!({
		"name": APP_FOLDER_NAME,
		"mimeType": "application/vnd.google-apps.folder"
	});

	let create_response = client
		.post("https://www.googleapis.com/drive/v3/files")
		.headers(headers.clone())
		.json(&folder_metadata)
		.send()
		.await
		.map_err(|e| {
			e.to_string()
		})?;

	let create_text = create_response.text().await.map_err(|e| e.to_string())?;

	let folder: DriveFolder = serde_json::from_str(&create_text)
		.map_err(|e| format!("Erro ao parsear pasta criada: {}. Resposta: {}", e, create_text))?;

	let permission_body = serde_json::json!({
		"role": "reader",
		"type": "anyone"
	});

	let _permission_response = client
		.post(&format!("https://www.googleapis.com/drive/v3/files/{}/permissions", folder.id))
		.headers(headers)
		.json(&permission_body)
		.send()
		.await
		.map_err(|e| format!("Erro ao definir permissões: {}", e))?;

	Ok(folder)
}

#[command]
pub async fn upload_file_path(
    window: tauri::Window,
    file_path: String,
    folder_id: String,
    credentials: State<'_, GoogleCredentials>,
    cancellation: State<'_, CancellationSet>,
) -> Result<DriveFile, String> {
    let tokens_fut = get_tokens(credentials.clone());
    let delete_fut = delete_old_files(&folder_id, credentials.clone());
    let (tokens_result, _) = tokio::join!(tokens_fut, delete_fut);
    let tokens = tokens_result?;

    let file_name = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Nome do arquivo inválido")?
        .to_string();

    let metadata = tokio::fs::metadata(&file_path)
        .await
        .map_err(|e| format!("Failed to read file metadata: {}", e))?;
    let file_size = metadata.len();

    const RESUMABLE_THRESHOLD: u64 = 5 * 1024 * 1024; // 5MB

    let file = if file_size >= RESUMABLE_THRESHOLD {
        upload_resumable(&window, &file_path, &file_name, &folder_id, &tokens.access_token, &cancellation).await?
    } else {
        let file_content = tokio::fs::read(&file_path)
            .await
            .map_err(|e| format!("Erro ao ler arquivo: {}", e))?;
        upload_multipart(&window, file_content, &file_name, &folder_id, &tokens.access_token).await?
    };

    set_public_permission(&file.id, &tokens.access_token).await?;
    Ok(file)
}

#[command]
pub async fn cancel_upload(file_name: String, cancellation: State<'_, CancellationSet>) -> Result<(), String> {
    cancellation.cancelled.lock().unwrap().insert(file_name);
    Ok(())
}

#[command]
pub async fn cleanup_old_files(credentials: State<'_, GoogleCredentials>) -> Result<(), String> {
    let folder = get_or_create_app_folder(credentials.clone()).await?;
    delete_old_files(&folder.id, credentials).await
}

#[command]
pub async fn list_recent_files(credentials: State<'_, GoogleCredentials>) -> Result<Vec<DriveFile>, String> {
	let folder = get_or_create_app_folder(credentials.clone()).await?;

	let _ = delete_old_files(&folder.id, credentials.clone()).await;

	let tokens = get_tokens(credentials.clone()).await?;
	let client = reqwest::Client::new();

	let mut headers = HeaderMap::new();
	headers.insert(
		AUTHORIZATION,
		HeaderValue::from_str(&format!("Bearer {}", tokens.access_token)).unwrap(),
	);

	let query = format!("'{}' in parents and trashed = false", folder.id);
	
	let response = client
		.get("https://www.googleapis.com/drive/v3/files")
		.headers(headers)
		.query(&[
			("q", &query),
			("orderBy", &"modifiedTime desc".to_string()),
			("fields", &"files(id,name,webViewLink)".to_string()),
			("pageSize", &"50".to_string()),
		])
		.send()
		.await
		.map_err(|e| e.to_string())?;

	#[derive(Debug, Deserialize)]
	struct FileList {
		files: Vec<DriveFile>,
	}

	let response_text = response.text().await.map_err(|e| e.to_string())?;
	let file_list: FileList = serde_json::from_str(&response_text)
		.map_err(|e| format!("Error parsing file list: {}", e))?;

	Ok(file_list.files)
}

#[derive(Debug, Deserialize)]
struct FileIdOnly {
	id: String,
}

async fn delete_old_files(folder_id: &str, credentials: State<'_, GoogleCredentials>) -> Result<(), String> {
	let config = load_or_create_config().await?;
	let hours_threshold = config.retention_hours;
	let tokens = get_tokens(credentials).await?;
	let client = reqwest::Client::new();
	
	let mut headers = HeaderMap::new();
	headers.insert(
		AUTHORIZATION, 
		HeaderValue::from_str(&format!("Bearer {}", tokens.access_token)).unwrap()
	);

	let threshold_date = Utc::now() - chrono::Duration::hours(hours_threshold);
	let query = format!(
		"'{}' in parents and trashed = false and modifiedTime < '{}'",
		folder_id,
		threshold_date.format("%Y-%m-%dT%H:%M:%S.%3fZ")
	);

	let response = client
		.get("https://www.googleapis.com/drive/v3/files")
		.headers(headers.clone())
		.query(&[
			("q", &query),
			("fields", &"files(id)".to_string()),
		])
		.send()
		.await
		.map_err(|e| e.to_string())?;

	#[derive(Debug, Deserialize)]
	struct FileList {
		files: Vec<FileIdOnly>,
	}

	let file_list: FileList = response.json().await
		.map_err(|e| format!("Error parsing file list: {}", e))?;

	for file in file_list.files {
		let _ = client
			.delete(&format!("https://www.googleapis.com/drive/v3/files/{}", file.id))
			.headers(headers.clone())
			.send()
			.await
			.map_err(|e| format!("Error deleting file {}: {}", file.id, e))?;
	}

	Ok(())
}

#[command]
pub async fn delete_file(file_id: String, credentials: State<'_, GoogleCredentials>) -> Result<(), String> {
	let tokens = get_tokens(credentials).await?;
	let client = reqwest::Client::new();
	
	let mut headers = HeaderMap::new();
	headers.insert(
		AUTHORIZATION, 
		HeaderValue::from_str(&format!("Bearer {}", tokens.access_token)).unwrap()
	);

	client
		.delete(&format!("https://www.googleapis.com/drive/v3/files/{}", file_id))
		.headers(headers)
		.send()
		.await
		.map_err(|e| format!("Error deleting file: {}", e))?;

	Ok(())
}