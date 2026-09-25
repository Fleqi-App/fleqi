import type { CSSProperties } from 'react';
import folder from './assets/icons/folder/folder.svg';
import gearshape2 from './assets/icons/common/gearshape.2.svg';
import paintpalette from './assets/icons/common/paintpalette.svg';
import customLink from './assets/icons/common/custom.link.svg';
import shield from './assets/icons/common/shield.lefthalf.filled.svg';
import clockRotate from './assets/icons/common/clock.arrow.trianglehead.counterclockwise.rotate.90.svg';
import infoCircle from './assets/icons/common/info.circle.svg';

const glyphs = { folder, gearshape2, paintpalette, customLink, shield, clockRotate, infoCircle };
export type GlyphName = keyof typeof glyphs;
export function AssetIcon({ name, size = 18, className = '' }: { name: GlyphName; size?: number; className?: string }) {
  return <span aria-hidden="true" className={`asset-icon ${className}`} style={{ width: size, height: size, '--icon-url': `url("${glyphs[name]}")` } as CSSProperties}/>;
}
export function AccentIcon({ name, size = 16 }: { name: GlyphName; size?: number }) {
  return <span aria-hidden="true" className="tint-icon" style={{ width: size, height: size, '--tint-light': '#2E6FE4', '--tint-dark': '#7FB0FF', '--icon-url': `url("${glyphs[name]}")` } as CSSProperties}/>;
}
export function SettingsIcon() { return <AssetIcon name="gearshape2" size={20}/>; }
