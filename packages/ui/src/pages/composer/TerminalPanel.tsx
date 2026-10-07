import { useCallback, useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import type { TerminalSnapshot } from "@fleqi/contracts";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/ui/button";
import { InlineStatus } from "../../components/InlineStatus";
import { TrafficLights } from "../../components/WindowChrome";
import { isAppError, type TerminalStreamEvent } from "../../adapters/host";
import { useHost } from "../../store/host";
import { authorizeTerminalStyles, terminalDocument } from "./terminal-document";

/** 消费位点回执周期：宿主 terminal_ack 记录 UI 已消费的 cursor（流控回执）。 */
const ACK_INTERVAL_MS = 2000;

/**
 * 终端面板（UI-TERMINAL，M4）：xterm.js 完整渲染（ANSI/光标/清屏）、
 * fit→resize 同步 PTY 尺寸、onData 原始输入直通（IME 由 xterm 组合处理），
 * 输出经宿主订阅按 cursor 追加；定期回执消费位点。
 */
export function TerminalPanel({ sessionId, onClose }: { sessionId: string | null; onClose: () => void }) {
  const host = useHost();
  const isMac = host.bootstrap?.buildInfo.targetOs === "macos";
  const [snapshot, setSnapshot] = useState<TerminalSnapshot | null>(null);
  const [status, setStatus] = useState<"idle" | "opening" | "running" | "failed">("idle");
  const [error, setError] = useState<string | null>(null);
  const leaseRef = useRef<string | null>(null);
  const cursorRef = useRef(0);
  const ackedRef = useRef(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const statusRef = useRef(status);
  const generationRef = useRef(0);
  const unsubscribeRef = useRef<(() => Promise<void>) | null>(null);
  statusRef.current = status;

  const open = useCallback(async () => {
    if (!sessionId || !containerRef.current) return;
    setStatus("opening");
    setError(null);
    const generation = ++generationRef.current;
    const current = () => generationRef.current === generation;
    // xterm 实例在 effect 里创建；这里只负责宿主侧打开与订阅。
    try {
      await host.adapter.terminalOpen(sessionId, xtermRef.current?.cols, xtermRef.current?.rows);
      if (!current()) return;
      const lease = await host.adapter.terminalAcquireLease(sessionId, "composer-panel");
      if (!current()) { await host.adapter.terminalReleaseLease(sessionId, lease); return; }
      leaseRef.current = lease;
      const snap = await host.adapter.terminalSnapshot(sessionId);
      if (!current()) return;
      setSnapshot(snap);
      cursorRef.current = Number(snap.streamCursor) || 0;
      ackedRef.current = cursorRef.current;
      xtermRef.current?.reset();
      xtermRef.current?.write(snap.screen);
      setStatus("running");
      statusRef.current = "running";
      // 窗口可能已在 terminalOpen 在途期间展开；那次 ResizeObserver 发生在
      // opening 状态，尚未通知 PTY。连接完成后必须再同步当前实际行列。
      fitRef.current?.fit();
      if (xtermRef.current) {
        await host.adapter.terminalResize(sessionId, xtermRef.current.cols, xtermRef.current.rows);
      }
      if (!current()) return;
      xtermRef.current?.focus();
      const unsubscribe = await host.adapter.terminalSubscribe(sessionId, cursorRef.current, (event: TerminalStreamEvent) => {
        if (!current()) return;
        if (event.kind === "output" && event.bytes && event.bytes.length > 0) {
          xtermRef.current?.write(new Uint8Array(event.bytes));
          cursorRef.current = event.cursor ?? cursorRef.current;
        } else if (event.kind === "promptReady") {
          void host.adapter.terminalSnapshot(sessionId).then((value) => {
            if (current()) setSnapshot(value);
          }, () => undefined);
        } else if (event.kind === "exited") {
          setStatus("failed");
          setError("终端已退出");
        }
      });
      if (current()) unsubscribeRef.current = unsubscribe;
      else await unsubscribe();
    } catch (reason) {
      if (!current()) return;
      setStatus("failed");
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    }
  }, [host.adapter, sessionId]);

  useEffect(() => {
    if (!sessionId || !containerRef.current) return;
    const terminal = new Terminal({
      documentOverride: terminalDocument(),
      fontSize: host.bootstrap?.settings.terminalFontSize ?? 13,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      cursorBlink: true,
      convertEol: false,
      scrollback: 10000,
      minimumContrastRatio: 7,
      theme: {
        background: "#18191b", foreground: "#f3f4f6",
        cursor: "#f3f4f6", cursorAccent: "#18191b",
        selectionBackground: "#435b80", selectionInactiveBackground: "#35445b",
        selectionForeground: "#ffffff",
        black: "#a1a1aa", red: "#ff8d8d", green: "#90dfa5", yellow: "#f5d68a",
        blue: "#91bbff", magenta: "#d8a4ef", cyan: "#83d9e0", white: "#e4e4e7",
        brightBlack: "#b4b4bf", brightRed: "#ffb1b1", brightGreen: "#b4edc3", brightYellow: "#ffe5a6",
        brightBlue: "#b6d1ff", brightMagenta: "#eac5ff", brightCyan: "#acf0f5", brightWhite: "#ffffff",
      },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(containerRef.current);
    authorizeTerminalStyles(containerRef.current);
    fit.fit();
    xtermRef.current = terminal;
    fitRef.current = fit;
    let disposed = false;
    let inputQueue = Promise.resolve();
    // 原始输入直通：xterm 已完成按键/组合键/粘贴的终端序列化；IME 组合不发送半角输入。
    terminal.onData((data) => {
      if (statusRef.current !== "running" || !sessionId) return;
      const lease = leaseRef.current;
      inputQueue = inputQueue.then(async () => {
        if (disposed) return;
        await host.adapter.terminalInput(sessionId, lease, new TextEncoder().encode(data));
      }).catch((reason: unknown) => {
        if (!disposed) setError(isAppError(reason) ? reason.message : String(reason));
      });
    });
    let resizeFrame = 0;
    let lastSize = "";
    const resize = () => {
      resizeFrame = 0;
      if (!containerRef.current || containerRef.current.clientHeight < 40) return;
      fit.fit();
      const cols = terminal.cols;
      const rows = terminal.rows;
      const size = `${cols}:${rows}`;
      if (sessionId && statusRef.current === "running" && size !== lastSize) {
        lastSize = size;
        void host.adapter.terminalResize(sessionId, cols, rows).catch(() => undefined);
      }
    };
    const onResize = () => { if (!resizeFrame) resizeFrame = requestAnimationFrame(resize); };
    window.addEventListener("resize", onResize);
    const observer = typeof ResizeObserver !== "undefined" ? new ResizeObserver(onResize) : null;
    observer?.observe(containerRef.current);
    void open();
    return () => {
      disposed = true;
      window.removeEventListener("resize", onResize);
      observer?.disconnect();
      cancelAnimationFrame(resizeFrame);
      generationRef.current += 1;
      statusRef.current = "idle";
      const unsubscribe = unsubscribeRef.current;
      unsubscribeRef.current = null;
      void unsubscribe?.().catch(() => undefined);
      const lease = leaseRef.current;
      if (lease) void host.adapter.terminalReleaseLease(sessionId, lease).catch(() => undefined);
      leaseRef.current = null;
      xtermRef.current = null;
      fitRef.current = null;
      terminal.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 实例随会话重建；status 由 open 更新后经 ref 生效
  }, [sessionId, open, host.adapter]);

  // PTY 控制事件由应用层转成低频 terminalChanged；字节 Channel 只保证输出。
  useEffect(() => {
    if (!sessionId || status !== "running") return;
    let current = true;
    void host.adapter.terminalSnapshot(sessionId).then((value) => {
      if (current) setSnapshot(value);
    }, () => undefined);
    return () => { current = false; };
  }, [host.adapter, sessionId, host.eventVersion, status]);

  // 消费位点回执：UI 已渲染到 cursor，宿主记录（有界环 + 分段持久化保证内存上限）。
  useEffect(() => {
    if (!sessionId || status !== "running") return;
    const timer = window.setInterval(() => {
      if (cursorRef.current > ackedRef.current) {
        ackedRef.current = cursorRef.current;
        void host.adapter.terminalAck(sessionId, ackedRef.current).catch(() => undefined);
      }
    }, ACK_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [host.adapter, sessionId, status]);

  return (
    <section
      aria-label="终端面板"
      data-testid="terminal-panel"
      data-status={status}
      className="composer-panel terminal-panel absolute inset-x-2 top-2 bottom-[80px] z-20 flex min-h-0 flex-col overflow-hidden rounded-2xl border shadow-lg"
    >
      <header className="terminal-titlebar flex h-11 shrink-0 items-center gap-2 border-b px-3">
        {isMac && <TrafficLights variant="close" closeLabel="收起终端" onClose={onClose} className="mr-1" />}
        <h2 className="text-sm font-semibold">终端</h2>
        {snapshot && (
          <Badge tone={snapshot.shellReadiness === "ready" ? "success" : "warning"} data-testid="terminal-readiness">
            {snapshot.shellReadiness === "ready" ? "可输入" : snapshot.shellReadiness === "unknown" ? "状态未知" : "忙碌"}
          </Badge>
        )}
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground" data-testid="terminal-directory">
          {snapshot?.currentDirectory ?? (status === "opening" ? "创建中" : "未启动")}
        </span>
        {!isMac && <Button variant="ghost" size="icon-sm" onClick={onClose} aria-label="收起终端">
          <X aria-hidden="true" className="size-4" />
        </Button>}
      </header>
      {snapshot?.shell === "/bin/bash" && snapshot.shellReadiness === "unknown" && <InlineStatus tone="warning" className="px-3 py-1">Bash 自动目录同步尚未就绪或不可用，请在终端手动操作。</InlineStatus>}
      {error && (
        <InlineStatus tone="error" className="px-3 py-1">
          {error}
        </InlineStatus>
      )}
      <div className="terminal-inset min-h-0 flex-1 overflow-hidden p-3">
        <div ref={containerRef} data-testid="terminal-output" className="terminal-output h-full overflow-hidden" />
      </div>
    </section>
  );
}
