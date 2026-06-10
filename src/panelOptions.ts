import type { PolishStyle } from "./types/dictation";

export const shortcutOptions = [
  "Ctrl+Alt+D",
  "Ctrl+Alt+Space",
  "Ctrl+Shift+Space",
  "Ctrl+Win",
  "Ctrl+Win+Space",
];

export const languageOptions = [
  { value: "auto", label: "Auto-detect" },
  { value: "en", label: "English" },
  { value: "es", label: "Spanish" },
  { value: "fr", label: "French" },
  { value: "de", label: "German" },
  { value: "it", label: "Italian" },
  { value: "pt", label: "Portuguese" },
  { value: "nl", label: "Dutch" },
  { value: "pl", label: "Polish" },
  { value: "ja", label: "Japanese" },
  { value: "ko", label: "Korean" },
  { value: "zh", label: "Chinese" },
];

export const polishStyleOptions: Array<{ value: PolishStyle; label: string }> = [
  { value: "concise", label: "Concise" },
  { value: "formal", label: "Formal" },
  { value: "casual", label: "Casual" },
  { value: "excited", label: "Excited" },
  { value: "summarize", label: "Summarize" },
];
