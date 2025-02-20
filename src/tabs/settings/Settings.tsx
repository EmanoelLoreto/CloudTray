import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';

import './Settings.css';

export interface AppConfig {
    retention_hours: number;
}

import { invoke } from '@tauri-apps/api/tauri';
import { open } from '@tauri-apps/api/shell';

import back from '../../assets/arrow-back.svg';
import flagBr from '../../assets/flag-br.svg';
import flagEs from '../../assets/flag-us.svg';
import google from '../../assets/google.svg';

import Separator from '../../components/Separator';
import Button from '../../components/Button';

const Settings = ({ setTab }: { setTab: (tab: 'upload' | 'settings' | 'recents') => void }) => {
	const { t, i18n } = useTranslation();

    const [config, setConfig] = useState<AppConfig | null>(null);
    const [folderUrl, setFolderUrl] = useState<string>('');

    useEffect(() => {
        loadConfig();
        loadFolderUrl();
    }, []);

    const loadConfig = async () => {
        try {
            const savedConfig = await invoke<AppConfig>('load_or_create_config');
            setConfig(savedConfig);
        } catch (error) {
            console.error('Erro ao carregar configuração:', error);
        }
    };

    const loadFolderUrl = async () => {
        try {
            const folder = await invoke<{ id: string }>('get_or_create_app_folder');
            setFolderUrl(`https://drive.google.com/drive/folders/${folder.id}`);
        } catch (error) {
            console.error('Erro ao carregar URL da pasta:', error);
        }
    };

    const handleRetentionChange = async (hours: number) => {
        try {
            const newConfig: AppConfig = { retention_hours: hours };
            await invoke('save_config', { config: newConfig });
            setConfig(newConfig);
        } catch (error) {
            console.error('Erro ao atualizar configuração:', error);
        }
    };

    const openDriveFolder = () => {
        if (folderUrl) {
            open(folderUrl);
        }
    };

    const handleChangeLanguage = () => {
        i18n.changeLanguage(i18n.language === 'en' ? 'pt' : 'en')
    };

    const retentionOptions = [
        { value: 1, label: t('settings.oneHour') },
        { value: 3, label: t('settings.threeHours') },
        { value: 12, label: t('settings.twelveHours') },
        { value: 24, label: t('settings.oneDay') },
        { value: 48, label: t('settings.twoDays') },
        { value: 72, label: t('settings.threeDays') },
    ];

    if (!config) return null;

    return (
        <main className="container container-settings">
            <div className="settings-header" onClick={() => setTab('upload')}>
                <button className="back-button">
                    <img src={back} alt="Voltar" />
                </button>
                <h3>{t('settings.settings')}</h3>
            </div>
            <div className="select-container">
                <label className="select-label">{t('settings.timeExpiration')}:</label>
                <select
                    value={config.retention_hours}
                    onChange={(e) => handleRetentionChange(Number(e.target.value))}
                    className="retention-select select-select"
                >
                    {retentionOptions.map(option => (
                        <option key={option.value} value={option.value}>
                            {option.label}
                        </option>
                    ))}
                </select>
            </div>

            <Separator />

            <Button
                onClick={openDriveFolder}
                label={t('settings.openGoogle')}
                icon={<img className='icon-button' style={{ filter: 'invert(0)', borderRadius: '8px' }} src={google} />}
                disabled={!folderUrl}
            />

            <Separator />

            <Button
                onClick={handleChangeLanguage}
                label={t('settings.changeLanguage')}
                icon={<img className='icon-button' style={{ filter: 'invert(0)', borderRadius: '8px' }} src={i18n.language === 'en' ? flagBr : flagEs} />}
            />
        </main>
    );
}

export default Settings;