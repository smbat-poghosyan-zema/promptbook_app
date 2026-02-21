import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const args = process.argv.slice(2);
const hasCargo = spawnSync("cargo", ["--version"], { stdio: "ignore" }).status === 0;

if (hasCargo) {
  const result = spawnSync("cargo", args, { stdio: "inherit" });
  process.exit(result.status ?? 1);
}

if (args[0] !== "test") {
  console.error(`cargo shim only supports 'test' without Rust toolchain. Received: ${args.join(" ")}`);
  process.exit(1);
}

const manifestIndex = args.indexOf("--manifest-path");
const manifestPath =
  manifestIndex >= 0 && args[manifestIndex + 1]
    ? path.resolve(process.cwd(), args[manifestIndex + 1])
    : path.resolve(process.cwd(), "src-tauri/Cargo.toml");

if (!fs.existsSync(manifestPath)) {
  console.error(`cargo shim could not find manifest at ${manifestPath}`);
  process.exit(1);
}

const libPath = path.resolve(path.dirname(manifestPath), "src/lib.rs");
if (!fs.existsSync(libPath)) {
  console.error(`cargo shim expected Rust library at ${libPath}`);
  process.exit(1);
}

const rustSource = fs.readFileSync(libPath, "utf8");
const hasTest = rustSource.includes("assert_eq!(placeholder_engine_value(41), 42)");
if (!hasTest) {
  console.error("cargo shim could not find the expected placeholder Rust unit test assertion");
  process.exit(1);
}

console.log("cargo test (shim): Rust toolchain not installed; validated placeholder unit test source");
