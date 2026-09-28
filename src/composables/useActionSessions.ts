// 安装 / 卸载 / 升级任务的会话状态。
//
// 为什么不留在 ActionPanel 里：任务启动后进程会一直跑到自己结束或被取消，而面板是按
// "选中的包"挂载的。切到别的包、换个视图，面板就被销毁重建，组件里的日志、退出码、
// 任务 id 一并消失——回来只剩空白，既看不出任务还在跑，也不知道卡在哪一步；
// 挂在面板上的 `proc://event` 监听器同样随卸载退订，中间那段输出直接丢掉。
//
// 所以会话按 `来源:操作:包名:类型` 登记在模块级 Map 里，进程事件由一个应用级监听器
// 按任务 id 分发。面板退化成这份状态的一个视图，视图层用 onActionSettled 感知结束。
import { onScopeDispose, reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { PackageAction, ProcEvent, RequestOptions } from "../types";

export type LogLine = { stream: string; line: string };

export type ActionTarget = {
  source: string;
  action: PackageAction;
  name: string;
  version?: string | null;
  kind?: string | null;
};

export type ActionSession = {
  key: string;
  target: ActionTarget;
  lines: LogLine[];
  /** 本次执行的完整命令，日志首行展示它 */
  display: string | null;
  taskId: string | null;
  running: boolean;
  /** 已经出过结果：成功、失败或压根没启动起来 */
  finished: boolean;
  exitCode: number | null;
  permissionIssue: boolean;
};

/** 输出最多留这么多行：长时间任务不该把内存吃满 */
const MAX_LINES = 2000;

const sessions = new Map<string, ActionSession>();
/** 任务 id → 会话。事件只带 id，靠它找回落到哪个包上 */
const byTask = new Map<string, ActionSession>();
const settledHandlers = new Set<(session: ActionSession) => void>();

let listening: Promise<UnlistenFn> | null = null;

export function actionSessionKey(target: ActionTarget): string {
  return [target.source, target.action, target.name, target.kind ?? ""].join(":");
}

function blankSession(key: string, target: ActionTarget): ActionSession {
  return reactive<ActionSession>({
    key,
    target,
    lines: [],
    display: null,
    taskId: null,
    running: false,
    finished: false,
    exitCode: null,
    permissionIssue: false,
  });
}

/**
 * 取某个目标的会话。
 *
 * 没有执行过就现造一个空白会话，且**不登记**：浏览几百个包不该在内存里留下几百条空记录，
 * 只有真正执行过（或正在执行）的才值得记住。
 */
export function actionSession(target: ActionTarget): ActionSession {
  const key = actionSessionKey(target);
  return sessions.get(key) ?? blankSession(key, target);
}

/** 订阅任务结束（成功或失败）。在组件作用域内调用，卸载时自动退订。 */
export function onActionSettled(handler: (session: ActionSession) => void) {
  settledHandlers.add(handler);
  onScopeDispose(() => settledHandlers.delete(handler));
}

/**
 * 事件监听只建一次，且不随面板卸载退订——这是"切走再回来还能看到进度"的前提。
 * 诊断页的命令试跑走的是同一个事件名，任务 id 不在 byTask 里，直接忽略。
 */
async function ensureListener(): Promise<void> {
  if (listening) {
    await listening;
    return;
  }
  const pending = listen<ProcEvent>("proc://event", ({ payload }) => handleEvent(payload));
  listening = pending;
  try {
    await pending;
  } catch (error) {
    // 没装上就允许下次重试，否则一次失败会永久卡住所有执行
    listening = null;
    throw error;
  }
}

function handleEvent(event: ProcEvent) {
  const session = byTask.get(event.id);
  // 会话已经被下一次执行接管时，迟到的旧事件不能改写新任务的日志
  if (!session || session.taskId !== event.id) return;
  if (event.kind === "output") {
    pushLine(session, event.stream, event.line);
  } else if (event.kind === "exit") {
    session.exitCode = event.code;
    session.running = false;
    session.finished = true;
    byTask.delete(event.id);
    pushLine(
      session,
      "meta",
      `退出：code=${event.code ?? "被信号终止"} · killed=${event.killed} · ${event.elapsedMs}ms`,
    );
    for (const handler of settledHandlers) handler(session);
    // 权限判断要多问后端一次，慢一点也无所谓，别挡着结束通知
    void checkPermission(session);
  }
}

function pushLine(session: ActionSession, stream: string, line: string) {
  session.lines.push({ stream, line });
  if (session.lines.length > MAX_LINES) {
    session.lines.splice(0, session.lines.length - MAX_LINES);
  }
}

async function checkPermission(session: ActionSession) {
  if (session.exitCode === 0) return;
  const texts = session.lines.filter((item) => item.stream !== "meta").map((item) => item.line);
  try {
    session.permissionIssue = await invoke<boolean>("is_permission_issue", { lines: texts });
  } catch {
    session.permissionIssue = false;
  }
}

function reset(session: ActionSession, display: string | null) {
  session.lines = [];
  session.display = display;
  session.taskId = null;
  session.exitCode = null;
  session.finished = false;
  session.permissionIssue = false;
}

export type StartOptions = RequestOptions & { admin: boolean; display?: string | null };

/** 启动一次执行。同一会话不允许并发，跨包并发由后端的全局锁兜底。 */
export async function startAction(session: ActionSession, options: StartOptions): Promise<void> {
  if (session.running) return;
  // 上一次的 id 已经作废（取消后可能根本收不到退出事件），别留下一张过期映射
  if (session.taskId) byTask.delete(session.taskId);
  reset(session, options.display ?? null);

  const id = `action-${Date.now()}`;
  session.taskId = id;
  try {
    await ensureListener();
    // 先登记再 invoke：进程可能在命令返回之前就开始吐输出
    sessions.set(session.key, session);
    byTask.set(id, session);
    session.running = true;
    await invoke<string>("run_package_action", {
      taskId: id,
      source: session.target.source,
      action: session.target.action,
      name: session.target.name,
      version: session.target.version ?? null,
      kind: session.target.kind ?? null,
      admin: options.admin,
      registry: options.registry,
      proxy: options.proxy,
      insecure: options.insecure,
    });
    pushLine(session, "meta", `启动 ${session.display ?? session.target.name}`);
  } catch (error) {
    // 没跑起来：把状态收回来，别让面板永远显示"执行中"
    byTask.delete(id);
    session.running = false;
    session.taskId = null;
    session.finished = true;
    pushLine(session, "stderr", error instanceof Error ? error.message : String(error));
  }
}

export async function cancelAction(session: ActionSession): Promise<void> {
  if (!session.taskId) return;
  await invoke<boolean>("cancel_command", { id: session.taskId });
}
