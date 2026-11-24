import { AppWindow } from "lucide-react";
import { useState } from "react";
import type { AppSettings, ApplicationProfile } from "../types/dictation";
import { PanelTitle } from "./PanelTitle";

interface Props {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
}

const styles = [
  { value: "none", label: "Natural" },
  { value: "concise", label: "Concise" },
  { value: "formal", label: "Formal" },
  { value: "casual", label: "Casual" },
  { value: "excited", label: "Energetic" },
] as const;

function applicationKey(value: string) {
  return value.trim().toLowerCase().replace(/\.exe$/, "");
}

function override(value: string) {
  return value === "inherit" ? null : value === "on";
}

export function ApplicationProfiles({ settings, setSettings }: Props) {
  const [application, setApplication] = useState("");
  const profiles = settings.applicationProfiles;
  const name = application.trim();
  const duplicate = profiles.some((profile) => applicationKey(profile.application) === applicationKey(name));
  const invalidName = !name || /[/\\]/.test(name) || name.length > 128;
  const update = (index: number, patch: Partial<ApplicationProfile>) =>
    setSettings({
      ...settings,
      applicationProfiles: profiles.map((profile, position) =>
        position === index ? { ...profile, ...patch } : profile),
    });

  return (
    <div className="settings-section" id="settings-profiles">
      <PanelTitle icon={<AppWindow size={22} />} title="Application profiles & voice editing" />
      <p className="muted">
        Profiles match an application executable, such as notepad.exe or Code.exe.
        Settings are fixed when recording starts. Unset options use your defaults.
      </p>
      <label>
        <span>Voice editing shortcut</span>
        <input
          aria-describedby="voice-edit-help"
          value={settings.voiceEditHotkey}
          maxLength={80}
          onChange={(event) => setSettings({ ...settings, voiceEditHotkey: event.currentTarget.value })}
          placeholder="Leave empty to disable"
        />
      </label>
      <p className="muted" id="voice-edit-help">
        Select text in another app, press {settings.voiceEditHotkey || "your editing shortcut"},
        speak an instruction, then press it again to finish. Use Cancel on the floating
        control to abort. The selection must stay unchanged. Password and read-only fields are excluded.
        Uppercase and lowercase run instantly; other edits use your configured local AI
        runtime. Use Balanced (Qwen2.5 1.5B) for instruction following; prepare it under AI polish if needed. Voice editing does not send selected
        text to an external provider.
      </p>
      {profiles.map((profile, index) => (
        <fieldset className="profile-rule" key={profile.application}>
          <legend>{profile.application}</legend>
          <div className="profile-fields">
            <label>
              <span>Cleanup for {profile.application}</span>
              <select value={profile.cleanupEnabled === null ? "inherit" : profile.cleanupEnabled ? "on" : "off"}
                onChange={(event) => update(index, { cleanupEnabled: override(event.currentTarget.value) })}>
                <option value="inherit">Use default</option>
                <option value="on">On</option>
                <option value="off">Off — keep recognized wording</option>
              </select>
            </label>
            <label>
              <span>AI polish for {profile.application}</span>
              <select value={profile.autoPolish === null ? "inherit" : profile.autoPolish ? "on" : "off"}
                onChange={(event) => update(index, { autoPolish: override(event.currentTarget.value) })}>
                <option value="inherit">Use default</option>
                <option value="off">Off — shortest submit time</option>
                <option value="on">On — adds a rewrite before paste</option>
              </select>
            </label>
            <label>
              <span>Writing style for {profile.application}</span>
              <select value={profile.polishStyle ?? "inherit"}
                onChange={(event) => update(index, {
                  polishStyle: styles.find((style) => style.value === event.currentTarget.value)?.value ?? null,
                })}>
                <option value="inherit">Use default</option>
                {styles.map((style) => <option value={style.value} key={style.value}>{style.label}</option>)}
              </select>
            </label>
          </div>
          <label>
            <span>Additional AI instructions for {profile.application}</span>
            <textarea value={profile.instructions} maxLength={4000} rows={2}
              placeholder="Applied to AI polish and voice editing in this app."
              onChange={(event) => update(index, { instructions: event.currentTarget.value })} />
          </label>
          <button type="button" className="button button--secondary"
            aria-label={"Remove profile for " + profile.application}
            onClick={() => setSettings({ ...settings, applicationProfiles: profiles.filter((_, position) => position !== index) })}>
            Remove profile
          </button>
        </fieldset>
      ))}
      {profiles.length === 0 && <p className="muted">No profiles yet. Every app uses your defaults.</p>}
      <div className="profile-add">
        <label>
          <span>Application executable</span>
          <input value={application} maxLength={128} placeholder="notepad.exe"
            onChange={(event) => setApplication(event.currentTarget.value)} />
        </label>
        <button type="button" className="button button--secondary"
          disabled={invalidName || duplicate || profiles.length >= 64}
          onClick={() => {
            setSettings({ ...settings, applicationProfiles: [...profiles, {
              application: name, cleanupEnabled: null, autoPolish: null, polishStyle: null, instructions: "",
            }] });
            setApplication("");
          }}>
          Add profile
        </button>
      </div>
      {duplicate && <p className="muted">This application already has a profile.</p>}
      <p className="muted">Save changes to apply profiles and the editing shortcut.</p>
    </div>
  );
}
