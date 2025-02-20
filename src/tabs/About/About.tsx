import { open as openShell } from '@tauri-apps/api/shell';
import { useTranslation } from 'react-i18next';

import back from '../../assets/arrow-back.svg';

import './About.css';

const Settings = ({ setTab }: { setTab: (tab: 'upload' | 'settings' | 'recents') => void }) => {
	const { t } = useTranslation();

    return (
        <main className="container container-about">
            <div className="about-header" onClick={() => setTab('upload')}>
                <button className="back-button">
                    <img src={back} alt="Voltar" />
                </button>
                <h3>{t('about.about')}</h3>
            </div>
            <div className="about-content">
                <p>
                    {t('about.aboutContent')}
                </p>
                <p>
                    {t('about.aboutContent2')} <a href='#' onClick={() => openShell('https://www.youtube.com/watch?v=IN1zI7C8ER4')}>desktop app</a> {t('about.aboutContent2.2')}
                </p>
                <p>
                    {t('about.aboutContent3')} <a href='#' onClick={() => openShell('https://github.com/EmanoelLoreto/CloudTray')}>GitHub</a>.
                </p>
            </div>
        </main>
    );
}

export default Settings;