import { useState } from "react";

import { api, type StatusDto } from "../api";
import { Button, Callout } from "./ui";

/**
 * Permission banner.
 *
 * macOS grants Accessibility asynchronously, so clicking the button cannot be
 * assumed to fix anything. The banner re-checks on demand and explains the exact
 * step instead of showing a generic "permission required" message. Everything
 * else in the app stays usable without it, and the banner says so — a user who
 * cannot grant the permission should not feel the app is broken.
 */
export function PermissionBanner({
  status,
  onRecheck,
  busy,
}: {
  status: StatusDto;
  onRecheck: () => void;
  busy: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const [requested, setRequested] = useState(false);

  if (status.has_permission || !status.supported) {
    return null;
  }

  return (
    <Callout
      tone="warning"
      title={`需要「${status.permission_name}」权限才能移动窗口`}
      actions={
        <>
          <Button
            tone="accent"
            size="sm"
            disabled={busy}
            onClick={async () => {
              await api.requestPermission();
              setRequested(true);
            }}
          >
            请求授权
          </Button>
          <Button
            size="sm"
            disabled={busy}
            onClick={() => void api.openPermissionSettings()}
          >
            前往系统设置
          </Button>
          <Button size="sm" tone="quiet" onClick={onRecheck} disabled={busy}>
            重新检查
          </Button>
          <Button
            size="sm"
            tone="quiet"
            onClick={() => setExpanded((v) => !v)}
          >
            {expanded ? "收起" : "手动步骤"}
          </Button>
        </>
      }
    >
      <p>
        没有这项权限时其余功能仍可使用：可以预览布局、查看显示器、导出诊断，只是不会移动任何窗口。
      </p>
      {requested ? (
        <p className="mt-2">
          如果系统没有弹出授权窗口，说明它之前已被拒绝过。请用「前往系统设置」手动开启。
        </p>
      ) : null}
      {expanded ? (
        <ol className="mt-2 list-decimal space-y-1 pl-4">
          <li>打开「系统设置」→「隐私与安全性」→「{status.permission_name}」。</li>
          <li>点击左下角「+」，选择 Conflux 并确认。</li>
          <li>回到这里点「重新检查」。</li>
        </ol>
      ) : null}
    </Callout>
  );
}
