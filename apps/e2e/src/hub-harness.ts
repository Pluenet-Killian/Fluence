// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Spawns and supervises a real assembled hub for the T5 suite (PLAN §1 T5).
 *
 * The hub is the actual compiled binary (`fluence-hub`), configured headless via
 * `FLUENCE_*` env to a throwaway data dir, a fixed loopback port, and the built
 * web composer as its same-origin PWA (`FLUENCE_WEB_DIR`). No LLM and no Piper
 * are configured: suggestions degrade to the always-on n-gram fallback (D-2.6)
 * and the voice degrades to the OS voice ("une voix, toujours", SPEC §2.C) — so
 * the suite is hermetic, with no model downloads.
 *
 * A `control` token is minted through the real pairing flow (system token →
 * `POST /pair/window` → `POST /pair`), exactly as a device would. `kill`/`start`
 * model a crash and recovery on the *same* port so the composer can reconnect.
 */

import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
// apps/e2e/src → repo root.
const REPO_ROOT = join(HERE, "..", "..", "..");
const WEB_DIR = join(REPO_ROOT, "apps", "web-client", "dist");

/** A paired-device scope the harness can mint a token for. */
export type Scope = "control" | "display" | "care";

/** A running (and restartable) hub under test. */
export interface HubHandle {
  /** Same-origin base the composer talks to (`http://127.0.0.1:<port>`). */
  readonly origin: string;
  /** Bound loopback port (stable across `restart`). */
  readonly port: number;
  /** Throwaway data dir (store, tokens) — survives `restart`, removed on `stop`. */
  readonly dataDir: string;
  /** Mints a device token through the real pairing flow. */
  pairToken(scope: Scope): Promise<string>;
  /** Kills the hub process (SIGKILL) — models a crash. */
  kill(): Promise<void>;
  /** (Re)starts the hub on the same port and data dir. */
  start(): Promise<void>;
  /** Kill then start on the same port — a crash and recovery. */
  restart(): Promise<void>;
  /** Kills the hub and removes the data dir. */
  stop(): Promise<void>;
}

/** Resolves the hub binary: `FLUENCE_HUB_BIN`, else release, else debug. */
function hubBinary(): string {
  const fromEnv = process.env["FLUENCE_HUB_BIN"];
  if (fromEnv !== undefined && existsSync(fromEnv)) {
    return fromEnv;
  }
  const exe = process.platform === "win32" ? ".exe" : "";
  for (const profile of ["release", "debug"]) {
    const candidate = join(REPO_ROOT, "target", profile, `fluence-hub${exe}`);
    if (existsSync(candidate)) {
      return candidate;
    }
  }
  throw new Error(
    "fluence-hub binary not found — build it (`cargo build -p fluence-hub`) or set FLUENCE_HUB_BIN",
  );
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

/** Asks the OS for a free loopback port, then releases it for the hub to bind. */
function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (address === null || typeof address === "string") {
        server.close();
        reject(new Error("could not obtain a free port"));
        return;
      }
      const { port } = address;
      server.close(() => {
        resolve(port);
      });
    });
  });
}

async function fetchJson<T>(url: string, init: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  if (!response.ok) {
    throw new Error(`${init.method ?? "GET"} ${url} → ${String(response.status)}`);
  }
  return (await response.json()) as T;
}

/** Mints a device token via the real pairing flow (system token → window → pair). */
async function pairToken(origin: string, dataDir: string, scope: Scope): Promise<string> {
  const systemToken = readFileSync(join(dataDir, "system.token"), "utf8").trim();
  const window = await fetchJson<{ code: string }>(`${origin}/api/v1/pair/window`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-fluence-token": systemToken },
    body: JSON.stringify({ scope }),
  });
  const paired = await fetchJson<{ device_token: string }>(`${origin}/pair`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ code: window.code, device_name: `e2e-${scope}`, device_kind: "cli" }),
  });
  return paired.device_token;
}

/** Waits until the hub bound the expected port and answers `/pair/info`. */
async function waitForReady(
  origin: string,
  port: number,
  dataDir: string,
  logs: () => string,
): Promise<void> {
  const deadline = Date.now() + 25_000;
  const portFile = join(dataDir, "hub.port");
  while (Date.now() < deadline) {
    if (existsSync(portFile)) {
      const written = readFileSync(portFile, "utf8").trim();
      if (written.length > 0 && written !== String(port)) {
        throw new Error(
          `hub bound port ${written}, expected ${String(port)} — the fixed port was taken ` +
            `(SO_REUSEADDR / lingering socket?). Logs:\n${logs()}`,
        );
      }
      if (written === String(port)) {
        try {
          const info = await fetch(`${origin}/pair/info`);
          if (info.ok) {
            return;
          }
        } catch {
          // Not accepting connections yet — keep polling.
        }
      }
    }
    await sleep(100);
  }
  throw new Error(`hub did not become ready within 25s. Logs:\n${logs()}`);
}

async function killChild(child: ChildProcess | null): Promise<void> {
  if (child === null) {
    return;
  }
  if (child.exitCode !== null || child.signalCode !== null) {
    return; // already exited
  }
  const exited = new Promise<void>((resolve) => {
    child.once("exit", () => {
      resolve();
    });
  });
  child.kill("SIGKILL");
  await Promise.race([exited, sleep(5_000)]);
}

/** Spawns an assembled hub serving the built composer, and returns a handle. */
export async function startHub(): Promise<HubHandle> {
  const bin = hubBinary();
  if (!existsSync(WEB_DIR)) {
    throw new Error(
      `web composer not built at ${WEB_DIR} — run \`pnpm --filter @fluence/web-client build\``,
    );
  }
  const port = await freePort();
  const dataDir = mkdtempSync(join(tmpdir(), "fluence-e2e-"));
  const origin = `http://127.0.0.1:${String(port)}`;
  let child: ChildProcess | null = null;
  let logs = "";

  const spawnHub = async (): Promise<void> => {
    const proc = spawn(bin, [], {
      env: {
        ...process.env,
        FLUENCE_LISTEN_ADDR: "127.0.0.1",
        FLUENCE_PORT: String(port),
        FLUENCE_DATA_DIR: dataDir,
        FLUENCE_STORE_KEY_FILE: join(dataDir, "store.key"),
        FLUENCE_WEB_DIR: WEB_DIR,
        FLUENCE_HOUSEHOLD_NAME: "Fluence E2E",
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    // stdio is ["ignore", "pipe", "pipe"], so stdout/stderr are non-null here.
    proc.stdout.on("data", (chunk: Buffer) => {
      logs += chunk.toString();
    });
    proc.stderr.on("data", (chunk: Buffer) => {
      logs += chunk.toString();
    });
    child = proc;
    await waitForReady(origin, port, dataDir, () => logs);
  };

  await spawnHub();

  return {
    origin,
    port,
    dataDir,
    pairToken: (scope) => pairToken(origin, dataDir, scope),
    kill: async () => {
      await killChild(child);
      child = null;
    },
    start: async () => {
      // A brief wait lets the OS fully release the listening port before rebind.
      await sleep(300);
      await spawnHub();
    },
    restart: async () => {
      await killChild(child);
      child = null;
      await sleep(300);
      await spawnHub();
    },
    stop: async () => {
      await killChild(child);
      child = null;
      try {
        rmSync(dataDir, { recursive: true, force: true });
      } catch {
        // Best effort: a throwaway temp dir.
      }
    },
  };
}
