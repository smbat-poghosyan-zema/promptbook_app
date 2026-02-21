import fs from "node:fs";
import path from "node:path";

const root = process.cwd();

const allowedExtensions = new Set([".js", ".mjs", ".cjs", ".ts", ".tsx", ".rs"]);
let issues = 0;

function walk(dirPath) {
  const entries = fs.readdirSync(dirPath, { withFileTypes: true });

  for (const entry of entries) {
    const fullPath = path.join(dirPath, entry.name);

    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name === "target" || entry.name === "dist") {
        continue;
      }
      walk(fullPath);
      continue;
    }

    if (!allowedExtensions.has(path.extname(entry.name))) {
      continue;
    }

    const content = fs.readFileSync(fullPath, "utf8");
    if (content.includes("<<<<<<<") || content.includes(">>>>>>>")) {
      issues += 1;
      console.error(`Merge marker found: ${fullPath}`);
    }
  }
}

walk(root);

if (issues > 0) {
  process.exit(1);
}

console.log("eslint shim: no merge markers found");
