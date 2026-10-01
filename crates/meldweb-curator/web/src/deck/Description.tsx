import { useState } from "react";
import Markdown from "react-markdown";

/**
 * The deck's Markdown description, read until Edit opens it as text. A save is
 * one edit, not one per keystroke, so undo steps back over a rewrite whole.
 */
export function Description({
  text,
  onSave,
}: {
  text: string | undefined;
  onSave: (text: string) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);

  if (draft !== null) {
    const save = () => {
      onSave(draft);
      setDraft(null);
    };
    return (
      <form
        className="description editing"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <textarea
          aria-label="Description"
          value={draft}
          rows={Math.max(6, draft.split("\n").length + 1)}
          placeholder="What the deck is for, how it wins. Markdown."
          // biome-ignore lint/a11y/noAutofocus: Edit was just pressed to type here.
          autoFocus
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") setDraft(null);
            else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              save();
            }
          }}
        />
        <div className="description-actions">
          <button type="button" onClick={() => setDraft(null)}>
            Cancel
          </button>
          <button type="submit" className="primary">
            Save
          </button>
        </div>
      </form>
    );
  }

  if (!text) {
    return (
      <button
        type="button"
        className="ghost small description-add"
        onClick={() => setDraft("")}
      >
        Add a description
      </button>
    );
  }

  return (
    <section className="description">
      <div className="description-body">
        <Markdown>{text}</Markdown>
      </div>
      <button
        type="button"
        className="ghost small"
        onClick={() => setDraft(text)}
      >
        Edit
      </button>
    </section>
  );
}
