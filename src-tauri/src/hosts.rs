use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub id: String,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// "password" | "key" | "agent"
    pub auth_type: String,
    #[serde(default)]
    pub key_path: Option<String>,
}

fn hosts_file(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("hosts.json"))
}

fn read_all(app: &AppHandle) -> Result<Vec<Host>, String> {
    let path = hosts_file(app)?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("hosts.json rusak: {}", e))
}

/// Tulis lewat berkas sementara lalu rename: kalau proses mati di tengah
/// penyimpanan, hosts.json lama tetap utuh alih-alih tertulis separuh.
fn write_all(app: &AppHandle, hosts: &[Host]) -> Result<(), String> {
    let path = hosts_file(app)?;
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(hosts).map_err(|e| e.to_string())?;
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn hosts_list(app: AppHandle) -> Result<Vec<Host>, String> {
    read_all(&app)
}

/// Simpan host baru (id kosong) atau perbarui host lama (id sama).
#[tauri::command]
pub fn hosts_save(app: AppHandle, mut host: Host) -> Result<Vec<Host>, String> {
    let mut hosts = read_all(&app)?;
    if host.id.is_empty() {
        host.id = uuid::Uuid::new_v4().to_string();
        hosts.push(host);
    } else if let Some(existing) = hosts.iter_mut().find(|h| h.id == host.id) {
        *existing = host;
    } else {
        hosts.push(host);
    }
    write_all(&app, &hosts)?;
    Ok(hosts)
}

#[tauri::command]
pub fn hosts_delete(app: AppHandle, id: String) -> Result<Vec<Host>, String> {
    let mut hosts = read_all(&app)?;
    hosts.retain(|h| h.id != id);
    write_all(&app, &hosts)?;
    Ok(hosts)
}
