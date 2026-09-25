/** Authorize only xterm's generated styles with Tauri's per-document CSP nonce.
 * No global DOM patch, static nonce or unsafe-inline policy is needed. */
export function terminalDocument(): Document {
  const nonce = document.querySelector<HTMLMetaElement>('meta[name="fleqi-style-nonce"]')?.content;
  if (!nonce || nonce === "__TAURI_STYLE_NONCE__") return document;
  return new Proxy(document, {
    get(target, property) {
      if (property === "createElement") {
        return (name: string, options?: ElementCreationOptions) => {
          const element = target.createElement(name, options);
          if (element instanceof HTMLStyleElement) element.nonce = nonce;
          return element;
        };
      }
      const value: unknown = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
}

/** xterm 6's viewport uses window.document despite documentOverride. Reapply
 * that one stylesheet with the same nonce synchronously, before the first fit
 * or paint. The original node stays intact for subsequent xterm theme updates. */
export function authorizeTerminalStyles(container: HTMLElement): void {
  const nonce = document.querySelector<HTMLMetaElement>('meta[name="fleqi-style-nonce"]')?.content;
  if (!nonce || nonce === "__TAURI_STYLE_NONCE__") return;
  for (const style of container.querySelectorAll<HTMLStyleElement>("style")) {
    if (style.nonce === nonce) continue;
    style.nonce = nonce;
    style.textContent = style.textContent;
  }
}
