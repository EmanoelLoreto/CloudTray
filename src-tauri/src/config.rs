use serde::{Deserialize, Serialize};
use tauri::command;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
	pub retention_hours: i64,
}

impl Default for AppConfig {
	fn default() -> Self {
		Self {
			retention_hours: 24,
		}
	}
}

#[command]
pub async fn load_or_create_config() -> Result<AppConfig, String> {
  let config_dir = tauri::api::path::app_config_dir(&tauri::Config::default())
	  .ok_or("Não foi possível encontrar o diretório de configuração")?;
  let config_path = config_dir.join("config");
  let config_file = config_path.join("app_config.json");
  
  if !config_path.exists() {
	  tokio::fs::create_dir_all(&config_path)
		  .await
		  .map_err(|e| format!("Erro ao criar diretório de configuração: {}", e))?;
  }
  
  if config_file.exists() {
	  let config_str = tokio::fs::read_to_string(&config_file)
		  .await
		  .map_err(|e| format!("Erro ao ler arquivo de configuração: {}", e))?;
		  
	  serde_json::from_str(&config_str)
		  .map_err(|e| format!("Erro ao parsear configuração: {}", e))
  } else {
	  let default_config = AppConfig::default();
	  let config_json = serde_json::to_string_pretty(&default_config)
		  .map_err(|e| format!("Erro ao serializar configuração: {}", e))?;
		  
	  tokio::fs::write(&config_file, config_json)
		  .await
		  .map_err(|e| format!("Erro ao salvar configuração: {}", e))?;
		  
	  Ok(default_config)
  }
}

#[command]
pub async fn save_config(config: AppConfig) -> Result<(), String> {
  let config_dir = tauri::api::path::app_config_dir(&tauri::Config::default())
	  .ok_or("Não foi possível encontrar o diretório de configuração")?;
  let config_path = config_dir.join("config");
  let config_file = config_path.join("app_config.json");
  
  let config_json = serde_json::to_string_pretty(&config)
	  .map_err(|e| format!("Erro ao serializar configuração: {}", e))?;
	  
  tokio::fs::write(&config_file, config_json)
	  .await
	  .map_err(|e| format!("Erro ao salvar configuração: {}", e))
}
