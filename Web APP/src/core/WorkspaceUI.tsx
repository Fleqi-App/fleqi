import { useState } from 'react';
import { ArrowRight, ChevronRight, ChevronsUpDown, FolderClosed, History, Home, PanelLeft, Terminal } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { AccentIcon, AssetIcon, SettingsIcon } from '../icons';
import type { SettingsDestination, WorkspaceDestination, WorkspaceUIProps } from './contracts';

/** 截图二：工作区窗口外壳 + 六张“文件处理能力”卡片。 */
export function WorkspaceUI({
  theme = 'dark',
  account = { name: '未登录', detail: '通过 GitHub 登录', initials: 'FQ' },
  statusText = '菜单栏常驻',
  onOpenSettings,
  onNavigate,
  onRefresh,
  onOpenAccount,
  availableSettings = ['general'],
}: WorkspaceUIProps) {
  const [collapsed, setCollapsed] = useState(false);
  const workspaceItems = [['overview', '概览', Home], ['tasks', '任务记录', History], ['library', '文件能力', FolderClosed]] as const;
  const settingItems = [['models', '模型与账号', 'customLink'], ['permissions', '权限与自检', 'shield'], ['about', '关于与更新', 'infoCircle']] as const;
  const capabilities = [
    ['文件与文件夹', '新建、复制、移动、改名、批量编号、整理、回收站'],
    ['ZIP', '打包、列出内容、解压普通 ZIP'],
    ['图片', 'PNG / JPG / WebP 转换、缩放、旋转与 JPG 压缩'],
    ['音频与视频', 'MP3 / M4A / WAV、MP4 转换、音频提取与基础裁剪'],
    ['PDF', '普通未加密 PDF 合并、拆分、提页、旋转与结构压缩'],
    ['文本与文档', 'TXT / Markdown 读取与创建；简单 DOCX 创建与正文提取'],
  ];
  const openSettings = (page: SettingsDestination) => {
    if (availableSettings.includes(page)) onOpenSettings(page);
  };
  const navigate = (page: WorkspaceDestination) => onNavigate?.(page);

  return <section className={`core-window workspace-window ${theme}`} data-theme={theme} data-collapsed={collapsed} aria-label="Fleqi 工作区">
    <aside className="workspace-sidebar">
      <div className="window-controls" aria-hidden="true"><i/><i/><i/></div>
      <button className="workspace-brand" onClick={() => navigate('overview')} aria-label="Fleqi 概览">
        <img src="/app-icon.png" alt="" width={32} height={32}/>
        <span className="sidebar-copy"><strong>Fleqi</strong><small>文件快捷助手</small></span>
      </button>
      <nav className="workspace-navigation" aria-label="应用导航">
        <div className="navigation-group">
          <p className="sidebar-copy">工作区</p>
          {workspaceItems.map(([id, label, Icon]) => <button className="navigation-row" key={id} title={label} disabled={!onNavigate} onClick={() => navigate(id)}>
            <Icon size={16}/><span className="sidebar-copy">{label}</span>
          </button>)}
        </div>
        <div className="navigation-group">
          <p className="sidebar-copy">连接与工具</p>
          {settingItems.map(([id, label, icon]) => <button className="navigation-row" key={id} title={label} disabled={!availableSettings.includes(id)} onClick={() => openSettings(id)}>
            <AccentIcon name={icon} size={16}/><span className="sidebar-copy">{label}</span><ChevronRight className="sidebar-copy trailing" size={16}/>
          </button>)}
          <div className="navigation-row unavailable"><Terminal size={16}/><span className="sidebar-copy">侧边终端</span><small className="sidebar-copy trailing">P1</small></div>
        </div>
      </nav>
      <div className="workspace-sidebar-footer">
        <button className="workspace-account" disabled={!onOpenAccount} onClick={onOpenAccount} aria-label="GitHub 账号">
          <span className="account-initials">{account.initials}</span><span className="sidebar-copy"><strong>{account.name}</strong><small>{account.detail}</small></span><ChevronsUpDown className="sidebar-copy trailing" size={15}/>
        </button>
        <button className="navigation-row" onClick={() => openSettings('general')} aria-label="设置"><SettingsIcon/><span className="sidebar-copy">设置</span></button>
      </div>
    </aside>
    <main className="workspace-main">
      <header className="workspace-header">
        <Button variant="ghost" size="icon-sm" aria-label="切换侧栏" aria-expanded={!collapsed} onClick={() => setCollapsed(value => !value)}><PanelLeft size={17}/></Button>
        <span className="header-divider" aria-hidden="true"/>
        <nav aria-label="breadcrumb" className="breadcrumb"><span>工作区</span><ChevronRight size={15}/><h1>概览</h1></nav>
        <Button variant="ghost" size="icon-sm" className="trailing" aria-label="刷新状态" disabled={!onRefresh} onClick={onRefresh}><AssetIcon name="clockRotate" size={14}/></Button>
      </header>
      <div className="workspace-content">
        <div className="capabilities-heading"><h2>文件处理能力</h2><p>从 Finder 选中文件后，以自然语言发起任务。</p></div>
        <div className="capabilities-grid">
          {capabilities.map(([title, description]) => <Card className="capability-card" key={title}><AssetIcon name="folder" size={22}/><h3>{title}</h3><p>{description}</p></Card>)}
        </div>
        <Button variant="outline" className="files-settings-button" disabled={!availableSettings.includes('files')} onClick={() => openSettings('files')}>输出与文件处理设置<ArrowRight size={16}/></Button>
      </div>
      <footer className="workspace-footer"><i aria-hidden="true"/>{statusText}</footer>
    </main>
  </section>;
}
