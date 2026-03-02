// js/tree-hash.js
// Port of bX\TreeHash — structural hashing via xxh128 (hash-wasm)

import { createXXHash128 } from 'hash-wasm';

let hasherInstance = null;

// Initialize hash-wasm xxh128 (lazy, once)
async function getHasher() {
    if (!hasherInstance) {
        hasherInstance = await createXXHash128();
    }
    return hasherInstance;
}

// Compute xxh128 hex digest of a string
async function xxh128(str) {
    const hasher = await getHasher();
    hasher.init();
    hasher.update(str);
    return hasher.digest('hex');
}

// Sync xxh128 — only works after init (for fingerprint fallback)
function xxh128Sync(str) {
    if (!hasherInstance) {
        throw new Error('TreeHash not initialized. Call await TreeHash.hash() first or await TreeHash.init()');
    }
    hasherInstance.init();
    hasherInstance.update(str);
    return hasherInstance.digest('hex');
}

function isSequential(arr) {
    if (!Array.isArray(arr)) return false;
    if (arr.length === 0) return true;
    for (let i = 0; i < arr.length; i++) {
        if (!(i in arr)) return false;
    }
    return true;
}

function recursiveKsort(data) {
    if (!Array.isArray(data) && typeof data === 'object' && data !== null) {
        const sorted = {};
        const keys = Object.keys(data).sort();
        for (const k of keys) {
            sorted[k] = recursiveKsort(data[k]);
        }
        return sorted;
    }
    if (Array.isArray(data)) {
        return data.map(item => {
            if (typeof item === 'object' && item !== null) return recursiveKsort(item);
            return item;
        });
    }
    return data;
}

function collectHashesHybrid(data, path, index, minKeys) {
    for (const [key, val] of Object.entries(data)) {
        if (val === null || typeof val !== 'object') continue;

        const childPath = path === '' ? String(key) : `${path}.${key}`;

        if (Array.isArray(val)) {
            // Traverse lists to find sub-objects
            for (let i = 0; i < val.length; i++) {
                const item = val[i];
                if (item !== null && typeof item === 'object' && !Array.isArray(item)) {
                    const itemPath = `${childPath}.[${i}]`;
                    const itemKeys = Object.keys(item);
                    if (itemKeys.length >= minKeys) {
                        const json = JSON.stringify(item);
                        const hash = xxh128Sync(json);
                        if (Object.hasOwn(index, hash)) {
                            index[hash].count++;
                        } else {
                            index[hash] = { count: 1, path: itemPath, data: item };
                        }
                    }
                    collectHashesHybrid(item, itemPath, index, minKeys);
                }
            }
        } else {
            // Associative object
            const valKeys = Object.keys(val);
            if (valKeys.length >= minKeys) {
                const json = JSON.stringify(val);
                const hash = xxh128Sync(json);
                if (Object.hasOwn(index, hash)) {
                    index[hash].count++;
                } else {
                    index[hash] = { count: 1, path: childPath, data: val };
                }
            }
            collectHashesHybrid(val, childPath, index, minKeys);
        }
    }
}

// --- Fingerprint internals (sync, for equals/diff) ---

function primitiveFP(val) {
    if (val === null) return '\x00';
    if (val === true) return '\x01\x01';
    if (val === false) return '\x01\x00';
    if (typeof val === 'number') {
        if (Number.isInteger(val)) return '\x02' + String(val);
        return '\x03' + String(val);
    }
    if (typeof val === 'string') return '\x04' + val;
    return '\x04' + String(val);
}

function fingerprint(data) {
    if (data === null || typeof data !== 'object') {
        return primitiveFP(data);
    }
    if (Array.isArray(data)) {
        if (data.length === 0) return xxh128Sync('A:0');
        let buf = 'A:' + data.length;
        for (const item of data) {
            buf += '|' + fingerprint(item);
        }
        return xxh128Sync(buf);
    }
    // Object
    const keys = Object.keys(data).sort();
    if (keys.length === 0) return xxh128Sync('A:0');
    let buf = 'O:' + keys.length;
    for (const key of keys) {
        buf += '|' + key + ':' + fingerprint(data[key]);
    }
    return xxh128Sync(buf);
}

export class TreeHash {

    // Initialize hash-wasm (call once, or auto-inits on first use)
    static async init() {
        await getHasher();
    }

    // Hash any value → string hex 32 chars (xxh128) — async
    static async hash(data) {
        await getHasher(); // ensure init
        const fp = fingerprint(data);
        if (fp.length === 32 && /^[0-9a-f]+$/.test(fp)) return fp;
        return xxh128Sync(fp);
    }

    // Hash a tree with frequency index for dedup — async
    // normalize=true: ksort keys for cross-source dedup
    static async hashTree(data, basePath = '', minKeys = 2, normalize = true) {
        await getHasher(); // ensure init

        if (normalize) {
            data = recursiveKsort(data);
        }

        const index = {};
        collectHashesHybrid(data, basePath, index, minKeys);

        const json = JSON.stringify(data);
        const rootHash = await xxh128(json);

        return { root_hash: rootHash, index };
    }

    // Compare two structures: are they identical? — async (needs init)
    static async equals(a, b) {
        if (a === b) return true;
        await getHasher();
        return fingerprint(a) === fingerprint(b);
    }

    // Structural diff: returns paths where they differ — async (needs init)
    static async diff(a, b, path = '') {
        await getHasher();
        return TreeHash._diffSync(a, b, path);
    }

    static _diffSync(a, b, path) {
        if (a === b) return [];
        if (fingerprint(a) === fingerprint(b)) return [];

        if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null || Array.isArray(a) || Array.isArray(b)) {
            return [{ path, type: 'changed', old: a, new: b }];
        }

        const diffs = [];
        const allKeys = [...new Set([...Object.keys(a), ...Object.keys(b)])];

        for (const key of allKeys) {
            const currentPath = path === '' ? String(key) : `${path}.${key}`;

            if (!(key in a)) {
                diffs.push({ path: currentPath, type: 'added', value: b[key] });
                continue;
            }
            if (!(key in b)) {
                diffs.push({ path: currentPath, type: 'removed', value: a[key] });
                continue;
            }

            const valA = a[key];
            const valB = b[key];

            if (valA === valB) continue;
            if (fingerprint(valA) === fingerprint(valB)) continue;

            if (
                typeof valA === 'object' && typeof valB === 'object' &&
                valA !== null && valB !== null &&
                !Array.isArray(valA) && !Array.isArray(valB)
            ) {
                diffs.push(...TreeHash._diffSync(valA, valB, currentPath));
            } else {
                diffs.push({ path: currentPath, type: 'changed', old: valA, new: valB });
            }
        }

        return diffs;
    }
}
