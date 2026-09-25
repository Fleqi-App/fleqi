import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { ChevronRight, Keyboard, Search, X } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Input } from '../components/ui/input';
import { Switch } from '../components/ui/switch';
import { Choice, SettingRow, SettingsGroup } from './controls';
import { AccentIcon } from '../icons';
import type { GeneralSettingsUIProps } from './contracts';

export const settingsNavigation = [
  ['general', '通用', 'gearshape2', '启动 登录 底部栏 唤起 快捷键 气泡'],
  ['appearance', '外观', 'paintpalette', '主题 颜色 玻璃'],
  ['models', '模型与账号', 'customLink', '模型 账号 登录'],
  ['permissions', '权限与自检', 'shield', '权限 自检'],
  ['files', '文件处理', 'folder', '文件 输出 目录'],
  ['tasks', '任务与诊断', 'clockRotate', '任务 历史 诊断'],
  ['about', '关于与更新', 'infoCircle', '关于 版本 更新'],
] as const;

function shortcutLabel(value: string) {
  return value.replaceAll('Control', '⌃').replaceAll('Alt', '⌥').replaceAll('Super', '⌘').replaceAll('Shift', '⇧').replaceAll('Space', '空格').replaceAll('+', ' ');
}

function ShortcutControl({ value, onChange, onRecording }: { value: string; onChange: (value: string) => void; onRecording?: (recording: boolean) => void }) {
  const [recording, setRecording] = useState(false);
  const [candidate, setCandidate] = useState(value);
  const [hint, setHint] = useState('');
  const input = useRef<HTMLButtonElement>(null);
  useEffect(() => setCandidate(value), [value]);
  useEffect(() => {
    if (!recording) return;
    onRecording?.(true);
    const stop = () => setRecording(false);
    window.addEventListener('blur', stop);
    return () => { window.removeEventListener('blur', stop); onRecording?.(false); };
  }, [recording, onRecording]);

  return <div className="shortcut-control">
    <div className="shortcut-actions"><Button ref={input} variant="outline" aria-label="录入底部栏快捷键" aria-pressed={recording} onClick={() => { setRecording(true); setHint('按下组合键；Esc 取消'); input.current?.focus(); }} onBlur={() => setRecording(false)} onKeyDown={event => {
      if (!recording) return;
      event.preventDefault(); event.stopPropagation();
      if (event.key === 'Escape') { setRecording(false); setHint('已取消录入'); return; }
      if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return;
      if (event.nativeEvent.isComposing || (!event.ctrlKey && !event.altKey && !event.metaKey)) { setHint('请至少包含 Control、Option 或 Command'); return; }
      const code = event.code;
      const key = code === 'Space' ? 'Space' : /^Key[A-Z]$/.test(code) ? code.slice(3) : /^Digit[0-9]$/.test(code) ? code.slice(5) : /^F\d{1,2}$/.test(code) ? code : ({ ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right', Enter: 'Enter', Backspace: 'Backspace' } as Record<string, string>)[code];
      if (!key) { setHint('此按键暂不支持'); return; }
      setCandidate([event.ctrlKey && 'Control', event.altKey && 'Alt', event.shiftKey && 'Shift', event.metaKey && 'Super', key].filter(Boolean).join('+'));
      setRecording(false); setHint('点击保存');
    }}><Keyboard size={15}/>{recording ? '请按下快捷键…' : shortcutLabel(candidate)}</Button>
      <Button variant="outline" size="sm" disabled={recording || candidate === value} onClick={() => { onChange(candidate); setHint('已保存'); }}>保存</Button>
    </div>
    <p role="status">{hint || '点击录入，然后按下你要使用的组合键'}</p>
  </div>;
}

/** 截图一：设置外壳 + 通用页。仅输出设置 patch，不调用原生功能。 */
export function GeneralSettingsUI({ theme = 'dark', settings, version = '0.0.1', onChange, onClose, onNavigate, onShortcutRecording }: GeneralSettingsUIProps) {
  const [query, setQuery] = useState('');
  const keyword = query.trim().toLowerCase();
  const navigation = settingsNavigation.filter(([, name, , words]) => `${name} ${words}`.toLowerCase().includes(keyword));
  return <section className={`core-window settings-window ${theme}`} data-theme={theme} aria-label="Fleqi 通用设置">
    <aside className="settings-sidebar">
      <div className="settings-search"><Search size={14} aria-hidden="true"/><Input aria-label="搜索设置与功能" placeholder="搜索设置与功能" value={query} onChange={event => setQuery(event.target.value)}/>{query && <button aria-label="清除搜索" onClick={() => setQuery('')}><X size={12}/></button>}</div>
      <nav aria-label="设置分类">{navigation.map(([id, label, glyph]) => <button className="settings-navigation-row" key={id} aria-current={id === 'general' ? 'page' : undefined} disabled={id !== 'general' && !onNavigate} onClick={() => id === 'general' ? setQuery('') : onNavigate?.(id)}><AccentIcon name={glyph} size={16}/><span>{label}</span></button>)}{!navigation.length && <p className="settings-search-empty">没有匹配的设置</p>}</nav>
      <footer>Fleqi · {version}</footer>
    </aside>
    <main className="settings-main">
      <header className="settings-header"><nav aria-label="breadcrumb" className="breadcrumb"><span>设置</span><ChevronRight size={15}/><h1>通用</h1></nav><Button variant="ghost" size="icon-sm" aria-label="关闭设置" onClick={onClose}><X size={18}/></Button></header>
      <div className="settings-scroll">
        <SettingsGroup>
          <SettingRow title="登录时启动" description="登录后驻留菜单栏。"><Switch aria-label="登录时启动" checked={settings.launchAtLogin} onCheckedChange={value => onChange({ launchAtLogin: value })}/></SettingRow>
          <SettingRow title="启用底部快捷栏" description="使用菜单栏或快捷键唤起。"><Switch aria-label="启用底部快捷栏" checked={settings.barEnabled} onCheckedChange={value => onChange({ barEnabled: value })}/></SettingRow>
          <SettingRow title="唤起方式" description="移动 Finder 时隐藏，放下后重新显示。"><Choice label="唤起方式" value={settings.activation} onChange={value => onChange({ activation: value as 'manual' | 'automatic' })} options={[{ value: 'manual', label: '仅手动唤起' }, { value: 'automatic', label: '随 Finder 自动显示' }]}/></SettingRow>
        </SettingsGroup>
        <SettingsGroup title="任务回复气泡">
          <SettingRow title="显示方式" description={settings.bubbleSeconds === null ? '气泡常驻，右侧带关闭按钮。' : `任务结束后气泡显示 ${settings.bubbleSeconds.toFixed(1)} 秒后自动消失。`}><Choice label="显示方式" value={settings.bubbleSeconds === null ? 'pinned' : 'timed'} onChange={value => onChange({ bubbleSeconds: value === 'pinned' ? null : 4.8 })} options={[{ value: 'timed', label: '自动消失' }, { value: 'pinned', label: '常驻显示' }]}/></SettingRow>
          {settings.bubbleSeconds !== null && <div className="bubble-countdown"><div><label htmlFor="core-bubble-seconds">消失倒计时</label><span>{settings.bubbleSeconds.toFixed(1)} 秒</span></div><input id="core-bubble-seconds" type="range" min={1} max={30} step={0.1} value={settings.bubbleSeconds} aria-label="气泡消失秒数" style={{ '--range-progress': `${(settings.bubbleSeconds - 1) / 29 * 100}%` } as CSSProperties} onChange={event => onChange({ bubbleSeconds: Number(event.target.value) })}/><p>1–30 秒，无级调节。</p></div>}
        </SettingsGroup>
        <SettingsGroup title="快捷键"><div className="shortcut-section"><h3>显示 / 收起底部栏</h3><ShortcutControl value={settings.shortcut} onChange={value => onChange({ shortcut: value })} onRecording={onShortcutRecording}/></div></SettingsGroup>
        <p className="settings-footnote">关闭窗口后，Fleqi 继续驻留菜单栏。使用菜单中的“退出 Fleqi”结束应用。</p>
      </div>
    </main>
  </section>;
}
