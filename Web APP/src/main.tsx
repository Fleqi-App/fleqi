import { createRoot } from 'react-dom/client';
import { MotionProvider } from './motion';
import { installMotionOrigins } from './motion/origin';
import tokens from './motion/tokens.json';
import { App } from './preview/App';
import './shadcn.css';
import './motion.css';
import './styles.css';

installMotionOrigins();
document.documentElement.style.setProperty('--motion-curve', `cubic-bezier(${tokens.curve.join(',')})`);
for (const [name, seconds] of Object.entries(tokens.duration)) document.documentElement.style.setProperty(`--motion-${name}`, `${seconds}s`);
createRoot(document.getElementById('root')!).render(<MotionProvider><App/></MotionProvider>);
