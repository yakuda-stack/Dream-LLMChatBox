# Roadmap – Dream-LLMChatBox

English first, Deutsch below.

---

## English

### Done
- [x] `num_ctx` is always sent (default 65536, adjustable in Ollama settings)
- [x] Stop near the context limit with "Continue"
- [x] Folder attachments skip `.git`, `target`, `node_modules`, `venv`, `dist`, `build`

### 1. Tool calling (foundation)
- [x] Send tools to Ollama (`tools` in `/api/chat`) and handle `tool_calls` in the answer
- [x] Only enable for models with the "tools" capability (`/api/show`)
- [x] Show each tool call in the chat (what the model wants to do)
- [x] Agent loop: tool result goes back to the model, with a maximum number of steps

### 2. Folder access & sandbox
- [x] Whitelist of allowed folders (per chat or per profile)
- [x] Tools: read file, list folder, write file – only inside allowed folders
- [x] Block leaving the folder (`../`), symlinks out of the folder and system paths
- [x] Compact tool output: filter `.git`, `target/`, `venv/`, binary files; cut long output

### 3. Terminal commands with approval
- [x] Tool `run_terminal_command(command)` with a fixed working directory
- [x] Approval prompt in the UI: "Yes, once" / "Allow for this chat" / "No"
- [x] Session whitelist for safe commands (e.g. `cargo check`, `cargo test`, `pytest`)
- [x] Hard blacklist before the prompt (e.g. `rm -rf /`, `sudo`, leaving the project folder)
- [x] Capture `stdout`/`stderr`, shorten to the relevant error lines, send back to the model

### 4. Web access with approval
- [x] Web search (e.g. DuckDuckGo) as a tool
- [x] Open URLs only after asking ("May I open this page?")
- [x] Convert web pages to clean text/Markdown instead of raw HTML

### 5. Groups & memory
- [x] Groups, e.g. one per project; "New chat" asks for a group, with "Create group"
- [x] Sidebar: chats sorted under their group, groups can be collapsed
- [x] Per group: own start arguments and an optional standard folder that is shared in every new chat
- [x] Memory per group as Markdown files in `~/.local/share/dream-llmchatbox/groups/<group>/memory/` (readable and editable by hand, also with Obsidian), "Open folder" button
- [x] A short `overview.md` always goes to the model; the rest only via tools (search, read, write memory) to save context
- [x] Every memory change is shown as a tool box in the chat
- [x] Right-click on a group: rename, delete, export/import as one file (memory + settings, chats optional); imported tools and commands start switched off
- [x] Move chat data from `~/.config` to `~/.local/share` (config only for settings)
- [x] Per group: switch the global start arguments on/off for this group
- [x] Per group: switch the memory ("brain") on/off
- [x] Groups page: all groups as boxes, click one to change its settings

### 6. Activity per chat
- [x] The activity list (what the model did) is saved per chat; opening a chat shows only its steps

### 7. Profiles
- [x] Several profiles; every profile has its own chats, groups, memory and start arguments (settings, tools and voices stay shared)
- [x] Profile name, info about the user ("i") and a profile picture
- [x] Switch profiles at the bottom left
- [x] ~~Optional password~~ removed again to keep profiles simple

### 8. Coding mode
- [x] A new chat asks what it is for: "Other" or "Coding"; groups can remember it
- [x] Coding: "I have an ARCHITEKTUR.md" / "No ARCHITEKTUR.md yet" / "Brand new project"
- [x] With ARCHITEKTUR.md (or ARCHITECTURE.md): the app gives it to the model first, then the model works on the task (saves tokens)
- [x] Without: the model looks at the folder, writes ARCHITEKTUR.md from a fixed template (after your OK) and saves a summary in the group memory
- [x] Brand new project: the model asks what you want to build and writes ARCHITEKTUR.md
- [x] The model asks if it should create a ROADMAP.md with your goals as a checklist, reads it in later chats and ticks off finished items

### 9. Plugins & MCP
- [x] Plugin interface for own scripts (Bash/Python) as tools
- [x] Support standard MCP servers (e.g. `@modelcontextprotocol/server-filesystem`)
- [x] Manage plugins/MCP servers in the settings

### 10. Sidebar, models & work folders
- [x] Pin chats, sort them by hand (drag handle, move up/down), assign to a group by right-click
- [x] Multi-select chats/groups and merge their memories into a new group
- [x] Auto-Switch AI Model (BETA): router model + keep_alive-friendly choice
- [x] Context bar below the input
- [x] Work folder per group/chat with a project copy for read-only projects
- [x] Exact GPU name, only the active GPU
- [x] Model asks before guessing (web search or your answer)
- [x] Context saver: no double file reads, old tool results shortened, thinking not sent back
- [x] Coding tools `search_files` and `edit_file`
- [x] `ask_user`: questions with up to 4 answers to click + own answer
- [x] Group: remove global start arguments per group, test environment for every chat
- [x] Document RAG per group
- [x] Create documents (PDF, DOCX, slides)
- [x] Compress long chats (automatic + button)
- [x] Realtime speech input (live text)
- [x] Automations (scheduled tasks)
- [x] OpenAI-compatible servers (LM Studio, llama.cpp, …)
- [x] Global memory: projects + access on request
- [x] Architecture index in prompt and group memory
- [x] Online tab: Claude Code / Codex / Gemini / Grok CLIs
- [x] Lists with hanging indent
- [x] Checkpoints + Undo per answer
- [x] Preview tab (no web engine)
- [x] Start argument "Answer like an online AI" with style files
- [x] Start arguments with short title + "i"
- [x] App icon + desktop entry (Wayland)
- [x] Group: switch on globally-off start arguments per group; standard "no comments in code" (off)
- [x] Long commands (15 min, timer), `pause_and_wait` with auto-continue, loop guard
- [x] ARCHITEKTUR.md first in every chat with a folder, kept up to date
- [x] Thinking switch per chat (Auto / On / Off) + collapsible thinking
- [x] Work folder diff + "Apply to project"
- [x] "+" → Test environment (asks first, shows the path)
- [x] Chat search, Markdown export, automatic titles
- [x] Optional: run tests in a Docker/Podman container instead of a plain work folder
- [x] Guided tour + German/English
- [x] Reset everything
- [x] Ollama graphics backend switch
- [x] AppImage + AUR package (first release v0.1.0)
- [x] Ollama install button
- [x] Install script (`curl … | bash`)
- [x] Dark / Light / System mode

### Notes
- Small local models (8B and below) make more mistakes in multi-step tool use. The approval prompts are a safety net for that too.

---

## Deutsch

### Erledigt
- [x] `num_ctx` wird immer mitgeschickt (Standard 65536, einstellbar in den Ollama-Settings)
- [x] Stopp kurz vor dem Kontext-Limit mit „Weiter“
- [x] Ordner-Anhänge überspringen `.git`, `target`, `node_modules`, `venv`, `dist`, `build`

### 1. Tool-Calling (Grundlage)
- [x] Werkzeuge an Ollama schicken (`tools` in `/api/chat`) und `tool_calls` in der Antwort verarbeiten
- [x] Nur für Modelle mit „tools“-Fähigkeit aktivieren (`/api/show`)
- [x] Jeden Werkzeug-Aufruf im Chat anzeigen (was das Modell tun will)
- [x] Agenten-Schleife: Ergebnis geht zurück ans Modell, mit maximaler Schrittzahl

### 2. Ordner-Freigaben & Sandbox
- [x] Whitelist erlaubter Ordner (pro Chat oder pro Profil)
- [x] Werkzeuge: Datei lesen, Ordner auflisten, Datei schreiben – nur in erlaubten Ordnern
- [x] Verlassen des Ordners (`../`), Symlinks nach außen und Systempfade blockieren
- [x] Kompakte Ausgaben: `.git`, `target/`, `venv/`, Binärdateien filtern; lange Ausgaben kürzen

### 3. Terminal-Befehle mit Nachfrage
- [x] Werkzeug `run_terminal_command(command)` mit festem Arbeitsordner
- [x] Nachfrage in der Oberfläche: „Ja, einmal“ / „Für diesen Chat erlauben“ / „Nein“
- [x] Sitzungs-Whitelist für sichere Befehle (z. B. `cargo check`, `cargo test`, `pytest`)
- [x] Harte Blacklist vor der Nachfrage (z. B. `rm -rf /`, `sudo`, Projektordner verlassen)
- [x] `stdout`/`stderr` abfangen, auf die wichtigen Fehlerzeilen kürzen, ans Modell zurückgeben

### 4. Web-Zugriff mit Nachfrage
- [x] Websuche (z. B. DuckDuckGo) als Werkzeug
- [x] URLs nur nach Nachfrage öffnen („Darf ich diese Seite öffnen?“)
- [x] Webseiten als sauberer Text/Markdown statt rohes HTML

### 5. Gruppen & Gedächtnis
- [x] Gruppen, z. B. eine pro Projekt; „New chat“ fragt nach der Gruppe, mit „Gruppe erstellen“
- [x] Sidebar: Chats unter ihrer Gruppe einsortiert, Gruppen einklappbar
- [x] Pro Gruppe: eigene Start-Argumente und ein optionaler Standard-Ordner, der in jedem neuen Chat freigegeben ist
- [x] Gedächtnis pro Gruppe als Markdown-Dateien in `~/.local/share/dream-llmchatbox/groups/<gruppe>/memory/` (von Hand les- und bearbeitbar, auch mit Obsidian), Knopf „Ordner öffnen“
- [x] Eine kurze `overview.md` geht immer ans Modell, der Rest nur über Werkzeuge (Gedächtnis suchen, lesen, schreiben), damit Kontext gespart wird
- [x] Jede Änderung am Gedächtnis erscheint als Tool-Box im Chat
- [x] Rechtsklick auf eine Gruppe: umbenennen, löschen, als eine Datei exportieren/importieren (Gedächtnis + Einstellungen, Chats optional); importierte Tools und Befehle sind erst ausgeschaltet
- [x] Chat-Daten von `~/.config` nach `~/.local/share` umziehen (config nur für Einstellungen)
- [x] Pro Gruppe: die globalen Start-Argumente für diese Gruppe an/aus
- [x] Pro Gruppe: das Gedächtnis („Gehirn“) an/aus
- [x] Gruppen-Seite: alle Gruppen als Kästchen, anklicken und Einstellungen ändern

### 6. Aktivität pro Chat
- [x] Die Aktivitätsliste (was das Modell gemacht hat) wird pro Chat gespeichert; ein geöffneter Chat zeigt nur seine Schritte

### 7. Profile
- [x] Mehrere Profile; jedes Profil hat eigene Chats, Gruppen, Gedächtnis und Start-Argumente (Einstellungen, Werkzeuge und Stimmen bleiben gemeinsam)
- [x] Profilname, Info über den Nutzer („i“) und ein Profilbild
- [x] Profil wechseln unten links
- [x] ~~Optionales Passwort~~ wieder entfernt, damit Profile einfach bleiben

### 8. Coding-Modus
- [x] Ein neuer Chat fragt, wofür er ist: „Anderes“ oder „Programmieren“; Gruppen können sich das merken
- [x] Programmieren: „Ich habe eine ARCHITEKTUR.md“ / „Noch keine ARCHITEKTUR.md“ / „Ganz neues Projekt“
- [x] Mit ARCHITEKTUR.md (oder ARCHITECTURE.md): die App gibt sie dem Modell als Erstes, dann arbeitet es die Aufgabe ab (spart Tokens)
- [x] Ohne: das Modell schaut sich den Ordner an, schreibt ARCHITEKTUR.md nach fester Vorlage (nach deinem OK) und legt eine Kurzfassung im Gruppen-Gedächtnis ab
- [x] Ganz neues Projekt: das Modell fragt, was du bauen willst, und schreibt ARCHITEKTUR.md
- [x] Das Modell fragt, ob es eine ROADMAP.md mit deinen Zielen als Checkliste anlegen soll, liest sie in späteren Chats und hakt Erledigtes ab

### 9. Plugins & MCP
- [x] Plugin-Schnittstelle für eigene Skripte (Bash/Python) als Werkzeuge
- [x] Standard-MCP-Server unterstützen (z. B. `@modelcontextprotocol/server-filesystem`)
- [x] Plugins/MCP-Server in den Einstellungen verwalten

### 10. Seitenleiste, Modelle & Arbeitsordner
- [x] Chats anpinnen, von Hand sortieren (Griff, hoch/runter), per Rechtsklick einer Gruppe zuweisen
- [x] Mehrere Chats/Gruppen auswählen und ihre Gedächtnisse in eine neue Gruppe zusammenführen
- [x] Auto-Switch AI Model (BETA): Router-Modell + keep_alive-schonende Auswahl
- [x] Kontext-Leiste unter der Eingabe
- [x] Arbeitsordner pro Gruppe/Chat mit Projektkopie für schreibgeschützte Projekte
- [x] Genauer GPU-Name, nur die aktive GPU
- [x] Modell fragt nach, statt zu raten (Websuche oder deine Antwort)
- [x] Kontext sparen: keine doppelten Datei-Lesungen, alte Werkzeug-Ergebnisse gekürzt, Thinking wird nicht zurückgeschickt
- [x] Coding-Werkzeuge `search_files` und `edit_file`
- [x] `ask_user`: Fragen mit bis zu 4 Antworten zum Klicken + eigene Antwort
- [x] Gruppe: globale Start-Argumente pro Gruppe abschalten, Testumgebung für jeden Chat
- [x] Dokument-RAG pro Gruppe
- [x] Dokumente erzeugen (PDF, DOCX, Folien)
- [x] Lange Chats komprimieren (automatisch + Button)
- [x] Spracheingabe in Echtzeit (Live-Text)
- [x] Automationen (geplante Aufgaben)
- [x] OpenAI-kompatible Server (LM Studio, llama.cpp, …)
- [x] Globales Gedächtnis: Projekte + Zugriff auf Nachfrage
- [x] Architektur-Index im Prompt und im Gruppen-Gedächtnis
- [x] Online-Reiter: Claude Code / Codex / Gemini / Grok über ihre CLIs
- [x] Listen mit echter Einrückung
- [x] Checkpoints + Rückgängig pro Antwort
- [x] Vorschau-Tab (ohne Web-Engine)
- [x] Start-Argument „Antworte wie eine Online-KI“ mit Stil-Dateien
- [x] Start-Argumente mit Kurztitel + „i“
- [x] App-Icon + Desktop-Eintrag (Wayland)
- [x] Gruppe: global ausgeschaltete Start-Argumente nur für diese Gruppe einschalten; Standard „keine Kommentare im Code“ (aus)
- [x] Lange Befehle (15 Min., Timer), `pause_and_wait` mit automatischem Weiter, Schutz gegen Wiederholungen
- [x] ARCHITEKTUR.md zuerst lesen in jedem Chat mit Ordner, wird aktuell gehalten
- [x] Thinking-Schalter pro Chat (Auto / An / Aus) + aufklappbares Thinking
- [x] Arbeitsordner-Diff + „Ins Projekt übernehmen“
- [x] „+“ → Testumgebung (fragt vorher, zeigt den Pfad)
- [x] Chat-Suche, Markdown-Export, automatische Titel
- [x] Optional: Tests in einem Docker/Podman-Container statt im einfachen Arbeitsordner
- [x] Geführte Tour + Deutsch/Englisch
- [x] Alles zurücksetzen
- [x] Grafik-Backend für Ollama umschalten
- [x] AppImage + AUR-Paket (erstes Release v0.1.0)
- [x] Ollama-Installieren-Button
- [x] Installationsskript (`curl … | bash`)
- [x] Dunkel / Hell / System-Modus

### Hinweise
- Kleine lokale Modelle (8B und kleiner) machen bei mehrstufigen Werkzeug-Abläufen mehr Fehler. Die Nachfragen sind dafür auch ein Sicherheitsnetz.
