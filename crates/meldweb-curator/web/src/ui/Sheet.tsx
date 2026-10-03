import { type ReactNode, useEffect, useRef, useState } from "react";

export function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** A `<dialog>` that is open while mounted, and unmounts on Escape or Cancel. */
export function Sheet({
  label,
  onClose,
  children,
  wide,
}: {
  label: string;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    ref.current?.showModal();
  }, []);
  return (
    <dialog
      ref={ref}
      className={wide ? "sheet cover-sheet" : "sheet"}
      aria-label={label}
      onClose={onClose}
    >
      {children}
    </dialog>
  );
}

/** Runs `work`, holding the dialog busy, and shows what it threw. */
export function useBusy() {
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const run = async (work: () => Promise<string | null>) => {
    setBusy(true);
    setRefusal(null);
    try {
      const refused = await work();
      setRefusal(refused);
      return refused === null;
    } catch (e) {
      setRefusal(messageOf(e));
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { busy, refusal, run };
}
