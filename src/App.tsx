import { useCallback, useEffect, useRef, useState } from "react";
import { useDropzone } from "react-dropzone";

import { invoke } from "@tauri-apps/api/tauri";
import { emit, listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/api/dialog";
import { open as openShell } from '@tauri-apps/api/shell';

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

interface UploadProgress {
	[key: string]: number;
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

	const onDrop = useCallback(async (acceptedFiles: File[]) => {
		try {
			const appFolder = await invoke<{ id: string, name: string }>("get_or_create_app_folder");
			
			const initialProgress: UploadProgress = {};
			acceptedFiles.forEach(file => {
				initialProgress[file.name] = 1;
			});
			setUploadProgress(initialProgress);
	
			const unlisten = await listen<[string, number]>("upload-progress", (event) => {
				const [fileName, progress] = event.payload;
				setUploadProgress(prev => ({
					...prev,
					[fileName]: progress
				}));
			});

			const isSingleFile = acceptedFiles.length === 1;
			let uploadResult;
	
			for (const file of acceptedFiles) {
				const result = await invoke<{ id: string, name: string, webViewLink: string }>(
					"upload_file",
					{
						fileContent: Array.from(new Uint8Array(await file.arrayBuffer())),
						fileName: file.name,
						folderId: appFolder.id
					}
				);
				
				if (isSingleFile) {
					uploadResult = result;
				}
			}
	
			unlisten();

			if (isSingleFile && uploadResult) {
				setUploadFeedback({ type: 'success', message: t('app.uploadSuccess') });
				await navigator.clipboard.writeText(uploadResult.webViewLink);
				setCopiedId(uploadResult.id);
				setTimeout(() => setCopiedId(null), 5000);
			} else {
				setUploadFeedback({ type: 'success', message: t('app.uploadsSuccess') });
			}
	
			setTimeout(() => {
				setUploadProgress({});
				setUploadFeedback(null);
			}, 5000);
		} catch (error) {
			setUploadProgress({});
			setUploadFeedback({ type: 'error', message: t('app.uploadError') });
			setTimeout(() => {
				setUploadFeedback(null);
			}, 5000);
		}
	}, []);

	const {
		getRootProps,
		getInputProps,
		isDragActive
	} = useDropzone({ onDrop, multiple: true, noClick: true, noKeyboard: true });

	const handleFileSelect = async () => {
		try {
			const selected = await openDialog({
				multiple: true,
				filters: [{
					name: 'All Files',
					extensions: [
						// Imagens
						'png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'bmp', 'tiff', 'ico', 'raw', 'heic',
						// Vídeos
						'mp4', 'avi', 'mov', 'wmv', 'flv', 'mkv', 'webm', 'm4v', '3gp',
						// Áudios
						'mp3', 'wav', 'ogg', 'aac', 'wma', 'm4a', 'flac',
						// Documentos
						'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'txt', 'rtf', 'csv',
						// Arquivos compactados
						'zip', 'rar', '7z', 'tar', 'gz',
						// Outros
						'json', 'yaml', 'yml', 'toml', 'ini', 'conf', 'cfg', 'config', 'sql',
					]
				}]
			});
			
			if (selected) {
				const appFolder = await invoke<{ id: string, name: string }>("get_or_create_app_folder");
				const selectedFiles = Array.isArray(selected) ? selected : [selected];
	
				const initialProgress: UploadProgress = {};
				selectedFiles.forEach(filePath => {
					const fileName = filePath.split(/[/\\]/).pop() || filePath;
					initialProgress[fileName] = 1;
				});
				setUploadProgress(initialProgress);
	
				const unlisten = await listen<[string, number]>("upload-progress", (event) => {
					const [fileName, progress] = event.payload;
					setUploadProgress(prev => ({
						...prev,
						[fileName]: progress
					}));
				});

				const isSingleFile = selectedFiles.length === 1;
				let uploadResult;
	
				for (const filePath of selectedFiles) {
					try {
						const result = await invoke<{ id: string, name: string, webViewLink: string }>(
							"upload_file_path",
							{
								filePath,
								folderId: appFolder.id
							}
						);

						if (isSingleFile) {
							uploadResult = result;
						}
					}
					catch (error) {
						setUploadFeedback({ type: 'error', message: t('app.uploadError') });
					}
				}
	
				unlisten();

				if (isSingleFile && uploadResult) {
					setUploadFeedback({ type: 'success', message: t('app.uploadSuccess') });
					await navigator.clipboard.writeText(uploadResult.webViewLink);
					setCopiedId(uploadResult.id);
					setTimeout(() => setCopiedId(null), 5000);
				} else {
					setUploadFeedback({ type: 'success', message: t('app.uploadsSuccess') });
				}

				setTimeout(() => {
					setUploadProgress({});
					setUploadFeedback(null);
				}, 5000);
			}
		} catch (err) {
			setUploadProgress({});
			setUploadFeedback({ type: 'error', message: t('app.uploadError') });
			setTimeout(() => {
				setUploadFeedback(null);
			}, 5000);
		}
	};

	const handleGoogleLogin = async () => {
		try {
			await emit("close");
			const port = await invoke<number>("start_oauth_server");
			const redirectUri = `${GOOGLE_REDIRECT_URI}:${port}`;

			const unlisten = await listen("oauth_callback", async (event: any) => {
				const url = new URL(event.payload);
				const code = url.searchParams.get("code");
				
				if (code) {
					try {
						const tokens = await invoke<GoogleTokens>("exchange_auth_code", {
							code,
							redirectUri
						});
			
						try {
							await invoke("save_tokens", { tokens });
							setIsAuthenticated(true);
							setCheckingAuth(false);
							await emit("open");
						} catch (error) {
							console.error("Erro ao salvar tokens:", error);
						}
					} catch (error) {
						console.error("Erro detalhado:", error);
					}
				}
				
				unlisten();
			});

			const scope = encodeURIComponent("https://www.googleapis.com/auth/drive.file");
			const authUrl = `https://accounts.google.com/o/oauth2/v2/auth?` +
				`client_id=${GOOGLE_CLIENT_ID}&` +
				`redirect_uri=${encodeURIComponent(redirectUri)}&` +
				`response_type=code&` +
				`scope=${scope}&` +
				`access_type=offline`;

			await openShell(authUrl);
		} catch (error) {
			console.error("Erro na autenticação:", error);
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
					<div {...getRootProps()} className={`drop-area ${isDragActive ? 'drop-area-active' : ''}`}>
						<input {...getInputProps()} />
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
													className={`progress-fill ${progress < 100 ? 'animating' : 'success'}`}
													style={{ width: `${progress}%` }}
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
				{isAuthenticated && <Button onClick={() => setTab('settings')} label={t('app.settings')} hotkey="mod+s" />}
				{isAuthenticated && <Button onClick={handleLogout} label={t('app.logout')} hotkey="mod+l" />}
				<Button onClick={() => emit("quit")} label={t('app.quit')} hotkey="mod+q" />
			</div>
		</main>
	);
}

export default App;
