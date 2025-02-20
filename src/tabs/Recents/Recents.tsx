import { useEffect, useState } from "react";

import { invoke } from "@tauri-apps/api/tauri";
import back from "../../assets/arrow-back.svg";

import "./Recents.css";
import { useTranslation } from "react-i18next";

interface RecentFile {
    id: string;
    name: string;
    webViewLink: string;
}

const Recents = ({
    setTab,
    handleCopyLink,
    copiedId,
    recentFiles,
    setRecentFiles,
}: {
    setTab: (tab: 'upload' | 'settings' | 'recents') => void,
    handleCopyLink: (link: string, id: string) => void,
    copiedId: string | null,
    recentFiles: { id: string, name: string, webViewLink: string }[],
    setRecentFiles: (uploads: { id: string, name: string, webViewLink: string }[]) => void,
}) => {
	const { t } = useTranslation();

    const [loading, setLoading] = useState(true);

    let oldRecentFiles = recentFiles;

    const handleDelete = async (fileId: string) => {
        try {
            oldRecentFiles = recentFiles;

            // @ts-ignore
            setRecentFiles((prev) => prev.filter(file => file.id !== fileId));

            await invoke('delete_file', { fileId });
        } catch (error) {
            console.error('Error deleting file:', error);
            setRecentFiles(oldRecentFiles);
        }
    };

    useEffect(() => {
        const fetchRecentFiles = async () => {
            try {
                setLoading(true);

                const files = await invoke<RecentFile[]>("list_recent_files");

                setRecentFiles(files);
            } catch (error) {
                console.error('Error fetching recent files:', error);
            } finally {
                setLoading(false);
            }
        };
        fetchRecentFiles();
    }, []);

    return (
        <main className="container">
            <div className="recent-files-list-header" onClick={() => setTab('upload')}>
                <button className="back-button">
                    <img src={back} alt="Back" />
                </button>
                <h3>{t('recents.recents')}</h3>

                {recentFiles.length > 0 && loading && (
                    <div className="loading-spinner"></div>
                )}
            </div>
            {recentFiles.length > 0 && (
                <div className="recent-files-list">
                    {recentFiles.map(file => (
                        <div key={file.id} className="recent-file-item">
                            <span className="recent-file-name">
                                {file.name.length > 13
                                    ? `${file.name.slice(0, 13)}...` 
                                    : file.name}
                            </span>
                            <div className="buttons-container-file-item">
                                <button 
                                    className="copy-link-button"
                                    onClick={() => handleCopyLink(file.webViewLink, file.id)}
                                >
                                    {copiedId === file.id ? t('recents.copied') : t('recents.copyLink')}
                                </button>
                                <button 
                                    className="delete-button"
                                    onClick={() => handleDelete(file.id)}
                                >
                                🗑️
                                </button>
                            </div>
                        </div>
                    ))}
                </div>
            )}
            
            {recentFiles.length <= 0 && loading && (
                <div className="loading-container">
                    <p>{t('recents.loading')}</p>
                    <div className="loading-spinner"></div>
                </div>
            )}

            {recentFiles.length === 0 && !loading && (
                <div className="loading-container">
                    <p>{t('recents.noRecentUploads')}</p>
                </div>
            )}
        </main>
    );
};

export default Recents;