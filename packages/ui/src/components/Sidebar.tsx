import type { ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import fleqiIcon from "../../../../resources/icons/128x128.png";
import { Sidebar as SidebarRoot, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupLabel, SidebarGroupContent, SidebarHeader, SidebarMenu, SidebarMenuItem, SidebarMenuButton } from "./ui/sidebar";

export interface NavItem { id: string; label: string; icon: LucideIcon; unavailable?: string }
export interface NavGroup { label: string; items: NavItem[] }
export interface SidebarProps {
  title: string; groups: NavGroup[]; bottom?: NavGroup; current: string;
  onSelect: (id: string) => void; footer?: ReactNode; width?: number;
}

export function Sidebar({ title, groups, bottom, current, onSelect, footer }: SidebarProps) {
  const items = (group: NavGroup) => <SidebarMenu>{group.items.map((item) => <SidebarMenuItem key={item.id}>
    <SidebarMenuButton isActive={current === item.id} disabled={!!item.unavailable} tooltip={item.unavailable ?? item.label} aria-label={item.label} aria-current={current === item.id ? "page" : undefined} onClick={() => onSelect(item.id)} className="h-9 gap-3 px-3 text-[13px]" data-active={current === item.id}>
      <item.icon className="size-4" aria-hidden="true" /><span>{item.label}</span>
    </SidebarMenuButton>
  </SidebarMenuItem>)}</SidebarMenu>;
  return <SidebarRoot collapsible="icon" aria-label={title} className="app-sidebar border-r-0">
    <div className="native-titlebar-space h-12 shrink-0" aria-hidden="true" />
    <SidebarHeader className="px-3 pb-5 pt-1">
      <SidebarMenu><SidebarMenuItem><SidebarMenuButton size="lg" tooltip={title} onClick={() => onSelect(groups[0]?.items[0]?.id ?? current)} className="h-11 gap-3 px-2">
        <img src={fleqiIcon} alt="Fleqi" width={32} height={32} className="size-8 shrink-0 rounded-lg" />
        <span className="text-sm font-semibold tracking-tight">Fleqi</span>
      </SidebarMenuButton></SidebarMenuItem></SidebarMenu>
    </SidebarHeader>
    <SidebarContent className="gap-5 px-2">
      {groups.map((group) => <SidebarGroup key={group.label} className="p-0"><SidebarGroupLabel className="px-3 text-[11px] font-medium">{group.label}</SidebarGroupLabel><SidebarGroupContent>{items(group)}</SidebarGroupContent></SidebarGroup>)}
    </SidebarContent>
    <SidebarFooter className="gap-3 p-3">
      {bottom && items(bottom)}
      {footer && <div className="flex items-center gap-2 px-3 py-1 text-[11px] text-muted-foreground group-data-[collapsible=icon]:hidden"><span className="size-1.5 shrink-0 rounded-full bg-success" />{footer}</div>}
    </SidebarFooter>
  </SidebarRoot>;
}
