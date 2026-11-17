# Architektur – Dream-LLMChatBox

English first, Deutsch below.

---

## English

### Overview
Rust desktop app with a Slint 1.18 UI. It talks to a local Ollama server over HTTP (`ureq`). Data is stored per profile in SQLite and plain files. `main.rs` only declares modules and calls each module's `setup()`.

```
Slint UI (ui/*.slint)  ⇄  setup()/callbacks in src/*_ui.rs, chat.rs …
                               │
          ┌────────────────────┼─────────────────────┐
     ollama.rs            tools.rs (agent)        storage.rs (SQLite)
   /api/chat, /api/ps,   built-in + custom +      chats, messages,
   /api/generate          MCP tools, approval      activity, pins/order
```

### Data folders
| What | Where |
|---|---|
| Settings, tools, MCP | `~/.config/dream-llmchatbox/` (`settings.json`, `tools/`, `mcp.json`) |
| Profiles | `~/.local/share/dream-llmchatbox/profiles/<id>/` (`chats.db`, `groups/<group>/memory/*.md`, `start_args.json`, `avatar.png`) |
| Work folders | `<work folder>/<group>/<chat>/`, default `~/DreamWork` |

### Modules
| Area | Files | Job |
|---|---|---|
| Chat flow | `chat.rs` | Send, stream, agent loop (max. 10 tool steps, loop guard `repeats`, `pause_and_wait` + `schedule_continue`, command timer `start_ticker`), context limit, context bar, work folder prompt |
| Context saver | `context_saver.rs` | Per answer: skip re-reading unchanged files (`Reads`), shorten old tool results over ~55 % context (`shorten_old`), strip `<think>` from history |
| Model choice | `auto_switch.rs` | BETA: smallest model rates the task (`/api/generate`, keep_alive 30m), `pick()` chooses an installed model, keeps a loaded one if it fits |
| Ollama | `ollama.rs`, `server.rs`, `usage.rs` | HTTP API, start/stop server, VRAM/RAM display |
| Hardware | `hardware.rs` | Active GPU only (NVIDIA via `nvidia-smi`, AMD via sysfs `gpu_busy_percent` + `amdgpu.ids` → `glxinfo` → `lspci`), RAM, fit estimate |
| Tools | `tools.rs`, `file_tools.rs` (list/read/search/write/edit), `web_tools.rs`, `memory_tools.rs`, `custom_tools.rs`, `mcp.rs`, `shell.rs`, `commands.rs`, `approval.rs` | Tool definitions, sandboxed folders, approval bar, safe/blocked commands |
| Questions | `question.rs`, `ui/question-bar.slint` | Tool `ask_user`: max. 4 options + own answer, waits in the agent loop like the approval bar |
| Sidebar | `chat_list.rs`, `ui/chat-list.slint` (global `ChatList`), `ui/sidebar.slint` | Pin, manual order (`sort_key`), assign group, multi-select, memory merge, search, Markdown export, auto titles |
| Groups | `groups.rs`, `groups_ui.rs`, `ui/groups-page.slint` | Group settings, memory folder, export/import |
| Work folder UI | `work_ui.rs`, `ui/work-dialog.slint` | Changes chip, diff dialog (`similar`), apply to project, "🧪 Test environment" dialog (`TestEnvDialog`) |
| Work folders | `workspace.rs` | Base folder, per group/chat folders, project copy (skips `.git`, `target`, …) |
| Profiles | `profiles.rs`, `profiles_ui.rs`, `profile.rs` | Profile list, switching, settings dialog; old password profiles are decrypted once |
| Coding mode | `coding.rs`, `coding_ui.rs` | ARCHITEKTUR.md / ROADMAP.md workflow; `architecture_hint` for all other chats with a folder |
| Start arguments | `start_args.rs`, `start_args_page.rs` | Standard and own rules for every chat (incl. "ask when unknown", "no code comments" off by default); `for_group` adds the args a group switched on (`Group::extra_start_args`) and drops the ones it removed (`removed_start_args`); `title` gives short titles (`ui/arg-label.slint` with "i") |
| Documents (RAG) | `rag.rs`, `rag_ui.rs` | `Group::docs_folder`; text from pdftotext / unzip (docx, odt) / plain files, ~1200-char passages, vectors from Ollama `/api/embed`, index as JSON in `<profile>/rag/<hash>.json` (only changed files are embedded again); `search_documents` = cosine similarity, top 6 |
| OpenAI servers | `openai_compat.rs`, `servers_ui.rs`, `ui/server-models.slint` (global `ServerModels`) | `settings.servers`; `/models` for the list, `/chat/completions` with SSE streaming, `reasoning_content` as thinking, tool calls rebuilt from deltas; messages converted with `to_openai` (tool call ids) |
| Projects | `projects.rs` | Global memory: group folders + `<profile>/global.md`; tools `list_projects` / `request_project_access` (asked with the question bar, granted folders are added to the chat); `architecture_index` builds a short index from ARCHITEKTUR.md for the prompt and `architecture-index.md` in the group memory |
| Online CLIs | `cloud_cli.rs`, `cloud_ui.rs`, `ui/cloud-models.slint` (global `CloudModels`) | Providers claude/codex/gemini/grok: find binary (PATH, ~/.local/bin, nvm), install/login in a terminal (`terminal.rs`), run one answer per message (`chat` returns a `ChatOutcome` like Ollama); active ones are `settings.cloud_active` and appear as "☁ Name" in the model list |
| Languages | `i18n.rs`, `lang/de/LC_MESSAGES/dream-llmchatbox.po`, `build.rs` | Slint `@tr` via bundled translations without context; Rust texts via `tr()` / `trf()` reading the same .po (`include_str!`); `settings.ui_language`; `i18n::refresh` re-sets lists filled from Rust after a switch; texts for the model stay English |
| Tour | `tutorial.rs`, `ui/hint.slint` (global `Tutorial`, `Spot`, `TourOverlay`, `Hint`) | `STEPS` table (spot, page, models tab, card position, title, text); `Spot { name }` wraps a target and shows the glowing ⓘ only for the current step; language step first; `welcome_seen` = tour done; restart from Settings |
| Ollama backend | `ollama_backend.rs` | `settings.ollama_backend` (0 auto, 1 Vulkan, 2 ROCm, 3 CPU) → env vars (`OLLAMA_VULKAN`, `HIP/ROCR_VISIBLE_DEVICES`, `GGML_VK_VISIBLE_DEVICES`, `CUDA_VISIBLE_DEVICES`); system service: `pkexec sh -c` writes `/etc/systemd/system/ollama.service.d/dream-llmchatbox-backend.conf`, daemon-reload, restart; own server: `server::start` passes the env; `detect()` reads the last `load_backend` line from `journalctl -u ollama` |
| Release | `scripts/build_appimage.sh`, `scripts/bump_version.py`, `packaging/` | AppImage: `cargo build --release --locked`, AppDir with binary + desktop file + icons, appimagetool with static runtime and zsync update info (`gh-releases-zsync|yakuda-stack|Dream-LLMChatBox|latest|…`); version source of truth = `Cargo.toml`; PKGBUILD builds the GitHub tag with `--frozen`; `cli.rs` answers `--version`/`--help`; `desktop_entry::exec_path` uses `$APPIMAGE` and skips the user entry for `/usr` installs |
| Reset | `reset.rs`, `ui/reset-dialog.slint` | Deletes data dir, config dir and work folder (never home or `/`), then starts the app again |
| Container | `container.rs` | `settings.container_engine` ("" / podman / docker) + image; `commands::run` wraps commands whose folder is inside the work folder: `<engine> run --rm --init -v dir:/work:Z -w /work image sh -c cmd`; note added to the test environment prompt |
| Automations | `automations.rs`, `automations_ui.rs`, `ui/automations-page.slint` (global `Automations`) | `<profile>/automations.json`; `next_after` (daily / Mon–Fri / weekly / every N h) from `max(last_run, created)`; Slint timer every 30 s runs the first due one (one at a time), headless: system message + run note + prompt, no tools, Ollama / server / cloud CLI; result saved as a new chat (`storage::save_chat`), `last_chat` for "Open result", `notify-send`; page 5 from the profile menu |
| Live speech | `live_speech.rs`, `audio.rs` (`Live`) | While recording: every 1.2 s `Live::snapshot_from` + whisper on the open tail; full 20-s pieces are fixed and skipped next time; preview = draft before recording + text; on stop the draft is reset and the whole recording is transcribed as before; `SESSION` counter drops late results |
| Compress chats | `compact.rs` | Marker message with status `compact` and a ```dream-compact block (shown as ThinkBox). `range`: from the last marker to the newest user message before the last 4 messages; summary via the current model (Ollama or OpenAI server, earlier summary included); `chat::history` drops everything up to the newest marker and adds the summary to the system message; `checkpoint::shift` moves later checkpoints by one; automatic after an answer when `context_used >= compact_at` |
| Create documents | `documents.rs` | `create_document`: Markdown → temp file → `pandoc -s -o <path>`; format from the file ending; PDF tries typst → weasyprint → wkhtmltopdf → xelatex → pdflatex, else pandoc → DOCX → `soffice --convert-to pdf`; 2-minute limit; approval + checkpoint like `write_file` |
| Checkpoints | `checkpoint.rs` | Before `write_file`/`edit_file`: copy of the old file in `<profile>/checkpoints/<chat>/<answer>/` + `manifest.json`; `undo` restores in reverse order; only the newest answer with changes can be undone |
| Preview | `preview.rs`, `ui/preview.slint` (global `PreviewState`) | Side panel tab: files from the checkpoints of this chat + picked files; images, PDF (`pdftoppm`), text, HTML as text |
| Answer styles | `answer_style.rs` | Style files in `~/.config/dream-llmchatbox/answer_styles/*.md` (defaults written once); the start argument `answer_style` stores the chosen style in `StartArg::choice` |
| Theme | `theme.rs`, `ui/settings-dialog.slint`, `ui/app-window.slint` | `settings.theme` 0 System / 1 Light / 2 Dark → `theme-index`; app-window sets `Palette.color-scheme` in `changed theme-index` |
| Install script | `install.sh` | Detects the package manager (pacman/dnf/apt/zypper), installs build deps, checks Rust ≥ 1.88 (rustup fallback), clones `main` to `~/.cache/dream-llmchatbox-src`, `cargo build --release --locked`, installs binary/desktop/icons to `/usr/local`; `uninstall` |
| Desktop entry | `desktop_entry.rs`, `assets/icon.svg`/`icon.png` | Sets the xdg app id, writes the .desktop file and icon on start |
| Side panel | `activity.rs`, `browser.rs`, `ui/side-panel.slint` | Activity per chat, browser tab |
| Input & media | `composer.rs`, `attachments.rs`, `audio.rs`, `speech.rs`, `tts.rs`, `readaloud.rs` | Attachments, whisper, Piper |

### Database (`chats`)
`id, title, model, updated_at, group_id, folders, mode, pinned, sort_key, think_mode` (0 auto, 1 on, 2 off; `auto_switch::think_off`). Thinking is streamed as a ```` ```dream-think ```` block and removed before messages go back to the model. List order: `pinned DESC, COALESCE(sort_key, updated_at) DESC`. Moving a chat sets `sort_key` between its new neighbours.

### Rules
- No comments in code, English identifiers and UI strings (`@tr`).
- Every new feature: README line, CHANGELOG entry, ROADMAP checkbox.
- Slint: wrapped text needs an explicit-width wrapper; call callbacks before `close()` in popups.

---

## Deutsch

### Überblick
Rust-Desktop-App mit Slint-1.18-Oberfläche. Sie spricht per HTTP (`ureq`) mit einem lokalen Ollama-Server. Daten liegen pro Profil in SQLite und normalen Dateien. `main.rs` deklariert nur Module und ruft das `setup()` jedes Moduls auf.

### Datenordner
| Was | Wo |
|---|---|
| Einstellungen, Werkzeuge, MCP | `~/.config/dream-llmchatbox/` |
| Profile | `~/.local/share/dream-llmchatbox/profiles/<id>/` (Chats-DB, Gruppen-Gedächtnis, Start-Argumente, Bild) |
| Arbeitsordner | `<Arbeitsordner>/<Gruppe>/<Chat>/`, Standard `~/DreamWork` |

### Module (Kurzfassung)
- **chat.rs**: Senden, Streaming, Agenten-Schleife (max. 10 Werkzeug-Schritte), Kontext-Leiste, Nachfrage für den Arbeitsordner
- **auto_switch.rs** (BETA): kleinstes Modell bewertet die Aufgabe, `pick()` wählt ein installiertes Modell, ein geladenes passendes bleibt (VRAM wird nicht unnötig geleert)
- **context_saver.rs**: keine doppelten `read_file` pro Antwort, alte Werkzeug-Ergebnisse kürzen ab ~55 % Kontext, `<think>` nicht in den Verlauf
- **hardware.rs**: nur die aktive GPU, genauer Name über `amdgpu.ids`, iGPU wird ignoriert
- **chat_list.rs**: Anpinnen, Sortieren (`sort_key`), Gruppe zuweisen, Mehrfachauswahl, Gedächtnis-Merge
- **work_ui.rs**: Änderungen im Arbeitsordner als Diff, „Apply to project“
- **chat_list.rs** auch: Suche, Markdown-Export, automatische Titel
- **workspace.rs**: Arbeitsordner pro Gruppe/Chat mit Projektkopie; wird mit Chat bzw. Gruppe gelöscht
- **question.rs**: Werkzeug `ask_user` – bis zu 4 Antworten zum Klicken + eigene Antwort
- **file_tools.rs**: `list_folder`, `read_file`, `search_files`, `write_file`, `edit_file` (alles in freigegebenen Ordnern)
- **i18n.rs**: Deutsch/Englisch, `tr()` / `trf()`
- **tutorial.rs**: Schritt-für-Schritt-Tour mit Sprachauswahl
- **ollama_backend.rs**: Vulkan/ROCm/CPU für Ollama umschalten
- **cli.rs**: `--version` / `--help`
- **theme.rs**: appearance (System / Light / Dark)
- **reset.rs**: alles zurücksetzen
- **container.rs**: Befehle der Testumgebung optional in Podman/Docker
- **automations.rs / automations_ui.rs**: geplante Aufgaben, Zeitplan, Ausführung ohne Werkzeuge, Ergebnis als neuer Chat
- **live_speech.rs**: Live-Text beim Sprechen (Vorschau im Eingabefeld, am Ende komplette Erkennung)
- **compact.rs**: lange Chats zusammenfassen (Marker-Nachricht, automatisch ab `compact_at` %, Button „🗜 Compress“)
- **documents.rs**: `create_document` – Markdown über pandoc zu PDF/DOCX/ODT/PPTX/HTML/EPUB
- **tools.rs** & Co.: Werkzeuge, Freigaben, Nachfrage-Leiste, MCP
- **profiles*.rs**: Profile ohne Passwort; alte Passwort-Profile werden einmal entsperrt und dann unverschlüsselt gespeichert
- **groups*.rs**, **coding*.rs**, **start_args*.rs**, **activity.rs**, **browser.rs**: Gruppen, Coding-Modus, Start-Argumente, Seitenleiste rechts

### Regeln
- Keine Kommentare im Code, englische Namen und UI-Texte (`@tr`).
- Jedes neue Feature: README-Zeile, CHANGELOG-Eintrag, ROADMAP-Haken.
