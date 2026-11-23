// The detached window label identifies its project or conversation across launches.
export function windowPreferenceKey(key: string): string {
  const label = window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow().label;
  return label?.startsWith("project-") || label?.startsWith("chat-") ? `${key}:${label}` : key;
}

export function readWindowPreference(key: string): string | null {
  const scoped = windowPreferenceKey(key);
  return localStorage.getItem(scoped) ?? (scoped === key ? null : localStorage.getItem(key));
}

export function writeWindowPreference(key: string, value: string) {
  localStorage.setItem(windowPreferenceKey(key), value);
}
