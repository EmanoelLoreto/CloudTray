use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use crate::config::load_or_create_config;
use crate::auth::get_tokens;

use tauri::command;
use tauri::State;
use crate::GoogleCredentials;

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
) -> Result<DriveFile, String> {
	let _ = delete_old_files(&folder_id, credentials.clone()).await;

	let file_content = tokio::fs::read(&file_path)
		.await
		.map_err(|e| format!("Erro ao ler arquivo: {}", e))?;
	
	let file_name = std::path::Path::new(&file_path)
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or("Nome do arquivo inválido")?
		.to_string();

	upload_file(window, file_content, file_name, folder_id, credentials).await
}

#[command]
pub async fn upload_file(
	window: tauri::Window,
	file_content: Vec<u8>,
	file_name: String,
	folder_id: String,
	credentials: State<'_, GoogleCredentials>,
) -> Result<DriveFile, String> {
	let _ = delete_old_files(&folder_id, credentials.clone()).await;

	let tokens = get_tokens(credentials).await?;
	let client = reqwest::Client::new();

	let metadata = serde_json::json!({
		"name": file_name,
		"parents": [folder_id],
	});

	let mut headers = HeaderMap::new();
	headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", tokens.access_token)).unwrap());

	let mime_type = if file_name.to_lowercase().ends_with(".png") {
		"image/png"
	} else if file_name.to_lowercase().ends_with(".jpg") || file_name.to_lowercase().ends_with(".jpeg") {
		"image/jpeg"
	} else {
		"application/octet-stream"
	};

	let boundary = "foo_bar_baz";
	let metadata_part = format!(
		"--{}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{}\r\n",
		boundary,
		serde_json::to_string(&metadata).unwrap()
	);
	let file_part = format!(
		"--{}\r\nContent-Type: {}\r\n\r\n",
		boundary,
		mime_type
	);
	let end_boundary = format!("\r\n--{}--", boundary);

	let mut body = Vec::new();
	body.extend_from_slice(metadata_part.as_bytes());
	let _ = window.emit("upload-progress", (file_name.clone(), 10));

	body.extend_from_slice(file_part.as_bytes());
	let _ = window.emit("upload-progress", (file_name.clone(), 20));

	let chunk_size = file_content.len() / 60;
	if chunk_size > 0 {
		for (i, chunk) in file_content.chunks(chunk_size).enumerate() {
			body.extend_from_slice(chunk);
			let progress = 20 + ((i as f64 / (file_content.len() as f64 / chunk_size as f64)) * 70.0) as u32;
			let _ = window.emit("upload-progress", (file_name.clone(), progress));
		}
	} else {
		body.extend_from_slice(&file_content);
		let _ = window.emit("upload-progress", (file_name.clone(), 90));
	}

	body.extend_from_slice(end_boundary.as_bytes());

	headers.insert(
		CONTENT_TYPE,
		HeaderValue::from_str(&format!("multipart/related; boundary={}", boundary)).unwrap()
	);

	let response = client
		.post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id,name,webViewLink")
		.headers(headers.clone())
		.body(body)
		.send()
		.await
		.map_err(|e| {
			e.to_string()
		})?;

	let _ = window.emit("upload-progress", (file_name.clone(), 100));

	let response_text = response.text().await.map_err(|e| {
		e.to_string()
	})?;

	if let Ok(file) = serde_json::from_str::<DriveFile>(&response_text) {
		let permission_body = serde_json::json!({
			"role": "reader",
			"type": "anyone"
		});

		let permission_response = client
			.post(&format!("https://www.googleapis.com/drive/v3/files/{}/permissions", file.id))
			.headers(headers)
			.json(&permission_body)
			.send()
			.await
			.map_err(|e| format!("Erro ao definir permissões: {}", e))?;

		if !permission_response.status().is_success() {
			return Err("Falha ao definir permissões do arquivo".to_string());
		}

		Ok(file)
	} else {
		Err("Erro ao fazer parse do arquivo".to_string())
	}
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
		HeaderValue::from_str(&format!("Bearer {}", tokens.access_token)).unwrap()
	);

	let app_folder = get_or_create_app_folder(credentials).await?;
	
	let query = format!("'{}' in parents and trashed = false", app_folder.id);
	
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