const command = process.argv[2] ?? "dev";

if (command !== "dev" && command !== "build") {
  console.error(`Unsupported tauri command: ${command}`);
  process.exit(1);
}

console.log(`tauri ${command} (shim): scaffold placeholder for offline environment`);
