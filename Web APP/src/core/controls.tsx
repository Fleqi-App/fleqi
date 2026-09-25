import type { ReactNode } from 'react';
import { Card } from '../components/ui/card';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select';
import { AutoHeight } from '../motion';

export function Choice({ label, value, onChange, options }: { label: string; value: string; onChange: (value: string) => void; options: { value: string; label: string }[] }) {
  return <Select value={value} onValueChange={onChange}><SelectTrigger aria-label={label}><SelectValue/></SelectTrigger><SelectContent>{options.map(option => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}</SelectContent></Select>;
}

export function SettingRow({ title, description, children }: { title: string; description?: string; children?: ReactNode }) {
  return <div className="setting-row"><div className="setting-label"><h3>{title}</h3>{description && <p>{description}</p>}</div><div className="setting-control">{children}</div></div>;
}

export function SettingsGroup({ title, children }: { title?: string; children: ReactNode }) {
  return <section className="settings-group">{title && <h2>{title}</h2>}<Card className="settings-card"><AutoHeight>{children}</AutoHeight></Card></section>;
}
