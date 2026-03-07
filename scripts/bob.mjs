#!/usr/bin/env node
import { spawn } from "node:child_process";
import { access, readFile, readdir, rm } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const ROOT_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const WEB_DIR = path.join(ROOT_DIR, "web");
const ENV_PATH = path.join(ROOT_DIR, ".env");
const isWindows = process.platform === "win32";

function executable(name) {
  if (!isWindows) {
    return name;
  }
  if (name === "npm" || name === "npx") {
    return `${name}.cmd`;
  }
  return `${name}.exe`;
}

function run(cmd, args = [], options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(executable(cmd), args, {
      cwd: ROOT_DIR,
      stdio: "inherit",
      ...options,
    });

    child.on("error", (err) => reject(err));
    child.on("close", (code, signal) => {
      if (signal) {
        reject(new Error(`${cmd} terminated by signal ${signal}`));
        return;
      }
      if (code !== 0) {
        reject(new Error(`${cmd} ${args.join(" ")} exited with code ${code}`));
        return;
      }
      resolve();
    });
  });
}

function runQuiet(cmd, args = [], options = {}) {
  return new Promise((resolve) => {
    const child = spawn(executable(cmd), args, {
      cwd: ROOT_DIR,
      stdio: "ignore",
      ...options,
    });
    child.on("error", () => resolve(false));
    child.on("close", (code) => resolve(code === 0));
  });
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function ensureEnvExists() {
  try {
    await access(ENV_PATH);
  } catch {
    throw new Error("Missing .env file. Create it from .env.example and set required values.");
  }
}

async function parseEnvFile() {
  const raw = await readFile(ENV_PATH, "utf8");
  const result = {};
  for (const line of raw.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) {
      continue;
    }
    const idx = line.indexOf("=");
    if (idx < 0) {
      continue;
    }
    const key = line.slice(0, idx).trim();
    const value = line.slice(idx + 1).trim();
    result[key] = value;
  }
  return result;
}

async function maybeStartPodmanSocketLinux() {
  if (isWindows) {
    return;
  }

  const hasPodman = await runQuiet("podman", ["--version"]);
  const hasSystemctl = await runQuiet("systemctl", ["--version"]);
  if (!hasPodman || !hasSystemctl) {
    return;
  }

  const active = await runQuiet("systemctl", ["--user", "is-active", "podman.socket"]);
  if (active) {
    return;
  }

  console.log("Starting podman socket...");
  const started = await runQuiet("systemctl", ["--user", "start", "podman.socket"]);
  if (!started) {
    console.log("Warning: could not start podman.socket via systemctl --user; continuing.");
  }
}

async function request(url, options = {}) {
  try {
    return await fetch(url, options);
  } catch {
    return null;
  }
}

async function waitForHealth() {
  console.log("Waiting for API health endpoint...");
  for (let i = 0; i < 45; i += 1) {
    const res = await request("http://127.0.0.1:8787/healthz");
    if (res?.ok) {
      return true;
    }
    await sleep(2000);
  }
  return false;
}

async function validateAuth(token) {
  console.log("Validating auth protection (expecting 401 without token)...");
  const unauth = await request("http://127.0.0.1:8787/config");
  if (!unauth || unauth.status !== 401) {
    throw new Error(`Expected 401 from /config without token, got: ${unauth ? unauth.status : "request failed"}`);
  }

  console.log("Validating authenticated config access...");
  const auth = await request("http://127.0.0.1:8787/config", {
    headers: { "x-api-key": token },
  });
  if (!auth || auth.status !== 200) {
    throw new Error(`Expected 200 from /config with token, got: ${auth ? auth.status : "request failed"}`);
  }
}

async function removeTargetDirs(root) {
  const entries = await readdir(root, { withFileTypes: true });
  for (const entry of entries) {
    const full = path.join(root, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === ".git" || entry.name === "node_modules") {
        continue;
      }
      if (entry.name === "target") {
        await rm(full, { recursive: true, force: true });
      } else {
        await removeTargetDirs(full);
      }
    }
  }
}

async function cmdStart() {
  await ensureEnvExists();
  await maybeStartPodmanSocketLinux();

  console.log("Starting BoB services...");
  await run("docker", ["compose", "up", "--build", "-d"]);

  console.log("Running startup validation...");
  await cmdValidate();

  console.log("\nBoB API ready at http://127.0.0.1:8787");
  console.log("Stop with: node ./scripts/bob.mjs stop");
}

async function cmdStop(includeVolumes = false) {
  const args = ["compose", "down", "--remove-orphans"];
  if (includeVolumes) {
    args.push("--volumes");
  }
  console.log("Stopping BoB services...");
  await run("docker", args);
}

async function cmdBuild() {
  console.log("Installing/updating JS dependencies...");
  await run("npm", ["install"]);
  await run("npm", ["install"], { cwd: WEB_DIR });

  console.log("Building web UI...");
  await run("npm", ["run", "build"], { cwd: WEB_DIR });

  console.log("Building Rust workspace...");
  await run("cargo", ["build", "--workspace"]);

  console.log("Building container image...");
  await run("docker", ["compose", "build", "bob-api"]);

  console.log("Build complete.");
}

async function cmdValidate() {
  await ensureEnvExists();
  const env = await parseEnvFile();
  const token = env.BOB_API_TOKEN;

  if (!token) {
    throw new Error("BOB_API_TOKEN is missing in .env.");
  }

  if (!/^[A-Za-z0-9._~-]{16,}$/.test(token)) {
    throw new Error("BOB_API_TOKEN contains unsupported characters or is too short. Use only [A-Za-z0-9._~-] and at least 16 chars.");
  }

  const healthy = await waitForHealth();
  if (!healthy) {
    console.log("API is not healthy on http://127.0.0.1:8787/healthz");
    await run("docker", ["compose", "ps"]).catch(() => {});
    throw new Error("Health check failed.");
  }

  await validateAuth(token);
  console.log("Validation passed.");
}

async function cmdDesktop() {
  const rootNodeModules = path.join(ROOT_DIR, "node_modules");
  const webNodeModules = path.join(WEB_DIR, "node_modules");

  try {
    await access(rootNodeModules);
  } catch {
    console.log("Installing root dependencies...");
    await run("npm", ["install"]);
  }

  try {
    await access(webNodeModules);
  } catch {
    console.log("Installing web dependencies...");
    await run("npm", ["install"], { cwd: WEB_DIR });
  }

  const health = await request("http://127.0.0.1:8787/healthz");
  if (health?.ok) {
    console.log("BoB API detected at http://127.0.0.1:8787");
  } else {
    console.log("BoB API not running. Start it first with: node ./scripts/bob.mjs start");
    console.log("Launching desktop anyway (configure remote URL in Settings)...");
  }

  console.log("Starting BoB desktop app...");
  await run("npx", ["tauri", "dev", "--config", "desktop/tauri.conf.json"]);
}

async function cmdTest() {
  let needsCleanup = false;
  try {
    console.log("Stopping any existing stack...");
    await cmdStop(false).catch(() => {});

    console.log("Removing all target directories...");
    await removeTargetDirs(ROOT_DIR);

    console.log("Running full build pipeline...");
    await cmdBuild();

    console.log("Running Rust tests...");
    await run("cargo", ["test", "--workspace", "--all-targets"]);

    console.log("Running CLI smoke check...");
    await run("cargo", ["run", "-p", "bob-cli", "--", "--help"], { stdio: "ignore" });

    console.log("Starting server stack...");
    await cmdStart();
    needsCleanup = true;

    console.log("Running runtime validation...");
    await cmdValidate();

    console.log("All tests/build/startup validations passed.");
  } finally {
    if (needsCleanup) {
      await cmdStop(false).catch(() => {});
    }
  }
}

async function main() {
  const command = process.argv[2];
  const args = process.argv.slice(3);

  switch (command) {
    case "start":
      await cmdStart();
      break;
    case "stop":
      await cmdStop(args.includes("--volumes"));
      break;
    case "build":
      await cmdBuild();
      break;
    case "validate":
      await cmdValidate();
      break;
    case "desktop":
      await cmdDesktop();
      break;
    case "test":
      await cmdTest();
      break;
    default:
      console.log("Usage: node ./scripts/bob.mjs <start|stop|build|validate|desktop|test> [args]");
      process.exitCode = 1;
  }
}

main().catch((err) => {
  console.error(err.message || err);
  process.exit(1);
});
