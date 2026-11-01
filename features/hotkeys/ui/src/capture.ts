import type { HotkeyChord, HotkeyModifier } from "@qbit/ipc";
type KeyEvent = Pick<
  KeyboardEvent,
  | "key"
  | "code"
  | "location"
  | "ctrlKey"
  | "shiftKey"
  | "altKey"
  | "metaKey"
  | "repeat"
  | "isComposing"
  | "getModifierState"
>;
export type CaptureResult =
  | { kind: "chord"; chord: HotkeyChord }
  | { kind: "cancel" | "wait" | "unsupported" };
const special = new Set([
  "Enter",
  "Tab",
  "Backspace",
  "Delete",
  "Insert",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "CapsLock",
  "NumLock",
  "ScrollLock",
  "Pause",
  "PrintScreen",
  "ContextMenu",
]);
const punctuation: Record<string, [string, string]> = {
  Minus: ["-", "_"],
  Equal: ["=", "+"],
  BracketLeft: ["[", "{"],
  BracketRight: ["]", "}"],
  Backslash: ["\\", "|"],
  Semicolon: [";", ":"],
  Quote: ["'", '"'],
  Comma: [",", "<"],
  Period: [".", ">"],
  Slash: ["/", "?"],
  Backquote: ["\u0060", "~"],
};
/** Focused UI capture only. Non-Latin/dead/AltGraph and ambiguous punctuation
 * are rejected rather than guessing a Windows logical identity from position. */
export function captureChord(event: KeyEvent): CaptureResult {
  if (event.key === "Escape") return { kind: "cancel" };
  if (event.repeat || ["Control", "Shift", "Alt", "Meta"].includes(event.key))
    return { kind: "wait" };
  if (event.isComposing || event.getModifierState("AltGraph"))
    return { kind: "unsupported" };
  let token: string | undefined;
  if (event.location === 3) {
    if (event.key === "Enter") token = "NumpadEnter";
    else if (/^Numpad[0-9]$/.test(event.code) && /^[0-9]$/.test(event.key))
      token = event.code;
    else if (
      [
        "NumpadAdd",
        "NumpadSubtract",
        "NumpadMultiply",
        "NumpadDivide",
        "NumpadDecimal",
      ].includes(event.code)
    )
      token = event.code;
  } else if (/^[a-z]$/i.test(event.key))
    token = "Key" + event.key.toUpperCase();
  else if (/^[0-9]$/.test(event.key)) token = "Digit" + event.key;
  else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(event.key) || special.has(event.key))
    token = event.key;
  else if (event.key === " ") token = "Space";
  else if (punctuation[event.code]?.includes(event.key)) token = event.code;
  if (!token) return { kind: "unsupported" };
  const modifiers: HotkeyModifier[] = [];
  if (event.ctrlKey) modifiers.push("ctrl");
  if (event.shiftKey) modifiers.push("shift");
  if (event.altKey) modifiers.push("alt");
  if (event.metaKey) modifiers.push("meta");
  return {
    kind: "chord",
    chord: { modifiers, primary: { kind: "logical", token } },
  };
}
export function formatChord(chord: HotkeyChord): string {
  return [
    ...chord.modifiers.map((m) => m[0].toUpperCase() + m.slice(1)),
    chord.primary.token.replace(/^Key|^Digit/, ""),
  ].join(" + ");
}
