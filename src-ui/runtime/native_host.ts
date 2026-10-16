/** Host selection is independent of browser GPU capability and saved settings. */
export function isNativeHost(): boolean {
  return !!(window as any).__TAURI__?.core;
}
