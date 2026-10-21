import { useState } from "react";

import { api, type DisplayDto, type SettingsDto } from "../api";
import { DisplayCards } from "../components/DisplayCards";
import { Button } from "../components/ui";

/**
 * First-run guide.
 *
 * The product's central assumption is unusual and has to be explained once: this
 * tool cannot know which screen the remote client is showing, so the user has to
 * tell it. Without this, a new user sees "尚未选择目标显示器" and has no idea
 * whether the app is broken or waiting for input.
 *
 * Three steps, each with one decision. The guide ends by writing
 * `onboarding_done`, so it will not reappear even if the chosen target equals the
 * default.
 */
export function Onboarding({
  displays,
  settings,
  thumbnails,
  busy,
  onSelectTarget,
  onIdentify,
  onSetDetect,
  onFinish,
}: {
  displays: DisplayDto[];
  settings: SettingsDto;
  thumbnails: Record<string, string>;
  busy: boolean;
  onSelectTarget: (nativeId: string) => Promise<void>;
  onIdentify: () => void;
  onSetDetect: (
    level: SettingsDto["auto_detect"],
    auto: boolean,
  ) => Promise<void>;
  onFinish: () => void;
}) {
  // The step is persisted rather than held only in memory: hiding and re-showing
  // the window remounts React, and the guide used to silently jump back to step 1
  // — which looked like the "next" button did nothing.
  const [step, setStepState] = useState(settings.onboarding_step);
  const [chosen, setChosen] = useState<string | null>(
    settings.target?.native_id ?? null,
  );

  const setStep = (next: number) => {
    setStepState(next);
    // Fire and forget: a failed write only means the step is not remembered.
    void api.saveSettings({ onboarding_step: next });
  };

  const steps = [
    {
      title: "这个工具做什么",
      body: (
        <div className="space-y-3 text-[13px] leading-relaxed text-[var(--color-fg-soft)]">
          <p>
            当你在只有一块屏的设备上远程回这台电脑时，其他屏幕上的窗口是看不见的。
          </p>
          <p>
            Conflux 把这些窗口收拢到你指定的那块屏；远程结束后，再按之前的快照放回去。
          </p>
          <p className="border-l-2 border-[var(--color-border-strong)] pl-3 text-xs text-[var(--color-muted)]">
            它不传输画面，也不建立远程连接。远程软件照常使用。
          </p>
        </div>
      ),
    },
    {
      title: "指定远程端能看到的那块屏",
      body: (
        <div className="space-y-3">
          <p className="text-[13px] leading-relaxed text-[var(--color-fg-soft)]">
            下面的画面就是每块屏现在的真实内容。如果分不清是哪一块，先在屏幕上显示编号，再对照远程画面。
          </p>
          <DisplayCards
            displays={displays}
            target={
              chosen
                ? {
                    native_id: chosen,
                    label:
                      displays.find((d) => d.native_id === chosen)?.label ?? "",
                    present: true,
                    number:
                      displays.find((d) => d.native_id === chosen)?.number ?? null,
                  }
                : null
            }
            thumbnails={thumbnails}
            onSelect={(id) => setChosen(id)}
            disabled={busy}
          />
          <Button size="sm" onClick={onIdentify} disabled={busy}>
            在每块屏幕上显示编号
          </Button>
        </div>
      ),
    },
    {
      title: "让它自动开始和结束",
      body: (
        <div className="space-y-3">
          <p className="text-[13px] leading-relaxed text-[var(--color-fg-soft)]">
            Conflux 能识别一部分远程会话，在开始时整理、结束时恢复。检测依靠可验证的信号，
            级别越高覆盖越多，但也越容易误判。
          </p>
          <div className="rounded-[var(--radius-control)] border border-[var(--color-border)]">
            <DetectChoice
              title="手动"
              description="只用快捷键与托盘，检测保持关闭"
              selected={settings.auto_detect === "off"}
              disabled={busy}
              onSelect={() => onSetDetect("off", false)}
            />
            <DetectChoice
              title="自动（推荐）"
              description="识别远程桌面会话与虚拟显示器，开始时整理、结束时恢复"
              selected={settings.auto_detect !== "off" && settings.auto_gather}
              disabled={busy}
              onSelect={() => onSetDetect("balanced", true)}
            />
          </div>
          <p className="text-[11px] leading-relaxed text-[var(--color-muted)]">
            随时可以在「设置 → 远程会话检测」里改。改之前不会自动移动任何窗口。
          </p>
        </div>
      ),
    },
    {
      title: "随时可以纠正",
      body: (
        <div className="space-y-3 text-[13px] leading-relaxed text-[var(--color-fg-soft)]">
          <p>
            整理之后，如果发现远程端看到的不是那块屏，用这两个入口立即纠正：
          </p>
          <ul className="space-y-1.5 pl-4">
            <li className="list-disc">
              <strong className="font-medium">托盘菜单</strong>：右键托盘图标，选择「识别屏幕编号」或「重新整理到目标屏」。
            </li>
            <li className="list-disc">
              <strong className="font-medium">快捷键</strong>：在「设置」页录制。默认不占用任何组合键。
            </li>
          </ul>
          <p className="text-[var(--color-muted)]">
            恢复入口永远免费可用，不需要任何账号。
          </p>
        </div>
      ),
    },
    {
      title: "有些窗口不会被移动",
      body: (
        <div className="space-y-3 text-[13px] leading-relaxed text-[var(--color-fg-soft)]">
          <p>
            少数应用会自己纠正窗口位置，整理时它们留在原处。这不是失败：Conflux
            会把它们的名字列出来，并给一个「排除」按钮。
          </p>
          <p className="border-l-2 border-[var(--color-border-strong)] pl-3 text-xs text-[var(--color-muted)]">
            排除之后，这些应用既不会被移动，也不会被列入每次的未处理项。
          </p>
          <p className="text-xs text-[var(--color-muted)]">
            全屏应用、最小化的窗口和系统外壳本来就会跳过。
          </p>
        </div>
      ),
    },
  ];

  const current = steps[step]!;
  const isLast = step === steps.length - 1;
  const canAdvance = step !== 1 || chosen !== null;

  const next = async () => {
    if (step === 1 && chosen) {
      await onSelectTarget(chosen);
    }
    if (isLast) {
      onFinish();
      return;
    }
    setStep(step + 1);
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-12 shrink-0 items-center justify-between border-b border-[var(--color-border)] px-5">
        <span className="text-[13px] font-semibold tracking-tight">Conflux</span>
        <button
          type="button"
          onClick={onFinish}
          className="text-xs text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          跳过引导
        </button>
      </div>

      <div className="flex flex-1 flex-col justify-center overflow-y-auto px-8 py-6">
        <div className="mx-auto w-full max-w-lg">
          <p className="text-[11px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
            第 {step + 1} 步，共 {steps.length} 步
          </p>
          <h1 className="mt-1 text-lg font-semibold tracking-tight">
            {current.title}
          </h1>
          <div className="mt-4">{current.body}</div>

          <div className="mt-8 flex items-center justify-between">
            <Button
              variant="quiet"
              onClick={() => setStep(Math.max(0, step - 1))}
              disabled={step === 0 || busy}
            >
              上一步
            </Button>
            <Button
              variant="primary"
              onClick={() => void next()}
              disabled={!canAdvance || busy}
            >
              {isLast ? "开始使用" : "下一步"}
            </Button>
          </div>
          {step === 1 && chosen === null ? (
            <p className="mt-3 text-center text-xs text-[var(--color-muted)]">
              请先选择目标显示器。
            </p>
          ) : null}
        </div>
      </div>

      <div className="flex justify-center gap-1.5 pb-6">
        {steps.map((s, index) => (
          <span
            key={s.title}
            className={`size-1.5 rounded-full ${
              index === step
                ? "bg-[var(--color-accent)]"
                : "bg-[var(--color-border-strong)]"
            }`}
          />
        ))}
      </div>
    </div>
  );
}

/**
 * One radio-style row for the detection choice.
 *
 * A switch would work, but the two options are not independent toggles: "manual"
 * and "automatic" are alternatives, and presenting them as a pair of switches
 * invites the state "detection on, both actions off", which does nothing and
 * looks broken.
 */
function DetectChoice({
  title,
  description,
  selected,
  disabled,
  onSelect,
}: {
  title: string;
  description: string;
  selected: boolean;
  disabled: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      disabled={disabled}
      onClick={onSelect}
      className={`flex w-full items-start gap-2.5 border-b border-[var(--color-border)] px-3 py-2.5 text-left last:border-b-0 disabled:opacity-50 ${
        selected ? "bg-[var(--color-surface-hover)]" : ""
      }`}
    >
      <span
        aria-hidden
        className={`mt-0.5 flex size-3.5 shrink-0 items-center justify-center rounded-full border ${
          selected
            ? "border-[var(--color-accent)]"
            : "border-[var(--color-border-strong)]"
        }`}
      >
        {selected ? (
          <span className="size-1.5 rounded-full bg-[var(--color-accent)]" />
        ) : null}
      </span>
      <span className="min-w-0">
        <span className="block text-[13px] font-medium">{title}</span>
        <span className="mt-0.5 block text-xs leading-relaxed text-[var(--color-muted)]">
          {description}
        </span>
      </span>
    </button>
  );
}

/** Persist completion, so the guide never reappears. */
export async function completeOnboarding(): Promise<void> {
  await api.saveSettings({ onboarding_done: true });
}
