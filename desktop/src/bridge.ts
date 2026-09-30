import { invoke, isTauri } from '@tauri-apps/api/core';
export const native = isTauri();
export function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (!native) return Promise.reject(new Error('Native runtime unavailable. Open the Network Lantern desktop application to use this operation.'));
  return invoke<T>(name, args);
}
