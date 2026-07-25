"""Helper for production dogfood runs — seed settings and read session evidence."""
import json
import sqlite3
import subprocess
import sys
import time
from pathlib import Path

APP_DIR = Path.home() / "AppData" / "Local" / "Atmospeak"
DB_PATH = APP_DIR / "wind-speak.sqlite3"
EXE_PATH = Path(__file__).resolve().parents[1] / "src-tauri" / "target" / "debug" / "atmospeak.exe"
ONBOARDING_VERSION = "phase-a-honest-mvp-v1"


def base_settings(mode: str) -> dict:
    return {
        "hotkey": "Ctrl+Win",
        "mode": mode,
        "microphoneName": None,
        "restoreClipboard": True,
        "autoInject": True,
        "cleanupEnabled": True,
        "startAtLogin": False,
        "onboardingComplete": True,
        "onboardingVersion": ONBOARDING_VERSION,
        "advancedRuntimeEnabled": False,
        "advancedModelPath": "",
        "advancedWhisperCliPath": "",
    }


def seed_settings(mode: str) -> None:
    payload = json.dumps(base_settings(mode))
    conn = sqlite3.connect(DB_PATH)
    conn.execute(
        "insert into settings (key, value) values ('app', ?) "
        "on conflict(key) do update set value = excluded.value",
        (payload,),
    )
    conn.commit()
    conn.close()
    print(f"seeded settings mode={mode}")


def latest_session() -> dict | None:
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    row = conn.execute(
        "select id, raw_text, cleaned_text, audio_path, duration_ms, injected, created_at "
        "from transcript_sessions order by created_at desc limit 1"
    ).fetchone()
    conn.close()
    return dict(row) if row else None


def stop_app() -> None:
    subprocess.run(
        ["powershell", "-NoProfile", "-Command", "Get-Process atmospeak -ErrorAction SilentlyContinue | Stop-Process -Force"],
        check=False,
    )
    time.sleep(1)


def start_app() -> None:
    if not EXE_PATH.exists():
        raise FileNotFoundError(EXE_PATH)
    subprocess.Popen([str(EXE_PATH)], cwd=EXE_PATH.parent)
    time.sleep(4)


def main() -> None:
    cmd = sys.argv[1] if len(sys.argv) > 1 else "latest"
    if cmd == "seed-toggle":
        stop_app()
        seed_settings("toggle")
        start_app()
    elif cmd == "seed-ptt":
        stop_app()
        seed_settings("pushToTalk")
        start_app()
    elif cmd == "latest":
        session = latest_session()
        print(json.dumps(session, indent=2) if session else "no sessions")
    else:
        raise SystemExit(f"unknown command: {cmd}")


if __name__ == "__main__":
    main()
