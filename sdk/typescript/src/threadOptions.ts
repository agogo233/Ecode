export type ApprovalMode = "never" | "on-request" | "on-failure" | "untrusted";

export type SandboxMode = "read-only" | "workspace-write" | "danger-full-access";

export type ThreadOptions = {
  model?: string;
  /** Source classification applied when this thread is first created. */
  threadSource?: string;
  sandboxMode?: SandboxMode;
  workingDirectory?: string;
  skipGitRepoCheck?: boolean;
};
