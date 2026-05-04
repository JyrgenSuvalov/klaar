import { Switch } from "@/components/ui/switch";
import { useProfileStore } from "@/store/profileStore";
import { a11y } from "@/i18n/a11yStrings";

export function SettingsPanel() {
  const autoLaunch = useProfileStore((s) => s.autoLaunch);
  const setAutoLaunch = useProfileStore((s) => s.setAutoLaunch);

  return (
    <div className="flex items-center gap-2">
      <label
        htmlFor="auto-launch-toggle"
        className="text-[10px] uppercase tracking-wider cursor-pointer"
        style={{ color: "var(--color-text-secondary)" }}
      >
        Auto-start
      </label>
      <Switch
        id="auto-launch-toggle"
        checked={autoLaunch}
        onCheckedChange={setAutoLaunch}
        aria-label={a11y.autoLaunchToggle()}
      />
    </div>
  );
}
