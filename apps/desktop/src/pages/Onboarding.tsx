import { useState } from "react";
import { t } from "../i18n";
import { api, errorMessage, type DisplayDto, type DisplayThumbnailDto, type SettingsDto } from "../api";
import { DisplayCards } from "../components/DisplayCards";
import { Button } from "../components/ui";

export function Onboarding({ displays, settings, thumbnails, busy, onSelectTarget, onIdentify, onFinish }: {
  displays: DisplayDto[];
  settings: SettingsDto;
  thumbnails: Record<string, DisplayThumbnailDto>;
  busy: boolean;
  onSelectTarget: (nativeId: string) => Promise<boolean>;
  onIdentify: () => void;
  onFinish: () => void;
}) {
  const [step, setStep] = useState(Math.min(1, Math.max(0, settings.onboarding_step)));
  const [chosen, setChosen] = useState(settings.target?.native_id ?? null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const display = displays.find((item) => item.native_id === chosen);
  const advance = async () => {
    if (saving || busy) return;
    setSaving(true);
    setError(null);
    try {
      if (step === 0) {
        await api.saveSettings({ onboarding_step: 1 });
        setStep(1);
      } else if (display && await onSelectTarget(display.native_id)) {
        onFinish();
      }
    } catch (cause) {
      setError(errorMessage(cause));
    } finally { setSaving(false); }
  };
  return (
    <div className="flex h-full flex-col">
      <header className="flex h-12 shrink-0 items-center justify-between border-b border-[var(--color-border)] px-5">
        <span className="text-[13px] font-semibold">Unfy</span>
        <Button variant="quiet" disabled={busy || saving} onClick={onFinish}>{t("跳过引导")}</Button>
      </header>
      <main className="min-h-0 flex-1 overflow-y-auto px-6 py-8">
        <div className="mx-auto max-w-xl space-y-4">
          <p className="text-xs text-[var(--color-muted)]">{t("第 {step} 步，共 {total} 步", { step: step + 1, total: 2 })}</p>
          <h1 className="text-xl font-semibold">{t(step === 0 ? "安装在被控主机" : "指定远程端能看到的那块屏")}</h1>
          {step === 0 ? (
            <div className="space-y-3 text-[13px] leading-relaxed">
              <p>{t("请在需要移动窗口的被控电脑上运行 Unfy。它不传输画面，也不建立远程连接。")}</p>
              <p>{t("选择目标后，你可以手动整理一次，再恢复整理前布局。快捷键与自动化可稍后在设置中选择。")}</p>
            </div>
          ) : (
            <div className="space-y-3">
              <p className="text-[13px]">{t("选择只保存目标，不会移动窗口。画面不可用时可用 Unfy 编号确认。")}</p>
              <DisplayCards displays={displays} target={display ? { native_id: display.native_id, label: display.label, present: true, confirmed: true, number: display.number } : null} thumbnails={thumbnails} onSelect={setChosen} disabled={busy || saving} />
              <Button onClick={onIdentify} disabled={busy || saving}>{t("在屏幕上显示编号")}</Button>
              {!display ? <p className="text-xs">{t("请先选择目标显示器。")}</p> : null}
              <p className="text-xs text-[var(--color-muted)]">{t("如果需要窗口控制权限，进入概览后按提示授权；未授权时不会移动窗口。")}</p>
            </div>
          )}
          {error ? <p role="alert" className="text-xs text-[var(--color-danger)]">{error}</p> : null}
          <div className="flex justify-between gap-2">
            <Button variant="quiet" disabled={step === 0 || busy || saving} onClick={() => setStep(0)}>{t("上一步")}</Button>
            <Button variant="primary" disabled={busy || saving || (step === 1 && !display)} onClick={() => void advance()}>{t(step === 1 ? "开始使用" : "下一步")}</Button>
          </div>
        </div>
      </main>
    </div>
  );
}

export async function completeOnboarding(): Promise<void> {
  await api.saveSettings({ onboarding_done: true });
}
