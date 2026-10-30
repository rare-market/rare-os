/** Tiny async key-value port. */
export interface KV {
  get<T>(key: string): Promise<T | undefined>
  set<T>(key: string, value: T): Promise<void>
  remove(key: string): Promise<void>
}

/** Memory only: the unlock key for this session lives here and is never written to disk. */
export class MemoryKV implements KV {
  private m = new Map<string, string>()
  async get<T>(key: string): Promise<T | undefined> {
    const v = this.m.get(key)
    return v === undefined ? undefined : (JSON.parse(v) as T)
  }
  async set<T>(key: string, value: T): Promise<void> {
    this.m.set(key, JSON.stringify(value))
  }
  async remove(key: string): Promise<void> {
    this.m.delete(key)
  }
}

/** Persistent storage in the launcher's webview profile: the encrypted vault and public account data only. */
export class LocalKV implements KV {
  constructor(private prefix = '') {}
  async get<T>(key: string): Promise<T | undefined> {
    try {
      const v = localStorage.getItem(this.prefix + key)
      return v === null ? undefined : (JSON.parse(v) as T)
    } catch {
      return undefined
    }
  }
  async set<T>(key: string, value: T): Promise<void> {
    localStorage.setItem(this.prefix + key, JSON.stringify(value))
  }
  async remove(key: string): Promise<void> {
    localStorage.removeItem(this.prefix + key)
  }
}
