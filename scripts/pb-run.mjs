#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawn } from "node:child_process";

function die(msg) {
  console.error(msg);
  process.exit(1);
}

function parseArgs(argv) {
  const args = { agent: null, file: null, from: null, to: null, workspace: "." };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--agent") args.agent = argv[++i];
    else if (a === "--file") args.file = argv[++i];
    else if (a === "--from") args.from = argv[++i];
    else if (a === "--to") args.to = argv[++i];
    else if (a === "--workspace") args.workspace = argv[++i];
    else if (a === "--help") {
      console.log(`Usage:
  node scripts/pb-run.mjs --file promptbooks/xxx.yaml --agent codex|claude|copilot [--workspace .] [--from stepId] [--to stepId]

Notes:
- Creates .promptbook_runs/<runId>/steps/<stepId>.md
- Streams child output to console and saves logs under the run folder
`);
      process.exit(0);
    }
  }
  if (!args.file) die("Missing --file");
  if (!args.agent) die("Missing --agent");
  return args;
}

function requireCmd(cmd) {
  // best-effort; don't hard fail on which, but warn
  try {
    spawn("bash", ["-lc", `command -v ${cmd} >/dev/null 2>&1`]).on("exit", (code) => {
      if (code !== 0) console.error(`[warn] '${cmd}' not found in PATH (runner will still try)`);
    });
  } catch {}
}

function yamlLoadViaNode(yamlText) {
  // minimal dependency-free loader via dynamic import of yaml if available
  // If not available, instruct user to install.
  return import("yaml").then(({ parse }) => parse(yamlText));
}

function mkRunDir() {
  const runId = `${new Date().toISOString().replace(/[:.]/g, "-")}-${Math.random().toString(16).slice(2)}`;
  const runDir = path.join(".promptbook_runs", runId);
  fs.mkdirSync(path.join(runDir, "steps"), { recursive: true });
  fs.mkdirSync(path.join(runDir, "logs"), { recursive: true });
  return { runId, runDir };
}

function buildStepTaskMd(pb, step) {
  const rules = (pb.global_rules || []).map((r) => `- ${r}`).join("\n");
  const verify = (step.verify || []).map((c) => `- \`${c}\``).join("\n");
  return `# Task: ${pb.name} — ${step.id}: ${step.title}

## Global rules
${rules || "- (none)"}

## Step prompt
${step.prompt || ""}

## Verification commands
${verify || "- (none)"}

## Output required
- Implement the changes in this repo.
- Run the verification commands and ensure they pass.
- Summarize what changed and why.
`;
}

function cmdForAgent(agent, stepFileAbs, workspaceAbs) {
  if (agent === "codex") {
    requireCmd("codex");
    const prompt = [
      `You are an autonomous coding agent.`,
      `Read the task file at: ${stepFileAbs}`,
      `Work in the repo at: ${workspaceAbs}`,
      `Follow the task exactly, run verification commands, and ensure green.`,
      `Stop when done.`
    ].join("\n");
    // Non-interactive mode
    return { cmd: "codex", args: ["exec", "--skip-git-repo-check", "--full-auto", "--sandbox", "workspace-write", prompt] };
  }

  if (agent === "claude") {
    requireCmd("claude");
    const sys = `You are an autonomous coding agent. Use the provided task file content. Apply changes in the repo, run verify commands, and finish with a concise summary.`;
    // Pipe file into claude -p
    const bashCmd = `cd "${workspaceAbs}" && cat "${stepFileAbs}" | claude -p '${sys.replace(/'/g, `'\"'\"'`)}'`;
    return { cmd: "bash", args: ["-lc", bashCmd] };
  }

  if (agent === "copilot") {
    requireCmd("copilot");
    // Keep safest default: no allow-all-tools. User can run copilot interactively to approve tools.
    const prompt = `Read the task file at ${stepFileAbs} and implement it in the repo at ${workspaceAbs}. Run the verification commands listed in the task file.`;
    return { cmd: "bash", args: ["-lc", `cd "${workspaceAbs}" && copilot -p "${prompt.replace(/"/g, '\\"')}"`] };
  }

  die(`Unknown agent: ${agent}`);
}

async function main() {
  const args = parseArgs(process.argv);
  const pbText = fs.readFileSync(args.file, "utf8");
  let pb;
  try {
    pb = await yamlLoadViaNode(pbText);
  } catch (e) {
    die(
      `Failed to parse YAML. Install dependency first:\n` +
      `  pnpm add -D yaml\n\n` +
      `Error: ${e?.message || e}`
    );
  }

  const steps = pb.steps || [];
  if (!Array.isArray(steps) || steps.length === 0) die("Promptbook has no steps[]");

  const { runId, runDir } = mkRunDir();
  const workspaceAbs = path.resolve(args.workspace);

  const stepIds = steps.map((s) => s.id);
  const fromIdx = args.from ? stepIds.indexOf(args.from) : 0;
  const toIdx = args.to ? stepIds.indexOf(args.to) : steps.length - 1;
  if (fromIdx < 0) die(`--from step not found: ${args.from}`);
  if (toIdx < 0) die(`--to step not found: ${args.to}`);
  if (toIdx < fromIdx) die("--to must be >= --from");

  console.log(`[run] ${pb.name} (${pb.version})`);
  console.log(`[run] runId=${runId}`);
  console.log(`[run] agent=${args.agent}`);
  console.log(`[run] workspace=${workspaceAbs}`);
  console.log("");

  for (let i = fromIdx; i <= toIdx; i++) {
    const step = steps[i];
    const stepMd = buildStepTaskMd(pb, step);

    const stepFile = path.join(runDir, "steps", `${step.id}.md`);
    fs.writeFileSync(stepFile, stepMd, "utf8");

    const stepFileAbs = path.resolve(stepFile);
    const a = (step.agent || args.agent);

    console.log(`\n=== STEP ${step.id}: ${step.title} (agent=${a}) ===\n`);

    const { cmd, args: cmdArgs } = cmdForAgent(a, stepFileAbs, workspaceAbs);

    const logFile = path.join(runDir, "logs", `${step.id}.log`);
    const out = fs.openSync(logFile, "a");

    await new Promise((resolve, reject) => {
      const child = spawn(cmd, cmdArgs, {
        stdio: ["ignore", "pipe", "pipe"],
        env: { ...process.env },
      });

      const stamp = () => new Date().toISOString();
      child.stdout.on("data", (d) => {
        const s = d.toString();
        process.stdout.write(s);
        fs.writeSync(out, `[${stamp()}][stdout] ${s}`);
      });
      child.stderr.on("data", (d) => {
        const s = d.toString();
        process.stderr.write(s);
        fs.writeSync(out, `[${stamp()}][stderr] ${s}`);
      });

      child.on("error", reject);
      child.on("close", (code) => {
        fs.closeSync(out);
        if (code === 0) resolve();
        else reject(new Error(`Step ${step.id} failed with exit code ${code}. See ${logFile}`));
      });
    });
  }

  console.log(`\n[done] run logs: ${runDir}\n`);
}

main().catch((e) => {
  console.error(`\n[error] ${e?.message || e}\n`);
  process.exit(1);
});
