import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useProfileStore } from "@/store/profileStore";

export function ProfileSelector() {
  const profiles = useProfileStore((s) => s.profiles);
  const activeProfileId = useProfileStore((s) => s.activeProfileId);
  const activeProfileName = useProfileStore((s) => s.activeProfileName);
  const loadProfile = useProfileStore((s) => s.loadProfile);
  const isLoading = useProfileStore((s) => s.isLoading);

  const handleSelect = (id: string) => {
    if (id !== activeProfileId) {
      void loadProfile(id);
    }
  };

  return (
    <Select
      value={activeProfileId ?? ""}
      onValueChange={handleSelect}
      disabled={isLoading}
    >
      <SelectTrigger className="h-7 w-[160px] text-xs">
        <SelectValue placeholder="No profile">
          {activeProfileName ?? "No profile"}
        </SelectValue>
      </SelectTrigger>
      <SelectContent>
        {profiles.length === 0 ? (
          <div className="px-2 py-1.5 text-xs text-[--color-text-secondary]">
            No profiles
          </div>
        ) : (
          profiles.map((p) => (
            <SelectItem key={p.id} value={p.id} className="text-xs">
              {p.name}
            </SelectItem>
          ))
        )}
      </SelectContent>
    </Select>
  );
}
