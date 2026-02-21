import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const tests = [];

function format(actual) {
  return typeof actual === "string" ? actual : JSON.stringify(actual);
}

globalThis.describe = (_name, fn) => {
  fn();
};

globalThis.it = (name, fn) => {
  tests.push({ name, fn });
};

globalThis.expect = (actual) => ({
  toBe(expected) {
    if (!Object.is(actual, expected)) {
      throw new Error(`Expected ${format(actual)} to be ${format(expected)}`);
    }
  },
  toContain(expected) {
    if (typeof actual !== "string") {
      throw new Error("toContain() expects a string actual value");
    }

    if (!actual.includes(expected)) {
      throw new Error(`Expected ${actual} to contain ${expected}`);
    }
  },
  toThrow(expectedMessage) {
    if (typeof actual !== "function") {
      throw new Error("toThrow() expects a function");
    }

    let thrown;
    try {
      actual();
    } catch (error) {
      thrown = error;
    }

    if (!thrown) {
      throw new Error("Expected function to throw");
    }

    if (
      expectedMessage !== undefined &&
      !(thrown instanceof Error && thrown.message.includes(expectedMessage))
    ) {
      throw new Error(
        `Expected thrown message to contain ${expectedMessage}, got ${String(
          thrown instanceof Error ? thrown.message : thrown
        )}`
      );
    }
  }
});

const mode = process.argv[2] ?? "run";
if (mode !== "run") {
  console.error(`Unsupported vitest mode: ${mode}`);
  process.exit(1);
}

const testDir = path.resolve(process.cwd(), "test");
if (!fs.existsSync(testDir)) {
  console.log("No tests found");
  process.exit(0);
}

const testFiles = fs
  .readdirSync(testDir)
  .filter((fileName) => fileName.endsWith(".test.ts"))
  .map((fileName) => path.join(testDir, fileName));

for (const testFile of testFiles) {
  await import(pathToFileURL(testFile).href);
}

for (const testCase of tests) {
  await Promise.resolve(testCase.fn());
  console.log(`PASS ${testCase.name}`);
}

console.log(`\n${testFiles.length} test file(s), ${tests.length} tests passed`);
