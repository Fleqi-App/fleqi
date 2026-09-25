/** 两份 UI 的最小输入与回调；没有数据库、运行时或原生宿主类型。 */
export type Theme = 'light' | 'dark';
export type WorkspaceDestination = 'overview' | 'tasks' | 'library';
export type SettingsDestination = 'general' | 'appearance' | 'models' | 'permissions' | 'files' | 'tasks' | 'about';

export interface GeneralSettings {
  launchAtLogin: boolean;
  barEnabled: boolean;
  activation: 'manual' | 'automatic';
  bubbleSeconds: number | null;
  shortcut: string;
}

export interface WorkspaceUIProps {
  theme?: Theme;
  account?: { name: string; detail: string; initials: string };
  statusText?: string;
  onOpenSettings: (page: SettingsDestination) => void;
  onNavigate?: (page: WorkspaceDestination) => void;
  onRefresh?: () => void;
  onOpenAccount?: () => void;
  availableSettings?: readonly SettingsDestination[];
}

export interface GeneralSettingsUIProps {
  theme?: Theme;
  settings: GeneralSettings;
  version?: string;
  onChange: (patch: Partial<GeneralSettings>) => void;
  onClose: () => void;
  onNavigate?: (page: SettingsDestination) => void;
  onShortcutRecording?: (recording: boolean) => void;
}
