import type { GraphicsQuality } from "./hostBridge";

// Saved in the webview's own storage, so they last between launches. Storage can be unavailable, and then nothing is remembered.

export const readSavedSetting = (key: string) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};

export const saveSetting = (key: string, value: string) => {
  try {
    localStorage.setItem(key, value);
  } catch {}
};

export const readSavedVolume = () => {
  const volume = Number(readSavedSetting("volume") ?? Number.NaN);
  return Number.isFinite(volume) ? Math.min(Math.max(volume, 0), 1) : 1;
};

export const readSavedGraphicsQuality = (): GraphicsQuality => (readSavedSetting("graphicsQuality") === "high" ? "high" : "low");
