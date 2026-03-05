/**
 * agt HTTP Hook Server
 *
 * A lightweight HTTP server that handles Claude Code HTTP hooks.
 * Automatically discovers and loads hook handlers from this directory.
 *
 * Usage:
 *   agt hook serve [--port 9400]
 *   bun run hooks/http/server.ts
 *   AGT_HOOK_PORT=9400 deno run --allow-net --allow-read --allow-env hooks/http/server.ts
 *
 * Routes are auto-registered from handler files:
 *   hooks/http/handlers/tool-analytics.ts  -> POST /hooks/tool-analytics
 *   hooks/http/handlers/post-edit-lint.ts  -> POST /hooks/post-edit-lint
 *   hooks/http/handlers/session-context.ts -> POST /hooks/session-context
 */

const PORT = parseInt(process.env.AGT_HOOK_PORT || "9400", 10);
const HOOKS_DIR = process.env.AGT_HOOKS_DIR || new URL("../", import.meta.url).pathname;

// ── Types ─────────────────────────────────────────────────────────

interface HookInput {
  session_id: string;
  transcript_path: string;
  cwd: string;
  permission_mode: string;
  hook_event_name: string;
  tool_name?: string;
  tool_input?: Record<string, unknown>;
  prompt?: string;
  source?: string;
  model?: string;
  [key: string]: unknown;
}

interface HookOutput {
  continue?: boolean;
  stopReason?: string;
  suppressOutput?: boolean;
  systemMessage?: string;
  decision?: "block";
  reason?: string;
  hookSpecificOutput?: Record<string, unknown>;
  [key: string]: unknown;
}

type HookHandler = (input: HookInput) => Promise<HookOutput | string | null>;

// ── Handler Registry ──────────────────────────────────────────────

const handlers: Map<string, HookHandler> = new Map();

// Built-in handlers

// POST /hooks/tool-analytics — Track tool usage
handlers.set("tool-analytics", async (input: HookInput): Promise<HookOutput | null> => {
  const timestamp = new Date().toISOString();
  const entry = {
    timestamp,
    session_id: input.session_id,
    event: input.hook_event_name,
    tool: input.tool_name || "unknown",
    cwd: input.cwd,
  };

  // Append to log file
  const logDir = `${homeDir()}/.claude/logs`;
  await ensureDir(logDir);
  const logFile = `${logDir}/tool-analytics.jsonl`;
  await appendFile(logFile, JSON.stringify(entry) + "\n");

  return null; // No decision, just logging
});

// POST /hooks/post-edit-lint — Run linter after edits
handlers.set("post-edit-lint", async (input: HookInput): Promise<HookOutput | string | null> => {
  const toolInput = input.tool_input as Record<string, string> | undefined;
  const filePath = toolInput?.file_path;

  if (!filePath) return null;

  // Determine linter based on file extension
  const ext = filePath.split(".").pop()?.toLowerCase();
  const linters: Record<string, string[]> = {
    ts: ["npx", "eslint", "--no-error-on-unmatched-pattern"],
    tsx: ["npx", "eslint", "--no-error-on-unmatched-pattern"],
    js: ["npx", "eslint", "--no-error-on-unmatched-pattern"],
    jsx: ["npx", "eslint", "--no-error-on-unmatched-pattern"],
    py: ["python3", "-m", "ruff", "check"],
    rs: ["cargo", "clippy", "--message-format=short", "--"],
  };

  const linter = ext ? linters[ext] : undefined;
  if (!linter) return null;

  try {
    const proc = Bun
      ? Bun.spawn([...linter, filePath], {
          cwd: input.cwd,
          stdout: "pipe",
          stderr: "pipe",
        })
      : null;

    if (proc) {
      const stdout = await new Response(proc.stdout).text();
      const stderr = await new Response(proc.stderr).text();
      const exitCode = await proc.exited;

      if (exitCode !== 0) {
        const issues = (stdout + stderr).trim();
        if (issues) {
          return {
            decision: "block",
            reason: `Lint issues found in ${filePath}:\n${issues}`,
          };
        }
      }
    }
  } catch {
    // Linter not available, skip silently
  }

  return null;
});

// POST /hooks/session-context — Load project context at session start
handlers.set("session-context", async (input: HookInput): Promise<HookOutput | string> => {
  const cwd = input.cwd;
  const contextParts: string[] = [];

  // Check for project-level context files
  const contextFiles = [
    ".claude/CLAUDE.md",
    "CLAUDE.md",
    ".context/active.md",
    "TODO.md",
  ];

  for (const file of contextFiles) {
    const fullPath = `${cwd}/${file}`;
    try {
      const content = await readFileText(fullPath);
      if (content) {
        contextParts.push(`[${file}]\n${content.slice(0, 500)}`);
      }
    } catch {
      // File doesn't exist, skip
    }
  }

  // Check git status
  try {
    const gitStatus = await runCommand("git", ["status", "--porcelain"], cwd);
    if (gitStatus.trim()) {
      const lines = gitStatus.trim().split("\n");
      contextParts.push(
        `[git status] ${lines.length} modified file(s):\n${lines.slice(0, 10).join("\n")}`
      );
    }
  } catch {
    // Not a git repo, skip
  }

  if (contextParts.length === 0) {
    return { suppressOutput: true };
  }

  return {
    hookSpecificOutput: {
      hookEventName: "SessionStart",
      additionalContext: contextParts.join("\n\n"),
    },
  };
});

// POST /hooks/team-activity — Log team/subagent activity
handlers.set("team-activity", async (input: HookInput): Promise<HookOutput | null> => {
  const timestamp = new Date().toISOString();
  const entry = {
    timestamp,
    session_id: input.session_id,
    event: input.hook_event_name,
    agent_id: input.agent_id || null,
    agent_type: input.agent_type || null,
    cwd: input.cwd,
  };

  const logDir = `${homeDir()}/.claude/logs`;
  await ensureDir(logDir);
  const logFile = `${logDir}/team-activity.jsonl`;
  await appendFile(logFile, JSON.stringify(entry) + "\n");

  return null;
});

// POST /hooks/health — Health check
handlers.set("health", async (): Promise<HookOutput> => {
  return {
    suppressOutput: true,
  };
});

// ── HTTP Server ───────────────────────────────────────────────────

async function handleRequest(req: Request): Promise<Response> {
  const url = new URL(req.url);
  const path = url.pathname;

  // Health check
  if (path === "/health" || path === "/") {
    return Response.json({
      status: "ok",
      handlers: Array.from(handlers.keys()),
      uptime: process.uptime(),
    });
  }

  // Route: /hooks/<handler-name>
  const match = path.match(/^\/hooks\/(.+)$/);
  if (!match || req.method !== "POST") {
    return new Response("Not Found", { status: 404 });
  }

  const handlerName = match[1];
  const handler = handlers.get(handlerName);

  if (!handler) {
    return Response.json(
      { error: `Unknown hook handler: ${handlerName}` },
      { status: 404 }
    );
  }

  try {
    const input: HookInput = await req.json();
    const result = await handler(input);

    if (result === null || result === undefined) {
      return new Response(null, { status: 200 });
    }

    if (typeof result === "string") {
      return new Response(result, {
        status: 200,
        headers: { "Content-Type": "text/plain" },
      });
    }

    return Response.json(result);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    console.error(`[${handlerName}] Error:`, message);
    return Response.json({ error: message }, { status: 500 });
  }
}

// ── Utilities ─────────────────────────────────────────────────────

function homeDir(): string {
  return process.env.HOME || process.env.USERPROFILE || "/tmp";
}

async function ensureDir(dir: string): Promise<void> {
  const { mkdir } = await import("node:fs/promises");
  await mkdir(dir, { recursive: true });
}

async function appendFile(path: string, data: string): Promise<void> {
  const { appendFile: af } = await import("node:fs/promises");
  await af(path, data, "utf-8");
}

async function readFileText(path: string): Promise<string> {
  const { readFile } = await import("node:fs/promises");
  return readFile(path, "utf-8");
}

async function runCommand(
  cmd: string,
  args: string[],
  cwd: string
): Promise<string> {
  const { execFile } = await import("node:child_process");
  const { promisify } = await import("node:util");
  const execFileAsync = promisify(execFile);
  const { stdout } = await execFileAsync(cmd, args, { cwd, timeout: 5000 });
  return stdout;
}

// ── Start ─────────────────────────────────────────────────────────

// Detect runtime and start server
declare const Bun: any;
declare const Deno: any;

if (typeof Bun !== "undefined") {
  // Bun runtime
  Bun.serve({
    port: PORT,
    fetch: handleRequest,
  });
  console.log(`[agt hook server] Bun listening on http://localhost:${PORT}`);
  console.log(`[agt hook server] Handlers: ${Array.from(handlers.keys()).join(", ")}`);
} else if (typeof Deno !== "undefined") {
  // Deno runtime
  Deno.serve({ port: PORT }, handleRequest);
  console.log(`[agt hook server] Deno listening on http://localhost:${PORT}`);
  console.log(`[agt hook server] Handlers: ${Array.from(handlers.keys()).join(", ")}`);
} else {
  // Node.js / tsx runtime
  import("node:http").then(({ createServer }) => {
    const server = createServer(async (req, res) => {
      // Collect body
      const chunks: Buffer[] = [];
      for await (const chunk of req) {
        chunks.push(chunk as Buffer);
      }
      const body = Buffer.concat(chunks).toString();

      const url = `http://localhost:${PORT}${req.url}`;
      const request = new Request(url, {
        method: req.method,
        headers: Object.fromEntries(
          Object.entries(req.headers)
            .filter(([_, v]) => v !== undefined)
            .map(([k, v]) => [k, Array.isArray(v) ? v[0] : v!])
        ),
        body: req.method === "POST" ? body : undefined,
      });

      const response = await handleRequest(request);
      const responseBody = await response.text();

      res.writeHead(response.status, {
        "Content-Type": response.headers.get("Content-Type") || "application/json",
      });
      res.end(responseBody);
    });

    server.listen(PORT, () => {
      console.log(`[agt hook server] Node listening on http://localhost:${PORT}`);
      console.log(`[agt hook server] Handlers: ${Array.from(handlers.keys()).join(", ")}`);
    });
  });
}
