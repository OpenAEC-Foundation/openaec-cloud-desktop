import { invoke } from "@tauri-apps/api/core"

export interface RemoteEntry {
  name: string
  path: string
  is_dir: boolean
  size: number
  etag: string
  last_modified: string
}

export interface SyncReport {
  uploaded: number
  downloaded: number
  skipped: number
  errors: string[]
}

// Bindings naar de Rust-commands (src-tauri/src/lib.rs).
export const api = {
  connect: (base: string, user: string, pass: string) =>
    invoke<RemoteEntry[]>("connect", { base, user, pass }),
  listRemote: (path: string) => invoke<RemoteEntry[]>("list_remote", { path }),
  createLink: (path: string) => invoke<string>("create_link", { path }),
  syncFolder: (local: string, remote: string) =>
    invoke<SyncReport>("sync_folder", { local, remote }),
  enableCloudFolder: (local: string, remote: string) =>
    invoke<number>("enable_cloud_folder", { local, remote }),
}
