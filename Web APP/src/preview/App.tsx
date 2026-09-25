import { useEffect, useState } from 'react';
import { GeneralSettingsUI, WorkspaceUI, type GeneralSettings, type Theme } from '../../CoreUI';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '../components/ui/dialog';

const storageKey = 'fleqi.ui-extraction.preview.v1';
const defaults: GeneralSettings = { launchAtLogin: true, barEnabled: true, activation: 'manual', bubbleSeconds: 4.8, shortcut: 'Control+Alt+Space' };

function readPreviewSettings(): GeneralSettings {
  try {
    const saved = JSON.parse(localStorage.getItem(storageKey) ?? '{}');
    return {
      launchAtLogin: typeof saved?.launchAtLogin === 'boolean' ? saved.launchAtLogin : defaults.launchAtLogin,
      barEnabled: typeof saved?.barEnabled === 'boolean' ? saved.barEnabled : defaults.barEnabled,
      activation: saved?.activation === 'automatic' ? 'automatic' : 'manual',
      bubbleSeconds: saved?.bubbleSeconds === null ? null : typeof saved?.bubbleSeconds === 'number' && saved.bubbleSeconds >= 1 && saved.bubbleSeconds <= 30 ? saved.bubbleSeconds : defaults.bubbleSeconds,
      shortcut: typeof saved?.shortcut === 'string' && saved.shortcut.length < 100 ? saved.shortcut : defaults.shortcut,
    };
  } catch { return defaults; }
}

/** 仅供检视提取物的浏览器容器；不承担原生设置、登录或文件操作。 */
export function App() {
  const params = new URLSearchParams(location.search);
  const theme: Theme = params.get('theme') === 'light' ? 'light' : 'dark';
  const [standalone, setStandalone] = useState(params.get('surface') === 'settings');
  const [open, setOpen] = useState(false);
  const [settings, setSettings] = useState(readPreviewSettings);
  useEffect(() => { document.documentElement.classList.toggle('dark', theme === 'dark'); }, [theme]);
  const change = (patch: Partial<GeneralSettings>) => {
    setSettings(current => {
      const next = { ...current, ...patch };
      try { localStorage.setItem(storageKey, JSON.stringify(next)); } catch { /* 内存中的预览仍可使用。 */ }
      return next;
    });
  };
  const close = () => {
    if (standalone) { setStandalone(false); history.replaceState(null, '', `?theme=${theme}`); }
    setOpen(false);
  };
  return <div className="core-stage">
    {standalone ? <GeneralSettingsUI theme={theme} settings={settings} onChange={change} onClose={close}/> : <WorkspaceUI theme={theme} statusText="界面预览" onOpenSettings={() => setOpen(true)}/>}
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className={`core-settings-dialog ${theme}`} showCloseButton={false}>
        <DialogTitle className="sr-only">Fleqi 设置</DialogTitle><DialogDescription className="sr-only">通用设置界面</DialogDescription>
        <GeneralSettingsUI theme={theme} settings={settings} onChange={change} onClose={close}/>
      </DialogContent>
    </Dialog>
  </div>;
}
