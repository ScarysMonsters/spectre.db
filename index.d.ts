declare module '@sexfy/spectre.db' {
  export interface SpectreOptions {
    compress?: boolean;
    compression?: 'none' | 'gzip' | 'zstd';
    encryptionKey?: string;
    encryptBackups?: boolean;
    encryptSnapshot?: boolean;
    scryptLogN?: number;
    scryptR?: number;
    scryptP?: number;
    backupCount?: number;
    format?: 'auto' | 'v2' | 'json';
    lockTimeout?: number;
    walBuffering?: boolean;

    syncWal?: boolean;
    durability?: 'process' | 'durable';
    compactMode?: 'auto' | 'legacy' | 'segments';
    legacyThreshold?: number;
    segmentMergeEvery?: number;
    compactThreshold?: number;
    compactInterval?: number;
    warmKeys?: string[];
  }

  export interface Entry<T = any> { ID: string; data: T }

  export interface IndexHandle {
    path: string;
    get<T = any>(value: any): T[];
    keys(value: any): string[];
    range(opts?: { gte?: any; lt?: any; limit?: number }): string[];
    drop(): boolean;
  }

  export interface StatsShape {
    driver: string;
    engine: string;
    format: 'v2' | 'json';
    durability: 'process' | 'durable';
    compression: 'none' | 'gzip' | 'zstd';
    encrypted: boolean;
    snapshotEncrypted: boolean;
    entries: number;
    rawEntries: number;
    storeBytes: number;
    fileSize: number;
    walOps: number;
    walBytes: number;
    pendingWrites: number;
    cacheHits: number;
    cacheMisses: number;
    compressionRatio: number;
    segmentCount: number;
    generation: number;
    lastCompactionMs: number | null;
    lastLsn: number;
    recoveryCount: number;
    indexCount: number;
  }

  export type TransactionOperation =
    | { type: 'set'; key: string; value: any }
    | { type: 'delete'; key: string };

  export interface TransactionContext {
    get(key: string): any;
    set(key: string, value: any): any;
    delete(key: string): boolean;
    add(key: string, n: number): number;
    sub(key: string, n: number): number;
    push(key: string, value: any): number;
    pull(key: string, pred?: any): boolean;
  }

  export class Database {
    constructor(filePath?: string, options?: SpectreOptions);
    get ready(): Promise<Database>;
    get(key: string): any;
    set(key: string, value: any): any;
    has(key: string): boolean;
    delete(key: string): boolean;
    add(key: string, n: number): number;
    sub(key: string, n: number): number;
    push(key: string, value: any): number;
    pull(key: string, predicate?: any): boolean;
    all(prefix?: string): Entry[];
    filter(predicate: (data: any, ID: string) => boolean): Entry[];
    find(predicate: ((data: any, ID: string) => boolean) | Record<string, any>): Entry | null;
    startsWith(prefix: string): Entry[];
    count(prefix?: string): number;
    paginate(prefix?: string, page?: number, limit?: number, sortBy?: string, sortDesc?: boolean): {
      data: Entry[];
      pagination: { page: number; limit: number; total: number; pages: number; hasNext: boolean; hasPrev: boolean };
    };
    scan(opts?: { prefix?: string; after?: string; pageSize?: number; limit?: number }): AsyncIterableIterator<Entry>;
    iterate(opts?: { prefix?: string; after?: string; pageSize?: number; limit?: number }): AsyncIterableIterator<Entry>;
    cursor(prefix?: string, opts?: { after?: string; limit?: number }): { rows: Entry[]; cursor: string | null; done: boolean };
    setRaw(key: string, data: Uint8Array | Buffer): Uint8Array | Buffer;
    getRaw(key: string): Buffer | null;
    deleteRaw(key: string): boolean;
    index(path: string): IndexHandle;
    listIndexes(): string[];
    range(path: string, opts?: { gte?: any; lt?: any; limit?: number }): Entry[];
    transaction(ops: TransactionOperation[]): Promise<any[]>;
    transaction(fn: (tx: TransactionContext) => any): Promise<any>;
    compact(): Promise<void>;
    compactAsync(): Promise<'committed' | 'stale' | 'legacy'>;
    save(): Promise<void>;
    clear(): Promise<Database>;
    close(): Promise<void>;
    migrate(format: 'v2' | 'json'): Promise<Database>;
    getStats(): {
      driver: string; engine: string; format: string; compress: boolean; encrypted: boolean;
      entries: number; cacheSize: number; maxCacheSize: number; fileSize: number; storeBytes: number;
      walOps: number; walBytes: number; compactThreshold: number; shards: number;
      snapshotPath: string; walPath: string;
    };
    stats(): StatsShape;
    table(name: string): Table;
    on(event: string, listener: (...args: any[]) => void): this;
    once(event: string, listener: (...args: any[]) => void): this;
    off(event: string, listener: (...args: any[]) => void): this;
  }

  export class Table {
    constructor(db: Database, name: string);
    get(key: string): any;
    set(key: string, value: any): any;
    has(key: string): boolean;
    delete(key: string): boolean;
    add(key: string, n: number): number;
    sub(key: string, n: number): number;
    push(key: string, value: any): number;
    pull(key: string, pred?: any): boolean;
    all(): Entry[];
    count(): number;
    clear(): Promise<void>;
    transaction(fn: (tx: TransactionContext) => any): Promise<any>;
  }

  export const hasNativeEngine: boolean;
  export const version: string;
}
