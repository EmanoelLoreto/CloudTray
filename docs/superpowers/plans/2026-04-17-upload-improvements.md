# Upload Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the broken in-memory upload implementation with Google Drive's Resumable Upload API, real streaming progress tracking, and a progress bar that shows speed and ETA — making the app usable for files of any size (tested goal: 500MB+).

**Architecture:** The drag-and-drop path switches from passing raw bytes through IPC (which explodes memory) to using Tauri's native `onFileDropEvent` to get native file paths. All uploads go through a single `upload_file_path` backend command that branches on file size: multipart for < 5MB, resumable chunked upload (8MB chunks) for ≥ 5MB. A new `UploadProgressEvent` struct carries bytes_sent, total_bytes, percent, and speed_bps so the frontend can display accurate progress.

**Tech Stack:** Rust/Tauri v1, reqwest 0.11, tokio 1 (adding `time` feature), React 18, TypeScript, `@tauri-apps/api/window` `onFileDropEvent`

---

## File Structure

| File | Changes |
|------|---------|
| `src-tauri/Cargo.toml` | Add `time` to tokio features |
| `src-tauri/src/drive.rs` | Add `UploadProgressEvent`; add `mime_type_for`, `set_public_permission`, `upload_multipart`, `initiate_resumable_session`, `upload_chunk_with_retry`, `upload_resumable`; update `upload_file_path`; fix `list_recent_files` double lookup; remove `upload_file` |
| `src-tauri/src/main.rs` | Remove `drive::upload_file` from `invoke_handler` |
| `src/App.tsx` | Replace `useDropzone` onDrop with Tauri `onFileDropEvent`; add `FileProgress` interface; add `formatSpeed`/`formatETA` helpers; extract shared `uploadFiles` function; fix `unlisten` leak; update progress JSX |
| `src/App.css` | Shimmer animation; progress bar height 6px; `max-height` 150px; `progress-info` row style |

---

## Task 1: Backend — UploadProgressEvent + fix list_recent_files double lookup

**Files:**
- Modify: `src-tauri/src/drive.rs`

- [ ] **Step 1: Add `UploadProgressEvent` struct and `mime_type_for` helper at the top of drive.rs, after the existing struct definitions (after line 34)**

```rust
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
```

- [ ] **Step 2: Fix `list_recent_files` — remove second call to `get_or_create_app_folder`**

Current lines 245–260 in `drive.rs`:
```rust
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

	let app_folder = get_or_create_app_folder(credentials).await?;  // ← REMOVE THIS
	
	let query = format!("'{}' in parents and trashed = false", app_folder.id);  // ← change to folder.id
```

Replace the full function with:
```rust
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
```

- [ ] **Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1
```

Expected: no errors (warnings ok)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/drive.rs
git commit -m "fix: add UploadProgressEvent, mime_type_for helper, fix list_recent_files double lookup"
```

---

## Task 2: Backend — Add tokio time feature + run delete_old_files concurrently

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/drive.rs`

- [ ] **Step 1: Add `time` to tokio features in Cargo.toml**

Find this line in `src-tauri/Cargo.toml`:
```toml
tokio = { version = "1.0", features = ["fs", "io-util"] }
```

Replace with:
```toml
tokio = { version = "1.0", features = ["fs", "io-util", "time"] }
```

- [ ] **Step 2: Extract `set_public_permission` helper function**

Add this function in `drive.rs` after `mime_type_for`:

```rust
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
```

- [ ] **Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1
```

Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/drive.rs
git commit -m "feat: add tokio time feature and set_public_permission helper"
```

---

## Task 3: Backend — Implement resumable upload

**Files:**
- Modify: `src-tauri/src/drive.rs`

- [ ] **Step 1: Add the `use tokio::io::AsyncReadExt;` import at the top of drive.rs**

At the top of the file after the existing `use` statements, add:
```rust
use tokio::io::AsyncReadExt;
```

- [ ] **Step 2: Add `initiate_resumable_session` function**

Add this private async function after `set_public_permission`:

```rust
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
```

- [ ] **Step 3: Add `upload_chunk_with_retry` function**

```rust
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
```

- [ ] **Step 4: Add `upload_resumable` function**

```rust
async fn upload_resumable(
    window: &tauri::Window,
    file_path: &str,
    file_name: &str,
    folder_id: &str,
    access_token: &str,
) -> Result<DriveFile, String> {
    let metadata = tokio::fs::metadata(file_path)
        .await
        .map_err(|e| format!("Failed to read file metadata: {}", e))?;
    let file_size = metadata.len();
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
```

- [ ] **Step 5: Add `upload_multipart` function (extracted from existing `upload_file` logic)**

```rust
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

    let response_text = response.text().await.map_err(|e| e.to_string())?;
    serde_json::from_str::<DriveFile>(&response_text)
        .map_err(|_| format!("Failed to parse upload response: {}", response_text))
}
```

- [ ] **Step 6: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1
```

Expected: no errors

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/drive.rs
git commit -m "feat: implement resumable upload with chunked progress and retry logic"
```

---

## Task 4: Backend — Wire upload_file_path + remove upload_file command

**Files:**
- Modify: `src-tauri/src/drive.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Replace `upload_file_path` in drive.rs**

Replace the entire existing `upload_file_path` function (lines 114–133) with:

```rust
#[command]
pub async fn upload_file_path(
    window: tauri::Window,
    file_path: String,
    folder_id: String,
    credentials: State<'_, GoogleCredentials>,
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
        upload_resumable(&window, &file_path, &file_name, &folder_id, &tokens.access_token).await?
    } else {
        let file_content = tokio::fs::read(&file_path)
            .await
            .map_err(|e| format!("Erro ao ler arquivo: {}", e))?;
        upload_multipart(&window, file_content, &file_name, &folder_id, &tokens.access_token).await?
    };

    set_public_permission(&file.id, &tokens.access_token).await?;
    Ok(file)
}
```

- [ ] **Step 2: Delete the old `upload_file` command from drive.rs**

Remove the entire `#[command] pub async fn upload_file(...)` function (lines 136–242 in the original file). It is replaced by `upload_multipart` + `upload_resumable` called from `upload_file_path`.

- [ ] **Step 3: Remove `drive::upload_file` from main.rs invoke_handler**

In `src-tauri/src/main.rs`, find:
```rust
        .invoke_handler(tauri::generate_handler![
            set_google_credentials,
            auth::start_oauth_server, 
            auth::exchange_auth_code, 
            auth::save_tokens,
            auth::get_tokens,
            auth::logout,
            drive::upload_file,          // ← remove this line
            drive::get_or_create_app_folder,
            drive::upload_file_path,
            drive::list_recent_files,
            drive::delete_file,
            config::load_or_create_config,
            config::save_config,
        ])
```

Remove the `drive::upload_file,` line.

- [ ] **Step 4: Build and verify**

```bash
cd src-tauri && cargo build 2>&1
```

Expected: successful build, no errors

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/drive.rs src-tauri/src/main.rs
git commit -m "feat: wire upload_file_path to resumable/multipart, remove upload_file command"
```

---

## Task 5: Frontend — Replace react-dropzone drop handler with Tauri native file drop

**Files:**
- Modify: `src/App.tsx`

This task replaces the drag-and-drop bytes-through-IPC approach with Tauri's `onFileDropEvent`, which gives native file paths directly — no ArrayBuffer, no memory issues.

- [ ] **Step 1: Add Tauri window import at the top of App.tsx**

After the existing Tauri imports (around line 7), add:
```typescript
import { appWindow } from '@tauri-apps/api/window';
```

- [ ] **Step 2: Replace `isDragActive` state + remove `useDropzone`**

Remove this block (approximately lines 110–114):
```typescript
const {
    getRootProps,
    getInputProps,
    isDragActive
} = useDropzone({ onDrop, multiple: true, noClick: true, noKeyboard: true });
```

Replace with:
```typescript
const [isDragActive, setIsDragActive] = useState(false);
```

- [ ] **Step 3: Register Tauri file drop listener in the main `useEffect`**

Inside the existing `useEffect` (after `checkAuth()` call, before the closing `}, []);`), add:

```typescript
    const unlistenFileDrop = appWindow.onFileDropEvent((event) => {
        if (event.payload.type === 'hover') {
            setIsDragActive(true);
        } else if (event.payload.type === 'drop') {
            setIsDragActive(false);
            if (isAuthenticated && event.payload.paths.length > 0) {
                uploadFiles(event.payload.paths);
            }
        } else {
            setIsDragActive(false);
        }
    });

    return () => {
        unlistenFileDrop.then(fn => fn());
    };
```

Note: the existing `useEffect` does not have a cleanup return — add it now.

- [ ] **Step 4: Update the drop area JSX — remove getRootProps/getInputProps**

Find:
```tsx
<div {...getRootProps()} className={`drop-area ${isDragActive ? 'drop-area-active' : ''}`}>
    <input {...getInputProps()} />
```

Replace with:
```tsx
<div className={`drop-area ${isDragActive ? 'drop-area-active' : ''}`}>
```

- [ ] **Step 5: Remove the old `onDrop` useCallback** (approximately lines 50–108)

Delete the entire `const onDrop = useCallback(async (acceptedFiles: File[]) => { ... }, []);` block. It is fully replaced by the `onFileDropEvent` listener and `uploadFiles` function (added in Task 6).

- [ ] **Step 6: Remove `useDropzone` and `useCallback` from the React import**

Find:
```typescript
import { useCallback, useEffect, useRef, useState } from "react";
import { useDropzone } from "react-dropzone";
```

Replace with:
```typescript
import { useEffect, useRef, useState } from "react";
```

- [ ] **Step 7: Verify the app builds (frontend)**

```bash
yarn build 2>&1 | tail -20
```

Expected: no TypeScript errors (may have warnings about unused `onDrop` if Task 6 not yet done — that's ok at this step, proceed to Task 6 immediately)

---

## Task 6: Frontend — Shared uploadFiles function + fix event listener leaks

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Add `FileProgress` interface and update `UploadProgress` type**

Replace:
```typescript
interface UploadProgress {
    [key: string]: number;
}
```

With:
```typescript
interface FileProgress {
    bytes_sent: number;
    total_bytes: number;
    percent: number;
    speed_bps: number;
    error?: boolean;
}

interface UploadProgress {
    [fileName: string]: FileProgress;
}
```

- [ ] **Step 2: Add `DriveFile` interface**

After `UploadProgress`, add:
```typescript
interface DriveFile {
    id: string;
    name: string;
    webViewLink: string;
}
```

- [ ] **Step 3: Add `formatSpeed` and `formatETA` helper functions**

Add these pure functions before the `App` component definition:
```typescript
function formatSpeed(bps: number): string {
    if (bps === 0) return '';
    if (bps >= 1024 * 1024) return `${(bps / (1024 * 1024)).toFixed(1)} MB/s`;
    if (bps >= 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
    return `${bps} B/s`;
}

function formatETA(bytesSent: number, totalBytes: number, speedBps: number): string {
    if (speedBps === 0 || bytesSent >= totalBytes) return '';
    const remaining = (totalBytes - bytesSent) / speedBps;
    if (remaining >= 60) return `~${Math.ceil(remaining / 60)}min`;
    return `~${Math.ceil(remaining)}s`;
}
```

- [ ] **Step 4: Add `uploadFiles` function inside the `App` component**

Add this function inside `App`, before `handleFileSelect`:

```typescript
const uploadFiles = async (filePaths: string[]) => {
    let unlisten: (() => void) | undefined;
    try {
        const appFolder = await invoke<{ id: string; name: string }>("get_or_create_app_folder");

        const initialProgress: UploadProgress = {};
        filePaths.forEach(fp => {
            const name = fp.split(/[/\\]/).pop() || fp;
            initialProgress[name] = { bytes_sent: 0, total_bytes: 0, percent: 0, speed_bps: 0 };
        });
        setUploadProgress(initialProgress);

        unlisten = await listen<FileProgress & { file_name: string }>("upload-progress", (event) => {
            const { file_name, bytes_sent, total_bytes, percent, speed_bps } = event.payload;
            setUploadProgress(prev => ({
                ...prev,
                [file_name]: { bytes_sent, total_bytes, percent, speed_bps },
            }));
        });

        const isSingleFile = filePaths.length === 1;
        let lastResult: DriveFile | undefined;

        for (const filePath of filePaths) {
            try {
                const result = await invoke<DriveFile>("upload_file_path", {
                    filePath,
                    folderId: appFolder.id,
                });
                if (isSingleFile) lastResult = result;
            } catch {
                const name = filePath.split(/[/\\]/).pop() || filePath;
                setUploadProgress(prev => ({
                    ...prev,
                    [name]: { ...prev[name], error: true },
                }));
            }
        }

        if (isSingleFile && lastResult) {
            setUploadFeedback({ type: 'success', message: t('app.uploadSuccess') });
            await navigator.clipboard.writeText(lastResult.webViewLink);
            setCopiedId(lastResult.id);
            setTimeout(() => setCopiedId(null), 5000);
        } else {
            setUploadFeedback({ type: 'success', message: t('app.uploadsSuccess') });
        }

        setTimeout(() => {
            setUploadProgress({});
            setUploadFeedback(null);
        }, 5000);
    } catch {
        setUploadProgress({});
        setUploadFeedback({ type: 'error', message: t('app.uploadError') });
        setTimeout(() => setUploadFeedback(null), 5000);
    } finally {
        unlisten?.();
    }
};
```

- [ ] **Step 5: Replace `handleFileSelect` to use `uploadFiles`**

Replace the entire existing `handleFileSelect` function with:

```typescript
const handleFileSelect = async () => {
    try {
        const selected = await openDialog({
            multiple: true,
            filters: [{
                name: 'All Files',
                extensions: [
                    'png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'bmp', 'tiff', 'ico', 'raw', 'heic',
                    'mp4', 'avi', 'mov', 'wmv', 'flv', 'mkv', 'webm', 'm4v', '3gp',
                    'mp3', 'wav', 'ogg', 'aac', 'wma', 'm4a', 'flac',
                    'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'txt', 'rtf', 'csv',
                    'zip', 'rar', '7z', 'tar', 'gz',
                    'json', 'yaml', 'yml', 'toml', 'ini', 'conf', 'cfg', 'config', 'sql',
                ]
            }]
        });

        if (selected) {
            const paths = Array.isArray(selected) ? selected : [selected];
            await uploadFiles(paths);
        }
    } catch {
        setUploadFeedback({ type: 'error', message: t('app.uploadError') });
        setTimeout(() => setUploadFeedback(null), 5000);
    }
};
```

- [ ] **Step 6: Verify TypeScript build**

```bash
yarn build 2>&1 | tail -30
```

Expected: successful build with no TypeScript errors

- [ ] **Step 7: Commit**

```bash
git add src/App.tsx
git commit -m "feat: shared uploadFiles function, fix event listener leaks, Tauri native file drop"
```

---

## Task 7: Frontend — Progress bar UI (CSS + JSX)

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/App.css`

- [ ] **Step 1: Update the progress JSX in App.tsx**

Find the upload progress rendering block (the `Object.entries(uploadProgress).map(...)` section):

```tsx
{Object.entries(uploadProgress).map(([fileName, progress]) => (
    <div key={fileName} className="upload-progress">
        <span className="filename">
            {fileName.length > 13 ? fileName.slice(0, 13) + '...' : fileName}
        </span>
        <div className="progress-bar">
            <div 
                className={`progress-fill ${progress < 100 ? 'animating' : 'success'}`}
                style={{ width: `${progress}%` }}
            />
        </div>
    </div>
))}
```

Replace with:

```tsx
{Object.entries(uploadProgress).map(([fileName, progress]) => (
    <div key={fileName} className="upload-progress">
        <span className="filename">
            {fileName.length > 24 ? fileName.slice(0, 24) + '...' : fileName}
        </span>
        <div className="progress-bar">
            <div
                className={`progress-fill ${
                    progress.error ? 'error' :
                    progress.percent >= 100 ? 'success' :
                    progress.percent === 0 ? 'indeterminate' : ''
                }`}
                style={{ width: progress.percent === 0 ? '100%' : `${progress.percent}%` }}
            />
        </div>
        {progress.total_bytes > 0 && progress.percent < 100 && !progress.error && (
            <div className="progress-info">
                <span>{progress.percent}%</span>
                <span>{formatSpeed(progress.speed_bps)}</span>
                <span>{formatETA(progress.bytes_sent, progress.total_bytes, progress.speed_bps)}</span>
            </div>
        )}
    </div>
))}
```

- [ ] **Step 2: Update App.css — progress bar styles**

Replace the entire block from `.container-upload-files` through the end of the `@keyframes rotate` block with:

```css
.container-upload-files {
    overflow-y: auto;
    max-height: 150px;
    width: 100%;
}

.filename {
    font-size: 13px;
    margin-bottom: 3px;
    display: block;
    color: #9f9f9f;
    text-align: left;
}

.progress-bar {
    width: 100%;
    height: 6px;
    background-color: rgba(255, 255, 255, 0.08);
    border-radius: 3px;
    overflow: hidden;
}

.progress-fill {
    height: 100%;
    background-color: #007AFF;
    border-radius: 3px;
    transition: width 0.4s ease;
}

.progress-fill.success {
    background-color: #21f336;
    transition: width 0.2s ease;
}

.progress-fill.error {
    background-color: #e74c3c;
}

.progress-fill.indeterminate {
    width: 100% !important;
    background: linear-gradient(
        90deg,
        rgba(0, 122, 255, 0.10) 25%,
        rgba(0, 122, 255, 0.45) 50%,
        rgba(0, 122, 255, 0.10) 75%
    );
    background-size: 200% 100%;
    animation: shimmer 1.5s ease-in-out infinite;
}

.progress-info {
    display: flex;
    justify-content: space-between;
    margin-top: 2px;
    font-size: 11px;
    color: #6e6e6e;
}

@keyframes shimmer {
    0%   { background-position: -200% center; }
    100% { background-position:  200% center; }
}
```

- [ ] **Step 3: Verify full build**

```bash
yarn build 2>&1 | tail -20
```

Expected: successful build

- [ ] **Step 4: Run the app and test with a small file (< 5MB) to verify multipart path**

```bash
yarn tauri dev
```

- Drop a small file (e.g., a PNG). Expected: shimmer animation briefly, then blue fill reaching 100%, then green. Progress info row shows `100%`.

- [ ] **Step 5: Test with a large file (> 5MB) to verify resumable path**

- Drop or select a file > 5MB. Expected: progress bar fills smoothly as each 8MB chunk completes, speed (e.g. `12.3 MB/s`) and ETA (e.g. `~38s`) visible below the bar.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx src/App.css
git commit -m "feat: progress bar with speed and ETA display, shimmer indeterminate state"
```

---

## Self-Review

**Spec coverage check:**

| Spec requirement | Covered in |
|-----------------|------------|
| Resumable upload for ≥ 5MB | Task 3, Task 4 |
| 8MB chunk size | Task 3 (`CHUNK_SIZE`) |
| Retry up to 3 times per chunk | Task 3 (`upload_chunk_with_retry`) |
| Speed calculation (rolling avg 3 chunks) | Task 3 (`upload_resumable`) |
| `UploadProgressEvent` struct with bytes_sent/total_bytes/percent/speed_bps | Task 1 |
| Fix drag-and-drop memory bomb | Task 5 (Tauri `onFileDropEvent`) |
| Fix `list_recent_files` double folder lookup | Task 1 |
| `delete_old_files` runs concurrently with token fetch | Task 4 (`tokio::join!`) |
| `unlisten()` in `finally` | Task 6 |
| Shared `uploadFiles` function | Task 6 |
| Filename truncated at 24 chars | Task 7 |
| `max-height: 150px` | Task 7 |
| Shimmer animation | Task 7 |
| Progress info row (%, speed, ETA) | Task 7 |
| `set_public_permission` extracted | Task 2 |
| `mime_type_for` helper expanded | Task 1 |
| Remove `upload_file` command | Task 4 |

**Token caching** (spec mentions caching token validity for 60s): marked as out-of-scope in this plan — the `get_tokens` already does a live check, and the `tokio::join!` in Task 4 ensures it doesn't block the upload. Full caching would require a `Mutex<Option<(GoogleTokens, Instant)>>` state in `main.rs` and is a separate task.

**Placeholder scan:** none found.

**Type consistency:** `FileProgress` interface defined in Task 6 Step 1, used in the progress map in Task 7 Step 1 — consistent. `UploadProgressEvent` struct defined in Task 1 Step 1, used in Tasks 3 and 4 — consistent. `DriveFile` TypeScript interface defined in Task 6 Step 2, used in `uploadFiles` return typing — consistent.
