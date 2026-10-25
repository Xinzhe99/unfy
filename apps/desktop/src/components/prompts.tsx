import { t } from "../i18n";
import type { PendingWorkDto, StatusDto } from "../api";

/**
 * The two cards that sit above the pages, on every page.
 *
 * They used to live at the bottom of `App.tsx`, where nothing could render them:
 * the page tests take a page, and these belong to the shell. One of them was
 * showing raw Chinese copy in the English interface for that reason.
 */

export function PermissionPrompt({
  status,
  busy,
  onRequest,
  onSettings,
  onRecheck,
}: {
  status: StatusDto;
  busy: boolean;
  onRequest: () => void;
  onSettings: () => void;
  onRecheck: () => void;
}) {
  return (
    <div className="rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-warning)] bg-[var(--color-surface)] px-3.5 py-2.5">
      <p className="text-[13px] font-medium">
        {t("需要「{name}」权限才能移动窗口", {
          // The permission name is itself a catalogued sentence.
          name: t(status.permission_name),
        })}
      </p>
      <p className="mt-0.5 text-xs leading-relaxed text-[var(--color-fg-soft)]">{t("授权后才能查看窗口预览和移动窗口；显示器信息与诊断仍可使用。")}</p>
      <div className="mt-2.5 flex gap-1.5">
        <button
          type="button"
          onClick={onRequest}
          disabled={busy}
          className="rounded-[var(--radius-control)] bg-[var(--color-accent)] px-2.5 py-1 text-xs font-medium text-[var(--color-accent-fg)] disabled:bg-[var(--color-accent-soft)] disabled:text-[var(--color-muted)]"
        >{t("请求授权")}</button>
        <button
          type="button"
          onClick={onSettings}
          className="rounded-[var(--radius-control)] border border-[var(--color-border-strong)] px-2.5 py-1 text-xs font-medium"
        >{t("前往系统设置")}</button>
        <button
          type="button"
          onClick={onRecheck}
          disabled={busy}
          className="rounded-[var(--radius-control)] px-2.5 py-1 text-xs font-medium text-[var(--color-fg-soft)] hover:bg-[var(--color-surface-sunken)]"
        >{t("重新检查")}</button>
      </div>
    </div>
  );
}

export function PendingCard({
  pending,
  busy,
  onRestore,
  onDismiss,
  onRestoreTransaction,
  onRecheck,
}: {
  pending: PendingWorkDto;
  busy: boolean;
  onRestore: () => void;
  onDismiss: () => void;
  onRestoreTransaction: (transaction: string) => void;
  onRecheck: () => void;
}) {
  return (
    <div className="rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-warning)] bg-[var(--color-surface)] px-3.5 py-2.5">
      <p className="text-[13px] font-medium">{t("上次整理未完成")}</p>
      <p className="mt-0.5 text-xs leading-relaxed text-[var(--color-fg-soft)]">
        {t("有 {count} 个窗口的快照仍待处理{note}。", {
          count: pending.windows,
          note: pending.may_have_moved ? t("，其中部分可能已被移动") : "",
        })}
      </p>
      {pending.blockers > 0 ? (
        <p role="alert" className="mt-1 text-xs text-[var(--color-warning)]">
          {t("有 {count} 份恢复记录暂时无法读取。记录已保留，请重试检查或查看诊断。", { count: pending.blockers })}
        </p>
      ) : null}
      {pending.transactions?.map((transaction) => (
        <div key={transaction.id} className="mt-2 flex flex-wrap items-center gap-2 text-xs">
          <span>{t("恢复记录：{count} 个窗口", { count: transaction.windows })}</span>
          <button type="button" disabled={busy} onClick={() => onRestoreTransaction(transaction.id)}
            className="min-h-8 rounded-[var(--radius-control)] border border-[var(--color-border)] px-2">
            {t("查看并恢复")}
          </button>
        </div>
      ))}
      <div className="mt-2.5 flex gap-1.5">
        <button
          type="button"
          onClick={onRestore}
          disabled={busy}
          className="rounded-[var(--radius-control)] bg-[var(--color-accent)] px-2.5 py-1 text-xs font-medium text-[var(--color-accent-fg)] disabled:bg-[var(--color-accent-soft)] disabled:text-[var(--color-muted)]"
        >{t("尝试恢复")}</button>
        <button
          type="button"
          onClick={onDismiss}
          disabled={busy}
          className="rounded-[var(--radius-control)] px-2.5 py-1 text-xs font-medium text-[var(--color-fg-soft)] hover:bg-[var(--color-surface-sunken)]"
        >{t("稍后处理")}</button>
        <button type="button" onClick={onRecheck} disabled={busy} className="min-h-8 px-2 text-xs">{t("重新检查")}</button>
      </div>
    </div>
  );
}
