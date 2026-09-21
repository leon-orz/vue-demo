import { invoke } from '@tauri-apps/api/core'

/** 与 src-tauri/src/commands/keyring.rs 一一对应 */
export function keystoreSet(key: string, value: string): Promise<void> {
  return invoke('keystore_set', { key, value })
}

export function keystoreGet(key: string): Promise<string | null> {
  return invoke('keystore_get', { key })
}

export function keystoreDelete(key: string): Promise<void> {
  return invoke('keystore_delete', { key })
}
