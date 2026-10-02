import { useState } from "react";
import { loadModel, MODELS, type Model, saveModel } from "./models";

/** The model picked in this browser, and a setter that keeps the pick. */
export function useModel(): [Model, (model: Model) => void] {
  const [model, setModel] = useState(loadModel);
  return [
    model,
    (next) => {
      saveModel(next);
      setModel(next);
    },
  ];
}

/**
 * Which of Delver X's models the scanner runs. Picking one boots it the first
 * time, and it stays booted beside the others for the rest of the page.
 */
export function ModelPicker({
  model,
  onChange,
  disabled,
}: {
  model: Model;
  onChange: (model: Model) => void;
  disabled?: boolean;
}) {
  return (
    <label className="field scan-model">
      Model
      <select
        value={model}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value as Model)}
      >
        {MODELS.map((m) => (
          <option key={m.id} value={m.id}>
            {m.label} ({m.download})
          </option>
        ))}
      </select>
    </label>
  );
}
