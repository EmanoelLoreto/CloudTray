# Upload Improvements Design

**Date:** 2026-04-17  
**Status:** Approved  
**Scope:** Large file upload reliability, real progress tracking, progress bar UI

---

## Problem Summary

The current upload implementation is critically broken for files ≥ 50MB:

1. **Drag-and-drop memory bomb** (`App.tsx:75`): `Array.from(new Uint8Array(await file.arrayBuffer()))` converts a 500MB file into a JS array of 500M numbers at ~8 bytes each, consuming ~4GB of RAM before IPC serialization even begins.
2. **Backend loads entire file into memory** (`drive.rs:125`): `tokio::fs::read` allocates the full file at once.
3. **Fake progress**: progress events measure local memory copy speed, not bytes sent over the network. The bar fills in milliseconds then freezes while waiting for the HTTP response.
4. **No retry**: a single network hiccup during a 500MB upload fails the entire transfer.
5. **Event listener leaks**: `unlisten()` is not called in error paths.
6. **Duplicated upload logic**: `onDrop` and `handleFileSelect` are near-identical (~80 lines each).
7. **`list_recent_files` double folder lookup**: `get_or_create_app_folder` is called twice per invocation.
8. **Progress bar animation**: `rotateX` is a 3D flip with no visible indeterminate effect.
9. **Filename truncated at 13 chars**: too short to be useful.
10. **`container-upload-files` max-height: 50px**: hides all but one file in multi-upload.

---

## Architecture

### Upload path unification

Both drag-and-drop and file dialog converge on the same backend command:

```
Drag-and-drop file(s)
  → frontend calls save_temp_file(bytes) → returns temp path
  → calls upload_file_path(temp_path, folder_id)
  → backend deletes temp file after upload (success or failure)

File dialog selection
  → calls upload_file_path(file_path, folder_id)   [unchanged]
```

The existing `upload_file(file_content: Vec<u8>, ...)` command is removed. All uploads go through `upload_file_path`.

### Upload strategy by file size

| File size | Strategy |
|-----------|----------|
| < 5MB | Multipart upload (`uploadType=multipart`) — current approach, kept as-is |
| ≥ 5MB | Resumable upload (`uploadType=resumable`) — new implementation |

The 5MB threshold matches Google's own recommendation.

---

## Backend: Resumable Upload Flow (`drive.rs`)

### Step 1 — Initiate session

```
POST https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable
Authorization: Bearer <token>
Content-Type: application/json
X-Upload-Content-Type: <mime_type>
X-Upload-Content-Length: <file_size>

Body: { "name": "...", "parents": ["<folder_id>"] }
```

Response header `Location` contains the session URI. Store it for the duration of the upload.

### Step 2 — Upload chunks

Chunk size: **8MB** (8 × 1024 × 1024 bytes). Last chunk may be smaller.

```
PUT <session_uri>
Content-Range: bytes <start>-<end>/<total>
Content-Length: <chunk_size>

Body: <chunk bytes read from file>
```

After each successful chunk:
- Emit `upload-progress` event (see Event Payload below)
- Advance byte offset
- Continue to next chunk

### Step 3 — Retry logic

On chunk failure (network error, 5xx, timeout):
1. Query session status: `PUT <session_uri>` with `Content-Range: bytes */<total>`
2. Parse `Range` response header to find last confirmed byte
3. Retry from confirmed offset
4. Maximum **3 retries per chunk**, then return error to frontend

### Step 4 — Completion

The final chunk's 200/201 response body contains the `DriveFile` JSON.  
Set `reader/anyone` permission as today.  
Delete temp file (if drag-and-drop path).

### Speed calculation

Track `chunk_start_time` before each PUT. After response:
```rust
let elapsed_secs = chunk_start_time.elapsed().as_secs_f64();
let speed_bps = (chunk_size as f64 / elapsed_secs) as u64;
```

Smooth with a simple rolling average over the last 3 chunks to avoid jitter.

### Event payload

Replace the current `(String, u32)` tuple with a struct:

```rust
#[derive(Serialize, Clone)]
pub struct UploadProgressEvent {
    pub file_name: String,
    pub bytes_sent: u64,
    pub total_bytes: u64,
    pub percent: u32,
    pub speed_bps: u64,
}
```

Emitted as `"upload-progress"` event after each chunk.

---

## Frontend Changes (`App.tsx`)

### New command: `save_temp_file`

```typescript
// Called in onDrop before upload
const tempPath = await invoke<string>("save_temp_file", {
  fileName: file.name,
  fileContent: Array.from(new Uint8Array(await file.arrayBuffer()))
})
```

**Note:** `save_temp_file` still receives bytes via IPC, which is acceptable for files where drag-and-drop is used. For very large files (> ~100MB), users should use the file dialog path to avoid the ArrayBuffer entirely. The `save_temp_file` IPC call avoids the main bottleneck (the ArrayBuffer-to-number-array conversion is still present but the backend handles the rest efficiently). A future improvement could use Tauri's file drop event to get the native path directly, eliminating IPC bytes entirely.

### Shared upload function

Extract duplicated logic into a single function:

```typescript
async function uploadFiles(filePaths: string[], appFolderId: string): Promise<UploadResult>
```

Both `onDrop` and `handleFileSelect` call this function. Handles:
- Initial progress state
- Event listener setup/teardown (in `finally`)
- Per-file result accumulation
- Feedback state

### TypeScript event type

```typescript
interface UploadProgressEvent {
  file_name: string
  bytes_sent: number
  total_bytes: number
  percent: number
  speed_bps: number
}
```

### Error handling

- `unlisten()` moved to `finally` block — no more event listener leaks
- Per-file error tracking in batch uploads: one file failing does not abort others
- Upload feedback shows which file failed when in batch mode

---

## Frontend Changes (`App.css` + progress bar UI)

### Progress bar component

Each file in the upload list shows:

```
video_final_cut.mp4
[████████████░░░░░░░░░░░░] 48%   11.2 MB/s   ~42s restantes
```

Layout:
- Filename row: truncated at **24 chars** (up from 13)
- Progress bar: full width, height 6px (up from 4px)
- Info row below bar: `<percent>%` left-aligned, speed center, ETA right-aligned
- Font size 11px for info row, color `#9f9f9f`

### Progress bar states

| State | Visual |
|-------|--------|
| Queued (0%) | Shimmer animation (indeterminate) |
| Uploading (1–99%) | Blue fill (#007AFF), smooth transition |
| Complete (100%) | Green fill (#21f336), no animation |
| Error | Red fill (#e74c3c) |

### Shimmer animation (replaces rotateX)

```css
@keyframes shimmer {
  0%   { background-position: -200% center; }
  100% { background-position:  200% center; }
}

.progress-fill.indeterminate {
  width: 100%;
  background: linear-gradient(
    90deg,
    rgba(0,122,255,0.15) 25%,
    rgba(0,122,255,0.4)  50%,
    rgba(0,122,255,0.15) 75%
  );
  background-size: 200% 100%;
  animation: shimmer 1.5s ease-in-out infinite;
}
```

### Container height

`max-height: 50px` → `max-height: 150px` to show up to ~3 files with info rows.

---

## Bug Fixes (not covered above)

| Bug | Fix |
|-----|-----|
| `list_recent_files` calls `get_or_create_app_folder` twice | Remove second call on line 259, reuse result from line 246 |
| `delete_old_files` blocks upload start | Run concurrently with `tokio::join!` or fire-and-forget |
| `get_tokens` makes a live API call on every auth check | Cache token validity for 60s in memory using a `Mutex<Option<(GoogleTokens, Instant)>>` |

---

## Out of Scope

- Pause/resume UI controls (backend supports resume internally, but no user-facing button)
- Upload queue UI (uploading multiple files still sequential, not parallel)
- Notification on upload complete when app window is hidden
