import { useEffect, useRef, useState } from "react";

import { invoke } from "@tauri-apps/api/tauri";
import { emit, listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/api/dialog";
import { open as openShell } from '@tauri-apps/api/shell';
import { appWindow } from '@tauri-apps/api/window';

import "./App.css";

import Settings from "./tabs/settings/Settings";
import Recents from "./tabs/Recents/Recents";
import Button from "./components/Button";
import Separator from "./components/Separator";

import recent from "./assets/recent.svg";
import fileSearch from "./assets/file-search.svg";
import google from "./assets/google.svg";
import About from "./tabs/About/About";
import { useTranslation } from "react-i18next";

const GOOGLE_CLIENT_ID = import.meta.env.VITE_GOOGLE_CLIENT_ID;
const GOOGLE_CLIENT_SECRET = import.meta.env.VITE_GOOGLE_CLIENT_SECRET;
const GOOGLE_REDIRECT_URI = "http://localhost";

interface GoogleTokens {
	access_token: string;
	refresh_token: string;
	expires_in: number;
	token_type: string;
}

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

interface DriveFile {
    id: string;
    name: string;
    webViewLink: string;
}

export function formatSpeed(bps: number): string {
    if (bps === 0) return '';
    if (bps >= 1024 * 1024) return `${(bps / (1024 * 1024)).toFixed(1)} MB/s`;
    if (bps >= 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
    return `${bps} B/s`;
}

export function formatETA(bytesSent: number, totalBytes: number, speedBps: number): string {
    if (speedBps === 0 || bytesSent >= totalBytes) return '';
    const remaining = (totalBytes - bytesSent) / speedBps;
    if (remaining >= 60) return `~${Math.ceil(remaining / 60)}min`;
    return `~${Math.ceil(remaining)}s`;
}

const App = () => {
	const { t } = useTranslation();

	const checkingAuthRef = useRef(false);

	const [checkingAuth, setCheckingAuth] = useState(false);
	const [isAuthenticated, setIsAuthenticated] = useState(false);
	const [uploadProgress, setUploadProgress] = useState<UploadProgress>({});
	const [uploadFeedback, setUploadFeedback] = useState<{ type: 'success' | 'error', message: string } | null>(null);
	const [copiedId, setCopiedId] = useState<string | null>(null);
	const [tab, setTab] = useState<'upload' | 'settings' | 'recents' | 'about'>('upload');
	const [recentFiles, setRecentFiles] = useState<{ id: string, name: string, webViewLink: string }[]>([]);

	const [isDragActive, setIsDragActive] = useState(false);

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

	const handleGoogleLogin = async () => {
		try {
			setCheckingAuth(true);
			const port = await invoke<number>("start_oauth_server");
			const redirectUri = `${GOOGLE_REDIRECT_URI}:${port}`;

			const unlisten = await listen("oauth_callback", async (event: any) => {
				const url = new URL(event.payload);
				const code = url.searchParams.get("code");
				
				if (code) {
					try {
						if (!GOOGLE_CLIENT_ID || !GOOGLE_CLIENT_SECRET) {
							throw new Error("Google credentials not configured");
						}
						
						await invoke("set_google_credentials", {
							clientId: GOOGLE_CLIENT_ID,
							clientSecret: GOOGLE_CLIENT_SECRET
						});
						
						const tokens = await invoke<GoogleTokens>("exchange_auth_code", {
							code,
							redirectUri
						});
			
						try {
							await invoke("save_tokens", { tokens });
							setIsAuthenticated(true);
							await emit("open");
						} catch (error) {
							console.error("Erro ao salvar tokens:", error);
							await emit("open");
						}
					} catch (error) {
						await emit("open");
					}
				} else {
					await emit("open");
				}
				
				setCheckingAuth(false);
				unlisten();
			});

			const scope = encodeURIComponent("https://www.googleapis.com/auth/drive.file");
			const authUrl = `https://accounts.google.com/o/oauth2/v2/auth?` +
				`client_id=${GOOGLE_CLIENT_ID}&` +
				`redirect_uri=${encodeURIComponent(redirectUri)}&` +
				`response_type=code&` +
				`scope=${scope}&` +
				`access_type=offline`;

			await emit("close");
			await openShell(authUrl);
		} catch (error) {
			console.error("Erro na autenticação:", error);
			setCheckingAuth(false);
		}
	};

	useEffect(() => {
		const checkAuth = async () => {
			if (checkingAuthRef.current) return;
			checkingAuthRef.current = true;
			setCheckingAuth(true);

			try {
				await invoke("set_google_credentials", {
					clientId: GOOGLE_CLIENT_ID,
					clientSecret: GOOGLE_CLIENT_SECRET
				});

				await invoke("get_tokens");
				setIsAuthenticated(true);
				setCheckingAuth(false);
			} catch (error) {
				setIsAuthenticated(false);
				setCheckingAuth(false);
			}
		};

		if (navigator.platform.indexOf('Win') !== -1) {
			document.documentElement.classList.add('is-windows');
		}

		checkAuth();

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
	}, []);

	const handleLogout = async () => {
		await invoke("logout");
		setIsAuthenticated(false);
	};

	const handleCopyLink = async (link: string, id: string) => {
		await navigator.clipboard.writeText(link);
		setCopiedId(id);
		setTimeout(() => setCopiedId(null), 2000);
	};
	
	if (tab === 'settings') {
		return <Settings setTab={setTab} />;
	}

	if (tab === 'recents') {
		return <Recents
			setTab={setTab}
			handleCopyLink={handleCopyLink}
			copiedId={copiedId}
			recentFiles={recentFiles}
			setRecentFiles={setRecentFiles}
		/>
	}

	if (tab === 'about') {
		return <About setTab={setTab} />;
	}

	return (
		<main className="container" style={{ justifyContent: isAuthenticated ? 'flex-start' : 'space-between', height: isAuthenticated ? '100%' : '95%' }}>
			{!isAuthenticated && !checkingAuth && (
				<div className="container-login-google" onClick={() => handleGoogleLogin()}>
					<img src={google} alt="Google" className="icon-button" style={{ filter: 'invert(0)' }} />
					<span>{t('app.loginWithGoogleDrive')}</span>
				</div>
			)}

			{!isAuthenticated && checkingAuth && (
				<div className="container-login-google">
					<span>{t('app.checkingAuth')}</span>
					<div className="loading-spinner"></div>
				</div>
			)}
			
			{isAuthenticated && (
				<>
					<div className={`drop-area ${isDragActive ? 'drop-area-active' : ''}`}>
						{Object.entries(uploadProgress).length <= 0 && isDragActive && (
							<p className="drop-area-text drag">{t('app.startUpload')}</p>
						)}
						
						{Object.entries(uploadProgress).length <= 0 && !isDragActive && (
							<p className="drop-area-text drag">{t('app.dragFilesHere')}</p>
						)}

						{Object.entries(uploadProgress).length > 0 && (
							<>
								<p className="drop-area-text">{t('app.uploading')}</p>
							
								<div className="container-upload-files">
									{Object.entries(uploadProgress).map(([fileName, progress]) => (
										<div key={fileName} className="upload-progress">
											<span className="filename">
												{fileName.length > 13 ? fileName.slice(0, 13) + '...' : fileName}
											</span>
											<div className="progress-bar">
												<div
													className={`progress-fill ${progress.percent < 100 ? 'animating' : 'success'}`}
													style={{ width: `${progress.percent}%` }}
												/>
											</div>
										</div>
									))}
								</div>
							</>
						)}
					</div>


					{uploadFeedback && (
						<div className={`upload-feedback ${uploadFeedback.type}`}>
							{uploadFeedback.message}
						</div>
					)}

					{copiedId && (
						<div className="copied-feedback info">
							{t('app.copiedLink')}
						</div>
					)}

					<Separator />

					<Button
						onClick={handleFileSelect}
						label={t('app.selectFile')}
						hotkey="mod+o"
						icon={<img src={fileSearch} alt="Select file" className="icon-button" />}
					/>

					<Button
						onClick={() => setTab('recents')}
						label={t('app.recentUploads')}
						hotkey="mod+r"
						icon={<img src={recent} alt="Recent uploads" className="icon-button" />}
					/>
				</>
			)}
			<div>
				<Separator />

				<Button onClick={() => setTab('about')} label={t('app.about') + ' CloudTray'} />

				<Separator />

				<Button onClick={() => emit("close")} label={t('app.close')} hotkey="mod+c" />
				<Button onClick={() => setTab('settings')} label={t('app.settings')} hotkey="mod+s" />
				{isAuthenticated && <Button onClick={handleLogout} label={t('app.logout')} hotkey="mod+l" />}
				<Button onClick={() => emit("quit")} label={t('app.quit')} hotkey="mod+q" />
			</div>
		</main>
	);
}

export default App;
