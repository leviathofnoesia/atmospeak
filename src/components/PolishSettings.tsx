import { Sparkles } from "lucide-react";
import { useEffect, useState } from "react";
import {
  clearPolishApiKey,
  hasPolishApiKey,
  setPolishApiKey,
} from "../lib/api";
import type { AppSettings, ModelInventoryItem } from "../types/dictation";
import { ToggleRow } from "./ToggleRow";

interface PolishSettingsProps {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  polishInventory: ModelInventoryItem[];
  polishSetupBusy: boolean;
  polishSetupMessage: string;
  onEnsurePolishRuntime?: () => Promise<void>;
}

export function PolishSettings({
  settings,
  setSettings,
  polishInventory,
  polishSetupBusy,
  polishSetupMessage,
  onEnsurePolishRuntime,
}: PolishSettingsProps) {
  const polishModelInstalled = polishInventory.some(
    (model) => model.id === settings.polishModel && model.installed,
  );
  const [apiKeyDraft, setApiKeyDraft] = useState("");
  const [hasApiKey, setHasApiKey] = useState(false);
  const [apiKeyBusy, setApiKeyBusy] = useState(false);
  const [apiKeyMessage, setApiKeyMessage] = useState("");

  useEffect(() => {
    void hasPolishApiKey()
      .then(setHasApiKey)
      .catch(() => setHasApiKey(false));
  }, []);

  const onToggleAutoPolish = (autoPolish: boolean) => {
    setSettings({ ...settings, autoPolish });
    if (
      autoPolish &&
      settings.polishProvider === "bundled" &&
      onEnsurePolishRuntime &&
      !polishModelInstalled &&
      !polishSetupBusy
    ) {
      void onEnsurePolishRuntime();
    }
  };

  const onSaveApiKey = async () => {
    if (!apiKeyDraft.trim()) return;
    setApiKeyBusy(true);
    setApiKeyMessage("");
    const endpoint = settings.polishEndpoint.trim();
    try {
      await setPolishApiKey(apiKeyDraft.trim(), endpoint);
      const origin = new URL(endpoint).origin;
      setSettings({ ...settings, polishApiKeyOrigin: origin });
      setApiKeyDraft("");
      setHasApiKey(true);
      setApiKeyMessage("API key saved to the OS keyring and bound to this endpoint.");
    } catch (error: unknown) {
      setApiKeyMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setApiKeyBusy(false);
    }
  };

  const onClearApiKey = async () => {
    setApiKeyBusy(true);
    setApiKeyMessage("");
    try {
      await clearPolishApiKey();
      setSettings({ ...settings, polishApiKeyOrigin: "" });
      setHasApiKey(false);
      setApiKeyDraft("");
      setApiKeyMessage("API key cleared.");
    } catch (error: unknown) {
      setApiKeyMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setApiKeyBusy(false);
    }
  };

  return (
    <>
      <ToggleRow
        icon={<Sparkles size={18} />}
        label="AI auto-edit before paste"
        checked={settings.autoPolish}
        onChange={onToggleAutoPolish}
        disabled={apiKeyBusy}
      />
      {settings.autoPolish ? (
        <fieldset className="settings-polish" disabled={apiKeyBusy}>
          <p className="muted">
            Uses a small local model packaged with Atmospeak — no Ollama required. The first enable
            downloads ~470&nbsp;MB once, then rewrites stay on your machine.
          </p>
          {polishInventory.length > 0 ? (
            <label>
              <span>Edit model</span>
              <select
                value={settings.polishModel}
                onChange={(event) =>
                  setSettings({ ...settings, polishModel: event.currentTarget.value })
                }
              >
                {polishInventory.map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.label}
                    {model.installed ? "" : " (download)"}
                    {model.sizeMb ? ` · ~${model.sizeMb} MB` : ""}
                  </option>
                ))}
              </select>
            </label>
          ) : null}
          <label>
            <span>Style</span>
            <select
              value={settings.polishStyle}
              onChange={(event) =>
                setSettings({
                  ...settings,
                  polishStyle: event.currentTarget.value as AppSettings["polishStyle"],
                })
              }
            >
              <option value="none">None (cleanup only)</option>
              <option value="concise">Concise</option>
              <option value="formal">Formal</option>
              <option value="casual">Casual</option>
              <option value="excited">Excited</option>
            </select>
          </label>
          {onEnsurePolishRuntime ? (
            <button
              type="button"
              className="button button--ghost"
              disabled={polishSetupBusy || apiKeyBusy}
              onClick={() => void onEnsurePolishRuntime()}
            >
              {polishSetupBusy
                ? "Setting up…"
                : polishModelInstalled
                  ? "Warm local editor"
                  : "Download & set up local editor"}
            </button>
          ) : null}
          {polishSetupMessage ? <p className="muted">{polishSetupMessage}</p> : null}

          <details className="settings-polish__advanced">
            <summary>Advanced provider settings</summary>
            <label>
              <span>Provider</span>
              <select
                value={settings.polishProvider}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    polishProvider: event.currentTarget.value as AppSettings["polishProvider"],
                  })
                }
              >
                <option value="bundled">Bundled local (recommended)</option>
                <option value="ollama">Ollama</option>
                <option value="openaiCompatible">OpenAI-compatible</option>
              </select>
            </label>
            {settings.polishProvider !== "bundled" ? (
              <>
                <label>
                  <span>Endpoint</span>
                  <input
                    value={settings.polishEndpoint}
                    onChange={(event) =>
                      setSettings({ ...settings, polishEndpoint: event.currentTarget.value })
                    }
                    placeholder={
                      settings.polishProvider === "ollama"
                        ? "http://127.0.0.1:11434/v1/chat/completions"
                        : "https://api.openai.com/v1/chat/completions"
                    }
                  />
                </label>
                <label>
                  <span>Model id</span>
                  <input
                    value={settings.polishModel}
                    onChange={(event) =>
                      setSettings({ ...settings, polishModel: event.currentTarget.value })
                    }
                    placeholder="llama3.2"
                  />
                </label>
              </>
            ) : null}
            <label>
              <span>Custom instructions</span>
              <textarea
                value={settings.customInstructions}
                onChange={(event) =>
                  setSettings({ ...settings, customInstructions: event.currentTarget.value })
                }
                rows={3}
                placeholder="Optional guidance for the polish model"
              />
            </label>
            {settings.polishProvider === "openaiCompatible" ? (
              <label>
                <span>API key (OS keyring)</span>
                <input
                  type="password"
                  value={apiKeyDraft}
                  onChange={(event) => setApiKeyDraft(event.currentTarget.value)}
                  placeholder={hasApiKey ? "Key saved — enter a new one to replace" : "sk-…"}
                  autoComplete="off"
                />
                <div className="settings-polish__key-actions">
                  <button
                    type="button"
                    className="button button--ghost"
                    disabled={apiKeyBusy || !apiKeyDraft.trim()}
                    onClick={() => void onSaveApiKey()}
                  >
                    Save key
                  </button>
                  <button
                    type="button"
                    className="button button--ghost"
                    disabled={apiKeyBusy || !hasApiKey}
                    onClick={() => void onClearApiKey()}
                  >
                    Clear key
                  </button>
                </div>
              </label>
            ) : null}
            {apiKeyMessage ? <p className="muted">{apiKeyMessage}</p> : null}
            <p className="muted">
              Remote keys use the OS keyring (or <code>ATMOSPEAK_POLISH_API_KEY</code> /{" "}
              <code>OPENAI_API_KEY</code>). Auto-edit times out at 750&nbsp;ms and falls back to
              cleaned text.
            </p>
          </details>
        </fieldset>
      ) : null}
    </>
  );
}
