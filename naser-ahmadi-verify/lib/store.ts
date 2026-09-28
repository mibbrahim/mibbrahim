import { retentionSeconds } from "./config.ts";
import type { Submission } from "./types.ts";

// Stores submissions in Upstash Redis / Vercel KV over its REST API. Without
// those env vars it falls back to process memory, which only works for local
// development (serverless instances on Vercel don't share memory).

const url = process.env.KV_REST_API_URL || process.env.UPSTASH_REDIS_REST_URL;
const token = process.env.KV_REST_API_TOKEN || process.env.UPSTASH_REDIS_REST_TOKEN;

export const hasDurableStore = Boolean(url && token);

const INDEX = "subs";
const memory = new Map<string, string>();
const memoryIndex: string[] = [];

async function redis(commands: (string | number)[][]): Promise<unknown[]> {
  const res = await fetch(`${url}/pipeline`, {
    method: "POST",
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: JSON.stringify(commands),
    cache: "no-store",
  });
  if (!res.ok) throw new Error(`Storage error ${res.status}`);
  const out = (await res.json()) as { result?: unknown; error?: string }[];
  const err = out.find((r) => r.error);
  if (err) throw new Error(`Storage error: ${err.error}`);
  return out.map((r) => r.result);
}

export async function saveSubmission(sub: Submission, images: Record<string, string>): Promise<void> {
  const ttl = retentionSeconds();
  if (!hasDurableStore) {
    memory.set(`sub:${sub.code}`, JSON.stringify(sub));
    for (const [k, v] of Object.entries(images)) memory.set(`img:${sub.code}:${k}`, v);
    memoryIndex.unshift(sub.code);
    return;
  }
  const cmds: (string | number)[][] = [["SET", `sub:${sub.code}`, JSON.stringify(sub), "EX", ttl]];
  for (const [k, v] of Object.entries(images)) cmds.push(["SET", `img:${sub.code}:${k}`, v, "EX", ttl]);
  cmds.push(["LPUSH", INDEX, sub.code], ["LTRIM", INDEX, 0, 499]);
  await redis(cmds);
}

export async function getSubmission(code: string): Promise<Submission | null> {
  const raw = hasDurableStore ? (await redis([["GET", `sub:${code}`]]))[0] : memory.get(`sub:${code}`);
  return typeof raw === "string" ? (JSON.parse(raw) as Submission) : null;
}

export async function getImage(code: string, key: string): Promise<string | null> {
  const raw = hasDurableStore ? (await redis([["GET", `img:${code}:${key}`]]))[0] : memory.get(`img:${code}:${key}`);
  return typeof raw === "string" ? raw : null;
}

export async function listSubmissions(limit = 50): Promise<Submission[]> {
  const codes = hasDurableStore
    ? ((await redis([["LRANGE", INDEX, 0, limit - 1]]))[0] as string[])
    : memoryIndex.slice(0, limit);
  if (!codes.length) return [];
  const raws = hasDurableStore
    ? await redis(codes.map((c) => ["GET", `sub:${c}`]))
    : codes.map((c) => memory.get(`sub:${c}`));
  // Expired entries come back null; skip them.
  return raws.filter((r): r is string => typeof r === "string").map((r) => JSON.parse(r) as Submission);
}
