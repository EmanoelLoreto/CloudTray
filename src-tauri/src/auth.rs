use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};

use tauri_plugin_oauth::start;

use tauri::command;

use crate::drive::GoogleTokens;

use crate::GoogleCredentials;
use tauri::State;

#[command]
pub async fn start_oauth_server(window: tauri::Window) -> Result<u16, String> {
		start(move |url| {
				let _ = window.emit("oauth_callback", url);
		})
		.map_err(|err| err.to_string())
}

#[command]
pub async fn exchange_auth_code(
	code: String, 
	redirect_uri: String,
	credentials: State<'_, GoogleCredentials>
) -> Result<GoogleTokens, String> {
		let client = reqwest::Client::new();

    let client_id = credentials.client_id.lock().unwrap().clone();
    let client_secret = credentials.client_secret.lock().unwrap().clone();
		
		let response = client
				.post("https://oauth2.googleapis.com/token")
				.form(&[
						("client_id", &client_id),
						("client_secret", &client_secret),
						("code", &code),
						("redirect_uri", &redirect_uri),
						("grant_type", &String::from("authorization_code")),
				])
				.send()
				.await
				.map_err(|e| format!("Erro na requisição: {}", e))?;

		let response_text = response.text().await
				.map_err(|e| format!("Erro ao ler resposta: {}", e))?;
		
		serde_json::from_str(&response_text)
				.map_err(|e| format!("Erro ao parsear JSON: {}", e))
}

#[command]
pub async fn get_tokens(credentials: State<'_, GoogleCredentials>) -> Result<GoogleTokens, String> {
	let config_dir = tauri::api::path::app_config_dir(&tauri::Config::default())
			.ok_or("Não foi possível encontrar o diretório de configuração")?;
	let config_path = config_dir.join("config");
	let tokens_path = config_path.join("google_tokens.json");
	
	if !config_path.exists() {
			tokio::fs::create_dir_all(&config_path)
					.await
					.map_err(|e| format!("Erro ao criar diretório de configuração: {}", e))?;
	}
	
	if !tokens_path.exists() {
			return Err("Tokens file not found. Please authenticate first.".to_string());
	}
	
	match tokio::fs::read_to_string(&tokens_path).await {
			Ok(tokens_str) => {
					let tokens: GoogleTokens = serde_json::from_str(&tokens_str)
							.map_err(|e| format!("Erro ao parsear tokens JSON: {}", e))?;
					
					let client = reqwest::Client::new();
					let mut headers = HeaderMap::new();
					headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", tokens.access_token)).unwrap());
					
					let test_response = client
							.get("https://www.googleapis.com/drive/v3/files")
							.headers(headers)
							.send()
							.await;

					match test_response {
							Ok(response) if response.status().is_success() => Ok(tokens),
							_ => {
									refresh_access_token(tokens.refresh_token, credentials).await
							}
					}
			},
			Err(e) => {
					Err(format!("Error reading tokens file: {}", e))
			}
	}
}

#[command]
pub async fn save_tokens(tokens: GoogleTokens) -> Result<(), String> {
		let config_dir = tauri::api::path::app_config_dir(&tauri::Config::default())
				.ok_or("Não foi possível encontrar o diretório de configuração")?;
		let config_path = config_dir.join("config");
		let tokens_path = config_path.join("google_tokens.json");
		
		tokio::fs::create_dir_all(&config_path)
				.await
				.map_err(|e| format!("Erro ao criar diretório: {}", e))?;
		
		let tokens_json = serde_json::to_string_pretty(&tokens)
				.map_err(|e| format!("Erro ao serializar tokens: {}", e))?;
		
		tokio::fs::write(&tokens_path, tokens_json)
				.await
				.map_err(|e| format!("Erro ao salvar arquivo: {}", e))?;
		
		Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct RefreshTokenResponse {
		access_token: String,
		expires_in: i32,
		scope: String,
		token_type: String,
}

async fn refresh_access_token(
	refresh_token: String,
	credentials: State<'_, GoogleCredentials>
) -> Result<GoogleTokens, String> {
		let client = reqwest::Client::new();

    let client_id = credentials.client_id.lock().unwrap().clone();
    let client_secret = credentials.client_secret.lock().unwrap().clone();
		
		let response = client
				.post("https://oauth2.googleapis.com/token")
				.form(&[
						("client_id", &client_id),
						("client_secret", &client_secret),
						("refresh_token", &refresh_token),
						("grant_type", &String::from("refresh_token")),
				])
				.send()
				.await
				.map_err(|e| format!("Erro na requisição: {}", e))?;

		let response_text = response.text().await
				.map_err(|e| format!("Erro ao ler resposta: {}", e))?;
		
		let refresh_response: RefreshTokenResponse = serde_json::from_str(&response_text)
				.map_err(|e| format!("Erro ao parsear JSON: {}", e))?;
		
		let new_tokens = GoogleTokens {
				access_token: refresh_response.access_token,
				refresh_token,
				expires_in: refresh_response.expires_in,
				token_type: refresh_response.token_type,
		};
		
		save_tokens(new_tokens.clone()).await?;
		
		Ok(new_tokens)
}

#[command]
pub async fn logout() -> Result<(), String> {
		let config_dir = tauri::api::path::app_config_dir(&tauri::Config::default())
				.ok_or("Não foi possível encontrar o diretório de configuração")?;
		let tokens_path = config_dir.join("config").join("google_tokens.json");
		
		if tokens_path.exists() {
				tokio::fs::remove_file(&tokens_path)
						.await
						.map_err(|e| format!("Erro ao remover arquivo de tokens: {}", e))?;
		}
		
		Ok(())
}