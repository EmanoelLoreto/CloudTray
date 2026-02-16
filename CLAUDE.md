# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

CloudTray is a desktop system tray application built with Tauri (Rust + React) that enables quick file uploads to Google Drive with automatic link sharing. The app integrates OAuth2 authentication and manages file uploads through the Google Drive API.

## Architecture

### Frontend (React + TypeScript)
- **src/App.tsx**: Main application component handling OAuth flow, file upload UI (drag-and-drop and file dialog), upload progress tracking, and navigation between tabs
- **src/tabs/**: Contains Settings, Recents, and About tab components
- **src/i18n.ts**: i18next configuration for internationalization (English and Portuguese)
- Uses react-dropzone for drag-and-drop file uploads
- Uses react-hotkeys-hook for keyboard shortcuts
- Communicates with Rust backend via Tauri's `invoke()` API

### Backend (Rust)
- **src-tauri/src/main.rs**:
  - Tauri app initialization and system tray configuration
  - Registers all Tauri commands (handlers for frontend invocations)
  - Platform-specific setup (vibrancy for macOS, blur for Windows)
  - Global event listeners for window control ("quit", "close", "open")
  - System tray click handlers (left/right click to toggle window)

- **src-tauri/src/auth.rs**:
  - OAuth2 flow implementation using tauri-plugin-oauth
  - Token exchange, storage, and refresh logic
  - Tokens stored in `$APPCONFIG/config/google_tokens.json`

- **src-tauri/src/drive.rs**:
  - Google Drive API integration (file uploads, folder management, recent files listing)
  - Creates/finds "CloudTray" folder in user's Google Drive
  - Implements multipart file upload with progress tracking
  - Handles both in-memory uploads (from drag-and-drop) and file path uploads

- **src-tauri/src/config.rs**:
  - App configuration management (retention hours for file expiration)
  - Config stored in `$APPCONFIG/config/app_config.json`

### Key Technologies
- **Tauri v1**: Desktop framework combining Rust backend with web frontend
- **React 18**: UI framework
- **Vite**: Build tool and dev server (runs on port 1420)
- **TypeScript**: Type safety for frontend
- **Rust**: Native backend with tokio for async operations
- **reqwest**: HTTP client for Google API calls
- **i18next**: Internationalization (supports English and Portuguese)

## Development Commands

### Setup
```bash
# Install dependencies
yarn install

# Set up environment variables
# Create .env file with:
VITE_GOOGLE_CLIENT_ID=your_google_client_id
VITE_GOOGLE_CLIENT_SECRET=your_google_client_secret
```

### Development
```bash
# Run in development mode (starts Vite dev server + Tauri)
yarn tauri dev

# Run only frontend dev server
yarn dev
```

### Building
```bash
# Build frontend and compile Tauri app
yarn build

# Build Tauri app only (requires frontend to be built first)
yarn tauri build
```

### Rust Development
```bash
# Check Rust code
cd src-tauri && cargo check

# Run Rust tests
cd src-tauri && cargo test

# Format Rust code
cd src-tauri && cargo fmt
```

## Important Implementation Details

### OAuth Flow
1. Frontend calls `start_oauth_server()` which starts a local server on a random port
2. User is redirected to Google OAuth consent screen in their browser
3. Google redirects back to `http://localhost:<port>` with auth code
4. Backend emits "oauth_callback" event with the URL
5. Frontend extracts code and calls `exchange_auth_code()` to get tokens
6. Tokens are saved via `save_tokens()` and checked on app startup via `get_tokens()`

### File Upload Flow
1. User drops files or selects via dialog
2. Frontend calls `get_or_create_app_folder()` to ensure "CloudTray" folder exists
3. For drag-and-drop: Files are read as ArrayBuffer and passed to `upload_file()`
4. For file dialog: File paths are passed to `upload_file_path()`
5. Backend emits "upload-progress" events with [fileName, progress] during upload
6. Frontend displays progress bars and copies link to clipboard on completion

### System Tray Behavior
- Window is hidden by default and shown on tray icon click
- Window is always on top and positioned near the tray icon
- On Windows: Position is adjusted 10px higher for better alignment
- On macOS: Uses NSVisualEffectMaterial for native vibrancy effect
- App does not appear in dock/taskbar (skipTaskbar: true)

### Configuration Storage
All configuration files are stored in platform-specific app config directories:
- macOS: `~/Library/Application Support/com.cloudtray.app/config/`
- Windows: `%APPDATA%\com.cloudtray.app\config\`
- Linux: `~/.config/com.cloudtray.app/config/`

Files stored:
- `google_tokens.json`: OAuth tokens
- `app_config.json`: App settings (retention_hours)

### Internationalization
- Translation files are in `public/locales/` (en.json, pt.json)
- Language is auto-detected via i18next-browser-languagedetector
- Users can switch language in Settings tab
- Translations are accessed via `t('key')` from useTranslation hook

## Security Notes

- Google OAuth credentials must be set in `.env` file (not committed to repo)
- Tauri's filesystem access is scoped to `$APPCONFIG` directory only
- OAuth tokens are refreshed automatically when expired
- Files uploaded to Google Drive are made publicly readable by default
