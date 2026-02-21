import { pathToFileURL } from "node:url";
import path from "node:path";

const pending = [];

globalThis.describe = (_name, fn) => {
  fn();
};

globalThis.it = (name, fn) => {
  pending.push({ name, fn });
};

globalThis.expect = (actual) => ({
  toBe(expected) {
    if (!Object.is(actual, expected)) {
      throw new Error(`Expected ${String(actual)} to be ${String(expected)}`);
    }
  }
});

const mode = process.argv[2] ?? "run";
if (mode !== "run") {
  console.error(`Unsupported vitest mode: ${mode}`);
  process.exit(1);
}

const testFile = path.resolve(process.cwd(), "src/smoke.test.ts");
await import(pathToFileURL(testFile).href);

for (const testCase of pending) {
  await Promise.resolve(testCase.fn());
  console.log(`PASS ${testCase.name}`);
}

console.log(`\n1 test file, ${pending.length} tests passed`);
