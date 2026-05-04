import { useState, useCallback } from "react";
import { useProfileStore } from "@/store/profileStore";
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { a11y } from "@/i18n/a11yStrings";

export function ProfileActions() {
  const activeProfileId = useProfileStore((s) => s.activeProfileId);
  const updateProfile = useProfileStore((s) => s.updateProfile);
  const saveProfile = useProfileStore((s) => s.saveProfile);
  const deleteProfile = useProfileStore((s) => s.deleteProfile);
  const activeProfileName = useProfileStore((s) => s.activeProfileName);

  const [showSaveAs, setShowSaveAs] = useState(false);
  const [showDelete, setShowDelete] = useState(false);
  const [newName, setNewName] = useState("");

  const handleSave = useCallback(() => {
    if (activeProfileId) {
      void updateProfile(activeProfileId);
    } else {
      setNewName("");
      setShowSaveAs(true);
    }
  }, [activeProfileId, updateProfile]);

  const handleSaveAs = useCallback(() => {
    setNewName("");
    setShowSaveAs(true);
  }, []);

  const handleSaveAsConfirm = useCallback(() => {
    const trimmed = newName.trim();
    if (trimmed) {
      void saveProfile(trimmed);
      setShowSaveAs(false);
      setNewName("");
    }
  }, [newName, saveProfile]);

  const handleDelete = useCallback(() => {
    if (activeProfileId) {
      void deleteProfile(activeProfileId);
      setShowDelete(false);
    }
  }, [activeProfileId, deleteProfile]);

  const btnClass =
    "px-2 py-0.5 text-[10px] rounded border border-[--color-border] cursor-pointer transition-all hover:bg-[--color-surface-elevated] text-[--color-text-secondary] hover:text-[--color-text-primary] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]";

  return (
    <>
      <div className="flex items-center gap-1">
        <button className={btnClass} onClick={handleSave} title="Save current settings to profile">
          Save
        </button>
        <button className={btnClass} onClick={handleSaveAs} title="Save as new profile">
          Save As
        </button>
        {activeProfileId && (
          <button
            className={`${btnClass} hover:border-[--color-meter-red] hover:text-[--color-meter-red]`}
            onClick={() => setShowDelete(true)}
            title="Delete profile"
          >
            Delete
          </button>
        )}
      </div>

      {/* Save As dialog */}
      <Dialog open={showSaveAs} onOpenChange={setShowSaveAs}>
        <DialogContent hideCloseButton className="w-72 max-w-[90vw] p-4">
          <DialogTitle className="text-sm font-medium mb-3">
            Save Profile As
          </DialogTitle>
          <label htmlFor="profile-name" className="sr-only">
            {a11y.profileNameInput()}
          </label>
          <input
            id="profile-name"
            type="text"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleSaveAsConfirm();
            }}
            placeholder="Profile name"
            autoFocus
            className="w-full h-8 px-2 text-sm rounded border bg-[--color-surface-elevated] border-[--color-border] text-[--color-text-primary] placeholder:text-[--color-text-secondary] focus:outline-none focus:ring-1 focus:ring-[--color-accent]"
          />
          <div className="flex justify-end gap-2 mt-3">
            <button
              className="px-3 py-1 text-xs rounded border border-[--color-border] text-[--color-text-secondary] cursor-pointer hover:bg-[--color-surface-elevated] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
              onClick={() => setShowSaveAs(false)}
            >
              Cancel
            </button>
            <button
              className="px-3 py-1 text-xs rounded bg-[--color-accent] text-white cursor-pointer hover:bg-[--color-accent-hover] disabled:opacity-40 disabled:cursor-not-allowed focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
              onClick={handleSaveAsConfirm}
              disabled={!newName.trim()}
            >
              Save
            </button>
          </div>
        </DialogContent>
      </Dialog>

      {/* Delete confirmation dialog */}
      <Dialog open={showDelete} onOpenChange={setShowDelete}>
        <DialogContent
          hideCloseButton
          className="w-72 max-w-[90vw] p-4"
          onOpenAutoFocus={(event) => {
            // Radix's default is to focus the first tabbable descendant
            // (the Cancel button), which makes VoiceOver announce the
            // dialog's title + Cancel button but skip the description
            // (which lives on aria-describedby of the content root, not
            // of its children). Focus the content root instead so VO
            // reads "Delete Profile, dialog, Delete '<name>'? This
            // cannot be undone." before the user tabs to a button.
            event.preventDefault();
            (event.currentTarget as HTMLElement).focus();
          }}
        >
          <DialogTitle className="text-sm font-medium mb-2">
            Delete Profile
          </DialogTitle>
          <DialogDescription className="text-xs mb-3">
            Delete "{activeProfileName}"? This cannot be undone.
          </DialogDescription>
          <div className="flex justify-end gap-2">
            <button
              className="px-3 py-1 text-xs rounded border border-[--color-border] text-[--color-text-secondary] cursor-pointer hover:bg-[--color-surface-elevated] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
              onClick={() => setShowDelete(false)}
            >
              Cancel
            </button>
            <button
              className="px-3 py-1 text-xs rounded bg-[--color-meter-red] text-white cursor-pointer hover:opacity-80 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
              onClick={handleDelete}
            >
              Delete
            </button>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
