import { AnimatePresence, MotionConfig, motion, useReducedMotion } from 'motion/react';
import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import tokens from './tokens.json';

export const curve = tokens.curve as [number, number, number, number];
export const duration = tokens.duration;
export const collapsedScale = tokens.collapsedScale;
export const transition = (kind: keyof typeof duration = 'layout', reduced = false) => ({ type: 'tween' as const, ease: curve, duration: reduced ? 0 : duration[kind] });

export function MotionProvider({ children }: { children: ReactNode }) {
  const reduced = !!useReducedMotion();
  return <MotionConfig reducedMotion="user" transition={transition('layout', reduced)}>{children}</MotionConfig>;
}

/** 观察内容自身尺寸，避免父容器的动画反过来影响测量。 */
export function AutoHeight({ children }: { children: ReactNode }) {
  const content = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState<number>();
  const reduced = !!useReducedMotion();
  useLayoutEffect(() => {
    const element = content.current;
    if (!element) return;
    const measure = () => setHeight(element.offsetHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return <motion.div className="motion-size" initial={false} animate={{ height: height ?? 'auto' }} transition={transition('layout', reduced)}><div ref={content} className="motion-size-content">{children}</div></motion.div>;
}

export { AnimatePresence, motion, useReducedMotion };
