#!/usr/bin/env node
// Unified entry point for the Code CLI (fork of OpenAI Codex).

import path from "path";
import { fileURLToPath } from "url";
import { platform as nodePlatform, arch as nodeArch } from "os";
import { execSync } from "child_process";

// __dirname equivalent in ESM
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const { platform, arch } = process;

// Important: Never delegate to another system's `code` binary (e.g., VS Code).
// When users run via `npx @just-every/code`, we must always execute our
// packaged native binary by absolute path to avoid PATH collisions.

let targetTriple = null;
switch (platform) {
  case "linux":
  case "android":
    switch (arch) {
      case "x64":
        targetTriple = "x86_64-unknown-linux-musl";
        break;
      case "arm64":
        targetTriple = "aarch64-unknown-linux-musl";
        break;
      default:
        break;
    }
    break;
  case "darwin":
    switch (arch) {
      case "x64":
        targetTriple = "x86_64-apple-darwin";
        break;
      case "arm64":
        targetTriple = "aarch64-apple-darwin";
        break;
      default:
        break;
    }
    break;
  case "win32":
    switch (arch) {
      case "x64":
        targetTriple = "x86_64-pc-windows-msvc.exe";
        break;
      case "arm64":
        // We do not build this today, fall through...
      default:
        break;
    }
    break;
  default:
    break;
}

if (!targetTriple) {
  throw new Error(`Unsupported platform: ${platform} (${arch})`);
}

// Prefer new 'code-*' binary names; fall back to legacy 'coder-*' if missing.
let binaryPath = path.join(__dirname, "..", "bin", `code-${targetTriple}`);
let legacyBinaryPath = path.join(__dirname, "..", "bin", `coder-${targetTriple}`);

// --- Bootstrap helper (runs if the binary is missing, e.g. Bun blocked postinstall) ---
import { existsSync, chmodSync, statSync, openSync, readSync, closeSync, mkdirSync, readFileSync } from "fs";

const validateBinary = (p) => {
  try {
    const st = statSync(p);
    if (!st.isFile() || st.size === 0) {
      return { ok: false, reason: "empty or not a regular file" };
    }
    const fd = openSync(p, "r");
    try {
      const buf = Buffer.alloc(4);
      const n = readSync(fd, buf, 0, 4, 0);
      if (n < 2) return { ok: false, reason: "too short" };
      if (platform === "win32") {
        if (!(buf[0] === 0x4d && buf[1] === 0x5a)) return { ok: false, reason: "invalid PE header (missing MZ)" };
      } else if (platform === "linux" || platform === "android") {
        if (!(buf[0] === 0x7f && buf[1] === 0x45 && buf[2] === 0x4c && buf[3] === 0x46)) return { ok: false, reason: "invalid ELF header" };
      } else if (platform === "darwin") {
        const isMachO = (buf[0] === 0xcf && buf[1] === 0xfa && buf[2] === 0xed && buf[3] === 0xfe) ||
                        (buf[0] === 0xca && buf[1] === 0xfe && buf[2] === 0xba && buf[3] === 0xbe);
        if (!isMachO) return { ok: false, reason: "invalid Mach-O header" };
      }
    } finally {
      closeSync(fd);
    }
    return { ok: true };
  } catch (e) {
    return { ok: false, reason: e.message };
  }
};

const getCacheDir = (version) => {
  const plt = nodePlatform();
  const home = process.env.HOME || process.env.USERPROFILE || "";
  let base = "";
  if (plt === "win32") {
    base = process.env.LOCALAPPDATA || path.join(home, "AppData", "Local");
  } else if (plt === "darwin") {
    base = path.join(home, "Library", "Caches");
  } else {
    base = process.env.XDG_CACHE_HOME || path.join(home, ".cache");
  }
  const dir = path.join(base, "just-every", "code", version);
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
  return dir;
};

const getCachedBinaryPath = (version) => {
  // targetTriple already includes the proper extension on Windows ("...msvc.exe").
  // Do not append another suffix; just use the exact targetTriple-derived name.
  const cacheDir = getCacheDir(version);
  return path.join(cacheDir, `code-${targetTriple}`);
};

// Binary must be built locally via build-fast.sh — automatic download is disabled.
let binaryReady = existsSync(binaryPath) || existsSync(legacyBinaryPath);
if (!binaryReady) {
  console.error(`Native binary not found. Run ./build-fast.sh to build it locally.`);
  console.error(`Expected at: ${binaryPath} or ${legacyBinaryPath}`);
  process.exit(1);
}

// Prefer cached binary when available
try {
  const pkg = JSON.parse(readFileSync(path.join(__dirname, "..", "package.json"), "utf8"));
  const version = pkg.version;
  const cached = getCachedBinaryPath(version);
  const v = existsSync(cached) ? validateBinary(cached) : { ok: false };
  if (v.ok) {
    binaryPath = cached;
  } else if (!existsSync(binaryPath) && existsSync(legacyBinaryPath)) {
    binaryPath = legacyBinaryPath;
  }
} catch {
  // ignore
}

// Check if binary exists and try to fix permissions if needed
// fs imports are above; keep for readability if tree-shaken by bundlers
import { spawnSync } from "child_process";
if (existsSync(binaryPath)) {
  try {
    // Ensure binary is executable on Unix-like systems
    if (platform !== "win32") {
      chmodSync(binaryPath, 0o755);
    }
  } catch (e) {
    // Ignore permission errors, will be caught below if it's a real problem
  }
} else {
  console.error(`Binary not found: ${binaryPath}`);
  console.error(`Run ./build-fast.sh to build it locally.`);
  process.exit(1);
}

// Lightweight header validation to provide clearer errors before spawn
// Reuse the validateBinary helper defined above in the bootstrap section.

const validation = validateBinary(binaryPath);
if (!validation.ok) {
  console.error(`The native binary at ${binaryPath} appears invalid: ${validation.reason}`);
  console.error("Run ./build-fast.sh to rebuild.");
  process.exit(1);
}

// If running under npx/npm, emit a concise notice about which binary path is used
try {
  const ua = process.env.npm_config_user_agent || "";
  const isNpx = ua.includes("npx");
  if (isNpx && process.stderr && process.stderr.isTTY) {
    // Best-effort discovery of another 'code' on PATH for user clarity
    let otherCode = "";
    try {
      const cmd = process.platform === "win32" ? "where code" : "command -v code || which code || true";
      const out = spawnSync(process.platform === "win32" ? "cmd" : "bash", [
        process.platform === "win32" ? "/c" : "-lc",
        cmd,
      ], { encoding: "utf8" });
      const line = (out.stdout || "").split(/\r?\n/).map((s) => s.trim()).filter(Boolean)[0];
      if (line && !line.includes("@just-every/code")) {
        otherCode = line;
      }
    } catch {}
    if (otherCode) {
      console.error(`@just-every/code: running bundled binary -> ${binaryPath}`);
      console.error(`Note: a different 'code' exists at ${otherCode}; not delegating.`);
    } else {
      console.error(`@just-every/code: running bundled binary -> ${binaryPath}`);
    }
  }
} catch {}

// Use an asynchronous spawn instead of spawnSync so that Node is able to
// respond to signals (e.g. Ctrl-C / SIGINT) while the native binary is
// executing. This allows us to forward those signals to the child process
// and guarantees that when either the child terminates or the parent
// receives a fatal signal, both processes exit in a predictable manner.
const { spawn } = await import("child_process");

// Make the resolved native binary path visible to spawned agents/subprocesses.
process.env.CODE_BINARY_PATH = binaryPath;

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  env: { ...process.env, CODER_MANAGED_BY_NPM: "1", CODEX_MANAGED_BY_NPM: "1", CODE_BINARY_PATH: binaryPath },
});

child.on("error", (err) => {
  // Typically triggered when the binary is missing or not executable.
  const code = err && err.code;
  if (code === 'EACCES') {
    console.error(`Permission denied: ${binaryPath}`);
    console.error(`Try running: chmod +x "${binaryPath}"`);
  } else if (code === 'EFTYPE' || code === 'ENOEXEC') {
    console.error(`Failed to execute native binary: ${binaryPath}`);
    console.error("The file may be corrupt or of the wrong type.");
    console.error("Run ./build-fast.sh to rebuild.");
  } else {
    console.error(err);
  }
  process.exit(1);
});

// Forward common termination signals to the child so that it shuts down
// gracefully. In the handler we temporarily disable the default behavior of
// exiting immediately; once the child has been signaled we simply wait for
// its exit event which will in turn terminate the parent (see below).
const forwardSignal = (signal) => {
  if (child.killed) {
    return;
  }
  try {
    child.kill(signal);
  } catch {
    /* ignore */
  }
};

["SIGINT", "SIGTERM", "SIGHUP"].forEach((sig) => {
  process.on(sig, () => forwardSignal(sig));
});

// When the child exits, mirror its termination reason in the parent so that
// shell scripts and other tooling observe the correct exit status.
// Wrap the lifetime of the child process in a Promise so that we can await
// its termination in a structured way. The Promise resolves with an object
// describing how the child exited: either via exit code or due to a signal.
const childResult = await new Promise((resolve) => {
  child.on("exit", (code, signal) => {
    if (signal) {
      resolve({ type: "signal", signal });
    } else {
      resolve({ type: "code", exitCode: code ?? 1 });
    }
  });
});

if (childResult.type === "signal") {
  // Re-emit the same signal so that the parent terminates with the expected
  // semantics (this also sets the correct exit code of 128 + n).
  process.kill(process.pid, childResult.signal);
} else {
  process.exit(childResult.exitCode);
}
