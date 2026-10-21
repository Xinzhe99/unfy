import { useState } from "react";

import { api, type StatusDto } from "../api";

/**
 * Permission banner.
 *
 * macOS grants Accessibility asynchronously, so the banner cannot assume that
 * clicking the button fixes anything. It re-checks on demand and explains the
 * exact step, rather than showing a generic "permission required" message.
 */
export function PermissionBanner({
  status,
  onRequest,
  onRecheck,
  busy,
}: {
  status: StatusDto;
  onRequest: () => void;
  onRecheck: () => void;
  busy: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  if (status.has_permission || !status.supported) {
    return null;
  }

  return (
    <div className="mb-4 rounded-lg border border-[var(--color-warning)]/40 bg-[var(--color-warning)]/8 p-4">
      <p className="text-sm font-medium">
        需要「{status.permission_name}」权限才能移动窗口
      </p>
      <p className="mt-1 text-xs leading-relaxed text-[var(--color-muted)]">
        没有这项权限时，其余功能仍可使用：可以预览布局、查看显示器、导出诊断，只是不会移动任何窗口。
      </p>
      <div className="mt-3 flex flex-wrap gap-2">
        <button
          type="button"
          onClick={onRequest}
          disabled={busy}
          className="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
        >
          打开系统授权
        </button>
        <button
          type="button"
          onClick={() => void api.openPermissionSettings()}
          disabled={busy}
          className="rounded-md border border-[var(--color-border)] px-3 py-1.5 text-xs font-medium disabled:opacity-50"
        >
          前往系统设置
        </button>
        <button
          type="button"
          onClick={onRecheck}
          disabled={busy}
          className="rounded-md border border-[var(--color-border)] px-3 py-1.5 text-xs font-medium disabled:opacity-50"
        >
          我已授权，重新检查
        </button>
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="rounded-md px-3 py-1.5 text-xs font-medium text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          {expanded ? "收起步骤" : "手动授权步骤"}
        </button>
      </div>
      {expanded ? (
        <ol className="mt-3 list-decimal space-y-1 pl-5 text-xs text-[var(--color-muted)]">
          <li>打开「系统设置」→「隐私与安全性」→「辅助功能」。</li>
          <li>点击左下角「+」，选择 Conflux 后确认。</li>
          <li>回到这里点击「我已授权，重新检查」。</li>
        </ol>
      ) : null}
    </div>
  );
}
